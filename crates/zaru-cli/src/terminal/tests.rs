// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::cli::invocation::{Overrides, Request};
use crate::cli::namespace::Namespace;
use crate::compose::tests::futures_lite_block_on;
use crate::failure::Exit;
use crate::session::Record;
use crate::terminal::driver::{
    Guard, Pane as TurnPane, PaneConfirm, PaneSink, Surface, Turnable, question_for_the_shell,
    request_for, run,
};
use crate::terminal::fixtures::{Counting, Held, Recording, Restores, press, typed};
use crate::terminal::open::{Opening, opening_for};
use crate::terminal::source::{Source, Taken};
use crate::terminal::trie::{NOTHING_CACHED, NotesTrie};
use crate::terminal::vocabulary::{Transcript as Pane, Vocabulary};
use crate::tools::port::Question;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zaru_notes::trie::{CachedEntry, EntryKind as CachedKind};
use zaru_tui::shell::port::{CommandVocabulary, Register, TranscriptSource};
use zaru_tui::shell::{COMPOSER_ROWS, Key, Palette, Shell, Status, Struck};

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

/// The same pump over a script that may contain a paste.
fn pump_staged(struck: Vec<zaru_tui::shell::Struck>) -> (Shell, Recording, Exit) {
    pump_staged_over(struck, &NotesTrie::nothing_cached(WORKSPACE))
}

/// The same pump over a fast tier a check chose.
fn pump_over(keys: Vec<zaru_tui::shell::Input>, trie: &NotesTrie) -> (Shell, Recording, Exit) {
    pump_staged_over(keys.into_iter().map(Into::into).collect(), trie)
}

/// The same pump under a palette a check chose.
///
/// The product's own `Surface::draw` path, which is what makes this different
/// from painting a shell into a `TestBackend` by hand: `Recording` holds its
/// palette exactly as `Crossterm` holds the one `NO_COLOR` gave it, so a check
/// here exercises the seam a session actually goes through.
fn pump_painting(
    keys: Vec<zaru_tui::shell::Input>,
    trie: &NotesTrie,
    palette: Palette,
) -> (Shell, Recording, Exit) {
    pump_staged_painting(keys.into_iter().map(Into::into).collect(), trie, palette)
}

/// The pump every helper above reaches, over what the terminal handed across.
fn pump_staged_over(
    struck: Vec<zaru_tui::shell::Struck>,
    trie: &NotesTrie,
) -> (Shell, Recording, Exit) {
    pump_staged_painting(struck, trie, Palette::Coloured)
}

