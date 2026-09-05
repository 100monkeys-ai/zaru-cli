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
use crate::tools::port::{Confirm, Fetch, Question, Subprocess};
use crate::tools::seal::{NoMembrane, Verdict, Verdicts};
use crate::tools::tree::WorkingDirectory;
use crate::tools::writes::{FileWrites, Search};
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

impl FileWrites for Unbuilt {
    async fn apply(&self, tool: ToolName, _target: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("{tool} {UNBUILT}")))
    }
}
impl Search for Unbuilt {
    async fn find(&self, _query: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("fs.search {UNBUILT}")))
    }
}
impl Subprocess for Unbuilt {
    async fn run(
        &self,
        _line: &crate::process::line::CommandLine,
    ) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("cmd.run {UNBUILT}")))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(&self, _url: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("web.fetch {UNBUILT}")))
    }
}

/// A confirmer that answers as it was built to, and records what it was told.
struct Answering {
    answer: bool,
    asked: std::sync::Mutex<Vec<String>>,
}

impl Answering {
    const fn saying(answer: bool) -> Self {
        Self {
            answer,
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Confirm for Answering {
    fn confirm(&self, question: &Question) -> bool {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push(question.statement.clone());
        self.answer
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
     $verdicts:expr, $overflow:expr, $transcript:expr, $unbuilt:expr) => {
        Executor {
            working_directory: $working,
            mode: $mode,
            allowlist: $allow,
            destructive: $destructive,
            confirmer: $confirmer,
            verdicts: $verdicts,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            overflow: $overflow,
            transcript: $transcript,
            redactor: &HeldSecrets::none(),
            writes: $unbuilt,
            search: $unbuilt,
            subprocess: $unbuilt,
            fetch: $unbuilt,
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
    assert_eq!(
        values.len(),
        tool.fields().len(),
        "staging: {tool} takes {:?} and {} value(s) were supplied",
        tool.fields(),
        values.len()
    );
    let object: serde_json::Map<String, serde_json::Value> = tool
        .fields()
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
    let mut executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt
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
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            None,
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt
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
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            None,
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt
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
/// The executor derives the invocation from the request it was handed, so
/// there is no way to pass it a decision made about something else. The
/// mutant: taking an `Invocation` as a parameter, which is the shape that
/// lets a caller decide about `fs.read` and act as `fs.write`.
#[tokio::test]
async fn a_write_is_never_performed_under_a_reads_decision() {
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
    // stands between this request and an act.
    let mut executor = executor!(
        &working,
        Mode::Yolo,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt
    );

    let target = tree.project().join("inside").join("written");
    let asked = request(
        "fs.write",
        &[
            target.to_str().expect("utf-8"),
            "what a model asked to store",
        ],
    );
    let failure = executor
        .execute(&asked)
        .await
        .expect_err("fs.write has no product implementation, so it cannot have acted");

    assert!(
        failure.detail.contains(UNBUILT) && failure.detail.contains("fs.write"),
        "the write should have left through the port that has no implementation, and instead \
         reached: {}",
        failure.detail
    );
    assert!(
        !target.exists(),
        "a file was created at {}, so this crate's product tree performed a write",
        target.display()
    );

    // The accepting arm: the same executor, the same mode, a tool that does
    // act. Without it the assertions above are satisfied by an executor that
    // routes everything to a port.
    let inside = executor
        .execute(&request("fs.read", &["inside/file"]))
        .await
        .expect("fs.read acts");
    assert!(
        matches!(inside, ToolOutcome::Completed { .. }),
        "fs.read must still act, or this check is about an executor that does nothing"
    );
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
    let confirmer = Answering::saying(false);
    let mut executor = executor!(
        &working,
        Mode::Ask,
        &allow,
        &destructive,
        Some(&confirmer),
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt
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
    let confirmer = Answering::saying(false);
    {
        let mut executor = executor!(
            &working,
            Mode::Ask,
            &allow,
            &destructive,
            Some(&confirmer),
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt
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
        let confirmer = Answering::saying(true);
        {
            let mut executor = executor!(
                &working,
                mode,
                &allow,
                &destructive,
                Some(&confirmer),
                &membrane,
                &mut overflow,
                &mut transcript,
                &unbuilt
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
    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &allow,
        destructive: &destructive,
        confirmer: None,
        verdicts: &membrane,
        budget: OutputBudget::new(64).expect("a small budget"),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &HeldSecrets::none(),
        writes: &unbuilt,
        search: &unbuilt,
        subprocess: &unbuilt,
        fetch: &unbuilt,
    };

    let outcome = executor
        .execute(&request("fs.read", &["inside/file"]))
        .await
        .expect("no port failed");

    let ToolOutcome::Completed { result, .. } = outcome else {
        panic!("the read should have acted");
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
        let confirmer = Answering::saying(true);
        let mut executor = executor!(
            &working,
            mode,
            &allow,
            &destructive,
            Some(&confirmer),
            &membrane,
            &mut overflow,
            &mut transcript,
            &unbuilt
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
    let mut executor = executor!(
        &working,
        Mode::Yolo,
        &allow,
        &destructive,
        None,
        &membrane,
        &mut overflow,
        &mut transcript,
        &unbuilt
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
        Event::TurnStarted { n: 1, of: 4 },
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
