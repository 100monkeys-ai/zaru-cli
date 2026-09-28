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
    /// `ModelRequest::turn` is CUMULATIVE across a turn's rounds, so
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
        for message in request.turn {
            let zaru_core::conversation::Message::Tool { id, content, .. } = message else {
                continue;
            };
            if seen.iter().any(|(seen_id, _)| seen_id == id) {
                continue;
            }
            println!("  model was given: {content:?}");
            seen.push((id.clone(), content.clone()));
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
        };
        Ok(Prompt::new(Redacted::by(self.redactor, &rendered)))
    }
}

/// Prints every event, which is half of what this file is for, and keeps
/// what the person was shown of each call.
#[derive(Default)]
struct Printing {
    shown: Vec<zaru_core::tool_call::ResultView>,
}

impl EventSink for Printing {
    fn emit(&mut self, event: &Event) {
        println!("  event: {event:?}");
        if let Event::ToolShown { view, .. } = event {
            self.shown.push(view.clone());
        }
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

/// A user who says yes, because ADR-0011 D3's `ask` prompts before any write,
/// and who remembers what they were asked.
#[derive(Default)]
struct Accepting {
    asked: Mutex<Vec<String>>,
}
impl zaru_cli::tools::Confirm for Accepting {
    fn confirm(&self, question: &Question) -> Result<Answer, ConfirmFailure> {
        println!("  the user was asked: {}", question.statement);
        self.asked.lock().expect("asked poisoned").push(format!(
            "{}\n{}",
            question.statement,
            question.detail.join("\n")
        ));
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
        text: String::new(),
        echo: None,
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
    /// Every question the user was asked, in order.
    asked: Vec<String>,
    /// What the person was shown of each call that completed, in order.
    shown: Vec<zaru_core::tool_call::ResultView>,
}

/// Drive `script` through the real loop over the real tool surface, with a
/// small output budget.
async fn drive(
    scratch: &Scratch,
    script: Vec<ModelResponse>,
    redactor: &(dyn Redactor + Sync),
) -> Run {
    drive_within(scratch, script, redactor, 4096).await
}

/// Drive `script` with an output budget of `budget` bytes.
async fn drive_within(
    scratch: &Scratch,
    script: Vec<ModelResponse>,
    redactor: &(dyn Redactor + Sync),
    budget: usize,
) -> Run {
    drive_running(scratch, script, redactor, budget, &Unbuilt).await
}

/// Drive `script`, running commands through `subprocess`.
async fn drive_running<C: Subprocess + Sync>(
    scratch: &Scratch,
    script: Vec<ModelResponse>,
    redactor: &(dyn Redactor + Sync),
    budget: usize,
    subprocess: &C,
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
    let accepting = Accepting::default();
    let clock = Ticking::default();
    let policy = Policy { redactor };
    let mut sink = Printing::default();
    let calls = script.len();
    // The terminating text is appended here rather than written into every
    // script: a turn whose model only ever asks for tools never ends, and
    // forgetting it at one call site is a hang rather than a failure.
    let mut script = script;
    script.push(ModelResponse::Text {
        echo: None,
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
            budget: OutputBudget::new(budget).expect("a usable budget"),
            preview_budget: OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor,
            subprocess,
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
        asked: accepting.asked.into_inner().expect("asked poisoned"),
        shown: sink.shown,
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

// --------------------------------------------- reading a file in parts

/// What separates a line's number from the line in an `fs.read` answer.
const MARK: &str = "\u{2502}";

/// The binary's own output budget, so these checks see what a model sees.
const BUDGET: usize = zaru_cli::cli::layers::OUTPUT_BUDGET_BYTES;

/// One line of the large file these checks read, by its number.
fn entry(n: usize) -> String {
    format!("entry {n:05}: value {}", n * 7)
}

/// **A model finds the middle of a large file with a ranged read and edits
/// it there.**
///
/// The defect this was written against: `fs.read` took a path and nothing
/// else, and a large file reached the model as its first and last 16 KiB with
/// the middle cut out. Watched red on `6e94f43`: the ranged read was refused
/// for carrying a field `fs.read` did not take.
#[tokio::test]
async fn a_ranged_read_finds_the_middle_of_a_large_file_and_an_edit_there_lands() {
    let scratch = Scratch::new("ranged");
    let target = scratch.project().join("src").join("big.txt");
    let before: String = (1..=5_000).map(|n| format!("{}\n", entry(n))).collect();
    std::fs::write(&target, &before).expect("staging");

    let run = drive_within(
        &scratch,
        vec![
            call(
                "c1",
                "fs.read",
                serde_json::json!({ "path": "src/big.txt", "start_line": 2_498, "line_count": 5 }),
            ),
            call(
                "c2",
                "fs.edit",
                serde_json::json!({
                    "path": "src/big.txt",
                    "old": format!("{}\n", entry(2_500)),
                    "new": "entry 02500: CHANGED\n",
                }),
            ),
        ],
        &HeldSecrets::none(),
        BUDGET,
    )
    .await;

    let read = &run.given_to_the_model[0];
    let wanted: Vec<String> = (2_498..=2_502)
        .map(|n| format!("{n}{MARK}{}", entry(n)))
        .collect();
    assert!(
        wanted.iter().all(|line| read.contains(line.as_str())) && read.contains("5000 line(s)"),
        "a ranged fs.read did not return lines 2498 to 2502 of the 5,000-line file, numbered, \
         with the file's length, so the middle of a large file cannot be found: {read:?}"
    );
    assert!(
        !read.contains(&entry(2_497)) && !read.contains(&entry(2_503)),
        "the ranged read returned lines it was not asked for: {read:?}"
    );

    let after = std::fs::read_to_string(&target).expect("on disk");
    let expected: String = (1..=5_000)
        .map(|n| {
            if n == 2_500 {
                String::from("entry 02500: CHANGED\n")
            } else {
                format!("{}\n", entry(n))
            }
        })
        .collect();
    assert!(
        after == expected,
        "the edit found by the ranged read did not change line 2500 and only line 2500: {:?}",
        run.given_to_the_model[1]
    );
}

/// **Every kind of file a model may read is answered plainly**, through the
/// real loop at the binary's own budget, and none is cut by the budget.
///
/// Watched red on `6e94f43`, where a 100-line file came back with no line
/// numbers, a binary file came back as control bytes and a large one as its
/// two ends.
#[tokio::test]
async fn every_kind_of_file_a_model_reads_is_answered_plainly() {
    let scratch = Scratch::new("kinds");
    let project = scratch.project();
    let data = project.join("data");
    std::fs::create_dir_all(data.join("sub")).expect("staging");
    let write = |name: &str, bytes: &[u8]| std::fs::write(data.join(name), bytes).expect("staging");
    let hundred: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    write("l100.txt", hundred.as_bytes());
    let two_thousand: String = (1..=2_000).map(|n| format!("{}\n", entry(n))).collect();
    write("l2000.txt", two_thousand.as_bytes());
    write(
        "oneline.txt",
        format!("{}\n", "x".repeat(1 << 20)).as_bytes(),
    );
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend((0..=255_u8).cycle().take(10_000));
    write("logo.png", &png);
    write("latin1.txt", b"caf\xe9\n");
    write("empty.txt", b"");
    write("sub/a.txt", b"a\n");
    write("sub/b.txt", b"b\n");
    let outside = scratch.base.join("outside.txt");
    std::fs::write(&outside, "OUTSIDE-CONTENT\n").expect("staging");
    std::os::unix::fs::symlink(&outside, data.join("link")).expect("staging: a link out");

    let reads = [
        "data/l100.txt",
        "data/l2000.txt",
        "data/oneline.txt",
        "data/logo.png",
        "data/latin1.txt",
        "data/empty.txt",
        "data/sub",
        "data/missing.txt",
        "data/link",
    ];
    let script = reads
        .iter()
        .enumerate()
        .map(|(n, path)| {
            call(
                &format!("r{n}"),
                "fs.read",
                serde_json::json!({ "path": path }),
            )
        })
        .collect();
    let run = drive_within(&scratch, script, &HeldSecrets::none(), BUDGET).await;
    let given = &run.given_to_the_model;
    assert_eq!(given.len(), reads.len(), "one result per read");
    for (path, result) in reads.iter().zip(given) {
        println!(
            "--- {path}: {} bytes\n{}",
            result.len(),
            &result[..result.len().min(600)]
        );
        assert!(
            !result.contains("bytes elided"),
            "the output budget cut the answer to {path} in the middle"
        );
        assert!(
            !result.chars().any(|c| c.is_control() && c != '\n'),
            "the answer to {path} carries control bytes"
        );
    }

    assert!(
        given[0].contains(&format!("1{MARK}line 1\n"))
            && given[0].contains(&format!("100{MARK}line 100\n"))
            && given[0].contains("That is the whole file."),
        "a 100-line file did not come back whole and numbered: {:?}",
        given[0]
    );
    assert!(
        given[1].contains(&format!("500{MARK}{}", entry(500)))
            && !given[1].contains(&entry(501))
            && given[1].contains("2000 line(s)")
            && given[1].contains("start_line 501"),
        "a 2,000-line file did not come back as its first 500 lines with where to read on: {:?}",
        given[1]
    );
    assert!(
        given[2].contains("this line is 1048576 bytes long") && given[2].len() < 4_096,
        "a line of a mebibyte was not cut with its length named"
    );
    assert!(
        given[3].contains("a PNG image") && given[3].contains("10008 bytes"),
        "a binary file was not refused naming what it is and its size: {:?}",
        &given[3][..given[3].len().min(300)]
    );
    assert!(
        given[4].contains("is not UTF-8 text") && given[4].contains("5 bytes"),
        "a file that is not UTF-8 was not refused naming its size: {:?}",
        given[4]
    );
    assert!(
        given[5].contains("is empty"),
        "an empty file was not said to be empty: {:?}",
        given[5]
    );
    assert!(
        given[6].contains("is a folder") && given[6].contains("a.txt, b.txt"),
        "a folder was not said to be one with what it holds: {:?}",
        given[6]
    );
    assert!(
        given[7].contains("there is no file or folder at"),
        "a missing path was not said to be missing: {:?}",
        given[7]
    );

    // The permission rule is today's: a link out of the tree is asked about,
    // marked as outside, and read once allowed.
    assert_eq!(
        run.asked.len(),
        1,
        "exactly one read was outside the tree, so exactly one question: {:?}",
        run.asked
    );
    assert!(
        run.asked[0].contains("OUTSIDE"),
        "the question about a link out of the tree does not mark it: {:?}",
        run.asked[0]
    );
    assert!(
        given[8].contains(&format!("1{MARK}OUTSIDE-CONTENT")),
        "the link, once allowed, was not read: {:?}",
        given[8]
    );
}

// ------------------------------------------ changing and writing a file

/// **An edit keeps the file's line endings and its final newline, and says
/// by line number what it changed.**
///
/// Watched red on `f77b1d5`: an edit written with `\n` did not match a file
/// whose lines end in CRLF.
#[tokio::test]
async fn an_edit_keeps_the_files_endings_and_says_which_lines_changed() {
    let scratch = Scratch::new("endings");
    let project = scratch.project();
    std::fs::write(project.join("crlf.txt"), "one\r\ntwo\r\nthree\r\n").expect("staging");
    std::fs::write(project.join("bare.txt"), "first\nsecond\nlast line").expect("staging");
    let big: String = (1..=5_000).map(|n| format!("{}\n", entry(n))).collect();
    std::fs::write(project.join("big.txt"), &big).expect("staging");

    let run = drive_within(
        &scratch,
        vec![
            call(
                "e1",
                "fs.edit",
                serde_json::json!({ "path": "crlf.txt", "old": "one\ntwo\n", "new": "uno\ndos\ntres\n" }),
            ),
            call(
                "e2",
                "fs.edit",
                serde_json::json!({ "path": "bare.txt", "old": "last line", "new": "final line\n" }),
            ),
            call(
                "e3",
                "fs.edit",
                serde_json::json!({ "path": "big.txt", "old": format!("{}\n", entry(2_500)), "new": "one\ntwo\n" }),
            ),
        ],
        &HeldSecrets::none(),
        BUDGET,
    )
    .await;
    let given = &run.given_to_the_model;

    assert_eq!(
        std::fs::read(project.join("crlf.txt")).expect("on disk"),
        b"uno\r\ndos\r\ntres\r\nthree\r\n",
        "an edit written with \\n did not keep a CRLF file's line endings: {:?}",
        given[0]
    );
    assert!(
        given[0].contains("lines 1 to 2 became lines 1 to 3"),
        "the edit did not say which lines it changed: {:?}",
        given[0]
    );
    assert_eq!(
        std::fs::read(project.join("bare.txt")).expect("on disk"),
        b"first\nsecond\nfinal line",
        "an edit gave a file with no final newline one: {:?}",
        given[1]
    );
    assert!(
        given[1].contains("did not end with a newline"),
        "the edit did not say it kept the file without a final newline: {:?}",
        given[1]
    );
    assert!(
        given[2].contains("line 2500 became lines 2500 to 2501")
            && given[2].contains("5001 line(s)"),
        "an edit in the middle of a large file did not say which lines changed: {:?}",
        given[2]
    );
}

/// **An edit whose text is absent shows the nearest lines; one whose text
/// occurs twice says how many times and where; neither changes the file.
/// With `all` every occurrence is replaced, and the person is told so.**
///
/// Watched red on `f77b1d5`: the refusal for absent text showed nothing of
/// the file.
#[tokio::test]
async fn an_absent_or_repeated_edit_changes_nothing_and_says_where() {
    let scratch = Scratch::new("absent");
    let project = scratch.project();
    let code = "def f():\n    return 1\n\ndef g():\n    return 2\n";
    std::fs::write(project.join("code.py"), code).expect("staging");
    let twice = "alpha\nbeta\nalpha\ngamma\n";
    std::fs::write(project.join("twice.txt"), twice).expect("staging");

    let run = drive_within(
        &scratch,
        vec![
            call(
                "a1",
                "fs.edit",
                serde_json::json!({ "path": "code.py", "old": "def g():\n  return 2\n", "new": "def g():\n    return 3\n" }),
            ),
            call(
                "a2",
                "fs.edit",
                serde_json::json!({ "path": "twice.txt", "old": "alpha", "new": "ALPHA" }),
            ),
            call(
                "a3",
                "fs.edit",
                serde_json::json!({ "path": "twice.txt", "old": "alpha", "new": "ALPHA", "all": true }),
            ),
        ],
        &HeldSecrets::none(),
        BUDGET,
    )
    .await;
    let given = &run.given_to_the_model;

    assert_eq!(
        std::fs::read_to_string(project.join("code.py")).expect("on disk"),
        code,
        "an edit whose text is absent changed the file"
    );
    assert!(
        given[0].contains("does not occur")
            && given[0].contains(&format!("4{MARK}def g():"))
            && given[0].contains(&format!("5{MARK}    return 2")),
        "an edit whose text is absent did not show the nearest lines, numbered: {:?}",
        given[0]
    );
    assert!(
        given[1].contains("occurs 2 times")
            && given[1].contains("line 1, column 1; line 3, column 1")
            && given[1].contains("all"),
        "an edit whose text occurs twice did not say how many times, where, and how to replace \
         every one: {:?}",
        given[1]
    );
    assert_eq!(
        std::fs::read_to_string(project.join("twice.txt")).expect("on disk"),
        "ALPHA\nbeta\nALPHA\ngamma\n",
        "an edit with all set did not replace every occurrence, or the refused one acted: {:?}",
        given[2]
    );
    assert!(
        given[2].contains("replaced 2 occurrence(s)")
            && given[2].contains("line 1 became line 1; line 3 became line 3"),
        "an edit with all set did not say which lines it changed: {:?}",
        given[2]
    );
    let every = run
        .asked
        .iter()
        .filter(|asked| asked.contains("replaces every occurrence of:"))
        .count();
    assert_eq!(
        every, 1,
        "the person was not told that one edit replaces every occurrence: {:?}",
        run.asked
    );
}

/// **A write over a file that exists says it replaced it and how large the
/// old one was.**
///
/// Watched red on `f77b1d5`: it said only how many bytes it wrote.
#[tokio::test]
async fn a_write_over_an_existing_file_says_it_replaced_it_and_how_large_it_was() {
    let scratch = Scratch::new("replace");
    let project = scratch.project();
    std::fs::write(project.join("existing.txt"), "old contents\n".repeat(10)).expect("staging");

    let run = drive_within(
        &scratch,
        vec![
            call(
                "w1",
                "fs.write",
                serde_json::json!({ "path": "existing.txt", "contents": "new\n" }),
            ),
            call(
                "w2",
                "fs.write",
                serde_json::json!({ "path": "fresh.txt", "contents": "a\nb\n" }),
            ),
        ],
        &HeldSecrets::none(),
        BUDGET,
    )
    .await;
    let given = &run.given_to_the_model;
    assert!(
        given[0].contains("replaced") && given[0].contains("had 130 byte(s)"),
        "a write over an existing file did not say it replaced it and how large it was: {:?}",
        given[0]
    );
    assert!(
        given[1].contains("created") && given[1].contains("2 line(s)"),
        "a write of a new file did not say it created it: {:?}",
        given[1]
    );
}

// --- what the person is shown of each call --------------------------------

/// A real process runner over the scratch project, with the five variables
/// ADR-0011 D2 gives a child.
fn spawner(scratch: &Scratch) -> (WorkingDirectory, zaru_cli::process::Environment) {
    let working = WorkingDirectory::at(scratch.project()).expect("the working directory resolves");
    let environment = zaru_cli::process::Environment::inherited_minimum(
        &zaru_cli::config::Variables::of([("PATH", "/usr/bin:/bin")]),
    )
    .expect("the harness's own values pass on");
    (working, environment)
}

/// Every text a view holds, summary first.
fn texts(view: &zaru_core::tool_call::ResultView) -> Vec<String> {
    std::iter::once(view.summary.clone())
        .chain(view.rows.iter().map(|row| row.text.clone()))
        .collect()
}

/// The file a view's note names, when it names one.
fn kept_file(view: &zaru_core::tool_call::ResultView) -> Option<std::path::PathBuf> {
    view.rows.iter().find_map(|row| {
        let (_, path) = row.text.split_once("all of it: ")?;
        Some(std::path::PathBuf::from(path))
    })
}

/// **After a command runs, the person is shown its exit, how much it printed
/// on each stream and its last lines, and the whole is kept in the session
/// directory; the model is sent what it was sent before.**
///
/// The survey of 2026-09-28 measured the pane saying `cmd.run reported a
/// failure · 1256 bytes` and nothing of the output. Red with the executor
/// composing no view: "the person was shown nothing of the command: []".
#[tokio::test]
async fn a_command_shows_the_person_its_exit_and_its_last_lines() {
    let scratch = Scratch::new("command-shown");
    let (working, environment) = spawner(&scratch);
    let spawn = zaru_cli::process::Spawn::new(
        &working,
        environment,
        zaru_cli::process::ProcessCeiling::new(Duration::from_secs(30)).expect("not zero"),
    );
    let run = drive_running(
        &scratch,
        vec![call(
            "c1",
            "cmd.run",
            serde_json::json!({"command": "sh -c \"seq 1 30; echo boom 1>&2; exit 3\""}),
        )],
        &HeldSecrets::none(),
        4096,
        &spawn,
    )
    .await;

    let view = run.shown.first().unwrap_or_else(|| {
        panic!(
            "the person was shown nothing of the command: {:?}",
            run.shown
        )
    });
    assert_eq!(
        view.summary,
        "exit 3 · 30 lines on standard output, 1 on standard error"
    );
    let shown = texts(view);
    assert!(
        shown.iter().any(|text| text == "30") && shown.iter().any(|text| text == "boom"),
        "the command's last line and its standard error are not shown: {shown:#?}"
    );
    let kept = kept_file(view).expect("thirty lines do not fit, so the note names the whole");
    assert!(
        kept.starts_with(&run.session_directory),
        "the whole is kept outside the session directory: {}",
        kept.display()
    );
    let whole = std::fs::read_to_string(&kept).expect("the file the note names is there");
    assert!(
        whole.contains("\n1\n2\n3\n"),
        "the whole does not hold the first lines: {whole}"
    );

    assert_eq!(
        run.given_to_the_model.first().map(String::as_str),
        Some(
            format!(
                "exit code: 3\nstdout:\n{}\nstderr:\nboom\n",
                (1..=30).map(|n| format!("{n}\n")).collect::<String>()
            )
            .as_str()
        ),
        "the model was sent something other than the command's result"
    );
}

/// **Security corpus.** A stored key a command prints reaches no row the
/// person is shown and no file that keeps the whole: the redaction marker
/// stands where it was.
///
/// The mutant: composing the view from the capture before it is redacted,
/// which prints the key's ASCII core in a row.
#[tokio::test]
async fn a_stored_key_a_command_prints_is_shown_as_its_marker_and_kept_nowhere() {
    let scratch = Scratch::new("command-secret");
    let bearer = planted_bearer("printed");
    let (held, alias) = store_holding(&scratch, "printed", &bearer);
    std::fs::write(scratch.project().join("secret.txt"), format!("{bearer}\n"))
        .expect("staging: a file holding the key");
    let (working, environment) = spawner(&scratch);
    let spawn = zaru_cli::process::Spawn::new(
        &working,
        environment,
        zaru_cli::process::ProcessCeiling::new(Duration::from_secs(30)).expect("not zero"),
    );
    let run = drive_running(
        &scratch,
        vec![call(
            "c1",
            "cmd.run",
            serde_json::json!({"command": "sh -c \"seq 1 20; cat secret.txt; cat secret.txt 1>&2\""}),
        )],
        &held,
        4096,
        &spawn,
    )
    .await;

    let view = run.shown.first().expect("the command completed");
    let core = ascii_core(&bearer);
    for text in texts(view) {
        assert!(
            !text.contains(core),
            "a row the person is shown holds the stored key: {text:?}"
        );
    }
    assert!(
        texts(view)
            .iter()
            .any(|text| text.contains(&marker(&alias))),
        "the marker does not stand where the key was: {:#?}",
        texts(view)
    );
    let kept = kept_file(view).expect("twenty-one lines do not fit");
    let whole = std::fs::read_to_string(&kept).expect("the kept file is there");
    assert!(
        !whole.contains(core),
        "the file that keeps the whole holds the stored key"
    );
}

/// **Security corpus.** A command's output that would move the cursor, clear
/// the screen, set the title or switch screens reaches the person written
/// out, and nothing in a row is a control character.
///
/// The mutant: rows built from the output as it came, which prints the first
/// control character in a row.
#[tokio::test]
async fn hostile_output_reaches_no_row_as_a_control_character() {
    let scratch = Scratch::new("command-hostile");
    std::fs::write(
        scratch.project().join("hostile.txt"),
        "\u{1b}[2J\u{1b}[H\u{1b}]0;TITLE\u{7}\u{1b}[?1049h\u{1b}[10;10Hmoved\rOVER\u{8}\u{9b}31m\n",
    )
    .expect("staging: hostile bytes");
    let (working, environment) = spawner(&scratch);
    let spawn = zaru_cli::process::Spawn::new(
        &working,
        environment,
        zaru_cli::process::ProcessCeiling::new(Duration::from_secs(30)).expect("not zero"),
    );
    let run = drive_running(
        &scratch,
        vec![call(
            "c1",
            "cmd.run",
            serde_json::json!({"command": "cat hostile.txt"}),
        )],
        &HeldSecrets::none(),
        4096,
        &spawn,
    )
    .await;

    let view = run.shown.first().expect("the command completed");
    for text in texts(view) {
        let bad: Vec<char> = text
            .chars()
            .filter(|character| character.is_control())
            .collect();
        assert!(bad.is_empty(), "a row holds {bad:?}: {text:?}");
    }
    assert!(
        texts(view)
            .iter()
            .any(|text| text.contains("\\u{1b}]0;TITLE\\u{7}")),
        "the title sequence is not written out where it was: {:#?}",
        texts(view)
    );
}

/// **After an edit or a write, the person is shown what changed**: an edit in
/// the middle of a file as its removed and added lines with the lines around
/// them and their numbers; a new file as its first lines and its length; a
/// file replaced as the lines that differ. The model is sent what it was
/// sent before.
///
/// Red with the executor composing no view: "the person was shown nothing
/// of the edit".
#[tokio::test]
async fn an_edit_and_a_write_show_the_person_what_changed() {
    let scratch = Scratch::new("change-shown");
    let forty: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    std::fs::write(scratch.project().join("src").join("lib.rs"), &forty)
        .expect("staging: a forty-line file");
    let run = drive(
        &scratch,
        vec![
            call(
                "c1",
                "fs.edit",
                serde_json::json!({"path": "src/lib.rs", "old": "line 20\n", "new": "line twenty\n"}),
            ),
            call(
                "c2",
                "fs.write",
                serde_json::json!({"path": "notes.txt", "contents": "one\ntwo\n"}),
            ),
            call(
                "c3",
                "fs.write",
                serde_json::json!({"path": "notes.txt", "contents": "one\n2\n"}),
            ),
        ],
        &HeldSecrets::none(),
    )
    .await;

    let edit = run
        .shown
        .first()
        .expect("the person was shown nothing of the edit");
    assert_eq!(edit.summary, "changed src/lib.rs · 1 line removed, 1 added");
    let rows: Vec<(zaru_core::tool_call::Mark, Option<usize>, &str)> = edit
        .rows
        .iter()
        .map(|row| (row.mark, row.number, row.text.as_str()))
        .collect();
    use zaru_core::tool_call::Mark::{Added, Context, Removed};
    assert_eq!(
        rows,
        vec![
            (Context, Some(18), "line 18"),
            (Context, Some(19), "line 19"),
            (Removed, Some(20), "line 20"),
            (Added, Some(20), "line twenty"),
            (Context, Some(21), "line 21"),
            (Context, Some(22), "line 22"),
        ]
    );

    let created = run.shown.get(1).expect("the write completed");
    assert_eq!(created.summary, "created notes.txt · 2 lines, 8 bytes");
    let replaced = run.shown.get(2).expect("the second write completed");
    assert_eq!(
        replaced.summary,
        "replaced notes.txt · 1 line removed, 1 added"
    );

    assert!(
        run.given_to_the_model
            .first()
            .is_some_and(|given| given.contains("replaced 1 occurrence(s) in")),
        "the model was not sent the edit's own answer: {:?}",
        run.given_to_the_model
    );
}

/// A read, a listing and a search are each one line: which lines of which
/// file, how many entries, how many matches.
#[tokio::test]
async fn a_read_a_listing_and_a_search_are_one_line_each() {
    let scratch = Scratch::new("one-line-shown");
    std::fs::write(
        scratch.project().join("src").join("a.rs"),
        "let retry = 1;\nretry += 1;\nlast\n",
    )
    .expect("staging: a file");
    let run = drive(
        &scratch,
        vec![
            call("c1", "fs.read", serde_json::json!({"path": "src/a.rs"})),
            call("c2", "fs.list", serde_json::json!({"path": "src"})),
            call(
                "c3",
                "fs.search",
                serde_json::json!({"root": ".", "needle": "retry"}),
            ),
        ],
        &HeldSecrets::none(),
    )
    .await;
    let summaries: Vec<(&str, usize)> = run
        .shown
        .iter()
        .map(|view| (view.summary.as_str(), view.rows.len()))
        .collect();
    assert_eq!(
        summaries,
        vec![
            ("read src/a.rs · lines 1 to 3 of 3", 0),
            ("listed src · 1 entry", 0),
            ("searched . for \"retry\" · 2 lines found in 1 file", 0),
        ]
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
