// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::cli::invocation::{Overrides, Request};
use crate::cli::namespace::Namespace;
use crate::compose::tests::futures_lite_block_on;
use crate::failure::Exit;
use crate::session::Record;
use crate::terminal::driver::{
    Guard, Pane as TurnPane, PaneConfirm, PaneSink, Turnable, question_for_the_shell, request_for,
    run,
};
use crate::terminal::fixtures::{Counting, Held, Recording, Restores, press, typed};
use crate::terminal::open::is_a_session;
use crate::terminal::source::{Source, Taken};
use crate::terminal::trie::{NOTHING_CACHED, NotesTrie};
use crate::terminal::vocabulary::{Transcript as Pane, Vocabulary};
use crate::tools::port::Question;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zaru_notes::trie::{CachedEntry, EntryKind as CachedKind};
use zaru_tui::shell::port::{CommandVocabulary, TranscriptSource};
use zaru_tui::shell::{COMPOSER_ROWS, Key, Shell, Status};

const VERSION: &str = "0.0.0";

/// The workspace the checks below attach their sessions to.
const WORKSPACE: &str = "zaru";

/// What the checks below have their shell say to a task, standing in for the
/// refusal `terminal::open` resolves once when it opens a real session.
const CANNOT: &str = "this check's shell resolved no provider";

/// One cached entity in [`WORKSPACE`].
fn entry(path: &str, title: &str, kind: CachedKind) -> CachedEntry {
    CachedEntry::new(WORKSPACE, path, title, kind)
}

/// Type `text` and send nothing, so the line stays in the composer.
///
/// [`typed`] appends `Enter`, which submits the line and replaces the composer
/// — so it can say nothing about what the strip showed while the user typed.
fn keys(text: &str) -> Vec<zaru_tui::shell::Input> {
    text.chars().map(|ch| press(Key::Char(ch))).collect()
}

/// The strip's rows out of a painted frame: what is below the input row.
///
/// The shell paints a status line, then the pane, then a fixed composer area at
/// the foot — so the input is the first of the last `COMPOSER_ROWS` rows and
/// the strip is everything after it. Read out of the buffer and trimmed, with
/// blank rows dropped, so what comes back is the lines a person can read.
fn strip_rows(frame: &[String]) -> Vec<String> {
    let input_row = frame.len() - usize::from(COMPOSER_ROWS);
    frame[input_row + 1..]
        .iter()
        .map(|row| row.trim_end().to_owned())
        .filter(|row| !row.is_empty())
        .collect()
}
const REPORT_AT: &str = "https://github.com/100monkeys-ai/zaru-cli";

fn shell() -> Shell {
    Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"))
}

fn pump(keys: Vec<zaru_tui::shell::Input>) -> (Shell, Recording, Exit) {
    pump_over(keys, &NotesTrie::nothing_cached(WORKSPACE))
}

/// The same pump over a fast tier a check chose.
fn pump_over(keys: Vec<zaru_tui::shell::Input>, trie: &NotesTrie) -> (Shell, Recording, Exit) {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let source = Source::scripted(keys);
    let pace = Held::default();
    let mut shell = shell();
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    shell.composer_mut().set_absence(trie.absence());
    let mut turns = Turnable::Cannot(vec![zaru_tui::shell::port::Line::new(
        zaru_tui::shell::port::Register::Failed,
        CANNOT.to_owned(),
    )]);
    let pumped = futures_lite_block_on(run(
        &mut shell,
        &mut surface,
        &source,
        &pace,
        &runner,
        trie,
        &Vocabulary,
        &mut turns,
    ))
    .expect("the recording terminal never fails");
    assert_eq!(
        source.contended(),
        0,
        "the source was contended {} time(s), which a single-threaded pump cannot do",
        source.contended()
    );
    (shell, surface, pumped.exit)
}

// ------------------------------------------------- ADR-0008 clause 3, whole

/// A clock the check sets, so every elapsed time is an exact value.
#[derive(Debug, Default)]
struct Ticking(std::sync::Mutex<core::time::Duration>);

impl zaru_core::iteration::Clock for Ticking {
    fn now(&self) -> core::time::Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// A model that answers once, so the turn is one exchange and no tool runs.
#[derive(Debug)]
struct OneAnswer(&'static str);

impl zaru_core::tool_call::Model for OneAnswer {
    fn capabilities(&self) -> zaru_core::tool_call::Capabilities {
        zaru_core::tool_call::Capabilities { tool_calling: true }
    }

    async fn respond(
        &self,
        _request: &zaru_core::tool_call::ModelRequest<'_>,
    ) -> Result<zaru_core::tool_call::ModelResponse, zaru_core::iteration::PortFailure> {
        Ok(zaru_core::tool_call::ModelResponse::Text {
            text: self.0.to_owned(),
            tokens: zaru_core::tool_call::TokenUsage {
                prompt: 7,
                completion: 3,
            },
        })
    }
}

/// A tool surface that offers nothing, because this turn asks for nothing.
#[derive(Debug, Default)]
struct NoTools(Vec<zaru_core::tool_call::ToolDescriptor>);

impl zaru_core::tool_call::ToolExecutor for NoTools {
    fn descriptors(&self) -> &[zaru_core::tool_call::ToolDescriptor] {
        &self.0
    }

    async fn execute(
        &mut self,
        _request: &zaru_core::tool_call::ToolRequest,
    ) -> Result<zaru_core::tool_call::ToolOutcome, zaru_core::iteration::PortFailure> {
        Err(zaru_core::iteration::PortFailure::new(
            "this turn asks for no tool",
        ))
    }
}

/// A context policy that renders the task, holding nothing.
#[derive(Debug, Default)]
struct Plain;

impl zaru_core::iteration::ContextPolicy for Plain {
    async fn assemble(
        &self,
        turn: &zaru_core::iteration::Turn<'_>,
    ) -> Result<zaru_core::iteration::Prompt, zaru_core::iteration::ContextRefusal> {
        let rendered = match turn {
            zaru_core::iteration::Turn::Initial { task } => (*task).to_owned(),
            _ => "not this check's turn".to_owned(),
        };
        Ok(zaru_core::iteration::Prompt::new(
            zaru_core::redaction::Redacted::by(&Nothing, &rendered),
        ))
    }
}

/// A redactor holding nothing, so a check can build a `Redacted`.
#[derive(Debug, Default)]
struct Nothing;

impl zaru_core::redaction::Redactor for Nothing {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

/// **ADR-0008 clause 3.** One emission, two consumers, on one slice.
///
/// The clause is "The event stream is consumed by both the terminal renderer
/// and the transcript writer, **from one emission**". The transcript writer
/// has been on the loop's slice since a provider client was wired to it, and
/// the arc that put it there said what was missing: "the shell is handed a
/// transcript rather than being a sink on that slice, and putting it there is
/// the arc that owns `zaru-tui`".
///
/// Both are on the slice here and the loop is the emitter, so this is the
/// clause rather than a restatement of it: `run` "constructs each event once
/// and hands the same value to every registered sink in turn". Neither arm
/// travels through the other — the file is read back off the filesystem with
/// `std::fs` and the pane's lines are read off the shell — and both are
/// compared against the events, so an implementation that painted the file
/// rather than the emission could not satisfy it.
/// An inner loop nothing supplies, for the turns these checks drive.
///
/// `zaru-cli` carried one of these in its **product** tree until 2026-09-05,
/// when [ADR-0009] D4's branch got a real implementation and the uninhabited
/// stand-in was deleted with the refusal it stood beside. A check that drives
/// `tool_call::run` over a turn with no validators still has to name a type
/// for the `None`, and an empty enum is the one that cannot be constructed by
/// mistake — which is why it is here, in the test tree, rather than back in
/// the product one.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
enum NoInner {}

impl zaru_core::tool_call::InnerLoop for NoInner {
    /// Unreachable: there is no value of `Self` to have called it on.
    async fn iterate(
        &self,
        _task: &str,
    ) -> Result<zaru_core::iteration::Outcome, zaru_core::iteration::PortFailure> {
        match *self {}
    }
}

#[test]
fn one_emission_reaches_the_transcript_and_the_pane() {
    use zaru_core::tool_call::{Ports, Start, ToolCalling};

    let scratch = crate::credentials::fixtures::ScratchRoot::new();
    let path = scratch.store_root().join("transcript.jsonl");
    std::fs::create_dir_all(path.parent().expect("the path has a parent"))
        .expect("the scratch root is writable");

    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut written = crate::compose::Records::appending_to(&path).expect("the transcript opens");

    let model = OneAnswer("the rehearsal number is 4173");
    let mut tools = NoTools::default();
    let policy = Plain;
    let clock = Ticking::default();
    let witness = ToolCalling::required(&model, "a-model").expect("the model calls tools");

    let outcome = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let mut painted = PaneSink::over(&pane);
        let mut sinks: [&mut dyn zaru_core::tool_call::EventSink; 2] = [&mut written, &mut painted];
        let ran = futures_lite_block_on(zaru_core::tool_call::run(
            3,
            Start::Task("say the number"),
            crate::cli::layers::tool_call_ceiling(),
            witness,
            Ports {
                model: &model,
                tools: &mut tools,
                context: &policy,
                clock: &clock,
                redactor: &Nothing,
            },
            Option::<&NoInner>::None,
            &mut sinks,
        ));
        assert_eq!(
            painted.contended(),
            0,
            "the pane's lock was contended, which a single-threaded turn cannot do; the sink \
             dropped {} event(s)",
            painted.contended()
        );
        ran
    };
    outcome.expect("the staged turn answers");

    // The file, read back with `std::fs` rather than through the sink.
    let bytes = std::fs::read_to_string(&path).expect("the transcript is there");
    let recorded: Vec<zaru_core::tool_call::Event> = bytes
        .lines()
        .map(|line| match serde_json::from_str::<Record>(line) {
            Ok(Record::TurnLoop(event)) => event,
            other => panic!("a line is not one of the outer loop's events: {other:?}"),
        })
        .collect();
    assert!(
        !recorded.is_empty(),
        "the turn wrote no event at all, so this check compared two empty lists"
    );

    // The pane, read off the shell.
    let painted: Vec<String> = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect();
    assert_eq!(
        painted.len(),
        recorded.len(),
        "the file holds {} event(s) and the pane holds {} line(s), so the two consumers did not \
         see one emission:\n  file: {recorded:?}\n  pane: {painted:?}",
        recorded.len(),
        painted.len()
    );
    for (event, line) in recorded.iter().zip(painted.iter()) {
        assert_eq!(
            line,
            &crate::terminal::vocabulary::turn_line(event).text,
            "the pane's line is not this event's line: {event:?}"
        );
    }
    // The first record is the turn this caller named, which is what makes the
    // turn number the caller's rather than one the loop restarts.
    assert!(
        matches!(
            recorded.first(),
            Some(zaru_core::tool_call::Event::TurnStarted { n: 3, .. })
        ),
        "the loop did not start the turn the caller numbered: {:?}",
        recorded.first()
    );
}

// -------------------------------------------- ADR-0011 D3, answered in a pane

/// The question is answered in the pane, and only `y` is a yes.
///
/// The accepting arm and the declining arms are in one check so that an
/// implementation answering the same way to everything fails whichever way it
/// answers. The declining set is `Shell::key`'s own — `n`, `Esc` and `Enter`,
/// the last being the default the prompt renders as `N`.
#[test]
fn a_question_is_answered_in_the_pane_and_only_y_is_a_yes() {
    use crate::tools::port::Confirm as _;

    for (key, expected) in [
        (Key::Char('y'), true),
        (Key::Char('Y'), true),
        (Key::Char('n'), false),
        (Key::Char('N'), false),
        (Key::Esc, false),
        (Key::Enter, false),
    ] {
        let mut shell = shell();
        let restores: Restores = Arc::new(AtomicUsize::new(0));
        // A key the shell ignores first, so the loop is asserted to keep
        // asking rather than to answer whatever it read.
        let mut surface = Recording::of(restores);
        let source = Source::scripted(vec![press(Key::Char('q')), press(key)]);
        let pace = Held::default();
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let confirm = PaneConfirm::over(&pane, &source, &pace);

        let answered = confirm
            .confirm(&Question {
                statement: "run `rm -rf build` in /home/someone/project".to_owned(),
                prominent: true,
            })
            .expect("the pane answered");
        assert_eq!(
            answered, expected,
            "{key:?} was read as {answered} rather than {expected}"
        );
    }
}

/// A terminal that stops answering is a failure, never a `no`.
///
/// `crate::tools::prompt`'s own rule, stated for this surface: answering
/// `false` would put "the user declined" in the transcript of a question
/// nobody saw. **Its accepting sibling is the check above**, which answers.
#[test]
fn a_pane_that_runs_out_of_keys_refuses_rather_than_declining() {
    use crate::tools::port::Confirm as _;

    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(restores);
    let source = Source::scripted(Vec::new());
    let pace = Held::default();
    let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
    let confirm = PaneConfirm::over(&pane, &source, &pace);

    let outcome = confirm.confirm(&Question {
        statement: "write build/out.txt".to_owned(),
        prominent: false,
    });
    let failure = outcome.expect_err("a pane with no answer must not answer");
    assert!(
        failure.to_string().contains("stopped answering"),
        "the failure does not say what happened: {failure}"
    );
}

/// The question the pane paints is the statement it was handed, and the pane
/// is what the user is looking at while it stands.
#[test]
fn the_question_reaches_the_painted_frame_before_a_key_is_read() {
    use crate::tools::port::Confirm as _;

    const STATEMENT: &str = "run `rm -rf build` in /home/someone/project";
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(restores);
    let source = Source::scripted(vec![press(Key::Char('y'))]);
    let pace = Held::default();
    {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let confirm = PaneConfirm::over(&pane, &source, &pace);
        confirm
            .confirm(&Question {
                statement: STATEMENT.to_owned(),
                prominent: true,
            })
            .expect("the pane answered");
    }

    let first = surface
        .frames
        .first()
        .expect("no frame was painted")
        .clone();
    let rows: String = first.join("\n");
    // The statement crosses unchanged, which is ADR-0011 D3's own rule; the
    // frame is 72 columns wide, so the assertion is on the part that fits.
    assert!(
        rows.contains("run `rm -rf build`"),
        "the question was not painted before the answer was read:\n{rows}"
    );
    assert!(
        rows.contains(zaru_tui::shell::render::PROMINENT),
        "ADR-0011 D6's prominence did not reach the frame:\n{rows}"
    );
}

// ------------------------------------------- the terminal is always given back

/// A restore written at the end of a loop is a restore that happens on the
/// paths the author thought of. This one is a `Drop`.
#[test]
fn the_terminal_is_restored_on_an_ordinary_exit() {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    {
        let _guard = Guard::new(Counting(Arc::clone(&restores)));
        assert_eq!(
            restores.load(Ordering::SeqCst),
            0,
            "the guard restored before it was dropped"
        );
    }
    assert_eq!(
        restores.load(Ordering::SeqCst),
        1,
        "the terminal was not given back when the guard went out of scope"
    );
}

/// The path that matters. A panic that left the terminal in raw mode would
/// make ADR-0016 D3's defect report unreadable at the moment the user most
/// needs to read it, because the report is written to a terminal that is no
/// longer echoing or wrapping.
#[test]
fn the_terminal_is_restored_when_the_shell_panics() {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&restores);

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _guard = Guard::new(Counting(counted));
        panic!("a defect inside the pump");
    }));
    std::panic::set_hook(previous);

    assert!(
        unwound.is_err(),
        "the panic did not happen, so this check asserted nothing"
    );
    assert_eq!(
        restores.load(Ordering::SeqCst),
        1,
        "the terminal was not given back when the shell panicked"
    );
}

