// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D4's branch, driven by a caller outside this crate.
//!
//! # No socket, and that was a ruling rather than a convenience
//!
//! The first form of this file bound `127.0.0.1:0` and served Gemini-shaped
//! JSON, so that the loop could be reached from the built binary on a runner
//! with no key. **It was refused on 2026-09-05**, by the ruling already given
//! to the `composer-wiring` arc on the same proposal: a listener answering
//! canned provider JSON is a fake of Google at the wire, which is the mock
//! [Testing] refuses, and `provider-client`'s recorded-exchange fixture
//! already covers the mapping from those bytes to a [`ModelResponse`].
//!
//! So the loop is driven **here**, at the library level, over a staged
//! [`Model`]. [`Generating`](zaru_cli::compose::Generating) is generic over
//! that port, so a staged model reaching `compose`'s branch needs no socket
//! and no credential — and everything between the candidate and the validator
//! is the product's own: the real [`Executor`] over the real working-directory
//! boundary and permission decision, the real [`Dispatch`] over a real
//! [`Plan`], validators run as **real child processes** through
//! [`Spawn`], and the real transcript.
//!
//! **What is left to the artefact, and it is one thing.** Whether a real model
//! produces a candidate at all is a question about a model, and no fixture can
//! answer it; that is the arc's run against `gemini-3.6-flash` behind the key
//! discipline, quoted in its report, and the finding it produced is on
//! [ADR-0008]'s amendments page.
//!
//! # What is deliberately not here
//!
//! **A validator killed at the process ceiling.** `cli::layers::PROCESS_CEILING`
//! is a compiled-in two minutes with no configuration key, so a check that
//! reached it would take two minutes — and `process_from_outside.rs` already
//! drives a kill against a ceiling it owns. What is asserted instead is the
//! property that matters here: a validator whose command exits non-zero is
//! exhaustion and never success.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [`Dispatch`]: zaru_core::iteration::validator::Dispatch
//! [`Executor`]: zaru_cli::tools::Executor
//! [`Plan`]: zaru_core::iteration::validator::Plan
//! [`Spawn`]: zaru_cli::process::Spawn

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::compose::{Applying, Generating, Inner, Records, Shared};
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
};
use zaru_cli::process::{Environment, ProcessCeiling, Spawn};
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction};
use zaru_cli::session::{SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::port::Answer;
use zaru_cli::tools::{
    Confirm, ConfirmFailure, Executor, Fetch, Invocation, Mode, NoMembrane, OutputBudget, Question,
    SessionOverflow, WorkingDirectory,
};
use zaru_core::iteration::validator::{Declared, Dispatch, Expect, Name, Pattern, Plan, Run};
use zaru_core::iteration::{
    Ceiling, Clock, ContextPolicy, ContextRefusal, Limits, Outcome as LoopOutcome, PortFailure,
    Prompt, TruncationBudget, Turn,
};
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{
    Capabilities, Model, ModelRequest, ModelResponse, Outcome, Ports, Start, TokenUsage,
    ToolCallCeiling, ToolCalling, ToolRequest, run,
};

// ------------------------------------------------------------------ scratch

/// A project to run in and a session beside it, removed when the check ends.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!(
                "iw-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(base.join("project")).expect("staging: the project");
        std::fs::create_dir_all(base.join("sessions")).expect("staging: the session root");
        Self { base }
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }

    fn sessions(&self) -> std::path::PathBuf {
        self.base.join("sessions")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// -------------------------------------------------------------- staged ports

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// The provider, staged, because no product tree has one that answers offline.
///
/// It records the prompt of every exchange, which is what the redaction check
/// and the refinement check read: what the **model** was given, rather than
/// what the harness says it was given.
struct Provider {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    prompts: Mutex<Vec<String>>,
}

impl Provider {
    fn scripted(responses: impl IntoIterator<Item = ModelResponse>) -> Self {
        Self {
            script: Mutex::new(responses.into_iter().collect()),
            prompts: Mutex::new(Vec::new()),
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().expect("poisoned").clone()
    }

    fn asked(&self) -> usize {
        self.prompts.lock().expect("poisoned").len()
    }
}

impl Model for Provider {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        self.prompts
            .lock()
            .expect("poisoned")
            .push(request.prompt.rendered());
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

/// The context policy, staged, so that what a candidate's generator is handed
/// is exactly what the loop constructed and nothing else.
#[derive(Default)]
struct Policy;

impl ContextPolicy for Policy {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        Ok(Prompt::new(Redacted::by(&HeldSecrets::none(), &rendered)))
    }
}

struct Unbuilt;

impl Fetch for Unbuilt {
    async fn retrieve(
        &self,
        _url: &zaru_cli::web::RequestedUrl,
        _followed: usize,
    ) -> Result<zaru_cli::tools::Retrieved, PortFailure> {
        Err(PortFailure::new("web.fetch is not reached by these checks"))
    }
}

/// Nothing is pre-approved, so every write is a call that needs asking.
struct NothingAllowed;
impl zaru_cli::tools::Allowlist for NothingAllowed {
    fn approves(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}

struct NothingDestructive;
impl zaru_cli::tools::DestructiveMatch for NothingDestructive {
    fn is_destructive(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}

/// A user who declines the first `decline` questions and accepts the rest.
struct Declining {
    decline: usize,
    asked: Mutex<usize>,
}

impl Declining {
    const fn once() -> Self {
        Self {
            decline: 1,
            asked: Mutex::new(0),
        }
    }

    const fn nothing() -> Self {
        Self {
            decline: 0,
            asked: Mutex::new(0),
        }
    }

    fn asked(&self) -> usize {
        *self.asked.lock().expect("poisoned")
    }
}

impl Confirm for Declining {
    fn confirm(&self, _question: &Question) -> Result<Answer, ConfirmFailure> {
        let mut asked = self.asked.lock().expect("poisoned");
        *asked += 1;
        Ok(if *asked > self.decline {
            Answer::Once
        } else {
            Answer::No
        })
    }
}

// ------------------------------------------------------------------ staging

fn usage() -> TokenUsage {
    TokenUsage {
        prompt: 11,
        completion: 7,
    }
}

/// One answer proposing a write, which is the candidate shape a model produces
/// through the provider's own function-calling contract.
fn writes(path: &str, contents: &str) -> ModelResponse {
    writes_all(&[(path, contents)])
}

/// One answer proposing several writes, in order.
fn writes_all(pairs: &[(&str, &str)]) -> ModelResponse {
    ModelResponse::Calls {
        text: String::new(),
        echo: None,
        calls: pairs
            .iter()
            .enumerate()
            .map(|(index, (path, contents))| ToolRequest {
                id: format!("call-{index}"),
                name: "fs.write".to_owned(),
                arguments: serde_json::json!({ "path": path, "contents": contents }).to_string(),
            })
            .collect(),
        tokens: usage(),
    }
}

/// A plan of one validator: run `command`, and pass when its output matches.
fn one_validator(command: &str, wanted: &str) -> Plan {
    Plan::from_declared(vec![Declared {
        name: Name::new("report").expect("a validator name"),
        run: Run::new(command).expect("a command line"),
        expect: Expect::Matches(Pattern::new(wanted).expect("a pattern")),
        after: Vec::new(),
    }])
    .expect("one validator resolves")
}

/// Everything a run of the branch needs, so that a check reads as the case it
/// is about rather than as eleven constructors.
struct Run_<'a> {
    scratch: &'a Scratch,
    plan: &'a Plan,
    provider: &'a Provider,
    ceiling: u32,
    mode: Mode,
    confirmer: Option<&'a (dyn Confirm + Sync)>,
    held: &'a HeldSecrets,
    /// Whether the turn is handed an inner loop, which is ADR-0009 D4's
    /// branch: `true` is a project that declared validators and `false` is
    /// the outer tool-call loop running alone.
    with_inner: bool,
}

/// Drive [ADR-0009] D4's branch to an outcome, and hand back the transcript.
///
/// Every port between the candidate and the validator is the **product's**:
/// `compose`'s `Generating`, `Applying` and `Inner`, `zaru-cli`'s `Executor`
/// over ADR-0011 D4's boundary and D3's permission decision, `zaru-core`'s
/// `Dispatch`, and `Spawn` running each validator as a real child process.
fn drive<P: ContextPolicy + Sync>(staged: &Run_<'_>, policy: &P) -> (Outcome, String) {
    drive_narrating(staged, policy, None)
}

/// The same, with [ADR-0028] D3's subscriber attached.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
fn drive_narrating<P: ContextPolicy + Sync>(
    staged: &Run_<'_>,
    policy: &P,
    narrator: Option<&dyn zaru_cli::compose::Narrator>,
) -> (Outcome, String) {
    let working = WorkingDirectory::at(staged.scratch.project()).expect("the boundary resolves");
    let store = SessionStore::open(staged.scratch.sessions()).expect("the session store opens");
    let id = SessionId::mint(&SystemWallClock).expect("a session id");
    let session = store.start(id).expect("the session starts");
    let transcript_path = session.transcript_path();

    let mut transcript = Transcript::append_to(&transcript_path).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(session.directory());
    let allowlist = NothingAllowed;
    let destructive = NothingDestructive;
    let membrane = NoMembrane;
    let unbuilt = Unbuilt;
    let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
        "PATH",
        "/usr/bin:/bin",
    )]))
    .expect("a child environment");
    let spawn = Spawn::new(
        &working,
        environment,
        ProcessCeiling::new(Duration::from_secs(20)).expect("a usable ceiling"),
    );

    let no_grants = zaru_cli::tools::grants::SessionGrants::none();
    let executor = Executor {
        working_directory: &working,
        mode: staged.mode,
        allowlist: &allowlist,
        destructive: &destructive,
        session_grants: &no_grants,
        confirmer: staged.confirmer,
        verdicts: &membrane,
        budget: OutputBudget::new(4096).expect("a usable budget"),
        preview_budget: OutputBudget::new(4096).expect("a usable budget"),
        search_ceiling: zaru_cli::cli::layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: staged.held,
        subprocess: &spawn,
        fetch: &unbuilt,
        projected: &zaru_cli::tools::NoProjection,
        declared: zaru_cli::tools::descriptor_set(),
    };

    let clock = Ticking::default();
    let patterns = zaru_cli::validators::Patterns::new(zaru_cli::cli::layers::pattern_ceiling());
    let schemas =
        zaru_cli::validators::SchemaFiles::new(&working, zaru_cli::cli::layers::file_ceiling());
    let dispatch = Dispatch::new(staged.plan, &spawn, &patterns, &schemas);

    let outcome = {
        let cell = tokio::sync::Mutex::new(executor);
        let mut tools = Shared::over(&cell, zaru_cli::tools::descriptor_set());
        let generating = Generating::over(staged.provider);
        let applying = Applying::through(tools);
        let inner = Inner::over(
            zaru_core::iteration::Ports {
                generator: &generating,
                executor: &applying,
                validators: &dispatch,
                context: policy,
                clock: &clock,
                redactor: staged.held,
            },
            Limits {
                ceiling: Ceiling::new(staged.ceiling).expect("a usable ceiling"),
                budget: TruncationBudget::new(4096).expect("a usable budget"),
            },
            &transcript_path,
            narrator,
        );
        let witness = ToolCalling::required(staged.provider, "staged").expect("it calls tools");
        let mut sink = Records::appending_to(&transcript_path).expect("a second handle");
        block_on(run(
            1,
            Start::Task("do the work"),
            ToolCallCeiling::new(8).expect("a usable ceiling"),
            witness,
            Ports {
                model: staged.provider,
                tools: &mut tools,
                context: policy,
                clock: &clock,
                redactor: staged.held,
            },
            if staged.with_inner {
                Some(&inner)
            } else {
                None
            },
            &mut [&mut sink],
        ))
        .expect("no port failed")
    };

    let written = std::fs::read_to_string(&transcript_path).expect("the transcript was written");
    (outcome, written)
}

