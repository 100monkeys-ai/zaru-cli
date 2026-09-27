// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside both crates drives [ADR-0008] trigger clause 6's decision
//! end to end: a bearer value the harness holds, planted where the harness
//! will read it, and the prompt the model is given printed beside the bytes
//! the session kept.
//!
//! Three real things and one this file supplies. Real: a credential store on
//! its own root, `zaru-cli`'s tool surface reading a real file with
//! `std::fs`, and a real session directory with a real transcript and a real
//! overflow sink. Supplied here: the model, because nothing in any product
//! tree implements one, and the sealing port, because ADR-0007 clause 4 waits
//! on ADR-0003 D2's `aes-gcm` amendment.
//!
//! **Evidence about the mechanism, and it must never be quoted as evidence
//! about the `zaru` binary**, which takes no arguments, prints its version
//! and its composition, exits 0, and reaches none of this.
//!
//! # The one assertion here whose sign is inverted
//!
//! `the_session_keeps_the_bytes_the_model_was_not_given` asserts the value is
//! **present** on disk. [ADR-0010]'s own Negative section is why: plain-text
//! session files "contain \[s\] whatever the session contained, including
//! secrets that appeared in command output. Filesystem permissions are the
//! only protection". Redaction is on prompts and not on the record, and that
//! difference is checked rather than claimed. The `session-lifecycle` arc
//! wrote an assertion of this shape backwards once and recorded it; this one
//! is written knowing that.
//!
//! # No credential, no network, no process
//!
//! Every bearer here is a generated nonce that authenticates nothing, ending
//! in a decomposed grapheme cluster, a precomposed one and an astral-plane
//! character — so that an absence assertion is not satisfied by a formatter
//! that escapes. Nothing spawns a process or opens a socket; the only real
//! effects are file reads and two directories under the system temporary
//! directory, removed when each check ends.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
};
use zaru_cli::process::CommandLine;
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction, marker};
use zaru_cli::session::{SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::port::Answer;
use zaru_cli::tools::{
    Captured, ConfirmFailure, Executor, Fetch, Mode, NoMembrane, OutputBudget, Question,
    SessionOverflow, Subprocess, WorkingDirectory,
};
use zaru_core::iteration::{
    Clock, ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn, ValidatorOutcome,
    ValidatorReport,
};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Ports, Start,
    TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

// --- staging ---------------------------------------------------------------

/// The awkward tail every planted value carries.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

/// A value shaped like a personal bearer token and authenticating nothing.
fn planted_bearer(label: &str) -> String {
    format!(
        "nn_mcp_{label}-{}-{}{AWKWARD_TAIL}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

/// Everything before the first non-ASCII character: what survives `{:?}`.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// Sealing, supplied from outside because no product tree implements it.
///
/// **This says nothing whatever about ADR-0007 D3's encryption at rest.** It
/// is a map of strings, and what a check may conclude from it is that the
/// store hands a secret to the port and takes it back.
/// The key port, implemented outside the crate that declares it.
///
/// That it can be implemented from out here is part of what this check
/// establishes: `KeyStore` is the seam a machine's own keyring sits behind, and
/// a trait that could only be implemented from inside would not be one. The key
/// is kept so this check can open what the store sealed **without going back
/// through the store**, which is the arm of the comparison that must not travel
/// through the code under test.
struct StagedKey(SealingKey);

impl StagedKey {
    fn minted() -> Self {
        Self(SealingKey::mint())
    }
}

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A directory the check owns, holding a store, a session and a project.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!(
                "rs-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(base.join("project").join("src")).expect("staging: the project");
        std::fs::create_dir_all(base.join("sessions")).expect("staging: the session root");
        Self { base }
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A store on the scratch root holding one bearer under one alias.
fn store_holding(
    scratch: &Scratch,
    alias: &str,
    value: &str,
) -> (CredentialStore, StagedKey, Alias) {
    let keys = StagedKey::minted();
    let mut store =
        CredentialStore::open(scratch.base.join("zaru")).expect("the credential store opens");
    let alias = Alias::new(alias).expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias.clone(),
        Description::new("the token this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    (store, keys, alias)
}

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// The provider, implemented here because no product tree has one. It keeps
/// every result it was handed, which is the whole evidence of this file.
struct Provider {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    seen: Mutex<Vec<String>>,
}

impl Provider {
    fn new(script: Vec<ModelResponse>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl Model for Provider {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        for result in request.results {
            println!("  model was given: {:?}", result.content.as_str());
            self.seen
                .lock()
                .expect("seen poisoned")
                .push(result.content.as_str().to_owned());
        }
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

/// A policy that hands the turn straight through, redacting as ADR-0013's
/// assembly does.
struct Policy<'a> {
    redactor: &'a (dyn Redactor + Sync),
    prompts: Mutex<Vec<String>>,
}

impl ContextPolicy for Policy<'_> {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => format!("[initial] {task}"),
            Turn::Refinement { refinement } => format!("[refinement] {}", refinement.as_str()),
            Turn::Resumed { interrupted } => {
                format!("[resumed] this did not complete: {}", interrupted.call())
            }
        };
        let prompt = Prompt::new(Redacted::by(self.redactor, &rendered));
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(prompt.as_str().to_owned());
        Ok(prompt)
    }
}

#[derive(Default)]
struct Quiet;

impl EventSink for Quiet {
    fn emit(&mut self, _event: &Event) {}
}

struct Unbuilt;

impl Subprocess for Unbuilt {
    async fn run(&self, _line: &CommandLine) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("cmd.run has no implementation"))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(&self, _url: &zaru_cli::web::RequestedUrl) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("web.fetch has no implementation"))
    }
}

struct Nothing;
impl zaru_cli::tools::Allowlist for Nothing {
    fn approves(&self, _invocation: &zaru_cli::tools::Invocation<'_>) -> bool {
        false
    }
}
impl zaru_cli::tools::DestructiveMatch for Nothing {
    fn is_destructive(&self, _invocation: &zaru_cli::tools::Invocation<'_>) -> bool {
        false
    }
}

struct Accepting;
impl zaru_cli::tools::Confirm for Accepting {
    fn confirm(&self, _question: &Question) -> Result<Answer, ConfirmFailure> {
        Ok(Answer::Once)
    }
}

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        unreachable!("no validators are declared in these checks")
    }
}

