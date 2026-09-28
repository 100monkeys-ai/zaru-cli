// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the acting half, and the cases acting makes reachable.
//!
//! # The security corpus this arc adds to
//!
//! [Testing] puts ADR-0011's surface among the boundaries whose escapes join
//! a permanent corpus that never shrinks. The `tool-surface` arc opened it
//! with five cases about a *classification*. These are about an **act**: the
//! seventeen-path table said which class a path falls in, and these say that
//! nothing outside the tree was actually read. Every one asserts on the
//! out-of-tree file's own contents rather than on its path, because a
//! classifier that is right and an executor that opens something else anyway
//! is exactly the defect a path assertion cannot see.
//!
//! Every out-of-tree case has an in-tree sibling the same rule must accept. A
//! table with no accepting arm is satisfied by an executor that refuses
//! everything.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use crate::redaction::HeldSecrets;
use crate::session::{Phase, Record, SessionStore, Transcript, resume};
use crate::tools::decision::Invocation;
use crate::tools::execute::{Executor, SessionOverflow};
use crate::tools::fixtures::{ScratchTree, nonce};
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::output::{Captured, OutputBudget};
use crate::tools::port::{Answer, Confirm, ConfirmFailure, Fetch, Question, Subprocess};
use crate::tools::seal::{NoMembrane, Verdict, Verdicts};
use crate::tools::tree::WorkingDirectory;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{ToolExecutor, ToolOutcome, ToolRequest};

/// An allowlist and a destructive matcher that answer as they were built to.
///
/// Local rather than `tools::fixtures`', because those hold a `RefCell` and
/// the tool-call loop's `execute` returns a `Send` future, so every port it
/// holds must be `Sync`. Changing the landed fixtures would move them under
/// every check that already uses them; two-line answering doubles do not.
struct StagedAllowlist(bool);

impl crate::tools::port::Allowlist for StagedAllowlist {
    fn approves(&self, _invocation: &Invocation<'_>) -> bool {
        self.0
    }
}

struct StagedDestructive(bool);

impl crate::tools::port::DestructiveMatch for StagedDestructive {
    fn is_destructive(&self, _invocation: &Invocation<'_>) -> bool {
        self.0
    }
}

/// What an unbuilt port says when it is reached.
///
/// A check asserts on this, so "the write left through the port that has no
/// implementation" is an observation rather than an inference.
const UNBUILT: &str = "has no product implementation";

/// Every acting port this arc does not build.
///
/// Each answers with a failure naming itself rather than acting, so a check
/// can tell "the executor routed here" from "the executor did it with
/// `std::fs`" — which is the whole question for the five tools that are
/// ported.
struct Unbuilt;

impl Subprocess for Unbuilt {
    async fn run(
        &self,
        _line: &crate::process::line::CommandLine,
    ) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("cmd.run {UNBUILT}")))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(
        &self,
        _url: &crate::web::RequestedUrl,
        _followed: usize,
    ) -> Result<crate::tools::Retrieved, PortFailure> {
        Err(PortFailure::new(format!("web.fetch {UNBUILT}")))
    }
}

/// A confirmer that answers as it was built to, and records what it was told.
struct Answering {
    answer: Answer,
    asked: std::sync::Mutex<Vec<String>>,
}