/// Restoring by hand and then dropping hands the terminal back once, not
/// twice. A second restore would leave the alternate screen a second time,
/// which on a real terminal scrolls the user's own scrollback away.
#[test]
fn the_terminal_is_restored_exactly_once_when_it_is_also_restored_by_hand() {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    {
        let mut guard = Guard::new(Counting(Arc::clone(&restores)));
        guard.restore_now();
        assert_eq!(restores.load(Ordering::SeqCst), 1);
        guard.restore_now();
    }
    assert_eq!(
        restores.load(Ordering::SeqCst),
        1,
        "the terminal was handed back more than once"
    );
}

// ----------------------------------------------- ADR-0015 D2, one vocabulary

/// The adapter answers from this crate's own closed enum and holds no list.
///
/// Walked from `Namespace::ALL` on both sides, so this cannot pass by two
/// lists agreeing: one of the two arms is the enum itself.
#[test]
fn the_vocabulary_is_adr_0015_d2s_own_closed_set() {
    let rows = Vocabulary.namespaces();
    assert_eq!(
        rows.len(),
        Namespace::ALL.len(),
        "the shell sees a different number of namespaces than ADR-0015 D2's table carries"
    );
    for (row, namespace) in rows.iter().zip(Namespace::ALL) {
        assert_eq!(row.slash, namespace.slash());
        assert_eq!(row.governs, namespace.governs());
        assert_eq!(row.built, namespace.is_built());
        assert_eq!(row.verbs, namespace.slash_verbs());
    }
}

/// ADR-0010 D4's in-session spellings, which the subcommand surface cannot
/// carry because outside a session they are flags.
#[test]
fn the_in_session_verbs_of_session_are_adr_0010_d4s_four() {
    assert_eq!(
        Namespace::Session.slash_verbs(),
        &["resume", "continue", "list", "rm"]
    );
    assert_eq!(
        Namespace::Session.verbs(),
        &["list", "rm"],
        "the out-of-session verbs changed, and `--resume` and `--continue` are flags there"
    );
}

/// The accepting sibling: only `/session` differs, and every other namespace
/// answers identically on both surfaces rather than by a second list.
#[test]
fn every_namespace_but_session_answers_identically_on_both_surfaces() {
    for namespace in Namespace::ALL {
        if namespace == Namespace::Session {
            continue;
        }
        assert_eq!(
            namespace.slash_verbs(),
            namespace.verbs(),
            "{namespace} answers two different verb lists for one operation"
        );
    }
}

/// ADR-0014 D5's nearest match is the one implementation, reached from both
/// surfaces.
#[test]
fn the_nearest_match_is_the_one_the_out_of_session_parser_uses() {
    // Two different answers, so an implementation that always returned one
    // namespace cannot pass. The `-> /session` arm alone was satisfied by
    // returning the nearest to *anything*, which a red-watch found.
    assert_eq!(Vocabulary.nearest("sessoin"), Some("/session"));
    assert_eq!(Vocabulary.nearest("modles"), Some("/models"));
    assert_eq!(
        Vocabulary.nearest_verb("/session", "resmue"),
        Some("resume")
    );
    assert_eq!(
        Vocabulary.nearest_verb("/runtime", "anything"),
        None,
        "a namespace that takes no verb offered one anyway"
    );
}

/// Every namespace this build implements reaches a request from the slash
/// side, and the ones that do not are exactly the two ADR-0010 D4 leaves
/// unanswered.
///
/// # This check exists because a rebase found the hole it closes
///
/// `dispatch`'s fall-through is a wildcard, and an eleventh **built**
/// namespace added without an arm there is reported to the user as
/// unavailable rather than failing to compile. That happened: `/providers`
/// arrived on `main` while this arc was in flight, `cli::namespace`'s own
/// exhaustive matches caught it one layer up, and nothing would have caught it
/// here. So the vocabulary is walked rather than listed.
///
/// The two exceptions are named rather than skipped. `/session resume` and
/// `/session continue` from **inside** a session are an operation no record
/// describes — what resuming does to the session you are already in is not
/// stated anywhere — so they are refused rather than answered, which is a
/// decision and belongs in a check.
#[test]
fn every_built_namespace_reaches_a_request_from_the_slash_side() {
    let mut unreachable: Vec<(&str, Option<&str>)> = Vec::new();
    for namespace in Vocabulary.namespaces() {
        if !namespace.built {
            continue;
        }
        let verbs: Vec<Option<&'static str>> = if namespace.verbs.is_empty() {
            vec![None]
        } else {
            namespace.verbs.iter().copied().map(Some).collect()
        };
        let mut any = false;
        for verb in verbs {
            // A command whose argument is required needs a plausible one, or
            // this check would report "unavailable" for a command that is
            // reachable and was simply asked wrongly. The words are the
            // check's own literals rather than anything the product produced.
            let words: Vec<String> = match (namespace.slash, verb) {
                ("/config", Some("explain")) => vec!["runtime.tier".to_owned()],
                ("/session", Some("rm")) => vec!["01JQZX8N3K4M5P6R7S8T9V0W1X".to_owned()],
                _ => Vec::new(),
            };
            let named = request_for(&zaru_tui::shell::Command {
                slash: namespace.slash,
                verb,
                words,
            });
            if named.is_none() {
                unreachable.push((namespace.slash, verb));
            } else {
                any = true;
            }
        }
        assert!(
            any,
            "`{}` is built and no verb of it reaches a request, so a user inside a session is \
             told it is unavailable while `zaru {}` runs",
            namespace.slash,
            namespace.slash.trim_start_matches('/')
        );
    }

    assert_eq!(
        unreachable,
        vec![("/session", Some("resume")), ("/session", Some("continue"))],
        "the set of slash spellings that reach no request is not the two ADR-0010 D4 leaves \
         unanswered"
    );
}

// --------------------------------------- ADR-0015 D2, one operation, two ways

/// The clause this arc exists for. A slash command runs the **same function**
/// its subcommand spelling runs, so the two spellings cannot come to disagree.
#[test]
fn a_slash_command_produces_what_its_subcommand_spelling_produces() {
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    let outside = runner.execute(&crate::cli::invocation::CommandLine {
        request: Request::Runtime,
        overrides: Overrides::default(),
    });

    let (shell, _, _) = pump(typed("/runtime"));
    let inside: Vec<String> = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect();

    for line in &outside.lines {
        assert!(
            inside.contains(line),
            "`/runtime` did not produce the line `zaru runtime` produces: {line:?} is absent \
             from {inside:?}"
        );
    }
    assert!(
        !outside.lines.is_empty(),
        "the out-of-session command produced nothing, so this check compared two empty lists"
    );
}

/// The four namespaces D2 names and this build does not implement are refused
/// in the pane saying so, and the session stays open.
#[test]
fn an_unbuilt_namespace_is_refused_in_the_pane_and_the_shell_stays_open() {
    let mut keys = typed("/stack install");
    keys.extend(typed("/exit"));
    let (shell, _, exit) = pump(keys);

    let said: String = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        said.contains("does not implement it yet"),
        "the pane does not carry the refusal: {said}"
    );
    assert_eq!(exit.code(), 0, "the shell did not exit 0 on the leave word");
}

/// A task is answered in the pane, the shell stays open, and **nothing on this
/// machine gains a session**.
///
/// The second half is the one that matters and it is why this check counts
/// sessions rather than only reading the pane. Until 2026-09-05 the pane's
/// task arm executed a `Request::Task` through a real `cli::Run`, and after
/// the arc that made that request run a turn, this check drove a path that
/// would mint a session, ask a model an empty prompt and read the user's own
/// standard input — under the developer's real `HOME`, because nothing here
/// can set one. It stayed green only because the machine running it had no
/// provider key.
///
/// So the count is taken through the product's own store, before and after,
/// and an unreadable store on either side is the same answer on both. The
/// staging is asserted too, so a pump that did nothing could not satisfy it.
#[test]
fn a_task_is_answered_in_the_pane_and_mints_no_session() {
    let before = sessions_on_this_machine();

    let mut keys = typed("rename the widget");
    keys.extend(typed("/exit"));
    let (shell, surface, exit) = pump(keys);

    let said: String = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        said.contains(CANNOT),
        "the pane does not carry the notice a task gets; it said {said:?}"
    );
    assert_eq!(exit.code(), 0);
    assert!(
        surface.frames.len() > 2,
        "the shell closed rather than staying open: only {} frames were painted",
        surface.frames.len()
    );

    let after = sessions_on_this_machine();
    assert_eq!(
        before, after,
        "typing a task at the prompt changed what sessions exist on this machine: \
         {before:?} before, {after:?} after"
    );
}

/// Every session id under this machine's own session root, or `None` when the
/// root cannot be read at all.
///
/// `None` on both sides of a pump is the same answer as an equal list: a
/// machine with no `~/.zaru` is one where a session could only have been
/// created by making the directory, which is exactly what is being asserted
/// did not happen.
fn sessions_on_this_machine() -> Option<Vec<crate::session::SessionId>> {
    crate::session::SessionStore::default_root()
        .ok()
        .map(crate::session::SessionStore::reading)
        .and_then(|store| store.ids().ok())
}

// ------------------------------------------- ADR-0008 D1's outer loop, rendered

/// Every one of the outer loop's events, in the order a turn emits them.
///
/// Built once and walked by the two checks below, so neither carries a list of
/// its own and a seventh event has to be given a register here rather than
/// falling into whatever the last arm was.
fn every_turn_event() -> Vec<zaru_core::tool_call::Event> {
    use core::time::Duration;
    use zaru_core::tool_call::{Event, TurnEnding};

    let took = Duration::from_millis(1_250);
    let mut events = vec![
        Event::TurnStarted { n: 2, of: 8 },
        Event::ModelResponded {
            round: 1,
            tokens: 451,
            calls: 1,
            elapsed: took,
        },
        Event::ToolRequested {
            round: 1,
            call: 1,
            name: "fs.read".to_owned(),
        },
        Event::ToolPermissionDecided {
            round: 1,
            call: 1,
            statement: "read notes.txt in /home/someone/project".to_owned(),
            permitted: true,
        },
        Event::ToolCompleted {
            round: 1,
            call: 1,
            name: "fs.read".to_owned(),
            failed: false,
            content_bytes: 42,
            elapsed: took,
        },
        Event::ToolRefused {
            round: 1,
            call: 2,
            name: "cmd.run".to_owned(),
            because: "the user declined".to_owned(),
            elapsed: took,
        },
    ];
    for ending in [
        TurnEnding::Answered,
        TurnEnding::Stopped,
        TurnEnding::CeilingReached,
        TurnEnding::Iterated {
            iterations: 3,
            succeeded: true,
        },
        TurnEnding::Iterated {
            iterations: 3,
            succeeded: false,
        },
    ] {
        events.push(Event::TurnEnded {
            n: 2,
            ending,
            rounds: 1,
            elapsed: took,
        });
    }
    events
}

/// The pane's line for a turn event, through the path a resumed session uses.
fn painted_turn_line(event: &zaru_core::tool_call::Event) -> zaru_tui::shell::port::Line {
    let pane = Pane::of(&[Record::TurnLoop(event.clone())]);
    let mut lines = pane.lines();
    assert_eq!(lines.len(), 1, "one record did not produce one line");
    lines.remove(0)
}