/// Poll a future to completion on a current-thread runtime, as the binary does.
fn block_on<F: core::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime")
        .block_on(future)
}

/// The name of each `Record::Loop` event in the transcript, in order.
fn loop_events(transcript: &str) -> Vec<String> {
    transcript
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|record| record.get("loop").cloned())
        .filter_map(|event| {
            event
                .as_object()
                .and_then(|event| event.keys().next().cloned())
        })
        .collect()
}

// ------------------------------------------------------------------- checks

/// ADR-0009 D4's branch runs, and ADR-0008 D5's exhaustion is reported as
/// itself at ADR-0016 D5's `1`.
///
/// The model writes the **wrong** file every time, so the declared validator
/// never passes and the loop reaches its ceiling. That is D5's outcome, and D5
/// says it "is not an error and is not a success" — so the class it is
/// presented in is [ADR-0016] D1 row 1's expected register, whose
/// `is_the_error_register()` is `false` by construction, at exit `1`.
///
/// **`RefinementConstructed` appears once, not twice.** ADR-0008 D1: the
/// ceiling is checked on the transition out of `Evaluate`, so a run of *n*
/// iterations at a ceiling of *n* emits *n* minus one. That is the assertion
/// that separates this loop from a retry wrapper with a counter.
///
/// Watched red by supplying `None` at the branch, which ran the tool-call loop
/// instead and printed *"a project that declares validators must run the
/// iteration loop"*.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn adr_0009_d4s_branch_runs_the_loop_and_exhaustion_is_reported_as_itself() {
    let scratch = Scratch::new("exhausted");
    let plan = one_validator("cat report.txt", "TOTAL: 3");
    let provider = Provider::scripted([
        writes("not-the-report", "no"),
        writes("not-the-report", "no again"),
    ]);
    let held = HeldSecrets::none();
    let accepting = Declining::nothing();

    let (outcome, transcript) = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 2,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: true,
        },
        &Policy,
    );

    let LoopOutcome::Exhausted {
        iterations,
        reason,
        last_failure,
    } = expect_iterated(&outcome)
    else {
        panic!(
            "a project that declares validators must run the iteration loop to exhaustion, and it \
             produced {outcome:?}"
        );
    };
    assert_eq!(iterations, 2, "a ceiling of two runs two iterations");
    assert_eq!(
        reason,
        zaru_core::iteration::ExhaustionReason::CeilingReached,
        "the run stopped at its ceiling and reported another reason"
    );
    assert!(
        last_failure.is_some(),
        "the ceiling route always carries the failure it stopped on"
    );
    assert_eq!(
        provider.asked(),
        2,
        "one generation per iteration, and the loop made a different number of exchanges"
    );

    let events = loop_events(&transcript);
    assert!(
        events.iter().any(|name| name == "loop_exhausted"),
        "the transcript must carry the loop's own exhaustion: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|name| name.as_str() == "refinement_constructed")
            .count(),
        1,
        "a run of two iterations at a ceiling of two emits ONE refinement, not two: ADR-0008 D1 \
         checks the ceiling on the way out of Evaluate. The events were {events:?}"
    );

    // The class this outcome is presented in, from the mapping the binary uses.
    let classified = zaru_cli::cli::classify::Surface::loop_exhausted(
        iterations,
        reason,
        last_failure.as_deref(),
    );
    let exit = zaru_cli::failure::Exit::Failed(classified);
    assert_eq!(
        exit.code(),
        1,
        "ADR-0016 D5's `1` is \"the work failed (loop exhausted, validator never satisfied)\""
    );
    assert_eq!(
        exit.class()
            .map(zaru_cli::failure::Class::is_the_error_register),
        Some(false),
        "ADR-0008 D5 puts exhaustion in a register of its own, and D1 row 1 keeps it out of the \
         error one"
    );
}