impl Answering {
    const fn saying(answer: Answer) -> Self {
        Self {
            answer,
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Confirm for Answering {
    fn confirm(&self, question: &Question) -> Result<Answer, ConfirmFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push(question.statement.clone());
        Ok(self.answer)
    }
}

/// A membrane that denies everything, for the seam's own check.
struct Denying(String);

impl Verdicts for Denying {
    fn verdict(&self, _invocation: &Invocation<'_>) -> Verdict {
        Verdict::Denied {
            code: String::from("PATH_NOT_ALLOWED"),
            reason: self.0.clone(),
        }
    }
}

/// A session on its own root, torn down when the check ends.
struct Scratch {
    root: std::path::PathBuf,
    session: crate::session::Session,
}

impl Scratch {
    fn new() -> Self {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("tcl-session"));
        std::fs::create_dir_all(&root).expect("staging: the session root");
        let store = SessionStore::open(&root).expect("staging: the session store");
        let id = crate::session::SessionId::mint(&crate::session::SystemWallClock)
            .expect("staging: a session id");
        let session = store.start(id).expect("staging: the session");
        Self { root, session }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Build an executor over a scratch tree and a scratch session.
macro_rules! executor {
    ($working:expr, $mode:expr, $allow:expr, $destructive:expr, $confirmer:expr,
     $verdicts:expr, $overflow:expr, $transcript:expr, $unbuilt:expr, $grants:expr) => {
        Executor {
            working_directory: $working,
            mode: $mode,
            allowlist: $allow,
            destructive: $destructive,
            session_grants: $grants,
            confirmer: $confirmer,
            verdicts: $verdicts,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            preview_budget: OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: crate::cli::layers::search_ceiling(),
            overflow: $overflow,
            transcript: $transcript,
            redactor: &HeldSecrets::none(),
            subprocess: $unbuilt,
            fetch: $unbuilt,
            // Nothing in this file's checks projects a server, so reaching one
            // is the harness having gone somewhere it had no business going --
            // which this says louder than a refusal would.
            projected: &crate::tools::fixtures::UnreachableProjection,
            declared: crate::tools::descriptor_set(),
        }
    };
}

/// A request whose arguments are the JSON object the named tool declares.
///
/// Values are positional, in `ToolName::fields` order, so a check reads as
/// ADR-0011 D1's row does. Built here rather than typed at each call site,
/// because a hand-written object at twenty call sites is the wire contract
/// transcribed twenty-one times.
///
/// It refuses rather than skips when the arity is wrong ([Verification
/// lessons] §4): a staging that quietly built the wrong object would make
/// every assertion below a statement about a refusal.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn request(name: &str, values: &[&str]) -> ToolRequest {
    let tool = ToolName::ALL
        .into_iter()
        .find(|tool| tool.as_str() == name)
        .unwrap_or_else(|| panic!("staging: {name} is not one of ADR-0011 D1's seven"));
    let required: Vec<&str> = tool
        .fields()
        .iter()
        .filter(|field| field.required)
        .map(|field| field.name)
        .collect();
    assert_eq!(
        values.len(),
        required.len(),
        "staging: {tool} requires {required:?} and {} value(s) were supplied",
        values.len()
    );
    let object: serde_json::Map<String, serde_json::Value> = required
        .iter()
        .zip(values)
        .map(|(field, value)| {
            (
                (*field).to_owned(),
                serde_json::Value::String((*value).to_owned()),
            )
        })
        .collect();
    raw_request(name, &serde_json::Value::Object(object).to_string())
}

/// A request whose arguments are exactly what the caller wrote.
///
/// For the checks that are about a name or an arguments text the contract
/// refuses, where building a well-formed object would be building the thing
/// under test.
fn raw_request(name: &str, arguments: &str) -> ToolRequest {
    ToolRequest {
        id: nonce("call"),
        name: name.to_owned(),
        arguments: arguments.to_owned(),
    }
}

/// ADR-0011 clause 1's first half, for the two built-ins that act.
///
/// The mutant: reading the spelling the caller passed rather than the
/// resolved path, which is what makes an escape possible at all.
#[tokio::test]
async fn a_read_inside_the_working_directory_returns_the_files_bytes() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the working directory resolves");
    let scratch = Scratch::new();
    let contents = nonce("what-is-inside");
    std::fs::write(tree.project().join("inside").join("file"), &contents).expect("staging");

    let mut transcript =
        Transcript::append_to(scratch.session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let mut executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );

    let outcome = executor
        .execute(&request("fs.read", &["inside/file"]))
        .await
        .expect("no port failed");

    match outcome {
        ToolOutcome::Completed { result, decision } => {
            assert!(
                result.content.as_str().contains(&contents),
                "the read did not return the file's bytes: {:?}",
                result.content
            );
            assert!(!result.failed, "a successful read is not a failure");
            assert!(
                decision.permitted,
                "an in-tree read at `ask` does not prompt"
            );
        }
        other => panic!("an ordinary in-tree read should have acted: {other:?}"),
    }
}

/// **Security corpus.** A read outside the tree does not read the file.
///
/// Asserted on the out-of-tree file's own contents, not on its path: the
/// mutant this catches is an executor that classifies correctly and then
/// opens the path it was handed anyway.
#[tokio::test]
async fn nothing_outside_the_working_directory_is_ever_read() {
    // Every shape ADR-0011 D4 names, and the two that a naive rule gets
    // wrong. Each is paired below with an in-tree case the same rule accepts.
    let outside = [
        "../elsewhere/secret",
        "../projectevil/loot",
        "escape/secret",
        "does-not-exist/../../elsewhere/secret",
    ];

    for target in outside {
        let tree = ScratchTree::new();
        let working = WorkingDirectory::at(tree.project()).expect("resolves");
        let scratch = Scratch::new();
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let mut overflow = SessionOverflow::in_session(scratch.session.directory());
        let allow = StagedAllowlist(false);
        let destructive = StagedDestructive(false);
        let unbuilt = Unbuilt;
        let membrane = NoMembrane;
        // No confirmer: an out-of-tree read prompts in `ask` (D4), and a
        // prompt nobody can answer is refused rather than performed.
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            None,
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );

        let outcome = executor
            .execute(&request("fs.read", &[target]))
            .await
            .expect("a refusal is not a port failure");

        let rendered = format!("{outcome:?}");
        assert!(
            !rendered.contains(tree.sentinel()),
            "reading {target:?} produced the contents of a file outside the working directory, \
             so the boundary was classified and then not honoured"
        );
        assert!(
            matches!(outcome, ToolOutcome::Refused { .. }),
            "{target:?} is outside the tree and had nobody to ask, so it must be refused: \
             {outcome:?}"
        );

        // The accepting arm. Without it an executor that refuses everything
        // satisfies every assertion above.
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            None,
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );
        let inside = executor
            .execute(&request("fs.read", &["inside/file"]))
            .await
            .expect("no port failed");
        assert!(
            matches!(inside, ToolOutcome::Completed { .. }),
            "the in-tree sibling of {target:?} must still be allowed, or this check is satisfied \
             by an executor that refuses everything: {inside:?}"
        );
    }
}

/// **Security corpus.** A decision reached about one call cannot authorise
/// another.
///
/// The executor derives the invocation from the request it was handed and
/// [`Executor::act`] matches the **subject** against the **call**, so a
/// decision reached about a path cannot be spent on a different act. The
/// mutant: taking an `Invocation` as a parameter, which is the shape that lets
/// a caller decide about `fs.read` and act as `fs.write`.
///
/// **Re-transcribed 2026-09-05, when `fs.write` began to act.** Until then the
/// assertion was that a write left through a port with no implementation,
/// which is a fact about the port rather than about the routing; the property
/// the corpus wants is that a request named `fs.read` never creates a file,
/// and it is now checkable against a write that really would have.
#[tokio::test]
async fn a_request_named_for_one_tool_never_performs_another() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(true);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    // `yolo` prompts for nothing, so nothing but the executor's own routing
    // stands between these requests and an act.
    let no_grants = crate::tools::grants::SessionGrants::none();
    let mut executor = executor!(
        &working,
        Mode::Yolo,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );

    let target = tree.project().join("inside").join("written");
    let spelled = target.to_str().expect("utf-8").to_owned();

    // A read of a path that does not exist. It fails as the work fails --
    // exit code, the operating system's own words -- and creates nothing.
    let read = executor
        .execute(&request("fs.read", &[&spelled]))
        .await
        .expect("a read that could not read is not a port failure");
    match read {
        ToolOutcome::Completed { result, .. } => assert!(
            result.failed,
            "a read of a path that is not there reports the work's failure: {:?}",
            result.content
        ),
        other => panic!("a read at yolo is not refused: {other:?}"),
    }
    assert!(
        !target.exists(),
        "a request named fs.read created {}, so the act was chosen by something other than the \
         call that was parsed",
        target.display()
    );

    // A read carrying a write's arguments is not a call at all. The contract
    // refuses it before the decision, so nothing is even classified.
    let confused = executor
        .execute(&request_raw_write_shaped_read(&spelled))
        .await
        .expect("a refusal is not a port failure");
    assert!(
        matches!(confused, ToolOutcome::Refused { .. }),
        "fs.read does not take a `contents` field, and a field nobody declared is refused rather \
         than dropped: {confused:?}"
    );
    assert!(
        !target.exists(),
        "a request named fs.read carrying a write's arguments created {}",
        target.display()
    );

    // The accepting arm, and it is what makes the two assertions above mean
    // something: the SAME path, the SAME executor, the SAME mode, asked for
    // as a write -- and the file appears.
    let wrote = executor
        .execute(&request(
            "fs.write",
            &[&spelled, "what a model asked to store"],
        ))
        .await
        .expect("fs.write acts");
    assert!(
        matches!(wrote, ToolOutcome::Completed { .. }),
        "fs.write must act, or this check is about an executor that does nothing: {wrote:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&target).expect("the file the write created is on disk"),
        "what a model asked to store",
        "the write did not put the bytes there"
    );
}