/// The outer loop's events are sentences in this shell's own registers, and
/// **not** a `Debug` dump in the narration one.
///
/// Until 2026-09-05 the arm was `Line::new(Register::Plain, format!("{event:?}"))`,
/// so every event rendered as its Rust shape and three registers this shell
/// defines were unreachable from a real turn. That was already what a person
/// saw: `zaru --resume <id>` at a terminal over a session `zaru "<task>"`
/// created painted whole turns that way.
///
/// The second arm is what discriminates. A rendering that carried the variant's
/// own identifier is a `Debug` of it whatever else it also says, and no
/// sentence composed for a reader has a reason to contain one.
#[test]
fn every_turn_event_renders_in_a_register_this_shell_defines_and_never_as_debug() {
    use zaru_tui::shell::port::Register;

    let expected = [
        Register::Plain,
        Register::Plain,
        Register::Call,
        Register::Call,
        Register::Call,
        // ADR-0011 D6 gives the harness no veto and the user's "no" is an
        // answer, so a refused call is announced -- never `Failed`.
        Register::Announced,
        Register::Succeeded,
        Register::Failed,
        // ADR-0008 D5: exhaustion is neither.
        Register::Exhausted,
        Register::Succeeded,
        Register::Exhausted,
    ];
    let events = every_turn_event();
    assert_eq!(
        events.len(),
        expected.len(),
        "the event list and the register list are different lengths, so this \
         check would have compared a prefix"
    );

    let identifiers = [
        "TurnStarted",
        "ModelResponded",
        "ToolRequested",
        "ToolPermissionDecided",
        "ToolCompleted",
        "ToolRefused",
        "TurnEnded",
        "TurnEnding",
    ];
    for (event, want) in events.iter().zip(expected) {
        let line = painted_turn_line(event);
        assert_eq!(
            line.register, want,
            "{event:?} rendered in {:?} rather than {want:?}",
            line.register
        );
        for identifier in identifiers {
            assert!(
                !line.text.contains(identifier),
                "the line for {event:?} carries the Rust identifier {identifier:?}, so it \
                 is a `Debug` of the event rather than a sentence: {:?}",
                line.text
            );
        }
        assert!(
            !line.text.trim().is_empty(),
            "{event:?} rendered as nothing at all"
        );
    }
}

/// A refused call and a failed turn are different registers, and each carries
/// the words its own event holds.
///
/// Two arms in opposite directions. A rendering that put everything in the
/// error register would satisfy the second and fail the first; one that put
/// everything in a decision register would do the reverse.
#[test]
fn a_refused_call_is_announced_and_a_stopped_turn_is_failed() {
    use zaru_tui::shell::port::Register;

    let events = every_turn_event();
    let refused = painted_turn_line(
        events
            .iter()
            .find(|event| matches!(event, zaru_core::tool_call::Event::ToolRefused { .. }))
            .expect("the staged events carry a refusal"),
    );
    assert_eq!(
        refused.register,
        Register::Announced,
        "a declined prompt was rendered in {:?}; ADR-0011 D6 makes it a decision \
         and its own event says a consumer renders it \"never in the error one\"",
        refused.register
    );
    assert!(
        refused.text.contains("the user declined"),
        "the refusal does not carry the refusing surface's own words: {:?}",
        refused.text
    );

    let stopped = painted_turn_line(
        events
            .iter()
            .find(|event| {
                matches!(
                    event,
                    zaru_core::tool_call::Event::TurnEnded {
                        ending: zaru_core::tool_call::TurnEnding::Stopped,
                        ..
                    }
                )
            })
            .expect("the staged events carry a stopped turn"),
    );
    assert_eq!(
        stopped.register,
        Register::Failed,
        "a turn that stopped without answering was rendered in {:?}",
        stopped.register
    );
    assert_ne!(
        refused.register, stopped.register,
        "a declined prompt and a failed turn render identically, so a reader \
         cannot tell an answer they gave from a failure they did not"
    );
}

// ------------------------------------------- ADR-0008 D3's eight events, rendered

/// Every one of the inner loop's events, in the order a run emits them.
///
/// Built once and walked by the check below, so it carries no list of its own
/// and a ninth event has to be given a sentence here rather than falling into
/// whatever the last arm was. Both `ExhaustionReason` variants and all three
/// `ValidatorOutcome`s appear, because each is a separate arm of the rendering
/// and a fixture carrying one of the three would leave two unexercised.
fn every_loop_event() -> Vec<zaru_core::iteration::Event> {
    use core::time::Duration;
    use zaru_core::iteration::{Event, ExhaustionReason, ValidatorOutcome};

    let took = Duration::from_millis(1_250);
    let mut events = vec![
        Event::IterationStarted { n: 2, of: 5 },
        Event::CandidateGenerated {
            tokens: 451,
            elapsed: took,
        },
        Event::ExecutionCompleted {
            exit_code: 1,
            stdout_bytes: 42,
            stderr_bytes: 7,
            elapsed: took,
        },
    ];
    for outcome in [
        ValidatorOutcome::Passed,
        ValidatorOutcome::Failed,
        ValidatorOutcome::Skipped,
    ] {
        events.push(Event::ValidatorEvaluated {
            name: "tests".to_owned(),
            outcome,
            detail: "2 of 3 assertions held".to_owned(),
        });
    }
    events.push(Event::IterationFailed {
        n: 2,
        reason: "the third assertion did not hold".to_owned(),
        elapsed: took,
    });
    events.push(Event::RefinementConstructed {
        n: 2,
        failure_excerpt: "the third assertion did not hold".to_owned(),
    });
    events.push(Event::LoopSucceeded {
        iterations: 3,
        elapsed: took,
        total_elapsed: took,
    });
    for reason in [
        ExhaustionReason::CeilingReached,
        ExhaustionReason::ContextWindowExceeded {
            needed: 9_000,
            window: 8_000,
        },
    ] {
        events.push(Event::LoopExhausted {
            iterations: 3,
            reason,
            last_failure: Some("the third assertion did not hold".to_owned()),
        });
    }
    events
}

/// The pane's line for a loop event, through the path a resumed session uses.
fn painted_loop_line(event: &zaru_core::iteration::Event) -> zaru_tui::shell::port::Line {
    let pane = Pane::of(&[Record::Loop(event.clone())]);
    let mut lines = pane.lines();
    assert_eq!(lines.len(), 1, "one record did not produce one line");
    lines.remove(0)
}

/// The inner loop's events are sentences, and **not** a `Debug` dump of the
/// two enums they carry.
///
/// The sibling of
/// [`every_turn_event_renders_in_a_register_this_shell_defines_and_never_as_debug`],
/// for the loop whose events [ADR-0008] D3 actually enumerates. That check
/// found the outer loop rendering as its Rust shape; this one holds the two
/// places the inner loop still did on 2026-09-05 — `ValidatorEvaluated`'s
/// outcome, rendered `{outcome:?}`, and `LoopExhausted`'s reason, rendered
/// `{reason:?}` and so printing `ContextWindowExceeded { needed: 9000, window:
/// 8000 }` at a reader.
///
/// The second is the load-bearing one. ADR-0008 D3's own words for why that
/// variant carries its two numbers are that "D7 asks for a *clear* reason and
/// a reader cannot act on 'the window was exceeded' without knowing by how
/// much" — and a struct dump is not a clear reason, it is the numbers with the
/// field names of the type that holds them.
///
/// The identifier list is what discriminates. A rendering that carried a
/// variant's own identifier is a `Debug` of it whatever else it also says, and
/// no sentence composed for a reader has a reason to contain one.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[test]
fn every_loop_event_renders_as_a_sentence_and_never_as_debug() {
    let identifiers = [
        "IterationStarted",
        "CandidateGenerated",
        "ExecutionCompleted",
        "ValidatorEvaluated",
        "IterationFailed",
        "RefinementConstructed",
        "LoopSucceeded",
        "LoopExhausted",
        "ValidatorOutcome",
        "Passed",
        "Failed",
        "Skipped",
        "ExhaustionReason",
        "CeilingReached",
        "ContextWindowExceeded",
        "needed:",
        "window:",
    ];

    for event in every_loop_event() {
        let line = painted_loop_line(&event);
        for identifier in identifiers {
            assert!(
                !line.text.contains(identifier),
                "the line for {event:?} carries the Rust identifier {identifier:?}, so it \
                 is a `Debug` of the event rather than a sentence: {:?}",
                line.text
            );
        }
        assert!(
            !line.text.trim().is_empty(),
            "{event:?} rendered as nothing at all"
        );
    }
}

/// The window route's line carries both of its numbers, in the words the
/// binary already uses for them.
///
/// **Two callers, one wording.** [`crate::cli::render::exhaustion`] is what
/// `zaru "<task>"` prints when an iterated turn exhausts, and it is what the
/// pane paints; a second phrasing here would let the exit-code reader and the
/// person watching disagree about why a run stopped. So the assertion is
/// against that function's own output rather than against a literal this check
/// owns — a literal would pass a renderer that had drifted from the binary.
#[test]
fn the_exhaustion_reason_the_pane_paints_is_the_one_the_binary_prints() {
    use zaru_core::iteration::ExhaustionReason;

    for reason in [
        ExhaustionReason::CeilingReached,
        ExhaustionReason::ContextWindowExceeded {
            needed: 9_000,
            window: 8_000,
        },
    ] {
        let line = painted_loop_line(&zaru_core::iteration::Event::LoopExhausted {
            iterations: 3,
            reason,
            last_failure: None,
        });
        let said = crate::cli::render::exhaustion(3, reason);
        assert!(
            line.text.contains(&said),
            "the pane says {:?} where the binary says {said:?}, so one run has two \
             explanations depending on where it is read",
            line.text
        );
    }
}

/// As much of a line as a 72-column pane can show.
///
/// The pane truncates to its width, so a check that looked for the whole
/// sentence would fail on every line longer than the frame and would be
/// asserting the terminal's width rather than the renderer's output.
fn as_far_as_the_frame_shows(text: &str) -> String {
    text.trim().chars().take(40).collect()
}

/// The inner loop's narrative is painted as it arrives, and it is on the
/// frame — [ADR-0028] D1 and D3.
///
/// D1: iteration state changes "surface as plain-English events inline in the
/// conversation: what was tried, what failed, what changed, what succeeded".
/// D3: the harness "renders the loop's typed events per [ADR-0008] D3" and
/// "**neither reconstructs the narrative from inference**".
///
/// Until 2026-09-05 nothing subscribed to that stream at all:
/// `compose::Inner` handed `iteration::run` the transcript writer alone, so
/// every one of these lines existed only in `transcript.jsonl` and was
/// reachable only by `zaru --resume <id>` afterwards. A person watching a run
/// saw the turn start and the turn end.
///
/// **Read off `TestBackend`, not off the shell.** `Recording` collects the
/// rows of each painted frame out of the backend's buffer, so this asserts
/// what a terminal would actually show rather than what the shell was told —
/// and it paints once per event, which is what "as the work proceeds" means.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn the_inner_loops_narrative_is_painted_on_the_frame_as_it_arrives() {
    use crate::compose::Narrator;
    use crate::terminal::driver::PaneNarrator;

    let events = every_loop_event();
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));

    let narrator = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let narrator = PaneNarrator::over(&pane);
        for event in &events {
            narrator.narrate(event);
        }
        narrator.contended()
    };
    assert_eq!(
        narrator, 0,
        "the pane's lock was contended, which a single-threaded run cannot do; \
         the narrator dropped {narrator} event(s)"
    );

    assert_eq!(
        surface.frames.len(),
        events.len(),
        "the pane painted {} frame(s) for {} event(s); the narrative is supposed \
         to arrive as the work proceeds rather than in one repaint at the end",
        surface.frames.len(),
        events.len()
    );

    // Each line on the frame painted at the moment its event arrived, rather
    // than all of them on the last one. The pane shows the tail -- ADR-0010
    // D4's "re-renders the last" -- so a long enough run scrolls its own
    // opening off, and looking only at the end would assert the pane's height.
    // This is also the stronger claim: it is what "as the work proceeds" means.
    for (index, event) in events.iter().enumerate() {
        let line = crate::terminal::vocabulary::loop_line(event);
        let wanted = as_far_as_the_frame_shows(&line.text);
        let frame = surface.frames[index].join("\n");
        assert!(
            frame.contains(&wanted),
            "the frame painted when {event:?} arrived does not carry its line: \
             expected {wanted:?} in\n{frame}"
        );
    }
}

/// Exhaustion, success and failure are three glyphs on the frame, not one.
///
/// [ADR-0008] D5 makes exhaustion "not an error and … not a success", and its
/// trigger clause 4 asks for a test that asserts the distinction. That clause
/// is already satisfied off the binary's exit code; this holds the same
/// property one layer out, where a person reads it — the three registers reach
/// the painted buffer as three different opening glyphs.
///
/// The glyphs themselves are not asserted, because `Register::glyph`'s own
/// documentation says three of the six are drafted and open to Jeshua's veto.
/// What is asserted is that no two of the three are the same, which is what
/// D5 actually requires and what survives a change of characters.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[test]
fn success_exhaustion_and_failure_reach_the_frame_as_three_different_glyphs() {
    use crate::compose::Narrator;
    use crate::terminal::driver::PaneNarrator;

    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));

    // All three through the sinks a real turn uses, so this asserts the
    // production paths rather than a register handed straight to the pane.
    // Success and exhaustion are the inner loop's own terminal events; the
    // error register has no iteration event by design -- ADR-0028 D2 keeps an
    // iteration's failure out of it -- so it comes from the outer loop's
    // `TurnEnding::Stopped`, which is what that register is for.
    let succeeded = zaru_core::iteration::Event::LoopSucceeded {
        iterations: 2,
        elapsed: core::time::Duration::from_millis(500),
        total_elapsed: core::time::Duration::from_millis(900),
    };
    let exhausted = zaru_core::iteration::Event::LoopExhausted {
        iterations: 3,
        reason: zaru_core::iteration::ExhaustionReason::CeilingReached,
        last_failure: None,
    };
    let stopped = zaru_core::tool_call::Event::TurnEnded {
        n: 1,
        ending: zaru_core::tool_call::TurnEnding::Stopped,
        rounds: 1,
        elapsed: core::time::Duration::from_millis(500),
    };

    {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let narrator = PaneNarrator::over(&pane);
        narrator.narrate(&succeeded);
        narrator.narrate(&exhausted);
        let mut sink = PaneSink::over(&pane);
        zaru_core::tool_call::EventSink::emit(&mut sink, &stopped);
    }

    let wanted = [
        crate::terminal::vocabulary::loop_line(&succeeded).text,
        crate::terminal::vocabulary::loop_line(&exhausted).text,
        crate::terminal::vocabulary::turn_line(&stopped).text,
    ];
    let frame = surface.frames.last().expect("a frame was painted");
    let opening: Vec<char> = wanted
        .iter()
        .map(|text| {
            let wanted = as_far_as_the_frame_shows(text);
            let row = frame
                .iter()
                .find(|row| row.contains(&wanted))
                .unwrap_or_else(|| panic!("the frame has no row for {wanted:?}: {frame:?}"));
            row.trim_start()
                .chars()
                .next()
                .unwrap_or_else(|| panic!("the row for {text:?} opens with nothing"))
        })
        .collect();

    assert_ne!(
        opening[0], opening[1],
        "success and exhaustion open with the same glyph {:?}, so a reader \
         cannot tell a loop that finished from one that ran out",
        opening[0]
    );
    assert_ne!(
        opening[1], opening[2],
        "exhaustion and a defect open with the same glyph {:?}; ADR-0008 D5 \
         says exhaustion is not an error",
        opening[1]
    );
    assert_ne!(
        opening[0], opening[2],
        "success and a defect open with the same glyph {:?}",
        opening[0]
    );
}