/// The loop refines: a validator that fails once passes after the model's fix.
///
/// This is the product claim and it is the accepting sibling of the check
/// above. The model's **first** candidate writes the wrong file and its second
/// writes the right one, so the validator fails, the failure reaches the
/// refinement, and the next iteration succeeds. Without this arm the
/// exhaustion check is satisfied by a loop that can never succeed at all.
///
/// **The refinement prompt is read off the model rather than off the harness**:
/// the staged provider records what it was handed, so the assertion is about
/// what the model actually saw.
///
/// Watched red by making both answers write the wrong file, which printed
/// *"the loop must succeed once the model writes the file the validator
/// wants"*.
#[test]
fn a_validator_that_fails_once_passes_after_the_models_fix() {
    let scratch = Scratch::new("refined");
    let plan = one_validator("cat report.txt", "TOTAL: 3");
    // Three answers for a ceiling of three, though a correct loop uses two:
    // the third exists so that an implementation which applied NOTHING runs to
    // the ceiling and meets this check's own sentence, rather than dying on a
    // staging that ran out ([Verification lessons] §4 -- a check that fails in
    // its fixture asserts nothing about the product).
    let provider = Provider::scripted([
        writes("not-the-report", "no"),
        writes("report.txt", "TOTAL: 3\n"),
        writes("not-the-report", "nor this"),
    ]);
    let held = HeldSecrets::none();
    let accepting = Declining::nothing();

    let (outcome, transcript) = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 3,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: true,
        },
        &Policy,
    );

    let LoopOutcome::Succeeded { iterations, .. } = expect_iterated(&outcome) else {
        panic!(
            "the loop must succeed once the model writes the file the validator wants, and it \
             produced {outcome:?}"
        );
    };
    assert_eq!(
        iterations, 2,
        "one failing iteration and one passing one is two"
    );
    assert_eq!(
        std::fs::read_to_string(scratch.project().join("report.txt")).expect("the write landed"),
        "TOTAL: 3\n",
        "the succeeding candidate's write must be on disk"
    );

    let events = loop_events(&transcript);
    assert!(
        events.iter().any(|name| name == "refinement_constructed"),
        "a run that failed once must have refined: {events:?}"
    );
    assert!(
        events.iter().any(|name| name == "loop_succeeded"),
        "a run that ended satisfied must say so on the stream: {events:?}"
    );

    // ADR-0008 clause 2, from the model's own side: the second prompt is the
    // refinement, and it carries what the validator printed.
    let prompts = provider.prompts();
    assert_eq!(prompts.len(), 2, "two iterations are two exchanges");
    assert!(
        prompts[1].contains("did not satisfy the declared validators"),
        "the second exchange must be the refinement `zaru-core` constructed: {:?}",
        prompts[1]
    );
    assert!(
        prompts[1].contains("cat: report.txt: No such file or directory"),
        "ADR-0008 D4 carries the validator's own output into the next prompt verbatim, and the \
         model was given {:?}",
        prompts[1]
    );
}