/// The same, under a palette.
///
/// The palette is a second axis rather than a second pump: every helper above
/// reaches this one function, and the two that have no opinion about colour
/// pass what the product passes when `NO_COLOR` is unset.
fn pump_staged_painting(
    struck: Vec<zaru_tui::shell::Struck>,
    trie: &NotesTrie,
    palette: Palette,
) -> (Shell, Recording, Exit) {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::painting(Arc::clone(&restores), 72, palette);
    let source = Source::staged(struck);
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
    let exit = match pumped.outcome {
        crate::terminal::driver::Pumped::Left(exit) => exit,
        crate::terminal::driver::Pumped::Switch(id) => {
            panic!("the pump asked to switch to {id} rather than leaving")
        }
    };
    (shell, surface, exit)
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

/// The typed line is echoed above whatever the turn produces.
///
/// The survey's row 5: on Enter the typed text "vanishes from the composer and
/// **is never rendered anywhere**", so "a person scrolling a long session
/// cannot tell which answer belongs to which question". This is that line.
///
/// # Why this drives the arm with no provider, and why that is not a weaker case
///
/// `run_a_turn` needs a real `GeminiClient` and no offline check can reach it,
/// which is the lesson `Pane`'s `Drop` records in this same file: a line placed
/// there had a mutation deleting it redden **nothing**, and "a property whose
/// only guarantee is that somebody remembered to write one line is the shape
/// this workspace keeps replacing". So the echo is in `run`'s task arm, which
/// this pump reaches, and which covers `Turnable::Cannot` as well — a person
/// typing into a session that resolved no provider sees their own line above
/// the refusal rather than a refusal floating over nothing.
///
/// # What discriminates
///
/// **The order**, not the presence. The echo has to be *above* what the turn
/// said, because a line under its own answer is the confusion row 5 is about;
/// so the two are found by offset. And the wording is `vocabulary::spoken`'s,
/// the same function `--resume` replays a `Record::Conversation` through, so
/// this asserts the same string the file will render to.
///
/// **The mutant:** deleting the `shell.notice` in `run`'s `Action::Task` arm,
/// which is what every build before 2026-09-06 did.
///
/// **The accepting sibling** is the second half: a slash command is not a task
/// and is echoed by nobody, so this cannot pass against a shell that prints a
/// `user:` line for every input it receives.
#[test]
fn the_typed_line_is_echoed_above_what_the_turn_said() {
    use crate::session::Voice;

    let task = "rename the widget";
    let mut keys = typed(task);
    keys.extend(typed("/exit"));
    let (shell, _surface, _exit) = pump(keys);

    let said: String = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");

    let echo = format!("{}: {task}", Voice::User.spoken_as());
    let at = said.find(&echo).unwrap_or_else(|| {
        panic!("the typed line was never rendered anywhere, which is the survey's row 5: {said:?}")
    });
    let answered = said
        .find(CANNOT)
        .expect("the staging's own refusal must be on the pane, or nothing here is ordered");
    assert!(
        at < answered,
        "the typed line is echoed below what the turn said, so a reader still cannot tell \
         which answer belongs to which question: {said:?}"
    );

    // The accepting sibling: a slash command is not a task and nobody echoes
    // it, so this check cannot pass against a shell that prints a `user:` line
    // for every input.
    let (only_commands, _, _) = pump(typed("/exit"));
    let commanded: String = only_commands
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !commanded.contains(&format!("{}: ", Voice::User.spoken_as())),
        "a session in which nothing was asked painted a user line anyway: {commanded:?}"
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

/// [ADR-0028] D2's own subject is in the register D2's heading names, and
/// still never in the register its second half reserves.
///
/// # Both halves of one clause, asserted apart
///
/// D2's heading is "Failure is shown, **in its own register**, never as an
/// error", and its body is "never in the register reserved for defects". Until
/// 2026-09-13 an iteration's failure was `Register::Plain` — the register of
/// ordinary narration — which honoured the body and not the heading, and left
/// D2's remaining word, "coloured", unsatisfiable: colouring `Plain` colours
/// every line of narration and is a theme rather than a register. `Setback` is
/// the register the heading names, accepted as an Update on that record under
/// the coordinator's ruling of 2026-09-13 23:58Z and open to Jeshua's veto.
///
/// **Three assertions rather than one.** `Setback` alone would pass on a
/// renderer that put every line there; `not Failed` alone is what stood before
/// and says nothing about the heading; `not Plain` is what changed.
///
/// The cell-level form of the same claim —  read out of a painted buffer with
/// the colour on it — is
/// [`corpus_an_iteration_failure_never_carries_the_error_registers_colour`].
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn an_iteration_failure_is_a_setback_and_never_the_error_register() {
    let line = painted_loop_line(&zaru_core::iteration::Event::IterationFailed {
        n: 2,
        reason: "greets: failed".to_owned(),
        elapsed: core::time::Duration::from_millis(2_570),
    });

    assert_eq!(
        line.register,
        Register::Setback,
        "ADR-0028 D2's heading is \"Failure is shown, in its own register\", and an \
         iteration's failure rendered in {:?} instead: {:?}",
        line.register,
        line.text
    );
    assert_ne!(
        line.register,
        Register::Failed,
        "ADR-0028 D2 puts an iteration's failure \"never in the register reserved for \
         defects\", which is ADR-0016 D1's error register: {:?}",
        line.text
    );
    assert_ne!(
        line.register,
        Register::Plain,
        "an iteration's failure is back in the register of ordinary narration, which is \
         what the Update of 2026-09-13 on ADR-0028 D2 moved it out of: {:?}",
        line.text
    );
}

/// Only a validator that **failed** is a setback, and the other two outcomes
/// are ordinary narration.
///
/// The accepting sibling of the check above, and it is the arm that stops
/// `Setback` becoming a second name for `Plain`. A skip is deliberately not a
/// setback: [ADR-0009] D2's `skipped` is a validator whose prerequisite
/// failed, so the setback belongs to the prerequisite and its own row already
/// carries it.
///
/// Walks every variant of the outcome enum rather than the two the change was
/// about, so a fourth outcome arriving with no register decision is a failure
/// here as well as a build error in `vocabulary::register_for`.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[test]
fn only_a_failing_validator_is_a_setback() {
    use zaru_core::iteration::ValidatorOutcome as What;

    for (outcome, expected) in [
        (What::Failed, Register::Setback),
        (What::Passed, Register::Plain),
        (What::Skipped, Register::Plain),
    ] {
        let line = painted_loop_line(&zaru_core::iteration::Event::ValidatorEvaluated {
            name: "greets".to_owned(),
            outcome,
            detail: "the third assertion did not hold".to_owned(),
        });
        assert_eq!(
            line.register,
            expected,
            "a validator that {} rendered in {:?} rather than {expected:?}: {:?}",
            crate::cli::render::validator_outcome(outcome),
            line.register,
            line.text
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

/// Only a session request opens a shell, and each names which session.
///
/// A shell that opened for `zaru runtime` would turn a question into a
/// session, and ADR-0010 D1 makes a session a directory on disk. **A bare
/// `zaru` is the third**, since 2026-09-06: it is not a question but the
/// request to be in one, and it mints.
#[test]
fn only_a_session_request_opens_a_shell_and_each_names_which() {
    let id = crate::session::SessionId::parse("01JQZX8N3K4M5P6R7S8T9V0W1X").expect("a ULID");
    assert_eq!(opening_for(&Request::Session), Some(Opening::New));
    assert_eq!(
        opening_for(&Request::Continue),
        Some(Opening::MostRecentHere)
    );
    assert_eq!(
        opening_for(&Request::Resume { id: id.clone() }),
        Some(Opening::Existing(id))
    );
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
            opening_for(&request).is_none(),
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
        let _ = sender.send(press(Key::Char('z')).into());
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
        Some(press(Key::Char('z')).into()),
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
    assert_eq!(
        source.try_next(),
        Taken::Struck(press(Key::Char('y')).into())
    );
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
        let _ = sender.send(press(Key::Char('y')).into());
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

/// The pace is also the clock, and that is what makes a live figure checkable.
///
/// [ADR-0028] D5's meter reads a [`Clock`](zaru_core::iteration::Clock) on
/// every beat, so in a check the two have to agree about what a beat is worth.
/// Reading this counter as `TICK × beats` makes the elapsed figure an
/// **exact** number — `0.10s` after one beat, `0.30s` after three — where a
/// clock a check could not set would leave it asserting about how the machine
/// happened to schedule, which is what `Held`'s own documentation exists to
/// refuse (library verification-lessons §57).
///
/// It is deliberately the same value rather than a second fixture: a pace and
/// a clock that could disagree about how long a beat took would let a check
/// pass while the two readings drifted.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
impl zaru_core::iteration::Clock for Releasing {
    fn now(&self) -> core::time::Duration {
        let beats = self.beats.load(Ordering::SeqCst);
        crate::terminal::source::TICK * u32::try_from(beats).unwrap_or(u32::MAX)
    }
}

impl Releasing {
    /// How many beats have been waited, for a staging that keys on them.
    fn beats_so_far(&self) -> usize {
        self.beats.load(Ordering::SeqCst)
    }

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

/// Everything a metered race needs, staged: the pace that is also the clock,
/// the turn it releases, and the shell the meter writes to.
///
/// The token reader is a closure the check owns, so what "the provider
/// reported" is at any beat is a value this check chose rather than one a
/// provider produced — which is the only way to drive `Meter` without a key,
/// a network or a `Prepared`.
struct Metered {
    reported: Arc<std::sync::Mutex<Option<crate::providers::TokenUsage>>>,
}

impl Metered {
    fn new() -> Self {
        Self {
            reported: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// What the provider says it has spent, from now on.
    fn reports(&self, usage: Option<crate::providers::TokenUsage>) {
        *self.reported.lock().expect("the check owns this lock") = usage;
    }

    fn reader(&self) -> impl Fn() -> Option<crate::providers::TokenUsage> + use<> {
        let slot = Arc::clone(&self.reported);
        move || slot.lock().expect("the check owns this lock").clone()
    }
}

/// A provider that reports a different figure at each stage of one turn,
/// keyed on the **beat count** rather than on a sleep.
///
/// **The first version of this staged the figures from a thread with two
/// `sleep`s and it was not an instrument**: `Releasing` returns at once, so
/// four beats passed in microseconds and the check failed against its own
/// staging rather than against the product. Keying on the beat makes the
/// three stages happen in a fixed order every run, which is library
/// verification-lessons §57 — a check over a random instrument is not a check.
fn reports_by_beat(
    pace: &Releasing,
) -> impl Fn() -> Option<crate::providers::TokenUsage> + use<'_> {
    move || match pace.beats_so_far() {
        0 | 1 => None,
        2 | 3 => Some(crate::providers::TokenUsage::counted(390, 79)),
        _ => Some(crate::providers::TokenUsage::counted(902, 145)),
    }
}

/// The status row of every frame a metered race painted.
fn status_rows(surface: &Recording) -> Vec<String> {
    surface
        .frames
        .iter()
        .map(|frame| frame[0].trim_end().to_owned())
        .collect()
}

/// [ADR-0028] D5's meter advances across the beats of one turn.
///
/// **This is the whole of what survey row 2 asked for**: "nothing moves" was
/// measured at a real terminal on 2026-09-05, with the pane's last line
/// unchanged for the whole exchange. The turn here is held open for five
/// beats and the status row is read on every frame; the figures are exact
/// because the pace is also the clock, so nothing here asserts about
/// wall-clock time.
///
/// The mutants: the beat branch calling `paint` instead of `tick`; `Meter`
/// reading its start time on every refresh instead of once.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn the_elapsed_figure_advances_across_the_beats_of_one_turn() {
    let (source, sent) = live_source(Vec::new());
    let (staged, pace) = Raceable::gated(5, sent);
    let metered = Metered::new();
    let reader = metered.reader();
    let meter = crate::terminal::driver::Meter::started(&pace, &reader);
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
            Some(&meter),
            staged.turn(),
        ))
    };
    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );
    assert!(
        staged.finished.load(Ordering::SeqCst),
        "the staged turn never ran, so this check asserted nothing"
    );

    let rows = status_rows(&surface);
    assert!(
        rows.len() >= 5,
        "the race painted {} frame(s), which is too few to watch a figure rise",
        rows.len()
    );
    // Exact figures, because a beat is worth `TICK` to both the pace and the
    // clock. A `contains` over a rising set rather than an equality over the
    // whole row: what this check is about is the meter, not the row's order.
    for (beat, row) in rows.iter().enumerate() {
        let expected = crate::terminal::vocabulary::seconds(
            crate::terminal::source::TICK * u32::try_from(beat + 1).expect("a small beat count"),
        );
        assert!(
            row.contains(&expected),
            "frame {beat} must carry {expected:?}; the row was {row:?}"
        );
    }
    assert!(
        rows.first() != rows.last(),
        "every frame's status row was identical, so nothing moved while the turn ran"
    );
}

/// The token count changes when an exchange reports one, and not before.
///
/// `Provider::usage` answers `None` until a request has been made, and a
/// turn's several exchanges each replace the slot — so what a person watches
/// is a figure that arrives and then rises. **Not a sum**: ADR-0012 D7's
/// accumulating total is that record's author's, and a check that asserted one
/// here would be the "caller that summed" `Prepared::usage` warns about.
///
/// The mutants: `Meter::refresh` reading the token slot once, at construction;
/// `refresh` writing the narrow spelling into both fields.
#[test]
fn the_token_count_changes_when_an_exchange_reports_one_and_not_before() {
    let (source, sent) = live_source(Vec::new());
    let (staged, pace) = Raceable::gated(6, sent);
    // Nothing at first, then one exchange's usage, then a second exchange's
    // larger one -- each at a named beat, so the three stages happen in the
    // same order every run.
    let reader = reports_by_beat(&pace);
    let meter = crate::terminal::driver::Meter::started(&pace, &reader);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let _ = futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            Some(&meter),
            staged.turn(),
        ));
    }

    let rows = status_rows(&surface);
    let joined = rows.join("\n");
    assert!(
        rows.first()
            .is_some_and(|first| !first.contains("tokens:") && !first.contains(" tokens")),
        "the first frame must carry no token count, because no exchange had reported one; it \
         was {:?}",
        rows.first()
    );
    assert!(
        joined.contains("tokens: 390 prompt + 79 completion = 469"),
        "the first exchange's count must reach the row; the frames were {joined}"
    );
    assert!(
        joined.contains("tokens: 902 prompt + 145 completion = 1047"),
        "the second exchange's count must replace it; the frames were {joined}"
    );
    assert!(
        !joined.contains("= 1516"),
        "the row must not sum the two exchanges; ADR-0012 D7's total is not this arc's to take"
    );
}