// -------------------------------------------------------------- ADR-0011 D3

/// ADR-0011 D3's question crosses to the shell with its sentence unchanged.
///
/// That record's port says the statement "is composed once, by the decision,
/// and handed here — rather than composed where it is rendered — so that what
/// the user was told and what the harness believes it asked cannot drift
/// apart". A conversion that reworded it would be that drift.
#[test]
fn a_question_crosses_to_the_shell_with_its_statement_unchanged() {
    for prominent in [false, true] {
        let question = Question {
            statement: "run `rm -rf build` in /home/someone/project".to_owned(),
            prominent,
        };
        let crossed = question_for_the_shell(&question);
        assert_eq!(crossed.statement, question.statement);
        assert_eq!(crossed.prominent, question.prominent);
    }
}

/// The default reaches the buffer through the real pump, not only through the
/// shell's own unit check.
#[test]
fn a_confirmation_renders_its_default_through_the_pump() {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let source = Source::scripted(vec![press(Key::Enter)]);
    let pace = Held::default();
    let mut shell = shell();
    shell.ask(question_for_the_shell(&Question {
        statement: "run `rm -rf build`".to_owned(),
        prominent: true,
    }));
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    futures_lite_block_on(run(
        &mut shell,
        &mut surface,
        &source,
        &pace,
        &runner,
        &NotesTrie::nothing_cached(WORKSPACE),
        &Vocabulary,
        &mut Turnable::Cannot(Vec::new()),
    ))
    .expect("pump");

    let first = surface.frames.first().expect("no frame was painted");
    // The vocabulary asserted here is the plain prompt's own constant, so this
    // cannot pass by two spellings agreeing: there is one source and the pane
    // paints what it was handed. **The constant is deliberately not repeated
    // here** -- writing `[y/N]` in this check would be the second spelling the
    // lift removed, one layer out. What is asserted instead is that it says
    // something, so the `contains` below cannot pass on an empty needle.
    let vocabulary = crate::tools::prompt::SUFFIX.trim();
    assert!(
        !vocabulary.is_empty(),
        "the plain prompt's answers are empty, so the assertion below is `contains(\"\")`"
    );
    assert!(
        first.iter().any(|row| row.contains(vocabulary)),
        "the plain prompt's answers line is not in the pane's first frame: {first:#?}"
    );
    assert!(
        first.iter().any(|row| row.contains("! run `rm -rf build`")),
        "ADR-0011 D6's marking is not in the frame: {first:#?}"
    );
    assert_eq!(
        shell.answer(),
        Some(false),
        "Enter through the pump did not decline"
    );
}

/// The pane and the plain prompt agree on the one case they share.
///
/// # They cannot be one function, and this says why rather than sharing a name
///
/// `tools::prompt::answer` reads a **typed line** — it trims, it accepts `yes`
/// as well as `y`, and it treats end of input as a no. The pane reads a
/// **keystroke**: there is no line to trim, no `yes` to spell, and no end of
/// input to see. So the two rules are two rules, and forcing them into one
/// would mean inventing an input model neither surface has.
///
/// What they share is the answer to "what is a yes", and that is asserted on
/// both sides here rather than assumed from the fact that both say `y`. The
/// decline arm is the one that discriminates: an implementation where
/// everything accepts would pass the first assertion alone.
#[test]
fn the_pane_and_the_plain_prompt_agree_on_what_a_yes_is() {
    use crate::tools::prompt::answer;

    assert!(answer(Some("y")), "the plain prompt does not accept `y`");
    assert!(!answer(Some("")), "the plain prompt accepts an empty line");
    assert!(!answer(None), "the plain prompt accepts end of input");

    let mut accepting = shell();
    accepting.ask(question_for_the_shell(&Question {
        statement: "run `rm -rf build`".to_owned(),
        prominent: false,
    }));
    let _ = accepting.key(
        press(Key::Char('y')),
        core::time::Duration::from_millis(1),
        &NotesTrie::nothing_cached(WORKSPACE),
        &Vocabulary,
    );
    assert_eq!(
        accepting.answer(),
        Some(true),
        "the pane does not accept `y`"
    );

    let mut declining = shell();
    declining.ask(question_for_the_shell(&Question {
        statement: "run `rm -rf build`".to_owned(),
        prominent: false,
    }));
    let _ = declining.key(
        press(Key::Enter),
        core::time::Duration::from_millis(1),
        &NotesTrie::nothing_cached(WORKSPACE),
        &Vocabulary,
    );
    assert_eq!(
        declining.answer(),
        Some(false),
        "the pane accepts the default, where the plain prompt declines an empty line"
    );
}

// ------------------------------------------------------- the tty branch itself

/// Only a session request opens a shell.
///
/// A shell that opened for `zaru runtime` would turn a question into a
/// session, and ADR-0010 D1 makes a session a directory on disk.
#[test]
fn only_a_session_request_opens_a_shell() {
    assert!(is_a_session(&Request::Continue));
    assert!(is_a_session(&Request::Resume {
        id: crate::session::SessionId::parse("01JQZX8N3K4M5P6R7S8T9V0W1X").expect("a ULID"),
    }));
    for request in [
        Request::Help,
        Request::Version,
        Request::Runtime,
        Request::Models,
        Request::Init,
        Request::SessionsList,
        Request::NotesTokens,
    ] {
        assert!(
            !is_a_session(&request),
            "{request:?} would have opened a shell"
        );
    }
}

/// The pump gives the terminal back and exits 0 on the leave word.
#[test]
fn the_pump_exits_zero_on_the_leave_word() {
    let (_, _, exit) = pump(typed("/exit"));
    assert_eq!(exit.code(), 0);
}

/// ADR-0005 D3's fast tier, driven through the real pump and read out of the
/// terminal's own buffer.
///
/// This is the check the deleted one said would have to change: it asserted
/// that nothing implemented the trie. Something does.
///
/// **The rows are read from the frame, never from the composer.** Every
/// expected value is a literal written here, so neither arm of the comparison
/// travels back through the code that painted it.
#[test]
fn a_populated_fast_tier_puts_its_matches_on_the_strip_as_the_user_types() {
    let trie = NotesTrie::attached_to(
        vec![
            entry(
                "architecture/bóunded",
                "Bóunded Contexts ✦",
                CachedKind::Page,
            ),
            entry("atoms/mémbrane", "Mémbrane ✦", CachedKind::Atom),
            entry("operations/tésting", "Tésting ✦", CachedKind::Page),
        ],
        WORKSPACE,
    );
    assert_eq!(trie.cached(), 3, "the check staged three entities");
    assert_eq!(
        trie.absence(),
        None,
        "a populated tier has nothing to apologise for"
    );

    let (_, surface, _) = pump_over(keys("mém"), &trie);
    let last = surface.frames.last().expect("the pump painted a frame");
    let strip = strip_rows(last);
    assert_eq!(
        strip,
        vec!["Mémbrane ✦"],
        "typing `mém` should put the one entity whose name begins with it on the strip; the \
         frame's composer rows were {strip:?}"
    );

    let (_, surface, _) = pump_over(keys("bó"), &trie);
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert_eq!(
        strip,
        vec!["Bóunded Contexts ✦"],
        "and a two-character prefix is served by the fast tier alone, which is ADR-0005 D1 row 4"
    );
}

/// An entry in another workspace is never shown, and the same entry in the
/// attached one is.
///
/// ADR-0006 D2 makes the attached workspace the composer's scope. The
/// accepting sibling moves the *attachment* rather than the entry, so a
/// filter that refused everything cannot pass: the corpus is byte-identical
/// across the two halves and only the workspace this session is attached to
/// changes.
#[test]
fn an_entry_from_another_workspace_is_never_shown_and_the_same_entry_attached_is() {
    let corpus = vec![
        CachedEntry::new(
            WORKSPACE,
            "architecture/bóunded",
            "Ours ✦",
            CachedKind::Page,
        ),
        CachedEntry::new(
            "aegis",
            "architecture/bóunded",
            "Theirs ✦",
            CachedKind::Page,
        ),
    ];

    // Typed against the PATH the two share, which is the whole point: the key
    // is identical in both workspaces, so nothing but the attachment can be
    // what separates them.
    let (_, surface, _) = pump_over(
        keys("architecture/bó"),
        &NotesTrie::attached_to(corpus.clone(), WORKSPACE),
    );
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert_eq!(
        strip,
        vec!["Ours ✦"],
        "the attached workspace's entity is shown and the other workspace's is not, though both \
         share a path; the composer rows were {strip:?}"
    );

    let (_, surface, _) = pump_over(
        keys("architecture/bó"),
        &NotesTrie::attached_to(corpus, "aegis"),
    );
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert_eq!(
        strip,
        vec!["Theirs ✦"],
        "and attaching to the other workspace shows its entity, so the rule is the attachment \
         rather than a refusal of everything: the composer rows were {strip:?}"
    );
}

/// With nothing cached the strip says why, where before it painted nothing.
///
/// This is what a user sees on every machine today, and it is the difference
/// this arc makes to the real artefact. The sentence is read out of the
/// terminal's buffer and compared against the constant the product spells
/// once.
#[test]
fn with_nothing_cached_the_strip_says_so_rather_than_going_blank() {
    let trie = NotesTrie::nothing_cached(WORKSPACE);
    assert_eq!(trie.cached(), 0);

    let (_, surface, _) = pump_over(keys("mém"), &trie);
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert_eq!(
        strip,
        vec![NOTHING_CACHED.to_owned()],
        "an empty fast tier must say why rather than paint nothing; the composer rows were \
         {strip:?}"
    );
    assert!(
        NOTHING_CACHED.chars().count() <= 72,
        "the line is {} characters and the shell's own frame is 72 columns wide, so a longer one \
         is clipped rather than wrapped",
        NOTHING_CACHED.chars().count()
    );
}

/// A slash line reaches neither the trie nor the strip, driven through the
/// real pump.
///
/// The composer's own check counts consultations; this one asserts the
/// consequence a user sees, over a tier that would have had something to say.
#[test]
fn a_slash_line_puts_nothing_on_the_strip_though_the_tier_could_have_answered() {
    let trie = NotesTrie::attached_to(
        vec![entry("nótes/one", "Nótes ✦", CachedKind::Page)],
        WORKSPACE,
    );

    let (_, surface, _) = pump_over(keys("nót"), &trie);
    assert_eq!(
        strip_rows(surface.frames.last().expect("a frame")),
        vec!["Nótes ✦"],
        "the tier answers this prefix, which is what makes the next half a statement about the \
         slash rather than about an empty trie"
    );

    let (_, surface, _) = pump_over(keys("/nót"), &trie);
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert!(
        strip.is_empty(),
        "`/nót` is a command line and ADR-0015 D2 decides that before the strip sees a \
         keystroke; the composer rows were {strip:?}"
    );
}

/// What the strip renders is what the trie holds, byte for byte.
///
/// **The strip is deliberately not a redaction seam**, confirmed 2026-09-05
/// under directive 20 and recorded on ADR-0008's clause 6 Update. That clause
/// puts a `Redactor` on "every path from captured bytes into a model prompt";
/// a hint strip is neither, and the titles are the user's own notes read back
/// to them. ADR-0005's own Positive section is "The user *sees* what their
/// cortex knows", and a strip that altered a user's own note titles in front
/// of them would be the opposite of it.
///
/// So the assertion is identity and the mutant is **any transform at all**.
/// The staged title carries a value shaped like a held secret, and it is
/// asserted present rather than absent — which is the arm that would fail if
/// somebody wired a redactor in here.
#[test]
fn what_the_strip_renders_is_what_the_trie_holds_byte_for_byte() {
    let title = "nn_mcp_ábcd1234 · a nóte of mine ✦";
    let trie = NotesTrie::attached_to(
        vec![entry("nótes/awkward", title, CachedKind::Page)],
        WORKSPACE,
    );

    let (_, surface, _) = pump_over(keys("nót"), &trie);
    let strip = strip_rows(surface.frames.last().expect("a frame"));
    assert_eq!(
        strip,
        vec![title.to_owned()],
        "the strip must render the cached title unchanged; a transform of any kind — redaction, \
         truncation, case folding — moves this comparison. The composer rows were {strip:?}"
    );
}

// ---------------------------------------------------------------------------
// ADR-0013 D3 and D4 — the two announcement lines, in the pane
// ---------------------------------------------------------------------------

