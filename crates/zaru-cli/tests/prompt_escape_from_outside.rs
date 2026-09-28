// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What `Esc` and `Ctrl-C` do at a permission question asked during a turn.
//!
//! # The defect these checks were written against
//!
//! Until 2026-09-28 a question asked during a turn was answered inside a call
//! that read the terminal itself and slept between keys, on the turn's only
//! thread. `Ctrl-C` was ignored there. So a person who wanted to stop a turn
//! had to say no to every call the model asked for next, and while a question
//! stood nothing else on that thread ran: not the signal listener, not the
//! watch for a terminal that has gone.
//!
//! The rule now, at every question a turn asks (a tool call, a fetch, the
//! validators):
//!
//! - `Esc` says no to this one question, and the turn goes on. The model is
//!   told the person said no.
//! - `Ctrl-C` stops the whole turn. The call does not run, no other call
//!   runs, the transcript holds the call as started and never finished, and
//!   the next turn's model is told the call did not finish. The session stays
//!   open and the composer takes the next line.
//!
//! # How the turns are driven
//!
//! Through the product's own pieces: the race that reads the terminal beside
//! a turn, the pane and its confirmer, the tool-call loop and the real
//! executor over a scratch working directory. The keys come from a scripted
//! source and the model is a recording double. No check here calls a provider
//! or serves one on a socket.