/// An `fs.read` request carrying `fs.write`'s arguments.
///
/// Built by hand rather than through `request`, which refuses a wrong arity
/// as a staging error -- and a wrong arity is exactly what this is.
fn request_raw_write_shaped_read(path: &str) -> ToolRequest {
    raw_request(
        "fs.read",
        &serde_json::json!({ "path": path, "contents": "what a model asked to store" }).to_string(),
    )
}

/// ADR-0011 D3's `ask` mode on a command, with the user declining.
///
/// The mutant: treating a declined prompt as permission, or as an error. It
/// is neither: the call does not act and the outcome is a refusal the model
/// is told about.
#[tokio::test]
async fn a_command_at_ask_with_the_user_declining_does_not_act() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let confirmer = Answering::saying(Answer::No);
    let no_grants = crate::tools::grants::SessionGrants::none();
    let mut executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        Some(&confirmer),
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );

    // `Unbuilt::run` answers with a failure naming itself, so reaching it at
    // all would surface as an `Err` here rather than as a refusal.
    let outcome = executor
        .execute(&request("cmd.run", &["rm -rf /"]))
        .await
        .expect("a declined prompt is not a port failure");

    match outcome {
        ToolOutcome::Refused {
            decision, because, ..
        } => {
            assert!(!decision.permitted);
            assert!(
                because.contains("did not permit"),
                "the refusal should say the user was asked and declined: {because:?}"
            );
        }
        other => panic!("a declined command must not act: {other:?}"),
    }
    assert_eq!(
        confirmer.asked.lock().expect("asked poisoned").len(),
        1,
        "the user should have been asked exactly once"
    );
}

/// ADR-0010 D4 and ADR-0011 D4 together: a refused call closes its own pair,
/// and a resumed session does not report it as interrupted.
///
/// **The mutant this exists for**: writing only a `Started` for a refusal,
/// which was the shape before `Phase::Refused`. A resume then reports a call
/// the user consciously declined as one that never completed, and the model
/// is told the opposite of what happened.
#[tokio::test]
async fn a_refused_call_is_recorded_and_is_never_reported_as_interrupted() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let confirmer = Answering::saying(Answer::No);
    {
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            Some(&confirmer),
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );
        executor
            .execute(&request("cmd.run", &["echo hello"]))
            .await
            .expect("a refusal is not a port failure");
    }

    let restored = resume(scratch.session.directory(), 16).expect("the session resumes");
    let phases: Vec<Phase> = restored
        .tail
        .iter()
        .filter_map(|record| match record {
            Record::ToolCall(call) => Some(call.phase),
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        vec![Phase::Started, Phase::Refused],
        "a refused call owes a pair: ADR-0011 D4 says mode may remove the prompt and never the \
         record, and a `Started` with nothing closing it is an interruption"
    );
    assert!(
        restored.interrupted.is_none(),
        "the user declined this call, so a resumed session must not tell the model it did not \
         complete: {:?}",
        restored.interrupted
    );
}

/// ADR-0011 D4's "mode may remove the prompt; it never removes the record",
/// now with a call that really acted behind it.
#[tokio::test]
async fn the_record_is_written_at_every_mode_including_yolo() {
    for mode in Mode::ALL {
        let tree = ScratchTree::new();
        let working = WorkingDirectory::at(tree.project()).expect("resolves");
        let scratch = Scratch::new();
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let mut overflow = SessionOverflow::in_session(scratch.session.directory());
        let allow = StagedAllowlist(true);
        let destructive = StagedDestructive(false);
        let unbuilt = Unbuilt;
        let membrane = NoMembrane;
        let confirmer = Answering::saying(Answer::Once);
        {
            let no_grants = crate::tools::grants::SessionGrants::none();
            let mut executor = executor!(
                &working,
                mode,
                &allow,
                &destructive,
                Some(&confirmer),
                &membrane,
                &mut overflow,
                &mut transcript,
                &unbuilt,
                &no_grants
            );
            executor
                .execute(&request("fs.read", &["inside/file"]))
                .await
                .expect("no port failed");
        }

        let restored = resume(scratch.session.directory(), 16).expect("resumes");
        let lines: Vec<String> = restored
            .tail
            .iter()
            .filter_map(|record| match record {
                Record::ToolCall(call) => Some(call.line.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines.len(),
            2,
            "at {mode} the call owes a started and a completed line"
        );
        assert!(
            lines[0].starts_with("fs.read "),
            "the record should name the tool at {mode}: {:?}",
            lines[0]
        );
    }
}

/// ADR-0011 clause 6, now whole: oversized output is truncated, the elision
/// is marked, and the full text is in the session directory at the path shown.
///
/// The mutant: clipping without preserving, which is the truncation D5 says
/// a user cannot notice.
#[tokio::test]
async fn oversized_output_is_preserved_in_the_session_directory_at_the_path_shown() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let big = nonce("HEAD").repeat(200);
    std::fs::write(tree.project().join("inside").join("file"), &big).expect("staging");

    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(true);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &allow,
        destructive: &destructive,
        session_grants: &no_grants,
        confirmer: None,
        verdicts: &membrane,
        budget: OutputBudget::new(64).expect("a small budget"),
        preview_budget: OutputBudget::new(4096).expect("a usable budget"),
        search_ceiling: crate::cli::layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &HeldSecrets::none(),
        subprocess: &unbuilt,
        fetch: &unbuilt,
        projected: &crate::tools::NoProjection,
        declared: crate::tools::descriptor_set(),
    };

    // A search, because an `fs.read` sizes its own answer to the budget
    // since 2026-09-28 and so is not what the budget cuts. The line the
    // search finds is the whole file.
    let outcome = executor
        .execute(&request("fs.search", &["inside/file", "HEAD"]))
        .await
        .expect("no port failed");

    let ToolOutcome::Completed { result, .. } = outcome else {
        panic!("the search should have acted");
    };
    assert!(
        result.content.as_str().contains("bytes elided"),
        "the output exceeded the budget and the elision was not marked: {:?}",
        result.content
    );
    let marker = "full output: ";
    let shown = result.content.as_str();
    let at = shown.find(marker).expect("D5 requires the path be shown");
    let path: std::path::PathBuf = shown[at + marker.len()..].trim().into();
    assert!(
        path.starts_with(scratch.session.directory()),
        "the full text must be preserved in the session directory, and went to {}",
        path.display()
    );
    let preserved = std::fs::read_to_string(&path).expect("the sink wrote the file it named");
    assert!(
        preserved.contains(&big),
        "the preserved file does not carry the whole output, so the truncation lost bytes nobody \
         can recover"
    );
}