/// D3's line, transcribed: `◈ compacted 34 earlier turns · 18.2k → 2.1k
/// tokens · full history in transcript`.
///
/// The marker is asserted to come from the **register** rather than from the
/// text, which is ADR-0008 D3's split — "the loop emits it; the terminal does
/// not reach in" — read from the other side.
///
/// The mutant: emitting `◈` in `render::announcement` as well, which doubles
/// it on the rendered line.
#[test]
fn a_compaction_announces_itself_in_the_panes_announcement_register() {
    use zaru_core::context::{Announcement, Compaction};
    use zaru_tui::shell::port::Register;

    let transcript =
        crate::terminal::vocabulary::Transcript::of(&[crate::session::Record::Compacted(
            Compaction {
                announcements: vec![Announcement::Compacted {
                    turns: 34,
                    before: 18_200,
                    after: 2_100,
                }],
                raw: None,
            },
        )]);
    let lines = zaru_tui::shell::port::TranscriptSource::lines(&transcript);

    assert_eq!(
        lines.len(),
        1,
        "one announcement is one line; got {lines:?}"
    );
    assert_eq!(
        lines[0].register,
        Register::Announced,
        "ADR-0002 D3 puts a compaction on the interrupt channel and ADR-0013 D3 opens its line \
         with the announcement marker, so it belongs in the announcement register"
    );
    assert_eq!(
        lines[0].text,
        "compacted 34 earlier turns · 18.2k → 2.1k tokens · full history in transcript",
        "ADR-0013 D3 spells this line and every word of it is the record's"
    );
    assert!(
        !lines[0].text.contains('◈'),
        "the glyph is the register's -- `zaru-tui`'s own contract is that the shell \"chooses the \
         glyph and nothing else\" -- so a producer that emitted one would put it on the line twice"
    );
}

/// D4's line, transcribed: `◈ dropped attachment: adrs/0117-aegis-edge-mode ·
/// re-attach with [[`.
///
/// Both parts of the identity are asserted present, because `ItemId`'s own
/// documentation is that "two workspaces may each hold
/// `architecture/bounded-contexts` and they are different pages".
///
/// The mutant: rendering `identity.path()` alone, which reddens the workspace
/// assertion.
#[test]
fn a_dropped_attachment_names_its_workspace_its_path_and_how_to_get_it_back() {
    use zaru_core::context::{Announcement, Compaction, ItemId};

    let transcript =
        crate::terminal::vocabulary::Transcript::of(&[crate::session::Record::Compacted(
            Compaction {
                announcements: vec![Announcement::AttachmentDropped {
                    identity: ItemId::new("adrs", "0117-aegis-edge-mode")
                        .expect("both parts are named"),
                    how_to_reattach: "re-attach with [[".to_owned(),
                }],
                raw: None,
            },
        )]);
    let lines = zaru_tui::shell::port::TranscriptSource::lines(&transcript);

    assert_eq!(
        lines[0].text, "dropped attachment: adrs/0117-aegis-edge-mode · re-attach with [[",
        "ADR-0013 D4 spells this line, and the workspace is part of the identity rather than \
         decoration"
    );
}

/// One `compact` call can announce several times — a layer-6 summary and then
/// one line per attachment D4 had to drop — and the pane shows all of them.
///
/// This is why `lines_for` returns many. D4: "Removing their choice without
/// telling them is worse than running out", so a renderer that showed the
/// first announcement and dropped the rest would drop exactly the lines that
/// clause exists to guarantee.
///
/// The mutant: taking `announcements.first()` instead of mapping them all.
#[test]
fn every_announcement_of_one_compaction_reaches_the_pane() {
    use zaru_core::context::{Announcement, Compaction, ItemId};

    let dropped = |path: &str| Announcement::AttachmentDropped {
        identity: ItemId::new("adrs", path).expect("both parts are named"),
        how_to_reattach: "re-attach with [[".to_owned(),
    };
    let transcript =
        crate::terminal::vocabulary::Transcript::of(&[crate::session::Record::Compacted(
            Compaction {
                announcements: vec![
                    Announcement::Compacted {
                        turns: 2,
                        before: 900,
                        after: 40,
                    },
                    dropped("0117-aegis-edge-mode"),
                    dropped("0118-something-else"),
                ],
                raw: None,
            },
        )]);
    let lines = zaru_tui::shell::port::TranscriptSource::lines(&transcript);

    assert_eq!(
        lines.len(),
        3,
        "three announcements came out of one compaction and all three are lines the user is owed; \
         the pane rendered {}: {lines:?}",
        lines.len()
    );
    assert!(
        lines[2].text.contains("0118-something-else"),
        "the last announcement is the one a one-line-per-record renderer would lose; the pane \
         showed {:?}",
        lines[2].text
    );
}

/// ADR-0013 D1's layer 6 is "conversation **and tool results**", and the tool
/// results are on the event stream rather than in `Ran`, which carries what
/// the turn *printed*.
///
/// `ToolLines` is the third consumer of ADR-0008 clause 3's one emission, and
/// it keeps exactly the lines the shell's own vocabulary puts in the call
/// register — so what reaches layer 6 and what the pane painted are the same
/// bytes rather than two renderings.
///
/// The mutant: collecting every register rather than `Call`, which sweeps the
/// model's own narration into layer 6 twice; and collecting none, which
/// reddens the count.
#[test]
fn a_turns_tool_lines_are_collected_for_layer_six_and_nothing_else_is() {
    use zaru_core::tool_call::{Event, EventSink, TurnEnding};

    let mut collector = crate::compose::ToolLines::default();
    let elapsed = core::time::Duration::from_millis(5);
    for event in [
        Event::TurnStarted { n: 1, of: 8 },
        Event::ModelResponded {
            round: 1,
            tokens: 400,
            calls: 1,
            elapsed,
        },
        Event::ToolRequested {
            round: 1,
            call: 1,
            name: "fs.read".to_owned(),
        },
        Event::ToolCompleted {
            round: 1,
            call: 1,
            name: "fs.read".to_owned(),
            failed: false,
            content_bytes: 82,
            elapsed,
        },
        Event::TurnEnded {
            n: 1,
            ending: TurnEnding::Answered,
            rounds: 1,
            elapsed,
        },
    ] {
        collector.emit(&event);
    }

    let lines = collector.taken();
    assert!(
        lines.iter().any(|line| line.contains("fs.read")),
        "a tool call's rendered line is what layer 6 owes the next turn; got {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("turn 1")),
        "the turn's own narration is not a tool result, and sweeping it in would put the pane's \
         commentary into the next turn's prompt: {lines:?}"
    );
    assert!(
        collector.taken().is_empty(),
        "taking twice must not repeat a turn's tool lines into the turn after it"
    );
}

// ---------------------------------------------- the asynchronous terminal source

/// The reader thread stops and is joined when the source is dropped.
///
/// ADR-0005's and ADR-0008's gap paragraphs asked for "a source that could be
/// polled beside the turn", and a source with a thread behind it owes one
/// thing back: **the thread cannot outlive the shell.** A detached reader
/// would keep the terminal's event source open after the guard gave the
/// terminal back, and whatever read standard input next would be racing it.
///
/// The body here is not the terminal's — see `Source::over` for why the two
/// are separate — so what this asserts is the flag, the join and the ordering,
/// which is all of the lifecycle.
///
/// # Why the body sleeps, when nothing else in this module does
///
/// The mutant is *drop the join and keep the flag*, and it is the shape
/// library verification-lessons §57 is written about: with a body that ends
/// the instant it sees the flag, a detached drop still usually observes it
/// ended, so the watch is a coin and a green run says nothing. **That mutation
/// survived the first form of this check**, which is how the fixture below
/// came to be written this way.
///
/// So the shutdown path is made slow *on purpose* and by a definite amount.
/// The sleep is in the fixture, never in an assertion: what is asserted is an
/// ordering — did `drop` return before the body finished — and the mutant
/// turns a `Duration` of `SHUTDOWN` into one of microseconds, five orders of
/// magnitude apart. That is a defect made certain rather than likelier, which
/// is what §57 asks for.
#[test]
fn the_reader_thread_is_stopped_and_joined_when_the_source_is_dropped() {
    use core::sync::atomic::{AtomicBool, Ordering};

    /// Long enough that no scheduler confuses "joined" with "raced past".
    const SHUTDOWN: core::time::Duration = core::time::Duration::from_millis(300);

    let ended = Arc::new(AtomicBool::new(false));
    let watched = Arc::clone(&ended);
    let source = Source::over(move |sender, stop| {
        // One key first, so the check knows the thread really ran rather than
        // returning before it started.
        let _ = sender.send(press(Key::Char('z')));
        while !stop.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }
        std::thread::sleep(SHUTDOWN);
        watched.store(true, Ordering::Release);
    });

    // Staging: the thread is alive and has produced something. A real runtime,
    // because this genuinely waits on another thread -- `futures_lite_block_on`
    // is the helper for adapters that never yield and it says so when they do.
    let runtime = crate::compose::turn::runtime().expect("a runtime");
    assert_eq!(
        runtime.block_on(source.next()),
        Some(press(Key::Char('z'))),
        "the reader thread produced nothing, so this check would assert its exit without ever \
         having asserted its entry"
    );
    assert!(
        !ended.load(Ordering::Acquire),
        "the reader thread ended before the source was dropped, so dropping it proves nothing"
    );

    let started = std::time::Instant::now();
    drop(source);
    let waited = started.elapsed();

    assert!(
        ended.load(Ordering::Acquire),
        "`Source`'s drop returned while the reader thread was still running, so nothing joins it \
         and it can outlive the shell; the drop took {waited:?} against a shutdown path of \
         {SHUTDOWN:?}"
    );
}

/// A drained script ends; a source with keys left does not.
///
/// The two answers a synchronous reader has to tell apart, asserted together
/// so neither can be read as the other. `Taken::Ended` is what makes a
/// confirmation refuse rather than decline.
#[test]
fn a_source_says_nothing_yet_and_never_again_in_different_words() {
    let source = Source::scripted(vec![press(Key::Char('y'))]);
    assert_eq!(source.try_next(), Taken::Key(press(Key::Char('y'))));
    assert_eq!(
        source.try_next(),
        Taken::Ended,
        "a drained script must end, or a pane out of keys would wait for ever"
    );

    // A live reader that has sent nothing yet says `Nothing`, which is the
    // answer a beat is painted through.
    let waiting = Source::over(|_sender, stop| {
        while !stop.load(core::sync::atomic::Ordering::Acquire) {
            std::hint::spin_loop();
        }
    });
    assert_eq!(
        waiting.try_next(),
        Taken::Nothing,
        "a source whose reader has sent nothing must not report that it has ended"
    );
}

/// The pane keeps painting while a question stands and nothing has arrived.
///
/// ADR-0011 D3's prompt is answered in the pane, and until 2026-09-05 the pane
/// stopped while it waited. The beat is what makes the wait a repaint, and it
/// is counted rather than timed: `Held` returns at once, so this check cannot
/// pass or fail on how the machine scheduled.
#[test]
fn a_standing_question_paints_on_every_beat_it_waits() {
    use crate::tools::port::Confirm as _;
    use core::sync::atomic::Ordering;

    // A reader that answers only after three beats have been waited, so the
    // loop is asserted to paint *through* the wait rather than once at the
    // start. The interesting element is in the middle of the run, not at
    // either end of it (library verification-lessons §54).
    let beats = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&beats);
    let source = Source::over(move |sender, _stop| {
        while counted.load(Ordering::SeqCst) < 3 {
            std::hint::spin_loop();
        }
        let _ = sender.send(press(Key::Char('y')));
    });

    /// A beat that reports into the counter the reader above is watching.
    #[derive(Debug)]
    struct Counted(Arc<AtomicUsize>);
    impl crate::terminal::source::Pace for Counted {
        fn wait(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn elapse(&self) -> impl Future<Output = ()> + Send {
            self.0.fetch_add(1, Ordering::SeqCst);
            core::future::ready(())
        }
    }

    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let pace = Counted(Arc::clone(&beats));
    let painted = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let confirm = PaneConfirm::over(&pane, &source, &pace);
        confirm
            .confirm(&Question {
                statement: "write build/out.txt".to_owned(),
                prominent: false,
            })
            .expect("the pane answered")
    };
    assert!(painted, "`y` was read as a decline");

    assert!(
        beats.load(Ordering::SeqCst) >= 3,
        "the confirmation waited {} beat(s); the reader answers only after three, so a loop that \
         blocked on the channel instead of pacing could not have got here",
        beats.load(Ordering::SeqCst)
    );
    // One frame for the question, then one per beat waited, then one for the
    // answer. The floor is what discriminates a loop that painted once.
    assert!(
        surface.frames.len() >= beats.load(Ordering::SeqCst),
        "the pane painted {} frame(s) across {} beat(s), so it is not repainting while it waits",
        surface.frames.len(),
        beats.load(Ordering::SeqCst)
    );
    assert_eq!(
        source.contended(),
        0,
        "the source was contended {} time(s), which a single-threaded confirmation cannot do",
        source.contended()
    );
}

// -------------------------- the pane repaints and reads keys while a turn runs

/// A future that finishes only once the check lets it, so a turn can be held
/// suspended for exactly as long as the check needs and not one beat longer.
///
/// **It is not a timer.** `poll` reads a flag, so what makes it finish is
/// something the check did rather than something the machine scheduled
/// (library verification-lessons §57).
struct HeldOpen {
    release: Arc<std::sync::atomic::AtomicBool>,
    finished: Arc<std::sync::atomic::AtomicBool>,
}