use core::time::Duration;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::Mutex;
use zaru_cli::compose::{Facts, Records, SessionContext, boundary};
use zaru_cli::process::CommandLine;
use zaru_cli::redaction::HeldSecrets;
use zaru_cli::session::{Phase, Record, SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::terminal::driver::{Pane, PaneConfirm, Raced, Restore, Surface, race};
use zaru_cli::terminal::source::{Pace, Source};
use zaru_cli::tools::{
    Captured, Executor, Fetch, Mode, NoMembrane, OutputBudget, SessionOverflow, Subprocess,
    WorkingDirectory,
};
use zaru_core::conversation::Message;
use zaru_core::iteration::{Clock, PortFailure};
use zaru_core::tool_call::{
    Capabilities, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome, Ports, Start,
    TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};
use zaru_tui::shell::{Action, Input, Key, Palette, Shell, Status};

// ------------------------------------------------------------ the scratch

struct Scratch {
    base: std::path::PathBuf,
    session: zaru_cli::session::Session,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("resolves")
            .join(format!(
                "prompt-escape-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(base.join("project")).expect("staging");
        std::fs::create_dir_all(base.join("sessions")).expect("staging");
        std::fs::create_dir_all(base.join("home")).expect("staging");
        let store = SessionStore::open(base.join("sessions")).expect("staging: the store");
        let session = store
            .start(SessionId::mint(&SystemWallClock).expect("an id"))
            .expect("staging: the session");
        Self { base, session }
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }

    fn home(&self) -> std::path::PathBuf {
        self.base.join("home")
    }

    fn records(&self) -> Vec<Record> {
        zaru_cli::session::resume(self.session.directory(), usize::MAX)
            .expect("the session resumes")
            .records
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// ------------------------------------------------------------ the doubles

#[derive(Debug, Default)]
struct Still;
impl Clock for Still {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

static STILL: Still = Still;

/// A process runner and a fetcher that must never be reached: every call in
/// these checks is stopped or refused before it acts.
struct MustNotAct;
impl Subprocess for MustNotAct {
    async fn run(&self, line: &CommandLine) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!(
            "cmd.run acted on a call the person did not allow: {line:?}"
        )))
    }
}
impl Fetch for MustNotAct {
    async fn retrieve(
        &self,
        url: &zaru_cli::web::RequestedUrl,
        _followed: usize,
    ) -> Result<zaru_cli::tools::Retrieved, PortFailure> {
        Err(PortFailure::new(format!(
            "web.fetch acted on a call the person did not allow: {}",
            url.host()
        )))
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

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        Err(PortFailure::new("no inner loop here"))
    }
}

/// A beat that never sleeps: the races here are ended by the keys.
#[derive(Debug, Default)]
struct Instant;
impl Pace for Instant {
    fn wait(&self) {}

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        tokio::task::yield_now()
    }
}

/// Answers from a script and keeps what each request carried.
struct Scripted {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    sent: Mutex<Vec<(Vec<Message>, Vec<Message>)>>,
}

impl Scripted {
    fn answering(script: impl IntoIterator<Item = ModelResponse>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Each request's history (the earlier turns) and this turn's messages.
    fn sent(&self) -> Vec<(Vec<Message>, Vec<Message>)> {
        self.sent.lock().expect("poisoned").clone()
    }
}

impl Model for Scripted {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        self.sent
            .lock()
            .expect("poisoned")
            .push((request.prompt.history().to_vec(), request.turn.to_vec()));
        self.script
            .lock()
            .expect("poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

fn calls(id: &str, name: &str, arguments: &str) -> ModelResponse {
    ModelResponse::Calls {
        calls: vec![ToolRequest {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }],
        text: String::new(),
        echo: None,
        tokens: TokenUsage::default(),
    }
}

fn answer(text: &str) -> ModelResponse {
    ModelResponse::Text {
        text: text.to_owned(),
        echo: None,
        tokens: TokenUsage::default(),
    }
}

// ------------------------------------------------------------ the terminal

/// A terminal a check owns: a buffer, and every frame painted into it.
struct Recorded {
    terminal: Terminal<TestBackend>,
    frames: Vec<Vec<String>>,
}

impl Recorded {
    fn of() -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(100, 30)).expect("test terminal"),
            frames: Vec::new(),
        }
    }

    /// Every frame, each read as the words its rows wrap.
    fn read(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|rows| {
                rows.iter()
                    .map(|row| row.trim_end())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
}

impl Restore for Recorded {
    fn restore(&mut self) {}
}

impl Surface for Recorded {
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()> {
        let Ok(_) = self
            .terminal
            .draw(|frame| shell.render(frame, frame.area(), Palette::Monochrome));
        let buffer = self.terminal.backend().buffer();
        self.frames.push(
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect(),
        );
        Ok(())
    }

    fn area(&self) -> std::io::Result<ratatui::layout::Rect> {
        let Ok(size) = self.terminal.size();
        Ok(ratatui::layout::Rect::new(0, 0, size.width, size.height))
    }
}

fn press(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn ctrl_c() -> Input {
    Input {
        key: Key::Char('c'),
        ctrl: true,
        alt: false,
        shift: false,
    }
}

// ------------------------------------------------------------ one turn

fn facts() -> Facts {
    Facts {
        directory: Some("/work".to_owned()),
        system: "linux".to_owned(),
        date: "2026-09-28".to_owned(),
        tools: vec!["fs.read".to_owned()],
        mode: None,
    }
}

fn opened() -> SessionContext {
    SessionContext::opened(
        zaru_cli::compose::prefix_for(None, &facts()),
        zaru_cli::terminal::open::context_shape_of(None),
    )
}

/// What one turn left behind.
struct Turned {
    /// Whether the race came back with the turn finished or stopped.
    ended: &'static str,
    /// Every frame the pane painted, read as words.
    frames: Vec<String>,
    /// Whether a question was left standing on the shell afterwards.
    question_left: bool,
    /// What the composer did with a line typed after the turn.
    next_line: Action,
}

/// Run one turn of `session` in `ask` mode, with the pane's own confirmer
/// asking every question and `keys` as everything the person presses.
///
/// This is what `terminal::driver::run_a_turn` does, less the provider client
/// it needs a key for: the race, the pane, the confirmer, the tool-call loop
/// over the real executor, and the transcript. Then layer 6 is rebuilt from
/// the transcript, as the pump's `after` does for a turn that ended either way.
async fn a_turn_at_a_question(
    scratch: &Scratch,
    context: &mut SessionContext,
    model: &Scripted,
    keys: Vec<Input>,
    task: &str,
) -> Turned {
    let held = HeldSecrets::none();
    let working = WorkingDirectory::at(scratch.project()).expect("resolves");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    transcript
        .record(&boundary::spoken_by_the_user(&held, 1, task))
        .expect("the person's message is recorded");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let no_grants = zaru_cli::tools::grants::SessionGrants::none();
    let mut records = Records::appending_to(scratch.session.transcript_path()).expect("opens");

    let mut shell = Shell::open(Status::new("bare", scratch.session.id().to_string()));
    let mut surface = Recorded::of();
    let source = Source::scripted(keys);
    let pace = Instant;
    let mut now = Duration::ZERO;
    let trie = zaru_cli::terminal::NotesTrie::nothing_cached("zaru");
    let paths = zaru_cli::terminal::ProjectPaths::under(None);

    let ended = {
        let pane = Mutex::new(Pane::during(&mut shell, &mut surface, &STILL));
        let confirm = PaneConfirm::over(&pane, &source, &pace);
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Ask,
            allowlist: &Nothing,
            destructive: &Nothing,
            session_grants: &no_grants,
            confirmer: Some(&confirm),
            verdicts: &NoMembrane,
            budget: OutputBudget::new(32_768).expect("a budget"),
            preview_budget: OutputBudget::new(4096).expect("a budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &held,
            subprocess: &MustNotAct,
            fetch: &MustNotAct,
            projected: &zaru_cli::tools::NoProjection,
            declared: zaru_cli::tools::descriptor_set(),
        };
        let policy = context.policy(&held, false);
        let mut sinks: [&mut dyn EventSink; 1] = [&mut records];
        let raced = race(
            &pane,
            &source,
            &pace,
            &trie,
            &zaru_cli::terminal::Vocabulary,
            &paths,
            &mut now,
            None,
            None,
            run::<_, _, _, _, _, NeverIterates>(
                1,
                Start::Task(task),
                ToolCallCeiling::unlimited(),
                ToolCalling::required(model, "scripted").expect("it can"),
                Ports {
                    model,
                    tools: &mut executor,
                    context: &policy,
                    clock: &STILL,
                    redactor: &held,
                },
                None,
                &mut sinks,
            ),
        )
        .await;
        match raced {
            Raced::Ran(Ok(Outcome::Answered { .. })) => "the turn finished",
            Raced::Ran(Ok(other)) => panic!("the turn ended some other way: {other:?}"),
            Raced::Ran(Err(failure)) => panic!("a port failed: {failure:?}"),
            Raced::Interrupted => "the turn was stopped",
            Raced::SourceEnded => "the keys ran out",
        }
    };
    assert!(
        records.first_failure().is_none(),
        "the transcript refused a record"
    );
    boundary::rebuilt_from_the_transcript(context, &scratch.session)
        .expect("the transcript reads back");

    let question_left = shell.asking().is_some();
    // The next thing the person types, after the turn: it must be a task for
    // the next turn and not an answer to a question nobody is asking.
    let mut next_line = Action::Idle;
    for ch in "next task".chars() {
        next_line = shell.key(
            press(Key::Char(ch)),
            ratatui::layout::Rect::new(0, 0, 100, 20),
            now,
            &trie,
            &zaru_cli::terminal::Vocabulary,
            &paths,
        );
    }
    if next_line == Action::Idle {
        next_line = shell.key(
            press(Key::Enter),
            ratatui::layout::Rect::new(0, 0, 100, 20),
            now,
            &trie,
            &zaru_cli::terminal::Vocabulary,
            &paths,
        );
    }
    Turned {
        ended,
        frames: surface.read(),
        question_left,
        next_line,
    }
}

/// The phases the transcript's `tool_call` records hold for `line`, in order.
fn phases(scratch: &Scratch, line: &str) -> Vec<Phase> {
    scratch
        .records()
        .into_iter()
        .filter_map(|record| match record {
            Record::ToolCall(call) if call.line.contains(line) => Some(call.phase),
            _ => None,
        })
        .collect()
}

/// One kind of question a turn asks, with the call that raises it.
struct Kind {
    label: &'static str,
    tool: &'static str,
    arguments: &'static str,
    /// Text the transcript's line for the call holds.
    line: &'static str,
    /// What must not exist afterwards, if the call would have made something.
    made: Option<&'static str>,
}

const KINDS: [Kind; 3] = [
    Kind {
        label: "a command",
        tool: "cmd.run",
        arguments: r#"{"command":"touch COMMAND-RAN"}"#,
        line: "touch COMMAND-RAN",
        made: Some("COMMAND-RAN"),
    },
    Kind {
        label: "a file write",
        tool: "fs.write",
        arguments: r#"{"path":"WRITTEN.txt","contents":"x\n"}"#,
        line: "WRITTEN.txt",
        made: Some("WRITTEN.txt"),
    },
    Kind {
        label: "a fetch",
        tool: "web.fetch",
        arguments: r#"{"url":"https://example.com/page"}"#,
        line: "https://example.com/page",
        made: None,
    },
];

// ------------------------------------------------------------ the checks

/// `Esc` at each kind of question says no to that call, and the turn goes on
/// with the model told the person said no.
///
/// This was already true before 2026-09-28 and is pinned here beside the
/// `Ctrl-C` check below, so a change that made the two keys the same fails
/// one of them.
#[tokio::test]
async fn esc_at_each_question_says_no_to_that_call_and_the_turn_goes_on() {
    for kind in &KINDS {
        let scratch = Scratch::new("esc");
        let mut context = opened();
        let model = Scripted::answering([
            calls("call_1", kind.tool, kind.arguments),
            answer("I will not do that, then."),
        ]);
        let turned = a_turn_at_a_question(
            &scratch,
            &mut context,
            &model,
            vec![press(Key::Esc)],
            "do the thing",
        )
        .await;

        assert_eq!(
            turned.ended, "the turn finished",
            "Esc at {} did not let the turn go on",
            kind.label
        );
        let sent = model.sent();
        assert_eq!(
            sent.len(),
            2,
            "after Esc at {} the model was not asked again",
            kind.label
        );
        let told = sent[1].1.iter().any(|message| {
            matches!(message, Message::Tool { id, content, .. }
                if id == "call_1" && content.contains("did not permit it"))
        });
        assert!(
            told,
            "after Esc at {} the model was not told the person said no: {:?}",
            kind.label, sent[1].1
        );
        assert_eq!(
            phases(&scratch, kind.line),
            vec![Phase::Started, Phase::Refused],
            "Esc at {} is not recorded as a call asked about and refused",
            kind.label
        );
        if let Some(made) = kind.made {
            assert!(
                !scratch.project().join(made).exists(),
                "Esc at {} and the call ran anyway",
                kind.label
            );
        }
        assert!(!turned.question_left, "a question was left standing");
    }
}

/// `Ctrl-C` at each kind of question stops the whole turn.
///
/// The call does not run, the model is not asked again, the transcript holds
/// the call as started and never finished, the session is left ready for the
/// next line, and the next turn's model is told the call did not finish.
/// Watched red on `e5b9240`, where the key was ignored at a question and the
/// turn went on to ask the model again.
#[tokio::test]
async fn ctrl_c_at_each_question_stops_the_turn_and_the_next_turn_is_told() {
    for kind in &KINDS {
        let scratch = Scratch::new("ctrl-c");
        let mut context = opened();
        let model = Scripted::answering([
            calls("call_1", kind.tool, kind.arguments),
            answer("this answer must never be asked for"),
        ]);
        let turned = a_turn_at_a_question(
            &scratch,
            &mut context,
            &model,
            vec![press(Key::Char('q')), ctrl_c(), press(Key::Char('y'))],
            "do the thing",
        )
        .await;

        assert_eq!(
            turned.ended, "the turn was stopped",
            "Ctrl-C at {} did not stop the turn",
            kind.label
        );
        assert_eq!(
            model.sent().len(),
            1,
            "after Ctrl-C at {} the model was asked again",
            kind.label
        );
        assert_eq!(
            phases(&scratch, kind.line),
            vec![Phase::Started],
            "Ctrl-C at {} is not recorded as a call started and never finished",
            kind.label
        );
        if let Some(made) = kind.made {
            assert!(
                !scratch.project().join(made).exists(),
                "Ctrl-C at {} and the call ran anyway",
                kind.label
            );
        }
        assert!(
            !turned.question_left,
            "Ctrl-C at {} left the question standing, so the composer takes nothing",
            kind.label
        );
        assert_eq!(
            turned.next_line,
            Action::Task("next task".to_owned()),
            "after Ctrl-C at {} the composer did not take the next line",
            kind.label
        );

        // The next turn is told, in the conversation, that the call did not
        // finish: the record `turn-memory` defined for a call a turn left
        // without a result.
        let next = Scripted::answering([answer("Understood.")]);
        let history = {
            let held = HeldSecrets::none();
            let mut transcript =
                Transcript::append_to(scratch.session.transcript_path()).expect("opens");
            transcript
                .record(&boundary::spoken_by_the_user(&held, 2, "what happened?"))
                .expect("recorded");
            let mut records =
                Records::appending_to(scratch.session.transcript_path()).expect("opens");
            let working = WorkingDirectory::at(scratch.project()).expect("resolves");
            let mut overflow = SessionOverflow::in_session(scratch.session.directory());
            let no_grants = zaru_cli::tools::grants::SessionGrants::none();
            let mut executor = Executor {
                working_directory: &working,
                mode: Mode::Ask,
                allowlist: &Nothing,
                destructive: &Nothing,
                session_grants: &no_grants,
                confirmer: None,
                verdicts: &NoMembrane,
                budget: OutputBudget::new(32_768).expect("a budget"),
                preview_budget: OutputBudget::new(4096).expect("a budget"),
                search_ceiling: zaru_cli::cli::layers::search_ceiling(),
                overflow: &mut overflow,
                transcript: &mut transcript,
                redactor: &held,
                subprocess: &MustNotAct,
                fetch: &MustNotAct,
                projected: &zaru_cli::tools::NoProjection,
                declared: zaru_cli::tools::descriptor_set(),
            };
            let policy = context.policy(&held, false);
            let mut sinks: [&mut dyn EventSink; 1] = [&mut records];
            run::<_, _, _, _, _, NeverIterates>(
                2,
                Start::Task("what happened?"),
                ToolCallCeiling::unlimited(),
                ToolCalling::required(&next, "scripted").expect("it can"),
                Ports {
                    model: &next,
                    tools: &mut executor,
                    context: &policy,
                    clock: &STILL,
                    redactor: &held,
                },
                None,
                &mut sinks,
            )
            .await
            .expect("no port failed");
            next.sent()[0].0.clone()
        };
        let closed = history.iter().any(|message| {
            matches!(message, Message::Tool { id, content, failed: true, .. }
                if id == "call_1" && content == zaru_cli::compose::prose::CALL_DID_NOT_COMPLETE)
        });
        assert!(
            closed,
            "the turn after Ctrl-C at {} was not told the call did not finish: {history:?}",
            kind.label
        );
    }
}

/// The question's own line says what `Esc` does and what `Ctrl-C` does, in
/// words, at every kind of question a turn asks.
#[tokio::test]
async fn the_question_says_what_esc_and_ctrl_c_do() {
    for kind in &KINDS {
        let scratch = Scratch::new("words");
        let mut context = opened();
        let model =
            Scripted::answering([calls("call_1", kind.tool, kind.arguments), answer("fine")]);
        let turned = a_turn_at_a_question(
            &scratch,
            &mut context,
            &model,
            vec![press(Key::Esc)],
            "do the thing",
        )
        .await;
        let asked = turned
            .frames
            .iter()
            .find(|frame| frame.contains("Allow "))
            .unwrap_or_else(|| panic!("no frame showed the question for {}", kind.label));
        for words in ["esc says no to this call", "ctrl-c stops the turn"] {
            assert!(
                asked.contains(words),
                "the question for {} does not say {words:?}: {asked}",
                kind.label
            );
        }
    }
}

/// The question about a project's validators, asked at the start of a turn:
/// `Esc` says no and nothing is approved; `Ctrl-C` stops the turn and nothing
/// is approved; and its line says both.
#[tokio::test]
async fn the_validators_question_takes_esc_and_ctrl_c_the_same_way() {
    use zaru_cli::validators::approval::{Approvals, NotApproved, gate_in_a_turn};
    use zaru_core::iteration::validator::{Declared, Expect, Name, Run};

    let declared = [Declared::new(
        Name::new("plant").expect("a name"),
        Run::new("touch VALIDATOR-RAN").expect("a command"),
        Expect::ExitZero,
    )];
    for (label, keys, stops) in [
        ("Esc", vec![press(Key::Esc)], false),
        ("Ctrl-C", vec![ctrl_c(), press(Key::Char('y'))], true),
    ] {
        let scratch = Scratch::new("validators");
        let approvals = Approvals::under(&scratch.home());
        let mut shell = Shell::open(Status::new("bare", scratch.session.id().to_string()));
        let mut surface = Recorded::of();
        let source = Source::scripted(keys);
        let pace = Instant;
        let mut now = Duration::ZERO;
        let trie = zaru_cli::terminal::NotesTrie::nothing_cached("zaru");
        let paths = zaru_cli::terminal::ProjectPaths::under(None);
        let raced = {
            let pane = Mutex::new(Pane::during(&mut shell, &mut surface, &STILL));
            let confirm = PaneConfirm::over(&pane, &source, &pace);
            race(
                &pane,
                &source,
                &pace,
                &trie,
                &zaru_cli::terminal::Vocabulary,
                &paths,
                &mut now,
                None,
                None,
                gate_in_a_turn(
                    &approvals,
                    &scratch.project(),
                    &declared,
                    Some(&confirm),
                    "2026-09-28",
                ),
            )
            .await
        };
        match (stops, raced) {
            (false, Raced::Ran(Err(NotApproved::Declined))) | (true, Raced::Interrupted) => {}
            (_, other) => panic!("{label} at the validators question ended as {other:?}"),
        }
        assert!(
            approvals.latest().expect("readable").is_empty(),
            "{label} at the validators question approved them"
        );
        assert!(
            shell.asking().is_none(),
            "{label} left the validators question standing"
        );
        let asked = surface
            .read()
            .into_iter()
            .find(|frame| frame.contains("Allow these commands"))
            .expect("the question was painted");
        for words in ["esc says no", "ctrl-c stops the turn"] {
            assert!(
                asked.contains(words),
                "the validators question does not say {words:?}: {asked}"
            );
        }
    }
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