/// ADR-0004 D6's seam: a denying verdict refuses the call whatever the mode
/// is, and is presented as ADR-0016 D1 row 1's expected failure.
///
/// Two mutants. Consulting the verdict only outside `yolo`, which is the
/// "mode governs prompting only" clause broken. And presenting a denial in
/// the error register, which ADR-0016 D1 says it is not.
#[tokio::test]
async fn a_denied_verdict_refuses_at_every_mode_and_is_an_expected_failure() {
    let reason = nonce("not in [\"./**\"]");
    for mode in Mode::ALL {
        let tree = ScratchTree::new();
        let working = WorkingDirectory::at(tree.project()).expect("resolves");
        let scratch = Scratch::new();
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let mut overflow = SessionOverflow::in_session(scratch.session.directory());
        let allow = StagedAllowlist(true);
        let destructive = StagedDestructive(false);
        let unbuilt = Unbuilt;
        let membrane = Denying(reason.clone());
        let confirmer = Answering::saying(Answer::Once);
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            mode,
            &allow,
            &destructive,
            Some(&confirmer),
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );

        let outcome = executor
            .execute(&request("fs.read", &["inside/file"]))
            .await
            .expect("a denied verdict is not a port failure");

        match outcome {
            ToolOutcome::Refused { because, .. } => assert!(
                because.contains(&reason) && because.contains("PATH_NOT_ALLOWED"),
                "at {mode} the refusal should carry the registry's code and reason: {because:?}"
            ),
            other => panic!(
                "a denied verdict must refuse at {mode} -- ADR-0011 D3 says a user in `yolo` \
                 inside a membrane is still inside the membrane: {other:?}"
            ),
        }
    }

    // The presentation half, asserted on the type rather than on a renderer.
    let denied = Verdict::Denied {
        code: String::from("SUBCOMMAND_DENIED"),
        reason: String::from("curl not in allowed_subcommands"),
    };
    let classified = denied
        .as_expected_failure()
        .expect("a denial is a failure of some class");
    assert_eq!(
        classified.class(),
        crate::failure::Class::Expected,
        "ADR-0016 D1 row 1 names a denied verdict among the expected failures"
    );
    assert!(
        !classified.class().is_the_error_register(),
        "a denied verdict is the membrane working, and colouring it like a crash teaches users \
         to fear the thing that makes the product safe"
    );
    assert!(
        Verdict::Allowed.as_expected_failure().is_none(),
        "an allowed call is not a failure of any kind"
    );
}

/// The seven the model is offered are ADR-0011 D1's seven, from one walk.
#[test]
fn the_descriptors_are_the_seven_and_carry_the_records_own_words() {
    let offered = super::descriptors();
    assert_eq!(offered.len(), ToolName::ALL.len(), "seven, and no eighth");
    for (descriptor, tool) in offered.iter().zip(ToolName::ALL) {
        assert_eq!(descriptor.name, tool.as_str());
        assert_eq!(
            descriptor.description,
            tool.purpose(),
            "the description is D1's second column, transcribed"
        );
        assert_eq!(
            descriptor.parameters,
            crate::tools::arguments::schema(tool),
            "the descriptor offers the schema the parser enforces; ADR-0011 D1 named no argument \
             schema until directive 20 decided one on 2026-09-05, and an empty string -- what \
             this field held until then -- is not JSON, so a provider client refuses it"
        );
    }
}