/// [ADR-0013] D6's figure is the same bytes on every frame of one turn.
///
/// D7 of that record confines compaction to turn boundaries, so the number
/// cannot change mid-turn and a meter that recomputed it would be asserting a
/// reading no record makes. **This is the check that catches a meter reaching
/// too far**, which is the one way this arc could have moved a clause it said
/// it would not.
///
/// The mutant: `Meter::refresh` writing a context segment.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[test]
fn the_context_figure_is_the_same_bytes_on_every_frame_of_one_turn() {
    let (source, sent) = live_source(Vec::new());
    let (staged, pace) = Raceable::gated(5, sent);
    let metered = Metered::new();
    metered.reports(Some(crate::providers::TokenUsage::counted(1, 2)));
    let reader = metered.reader();
    let meter = crate::terminal::driver::Meter::started(&pace, &reader);
    let mut shell = shell();
    // A figure the host put there at the turn boundary before this turn.
    shell.set_context_usage(Some(zaru_tui::shell::Segment::new(
        "context 12.3k/1048.5k tokens",
        "12.3k/1048.5k",
    )));
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let _ = futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            Some(&meter),
            staged.turn(),
        ));
    }

    let segment = shell
        .status()
        .context
        .clone()
        .expect("the host put a figure on the row before the turn");
    assert_eq!(
        segment,
        zaru_tui::shell::Segment::new("context 12.3k/1048.5k tokens", "12.3k/1048.5k"),
        "a turn must leave ADR-0013 D6's figure exactly as it found it"
    );
    let rows = status_rows(&surface);
    assert!(
        rows.len() >= 5 && rows.iter().all(|row| row.contains("12.3k/1048.5k")),
        "every frame of the turn must carry the same context figure; the rows were {rows:?}"
    );
}

/// The model and the mode are on the row from the session's first frame.
///
/// `terminal::open` writes them once, before the pump runs, through the same
/// `refresh_status` that writes the row's two numbers — which is what keeps
/// "one place writes this row" literally true. This drives that function
/// rather than the binary, for the reason the neighbouring context check
/// already records.
///
/// The mutants: `refresh_status` writing the description only when `tokens`
/// is `Some`, so a session shows no model until its first exchange;
/// `Described::of` reading the alias instead of the resolved identifier.
#[test]
fn the_model_and_the_mode_are_on_the_row_from_the_sessions_first_frame() {
    let mut shell = shell();
    let context = crate::compose::SessionContext::opened(crate::compose::prefix_for(), crossable());
    let redactor = Nothing;
    let model = crate::providers::ModelId::for_a_check("gemini-3.6-flash");

    crate::terminal::driver::refresh_status(
        &mut shell,
        &context,
        None,
        Some(crate::terminal::driver::Described {
            model: &model,
            mode: crate::tools::Mode::Yolo,
        }),
        &redactor,
    );

    let row = shell.status().painted(200);
    assert!(
        row.contains("gemini-3.6-flash"),
        "the resolved model must be on the row; it was {row:?}"
    );
    assert!(
        row.contains("mode yolo"),
        "ADR-0011 D3's mode must be on the row; it was {row:?}"
    );
    assert!(
        shell.status().tokens.is_none(),
        "a session that has had no exchange must carry no token count"
    );
}

/// A model identifier a cloned repository chose cannot forge a second field.
///
/// # The case this capability arrives with
///
/// `model.<alias>` is free at every configuration layer, so `./zaru.toml`
/// chooses the string at `Rank::Model` — and until this arc there was nothing
/// a repository could put on ADR-0001 D2's row at all. An identifier carrying
/// the row's own separator would paint as **two** fields, and the second can
/// read as a tier: `x · runtime.tier = linked` puts a second membrane claim on
/// the one row that record exists to make unambiguous.
///
/// The property is asserted two ways, because either alone is satisfiable by
/// the wrong thing: the row carries **exactly one** `runtime.tier = `, and the
/// tier holds the row's **first cells** out of the painted buffer rather than
/// merely appearing in the string.
///
/// The accepting sibling is the second half — an ordinary identifier reaches
/// the row byte for byte — so the property is not bought by refusing
/// everything.
///
/// The mutant: `model_row` returning the identifier unaltered.
#[test]
fn corpus_a_model_identifier_cannot_forge_a_second_segment_on_the_row() {
    // Both structural spellings, and one of each alone: the separator forges a
    // second *field*, the prefix forges a second *tier claim* without needing
    // one, and the pair does both.
    for hostile in [
        "x · runtime.tier = linked",
        "x · gemini",
        "runtime.tier = linked",
    ] {
        let staged = crate::providers::ModelId::for_a_check(hostile);
        assert_eq!(
            crate::cli::render::model_row(&staged),
            None,
            "an identifier that could say what the row says must not be painted: {hostile:?}"
        );
    }

    let mut shell = shell();
    shell.describe(
        crate::cli::render::model_row(&crate::providers::ModelId::for_a_check(
            "x · runtime.tier = linked",
        )),
        None,
    );
    let row = shell.status().painted(200);
    assert_eq!(
        row.matches("runtime.tier = ").count(),
        1,
        "the row must carry exactly one tier claim; it was {row:?}"
    );
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::wide(Arc::clone(&restores), 120);
    surface.draw(&shell).expect("the test backend paints");
    let painted = surface.frames[0][0].clone();
    assert!(
        painted.starts_with("runtime.tier = bare · "),
        "the tier must hold the row's first cells; row 0 was {painted:?}"
    );

    // The accepting sibling: the property is not bought by refusing everything.
    let ordinary = crate::providers::ModelId::for_a_check("gemini-3.6-flash");
    assert_eq!(
        crate::cli::render::model_row(&ordinary).as_deref(),
        Some("gemini-3.6-flash"),
        "an ordinary identifier must reach the row byte for byte"
    );
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
            let _ = sender.send(key.into());
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

/// A keystroke during a turn reaches the composer, and `Enter` queues it as
/// the next turn's task.
///
/// **Renamed and rewritten 2026-09-13**, from
/// `a_keystroke_during_a_turn_is_neither_lost_nor_executed_as_a_task`. That
/// check asserted ADR-0015's 2026-09-05 ruling, under which the line was
/// refused with a notice and left on the input row; the 2026-09-13 amendment
/// reverses it, so the line is queued and the prompt is cleared because the
/// text moved.
///
/// Three halves rather than two: the typed text is **not lost** -- it is on
/// the input row, painted at the moment it was read -- it is **not executed
/// as this turn's task**, and on `Enter` it is **queued**, which the pinned
/// row above the composer says.
///
/// The mutants: route the mid-turn key to nothing; let `Enter` reach the
/// composer; let `Enter` reach `Shell::key`; leave the line in the prompt
/// rather than moving it.
#[test]
fn a_keystroke_during_a_turn_is_painted_and_enter_queues_it_as_the_next_task() {
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
            None,
            staged.turn(),
        ))
    };
    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );

    // Not lost: every keystroke before the `Enter` reached the composer and
    // was painted as it was read. The frame taken before the `Enter` is the
    // one that can say so, because the `Enter` moves the text out.
    let typed_frame = surface
        .frames
        .iter()
        .find(|frame| input_row(frame).contains("saffron"))
        .cloned()
        .expect(
            "no frame painted `saffron` on the input row, so the keystrokes never reached the \
             composer or were never painted",
        );
    assert!(input_row(&typed_frame).contains("saffron"));

    // Queued rather than run, and the prompt is cleared because the text
    // moved -- exactly as a typed `Enter` at the prompt moves it.
    assert_eq!(
        shell.queued().map(|task| task.task.as_str()),
        Some("saffron"),
        "`Enter` during a turn did not queue the line ADR-0015's 2026-09-13 amendment says it \
         queues; the shell holds {:?}",
        shell.queued()
    );
    assert_eq!(
        shell.composer().text(),
        "",
        "the prompt still holds the line that was queued, so the same text is in two places: \
         {:?}",
        shell.composer().text()
    );
    let last = surface.frames.last().expect("no frame was painted");
    assert!(
        last.iter().any(|row| row.contains("queued saffron")),
        "the pinned row does not say what is queued: {last:?}"
    );
}

