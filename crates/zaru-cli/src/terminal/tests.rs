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

    let mut collector = super::driver::ToolLines::default();
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