/// A name that is not a built-in is not a call, and the model is told rather
/// than the turn failing.
#[tokio::test]
async fn a_name_that_is_not_a_built_in_is_reported_to_the_model_and_is_not_an_error() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(true);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let mut executor = executor!(
        &working,
        Mode::Yolo,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );

    let outcome = executor
        .execute(&raw_request("fs.chmod", r#"{"path":"inside/file"}"#))
        .await
        .expect("an unknown name is not a port failure");

    match outcome {
        ToolOutcome::Refused { because, .. } => assert!(
            because.contains("fs.chmod") && because.contains("seven"),
            "the model should be told which name it asked for and that the set is closed: \
             {because:?}"
        ),
        other => panic!("there is no eighth built-in to call: {other:?}"),
    }
}

/// ADR-0016 clause 7, reachable for the first time: a turn that ran some of
/// its calls and had others refused reports what completed and what did not.
///
/// The mutant: counting a refused call as completed, which tells a reader the
/// harness did something it was told not to do.
#[test]
fn a_turn_that_ran_some_calls_and_had_others_refused_reports_both() {
    use zaru_core::tool_call::Event;

    let stream = vec![
        Event::TurnStarted { n: 1, of: Some(4) },
        Event::ToolCompleted {
            round: 1,
            call: 1,
            name: String::from("fs.read"),
            failed: false,
            content_bytes: 31,
            elapsed: core::time::Duration::from_millis(2),
        },
        Event::ToolRefused {
            round: 1,
            call: 2,
            name: String::from("cmd.run"),
            because: String::from("the user was asked about the call and did not permit it"),
            elapsed: core::time::Duration::from_millis(1),
        },
        Event::ToolCompleted {
            round: 1,
            call: 3,
            name: String::from("fs.list"),
            // A tool that ran and reported its own failure still completed as
            // a step: the harness did what it was asked.
            failed: true,
            content_bytes: 0,
            elapsed: core::time::Duration::from_millis(1),
        },
    ];

    let report = crate::failure::Partial::of_a_turn(&stream).expect("this turn is partial");
    let names = |steps: &[crate::failure::StepName]| {
        steps
            .iter()
            .map(|step| step.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(report.completed()),
        vec!["fs.read", "fs.list"],
        "a call that ran completed, even one whose tool reported bad news"
    );
    assert_eq!(
        names(report.not_completed()),
        vec!["cmd.run"],
        "a refused call is what is outstanding"
    );

    // Both ends refused, which is what makes this a *partial* report rather
    // than a shape that would happily describe a success or a failure.
    let all_done = vec![Event::ToolCompleted {
        round: 1,
        call: 1,
        name: String::from("fs.read"),
        failed: false,
        content_bytes: 1,
        elapsed: core::time::Duration::ZERO,
    }];
    assert_eq!(
        crate::failure::Partial::of_a_turn(&all_done),
        Err(crate::failure::PartialRefused::NothingOutstanding),
        "a turn that ran every call succeeded"
    );
    let none_done = vec![Event::ToolRefused {
        round: 1,
        call: 1,
        name: String::from("cmd.run"),
        because: String::from("no"),
        elapsed: core::time::Duration::ZERO,
    }];
    assert_eq!(
        crate::failure::Partial::of_a_turn(&none_done),
        Err(crate::failure::PartialRefused::NothingCompleted),
        "a turn that ran none of its calls is not a partial success"
    );
}

/// **Security corpus.** A write outside the working directory is refused at
/// the modes ADR-0011 D4 names, and at `yolo` it acts and is still recorded.
///
/// Every arm asserts on the out-of-tree file's **own contents**, never on its
/// path: a classifier that is right and an executor that opens something else
/// anyway is exactly the defect a path assertion cannot see. This is the read
/// corpus's shape applied to the act that destroys rather than discloses.
///
/// D4: "Mode may remove the prompt; it never removes the record." So the
/// `yolo` arm is not an escape — it is the record proving it, and the check
/// asserts the entry carries the out-of-tree marking at all three modes.
///
/// The mutants: classifying the write's path leniently, and dropping the
/// out-of-tree marking once the mode is `yolo`.
#[tokio::test]
async fn a_write_outside_the_working_directory_is_refused_and_always_recorded() {
    // Two spellings that leave the tree by different routes, and a symlink
    // that leaves it wearing an ordinary name -- the case a purely lexical
    // resolution gets wrong.
    for (target, why) in [
        ("../elsewhere/secret", "above the root"),
        (
            "../projectevil/loot",
            "a sibling whose name extends the root's",
        ),
        ("escape/secret", "through a symlink out of the tree"),
    ] {
        let tree = ScratchTree::new();
        let working = WorkingDirectory::at(tree.project()).expect("resolves");
        let scratch = Scratch::new();
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let mut overflow = SessionOverflow::in_session(scratch.session.directory());
        let allow = StagedAllowlist(false);
        let destructive = StagedDestructive(false);
        let unbuilt = Unbuilt;
        let membrane = NoMembrane;

        // `ask` and `allow` both prompt for an out-of-tree call whatever the
        // effect is (D4), and a prompt nobody can answer is refused.
        for mode in [Mode::Ask, Mode::Allow] {
            let no_grants = crate::tools::grants::SessionGrants::none();
            let mut executor = executor!(
                &working,
                mode,
                &allow,
                &destructive,
                None,
                &membrane,
                &mut overflow,
                &mut transcript,
                &unbuilt,
                &no_grants
            );
            let outcome = executor
                .execute(&request("fs.write", &[target, "OVERWRITTEN-BY-THE-MODEL"]))
                .await
                .expect("a refusal is not a port failure");
            assert!(
                matches!(outcome, ToolOutcome::Refused { .. }),
                "{why}: an out-of-tree write at {mode} with nobody to ask must be refused: \
                 {outcome:?}"
            );
        }

        // The file the boundary was protecting still holds its own bytes.
        let sentinel_path = working.classify(target);
        assert_eq!(
            std::fs::read_to_string(sentinel_path.resolved()).expect("the file is still there"),
            tree.sentinel(),
            "{why}: a file outside the working directory was overwritten by a call that was \
             refused"
        );

        // `yolo` prompts for nothing, so the write lands -- and the record
        // still says it left the tree. That is D4's second sentence, and it
        // is why this arm is here rather than omitted as an escape.
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            Mode::Yolo,
            &allow,
            &destructive,
            None,
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );
        let outcome = executor
            .execute(&request("fs.write", &[target, "OVERWRITTEN-BY-THE-MODEL"]))
            .await
            .expect("no port failed");
        assert!(
            matches!(outcome, ToolOutcome::Completed { .. }),
            "{why}: at yolo nothing prompts, and ADR-0011 D2 says this tier is not a sandbox: \
             {outcome:?}"
        );
        assert_eq!(
            std::fs::read_to_string(sentinel_path.resolved()).expect("on disk"),
            "OVERWRITTEN-BY-THE-MODEL",
            "{why}: the yolo arm claims to have written and did not, so the two arms above are \
             about an executor that never writes"
        );

        let recorded = std::fs::read_to_string(scratch.session.transcript_path())
            .expect("the transcript is on disk");
        assert!(
            recorded.contains("OUTSIDE the working directory"),
            "{why}: mode may remove the prompt and never the record, and the record does not say \
             the call left the tree: {recorded}"
        );
        assert!(
            !recorded.contains("OVERWRITTEN-BY-THE-MODEL"),
            "{why}: the transcript carries D4's rendered line -- the tool and its resolved path \
             -- and never a write's contents, which would put a whole file in it: {recorded}"
        );
        println!("{why}: refused twice, recorded three times");

        // The accepting arm. Without it every assertion above is satisfied by
        // an executor that refuses every write. It is at `ask` with a user who
        // says yes rather than at `yolo`, because D3 prompts before ANY write
        // and the discriminating question is whether the answer is honoured --
        // not whether prompting can be switched off.
        let saying_yes = Answering::saying(Answer::Once);
        let no_grants = crate::tools::grants::SessionGrants::none();
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            Some(&saying_yes),
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt,
            &no_grants
        );
        let inside = executor
            .execute(&request("fs.write", &["inside/written", "ordinary"]))
            .await
            .expect("no port failed");
        assert!(
            matches!(inside, ToolOutcome::Completed { .. }),
            "{why}: an in-tree write the user permitted must land: {inside:?}"
        );
        assert_eq!(
            std::fs::read_to_string(tree.project().join("inside").join("written"))
                .expect("the in-tree file is on disk"),
            "ordinary"
        );
    }
}