/// A second `Enter` during one turn replaces what is queued.
///
/// Its accepting sibling is the check above: one `Enter` queues what was
/// typed, so this cannot pass against an implementation that queues nothing.
#[test]
fn a_second_enter_during_one_turn_replaces_the_queued_task() {
    let mut typing = keys("first");
    typing.push(press(Key::Enter));
    typing.extend(keys("second"));
    typing.push(press(Key::Enter));
    let (source, sent) = live_source(typing);
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
            None,
            staged.turn(),
        ))
    };
    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Ran("the turn finished")
    );
    assert_eq!(
        shell.queued().map(|task| task.task.as_str()),
        Some("second"),
        "the second `Enter` did not replace the first's task; the shell holds {:?}",
        shell.queued()
    );
}

/// An `Enter` on an empty prompt during a turn queues nothing.
///
/// A queued nothing would be a pinned row promising a turn that never
/// arrives. Its accepting sibling is two checks above, where a prompt with
/// something in it does queue.
#[test]
fn an_enter_on_an_empty_prompt_during_a_turn_queues_nothing() {
    let (source, sent) = live_source(vec![press(Key::Enter)]);
    let (staged, pace) = Raceable::gated(1, sent);
    let mut shell = shell();
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut now = core::time::Duration::ZERO;
    let trie = NotesTrie::nothing_cached(WORKSPACE);

    {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        let _ = futures_lite_block_on(crate::terminal::driver::race(
            &pane,
            &source,
            &pace,
            &trie,
            &mut now,
            None,
            None,
            staged.turn(),
        ));
    }
    assert_eq!(
        shell.queued(),
        None,
        "an `Enter` on an empty prompt queued {:?}",
        shell.queued()
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
            None,
            staged.turn(),
        ))
    };

    assert_eq!(
        raced,
        crate::terminal::driver::Raced::Interrupted,
        "`Ctrl-C` during a turn did not stop the turn"
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

    crate::terminal::driver::refresh_status(&mut shell, &context, None, None, &redactor);
    let opened = context.usage(&redactor).used();

    for nth in 0..8 {
        context.record(Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }
    crate::terminal::driver::refresh_status(&mut shell, &context, None, None, &redactor);
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

    crate::terminal::driver::refresh_status(&mut shell, &context, None, None, &redactor);
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

    crate::terminal::driver::refresh_status(&mut shell, &context, Some(&usage), None, &redactor);

    assert_eq!(
        shell
            .status()
            .tokens
            .as_ref()
            .map(|segment| segment.full.as_str()),
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

    crate::terminal::driver::refresh_status(&mut shell, &context, None, None, &redactor);

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
        .draw(|frame| shell.render(frame, frame.area(), Palette::Coloured))
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
/// as the distinction**: no record gives an out-of-tree call one. What it
/// reads is `Placement::as_str`'s own words, taken from that constant rather
/// than retyped, so renaming the marking moves this check with it.
///
/// **This paragraph read "and a colour is not something this check could read
/// anyway" until 2026-09-13**, and that clause stopped being true when the
/// registers gained colours; it is corrected here rather than left, and the
/// two claims it made that must stay true — no register of its own, no colour
/// as the distinction — are now asserted rather than merely asserted to be
/// unassertable, by
/// [`corpus_an_out_of_tree_call_carries_its_registers_colour_and_the_marking_is_still_text`].
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
        // Two hundred columns, and the reason has changed. It was written
        // because at 72 the pane *clipped* and the marking was pushed off the
        // frame -- a finding recorded on ADR-0011 as an open question. The
        // pane wraps as of 2026-09-06 and the marking survives at every
        // width; `corpus_an_out_of_tree_marking_survives_a_pane_too_narrow_for_the_line`
        // below is that assertion, at exactly the 72 the question named. This
        // check keeps its own width so that what it holds stays one property:
        // that the renderer marks an escaping call and does not mark an
        // ordinary one, unmixed with anything about wrapping.
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
            None,
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
        let _ = sender.send(press(Key::Char('y')).into());
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
        crate::terminal::driver::race(&pane, &source, &pace, &trie, &mut now, None, None, turn)
            .await
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

// ----------------------------- ADR-0011 D4's pair, painted once — 2026-09-06

/// A staged in-tree call, for the pair checks below.
fn a_staged_call() -> crate::tools::TranscriptEntry {
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working = crate::tools::WorkingDirectory::at(tree.project()).expect("the project resolves");
    // The tree is dropped at the end of this function and the entry keeps the
    // rendered path, which is all these checks read. Nothing here opens a file.
    crate::session::fixtures::entry_for(&working, "note.txt", false)
}

/// A tool call's started-and-completed pair paints **one** line.
///
/// **The mutant**: `Phase::Completed` renders `call.line` again, which is what
/// shipped. Measured from main's binary at `8179f8a` on 2026-09-05, a resumed
/// session painted `fs.write /tmp/…/note.txt` on two adjacent rows for one
/// write — and `transcript.jsonl` shows why: the two `tool_call` records carry
/// byte-identical `line` fields and differ only in `phase`.
///
/// [ADR-0010] D4 needs the pair on disk, because a `Started` with no closing
/// record is how a killed process leaves an interruption behind. What a
/// *reader* needs is the call, once.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn a_started_and_completed_pair_paints_the_call_once() {
    use crate::session::{Record as SessionRecord, ToolCall};
    use zaru_tui::shell::port::{Register, TranscriptSource};

    let entry = a_staged_call();
    let lines = crate::terminal::vocabulary::Transcript::of(&[
        SessionRecord::ToolCall(ToolCall::started(&entry)),
        SessionRecord::ToolCall(ToolCall::completed(&entry)),
    ])
    .lines();

    assert_eq!(
        lines.len(),
        1,
        "one tool call painted {} line(s); the pair is one action: {lines:#?}",
        lines.len()
    );
    assert_eq!(lines[0].text, entry.render());
    assert_eq!(lines[0].register, Register::Call);
}

/// The accepting sibling: a **refused** call still paints, in its own register.
///
/// **The mutant**: every closing phase is silenced rather than the completion
/// alone. A user who declined a call has to be able to see that it did not
/// act, and `Phase::Refused` is the record that says so — so the check above
/// must not be satisfiable by a renderer that simply stopped painting closing
/// records.
#[test]
fn a_refused_call_still_paints_and_in_the_announced_register() {
    use crate::session::{Record as SessionRecord, ToolCall};
    use zaru_tui::shell::port::{Register, TranscriptSource};

    let entry = a_staged_call();
    let lines = crate::terminal::vocabulary::Transcript::of(&[
        SessionRecord::ToolCall(ToolCall::started(&entry)),
        SessionRecord::ToolCall(ToolCall::refused(&entry)),
    ])
    .lines();

    assert_eq!(lines.len(), 2, "a refused call painted {lines:#?}");
    assert_eq!(lines[0].register, Register::Call);
    assert_eq!(
        lines[1].register,
        Register::Announced,
        "ADR-0011 D6 gives the harness no veto and the user's `no` is an answer, so a \
         refusal is announced rather than shown as an ordinary call"
    );
}

/// The second accepting sibling: an **interrupted** call still paints its line.
///
/// A `Started` with no closing record is ADR-0010 D4's interruption. It paints
/// exactly what a completed call now paints — one line — which is worth
/// asserting rather than leaving as a consequence: it says in a check that the
/// pane no longer distinguishes the two, and that the distinction a reader
/// gets is `turn_line`'s `fs.write returned · N bytes · Ts` on the next row.
#[test]
fn an_interrupted_call_paints_its_line_exactly_as_a_completed_one_does() {
    use crate::session::{Record as SessionRecord, ToolCall};
    use zaru_tui::shell::port::TranscriptSource;

    let entry = a_staged_call();
    let interrupted = crate::terminal::vocabulary::Transcript::of(&[SessionRecord::ToolCall(
        ToolCall::started(&entry),
    )])
    .lines();
    let completed = crate::terminal::vocabulary::Transcript::of(&[
        SessionRecord::ToolCall(ToolCall::started(&entry)),
        SessionRecord::ToolCall(ToolCall::completed(&entry)),
    ])
    .lines();

    assert_eq!(interrupted, completed);
    assert_eq!(interrupted.len(), 1);
}

// ------------------ ADR-0009 D2's verdict, on the pane — 2026-09-06

/// A silent validator says so on the pane, rather than trailing an em dash.
///
/// **The mutant**: the renderer interpolates `detail` unconditionally, which
/// is what shipped. Measured from main's binary at `8179f8a` on 2026-09-05,
/// a `grep -q Hello greeting.txt` used as a gate — silent when it fails,
/// which is the ordinary shape — painted:
///
/// ```text
///   greets: failed —
///   iteration 1 failed: greets: · 2.57s
/// ```
///
/// An em dash with nothing after it, and a colon with nothing after it. The
/// phrase is `zaru-core`'s constant, read rather than retyped, because the
/// refinement prompt composes the same phrase for the same condition.
#[test]
fn a_validator_that_printed_nothing_says_so_on_the_pane() {
    let line =
        crate::terminal::vocabulary::loop_line(&zaru_core::iteration::Event::ValidatorEvaluated {
            name: "greets".to_owned(),
            outcome: zaru_core::iteration::ValidatorOutcome::Failed,
            detail: String::new(),
        });

    assert_eq!(
        line.text,
        format!(
            "greets: failed — {}",
            zaru_core::iteration::PRODUCED_NO_OUTPUT
        )
    );
    assert!(
        !line.text.trim_end().ends_with('—'),
        "the line still ends on a separator with nothing after it: {:?}",
        line.text
    );
}

/// A **skipped** validator says nothing, because it never ran.
///
/// **The mutant**: the phrase is composed for every silent outcome, which is
/// what this arc wrote first and what running the real binary caught —
/// `greets: passed — produced no output` reads correctly and
/// `lint: skipped — produced no output` does not, because ADR-0009 D2's skip
/// means the runner "is not called at all". A statement about a command that
/// never ran is the same class of untruth the bare em dash was.
#[test]
fn a_skipped_validator_says_nothing_because_it_never_ran() {
    let line =
        crate::terminal::vocabulary::loop_line(&zaru_core::iteration::Event::ValidatorEvaluated {
            name: "lint".to_owned(),
            outcome: zaru_core::iteration::ValidatorOutcome::Skipped,
            detail: String::new(),
        });

    // `cli::render::validator_outcome` spells ADR-0009 D2's `skipped` as
    // "did not run", which makes the old rendering read
    // `lint: did not run — produced no output` -- a sentence that contradicts
    // itself in six words.
    assert_eq!(line.text, "lint: did not run");
    assert!(
        !line.text.contains(zaru_core::iteration::PRODUCED_NO_OUTPUT),
        "a validator that never ran was said to have produced no output: {:?}",
        line.text
    );
    assert!(
        !line.text.contains('—'),
        "the separator is still there with nothing to introduce: {:?}",
        line.text
    );
}

/// A **passing** validator that printed nothing says so, like a failing one.
///
/// The common case: `grep -q` is silent when it succeeds too. Without this
/// arm, restricting the phrase to failures would leave `greets: passed —`
/// trailing exactly the dangling separator row 21 is about.
#[test]
fn a_passing_validator_that_printed_nothing_also_says_so() {
    let line =
        crate::terminal::vocabulary::loop_line(&zaru_core::iteration::Event::ValidatorEvaluated {
            name: "greets".to_owned(),
            outcome: zaru_core::iteration::ValidatorOutcome::Passed,
            detail: String::new(),
        });

    assert_eq!(
        line.text,
        format!(
            "greets: passed — {}",
            zaru_core::iteration::PRODUCED_NO_OUTPUT
        )
    );
}

/// The accepting sibling: a validator that spoke is quoted verbatim.
///
/// **The mutant**: the phrase replaces every detail rather than an empty one.
/// Without this arm the check above passes over a renderer that has stopped
/// showing failure text at all, which is what ADR-0009 D5 makes the loop's
/// whole input.
#[test]
fn a_validator_that_printed_something_is_quoted_verbatim_on_the_pane() {
    let detail = "assertion failed — left ≠ right";
    let line =
        crate::terminal::vocabulary::loop_line(&zaru_core::iteration::Event::ValidatorEvaluated {
            name: "greets".to_owned(),
            outcome: zaru_core::iteration::ValidatorOutcome::Failed,
            detail: detail.to_owned(),
        });

    assert_eq!(line.text, format!("greets: failed — {detail}"));
    assert!(
        !line.text.contains(zaru_core::iteration::PRODUCED_NO_OUTPUT),
        "the phrase was composed over a validator that did produce output: {:?}",
        line.text
    );
}

/// **Security corpus.** ADR-0011 D4's out-of-tree marking survives a pane too
/// narrow to hold the line.
///
/// **The mutant**: the pane clips instead of wrapping — which is what it did
/// until 2026-09-06, and which is why this check could not have been written
/// before then. `TranscriptEntry::render` appends the marking **last**, after
/// the target's resolved absolute path, so a deep enough path pushed
/// `OUTSIDE the working directory` past the right edge and D4's "it renders
/// differently in the transcript at every mode" was false for the reader the
/// clause is about.
///
/// This was a standing question on [ADR Status — open questions], raised by
/// the `narrative-rendering` arc and filed for the security corpus:
/// "**Whether ADR-0011 D4's out-of-tree marking may be truncated off a narrow
/// pane** … measured at 72 columns on 2026-09-05, which is why that clause's
/// own check paints at 200." Seventy-two is the width it named, so it is the
/// width here.
///
/// The accepting sibling is the second arm: an in-tree call at the same width
/// carries no marking, so this cannot pass on a renderer that marks
/// everything — the pair
/// `an_out_of_tree_call_renders_distinctly_on_the_frame_at_yolo` above already
/// holds at 200, and this is that pair at a width a person actually uses.
///
/// [ADR Status — open questions]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status-questions
#[test]
fn corpus_an_out_of_tree_marking_survives_a_pane_too_narrow_for_the_line() {
    use crate::tools::fixtures::ScratchTree;
    use crate::tools::tree::{Placement, WorkingDirectory};
    use zaru_tui::shell::port::{Line, Register};

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let escaping = crate::session::fixtures::entry_for(&working, "../elsewhere/secret", true);
    let ordinary = crate::session::fixtures::entry_for(&working, "notes.txt", false);

    let marking = Placement::OutOfTree.as_str();
    assert!(
        escaping.render().contains(marking),
        "the staged entry is not marked at all, so nothing below is about the pane"
    );
    // Seventy-two columns, the width the open question measured at, and the
    // rows read out of the buffer rather than out of `Line::rows`.
    let rows_at_72 = |entry: &crate::tools::TranscriptEntry| {
        let mut shell = shell();
        shell.notice(Line::new(Register::Call, entry.render()));
        painted_at(&shell, 72, 24).join("\n")
    };

    let escaped = rows_at_72(&escaping);
    assert!(
        escaped.contains(marking),
        "at 72 columns the out-of-tree marking is not on the frame; ADR-0011 D4 requires \
         the call to render differently at every mode, and a marking a narrow terminal \
         cannot show renders no differently to the reader that clause is about:\n{escaped}"
    );

    let inside = rows_at_72(&ordinary);
    assert!(
        !inside.contains(marking),
        "an ordinary in-tree call was marked as having left the tree, so the marking says \
         nothing:\n{inside}"
    );
}

/// [ADR-0010] D2's seventh producer, replayed in order above the new turn.
///
/// D4: a resume "re-renders the last stretch of transcript so the user can see
/// where they were", and until 2026-09-06 the file held no word of what was
/// asked or answered, so what a person saw on resuming a two-turn session was
/// loop bookkeeping. This is the pane half of that: the records are staged and
/// the assertion is read out of the painted **buffer**, not out of
/// `Line::rows`, so what is held is what a terminal actually shows.
///
/// **The order is the assertion, not the presence.** Four lines all present in
/// any arrangement would be satisfied by a pane that grouped every user line
/// together and every answer after them, which is exactly what a reader
/// scrolling a session cannot use — the survey's row 5 complaint is that "a
/// person ... cannot tell which answer belongs to which question". So each
/// line is sought in the remainder of the frame after the one before it.
///
/// **The mutant:** `lines_for`'s `Record::Conversation` arm returning
/// `Vec::new()`, which is what the arm did for every record of this kind
/// before the arc, and which the first arm below catches.
///
/// **The accepting sibling** is the second half: a transcript holding a turn
/// and no conversation paints neither voice, so this cannot pass against a
/// pane that prints a `user:` line for anything at all.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn adr_0010_d2s_conversation_replays_in_order_above_the_new_turn() {
    use crate::session::{Utterance, Voice};

    let spoken = |n: u32, voice: Voice, text: &str| {
        Record::Conversation(Utterance {
            n,
            voice,
            text: text.to_owned(),
        })
    };
    let turn = |n: u32| Record::TurnLoop(zaru_core::tool_call::Event::TurnStarted { n, of: 8 });

    let records = vec![
        spoken(1, Voice::User, "count to three"),
        turn(1),
        spoken(1, Voice::Zaru, "one, two, three"),
        spoken(2, Voice::User, "now backwards"),
        turn(2),
        spoken(2, Voice::Zaru, "three, two, one"),
    ];

    let mut resumed = shell();
    resumed.refresh(&Pane::of(&records));
    let painted = painted_at(&resumed, 100, 24).join("\n");

    let mut from = 0_usize;
    for expected in [
        "user: count to three",
        "zaru: one, two, three",
        "user: now backwards",
        "zaru: three, two, one",
    ] {
        let at = painted[from..].find(expected).unwrap_or_else(|| {
            panic!(
                "the resumed pane does not show {expected:?} after the line before it, so the \
                 conversation is either absent or out of order:\n{painted}"
            )
        });
        from += at + expected.len();
    }

    // The accepting sibling: a session with turns and no conversation paints
    // neither voice.
    let mut bare = shell();
    bare.refresh(&Pane::of(&[turn(1), turn(2)]));
    let bare_painted = painted_at(&bare, 100, 24).join("\n");
    for voice in [Voice::User, Voice::Zaru] {
        let marker = format!("{}: ", voice.spoken_as());
        assert!(
            !bare_painted.contains(&marker),
            "a transcript holding no conversation painted a {marker:?} line, so the pane is \
             composing one rather than reading one:\n{bare_painted}"
        );
    }
}

// ------------- ADR-0028 D2 and ADR-0016 D1, read off the cells a person sees

/// The palette reaches the pane through the product's own surface, and
/// `NO_COLOR` takes it away again.
///
/// # Why this is not the same claim as the checks below
///
/// Everything else in this file that reads a colour paints a shell into a
/// `TestBackend` directly. This one goes through `terminal::driver::run` and
/// `Surface::draw` — the path a session takes — with `Recording` holding its
/// palette exactly as `Crossterm` holds the one `palette_from_environment`
/// gave it. A palette that never reached the surface would satisfy every
/// other check here and fail this one.
///
/// Both arms, over the frames the pump actually recorded: coloured, at least
/// one cell carries a register's colour; monochrome, not one does.
#[test]
fn the_pump_paints_a_registers_colour_and_a_monochrome_palette_takes_it_away() {
    use ratatui::style::Color;

    let of_a_register = |colour: &Color| {
        *colour != Color::Reset && Register::ALL.iter().any(|r| r.colour() == *colour)
    };
    // A task the pump refuses paints `CANNOT` in `Register::Failed`, which is
    // a real product line in a register with a colour -- rather than a line
    // this check planted. `/exit` then leaves.
    let painted = |palette| {
        let mut keys = typed("do something");
        keys.extend(typed("/exit"));
        let (_, surface, _) = pump_painting(keys, &NotesTrie::nothing_cached(WORKSPACE), palette);
        surface
            .colours
            .iter()
            .flatten()
            .flatten()
            .filter(|colour| of_a_register(colour))
            .count()
    };

    let lit = painted(Palette::Coloured);
    let dark = painted(Palette::Monochrome);

    assert!(
        lit > 0,
        "no frame the pump recorded carries a register's colour, so the zero below is a fact \
         about a pane that paints none rather than about the palette"
    );
    assert_eq!(
        dark, 0,
        "the pump recorded {dark} cell(s) carrying a register's colour under a monochrome \
         palette, against {lit} in colour"
    );
}

/// **Security corpus.** [ADR-0016] D1's crash colour exists, and [ADR-0028]
/// D2's own subject provably does not carry it.
///
/// # The sentence this holds was vacuous until a colour existed
///
/// D1: "**Expected failures never render as errors.** An iteration that fails
/// is the mechanism operating, and **colouring it like a crash** teaches users
/// to fear the thing that makes the product work." Before 2026-09-13 no colour
/// existed anywhere in this workspace, so nothing could be coloured like a
/// crash and the sentence forbade nothing. A crash colour exists now, and this
/// is the check that says the setback does not wear it.
///
/// D1's trigger clause 2 — "An iteration failure is asserted not to render in
/// the error register" — has stood as "half satisfied … **two halves are
/// missing: there is no renderer**, and nothing in any product tree turns
/// `zaru-core`'s `Event::IterationFailed` into a classified failure". **This
/// is the renderer half**: the line is composed by the product's own
/// `loop_line` and the assertion is read out of the painted cell. The second
/// half does not move and is not claimed.
///
/// # Three arms, and one of them is the control
///
/// The setback's glyph carries `SETBACK`; it does not carry `FAILED`; and a
/// real `Register::Failed` line in the **same frame** does. Without the third
/// arm this passes on a renderer that colours nothing at all, which is the
/// shape [Verification lessons] §8 is written against.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn corpus_an_iteration_failure_never_carries_the_error_registers_colour() {
    use zaru_tui::shell::port::{FAILED, SETBACK};

    let setback = painted_loop_line(&zaru_core::iteration::Event::IterationFailed {
        n: 1,
        reason: "greets: failed".to_owned(),
        elapsed: core::time::Duration::from_millis(2_570),
    });
    let defect = zaru_tui::shell::port::Line::new(
        Register::Failed,
        "provider: no credential for alias `default`",
    );

    let mut shell = shell();
    shell.notice(setback.clone());
    shell.notice(defect);
    let frame = cells_at(&shell, 100, 24, Palette::Coloured);

    let marker = |needle: &str| {
        frame
            .iter()
            .find(|row| {
                row.iter()
                    .map(|(s, _)| s.as_str())
                    .collect::<String>()
                    .contains(needle)
            })
            .map(|row| (row[0].0.clone(), row[0].1))
            .unwrap_or_else(|| panic!("no painted row carries {needle:?}"))
    };

    let (glyph, colour) = marker("iteration 1 failed");
    assert_eq!(
        glyph,
        Register::Setback.glyph(),
        "the iteration's failure does not open with the setback's glyph"
    );
    assert_eq!(
        colour, SETBACK,
        "ADR-0028 D2's own subject is painted {colour:?} rather than the setback's colour"
    );
    assert_ne!(
        colour, FAILED,
        "ADR-0016 D1 forbids colouring an expected failure like a crash, and the iteration's \
         failure is painted the error register's own colour"
    );

    let (_, crash) = marker("no credential for alias");
    assert_eq!(
        crash, FAILED,
        "the control line is painted {crash:?} rather than the error register's colour, so \
         the assertions above pass on a renderer that colours nothing"
    );
}

