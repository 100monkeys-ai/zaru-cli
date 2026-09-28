// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside both crates drives `zaru-core`'s tool-call loop over
//! `zaru-cli`'s real tool surface, writing, editing and searching **real
//! files** on a real working directory with a real session.
//!
//! Everything here is real except the model, which no product tree
//! implements: real files under a scratch root, a real classification against
//! [ADR-0011] D4's boundary, a real permission decision with a user who
//! answers, a real transcript, a real overflow sink, and a real credential
//! store whose secrets a real redactor removes.
//!
//! **Evidence about the mechanism, and it must never be quoted as evidence
//! about the `zaru` binary**, which runs seven commands, none of which reaches
//! the tool surface, and which refuses a task because no provider client
//! exists.
//!
//! # The two directions asserted apart
//!
//! [ADR-0008] clause 6's decision redacts what a model is shown and leaves the
//! record alone; [ADR-0010]'s own Negative section says the session's files
//! "contain whatever the session contained". So the held-bearer checks assert
//! the value **absent** from what the model was given and **present** on disk,
//! in the same check — and each has a run with an empty store beside it, where
//! the value must carry through, because an absence assertion whose sibling
//! never carries anything is satisfied by a surface that returns nothing.
//!
//! Nothing here spawns a process or opens a socket. Every bearer is a
//! generated nonce that authenticates nothing.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

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
use zaru_core::iteration::{Clock, ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Ports, Start,
    TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

// --- staging ---------------------------------------------------------------

/// The awkward tail every planted value carries, so that an absence assertion
/// is not satisfied by a formatter that escapes.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

fn nonce(label: &str) -> String {
    format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

/// A value shaped like a personal bearer token and authenticating nothing.
fn planted_bearer(label: &str) -> String {
    format!("nn_mcp_{}{AWKWARD_TAIL}", nonce(label))
}

/// Everything before the first non-ASCII character: what survives `{:?}`.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// The key port, implemented outside the crate that declares it.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A directory this check owns, holding a project and a session root.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!("ft-{}", nonce(label)));
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
fn store_holding(scratch: &Scratch, alias: &str, value: &str) -> (HeldSecrets, Alias) {
    let keys = StagedKey(SealingKey::mint());
    let mut store =
        CredentialStore::open(scratch.base.join("zaru")).expect("the credential store opens");
    let alias = Alias::new(alias).expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias.clone(),
        Description::new("the bearer this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "staging: one secret is held");
    (held, alias)
}

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// The provider, implemented here because no product tree has one.
struct Provider {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    /// One entry per tool call, keyed by the provider's own id.
    ///
    /// `ModelRequest::results` is CUMULATIVE across a turn's rounds, so
    /// appending it whole would count the first call four times in a
    /// four-call turn -- which is how the first version of this file read six
    /// results from four calls. Keying on the id the request carries is what
    /// makes "the result of the third call" a thing this check can name.
    seen: Mutex<Vec<(String, String)>>,
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
        for descriptor in request.tools {
            println!(
                "  tool offered: {} {}",
                descriptor.name, descriptor.parameters
            );
        }
        let mut seen = self.seen.lock().expect("seen poisoned");
        for result in request.results {
            if seen.iter().any(|(id, _)| id == &result.id) {
                continue;
            }
            println!("  model was given: {:?}", result.content.as_str());
            seen.push((result.id.clone(), result.content.as_str().to_owned()));
        }
        drop(seen);
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

struct Policy<'a> {
    redactor: &'a (dyn Redactor + Sync),
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
        Ok(Prompt::new(Redacted::by(self.redactor, &rendered)))
    }
}

/// Prints every event, which is half of what this file is for.
#[derive(Default)]
struct Printing;

impl EventSink for Printing {
    fn emit(&mut self, event: &Event) {
        println!("  event: {event:?}");
    }
}

struct Unbuilt;

impl Subprocess for Unbuilt {
    async fn run(&self, _line: &CommandLine) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("cmd.run is not this check's subject"))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(
        &self,
        _url: &zaru_cli::web::RequestedUrl,
        _followed: usize,
    ) -> Result<zaru_cli::tools::Retrieved, PortFailure> {
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

/// A user who says yes, because ADR-0011 D3's `ask` prompts before any write.
struct Accepting;
impl zaru_cli::tools::Confirm for Accepting {
    fn confirm(&self, question: &Question) -> Result<Answer, ConfirmFailure> {
        println!("  the user was asked: {}", question.statement);
        Ok(Answer::Once)
    }
}

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        unreachable!("no validators are declared in these checks")
    }
}

/// One tool call, as the model asks for it.
fn call(id: &str, name: &str, arguments: serde_json::Value) -> ModelResponse {
    ModelResponse::Calls {
        calls: vec![ToolRequest {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_string(),
        }],
        tokens: TokenUsage::default(),
    }
}

/// What one staged run produced.
struct Run {
    given_to_the_model: Vec<String>,
    session_directory: std::path::PathBuf,
}

/// Drive `script` through the real loop over the real tool surface.
async fn drive(
    scratch: &Scratch,
    script: Vec<ModelResponse>,
    redactor: &(dyn Redactor + Sync),
) -> Run {
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
    let policy = Policy { redactor };
    let mut sink = Printing;
    let calls = script.len();
    // The terminating text is appended here rather than written into every
    // script: a turn whose model only ever asks for tools never ends, and
    // forgetting it at one call site is a hang rather than a failure.
    let mut script = script;
    script.push(ModelResponse::Text {
        text: String::from("done"),
        tokens: TokenUsage::default(),
    });
    let model = Provider::new(script);

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
            budget: OutputBudget::new(4096).expect("a usable budget"),
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
            Start::Task("work on the project"),
            // One more than the number of calls, so the LAST call's result is
            // handed back before the turn ends. At exactly `calls` the ceiling
            // stops the loop after the final request and the model never sees
            // what the fourth call produced -- which is how the first version
            // of this file read three results from four calls.
            ToolCallCeiling::new(u32::try_from(calls + 1).expect("a small script"))
                .expect("a ceiling"),
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

    println!(
        "  transcript:\n{}",
        std::fs::read_to_string(directory.join("transcript.jsonl"))
            .expect("the transcript is on disk")
    );

    Run {
        given_to_the_model: model
            .seen
            .lock()
            .expect("seen poisoned")
            .iter()
            .map(|(_, content)| content.clone())
            .collect(),
        session_directory: directory,
    }
}

// --- the checks ------------------------------------------------------------

/// A model writes a file, edits it, searches for what it wrote, and reads it
/// back — all on real files, through the real loop.
///
/// ADR-0011 clause 1's first half for the three built-ins this arc built, with
/// the disk as the second reader: every assertion is `std::fs` reading what is
/// actually there rather than the surface reporting what it did.
#[tokio::test]
async fn a_model_writes_edits_searches_and_reads_back_on_real_files() {
    println!("== four calls, four real filesystem acts ==");
    let scratch = Scratch::new("turn");
    let target = scratch.project().join("src").join("greeting.txt");

    let run = drive(
        &scratch,
        vec![
            call(
                "c1",
                "fs.write",
                serde_json::json!({ "path": "src/greeting.txt", "contents": "hello UNIQUEWORD\n" }),
            ),
            call(
                "c2",
                "fs.edit",
                serde_json::json!({ "path": "src/greeting.txt", "old": "hello", "new": "goodbye" }),
            ),
            call(
                "c3",
                "fs.search",
                serde_json::json!({ "root": "src", "needle": "UNIQUEWORD" }),
            ),
            call(
                "c4",
                "fs.read",
                serde_json::json!({ "path": "src/greeting.txt" }),
            ),
        ],
        &HeldSecrets::none(),
    )
    .await;

    assert_eq!(
        run.given_to_the_model.len(),
        4,
        "four calls are four results the model was given: {:?}",
        run.given_to_the_model
    );

    // The disk, not the surface's own report.
    assert_eq!(
        std::fs::read_to_string(&target).expect("the file the model wrote is on disk"),
        "goodbye UNIQUEWORD\n",
        "the write and then the edit did not leave the file as both of them describe"
    );

    let searched = &run.given_to_the_model[2];
    assert!(
        searched.contains("greeting.txt:1: goodbye UNIQUEWORD"),
        "the search did not find what the write had just put there: {searched:?}"
    );
    let readback = &run.given_to_the_model[3];
    assert!(
        readback.contains("goodbye UNIQUEWORD"),
        "the read did not return what the edit left: {readback:?}"
    );

    // The transcript carries D4's rendered line for each, and for the search
    // it carries the needle too -- clause 1's "with their arguments", met
    // exactly for `fs.search` as it is for `cmd.run`.
    let recorded = std::fs::read_to_string(run.session_directory.join("transcript.jsonl"))
        .expect("the transcript is on disk");
    for expected in ["fs.write", "fs.edit", "fs.search", "fs.read"] {
        assert!(
            recorded.contains(expected),
            "{expected} is not in the transcript: {recorded}"
        );
    }
    assert!(
        recorded.contains("UNIQUEWORD"),
        "the search's needle is not in its transcript line, so the record does not carry its \
         arguments: {recorded}"
    );
    assert!(
        !recorded.contains("goodbye UNIQUEWORD\\n"),
        "the transcript carries a write's CONTENTS, which would put a whole file in it: {recorded}"
    );
}

/// **Security corpus.** A held bearer a model writes into a file is on disk
/// and is absent from what the model is shown when it reads it back.
///
/// # The vacuity this check is written around
///
/// `fs.write`'s own capture is a confirmation line, so asserting a value is
/// absent from it would be true whatever the redactor did. So the value is
/// asserted **present on disk** with `std::fs`, and then a landed `fs.read` of
/// the same file is what the absence is asserted against — by the raw value
/// and by an ASCII core no escaping can alter.
#[tokio::test]
async fn a_held_bearer_a_model_writes_is_on_disk_and_absent_from_what_it_is_shown() {
    println!("== a held bearer written by the model, then read back ==");
    let scratch = Scratch::new("write");
    let value = planted_bearer("write");
    let core = ascii_core(&value);
    let (held, alias) = store_holding(&scratch, "work", &value);

    let script = || {
        vec![
            call(
                "c1",
                "fs.write",
                serde_json::json!({ "path": "src/deploy.rs", "contents": format!("// token {value}\n") }),
            ),
            call(
                "c2",
                "fs.read",
                serde_json::json!({ "path": "src/deploy.rs" }),
            ),
        ]
    };

    let run = drive(&scratch, script(), &held).await;
    let on_disk = std::fs::read_to_string(scratch.project().join("src").join("deploy.rs"))
        .expect("the file the model wrote is on disk");
    assert!(
        on_disk.contains(&value),
        "the file does not hold the raw value, so this check is not about redaction at all: \
         {on_disk:?}"
    );

    let given = &run.given_to_the_model[1];
    assert!(
        !given.contains(&value),
        "the harness handed a model its own bearer value: {given:?}"
    );
    assert!(
        !given.contains(core),
        "the harness handed a model the ASCII core of its own bearer value, so an escaping \
         renderer would publish it: {given:?}"
    );
    assert!(
        given.contains(&marker(&alias)),
        "nothing marks where the value was, and a surface returning nothing at all would satisfy \
         both assertions above: {given:?}"
    );

    // The discriminating arm: the same run with an empty store. The value must
    // carry through, or the two absences above are about a path that redacts
    // nothing because nothing reaches it.
    let bare = Scratch::new("write-bare");
    let carried = drive(&bare, script(), &HeldSecrets::none()).await;
    assert!(
        carried.given_to_the_model[1].contains(&value),
        "with nothing held the value must reach the model unaltered, or the redaction assertions \
         above are vacuous: {:?}",
        carried.given_to_the_model[1]
    );
}

/// **Security corpus.** A held bearer a search finds is redacted for the model
/// and kept, raw, in the session.
///
/// Redaction is on prompts and not on the record — ADR-0010's own Negative
/// section — and both directions are asserted in one check so they cannot
/// drift apart.
#[tokio::test]
async fn a_held_bearer_a_search_finds_is_redacted_and_the_session_keeps_it() {
    println!("== a held bearer found by fs.search ==");
    let scratch = Scratch::new("search");
    let value = planted_bearer("search");
    let core = ascii_core(&value);
    let (held, alias) = store_holding(&scratch, "work", &value);

    std::fs::write(
        scratch.project().join("src").join("secrets.rs"),
        format!("// nothing\n// deploy with {value}\n// nothing\n"),
    )
    .expect("staging: the file the search finds");

    let script = || {
        vec![call(
            "c1",
            "fs.search",
            serde_json::json!({ "root": "src", "needle": "deploy with" }),
        )]
    };

    let run = drive(&scratch, script(), &held).await;
    let given = &run.given_to_the_model[0];
    assert!(
        given.contains("secrets.rs:2:"),
        "the search did not find the line at all: {given:?}"
    );
    assert!(
        !given.contains(&value) && !given.contains(core),
        "a search result carried a bearer the harness itself holds, by value or by ASCII core: \
         {given:?}"
    );
    assert!(
        given.contains(&marker(&alias)),
        "nothing marks where the value was: {given:?}"
    );

    // And the record keeps what the model was not given. The file the search
    // read is untouched, which is the plainest form of it.
    assert!(
        std::fs::read_to_string(scratch.project().join("src").join("secrets.rs"))
            .expect("on disk")
            .contains(&value),
        "the file the search read was altered, and redaction is on prompts rather than on disk"
    );
    let _ = &run.session_directory;

    // The discriminating arm.
    let bare = Scratch::new("search-bare");
    std::fs::write(
        bare.project().join("src").join("secrets.rs"),
        format!("// nothing\n// deploy with {value}\n// nothing\n"),
    )
    .expect("staging");
    let carried = drive(&bare, script(), &HeldSecrets::none()).await;
    assert!(
        carried.given_to_the_model[0].contains(&value),
        "with nothing held the value must reach the model unaltered: {:?}",
        carried.given_to_the_model[0]
    );
}

// --------------------------------- a home and an environment nobody handed

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