/// The session's one tool surface offers one descriptor list to both loops.
///
/// [ADR-0009] D4's branch means the outer loop and the inner loop each hold a
/// handle to the same `Executor`, and each answers `descriptors()` through its
/// own `ToolExecutor` implementation. If those were two lists, the set a model
/// is offered inside an iteration and the set the executor will accept would
/// be two sets that can disagree — which is the drift
/// [`descriptor_set`](crate::tools::descriptor_set) exists to make
/// unrepresentable.
///
/// **Asserted by pointer, not by value.** Two separately built lists compare
/// equal: they are derived from the same `ToolName::ALL` walk, so an equality
/// assertion passes against exactly the defect this check is about. Pointer
/// identity is the only reading that separates one list from two copies of
/// one list ([Verification lessons] §13).
///
/// Watched red by giving `Shared::descriptors` a second `OnceLock` of its own,
/// which printed *"the two handles to one tool surface returned two descriptor
/// lists, so the set a model is offered and the set the executor accepts are
/// two sets"*.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn the_two_handles_to_one_tool_surface_return_one_descriptor_list() {
    use zaru_core::tool_call::ToolExecutor as _;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(true);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );

    // The list as the executor itself answers it, taken before the value
    // moves into the lock — which is the only order in which both readings
    // exist to be compared.
    let direct = executor.descriptors().as_ptr();

    let cell = tokio::sync::Mutex::new(executor);
    let outer = crate::compose::Shared::over(&cell, crate::tools::descriptor_set());
    let inner = outer;

    assert!(
        std::ptr::eq(outer.descriptors().as_ptr(), direct),
        "the two handles to one tool surface returned two descriptor lists, so the set a model \
         is offered and the set the executor accepts are two sets"
    );
    assert!(
        std::ptr::eq(inner.descriptors().as_ptr(), direct),
        "the two handles to one tool surface returned two descriptor lists, so the set a model \
         is offered and the set the executor accepts are two sets"
    );

    // The staging, asserted rather than assumed: a check comparing two empty
    // slices would pass by pointer as well ([Verification lessons] §8).
    assert_eq!(
        outer.descriptors().len(),
        ToolName::ALL.len(),
        "the list compared must be the seven ADR-0011 D1 names, or the identity above is an \
         identity between two nothings"
    );
}

/// A candidate applied whole reports zero, and its output is what the tools
/// produced.
///
/// ADR-0008's reserved question — "what an execution *is*" — was decided on
/// 2026-09-05 as a candidate applied through the same tool surface a turn
/// uses. This is that, from the executor's own side.
///
/// Watched red by reporting `1` for a candidate every call of which completed,
/// which printed *"a candidate whose every call completed reported a failing
/// execution, so the validators would be told the change did not apply"*.
#[tokio::test]
async fn a_candidate_applied_whole_reports_zero_and_carries_what_the_tools_produced() {
    use zaru_core::iteration::Executor as _;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let marker = nonce("applied");
    std::fs::write(tree.project().join("inside").join("file"), &marker).expect("staging");

    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(true);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let executor = executor!(
        &working,
        Mode::Yolo,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );
    let cell = tokio::sync::Mutex::new(executor);
    let applying = crate::compose::Applying::through(crate::compose::Shared::over(
        &cell,
        crate::tools::descriptor_set(),
    ));

    let candidate = candidate(&[("fs.read", &["inside/file"]), ("fs.list", &["inside"])]).await;
    let outcome = applying.execute(&candidate).await.expect("no port failed");

    assert_eq!(
        outcome.exit_code, 0,
        "a candidate whose every call completed reported a failing execution, so the validators \
         would be told the change did not apply"
    );
    assert!(
        outcome.stdout.contains(&marker),
        "the execution's standard output must carry what the tools produced, and it carried {:?}",
        outcome.stdout
    );
    assert!(
        outcome.stderr.is_empty(),
        "nothing was refused, so there is nothing for standard error to say: {:?}",
        outcome.stderr
    );
}

/// A refused call stops the candidate, and the call after it is not applied.
///
/// **For the security corpus, which only grows.** ADR-0011 D3's prompt is a
/// question about one call, and applying a candidate's second write after the
/// user declined its first would apply part of a change they said no to.
///
/// **The staging is the discriminating part and it took two attempts.** A
/// confirmer that declines *everything* cannot separate "stopped at the first
/// refusal" from "continued, and the second was refused too" — both leave the
/// second file absent, so the check passes against an executor with no stop in
/// it at all. That mutant survived, which is [Verification lessons] §13: an
/// invariant holding because both sides are wrong together. So the confirmer
/// declines the **first** question and accepts every one after it, and the
/// second write is a call that would land if anything asked for it.
///
/// **Asserted on the filesystem**, not on the outcome: an executor that
/// reported a refusal and wrote the file anyway is exactly the defect an
/// outcome assertion cannot see.
///
/// Watched red by removing the `break`, which printed *"the second call of a
/// candidate was applied after the first was refused: the file the user never
/// approved exists"*.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn a_refused_call_stops_the_candidate_and_the_call_after_it_is_not_applied() {
    use zaru_core::iteration::Executor as _;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let scratch = Scratch::new();
    let first = tree.project().join("inside").join("first");
    let second = tree.project().join("inside").join("second");
    let candidate = candidate(&[
        ("fs.write", &["inside/first", "one"]),
        ("fs.write", &["inside/second", "two"]),
    ])
    .await;

    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    // Nothing pre-approved, so every write is a call that needs asking.
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let confirmer = Declining::once();
    let no_grants = crate::tools::grants::SessionGrants::none();
    let executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        Some(&confirmer as &(dyn Confirm + Sync)),
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );
    let cell = tokio::sync::Mutex::new(executor);
    let applying = crate::compose::Applying::through(crate::compose::Shared::over(
        &cell,
        crate::tools::descriptor_set(),
    ));
    let outcome = applying.execute(&candidate).await.expect("no port failed");

    assert!(
        !second.exists(),
        "the second call of a candidate was applied after the first was refused: the file the \
         user never approved exists at {}",
        second.display()
    );
    assert!(
        !first.exists(),
        "the refused call itself wrote its file, which is a refusal in name only: {}",
        first.display()
    );
    assert_eq!(
        confirmer.asked(),
        1,
        "the candidate stopped at the refusal, so exactly one question reached the user; {} did",
        confirmer.asked()
    );
    assert_ne!(
        outcome.exit_code, 0,
        "a candidate the permission model stopped did not apply, and an execution reporting zero \
         would tell the validators it did"
    );
    assert!(
        !outcome.stderr.is_empty(),
        "the refusal's own sentence is what standard error carries, and it carried nothing"
    );

    // The accepting sibling, on the same candidate: a confirmer that says yes
    // applies both writes. Without it the refusal above is satisfied by an
    // executor that refuses everything.
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let accepting = Declining::nothing();
    let no_grants = crate::tools::grants::SessionGrants::none();
    let executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        Some(&accepting as &(dyn Confirm + Sync)),
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt,
        &no_grants
    );
    let cell = tokio::sync::Mutex::new(executor);
    let applying = crate::compose::Applying::through(crate::compose::Shared::over(
        &cell,
        crate::tools::descriptor_set(),
    ));
    let accepted = applying.execute(&candidate).await.expect("no port failed");
    assert_eq!(
        accepted.exit_code, 0,
        "the accepting sibling must apply, or the refusal above says nothing about the \
         permission model"
    );
    assert!(
        first.exists() && second.exists(),
        "both writes must land when nothing refuses them"
    );
}