/// **Security corpus.** [ADR-0011] D4's out-of-tree marking is still text, and
/// a colour is still not the distinction.
///
/// # What this pins, and against what
///
/// `an_out_of_tree_call_renders_distinctly_on_the_frame_at_yolo` says in its
/// own documentation that "**no register and no colour is claimed as the
/// distinction** … and a colour is not something this check could read
/// anyway". The last clause stopped being true on 2026-09-13, and the first
/// two must not: giving an out-of-tree call a colour of its own would make the
/// marking invisible to a reader with `NO_COLOR` set, to a monochrome
/// terminal, and to every check that reads the transcript file.
///
/// So: both calls' glyphs carry the **same** colour, `CALL`; the marking is in
/// the out-of-tree frame's symbols and not in the in-tree one's. The width is
/// 72, which is where that clause's own corpus check already measures.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn corpus_an_out_of_tree_call_carries_its_registers_colour_and_the_marking_is_still_text() {
    use crate::tools::fixtures::ScratchTree;
    use crate::tools::tree::{Placement, WorkingDirectory};
    use zaru_tui::shell::port::{CALL, Line};

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let escaping = crate::session::fixtures::entry_for(&working, "../elsewhere/secret", true);
    let ordinary = crate::session::fixtures::entry_for(&working, "notes.txt", false);
    let marking = Placement::OutOfTree.as_str();

    assert!(
        escaping.render().contains(marking),
        "the staged entry is not marked at all, so nothing below is about the frame"
    );

    // **Every** row's marker cell, not the first. A call carrying a resolved
    // absolute path wraps at 72 columns and `Placement::as_str` is appended
    // last, so the marking is on a *continuation* row -- and a renderer that
    // coloured an out-of-tree line differently would colour that row and leave
    // the first one alone. Reading only `frame[1]` let exactly that mutant
    // through on 2026-09-13; the escape bought this reach rather than another
    // assertion.
    let painted = |entry: &crate::tools::TranscriptEntry| {
        let mut shell = shell();
        shell.notice(Line::new(Register::Call, entry.render()));
        let frame = cells_at(&shell, 72, 24, Palette::Coloured);
        let pane = &frame[1..usize::from(24 - COMPOSER_ROWS)];
        let text: String = pane
            .iter()
            .map(|row| row.iter().map(|(s, _)| s.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        let leads: Vec<ratatui::style::Color> = pane
            .iter()
            .filter(|row| {
                !row.iter()
                    .map(|(s, _)| s.as_str())
                    .collect::<String>()
                    .trim()
                    .is_empty()
            })
            .map(|row| row[0].1)
            .collect();
        assert!(
            !leads.is_empty(),
            "the staged call painted no row at all, so the assertions below are about an \
             empty pane"
        );
        (leads, text)
    };

    let (escaped_leads, escaped_text) = painted(&escaping);
    let (inside_leads, inside_text) = painted(&ordinary);

    for (n, colour) in escaped_leads.iter().enumerate() {
        assert_eq!(
            *colour, CALL,
            "row {n} of an out-of-tree call carries {colour:?} rather than the call \
             register's colour, so the call is marked by a colour somewhere:\n{escaped_text}"
        );
    }
    for (n, colour) in inside_leads.iter().enumerate() {
        assert_eq!(
            *colour, CALL,
            "row {n} of an in-tree call carries {colour:?}, so the two calls differ by \
             colour and a monochrome terminal would lose ADR-0011 D4's distinction"
        );
    }
    assert!(
        escaped_text.contains(marking),
        "the out-of-tree marking is not on the frame:\n{escaped_text}"
    );
    assert!(
        !inside_text.contains(marking),
        "an ordinary in-tree call was marked as having left the tree:\n{inside_text}"
    );
}

/// A monochrome frame writes no colour sequence, and a coloured one does.
///
/// # The measurement a pseudo-terminal takes, taken offline
///
/// [operations/harness-look-and-feel] row 4 counted the SGR sequences in a
/// real capture and found no foreground colour anywhere. This is that count,
/// deterministic and in the suite: the bytes `ratatui`'s own crossterm backend
/// writes for one frame, scanned for the CSI foreground-colour sequences
/// crossterm emits.
///
/// **Both directions.** A check that only looked at the monochrome frame would
/// be satisfied by a renderer that never emits a colour at all, which is
/// exactly the state this arc changed.
///
/// [operations/harness-look-and-feel]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel
#[test]
fn a_monochrome_frame_writes_no_colour_sequence() {
    use zaru_tui::shell::port::Line;

    let mut shell = shell();
    for register in Register::ALL {
        shell.notice(Line::new(
            register,
            format!("{register:?} in its own register"),
        ));
    }

    // crossterm writes a foreground colour as `ESC [ 3 8 ; 5 ; n m` for an
    // indexed colour and `ESC [ 3 0-7 m` / `ESC [ 9 0-7 m` for the sixteen.
    // The scan is over the *set* sequence rather than over every escape,
    // because every frame ends with an unconditional reset that says nothing
    // about whether a colour was painted.
    let sets = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).into_owned();
        (30..=37)
            .chain(90..=97)
            .map(|code| format!("\x1b[{code}m"))
            .chain(core::iter::once("\x1b[38;5;".to_owned()))
            .chain(core::iter::once("\x1b[38;2;".to_owned()))
            .map(|needle| text.matches(&needle).count())
            .sum::<usize>()
    };

    let lit = sets(&written_bytes(&shell, 80, 24, Palette::Coloured));
    let dark = sets(&written_bytes(&shell, 80, 24, Palette::Monochrome));

    assert!(
        lit > 0,
        "a coloured frame wrote no foreground-colour sequence at all, so the zero below is a \
         fact about a renderer that paints none rather than about NO_COLOR"
    );
    assert_eq!(
        dark, 0,
        "a frame painted under NO_COLOR wrote {dark} foreground-colour sequence(s), against \
         {lit} for the same frame in colour"
    );
}