/// ADR-0008 clause 6's port on the refinement path, and ADR-0010 D2's record
/// keeping what it redacts.
///
/// **Two assertions that must both hold, and they point opposite ways.** The
/// validator prints a value the harness holds. The prompt the **model** was
/// given must not carry it; the transcript's own record of the failure must.
/// One of those alone proves nothing: an implementation that redacted
/// everything passes the first, and one that redacted nothing passes the
/// second.
///
/// The validator also prints a nonce beside it, which must survive into the
/// prompt — otherwise "absent" could be satisfied by a prompt carrying nothing
/// at all.
///
/// Watched red by handing the inner loop a redactor holding nothing, which
/// printed *"the harness's own held value reached the refinement prompt"*.
#[test]
fn a_held_secret_in_a_validators_output_is_redacted_in_the_refinement_and_kept_in_the_record() {
    let scratch = Scratch::new("redacted");
    let nonce = "rehearsal-4173";
    let secret = "nn_mcp_held4173iterationwiring";
    std::fs::write(
        scratch.project().join("leaked"),
        format!("{nonce} {secret}\n"),
    )
    .expect("staging: the file the validator prints");

    // `cat` prints and exits 0, and the validator wants a pattern the file does
    // not carry — so it fails while printing, which is what ADR-0009 D5 sends
    // into refinement.
    let plan = one_validator("cat leaked", "NEVER-MATCHES");
    let provider = Provider::scripted([
        writes("not-the-report", "no"),
        writes("not-the-report", "no again"),
    ]);
    let held = held_from_a_store(&scratch, secret);
    let accepting = Declining::nothing();

    let (_outcome, transcript) = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 2,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: true,
        },
        &Policy,
    );

    let prompts = provider.prompts();
    assert_eq!(prompts.len(), 2, "two iterations are two exchanges");
    let refinement = &prompts[1];
    assert!(
        refinement.contains(nonce),
        "ADR-0008 clause 2: the validator's own output reaches the refinement prompt verbatim, \
         and the nonce it printed is not in {refinement:?}"
    );
    assert!(
        !refinement.contains(secret),
        "the harness's own held value reached the refinement prompt: {refinement:?}"
    );

    let kept: String = transcript
        .lines()
        .filter(|line| line.contains("iteration_failed"))
        .collect();
    assert!(
        !kept.is_empty(),
        "the failing iteration must have been recorded, or the assertion below reads nothing"
    );
    assert!(
        kept.contains(secret),
        "ADR-0010 D2 keeps what the session contained, and the transcript's own record of the \
         failure lost the bytes the command printed: {kept}"
    );
}

/// A candidate's write outside the tree is decided exactly as a turn's is.
///
/// **For the security corpus, which only grows.** ADR-0011 D4's boundary
/// belongs to the tool surface, and a candidate reaches the tool surface — so
/// nothing classifies a candidate's path differently from a turn's. Both
/// halves are asserted: the write is refused and the file does not exist, and
/// the transcript marks the call `out_of_tree` in the same field a turn's call
/// is marked with and closes it as `refused`.
///
/// **The staging is the discriminating part and it took two attempts.** A
/// confirmer that declines *everything* cannot separate "the permission model
/// refused this" from "`fs.write` never works here"; the accepting sibling
/// below is the same candidate with a confirmer that says yes, and the write
/// lands.
///
/// Watched red by giving the tool surface a permission mode of its own rather
/// than the caller's — which is what a second executor built for the inner
/// loop would amount to — and it printed *"a candidate wrote outside the
/// working directory"*.
#[test]
fn a_candidates_write_outside_the_tree_is_decided_like_a_turns() {
    let scratch = Scratch::new("out-of-tree");
    let outside = scratch.base.join("outside-the-tree");
    let plan = one_validator("cat report.txt", "TOTAL: 3");
    let held = HeldSecrets::none();

    // --- Declined: refused, and nothing on disk ---------------------------
    //
    // TWO calls, the out-of-tree one first and an in-tree one after it. That
    // is what separates "the candidate stopped at the refusal" from "the
    // second call was refused as well": the second would land, and an
    // executor that carried on past a refusal leaves it on disk.
    let inside = scratch.project().join("would-land");
    let provider = Provider::scripted([writes_all(&[
        (
            outside.to_string_lossy().as_ref(),
            "this must not be written",
        ),
        ("would-land", "nor this"),
    ])]);
    let declining = Declining::once();
    let (_outcome, transcript) = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 1,
            mode: Mode::Ask,
            confirmer: Some(&declining),
            held: &held,
            with_inner: true,
        },
        &Policy,
    );

    assert!(
        !outside.exists(),
        "a candidate wrote outside the working directory, at {}",
        outside.display()
    );
    assert!(
        !inside.exists(),
        "the second call of a candidate was applied after the first was refused: the file the \
         user never approved exists at {}",
        inside.display()
    );
    assert_eq!(
        declining.asked(),
        1,
        "the candidate stopped at the refusal, so exactly one question reached the user; {} did",
        declining.asked()
    );
    assert!(
        transcript.contains("\"out_of_tree\":true"),
        "ADR-0011 D4 marks a call that left the tree, and a candidate's call is marked by the \
         same field a turn's is: {transcript}"
    );
    assert!(
        transcript.contains("\"phase\":\"refused\""),
        "D4's record closes a refused call as refused, and a candidate's closes the same way a \
         turn's does: {transcript}"
    );

    // --- The accepting sibling: the same write, permitted ------------------
    let sibling = Scratch::new("out-of-tree-permitted");
    let elsewhere = sibling.base.join("outside-the-tree");
    let landing = sibling.project().join("would-land");
    let provider = Provider::scripted([writes_all(&[
        (
            elsewhere.to_string_lossy().as_ref(),
            "permitted by the user who was asked",
        ),
        ("would-land", "and so is this"),
    ])]);
    let accepting = Declining::nothing();
    let _ = drive(
        &Run_ {
            scratch: &sibling,
            plan: &plan,
            provider: &provider,
            ceiling: 1,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: true,
        },
        &Policy,
    );
    assert!(
        elsewhere.exists() && landing.exists(),
        "both writes must land when the user says yes -- without this the refusal above says \
         nothing about the permission model"
    );
}