/// A user who declines the first `decline` questions and accepts the rest.
///
/// `Declining::once()` is the discriminating staging: the call after the
/// refusal is one that **would** land, so an executor that carries on past a
/// refusal leaves a file behind. A confirmer declining everything cannot
/// separate an executor that stops from one that does not — see the check
/// above, whose first form staged exactly that and let the mutant survive.
struct Declining {
    decline: usize,
    asked: std::sync::Mutex<usize>,
}

impl Declining {
    fn once() -> Self {
        Self {
            decline: 1,
            asked: std::sync::Mutex::new(0),
        }
    }

    fn nothing() -> Self {
        Self {
            decline: 0,
            asked: std::sync::Mutex::new(0),
        }
    }

    fn asked(&self) -> usize {
        *self.asked.lock().expect("not poisoned")
    }
}

impl Confirm for Declining {
    fn confirm(&self, _question: &Question) -> Result<Answer, ConfirmFailure> {
        let mut asked = self.asked.lock().expect("not poisoned");
        *asked += 1;
        Ok(if *asked > self.decline {
            Answer::Once
        } else {
            Answer::No
        })
    }
}

/// A candidate carrying the named calls, built from the same `request` helper
/// every other check here uses.
///
/// Built **through the generator** rather than by hand, so the shape a check
/// applies is the shape a model's answer actually produces. `Candidate` has no
/// public constructor for exactly that reason.
async fn candidate(calls: &[(&str, &[&str])]) -> crate::compose::Candidate {
    use zaru_core::iteration::Generator as _;

    let staged: Vec<ToolRequest> = calls
        .iter()
        .map(|(name, values)| request(name, values))
        .collect();
    let model = StagedCalls(std::sync::Mutex::new(Some(staged)));
    crate::compose::Generating::over(&model)
        .generate(&staged_prompt())
        .await
        .expect("the staged model answered")
        .candidate
}

/// A model that answers once with the calls it was staged with.
struct StagedCalls(std::sync::Mutex<Option<Vec<ToolRequest>>>);

impl zaru_core::tool_call::Model for StagedCalls {
    fn capabilities(&self) -> zaru_core::tool_call::Capabilities {
        zaru_core::tool_call::Capabilities { tool_calling: true }
    }

    async fn respond(
        &self,
        _request: &zaru_core::tool_call::ModelRequest<'_>,
    ) -> Result<zaru_core::tool_call::ModelResponse, PortFailure> {
        Ok(zaru_core::tool_call::ModelResponse::Calls {
            text: String::new(),
            echo: None,
            calls: self
                .0
                .lock()
                .expect("not poisoned")
                .take()
                .expect("staging: asked twice, staged once"),
            tokens: zaru_core::tool_call::TokenUsage {
                prompt: 0,
                completion: 0,
            },
        })
    }
}

fn staged_prompt() -> zaru_core::iteration::Prompt {
    zaru_core::iteration::Prompt::new(zaru_core::redaction::Redacted::by(
        &HeldSecrets::none(),
        "staging",
    ))
}

// --- ADR-0007 D5's projected call, through the executor ---------------------

/// A declared surface carrying D1's seven and one projected tool.
fn with_projected() -> Vec<zaru_core::tool_call::ToolDescriptor> {
    let mut declared = crate::tools::descriptors();
    declared.push(zaru_core::tool_call::ToolDescriptor {
        name: "notes:play.pages.read".to_owned(),
        description: "Read a page (via the cortex I share with the team)".to_owned(),
        parameters: r#"{"type":"object","properties":{"pathOrId":{"type":"string"}}}"#.to_owned(),
    });
    declared
}