/// The `NO_COLOR` convention, in both of its arms.
///
/// Present and non-empty disables; present and empty does not; absent does
/// not. The empty case is the one worth a check rather than a comment:
/// `NO_COLOR=` is what a shell leaves behind when a variable is cleared rather
/// than unset, and the convention is explicit that it does not count.
///
/// Read through `palette_for` rather than through the environment, because a
/// check that wrote to the environment would be changing state every other
/// check in this process shares.
#[test]
fn the_no_color_convention_holds_in_both_of_its_arms() {
    use crate::terminal::open::palette_for;
    use std::ffi::OsStr;
    use zaru_tui::shell::Palette;

    for (asked, expected) in [
        (None, Palette::Coloured),
        (Some(OsStr::new("")), Palette::Coloured),
        (Some(OsStr::new("1")), Palette::Monochrome),
        (Some(OsStr::new("0")), Palette::Monochrome),
        (Some(OsStr::new("anything at all")), Palette::Monochrome),
    ] {
        assert_eq!(
            palette_for(asked),
            expected,
            "NO_COLOR = {asked:?} resolved to the wrong palette"
        );
    }
}

/// Every cell a shell paints at a given size, as its symbol and its
/// foreground colour.
///
/// The colour half of [`painted_at`]. A colour is a property of a cell rather
/// than of a row, so a check about one reads the cell.
fn cells_at(
    shell: &Shell,
    width: u16,
    height: u16,
    palette: Palette,
) -> Vec<Vec<(String, ratatui::style::Color)>> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), palette))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    let cell = &buffer[(x, y)];
                    (cell.symbol().to_owned(), cell.fg)
                })
                .collect()
        })
        .collect()
}