/// A sealing key the check owns, so no keyring is needed.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A store on the scratch root holding one bearer, and the redactor the
/// composition would build from it.
///
/// Built through `held_secrets_for_redaction` rather than from a list, because
/// that is the one function the product uses: a check that assembled the held
/// set itself would be asserting about a set nothing in the product produces.
fn held_from_a_store(scratch: &Scratch, value: &str) -> HeldSecrets {
    let keys = StagedKey(SealingKey::mint());
    let mut store =
        CredentialStore::open(scratch.base.join("zaru")).expect("the credential store opens");
    let entry = Entry::notes(
        Alias::new("planted").expect("a plain name is a legal alias"),
        Description::new("the bearer this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "the store held nothing to redact");
    held
}

/// The turn ended as an iteration, or say what it ended as instead.
fn expect_iterated(outcome: &Outcome) -> LoopOutcome {
    match outcome {
        Outcome::Iterated(inner) => inner.clone(),
        other => panic!(
            "ADR-0009 D4's branch was supplied, so the turn's body is an iteration; it ended as \
             {other:?}"
        ),
    }
}

// ------------------------------- a validator interrupted while its child ran

/// A bound on failure, never a wait ([Verification lessons] §20).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
const VALIDATOR_POLL_BUDGET: usize = 200_000;

/// Whether a process id is still in the process table. A zombie still has an
/// entry, so absence is the stronger property.
fn still_running(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// **An interrupt while a declared validator's command is running.**
///
/// [ADR-0009] D3's `run` reaches the same `Spawn` [ADR-0011] D1's `cmd.run`
/// does, so the gap that record carried — "the loop blocks for as long as a
/// validator command runs" — was the same gap, and it closes the same way.
/// This is the half of the corpus about the **inner** loop.
///
/// Two things are asserted and they are different in kind.
///
/// **The child ends.** The validator's command reports its own process id and
/// then waits for a gate that never opens; the turn's future is dropped while
/// it runs, and the process leaves the process table. That is the same
/// property `process_from_outside.rs` holds for `cmd.run`, arriving through
/// the port a project's declaration reaches.
///
/// **And the loop reports nothing, which is a finding rather than a defect
/// this arc repairs.** The transcript's loop events are exactly
/// `iteration_started`, `candidate_generated`, `execution_completed` — the
/// iteration cut inside `Evaluate`, with no `validator_evaluated` for the
/// validator that was running and none of `iteration_failed`,
/// `refinement_constructed`, `loop_succeeded` or `loop_exhausted` after it.
/// That is [ADR-0010] D2's "at most the event in flight", honoured. But D4's `Interrupted` is a **tool call's** marker, derived from
/// a `Started` with no `Completed`, and the loop's events are not that shape:
/// a validator in flight is not derivable as interrupted the way a `cmd.run`
/// is, and nothing tells the next resume that an iteration was cut. Recorded
/// as a question for ADR-0010's author rather than answered here, because
/// giving a validator a started-and-completed pair would add a producer to
/// D2's list.
///
/// Its accepting sibling is every check above, each of which drives the same
/// loop to a terminal event.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn corpus_an_interrupt_during_a_validator_ends_its_child_and_the_loop_reports_nothing() {
    let scratch = Scratch::new("validator-interrupt");
    let pidfile = scratch.project().join("validator.pid");
    let never = scratch.project().join("gate-that-never-opens");
    let script = scratch.project().join("validator-waits.sh");
    std::fs::write(
        &script,
        format!(
            "echo $$ > '{}'\nn=0\nwhile [ ! -e '{}' ]; do\n  n=$((n+1))\n  if [ \"$n\" -gt 300 ]; \
             then exit 9; fi\n  sleep 0.01\ndone\nexit 0\n",
            pidfile.display(),
            never.display()
        ),
    )
    .expect("staging: the validator's script");
    let plan = one_validator(&format!("/bin/sh {}", script.display()), "never-matches");
    let provider = Provider::scripted([writes("out.txt", "anything"), writes("out.txt", "again")]);
    let held = HeldSecrets::none();

    let working = WorkingDirectory::at(scratch.project()).expect("the boundary resolves");
    let store = SessionStore::open(scratch.sessions()).expect("the session store opens");
    let id = SessionId::mint(&SystemWallClock).expect("a session id");
    let session = store.start(id).expect("the session starts");
    let transcript_path = session.transcript_path();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread runtime");

    let pid = runtime.block_on(async {
        let pid = {
            let mut transcript =
                Transcript::append_to(&transcript_path).expect("the transcript opens");
            let mut overflow = SessionOverflow::in_session(session.directory());
            let allowlist = NothingAllowed;
            let destructive = NothingDestructive;
            let membrane = NoMembrane;
            let unbuilt = Unbuilt;
            let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
                "PATH",
                "/usr/bin:/bin",
            )]))
            .expect("a child environment");
            let spawn = Spawn::new(
                &working,
                environment,
                ProcessCeiling::new(Duration::from_secs(20)).expect("a usable ceiling"),
            );
            let no_grants = zaru_cli::tools::grants::SessionGrants::none();
            let executor = Executor {
                working_directory: &working,
                mode: Mode::Yolo,
                allowlist: &allowlist,
                destructive: &destructive,
                session_grants: &no_grants,
                confirmer: None,
                verdicts: &membrane,
                budget: OutputBudget::new(4096).expect("a usable budget"),
                preview_budget: OutputBudget::new(4096).expect("a usable budget"),
                search_ceiling: zaru_cli::cli::layers::search_ceiling(),
                overflow: &mut overflow,
                transcript: &mut transcript,
                redactor: &held,
                subprocess: &spawn,
                fetch: &unbuilt,
                projected: &zaru_cli::tools::NoProjection,
                declared: zaru_cli::tools::descriptor_set(),
            };
            let clock = Ticking::default();
            let policy = Policy;
            let patterns =
                zaru_cli::validators::Patterns::new(zaru_cli::cli::layers::pattern_ceiling());
            let schemas = zaru_cli::validators::SchemaFiles::new(
                &working,
                zaru_cli::cli::layers::file_ceiling(),
            );
            let dispatch = Dispatch::new(&plan, &spawn, &patterns, &schemas);

            let cell = tokio::sync::Mutex::new(executor);
            let mut tools = Shared::over(&cell, zaru_cli::tools::descriptor_set());
            let generating = Generating::over(&provider);
            let applying = Applying::through(tools);
            let inner = Inner::over(
                zaru_core::iteration::Ports {
                    generator: &generating,
                    executor: &applying,
                    validators: &dispatch,
                    context: &policy,
                    clock: &clock,
                    redactor: &held,
                },
                Limits {
                    ceiling: Ceiling::new(2).expect("a usable ceiling"),
                    budget: TruncationBudget::new(4096).expect("a usable budget"),
                },
                &transcript_path,
                None,
            );
            let witness = ToolCalling::required(&provider, "staged").expect("it calls tools");
            let mut sink = Records::appending_to(&transcript_path).expect("a second handle");
            let mut sinks: [&mut dyn zaru_core::tool_call::EventSink; 1] = [&mut sink];
            let running = run(
                1,
                Start::Task("do the work"),
                ToolCallCeiling::new(8).expect("a usable ceiling"),
                witness,
                Ports {
                    model: &provider,
                    tools: &mut tools,
                    context: &policy,
                    clock: &clock,
                    redactor: &held,
                },
                Some(&inner),
                &mut sinks,
            );
            tokio::pin!(running);

            let mut polls = 0_usize;
            loop {
                tokio::select! {
                    biased;

                    done = &mut running => panic!(
                        "the turn finished before the validator could be interrupted: {done:?}"
                    ),

                    () = tokio::task::yield_now() => {
                        polls += 1;
                        if let Ok(held) = std::fs::read_to_string(&pidfile)
                            && let Ok(pid) = held.trim().parse::<u32>()
                        {
                            break pid;
                        }
                        assert!(
                            polls < VALIDATOR_POLL_BUDGET,
                            "the validator's command never reported its process id in \
                             {VALIDATOR_POLL_BUDGET} polls"
                        );
                    }
                }
            }
            // Everything the turn held is dropped as this block ends. That
            // is the interrupt.
        };

        // **The wait is inside the runtime, and that is not a detail.** A
        // killed child is reaped by the runtime's own reaper, so a check that
        // dropped the runtime first would be waiting for a zombie nobody had
        // ever waited for — and it would pass or fail on whether the reaping
        // happened to land before the drop returned, which is the
        // probabilistic instrument [Verification lessons] §57 refuses. The
        // shell keeps running on this same runtime after an interrupt, so
        // this is also what the product does.
        let mut polls = 0_usize;
        while still_running(pid) {
            polls += 1;
            assert!(
                polls < VALIDATOR_POLL_BUDGET,
                "the validator's child {pid} is still in the process table after the turn was \
                 interrupted, so a `Ctrl-C` during an iteration leaves a project's command running"
            );
            tokio::task::yield_now().await;
        }
        pid
    });
    println!("  the validator's child {pid} left the process table");

    let written = std::fs::read_to_string(&transcript_path).expect("the transcript was written");
    let events = loop_events(&written);
    println!("  the loop's events are {events:?}");
    assert_eq!(
        events,
        vec![
            String::from("iteration_started"),
            String::from("candidate_generated"),
            String::from("execution_completed"),
        ],
        "the loop's events are not the three an iteration cut inside `Evaluate` leaves, so this \
         check is not looking at an interrupted validator at all"
    );
    for terminal in [
        "validator_evaluated",
        "iteration_failed",
        "refinement_constructed",
        "loop_succeeded",
        "loop_exhausted",
    ] {
        assert!(
            !events.iter().any(|name| name == terminal),
            "the loop emitted `{terminal}` for an iteration that was interrupted, so the \
             transcript claims an outcome the loop never reached"
        );
    }
}
/// A real [`TurnContext`] over a real context, which is what the binary builds.
///
/// The staged [`Policy`] above is deliberately bare, so that the checks about
/// the loop see exactly what the loop constructed. The two checks below are
/// about the **prompt** instead, so they use the product's own policy: the
/// prefix, the byte counter, the redactor and the prepend are all the ones a
/// person's run gets.
/// The product's estimate before any answer, held for the life of the test
/// binary so a policy may borrow it.
fn estimate() -> &'static zaru_cli::providers::capacity::Calibration {
    static STARTING: std::sync::OnceLock<zaru_cli::providers::capacity::Calibration> =
        std::sync::OnceLock::new();
    STARTING.get_or_init(zaru_cli::providers::capacity::Calibration::starting)
}