impl Future for HeldOpen {
    type Output = &'static str;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        context: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Self::Output> {
        if self.release.load(Ordering::SeqCst) {
            self.finished.store(true, Ordering::SeqCst);
            core::task::Poll::Ready("the turn finished")
        } else {
            // Wake immediately: this stands in for a provider await, which is
            // woken by a socket rather than by a clock, and a future that
            // never re-armed would stall the whole `select!`.
            context.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

/// A beat that releases the held turn once its gate opens.
///
/// Deterministic in both directions: the turn cannot finish before the gate is
/// open and the beats counted, and it cannot fail to finish after.
#[derive(Debug)]
struct Releasing {
    beats: Arc<AtomicUsize>,
    after: usize,
    gate: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::terminal::source::Pace for Releasing {
    fn wait(&self) {
        self.count();
    }

    /// **Counted when polled, not when created**, and that is the whole of why
    /// this is a `poll_fn` rather than a `ready`.
    ///
    /// `tokio::select!` evaluates every branch's expression before it polls
    /// any of them, so a beat that counted at construction counted on the very
    /// iteration the turn won — which released the turn before the terminal
    /// branch had ever been polled, and the first form of these checks read an
    /// empty composer because of it. A beat is a beat that was *waited*.
    fn elapse(&self) -> impl Future<Output = ()> + Send {
        let beats = Arc::clone(&self.beats);
        let gate = Arc::clone(&self.gate);
        let release = Arc::clone(&self.release);
        let after = self.after;
        core::future::poll_fn(move |_| {
            let waited = beats.fetch_add(1, Ordering::SeqCst) + 1;
            if waited >= after && gate.load(Ordering::SeqCst) {
                release.store(true, Ordering::SeqCst);
            }
            core::task::Poll::Ready(())
        })
    }
}

impl Releasing {
    fn count(&self) {
        let beats = self.beats.fetch_add(1, Ordering::SeqCst) + 1;
        if beats >= self.after && self.gate.load(Ordering::SeqCst) {
            self.release.store(true, Ordering::SeqCst);
        }
    }
}

/// Everything a race check needs, staged together.
struct Raceable {
    beats: Arc<AtomicUsize>,
    release: Arc<std::sync::atomic::AtomicBool>,
    finished: Arc<std::sync::atomic::AtomicBool>,
}

impl Raceable {
    /// A turn released once `gate` is open and `after` beats have been waited.
    ///
    /// **This is what makes a check over a live reader deterministic rather
    /// than a coin.** `race` is `biased` and polls the terminal before the
    /// beat, so a buffered key is always taken before a beat fires; therefore
    /// the first beat after the reader has finished sending is a beat at which
    /// the channel is provably drained. The gate is that "has finished
    /// sending", set by the reader thread itself.
    fn gated(after: usize, gate: Arc<std::sync::atomic::AtomicBool>) -> (Self, Releasing) {
        let beats = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pace = Releasing {
            beats: Arc::clone(&beats),
            after,
            gate,
            release: Arc::clone(&release),
        };
        (
            Self {
                beats,
                release,
                finished: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            },
            pace,
        )
    }

    fn turn(&self) -> HeldOpen {
        HeldOpen {
            release: Arc::clone(&self.release),
            finished: Arc::clone(&self.finished),
        }
    }
}

/// A source that sends `keys` and then stays open, which is what a terminal
/// does.
///
/// A `Source::scripted` ends the moment it is drained, and `race` is `biased`
/// -- so a drained script wins the race before a single beat can fire, and a
/// check staged over one would be asserting `SourceEnded` rather than anything
/// about a suspended turn. Returns the gate the reader opens when it has sent
/// everything.
fn live_source(keys: Vec<zaru_tui::shell::Input>) -> (Source, Arc<std::sync::atomic::AtomicBool>) {
    let sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let opened = Arc::clone(&sent);
    let source = Source::over(move |sender, stop| {
        for key in keys {
            let _ = sender.send(key);
        }
        opened.store(true, Ordering::SeqCst);
        while !stop.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    });
    (source, sent)
}

/// The row the composer's input sits on, out of a painted frame.
fn input_row(frame: &[String]) -> String {
    frame[frame.len() - usize::from(COMPOSER_ROWS)]
        .trim_end()
        .to_owned()
}

/// The pane repaints while the turn is suspended.
///
/// This is the capability ADR-0005's and ADR-0008's gap paragraphs named:
/// "while the model is thinking the pane does not repaint". The turn here is
/// held open for five beats and the frame count is read across them, so what
/// is asserted is that painting happened *during* the wait rather than around
/// it. Nothing on the pane changes on a bare beat -- see `TICK` -- which is
/// why the assertion is on the count and not on the contents.
///
/// The mutant: remove the beat's branch from the `select!`.
#[test]
fn the_pane_repaints_while_a_turn_is_suspended() {
    let (source, sent) = live_source(Vec::new());
    let (staged, pace) = Raceable::gated(5, sent);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    let raced = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            staged.turn(),
        ))
    };

    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );
    assert!(
        staged.finished.load(Ordering::SeqCst),
        "the staged turn never ran, so this check asserted nothing about a suspended one"
    );
    // **A floor, not an equality**, and for the reason the interrupt check
    // above records: the gate is opened by a thread, so a beat can fire before
    // it is open and the count then runs past five. Five is the minimum the
    // turn was held for; the equality below is the property, and it is
    // deterministic because every beat that is polled paints.
    assert!(
        staged.beats.load(Ordering::SeqCst) >= 5,
        "the race waited {} beat(s) rather than at least the five the turn was held for",
        staged.beats.load(Ordering::SeqCst)
    );
    // **An equality, not a floor.** Every beat that was waited is a beat at
    // which the turn was still suspended, and every one of them paints; the
    // mutant that removes the beat's branch paints none. A floor would also
    // have passed on a pane that painted once and stopped.
    assert_eq!(
        surface.frames.len(),
        staged.beats.load(Ordering::SeqCst),
        "the pane painted {} frame(s) across {} beat(s) of a suspended turn, so it is not \
         repainting while the model thinks",
        surface.frames.len(),
        staged.beats.load(Ordering::SeqCst)
    );
}

/// A keystroke during a turn reaches the composer and starts no second turn.
///
/// Two halves, and the second is the one ADR-0015's ruling of 2026-09-05 owes:
/// the text is **not lost** -- it is on the input row, painted at the moment it
/// was read -- and it is **not executed as a task**, because `Enter` is
/// refused with `BUSY` rather than submitted or queued. The composed line
/// survives the refusal.
///
/// The mutants: route the mid-turn key to nothing; let `Enter` reach the
/// composer; let `Enter` reach `Shell::key`; clear the composer on the refusal.
#[test]
fn a_keystroke_during_a_turn_is_neither_lost_nor_executed_as_a_task() {
    // Held open until the source is drained: the beat count is far past what
    // the four keys need, so the keys are read while the turn is genuinely
    // suspended rather than after it finished.
    let mut typing = keys("saffron");
    typing.push(press(Key::Enter));
    let (source, sent) = live_source(typing);
    // One beat after the reader has finished sending, which `race`'s `biased`
    // ordering makes a beat at which every key has already been read.
    let (staged, pace) = Raceable::gated(1, sent);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    let raced = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            staged.turn(),
        ))
    };
    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );

    // Not lost: the text a user typed while waiting is in the composer.
    assert_eq!(
        shell.composer().text(),
        "saffron",
        "the line typed during the turn is not in the composer; it reads {:?}",
        shell.composer().text()
    );
    let last = surface.frames.last().expect("no frame was painted");
    assert!(
        input_row(last).contains("saffron"),
        "the line typed during the turn never reached the input row: {:?}",
        input_row(last)
    );

    // Not executed as a task, and not queued: the notice ADR-0015's ruling
    // names is on the pane, and the composer still holds the line.
    let pane_lines: Vec<String> = shell
        .pane_lines()
        .iter()
        .map(zaru_tui::shell::Line::painted)
        .collect();
    assert!(
        pane_lines
            .iter()
            .any(|line| line.contains(crate::terminal::driver::BUSY)),
        "`Enter` during a turn did not produce the refusal ADR-0015's ruling of 2026-09-05 \
         requires; the pane holds {pane_lines:?}"
    );
}

/// `Ctrl-C` during a turn leaves, and leaving drops the turn's future.
///
/// **The two are one act, which is the whole of what a mid-turn interrupt is
/// by the records' words.** ADR-0015's ruling gives this key one meaning --
/// it leaves, at ADR-0016 D5's `0` -- and dropping the future is ADR-0010 D2's
/// "a crash loses at most the event in flight" without a crash: whatever the
/// turn had already written is on disk and nothing after it is.
///
/// So the assertion is in two parts. The race reports `Interrupted`, and the
/// staged turn **never finished** -- it was dropped, not awaited to a value.
/// The interesting keystroke is staged in the middle of the run, with keys on
/// each side of it, so the check cannot be satisfied by *any* key ending the
/// race (library verification-lessons §54).
#[test]
fn ctrl_c_during_a_turn_leaves_and_the_turns_future_is_dropped() {
    // **Released after twenty beats, not never**, and that is a lesson rather
    // than a detail. The first form of this check held the turn open for ever
    // so that only the interrupt could end the race — and the mutant that
    // ignores the interrupt then span for ever, painting frames into a `Vec`
    // that reached six gigabytes on a machine shared with other builds. A
    // mutant that hangs is not a red; it is a hazard. So the turn is released
    // on a beat that cannot fire until every key has been read (`race` is
    // `biased`), and an ignored interrupt therefore reports `Ran` and fails
    // the assertion below in milliseconds.
    let (source, sent) = live_source(vec![
        press(Key::Char('h')),
        press(Key::Char('i')),
        zaru_tui::shell::Input {
            key: Key::Char('c'),
            ctrl: true,
            alt: false,
            shift: false,
        },
        press(Key::Char('x')),
        press(Key::Enter),
    ]);
    let (staged, pace) = Raceable::gated(20, sent);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    let raced = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            staged.turn(),
        ))
    };

    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Interrupted(zaru_tui::shell::Leaving::Interrupt),
        "`Ctrl-C` during a turn did not leave"
    );
    assert!(
        !staged.finished.load(Ordering::SeqCst),
        "the turn's future ran to completion, so it was awaited rather than dropped and nothing \
         was interrupted"
    );
    // Staging: the two keys before the interrupt were read, so the race was
    // genuinely running rather than ending on its first poll. **This is the
    // staging assertion and there is no second one**, which is a correction
    // rather than an omission: an earlier form also asserted that no beat had
    // been waited before the interrupt, and that is a race, not a property.
    // The reader is a thread, so on a loaded machine the select can poll
    // before it has sent and a beat fires first; the interrupt still wins and
    // the check still means what it says. **CI found it and this machine did
    // not** -- run 33975304551 on the GitHub runner, "the race waited 1
    // beat(s) before the interrupt", against green here every time. A
    // probabilistic assertion is the failure library verification-lessons §57
    // names, arriving in a check rather than in a mutation. What the composer
    // holds is the deterministic form of the same claim: those keys can only
    // have got there through `read_while_busy`, which only runs while the
    // turn is suspended.
    assert_eq!(
        shell.composer().text(),
        "hi",
        "the keys before the interrupt did not reach the composer, so the interrupt ended a race \
         that had not started"
    );
    // ADR-0016 D5's `0`, through the one function the pump uses.
    assert_eq!(zaru_tui::shell::Leaving::Interrupt.code(), 0);
}

// ---------------------------------------------------------------------------
// ADR-0013 clause 5 and ADR-0012 clause 6 — the two numbers on ADR-0001 D2's row
// ---------------------------------------------------------------------------

/// Limits a check can actually cross, so the threshold is reachable.
///
/// The product's window is `CONTEXT_WINDOW_TOKENS`, 1,048,576, and its
/// threshold three quarters of that. A check that filled 786 KB of layer 6 to
/// watch a number move would be measuring the machine. These are
/// `summariser_from_outside`'s own numbers, for the same reason it chose them.
fn crossable() -> zaru_core::context::ContextLimits {
    zaru_core::context::ContextLimits::new(
        zaru_core::context::ContextWindow::new(8_000).expect("not zero"),
        zaru_core::context::PressureThreshold::new(900).expect("not zero"),
    )
    .expect("the threshold is below the window")
}

/// A summariser that answers a fixed sentence, so a compaction can be driven
/// with no provider.
struct Staged;

impl zaru_core::context::Summariser for Staged {
    async fn summarise(
        &self,
        _span: &zaru_core::context::Span,
    ) -> Result<String, zaru_core::iteration::PortFailure> {
        Ok("the constraints so far".to_owned())
    }
}

/// The context number rises as a session holds more, and falls when a
/// compaction relieves it.
///
/// **A seam check, and it says so.** It drives `SessionContext` and
/// `refresh_status` directly rather than the binary: `terminal::open` opens a
/// context with the product's own limits and nothing a person types crosses
/// 786k, so the crossing this asserts is reachable at this seam and is *not*
/// claimed of `zaru --resume`.
///
/// **One arm of every comparison is outside the renderer.** The expected
/// numbers are read off `SessionContext::usage` itself and abbreviated by
/// `render::thousands`, so a renderer that agreed with itself could not
/// satisfy this (\[Verification lessons\] §10 and §11). What the check adds is
/// that the painted row carries them, and that the second number is *smaller*.
///
/// The mutant: caching the first usage, so the row never moves.
#[test]
fn the_context_number_on_the_row_rises_with_a_session_and_falls_on_a_compaction() {
    use zaru_core::context::Exchange;

    let redactor = Nothing;
    let mut context =
        crate::compose::SessionContext::opened(crate::compose::prefix_for(), crossable());
    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));

    crate::terminal::driver::refresh_status(&mut shell, &context, None, &redactor);
    let opened = context.usage(&redactor).used();

    for nth in 0..8 {
        context.record(Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }
    crate::terminal::driver::refresh_status(&mut shell, &context, None, &redactor);
    let loaded = context.usage(&redactor).used();
    let before = painted_row(&shell);

    assert!(
        loaded > opened,
        "the staging must actually load the context: it went {opened} -> {loaded}"
    );
    assert!(
        loaded > 900,
        "the staging must cross the pressure threshold of 900, or the compaction below is a \
         no-op and this check passes having compacted nothing; it reached {loaded}"
    );
    assert!(
        before.contains(&crate::cli::render::thousands(loaded)),
        "the row must carry what the context now holds; it was {before:?}"
    );

    let compaction = futures_lite_block_on(context.at_turn_boundary(&Staged, &redactor))
        .expect("the staged summariser answers");
    assert!(
        !compaction.announcements.is_empty(),
        "a compaction that announced nothing did not happen, and the fall below would be measuring \
         nothing"
    );

    crate::terminal::driver::refresh_status(&mut shell, &context, None, &redactor);
    let relieved = context.usage(&redactor).used();
    let after = painted_row(&shell);

    assert!(
        relieved < loaded,
        "ADR-0013 D2 replaces the oldest span with a summary, so the number must fall: it went \
         {loaded} -> {relieved}"
    );
    assert!(
        after.contains(&crate::cli::render::thousands(relieved)),
        "the row must carry the relieved number after the compaction; it was {after:?}"
    );
    assert_ne!(
        before, after,
        "a row that reads the same before and after a compaction is not carrying the number"
    );
}