/// **Security corpus.** The mutant: delete the `Cow::Owned` arm from
/// `call_for`, so a held value reaches the wire.
///
/// ADR-0007 D3: "a model that can read its own bearer token can exfiltrate it
/// through any tool that takes a string". A projected tool takes strings, and
/// this is the one surface in the harness that would carry one to somebody
/// else's server.
#[tokio::test]
async fn corpus_a_projected_call_carrying_a_held_value_is_refused_and_never_reaches_the_port() {
    let scratch = Scratch::new();
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("a directory");
    let mut transcript =
        Transcript::append_to(scratch.session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let projection = crate::tools::fixtures::StagedProjection::answering("{}");
    let declared = with_projected();

    // A value the store holds, planted here so the check owns it and reaching
    // the redactor through the product's own door rather than a constructor
    // written for a check -- `HeldSecrets` has none, which is the point.
    let store_root = crate::credentials::fixtures::ScratchRoot::new();
    let keys = crate::credentials::sealing::fixtures::StagedKey::minted();
    let mut store = crate::credentials::CredentialStore::open(store_root.store_root())
        .expect("a fresh root opens");
    let planted = format!("nn_mcp_{}", crate::credentials::fixtures::nonce("secret"));
    store
        .add(
            crate::credentials::Entry::notes(
                crate::credentials::Alias::new("play").expect("a usable alias"),
                crate::credentials::Description::new("the cortex I share with the team")
                    .expect("one line"),
                crate::credentials::Secret::notes(planted.clone()).expect("nn_mcp_ names a kind"),
                crate::credentials::Reach::InstanceLocked(crate::credentials::Instance::new(
                    "play.cortex.page",
                )),
            )
            .expect("an nn_ value builds a Nuclear Notes entry"),
            &keys,
            None,
        )
        .expect("the token is stored");
    let held = crate::redaction::held_secrets_for_redaction(&store, &keys)
        .expect("the store opens its own secrets");
    assert_eq!(held.len(), 1, "the redactor must actually hold the value");

    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &allow,
        destructive: &destructive,
        session_grants: &no_grants,
        confirmer: None,
        verdicts: &membrane,
        budget: OutputBudget::new(4096).expect("a usable budget"),
        preview_budget: OutputBudget::new(4096).expect("a usable budget"),
        search_ceiling: crate::cli::layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &held,
        subprocess: &unbuilt,
        fetch: &unbuilt,
        projected: &projection,
        declared: &declared,
    };

    // **`yolo` deliberately**, so the refusal cannot be mistaken for the
    // permission model declining: at this mode D3 prompts for nothing, and
    // this call still does not happen.
    let outcome = executor
        .execute(&raw_request(
            "notes:play.pages.read",
            &format!(r#"{{"pathOrId":"home","note":"{planted}"}}"#),
        ))
        .await
        .expect("a refusal is not a port failure");

    match outcome {
        ToolOutcome::Refused { because, .. } => {
            assert!(
                because.contains("carried a credential this harness holds"),
                "{because}"
            );
            assert!(
                !because.contains(&planted),
                "the refusal quoted the value it exists to keep in the store"
            );
            assert!(
                !because.contains(crate::credentials::fixtures::ascii_core(&planted)),
                "the refusal quoted the value's ASCII core"
            );
        }
        other => panic!("a held value must not reach a projected server: {other:?}"),
    }
    assert!(
        projection.asked().is_empty(),
        "the call reached the port: {:?}",
        projection.asked()
    );

    // The accepting sibling, so a refuse-everything implementation cannot
    // pass: the same call without the value goes through and reaches the port
    // with exactly what the decision was reached about.
    let outcome = executor
        .execute(&raw_request(
            "notes:play.pages.read",
            r#"{"pathOrId":"home"}"#,
        ))
        .await
        .expect("no port failed");
    assert!(
        matches!(outcome, ToolOutcome::Completed { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        projection.asked(),
        vec![r#"play pages.read {"pathOrId":"home"}"#.to_owned()],
        "the port was not handed the alias, the tool and the arguments the decision was about"
    );
}

/// The mutant: recognise a projected name by its shape rather than against
/// what this session declared.
#[tokio::test]
async fn a_projected_name_this_session_did_not_declare_is_not_a_call() {
    let scratch = Scratch::new();
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("a directory");
    let mut transcript =
        Transcript::append_to(scratch.session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    let projection = crate::tools::fixtures::StagedProjection::answering("{}");
    let declared = with_projected();

    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &allow,
        destructive: &destructive,
        session_grants: &no_grants,
        confirmer: None,
        verdicts: &membrane,
        budget: OutputBudget::new(4096).expect("a usable budget"),
        preview_budget: OutputBudget::new(4096).expect("a usable budget"),
        search_ceiling: crate::cli::layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &HeldSecrets::none(),
        subprocess: &unbuilt,
        fetch: &unbuilt,
        projected: &projection,
        declared: &declared,
    };

    // A tool on a server this session never offered, and a tool the offered
    // server does not carry. Both are the model asking for something that was
    // not declared, and both are told so.
    for invented in [
        "notes:someone.pages.apply_patch",
        "notes:play.pages.apply_patch",
    ] {
        let outcome = executor
            .execute(&raw_request(invented, r#"{"pathOrId":"home"}"#))
            .await
            .expect("a refusal is not a port failure");
        assert!(
            matches!(outcome, ToolOutcome::Refused { .. }),
            "{invented} was not declared and must not be callable: {outcome:?}"
        );
    }
    assert!(
        projection.asked().is_empty(),
        "an undeclared name reached the port: {:?}",
        projection.asked()
    );

    // The accepting sibling: the one that *was* declared still works.
    assert!(
        matches!(
            executor
                .execute(&raw_request(
                    "notes:play.pages.read",
                    r#"{"pathOrId":"home"}"#
                ))
                .await
                .expect("no port failed"),
            ToolOutcome::Completed { .. }
        ),
        "the declared tool stopped working, so the assertions above assert nothing"
    );
}

/// The mutant: make a projected refusal a `PortFailure`.
#[tokio::test]
async fn an_instances_refusal_reaches_the_model_as_a_tool_result_rather_than_ending_the_turn() {
    let scratch = Scratch::new();
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("a directory");
    let mut transcript =
        Transcript::append_to(scratch.session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let allow = StagedAllowlist(false);
    let destructive = StagedDestructive(false);
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let no_grants = crate::tools::grants::SessionGrants::none();
    // What the `play` token actually answers on every workspace it was probed
    // against: it authenticates and is a member of nothing.
    let projection = crate::tools::fixtures::StagedProjection::refusing(
        "You are not a member of that workspace.",
    );
    let declared = with_projected();

    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &allow,
        destructive: &destructive,
        session_grants: &no_grants,
        confirmer: None,
        verdicts: &membrane,
        budget: OutputBudget::new(4096).expect("a usable budget"),
        preview_budget: OutputBudget::new(4096).expect("a usable budget"),
        search_ceiling: crate::cli::layers::search_ceiling(),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &HeldSecrets::none(),
        subprocess: &unbuilt,
        fetch: &unbuilt,
        projected: &projection,
        declared: &declared,
    };

    let outcome = executor
        .execute(&raw_request(
            "notes:play.pages.read",
            r#"{"pathOrId":"home","workspace":"zaru"}"#,
        ))
        .await
        .expect("an instance refusing is not a port failure, and that is the point");

    match outcome {
        ToolOutcome::Completed { result, .. } => {
            assert!(result.failed, "the instance refused, so the result failed");
            assert!(
                result.content.as_str().contains("not a member"),
                "the instance's own words did not reach the model: {:?}",
                result.content
            );
        }
        other => panic!("an honest refusal is a tool result: {other:?}"),
    }
}

/// **A later turn does not overwrite an earlier turn's kept output.**
///
/// Each turn builds its own [`SessionOverflow`] over the same session
/// directory. Until 2026-09-28 each one started counting at one and opened its
/// file with `truncate`, so the second turn's first overflow wrote over
/// `output-0001.txt`, which the first turn had named to the model and which
/// the next turn's conversation still names. The file then held a different
/// command's output from the one it was named for.
#[test]
fn a_second_turns_overflow_keeps_the_first_turns_file() {
    use crate::tools::output::Overflow as _;

    let scratch = Scratch::new();
    let first = Captured {
        exit_code: 0,
        stdout: "the first turn's whole output".to_owned(),
        stderr: String::new(),
    };
    let second = Captured {
        exit_code: 0,
        stdout: "the second turn's whole output".to_owned(),
        stderr: String::new(),
    };

    let kept_first = SessionOverflow::in_session(scratch.session.directory())
        .preserve(&first)
        .expect("the first turn's output is kept");
    let kept_second = SessionOverflow::in_session(scratch.session.directory())
        .preserve(&second)
        .expect("the second turn's output is kept");

    assert_ne!(
        kept_first, kept_second,
        "the second turn kept its output at the path the first turn had already named"
    );
    let read = std::fs::read_to_string(&kept_first).expect("the first file is still there");
    assert!(
        read.contains("the first turn's whole output"),
        "the file the first turn named holds something else now: {read:?}"
    );
}