fn product_policy<'a>(
    context: &'a zaru_core::context::Context,
    held: &'a HeldSecrets,
    iterating: bool,
) -> zaru_cli::compose::TurnContext<'a> {
    zaru_cli::compose::TurnContext::over(context, estimate(), held, iterating)
}

/// [ADR-0008] D1's sentence reaches the model on the **first** exchange and on
/// every refinement after it.
///
/// # Why the first exchange is the load-bearing half
///
/// The failure this sentence exists to close was measured twice — four runs by
/// the `iteration-wiring` arc, then twenty-four by the `prompt-wording` arc on
/// the fixed client, of which the three look-first tasks exhausted eighteen
/// times out of eighteen. In every one of those exhaustions the model spent
/// iteration 1 on `fs.list`. **Iteration 1 is where the run is decided**, so a
/// check that asserted the sentence only in a refinement would pass over an
/// implementation that put it in `refinement::construct` — which is
/// `zaru-core`'s fixed prose, which ADR-0008 D4 forbids adding to, and which
/// the model does not see until it is already too late.
///
/// The assertion is on the **exact bytes** rather than on a phrase, so that
/// paraphrasing the constant reddens this check rather than passing it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[test]
fn the_iteration_instruction_reaches_the_model_on_the_first_exchange_and_on_every_refinement() {
    let scratch = Scratch::new("iteration-instruction");
    let plan = one_validator("cat report.txt", "TOTAL: 3");
    // Two candidates that never satisfy the validator, so the loop runs to its
    // ceiling of two and there is a refinement to read as well as a first ask.
    let provider = Provider::scripted([
        writes("not-the-report", "no"),
        writes("not-the-report", "no again"),
    ]);
    let held = HeldSecrets::none();
    let accepting = Declining::nothing();
    let context = zaru_core::context::Context::opened(
        zaru_cli::compose::prefix_for(None, &facts()),
        zaru_cli::cli::layers::context_limits(zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS),
        0,
    );
    let policy = product_policy(&context, &held, true);

    let _ = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 2,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: true,
        },
        &policy,
    );

    let prompts = provider.prompts();
    assert!(
        prompts.len() >= 2,
        "a ceiling of two must ask twice, so that there is a refinement to read; it asked \
         {} time(s)",
        prompts.len()
    );
    for (n, prompt) in prompts.iter().enumerate() {
        assert!(
            prompt.contains(zaru_cli::compose::prose::ITERATION_IS_ONE_EXCHANGE),
            "the model must be told what an iteration is on exchange {} -- iteration 1 is where \
             the run is decided -- and the prompt it actually got was:\n{prompt}",
            n + 1
        );
    }
}

