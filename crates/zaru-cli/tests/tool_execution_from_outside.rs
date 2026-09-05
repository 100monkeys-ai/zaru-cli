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
use zaru_cli::process::CommandLine;
use zaru_cli::redaction::HeldSecrets;
use zaru_cli::session::{Phase, Record, SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::terminal::driver::Pending;
use zaru_cli::tools::{
    Captured, ConfirmFailure, Executor, Fetch, Mode, NoMembrane, OutputBudget, Question,
    SessionOverflow, Subprocess, ToolName, Verdict, Verdicts, WorkingDirectory,
};
use zaru_core::iteration::{
    Clock, ContextPolicy, ContextRefusal, Interruption, PortFailure, Prompt, Turn,
};
use zaru_core::redaction::{Redacted, Redactor};
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
        Ok(Prompt::new(Redacted::by(&NothingHeld, &rendered)))
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

struct Declining;
impl zaru_cli::tools::Confirm for Declining {
    fn confirm(&self, question: &Question) -> Result<bool, ConfirmFailure> {
        println!("  the user is asked: {:?} -> no", question.statement);
        Ok(false)
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
                    arguments: serde_json::json!({ "path": "src/main.rs" }).to_string(),
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
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &HeldSecrets::none(),
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("read the entry point"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
                redactor: &HeldSecrets::none(),
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
                    arguments: serde_json::json!({ "path": "../elsewhere/secret" }).to_string(),
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
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &HeldSecrets::none(),
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("read the secret"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
                redactor: &HeldSecrets::none(),
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

/// **ADR-0010 D4, end to end, through the carrier a resumed session hands the
/// shell.**
///
/// D4: "An interrupted tool call is recorded as `Interrupted` **and the model
/// is told it did not complete**." Every part of that is the product's here:
/// `session::resume` derives the interruption, `terminal::driver::Pending`
/// carries it through `Interrupted::for_the_model`, and the loop turns it into
/// the `Turn::Resumed` the policy is handed. Nothing in this check builds an
/// `Interruption` by hand, which is what it did until 2026-09-05 — and while
/// it did, the only thing it showed was that *an outside caller* could do it.
///
/// Three arms, and the third is the one that discriminates:
///
/// 1. The first turn of the resumed session is `Resumed` and carries that
///    call's own line.
/// 2. The turn after it is `Initial` and carries the task. An interruption is
///    told once.
/// 3. **Nothing was re-executed.** The resumed turn requested no tool, the
///    transcript gained no `tool_call` record, and the file the interrupted
///    `fs.write` named is not on disk. Its accepting sibling is the last turn,
///    where a model that *does* ask for that write gets it — so the arm is not
///    a check that a file never appears (library verification lessons §4).
///
/// The name this check had until 2026-09-05 was
/// `an_interrupted_call_reaches_the_model_on_the_next_turn`, which stated a
/// weaker rule than the one held here.
#[tokio::test]
async fn a_resumed_session_tells_the_model_once_and_re_executes_nothing() {
    println!("== a resumed session with a call in flight ==");
    let scratch = Scratch::new("tcl-resumed");
    let tree = Tree::new("tcl-resumed-tree");
    let working = WorkingDirectory::at(tree.project()).expect("resolves");
    // A path with nothing at it, so "was it re-executed" is a question the
    // filesystem can answer. `src/main.rs` is staged by `Tree` and would be
    // there whatever happened.
    let written = tree.project().join("src").join("fresh.rs");
    assert!(
        !written.exists(),
        "staging: the file the interrupted call names is already on disk, so the arm below could \
         not fail"
    );

    // A `Started` with nothing closing it is what a killed process leaves.
    {
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        let target = working.classify("src/fresh.rs");
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
    let records_before = records_in(&scratch.session.transcript_path());

    let restored = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    let line = restored
        .interrupted
        .as_ref()
        .expect("a started call with nothing closing it is the interruption")
        .call
        .line
        .clone();
    println!("  interrupted: {line:?}");

    // The product's carrier, not this check's. `Pending::of` is the one thing
    // that turns what a resume derived into what a turn starts with.
    let mut pending = Pending::of(&restored, &HeldSecrets::none());
    let carried: Interruption = pending
        .tell_once()
        .expect("a resumed session owes the model the call that never completed");

    let clock = Ticking::default();
    let policy = Policy::default();
    let mut sink = Printing::default();
    let model = Provider(Mutex::new(
        [
            // The resumed turn: the model answers and asks for nothing.
            ModelResponse::Text {
                text: String::from("that write did not finish and I will not repeat it"),
                tokens: TokenUsage::default(),
            },
            // The turn the user asked for.
            ModelResponse::Text {
                text: String::from("nothing else to do"),
                tokens: TokenUsage::default(),
            },
            // The accepting sibling: a model that does ask for the write.
            ModelResponse::Calls {
                calls: vec![ToolRequest {
                    id: String::from("c1"),
                    name: String::from("fs.write"),
                    arguments: serde_json::json!({
                        "path": "src/fresh.rs",
                        "contents": "// written on purpose\n",
                    })
                    .to_string(),
                }],
                tokens: TokenUsage::default(),
            },
            ModelResponse::Text {
                text: String::from("written"),
                tokens: TokenUsage::default(),
            },
        ]
        .into(),
    ));
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let nothing = Nothing;
    let unbuilt = Unbuilt;
    let membrane = NoMembrane;

    {
        let mut executor = Executor {
            working_directory: &working,
            // `yolo`, so the last turn's write is not refused for want of a
            // confirmer. It is the same surface for all three turns, which is
            // what makes the third one evidence about the first two.
            mode: Mode::Yolo,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer: None,
            verdicts: &membrane,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &HeldSecrets::none(),
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        // Three turns over one surface, written out rather than folded into a
        // helper: what makes the third one evidence about the first two is
        // that they are the same executor, the same policy and the same model,
        // and a helper would hide exactly that.
        macro_rules! turn {
            ($n:expr, $start:expr) => {
                run::<_, _, _, _, _, NeverIterates>(
                    $n,
                    $start,
                    ToolCallCeiling::new(3).expect("a usable ceiling"),
                    ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
                    Ports {
                        model: &model,
                        tools: &mut executor,
                        context: &policy,
                        clock: &clock,
                        redactor: &HeldSecrets::none(),
                    },
                    None,
                    &mut [&mut sink],
                )
                .await
                .expect("no port failed")
            };
        }

        // Turn 2 of this session: the one a resumed shell owes the model.
        turn!(2, Start::Resumed(&carried));

        assert!(
            pending.tell_once().is_none(),
            "the interruption is still owed after the resumed turn, so the next turn would be \
             told it again; an interruption is told once"
        );

        // Turn 3: the line the user typed.
        turn!(3, Start::Task("what else is left"));

        // Turn 4, the accepting sibling: the same surface, asked for the write.
        turn!(4, Start::Task("write the file after all"));
    }

    let shown = policy.0.lock().expect("poisoned").clone();
    println!("  the model was shown: {shown:?}");
    assert_eq!(shown.len(), 3, "three turns were driven");
    assert!(
        shown[0].starts_with("[resumed]") && shown[0].contains(&line),
        "ADR-0010 D4: the model is told it did not complete. The interrupted call's own line \
         must reach what the model sees, and what reached it was {:?}",
        shown[0]
    );
    assert!(
        shown[1].starts_with("[initial]") && shown[1].contains("what else is left"),
        "the turn after a resumed one is the user's own, and an interruption is told once; what \
         reached the model was {:?}",
        shown[1]
    );

    // Arm 3. Read off the disk and off the loop's own events, neither of
    // which travels through `Pending` (library verification lessons §11).
    let requested: Vec<&Event> = sink
        .0
        .iter()
        .take_while(|event| !matches!(event, Event::TurnStarted { n: 3, .. }))
        .filter(|event| matches!(event, Event::ToolRequested { .. }))
        .collect();
    assert!(
        requested.is_empty(),
        "the resumed turn requested {} tool call(s); a resume never re-runs one: {requested:?}",
        requested.len()
    );
    assert_eq!(
        records_in(&scratch.session.transcript_path())
            .iter()
            .filter(|line| line.contains("\"tool_call\""))
            .count(),
        records_before
            .iter()
            .filter(|line| line.contains("\"tool_call\""))
            .count()
            + 2,
        "the interrupted call was re-executed, or the sibling's write was not recorded: the \
         transcript's tool-call records are {:?}",
        records_in(&scratch.session.transcript_path())
    );
    assert_eq!(
        std::fs::read_to_string(&written).expect("the sibling's write reached the disk"),
        "// written on purpose\n",
        "the accepting sibling did not write the file, so the arm above asserted that a file \
         nothing could have created was absent"
    );
}

/// **A resumed session whose last event is an *iteration*'s owes the model
/// nothing, and this pins that answer without giving it one.**
///
/// `process-async` measured, from outside both crates and over the product's
/// own loop, that a `Ctrl-C` while a declared validator's command was running
/// leaves the transcript holding exactly `iteration_started`,
/// `candidate_generated`, `execution_completed` — that is ADR-0010 D2's "at
/// most the event in flight" honoured exactly, and it means **a resume can
/// name the `cmd.run` that did not finish and cannot name the iteration that
/// did not finish**. Whether an interrupted iteration is derivable at all is
/// an open question on `operations/adr-status-questions`, and giving a
/// validator a started-and-completed pair would add a producer to D2's list,
/// which is ADR-0010's decision and not an implementer's.
///
/// So this check **answers nothing**. It holds what is true today —
/// `session::resume` scans `Record::ToolCall` and nothing else, so a session
/// whose last loop event is `execution_completed` reports no interruption and
/// its resumed first turn is `Initial`, exactly as a clean resume's — which is
/// what that question's own "checkable by" asks for. Deciding it either way
/// reddens this check rather than passing unnoticed.
///
/// The accepting sibling is the same transcript with a lone tool-call
/// `Started` appended: that one does owe the model a telling, so the check
/// cannot pass against a carrier that owes nothing for everything.
#[test]
fn a_session_interrupted_inside_an_iteration_owes_the_model_nothing() {
    println!("== a transcript that ends inside an iteration ==");
    let scratch = Scratch::new("tcl-iteration");
    let tree = Tree::new("tcl-iteration-tree");
    let working = WorkingDirectory::at(tree.project()).expect("resolves");

    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    for event in [
        zaru_core::iteration::Event::IterationStarted { n: 1, of: 3 },
        zaru_core::iteration::Event::CandidateGenerated {
            tokens: 42,
            elapsed: Duration::from_millis(7),
        },
        zaru_core::iteration::Event::ExecutionCompleted {
            exit_code: 0,
            stdout_bytes: 3,
            stderr_bytes: 0,
            elapsed: Duration::from_millis(11),
        },
    ] {
        transcript
            .record(&Record::Loop(event))
            .expect("a loop record is appended");
    }

    let restored = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    assert!(
        !restored.tail.is_empty(),
        "staging: the transcript holds no records, so nothing below was measured"
    );
    assert_eq!(
        restored.interrupted, None,
        "a session whose last record is an iteration's own event reported an interrupted tool \
         call; D4's `Interrupted` is a tool call's marker and the loop's events are not that shape"
    );
    assert!(
        !Pending::of(&restored, &HeldSecrets::none()).is_owed(),
        "a session interrupted inside an iteration owes the model a telling, which would be a \
         telling about a call the transcript does not name"
    );

    // The accepting sibling, on the same transcript: a tool call in flight is
    // still derived, so the carrier is not one that owes nothing for
    // everything.
    let target = working.classify("src/main.rs");
    let invocation =
        zaru_cli::tools::Invocation::on_path(ToolName::FsRead, &target).expect("a path tool");
    let decision = zaru_cli::tools::Decision::assess(Mode::Yolo, &invocation, &Nothing, &Nothing);
    transcript
        .record(&Record::ToolCall(zaru_cli::session::ToolCall::started(
            decision.entry(),
        )))
        .expect("the started line is written");
    let after = zaru_cli::session::resume(scratch.session.directory(), 32).expect("resumes");
    assert!(
        Pending::of(&after, &HeldSecrets::none()).is_owed(),
        "a tool call left in flight after the loop's events owes the model nothing, so the arm \
         above holds for a carrier that never owes anything"
    );
}

/// Every complete line of a transcript, off the filesystem.
fn records_in(path: &std::path::PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
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
                    arguments: serde_json::json!({ "path": "src/main.rs" }).to_string(),
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
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &HeldSecrets::none(),
            subprocess: &unbuilt,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("read the entry point"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
                redactor: &HeldSecrets::none(),
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

/// A redactor holding nothing, which is therefore the identity.
///
/// Every outside caller has to supply one, because a `Prompt` can only be
/// built from text that has passed [ADR-0008] clause 6's port — which is the
/// whole point of that type. It is declared here rather than shared because
/// an integration test cannot see another crate's test tree and [ADR-0003] D8
/// forbids the dependency that would let it, the same cost `zaru-cli`'s
/// Nuclear Notes fixture server already pays.
///
/// Holding nothing is also the **discriminating** arm: a check asserting that
/// a value is absent from a prompt is worthless unless the same run with
/// nothing held carries that value through byte for byte.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}