/// ADR-0012 D7's token line reaches the row, and it is the same string the
/// session prints on exit.
///
/// D7 asks for the numbers "per turn in the status line, per session on exit",
/// and two spellings of one register are two things that can disagree. So the
/// row's segment is asserted to be `render::usage`'s own output for the same
/// datum — not a lookalike composed beside it.
///
/// **What the value is, said rather than rounded up:** `Provider::usage`
/// reports the *last exchange*, so a turn that made six model calls puts the
/// sixth on this row. That is this record's own **proposed** Update of
/// 2026-09-05 and it is human-owned; nothing here sums.
///
/// The mutant: composing a second spelling in `refresh_status`.
#[test]
fn the_token_segment_is_the_line_the_session_prints_on_exit_and_not_a_second_spelling() {
    let redactor = Nothing;
    let context = crate::compose::SessionContext::opened(crate::compose::prefix_for(), crossable());
    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));
    let usage = crate::providers::TokenUsage::counted(390, 79);

    crate::terminal::driver::refresh_status(&mut shell, &context, Some(&usage), &redactor);

    assert_eq!(
        shell.status().tokens.as_deref(),
        Some(crate::cli::render::usage(&usage).as_str()),
        "the row's token segment must BE the exit line, so the two cannot disagree about a word"
    );
    assert!(
        painted_row(&shell).contains("tokens: 390 prompt + 79 completion = 469"),
        "and it must reach the painted buffer; the row was {:?}",
        painted_row(&shell)
    );
}

/// Before any exchange the token segment is absent, and the context segment is
/// not.
///
/// The two are absent for different reasons and only one of them clears: a
/// session has a context from the moment it opens, which is D6's "visible all
/// along", while `Provider::usage` answers `None` until a request has been
/// made because "a client that had made no request and reported a zero would
/// be inventing a datum".
///
/// The mutant: rendering a zero token line when there is no usage.
#[test]
fn a_session_that_has_not_asked_anything_shows_a_context_and_no_tokens() {
    let redactor = Nothing;
    let context = crate::compose::SessionContext::opened(crate::compose::prefix_for(), crossable());
    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));

    crate::terminal::driver::refresh_status(&mut shell, &context, None, &redactor);

    assert_eq!(
        shell.status().tokens,
        None,
        "no exchange has happened, so there is no token count to report and a zero would be \
         invented"
    );
    assert!(
        shell.status().context.is_some(),
        "a session has a context from the frame it opens on, which is D6's 'visible all along'"
    );
    let row = painted_row(&shell);
    assert!(
        row.contains("context ") && !row.contains("tokens:"),
        "the row must carry the context segment and no token segment; it was {row:?}"
    );
}

/// The status row as a person would see it, out of a painted frame.
///
/// Read from `TestBackend` rather than from `Status::painted`, because the
/// claim every check above makes is that a *user* meets the number: a row
/// composed correctly and dropped by the renderer would satisfy a string
/// comparison against the formatter.
fn painted_row(shell: &Shell) -> String {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(200, 8)).expect("test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}
// ------------------------------- ADR-0011 D4, rendered where a person reads it

/// An out-of-tree call renders distinctly on the painted pane, at `yolo` —
/// [ADR-0011] D4 and its trigger clause 4.
///
/// # What was missing, and it was not the marking
///
/// D4: anything above the working directory "prompts in `ask` and `allow`, and
/// **it renders differently in the transcript at every mode including
/// `yolo`**". Clause 4 asks for that, asserted. The record's own Status
/// tracking has said since `fs-tools` that "the distinction exists and is
/// asserted at every mode … what is missing is **the rendering**", which
/// "still waits on `zaru-tui`".
///
/// It was not waiting on a marking. `TranscriptEntry::render` has appended
/// `Placement::as_str` since the tool surface landed, and
/// `tools::execute` puts that same string on the event a turn emits:
/// `statement` is `decision.question().map_or_else(|| entry.render(), |q|
/// q.statement)`, and `Decision::question` is `format!("Allow {}?",
/// self.entry.render())` — so at **every** mode, prompt or no prompt, the
/// marking is inside the statement. What was missing is any assertion that it
/// survives to the frame a person actually reads.
///
/// # The property is text in the buffer, and deliberately not a colour
///
/// Read out of `TestBackend`'s cells. **No register and no colour is claimed
/// as the distinction**: no record gives an out-of-tree call one, inventing a
/// seventh register would be authoring, and a colour is not something this
/// check could read anyway. What it reads is `Placement::as_str`'s own words,
/// taken from that constant rather than retyped, so renaming the marking moves
/// this check with it.
///
/// **Both arms.** An assertion that only looked for the marking would be
/// satisfied by a renderer that marked everything, which is the same pair
/// `tools::tests::an_out_of_tree_call_renders_differently_from_an_ordinary_one`
/// holds one layer down — that one on the entry, this one on the frame.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn an_out_of_tree_call_renders_distinctly_on_the_frame_at_yolo() {
    use crate::tools::decision::{Assessment, Decision, Invocation};
    use crate::tools::fixtures::ScratchTree;
    use crate::tools::mode::Mode;
    use crate::tools::name::ToolName;
    use crate::tools::tree::{Placement, WorkingDirectory};

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let inside = working.classify("notes.txt");
    let outside = working.classify("../elsewhere/secret");

    // The statement exactly as `tools::execute` composes it, through the real
    // decision, at `yolo` -- the mode that removes the prompt and, D4 says,
    // never removes the record.
    let statement_for = |target: &crate::tools::tree::Target| {
        let decision = Decision::reach(
            Mode::Yolo,
            &Invocation::on_path(ToolName::FsRead, target).expect("addresses a path"),
            Assessment::default(),
        );
        decision
            .question()
            .map_or_else(|| decision.entry().render(), |question| question.statement)
    };

    let frame_for = |target: &crate::tools::tree::Target| {
        let mut shell = shell();
        let restores: Restores = Arc::new(AtomicUsize::new(0));
        // Wide enough for the whole line: the target is a scratch directory's
        // absolute path, and at the default 72 the marking is truncated off
        // the frame. That truncation is real and is recorded on ADR-0011 as a
        // finding -- a deep enough path hides D4's marking from a narrow
        // terminal -- but it is a question about the line's shape, which no
        // record gives, and not about whether this renderer marks the call.
        let mut surface = Recording::wide(Arc::clone(&restores), 200);
        {
            let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
            let mut sink = PaneSink::over(&pane);
            zaru_core::tool_call::EventSink::emit(
                &mut sink,
                &zaru_core::tool_call::Event::ToolPermissionDecided {
                    round: 1,
                    call: 1,
                    statement: statement_for(target),
                    permitted: true,
                },
            );
        }
        surface
            .frames
            .last()
            .expect("a frame was painted")
            .join("\n")
    };

    let marking = Placement::OutOfTree.as_str();
    let escaping = frame_for(&outside);
    let ordinary = frame_for(&inside);

    assert!(
        escaping.contains(marking),
        "a call outside the working directory is not marked on the frame at `yolo`; \
         ADR-0011 D4 requires it to render differently at every mode including this \
         one, and clause 4 asks for exactly this assertion:\n{escaping}"
    );
    assert!(
        !ordinary.contains(marking),
        "an ordinary in-tree call was marked as having left the tree, so the marking \
         says nothing:\n{ordinary}"
    );
}

/// The two once-ever lines read back in two registers, each the record's own.
///
/// The mutant this catches is one register for both. ADR-0002 D8 puts an
/// event-anchored recommendation "in the same visual register as a SEAL
/// verdict or a learning line", which is `Announced`; ADR-0011 D2 has the
/// harness "state plainly", and `Plain` is the absence of a marker rather than
/// a glyph nobody chose. A pane that announced the notice would put a `◈` on a
/// sentence no record gives one to, and a pane that rendered the
/// recommendation plainly would drop the marker D8 names for it.
#[test]
fn the_two_once_ever_lines_read_back_in_the_registers_their_records_give_them() {
    let pane = Pane::of(&[
        Record::Said(crate::session::Said {
            line: crate::session::SaidOnce::Notice,
            text: "bare tier has no membrane.".to_owned(),
        }),
        Record::Said(crate::session::Said {
            line: crate::session::SaidOnce::Recommendation,
            text: "no validators are declared · declare one".to_owned(),
        }),
    ]);
    let lines = zaru_tui::shell::port::TranscriptSource::lines(&pane);

    assert_eq!(lines.len(), 2, "one record, one line, twice: {lines:?}");
    assert_eq!(
        lines[0].register,
        zaru_tui::shell::port::Register::Plain,
        "ADR-0011 D2 has the harness state this plainly and names no marker for it",
    );
    assert_eq!(
        lines[1].register,
        zaru_tui::shell::port::Register::Announced,
        "ADR-0002 D8 puts an event-anchored recommendation in the same register as a learning \
         line, which is the announcement register",
    );
    assert_ne!(
        lines[0].register, lines[1].register,
        "two lines decided by two rules must not be read back through one register",
    );
    // The words are the record's, never composed here: ADR-0010 D2's pane
    // "shows what the file holds, unaltered".
    assert_eq!(lines[0].text, "bare tier has no membrane.");
    assert_eq!(lines[1].text, "no validators are declared · declare one");
}

// ------------------------------- ADR-0010 D4's interruption, held and told once

/// Stage a session directory whose transcript ends the way `phases` says.
///
/// The staging deliberately puts a **finished** call before whatever comes
/// last, so a carrier that reported the first started call, or any started
/// call, would name the wrong one.
fn a_session_whose_last_call(
    scratch: &crate::credentials::fixtures::ScratchRoot,
    seed: u8,
    close_it_with: Option<crate::session::Phase>,
) -> crate::session::Resumed {
    use crate::session::{Record as SessionRecord, SessionStore, ToolCall};

    let store = SessionStore::open(scratch.store_root()).expect("the store opens");
    let session = store
        .start(crate::session::fixtures::id_at(1_700_000_000_000, seed))
        .expect("the session starts");
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working = crate::tools::WorkingDirectory::at(tree.project()).expect("the project resolves");
    let finished = crate::session::fixtures::entry_for(&working, "src/finished.rs", false);
    let last = crate::session::fixtures::entry_for(&working, "src/in-flight.rs", true);

    let mut transcript = crate::session::Transcript::append_to(session.transcript_path())
        .expect("the transcript opens");
    for record in [
        SessionRecord::ToolCall(ToolCall::started(&finished)),
        SessionRecord::ToolCall(ToolCall::completed(&finished)),
        SessionRecord::ToolCall(ToolCall::started(&last)),
    ] {
        transcript.record(&record).expect("a record is appended");
    }
    if let Some(phase) = close_it_with {
        let closing = match phase {
            crate::session::Phase::Completed => ToolCall::completed(&last),
            crate::session::Phase::Refused => ToolCall::refused(&last),
            crate::session::Phase::Started => panic!("a `Started` does not close a pair"),
        };
        transcript
            .record(&SessionRecord::ToolCall(closing))
            .expect("the closing record is appended");
    }
    crate::session::resume(session.directory(), usize::MAX).expect("the session resumes")
}

/// **ADR-0010 D4's second half, on the carrier a resumed session hands a turn.**
///
/// D4: "An interrupted tool call is recorded as `Interrupted` **and the model
/// is told it did not complete**." The derivation is `session::resume`'s and
/// is not repeated here; what this holds is the rule the shell needs and had
/// nowhere to put — that a resumed session owes the model **one** telling,
/// before anything else, and owes it only when a call was genuinely in flight.
///
/// The mutants, named before the check was written ([Verification lessons]
/// §12):
///
/// - **The `Resumed` start is dropped** — `Pending::of` answers `None` for an
///   interrupted transcript, so a resumed session starts its first turn as
///   `Task` and D4's second half reaches no model. Caught by the first arm.
/// - **The interruption is told twice** — `tell_once` peeks instead of taking.
///   Caught by the second arm.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_resumed_session_owes_the_model_one_telling_and_then_owes_nothing() {
    use crate::terminal::driver::Pending;

    let scratch = crate::credentials::fixtures::ScratchRoot::new();
    let resumed = a_session_whose_last_call(&scratch, 41, None);
    let line = resumed
        .interrupted
        .as_ref()
        .expect("the staging left a call in flight")
        .call
        .line
        .clone();

    let mut pending = Pending::of(&resumed, &Nothing);
    assert!(
        pending.is_owed(),
        "a resumed session whose transcript ends in a call that never completed owes the model \
         nothing, so ADR-0010 D4's second half would reach no model",
    );

    let told = pending
        .tell_once()
        .expect("the first turn of a resumed session is told the interruption");
    assert_eq!(
        told.call(),
        line,
        "the telling named a different call from the one the transcript left in flight",
    );

    assert!(
        !pending.is_owed(),
        "the interruption is still owed after being told, so a second turn would be told it again",
    );
    assert!(
        pending.tell_once().is_none(),
        "the interruption was told on the second turn as well as the first; an interruption is \
         told once",
    );
}