/// What one staged run produced, so the two directions can be asserted apart.
struct Run {
    given_to_the_model: Vec<String>,
    session_directory: std::path::PathBuf,
}

/// Plant `value` in a file, let a model read it with `fs.read`, and report
/// what the model was given and where the session kept its bytes.
async fn read_a_file_carrying(
    scratch: &Scratch,
    value: &str,
    redactor: &(dyn Redactor + Sync),
    budget: usize,
) -> Run {
    std::fs::write(
        scratch.project().join("src").join("config.rs"),
        format!("// deploy with the token {value}\nfn main() {{}}\n").as_bytes(),
    )
    .expect("staging: the file the model reads");

    let store = SessionStore::open(scratch.base.join("sessions")).expect("the session store opens");
    let id = SessionId::mint(&SystemWallClock).expect("an id");
    let session = store.start(id).expect("the session starts");
    let directory = session.directory().to_path_buf();

    let working = WorkingDirectory::at(scratch.project()).expect("the working directory resolves");
    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let accepting = Accepting;
    let clock = Ticking::default();
    let policy = Policy {
        redactor,
        prompts: Mutex::new(Vec::new()),
    };
    let mut sink = Quiet;

    let model = Provider::new(vec![
        ModelResponse::Calls {
            calls: vec![ToolRequest {
                id: String::from("c1"),
                name: String::from("fs.read"),
                arguments: serde_json::json!({ "path": "src/config.rs" }).to_string(),
            }],
            tokens: TokenUsage::default(),
        },
        ModelResponse::Text {
            text: String::from("read"),
            tokens: TokenUsage::default(),
        },
    ]);

    {
        let no_grants = zaru_cli::tools::grants::SessionGrants::none();
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Ask,
            allowlist: &nothing,
            destructive: &nothing,
            session_grants: &no_grants,
            confirmer: Some(&accepting),
            verdicts: &membrane,
            budget: OutputBudget::new(budget).expect("a usable budget"),
            preview_budget: OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor,
            subprocess: &unbuilt,
            fetch: &unbuilt,
            projected: &zaru_cli::tools::NoProjection,
            declared: zaru_cli::tools::descriptor_set(),
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("read the config"),
            ToolCallCeiling::new(3).expect("a ceiling"),
            ToolCalling::required(&model, "staged").expect("the staged model can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
                redactor,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("no port failed");
    }

    Run {
        given_to_the_model: model.seen.lock().expect("seen poisoned").clone(),
        session_directory: directory,
    }
}

// --- the checks ------------------------------------------------------------

#[tokio::test]
async fn a_held_bearer_in_a_file_never_reaches_the_model() {
    println!("== a held bearer, read off disk by a model-driven fs.read ==");
    let scratch = Scratch::new("tool");
    let value = planted_bearer("tool");
    let core = ascii_core(&value);
    let (store, keys, alias) = store_holding(&scratch, "work", &value);
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1);

    let run = read_a_file_carrying(&scratch, &value, &held, 4096).await;
    assert_eq!(
        run.given_to_the_model.len(),
        1,
        "one tool call is one result the model was given"
    );
    let given = &run.given_to_the_model[0];

    assert!(
        !given.contains(&value),
        "the harness handed a model its own bearer value: {given:?}"
    );
    assert!(
        !given.contains(core),
        "the harness handed a model its own bearer value's ASCII core, so an \
         escaping renderer would publish it: {given:?}"
    );
    assert!(
        given.contains(&marker(&alias)),
        "nothing marks where the value was, and a tool surface that returned \
         nothing at all would satisfy both assertions above on its own: \
         {given:?}"
    );
    assert!(
        given.contains("fn main()"),
        "the rest of the file did not reach the model, so the redaction took \
         more than the secret: {given:?}"
    );
}