/// The same sentence is **absent** from a turn that declared no validators.
///
/// # This is the accepting sibling, and it is why ADR-0008 D1 owns the words
///
/// Without it the check above would pass over an implementation that put the
/// sentence into [ADR-0013] D1's layer 1 unconditionally — and there it would
/// be a **falsehood**: the outer tool-call loop hands a call's result back
/// inside the same turn, on `ModelRequest.turn`, so a model told its output
/// "does not come back to you inside this exchange" would have been lied to on
/// every `bare`-tier turn that declares nothing.
///
/// It also keeps the absence from passing vacuously: the task itself is
/// asserted **present** in the same prompt, so a policy that assembled nothing
/// at all could not satisfy this.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[test]
fn a_turn_with_no_declared_validators_is_not_told_an_iteration_is_one_exchange() {
    let scratch = Scratch::new("no-declared-validators");
    let plan = Plan::from_declared(Vec::new()).expect("a project may declare nothing");
    // One answer with nothing to execute, so the outer loop ends the turn.
    let provider = Provider::scripted([ModelResponse::Text {
        echo: None,
        text: "there is nothing to do".to_owned(),
        tokens: usage(),
    }]);
    let held = HeldSecrets::none();
    let accepting = Declining::nothing();
    let context = zaru_core::context::Context::opened(
        zaru_cli::compose::prefix_for(None, &facts()),
        zaru_cli::cli::layers::context_limits(zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS),
        0,
    );
    let policy = product_policy(&context, &held, false);

    let _ = drive(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 2,
            mode: Mode::Ask,
            confirmer: Some(&accepting),
            held: &held,
            with_inner: false,
        },
        &policy,
    );

    let prompts = provider.prompts();
    assert_eq!(prompts.len(), 1, "one answer with no calls is one exchange");
    assert!(
        prompts[0].contains("do the work"),
        "the accepting half: the task must reach the model, or the absence below is vacuous; the \
         prompt was:\n{}",
        prompts[0]
    );
    assert!(
        !prompts[0].contains(zaru_cli::compose::prose::ITERATION_IS_ONE_EXCHANGE),
        "a turn with no declared validators runs the outer loop, where that sentence is false, \
         and it was in the prompt anyway:\n{}",
        prompts[0]
    );
}

