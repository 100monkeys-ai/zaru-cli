// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives `zaru-core`'s tool-call loop over
//! `zaru-cli`'s real tool surface, on a real working directory and a real
//! session.
//!
//! The two loops meet here, and so do the two crates: `zaru-core` supplies
//! the cycle and knows nothing about tools, `zaru-cli` supplies the acting
//! half and knows nothing about the cycle. Nothing in either product tree
//! implements a model, so the provider is this file's.
//!
//! **Evidence about the mechanism, and it must never be quoted as evidence
//! about the `zaru` binary**, which takes no arguments, prints its version
//! and its composition, exits 0, and reaches none of this.
//!
//! Nothing here spawns a process, opens a socket, or holds a credential. The
//! only real effects are reads and a session directory under the system
//! temporary directory, removed when the check ends.

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::failure::{Class, Presentation};
use zaru_cli::session::{Phase, Record, SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::{
    Captured, Executor, Fetch, FileWrites, Mode, NoMembrane, OutputBudget, Question,
    SessionOverflow, Subprocess, ToolName, Verdict, Verdicts, WorkingDirectory,
};
use zaru_core::iteration::{
    Clock, ContextPolicy, ContextRefusal, Interruption, PortFailure, Prompt, Turn,
};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome, Ports,
    Start, TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

/// A directory the check owns, with the shapes ADR-0011 D4 separates.
struct Tree {
    base: std::path::PathBuf,
    sentinel: String,
}

impl Tree {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!(
                "{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        let tree = Self {
            base,
            sentinel: String::from("SENTINEL-outside-the-working-directory"),
        };
        std::fs::create_dir_all(tree.project().join("src")).expect("staging: project/src");
        std::fs::create_dir_all(tree.base.join("elsewhere")).expect("staging: elsewhere");
        std::fs::write(
            tree.project().join("src").join("main.rs"),
            b"fn main() { println!(\"zaru\") }\n",
        )
        .expect("staging: the file that is read");
        std::fs::write(
            tree.base.join("elsewhere").join("secret"),
            tree.sentinel.as_bytes(),
        )
        .expect("staging: the file that must not be read");
        tree
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A session on its own root.
struct Scratch {
    root: std::path::PathBuf,
    session: zaru_cli::session::Session,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .expect("resolves")
            .join(format!(
                "{label}-session-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(&root).expect("staging: the session root");
        let store = SessionStore::open(&root).expect("staging: the store");
        let id = SessionId::mint(&SystemWallClock).expect("staging: an id");
        let session = store.start(id).expect("staging: the session");
        Self { root, session }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// The provider, implemented here because no product tree has one.
struct Provider(Mutex<std::collections::VecDeque<ModelResponse>>);

impl Model for Provider {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        println!(
            "  model <- {} tool(s) offered, {} result(s) so far",
            request.tools.len(),
            request.results.len()
        );
        for result in request.results {
            println!("      {} -> {:?}", result.id, result.content);
        }
        self.0
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

#[derive(Default)]
struct Policy(Mutex<Vec<String>>);

impl ContextPolicy for Policy {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => format!("[initial] {task}"),
            Turn::Refinement { refinement } => format!("[refinement] {}", refinement.as_str()),
            Turn::Resumed { interrupted } => {
                format!("[resumed] this did not complete: {}", interrupted.call())
            }
        };
        self.0.lock().expect("poisoned").push(rendered.clone());
        Ok(Prompt::new(rendered))
    }
}

#[derive(Default)]
struct Printing(Vec<Event>);

impl EventSink for Printing {
    fn emit(&mut self, event: &Event) {
        println!("  event {event:?}");
        self.0.push(event.clone());
    }
}

/// Never reached in these checks; present because the executor needs one.
struct Unbuilt;

impl FileWrites for Unbuilt {
    async fn apply(&self, tool: ToolName, _target: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("{tool} has no implementation")))
    }
}
impl zaru_cli::tools::Search for Unbuilt {
    async fn find(&self, _query: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("fs.search has no implementation"))
    }
}
impl Subprocess for Unbuilt {
    async fn run(&self, _command: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("cmd.run has no implementation"))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(&self, _url: &str) -> Result<Captured, PortFailure> {
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

struct Declining;
impl zaru_cli::tools::Confirm for Declining {
    fn confirm(&self, question: &Question) -> bool {
        println!("  the user is asked: {:?} -> no", question.statement);
        false
    }
}

/// The whole thing: a model asks for a read inside the boundary, `std::fs`
/// performs it, the bytes return, and the model answers — with the transcript
/// written around the call.
#[tokio::test]
async fn a_model_reads_a_file_inside_the_boundary_and_the_bytes_reach_it() {
    println!("== a read inside the working directory ==");
    let tree = Tree::new("tcl-inside");
    let scratch = Scratch::new("tcl-inside");
    let working = WorkingDirectory::at(tree.project()).expect("the working directory resolves");
    let mut transcript =
        Transcript::append_to(scratch.session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let clock = Ticking::default();
    let policy = Policy::default();
    let mut sink = Printing::default();

    let model = Provider(Mutex::new(
        [
            ModelResponse::Calls {
                calls: vec![ToolRequest {
                    id: String::from("c1"),
                    name: String::from("fs.read"),
                    arguments: String::from("src/main.rs"),
                }],
                tokens: TokenUsage {
                    prompt: 9,
                    completion: 3,
                },
            },
            ModelResponse::Text {
                text: String::from("the entry point prints zaru"),
                tokens: TokenUsage {
                    prompt: 9,
                    completion: 6,
                },
            },
        ]
        .into(),
    ));

    let outcome = {
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Ask,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer: None,
            verdicts: &membrane,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            overflow: &mut overflow,
            transcript: &mut transcript,
            writes: &unbuilt,
            search: &unbuilt,
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, NeverIterates>(
            1,
            Start::Task("read the entry point"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("no port failed")
    };

    println!("  outcome {outcome:?}");
    assert!(matches!(outcome, Outcome::Answered { .. }));

    let restored = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    println!("  the transcript holds:");
    for record in &restored.tail {
        println!("      {record:?}");
    }
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
        vec![Phase::Started, Phase::Completed],
        "ADR-0010 D2 and ADR-0011 D4: the call owes a pair around the act"
    );
    assert!(
        restored.interrupted.is_none(),
        "the call completed, so nothing was in flight"
    );
}

/// **Security corpus, end to end.** A read outside the boundary is refused,
/// nothing outside is read, and the model is told rather than the turn
/// failing.
#[tokio::test]
async fn a_read_outside_the_boundary_is_refused_and_its_bytes_never_reach_the_model() {
    println!("== a read outside the working directory ==");
    let tree = Tree::new("tcl-outside");
    let scratch = Scratch::new("tcl-outside");
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;
    let declining = Declining;
    let clock = Ticking::default();
    let policy = Policy::default();
    let mut sink = Printing::default();

    let model = Provider(Mutex::new(
        [
            ModelResponse::Calls {
                calls: vec![ToolRequest {
                    id: String::from("c1"),
                    name: String::from("fs.read"),
                    arguments: String::from("../elsewhere/secret"),
                }],
                tokens: TokenUsage::default(),
            },
            ModelResponse::Text {
                text: String::from("I could not read that"),
                tokens: TokenUsage::default(),
            },
        ]
        .into(),
    ));

    let outcome = {
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Ask,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer: Some(&declining),
            verdicts: &membrane,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            overflow: &mut overflow,
            transcript: &mut transcript,
            writes: &unbuilt,
            search: &unbuilt,
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, NeverIterates>(
            1,
            Start::Task("read the secret"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("a declined prompt is not a failure")
    };

    println!("  outcome {outcome:?}");
    let everything = format!("{outcome:?}{:?}", sink.0);
    assert!(
        !everything.contains(&tree.sentinel),
        "a file outside the working directory was read and its contents travelled"
    );
    assert!(
        sink.0
            .iter()
            .any(|event| matches!(event, Event::ToolRefused { .. })),
        "the stream should say the call was refused"
    );

    let restored = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    println!("  the transcript holds:");
    for record in &restored.tail {
        println!("      {record:?}");
    }
    let calls: Vec<&zaru_cli::session::ToolCall> = restored
        .tail
        .iter()
        .filter_map(|record| match record {
            Record::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls.iter().map(|call| call.phase).collect::<Vec<_>>(),
        vec![Phase::Started, Phase::Refused],
        "ADR-0011 D4: mode may remove the prompt, never the record"
    );
    assert!(
        calls[0].out_of_tree,
        "D4 requires an out-of-tree call to render differently: {:?}",
        calls[0]
    );
    assert!(
        restored.interrupted.is_none(),
        "the user declined; a resumed session must not call that an interruption"
    );
}

/// ADR-0010 D4 end to end: a session with a call in flight is resumed, and
/// the interruption reaches what the model is shown.
#[tokio::test]
async fn an_interrupted_call_reaches_the_model_on_the_next_turn() {
    println!("== a resumed session with a call in flight ==");
    let scratch = Scratch::new("tcl-resumed");
    // A `Started` with nothing closing it is what a killed process leaves.
    {
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let tree = Tree::new("tcl-resumed-tree");
        let working = WorkingDirectory::at(tree.project()).expect("resolves");
        let target = working.classify("src/main.rs");
        let invocation =
            zaru_cli::tools::Invocation::on_path(ToolName::FsWrite, &target).expect("a path tool");
        let decision =
            zaru_cli::tools::Decision::assess(Mode::Yolo, &invocation, &Nothing, &Nothing);
        transcript
            .record(&Record::ToolCall(zaru_cli::session::ToolCall::started(
                decision.entry(),
            )))
            .expect("the started line is written");
    }

    let restored = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    let interrupted = restored
        .interrupted
        .as_ref()
        .expect("a started call with nothing closing it is the interruption");
    println!("  interrupted: {:?}", interrupted.call.line);

    let clock = Ticking::default();
    let policy = Policy::default();
    let mut sink = Printing::default();
    let model = Provider(Mutex::new(
        [ModelResponse::Text {
            text: String::from("that write did not finish and I will not repeat it"),
            tokens: TokenUsage::default(),
        }]
        .into(),
    ));
    let tree = Tree::new("tcl-resumed-second");
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;

    // The conversion `zaru-cli` owns, into the datum `zaru-core` carries.
    let carried: Interruption = interrupted.for_the_model();

    {
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Ask,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer: None,
            verdicts: &membrane,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            overflow: &mut overflow,
            transcript: &mut transcript,
            writes: &unbuilt,
            search: &unbuilt,
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, NeverIterates>(
            2,
            Start::Resumed(&carried),
            ToolCallCeiling::new(2).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("no port failed");
    }

    let shown = policy.0.lock().expect("poisoned").clone();
    println!("  the model was shown: {shown:?}");
    assert_eq!(shown.len(), 1);
    assert!(
        shown[0].contains(&interrupted.call.line),
        "ADR-0010 D4: the model is told it did not complete. The interrupted call's own line \
         must reach what the model sees, and what reached it was {:?}",
        shown[0]
    );
}

/// ADR-0004's seam, from outside: a denying membrane refuses at `yolo`, and
/// the denial presents as an expected failure rather than an error.
#[tokio::test]
async fn a_denying_membrane_refuses_at_yolo_and_presents_as_an_expected_failure() {
    println!("== a denying membrane at yolo ==");
    struct Denying;
    impl Verdicts for Denying {
        fn verdict(&self, _invocation: &zaru_cli::tools::Invocation<'_>) -> Verdict {
            Verdict::Denied {
                code: String::from("PATH_NOT_ALLOWED"),
                reason: String::from("not in [\"./**\"]"),
            }
        }
    }

    let tree = Tree::new("tcl-seal");
    let scratch = Scratch::new("tcl-seal");
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let clock = Ticking::default();
    let policy = Policy::default();
    let mut sink = Printing::default();
    let denying = Denying;

    let model = Provider(Mutex::new(
        [
            ModelResponse::Calls {
                calls: vec![ToolRequest {
                    id: String::from("c1"),
                    name: String::from("fs.read"),
                    arguments: String::from("src/main.rs"),
                }],
                tokens: TokenUsage::default(),
            },
            ModelResponse::Text {
                text: String::from("the membrane refused"),
                tokens: TokenUsage::default(),
            },
        ]
        .into(),
    ));

    {
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Yolo,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer: None,
            verdicts: &denying,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            overflow: &mut overflow,
            transcript: &mut transcript,
            writes: &unbuilt,
            search: &unbuilt,
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, NeverIterates>(
            1,
            Start::Task("read the entry point"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("a denied verdict is not a port failure");
    }

    assert!(
        sink.0
            .iter()
            .any(|event| matches!(event, Event::ToolRefused { .. })),
        "ADR-0011 D3: a user in `yolo` inside a membrane is still inside the membrane"
    );

    let denied = Verdict::Denied {
        code: String::from("PATH_NOT_ALLOWED"),
        reason: String::from("not in [\"./**\"]"),
    };
    let classified = denied.as_expected_failure().expect("a denial is a failure");
    let presentation = Presentation::of(&classified);
    println!("  presented as: {presentation:?}");
    assert_eq!(presentation.class, Class::Expected);
    assert!(
        !presentation.class.is_the_error_register(),
        "ADR-0016 D1: a denied verdict is not an error -- this is the membrane working"
    );
}

/// A type that satisfies the loop's inner-loop parameter and is never
/// supplied. `None::<&NeverIterates>` is what a project with no manifest
/// hands in, and this is the type that says so at the call site.
struct NeverIterates;

impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        unreachable!("no check here declares validators")
    }
}