#[tokio::test]
async fn with_nothing_held_the_same_bytes_reach_the_model_unaltered() {
    // The arm that discriminates. Without it, a tool surface that returned an
    // empty result would satisfy every absence assertion above.
    println!("== the same run, with the harness holding nothing ==");
    let scratch = Scratch::new("nothing");
    let value = planted_bearer("nothing");

    let run = read_a_file_carrying(&scratch, &value, &HeldSecrets::none(), 4096).await;
    let given = &run.given_to_the_model[0];
    assert!(
        given.contains(&value),
        "with nothing held the file's bytes must reach the model unaltered, \
         or the check above is not about redaction: {given:?}"
    );
    assert!(
        !given.contains("<redacted: "),
        "nothing was held and something was marked as redacted anyway: \
         {given:?}"
    );
}

#[tokio::test]
async fn the_session_keeps_the_bytes_the_model_was_not_given() {
    // The inverted assertion, and the whole reason it is here: ADR-0010's
    // Negative section says the session's plain files carry whatever the
    // session carried, and "filesystem permissions are the only protection".
    // Redaction is on prompts and not on the record. A budget small enough to
    // overflow is what puts the whole capture on disk under ADR-0011 D5.
    println!("== what the session kept ==");
    let scratch = Scratch::new("record");
    let value = planted_bearer("record");
    let core = ascii_core(&value);
    let (store, keys, alias) = store_holding(&scratch, "work", &value);
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    let run = read_a_file_carrying(&scratch, &value, &held, 24).await;
    let given = &run.given_to_the_model[0];
    assert!(
        given.contains(&marker(&alias)) || given.contains("bytes elided"),
        "the staging must have gone through D5's truncation, or there is no \
         preserved file for this check to read: {given:?}"
    );
    assert!(!given.contains(&value) && !given.contains(core));

    let mut preserved: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&run.session_directory).expect("the session directory is there")
    {
        let path = entry.expect("a directory entry").path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        println!("  {} -- {} byte(s)", path.display(), text.len());
        if text.contains(&value) {
            preserved.push(path.display().to_string());
        }
    }
    assert!(
        !preserved.is_empty(),
        "no file in the session directory carries the raw value. ADR-0010's \
         Negative section says the record holds whatever the session held, \
         and ADR-0011 D5 requires the full text be written where the user can \
         read it -- a redaction that reached the record would have taken the \
         evidence with it. Directory: {}",
        run.session_directory.display()
    );
    println!("  the raw value survives in: {preserved:?}");
}