/// The bytes `ratatui`'s own crossterm backend would write for one frame.
///
/// **This is the measurement a pseudo-terminal capture takes, taken offline
/// and deterministically.** `ratatui`'s backend emits a colour sequence only
/// where a cell's colour differs from the previous cell's, starting from
/// `Reset`, so a frame of `Reset` cells writes none at all beyond the
/// unconditional reset it ends every frame with. `zaru-tui` cannot take this
/// reading — the crossterm backend is behind a feature that crate deliberately
/// does not carry — so it lives here, where `ratatui` is taken with it.
fn written_bytes(shell: &Shell, width: u16, height: u16, palette: Palette) -> Vec<u8> {
    use ratatui::Terminal;
    use ratatui::backend::{Backend, CrosstermBackend};

    /// A writer the check can read back, because `CrosstermBackend`'s own is
    /// private.
    #[derive(Clone, Default)]
    struct Shared(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("the buffer").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let written = Shared::default();
    let mut terminal =
        Terminal::new(CrosstermBackend::new(written.clone())).expect("a backend over a buffer");
    terminal
        .resize(ratatui::layout::Rect::new(0, 0, width, height))
        .expect("resize");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), palette))
        .expect("draw");
    terminal.backend_mut().flush().expect("flush");
    let bytes = written.0.lock().expect("the buffer").clone();
    bytes
}