/// The accepting siblings: a session that owes nothing must be told nothing.
///
/// Three shapes, because three different rules would pass the check above and
/// fail here. A transcript whose last call **completed**; one whose last call
/// the user **refused**, which closes the pair exactly as a completion does
/// (ADR-0016's ruling of 2026-09-04: a refusal is not a failure, and it is not
/// an interruption either — telling the model that a call the user declined
/// did not complete says the opposite of what happened); and a session being
/// minted, which is `Pending::none`.
///
/// The mutant this catches: **a clean resume is told anyway** — `Pending::of`
/// carrying an interruption whatever `Resumed::interrupted` holds.
#[test]
fn a_session_with_nothing_in_flight_owes_the_model_nothing() {
    use crate::terminal::driver::Pending;

    let scratch = crate::credentials::fixtures::ScratchRoot::new();

    for (seed, closing, what) in [
        (42, crate::session::Phase::Completed, "completed"),
        (43, crate::session::Phase::Refused, "the user refused"),
    ] {
        let resumed = a_session_whose_last_call(&scratch, seed, Some(closing));
        assert_eq!(
            resumed.interrupted, None,
            "the staging is wrong: a call the record says closed the pair was derived as an \
             interruption",
        );
        let mut pending = Pending::of(&resumed, &Nothing);
        assert!(
            !pending.is_owed() && pending.tell_once().is_none(),
            "a session whose last call {what} owes the model an interruption, so a resume would \
             tell the model an action that finished did not complete",
        );
    }

    let mut minted = Pending::none();
    assert!(
        !minted.is_owed() && minted.tell_once().is_none(),
        "a session being minted owes an interruption, and nothing has happened in it yet",
    );
}

/// **The mechanism has a product caller, which for a day it did not.**
///
/// [ADR-0010]'s own Status tracking carried this, written by `session-restore`
/// on 2026-09-05: "Nothing in `zaru-cli`'s product tree constructs
/// `Turn::Resumed` … So 'the model is told it did not complete' is reachable
/// from an outside caller and from no door a person can open." That is library
/// [Verification lessons] §25's cheap companion in as many words — "a
/// mechanism whose only callers are in the test suite is a mechanism nobody
/// has been shown to reach, and one search over the production sources answers
/// it without a run" — and it is a search rather than a run because no check
/// in this repository can construct a [`Turns`]: it holds a `&Prepared`, whose
/// fields are private and include a provider client, so there is no way to
/// call `run_a_turn` without a key. What that costs is stated rather than
/// glossed: **this says the call site exists and says nothing about what it
/// does**, and the behaviour is held by the artefact on ADR-0010's Status
/// tracking and by the checks above.
///
/// Two mutants: deleting the `Start::Resumed` call site in
/// `turns_of_one_line`, and deleting `Pending::of`'s call site in
/// `terminal::open`, which between them are the whole of the wiring.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn adr_0010_d4s_resumed_turn_is_started_by_product_source_and_not_only_by_a_check() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0usize;
    let mut starts: Vec<String> = Vec::new();
    let mut tellings: Vec<String> = Vec::new();
    let mut carriers: Vec<String> = Vec::new();
    let mut controls = 0usize;
    let mut frontier = vec![src];

    while let Some(directory) = frontier.pop() {
        for entry in std::fs::read_dir(&directory).expect("a source directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                frontier.push(path);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            // The checks and their fixtures are not the product, and the whole
            // point of this check is that a call site in one of them is not a
            // door a person can open.
            if path
                .file_name()
                .is_some_and(|name| name == "tests.rs" || name == "fixtures.rs")
            {
                continue;
            }
            files += 1;
            let source = std::fs::read_to_string(&path).expect("a source file is readable");
            for (number, line) in source.lines().enumerate() {
                let at = format!("{}:{}", path.display(), number + 1);
                // Code rather than prose: every mention in this tree that is
                // not a call is inside a doc comment or a link definition.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                // `Start::Resumed(&` rather than `Start::Resumed(`, because
                // the latter also matches the *pattern* `run_a_turn` uses to
                // read the start it was handed — measured, not predicted: the
                // looser spelling stayed green under a mutation that deleted
                // the construction and left the pattern (library verification
                // lessons §9). A construction takes a reference; the pattern
                // binds a name.
                if line.contains("Start::Resumed(&") {
                    starts.push(at.clone());
                }
                if line.contains(".tell_once()") {
                    tellings.push(at.clone());
                }
                if line.contains("Pending::of(") {
                    carriers.push(at.clone());
                }
                // The liveness control: a spelling that must not be found, so
                // "nothing found" is evidence the instrument could have found
                // something (library verification lessons §8).
                if line.contains("Start::NeverBuilt(") {
                    controls += 1;
                }
            }
        }
    }

    assert!(
        files > 40,
        "this scan read {files} product file(s), which is too few to have asserted anything about \
         where a resumed turn is started"
    );
    assert_eq!(
        controls, 0,
        "the scan matched a variant that does not exist, so its matcher says nothing"
    );
    assert!(
        !starts.is_empty(),
        "no product source starts a turn with `Start::Resumed`, so ADR-0010 D4's second half is \
         reachable from an outside caller and from no door a person can open"
    );
    assert!(
        !tellings.is_empty(),
        "no product source ever asks a session what it owes the model, so the interruption is \
         carried and never told"
    );
    assert!(
        !carriers.is_empty(),
        "no product source builds the carrier a resumed session hands a turn, so `Pending` is a \
         mechanism nobody has been shown to reach"
    );
    println!("  the resumed turn is started at: {starts:?}");
    println!("  the interruption is told at: {tellings:?}");
    println!("  the carrier is built at: {carriers:?}");
}

/// The answer's text is painted while the turn is still running.
///
/// This is the property a stream exists for and the one no earlier check
/// could make: `the_pane_repaints_while_a_turn_is_suspended` asserts the beat
/// still fires, and nothing on the pane changes on a bare beat. Here the
/// deltas arrive on the channel the driver races, so the frames captured
/// *during* the turn carry text that grows — and the turn has not ended, so
/// the answer cannot have come from `Ran::lines`.
///
/// The mutant: remove the delta branch from the `select!`, or paint the delta
/// without adding it to the shell.
#[test]
fn the_answers_text_is_painted_across_beats_before_the_turn_ends() {
    let (source, sent) = live_source(Vec::new());
    let (staged, pace) = Raceable::gated(5, sent);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    // The provider's side of the channel, filled before the race starts so
    // every delta is waiting: the assertion is about the driver painting
    // them during the turn, not about when a socket delivers them.
    let (sender, mut deltas) = tokio::sync::mpsc::unbounded_channel();
    for delta in ["One", "\nTwo", "\nThree"] {
        sender
            .send(delta.to_owned())
            .expect("the receiver is alive");
    }
    drop(sender);

    let raced = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            Some(&mut deltas),
            staged.turn(),
        ))
    };

    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );

    // What a reader saw: frames captured DURING the turn, carrying text that
    // grows. The turn had not ended, so this cannot have come from `Ran`.
    let painted: Vec<String> = surface.frames.iter().map(|rows| rows.join("\n")).collect();

    // The answer's text grew across the frames: a frame carrying only the
    // first delta comes before one carrying all three. That ordering is the
    // whole claim — a client that handed the answer over in one piece would
    // produce the last frame and never the first.
    let first_only = painted
        .iter()
        .position(|frame| frame.contains("One") && !frame.contains("Three"));
    let all_three = painted.iter().position(|frame| frame.contains("Three"));
    assert!(
        first_only.is_some(),
        "no frame painted during the turn carried only the first delta, so the answer did not \
         arrive in pieces"
    );
    assert!(
        all_three.is_some(),
        "the last delta never reached a frame, so the deltas did not reach the shell through \
         the driver's own loop"
    );
    assert!(
        first_only < all_three,
        "the whole answer was painted before a piece of it: {first_only:?} then {all_three:?}"
    );

    // And the provisional line did not outlive the turn: `Pane`'s `Drop` took
    // it when the block above ended the borrow. This is the assertion the
    // mutation of 2026-09-05 showed a remembered call could not support.
    assert_eq!(
        shell.streaming(),
        None,
        "the provisional streamed line outlived the turn, so the answer will be painted twice"
    );
}

// ------------------ ADR-0011 D3's question, raised inside a turn a pump races

/// A beat that counts, yields, and gives up rather than spinning for ever.
///
/// **It never sleeps and never reads a clock**, which is the discipline every
/// `Pace` a check owns already keeps: what ends the loop below is something
/// the check counted, not something the machine scheduled (library
/// verification-lessons §57). The ceiling is what makes the red a *sentence*
/// rather than a hang — the confirmation this drives cannot end itself when
/// the receiver is held, because `try_lock` fails before `try_recv` and so
/// even a disconnected channel is never seen.
///
/// `elapse` is `Pending` for ever, which is what the product's beat is for the
/// whole of a suspended turn: [`crate::terminal::source::Beat`] sleeps a
/// tenth of a second, and a beat that returned `Ready` on every poll would end
/// each `select!` invocation after one poll and drop the branch futures with
/// it — which is precisely the state this check exists to get out of.
#[derive(Debug)]
struct Bounded {
    beats: Arc<AtomicUsize>,
    limit: usize,
    watched: Arc<Source>,
}

impl crate::terminal::source::Pace for Bounded {
    fn wait(&self) {
        let waited = self.beats.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            waited <= self.limit,
            "the confirmation waited {waited} beat(s) without reading the key the terminal sent; \
             the source reports contended={} and one more read of it takes {:?}",
            self.watched.contended(),
            self.watched.try_next()
        );
        std::thread::yield_now();
    }

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        core::future::poll_fn(move |_| core::task::Poll::Pending)
    }
}

/// A question raised inside a race is answered by a key the terminal sends.
///
/// # The composition no other check makes
///
/// The four checks above that answer a question call
/// [`PaneConfirm::confirm`] directly, with no [`race`](crate::terminal::driver::race)
/// alive over the same [`Source`]; the four that drive `race` race a future
/// that never confirms. **The defect lives only where the two meet**, and it
/// lived there from 2026-09-05 until this check: `Source::next` held the
/// receiver's guard across `recv().await`, `tokio::select!` keeps a branch
/// future alive across every poll of one invocation, and so the confirmation
/// raised inside the turn's own poll found the receiver locked by the pump's
/// branch. Every `try_next` failed before reaching the channel, the answer sat
/// in the channel unread, and ADR-0011 D3's prompt could not be answered at
/// the default mode — the register's Medium row of that day.
///
/// **A real key, not a staged answer.** The reader is a thread of its own
/// sending down the real channel, as a person does: they read the prompt, then
/// they press the key. And **a real runtime**, because
/// [`futures_lite_block_on`](crate::compose::tests::futures_lite_block_on)
/// polls once and panics on a yield, so no check that goes through it can
/// produce a `select!` invocation that spans two polls — which is the only
/// state in which the defect exists.
///
/// The mutant: give `Source::next` back its `recv().await` under the guard.
#[tokio::test]
async fn a_question_raised_inside_a_race_is_answered_by_a_real_key() {
    use crate::tools::port::Confirm as _;

    // The reader answers only once the question stands, which is both what a
    // person does and what keeps the key out of `read_while_busy`: a key
    // already in the channel would be taken by the pump's branch and put in
    // the composer, which is a different claim from this one.
    let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watching = Arc::clone(&asked);
    let source = Arc::new(Source::over(move |sender, stop| {
        while !watching.load(Ordering::SeqCst) {
            if stop.load(Ordering::Acquire) {
                return;
            }
            std::thread::yield_now();
        }
        let _ = sender.send(press(Key::Char('y')));
        while !stop.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }));

    let pace = Bounded {
        beats: Arc::new(AtomicUsize::new(0)),
        limit: 1000,
        watched: Arc::clone(&source),
    };
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    let raced = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let confirm = PaneConfirm::over(&pane, &source, &pace);
        let mut polls = 0_usize;
        // The turn: one suspension, woken at once, then the tool call's
        // question. The suspension is the whole staging — it is the poll at
        // which the pump's own branch takes the source, and a turn that
        // confirmed on its first poll would find the lock free and assert
        // nothing.
        let turn = core::future::poll_fn(|context| {
            polls += 1;
            if polls == 1 {
                context.waker().wake_by_ref();
                return core::task::Poll::Pending;
            }
            asked.store(true, Ordering::SeqCst);
            core::task::Poll::Ready(
                confirm
                    .confirm(&Question {
                        statement: "write build/out.txt".to_owned(),
                        prominent: false,
                    })
                    .map_err(|failure| format!("{failure}")),
            )
        });
        crate::terminal::driver::race(&pane, &source, &pace, &trie, &mut now, None, turn).await
    };

    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran(Ok(true)),
        "the question raised inside the race was not answered `y` by the key the terminal sent"
    );
    // The invariant `Source::contended`'s documentation argues for. It was
    // asserted before this check and held, because nothing had ever raised a
    // question while the pump's branch was live.
    assert_eq!(
        source.contended(),
        0,
        "the source was contended {} time(s), so a reader held the receiver while the other \
         needed it",
        source.contended()
    );
    // ADR-0005 D1: the answer is not a keystroke the composer sees. A `y` that
    // reached the strip would be a question answered and a line the user never
    // typed, both at once.
    assert_eq!(
        shell.composer().text(),
        "",
        "the answer reached the composer, which reads {:?}",
        shell.composer().text()
    );
    // The question was on the frame before the key was read: `confirm` paints
    // it and then waits, so a check that only read the answer could not say
    // the person had been asked.
    let first = surface.frames.first().expect("no frame was painted");
    assert!(
        first.join("\n").contains("write build/out.txt"),
        "the first frame painted does not carry the question, so the key was read before the \
         person could have seen it"
    );
}
