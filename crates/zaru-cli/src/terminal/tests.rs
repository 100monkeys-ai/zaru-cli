// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::cli::invocation::{Overrides, Request};
use crate::cli::namespace::Namespace;
use crate::failure::Exit;
use crate::terminal::driver::{Guard, question_for_the_shell, request_for, run};
use crate::terminal::fixtures::{Counting, Recording, Restores, press, typed};
use crate::terminal::open::is_a_session;
use crate::terminal::trie::{NOTHING_CACHED, NotesTrie};
use crate::terminal::vocabulary::Vocabulary;
use crate::tools::port::Question;
use core::cell::Cell;
use std::rc::Rc;
use zaru_notes::trie::{CachedEntry, EntryKind as CachedKind};
use zaru_tui::shell::port::CommandVocabulary;
use zaru_tui::shell::{COMPOSER_ROWS, Key, Shell, Status};

const VERSION: &str = "0.0.0";

/// The workspace the checks below attach their sessions to.
const WORKSPACE: &str = "zaru";

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
    let restores: Restores = Rc::new(Cell::new(0));
    let mut surface = Recording::of(keys, Rc::clone(&restores));
    let mut shell = shell();
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    shell.composer_mut().set_absence(trie.absence());
    let pumped = run(&mut shell, &mut surface, &runner, trie, &Vocabulary)
        .expect("the recording terminal never fails");
    (shell, surface, pumped.exit)
}

// ------------------------------------------- the terminal is always given back

/// A restore written at the end of a loop is a restore that happens on the
/// paths the author thought of. This one is a `Drop`.
#[test]
fn the_terminal_is_restored_on_an_ordinary_exit() {
    let restores: Restores = Rc::new(Cell::new(0));
    {
        let _guard = Guard::new(Counting(Rc::clone(&restores)));
        assert_eq!(
            restores.get(),
            0,
            "the guard restored before it was dropped"
        );
    }
    assert_eq!(
        restores.get(),
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
    let restores: Restores = Rc::new(Cell::new(0));
    let counted = Rc::clone(&restores);

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
        restores.get(),
        1,
        "the terminal was not given back when the shell panicked"
    );
}

/// Restoring by hand and then dropping hands the terminal back once, not
/// twice. A second restore would leave the alternate screen a second time,
/// which on a real terminal scrolls the user's own scrollback away.
#[test]
fn the_terminal_is_restored_exactly_once_when_it_is_also_restored_by_hand() {
    let restores: Restores = Rc::new(Cell::new(0));
    {
        let mut guard = Guard::new(Counting(Rc::clone(&restores)));
        guard.restore_now();
        assert_eq!(restores.get(), 1);
        guard.restore_now();
    }
    assert_eq!(
        restores.get(),
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
        said.contains(crate::terminal::driver::NO_TASK_TURN_YET),
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
    let restores: Restores = Rc::new(Cell::new(0));
    let mut surface = Recording::of(vec![press(Key::Enter)], Rc::clone(&restores));
    let mut shell = shell();
    shell.ask(question_for_the_shell(&Question {
        statement: "run `rm -rf build`".to_owned(),
        prominent: true,
    }));
    let runner = crate::cli::Run {
        version: VERSION,
        report_at: REPORT_AT,
    };
    run(
        &mut shell,
        &mut surface,
        &runner,
        &NotesTrie::nothing_cached(WORKSPACE),
        &Vocabulary,
    )
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