/// The buffer a shell paints at a given size, as rows.
///
/// `painted_row` above reads row zero at 200 columns for the status line; this
/// reads the whole frame at a size the caller gives, which is what a check
/// about wrapping needs.
fn painted_at(shell: &Shell, width: u16, height: u16) -> Vec<String> {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), Palette::Coloured))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// The escape sequences bracketed paste is armed and disarmed with.
///
/// `Crossterm` cannot be constructed here — it is three system calls against a
/// terminal a check does not have, which that type's own documentation says —
/// so the sequence is asserted over a writer instead of argued for in a
/// comment. The literals are the ones the look-and-feel survey measured the
/// gap by: its row 13 reads "`ESC[?2004h` appears nowhere in any capture".
#[test]
fn arming_and_disarming_write_the_bracketed_paste_sequences() {
    let mut armed = Vec::new();
    crate::terminal::driver::arm(&mut armed).expect("a vector never fails to be written to");
    assert_eq!(
        String::from_utf8(armed.clone()).expect("the sequence is ASCII"),
        "\u{1b}[?2004h",
        "arming wrote {:?}, and a terminal that was not asked frames no paste",
        String::from_utf8_lossy(&armed)
    );

    let mut disarmed = Vec::new();
    crate::terminal::driver::disarm(&mut disarmed);
    assert_eq!(
        String::from_utf8(disarmed.clone()).expect("the sequence is ASCII"),
        "\u{1b}[?2004l",
        "disarming wrote {:?}, and a terminal left armed tells every later program that a paste \
         is bracketed",
        String::from_utf8_lossy(&disarmed)
    );
}

/// A three-line paste through the pump is one task, not three.
///
/// The survey's row 13, measured at the pump rather than at the composer:
/// "Pasting three lines ran two turns and left the third in the composer."
/// This stages the paste as the terminal now hands it over — one event — and
/// asserts that the `Enter` after it submits the block whole.
///
/// The mutants: route a paste to nothing; submit a paste without waiting for
/// `Enter`; keep only its first line.
#[test]
fn a_three_line_paste_is_submitted_as_one_task() {
    let (_, surface, exit) = pump_staged(vec![
        Struck::Pasted("réad src/main.rs\nthen tell me\nwhat it does".to_owned()),
        press(Key::Enter).into(),
        Struck::Key(press(Key::Char('e'))),
        Struck::Key(press(Key::Char('x'))),
        Struck::Key(press(Key::Char('i'))),
        Struck::Key(press(Key::Char('t'))),
        press(Key::Enter).into(),
    ]);

    assert_eq!(exit.code(), 0, "the pump should have left through the word");
    let spoken: Vec<&String> = surface
        .frames
        .last()
        .expect("no frame was painted")
        .iter()
        .filter(|row| row.contains("réad src/main.rs"))
        .collect();
    assert_eq!(
        spoken.len(),
        1,
        "the pasted block should be echoed as one task; the last frame holds {spoken:?}"
    );
    let last = surface.frames.last().expect("no frame was painted");
    assert!(
        last.iter().any(|row| row.contains("what it does")),
        "the block's third line never reached the pane, so it was not part of the task: {last:?}"
    );
}

/// A paste while a permission question stands is absorbed, exactly as a key is.
///
/// ADR-0011 D3's `ask` "prompts before any write or command", and a prompt a
/// user can paste past is no more a prompt than one they can type past. Two
/// arms, because absorbing everything would be a prompt nobody can answer: the
/// paste changes nothing, and the `y` after it still answers.
#[test]
fn a_paste_while_a_question_stands_is_absorbed_and_the_answer_after_it_is_read() {
    use crate::tools::port::Confirm as _;

    let source = Source::staged(vec![
        Struck::Pasted("y\ny\ny".to_owned()),
        Struck::Key(press(Key::Char('y'))),
    ]);
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let mut shell = shell();
    let pace = Held::default();
    let answered = {
        let pane = std::sync::Mutex::new(TurnPane::of(&mut shell, &mut surface));
        PaneConfirm::over(&pane, &source, &pace)
            .confirm(&Question {
                statement: "Allow fs.write /tmp/note.txt?".to_owned(),
                prominent: false,
            })
            .expect("the terminal answered")
    };
    assert!(
        answered,
        "the `y` after the paste was not read, so the paste consumed the answer"
    );
    assert_eq!(
        shell.composer().text(),
        "",
        "the paste reached the composer while a question stood; it holds {:?}",
        shell.composer().text()
    );
}

/// A task queued during a turn is submitted the moment the turn ends, without
/// a keystroke, through the one path a typed `Enter` takes.
///
/// # The staging, and why it can tell the two apart
///
/// The source carries **exactly** the keys for the first task and nothing
/// after it. A pump that waited for a keystroke before submitting what was
/// queued would fall out of its loop when the source ended and leave at
/// `Exit::Succeeded`; one that drains before reading the terminal runs
/// `/exit` and leaves through the word. The two exits are the same code, so
/// the discriminator is the **pane**: leaving through `/exit` never paints a
/// second refusal, where a second read would have painted nothing at all.
///
/// It also asserts the one-path property directly: `/exit` is a command
/// rather than a task, and it is honoured — so the queue reaches
/// `Shell::submit` rather than a second grammar that treats everything
/// queued as task words.
///
/// The mutants: never drain; drain only after the next keystroke; submit the
/// queued line as a task rather than through `Shell::submit`.
#[test]
fn a_queued_task_runs_when_the_turn_ends_with_no_keystroke() {
    let restores: Restores = Arc::new(AtomicUsize::new(0));
    let mut surface = Recording::of(Arc::clone(&restores));
    let trie = NotesTrie::nothing_cached(WORKSPACE);
    // The first task's keys and nothing else. `typed` appends the `Enter`.
    let source = Source::scripted(typed("the first thing"));
    let pace = Held::default();
    let mut shell = shell();
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    // Staged as though an `Enter` during the first turn had queued it. The
    // pump's own mid-turn path is asserted by
    // `a_keystroke_during_a_turn_is_painted_and_enter_queues_it_as_the_next_task`;
    // this check is about what the pump does with one that is already there,
    // which `Turnable::Cannot` cannot produce because it races no turn.
    shell.queue(zaru_tui::shell::Queued::of("/exit"));
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
        &trie,
        &Vocabulary,
        &mut turns,
    ))
    .expect("the recording terminal never fails");

    assert!(
        matches!(
            pumped.outcome,
            crate::terminal::driver::Pumped::Left(Exit::Succeeded)
        ),
        "the pump did not leave"
    );
    assert_eq!(
        shell.queued(),
        None,
        "the queued task is still queued after the turn ended: {:?}",
        shell.queued()
    );
    // One refusal, from the one task the source carried. A queued `/exit`
    // that had been read as task words rather than as a command would have
    // produced a second.
    let refusals = shell
        .pane_lines()
        .iter()
        .filter(|line| line.text.contains(CANNOT))
        .count();
    assert_eq!(
        refusals, 1,
        "the queued `/exit` was submitted as a task rather than through the one path a typed \
         `Enter` takes, so the pane holds {refusals} refusals rather than one"
    );
    let last = surface.frames.last().expect("no frame was painted");
    assert!(
        !last.iter().any(|row| row.contains("queued")),
        "the pinned row outlived the task it was about: {last:?}"
    );
}