/// One emission of the **inner** loop's stream reaches the transcript and a
/// subscriber — [ADR-0008] clause 3, for the loop D3 enumerates.
///
/// # Why this is a second check and not a re-run of the outer loop's
///
/// Clause 3 was claimed on 2026-09-05 with the outer loop's stream: `PaneSink`
/// beside `compose::Records` on one `tool_call::run` slice. But D3's own list
/// is the **eight** iteration events — `IterationStarted`,
/// `CandidateGenerated`, `ValidatorEvaluated`, `RefinementConstructed` — and
/// on that stream there was no second consumer at all. `compose::Inner` passed
/// `iteration::run` the transcript writer alone, so the narrative
/// [ADR-0028] D1 is about existed only on disk.
///
/// This drives the product's own ports to a real outcome with a subscriber
/// attached and compares what it saw against what the file holds, line for
/// line and in order. The two lists are built from one `emit`, so a
/// disagreement between them is the loop handing two consumers different
/// values — which its own contract forbids: it "constructs each event once and
/// hands the same value to every registered sink in turn".
///
/// Watched red by dropping the narrator from the slice in `compose::iterate`,
/// which is what the code did until 2026-09-05; it printed *"the subscriber
/// saw 0 event(s) and the transcript holds 12"*.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn one_emission_of_the_inner_loops_stream_reaches_the_transcript_and_a_subscriber() {
    /// Every event handed to the subscriber, in the order it arrived.
    #[derive(Default)]
    struct Watching(std::sync::Mutex<Vec<zaru_core::iteration::Event>>);

    impl zaru_cli::compose::Narrator for Watching {
        fn narrate(&self, event: &zaru_core::iteration::Event) {
            self.0
                .lock()
                .expect("the subscriber's lock is uncontended")
                .push(event.clone());
        }

        /// This run is not interrupted, and asserting that is the point.
        ///
        /// The port's other method is reached only from a dropped turn, which
        /// this check does not stage: a panic here says the inner loop called
        /// it, which would mean an ordinary run reported itself interrupted.
        fn announce_interrupted(&self) {
            panic!("an uninterrupted run told its subscriber the turn had been interrupted");
        }

        /// The inner loop never says a session's once-ever line, and asserting
        /// that is the point.
        ///
        /// The port's third method belongs to `compose::turn::ran`, which says
        /// it once before the turn narrates anything; a panic here says the
        /// loop reached it, which would mean the notice had become a thing a
        /// turn emits rather than a thing a session says.
        fn announce_session_notice(&self, sentence: &str) {
            panic!("the inner loop said a session's once-ever line: {sentence:?}");
        }
    }

    let scratch = Scratch::new("one-emission");
    let plan = one_validator("cat report.txt", "TOTAL: 3");
    let held = HeldSecrets::none();
    // Two iterations against a validator that never passes, so the run reaches
    // an exhaustion and the stream carries a failure and a refinement rather
    // than only a start and a success.
    let provider = Provider::scripted([
        writes_all(&[("report.txt", "TOTAL: 1")]),
        writes_all(&[("report.txt", "TOTAL: 2")]),
    ]);
    let watching = Watching::default();

    let (_outcome, transcript) = drive_narrating(
        &Run_ {
            scratch: &scratch,
            plan: &plan,
            provider: &provider,
            ceiling: 2,
            mode: Mode::Yolo,
            confirmer: None,
            held: &held,
            with_inner: true,
        },
        &Policy,
        Some(&watching),
    );

    let seen = watching
        .0
        .lock()
        .expect("the subscriber's lock is uncontended")
        .clone();

    // The file, read back with `std::fs`' own bytes rather than through any
    // sink, so neither side of the comparison came from the other.
    let recorded: Vec<zaru_core::iteration::Event> = transcript
        .lines()
        .filter_map(
            |line| match serde_json::from_str::<zaru_cli::session::Record>(line).ok()? {
                zaru_cli::session::Record::Loop(event) => Some(event),
                _ => None,
            },
        )
        .collect();

    assert!(
        !recorded.is_empty(),
        "the run wrote no iteration event at all, so this check compared two empty lists"
    );
    assert_eq!(
        seen.len(),
        recorded.len(),
        "the subscriber saw {} event(s) and the transcript holds {}",
        seen.len(),
        recorded.len()
    );
    assert_eq!(
        seen, recorded,
        "the subscriber and the transcript disagree about what the loop did, so they \
         were not handed the same values from one emission"
    );

    // The narrative is actually a narrative, rather than a start and a stop.
    let kinds: Vec<&str> = seen
        .iter()
        .map(|event| match event {
            zaru_core::iteration::Event::IterationStarted { .. } => "started",
            zaru_core::iteration::Event::CandidateGenerated { .. } => "generated",
            zaru_core::iteration::Event::ExecutionCompleted { .. } => "executed",
            zaru_core::iteration::Event::ValidatorEvaluated { .. } => "evaluated",
            zaru_core::iteration::Event::IterationFailed { .. } => "failed",
            zaru_core::iteration::Event::RefinementConstructed { .. } => "refined",
            zaru_core::iteration::Event::LoopSucceeded { .. } => "succeeded",
            zaru_core::iteration::Event::LoopExhausted { .. } => "exhausted",
        })
        .collect();
    for wanted in [
        "started",
        "generated",
        "executed",
        "evaluated",
        "failed",
        "refined",
    ] {
        assert!(
            kinds.contains(&wanted),
            "the subscriber never saw a {wanted:?} event; ADR-0028 D1 asks for what was \
             tried, what failed and what changed, and this stream carried {kinds:?}"
        );
    }
}

// --------------------------------- a home and an environment nobody handed

/// The facts a check's layer 1 is built from: fixed, so a prompt a check
/// compares is the same on every machine and every day.
fn facts() -> zaru_cli::compose::Facts {
    zaru_cli::compose::Facts {
        directory: Some("/work".to_owned()),
        system: "linux".to_owned(),
        date: "2026-09-28".to_owned(),
        tools: vec!["fs.read".to_owned()],
        mode: None,
    }
}

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