#[tokio::test]
async fn a_held_bearer_in_validator_output_never_reaches_the_refinement_prompt() {
    // The refinement path, driven from outside both crates with the product
    // redactor, and read out of the *generator* -- one layer beyond the
    // policy that built the prompt.
    println!("== a held bearer in a validator's own output ==");
    let scratch = Scratch::new("loop");
    let value = planted_bearer("loop");
    let core = ascii_core(&value);
    let (store, keys, alias) = store_holding(&scratch, "work", &value);
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    for (holding, expect_present) in [
        (&held as &(dyn Redactor + Sync), false),
        (&HeldSecrets::none() as &(dyn Redactor + Sync), true),
    ] {
        let generator = Recording::default();
        let validators = Failing(value.clone());
        let policy = Policy {
            redactor: holding,
            prompts: Mutex::new(Vec::new()),
        };
        let clock = Ticking::default();

        zaru_core::iteration::run(
            "make it pass",
            zaru_core::iteration::Limits {
                ceiling: zaru_core::iteration::Ceiling::new(2).expect("a ceiling"),
                budget: zaru_core::iteration::TruncationBudget::new(4096).expect("a budget"),
            },
            zaru_core::iteration::Ports {
                generator: &generator,
                executor: &generator,
                validators: &validators,
                context: &policy,
                clock: &clock,
                redactor: holding,
            },
            &mut [],
        )
        .await
        .expect("the staged run reaches an outcome");

        let prompts = generator.prompts.lock().expect("prompts poisoned").clone();
        assert_eq!(prompts.len(), 2, "two iterations are two prompts");
        let refinement = &prompts[1];
        println!("  refinement prompt: {refinement:?}");
        if expect_present {
            assert!(
                refinement.contains(&value),
                "with nothing held the validator's own output must reach the \
                 prompt verbatim, which is ADR-0008 D4: {refinement:?}"
            );
        } else {
            assert!(
                !refinement.contains(&value) && !refinement.contains(core),
                "a held bearer reached the refinement prompt: {refinement:?}"
            );
            assert!(
                refinement.contains(&marker(&alias)),
                "nothing marks where the value was: {refinement:?}"
            );
        }
    }
}

/// A generator that answers with a fixed candidate and keeps every prompt,
/// and an executor that reports nothing. One type because a check needs both
/// and neither has state the other would disturb.
#[derive(Default)]
struct Recording {
    prompts: Mutex<Vec<String>>,
}

impl zaru_core::iteration::Generator for Recording {
    type Candidate = String;

    async fn generate(
        &self,
        prompt: &Prompt,
    ) -> Result<zaru_core::iteration::Generated<String>, PortFailure> {
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(prompt.as_str().to_owned());
        Ok(zaru_core::iteration::Generated {
            candidate: String::from("a candidate"),
            tokens: 1,
        })
    }
}

impl zaru_core::iteration::Executor for Recording {
    type Candidate = String;

    async fn execute(
        &self,
        _candidate: &String,
    ) -> Result<zaru_core::iteration::ExecutionOutcome, PortFailure> {
        Ok(zaru_core::iteration::ExecutionOutcome {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// A validator that always fails, with the planted value in its own output.
struct Failing(String);

impl zaru_core::iteration::Validators for Failing {
    async fn evaluate(
        &self,
        _execution: &zaru_core::iteration::ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        Ok(vec![ValidatorReport {
            name: String::from("deploy"),
            outcome: ValidatorOutcome::Failed,
            detail: format!("401 unauthorized for token {}", self.0),
        }])
    }
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
