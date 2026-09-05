// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::composer::fixtures::{TrieOf, typing};
use crate::shell::command::{LEAVE, Refused, Typed, read};
use crate::shell::fixtures::{
    SECRET_NONCE, StagedTranscript, StagedVocabulary, TRANSCRIPT_NONCE, painted,
};
use crate::shell::port::{CommandVocabulary, Confirmation, Line, Register};
use crate::shell::{Action, COMPOSER_ROWS, Leaving, Shell, Status};
use core::time::Duration;
use tui_textarea::{Input, Key};

const WIDTH: u16 = 60;
const HEIGHT: u16 = 16;
const NOW: Duration = Duration::from_millis(10);

/// The answers line a check hands the shell.
///
/// **A literal this check owns, and deliberately not the product's.** The one
/// source is `zaru_cli::tools::prompt::SUFFIX`, which this crate cannot name;
/// what the shell owes is to paint whatever it was handed, and a check that
/// read the product's constant would be comparing it with itself.
/// `zaru-cli`'s `a_confirmation_renders_its_default_through_the_pump` is where
/// the real vocabulary is asserted to reach the buffer.
const STAGED_ANSWERS: &str = "[y/N]";

fn shell() -> Shell {
    Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"))
}

fn key(shell: &mut Shell, key: Key) -> Action {
    shell.key(
        Input {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        },
        NOW,
        &TrieOf::new(0),
        &StagedVocabulary,
    )
}

fn line(shell: &mut Shell, text: &str) -> Action {
    typing(shell.composer_mut(), text, NOW, &TrieOf::new(0));
    key(shell, Key::Enter)
}

// ---------------------------------------------------------------- ADR-0005 D2

/// ADR-0005 D2, one layer out. "The strip renders below the input and its
/// height changes never reflow the text the user is composing. The cursor does
/// not move because a search result arrived."
///
/// That record's clause 5 asserts it inside the composer's own area. This
/// asserts it inside the **shell's** frame, which is where it can actually
/// fail: a host that sized the composer's area by `Composer::height` and
/// anchored it to the bottom would move the input row every time the strip
/// grew, and nothing in the composer's own checks would notice.
///
/// The strip size is the only variable that moves.
#[test]
fn the_input_row_is_byte_identical_whatever_the_strip_shows_inside_the_shell() {
    let mut rows = Vec::new();
    let mut cursors = Vec::new();
    for count in [0usize, 1, 6] {
        let mut shell = shell();
        let trie = TrieOf::new(count);
        typing(shell.composer_mut(), "mémo", NOW, &trie);
        let (painted_rows, cursor) = painted(&shell, WIDTH, HEIGHT);
        let input_row = usize::from(HEIGHT - COMPOSER_ROWS);
        rows.push(painted_rows[input_row].clone());
        cursors.push(cursor);
    }

    assert_eq!(
        rows[0], rows[1],
        "the input row moved between a strip of nothing and a strip of one line: {:?} then {:?}",
        rows[0], rows[1]
    );
    assert_eq!(
        rows[0], rows[2],
        "the input row moved between a strip of nothing and a strip of six lines: {:?} then {:?}",
        rows[0], rows[2]
    );
    assert_eq!(
        cursors[0], cursors[1],
        "the cursor moved between a strip of nothing and a strip of one line"
    );
    assert_eq!(
        cursors[0], cursors[2],
        "the cursor moved between a strip of nothing and a strip of six lines"
    );
    assert!(
        rows[0].starts_with("mémo"),
        "the row read back is not the input row at all: {:?}",
        rows[0]
    );
}

// ---------------------------------------------------------------- ADR-0001 D2

/// ADR-0001 D2: "Status line renders the tier at all times. A user must never
/// be uncertain which membrane they are inside."
///
/// **At all times** is the load-bearing half, so this renders four genuinely
/// different states rather than one: empty, mid-composition with a strip up,
/// a full pane, and a standing confirmation. A status line that survived only
/// the quiet state would satisfy a check that rendered one frame.
#[test]
fn the_status_line_names_the_tier_in_every_state() {
    let expected = "runtime.tier = bare · session 01JQZX8N3K4M5P6R7S8T9V0W1X";

    let mut states: Vec<(&str, Shell)> = Vec::new();
    states.push(("an empty prompt", shell()));

    let mut typed = shell();
    typing(typed.composer_mut(), "mémo", NOW, &TrieOf::new(6));
    states.push(("mid-composition with six strip rows", typed));

    let mut full = shell();
    full.refresh(&StagedTranscript(
        (0..40)
            .map(|i| Line::new(Register::Plain, format!("line {i}")))
            .collect(),
    ));
    states.push(("a pane longer than the terminal", full));

    let mut asking = shell();
    asking.ask(Confirmation::new(
        "delete every file under /tmp",
        STAGED_ANSWERS,
        true,
    ));
    states.push(("a standing confirmation", asking));

    for (what, shell) in states {
        let (rows, _) = painted(&shell, WIDTH, HEIGHT);
        assert!(
            rows[0].starts_with(expected),
            "the status line does not name the tier with {what}; row 0 was {:?}",
            rows[0]
        );
    }
}

// -------------------------------------------------------- ADR-0008 clauses 4, 5

/// ADR-0008 clause 4: "Exhaustion renders distinctly from both success and
/// error, and a test asserts the distinction."
///
/// D5's argument is that exhaustion "is not an error and is not a success", so
/// what this holds is a **three-way** distinction rather than "exhaustion
/// looks like something". All three are read out of one painted frame.
#[test]
fn exhaustion_renders_distinctly_from_both_success_and_error() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript::three_outcomes());
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);

    let succeeded = rows
        .iter()
        .find(|row| row.contains("succeeded after"))
        .expect("no success line was painted");
    let exhausted = rows
        .iter()
        .find(|row| row.contains("exhausted at the ceiling"))
        .expect("no exhaustion line was painted");
    let failed = rows
        .iter()
        .find(|row| row.contains("no credential for alias"))
        .expect("no failure line was painted");

    let marker = |row: &str| row.chars().next().expect("an empty painted row");
    assert_ne!(
        marker(exhausted),
        marker(succeeded),
        "ADR-0008 D5 says exhaustion is not a success, and both lines open with the same glyph: \
         {exhausted:?} against {succeeded:?}"
    );
    assert_ne!(
        marker(exhausted),
        marker(failed),
        "ADR-0008 D5 says exhaustion is not an error, and both lines open with the same glyph: \
         {exhausted:?} against {failed:?}"
    );
    assert_ne!(
        marker(succeeded),
        marker(failed),
        "a success and a failure open with the same glyph: {succeeded:?} against {failed:?}"
    );
}

/// Every register has a glyph of its own.
///
/// The check above asserts the three the record names. This asserts the
/// property the record's argument rests on for the whole set, so a seventh
/// register cannot arrive sharing a marker with something it is not.
#[test]
fn no_two_registers_share_a_glyph() {
    let mut seen: Vec<(&'static str, Register)> = Vec::new();
    for register in Register::ALL {
        let glyph = register.glyph();
        if let Some((_, other)) = seen.iter().find(|(taken, _)| *taken == glyph) {
            panic!("{register:?} and {other:?} both render as {glyph:?}");
        }
        seen.push((glyph, register));
    }
    assert_eq!(seen.len(), Register::ALL.len());
}

/// ADR-0008 clause 5: "Per-iteration elapsed time appears in the rendered
/// output." D6: "Each iteration renders its own elapsed time as it completes...
/// The loop trades wall-clock for correctness and that trade must be visible
/// while it is being paid."
///
/// Read out of the painted buffer rather than out of the line the check
/// planted, so a renderer that dropped the tail of a line would redden.
#[test]
fn every_iteration_line_carries_its_own_elapsed_time_in_the_rendered_output() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript::three_outcomes());
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);

    for expected in ["4.20s", "9.10s"] {
        assert!(
            rows.iter().any(|row| row.contains(expected)),
            "no painted row carries the elapsed time {expected:?}; the pane was {rows:#?}"
        );
    }
}

// ---------------------------------------------------- ADR-0010 D4, the tail

/// ADR-0010 D4: resume "re-renders the last stretch of transcript so the user
/// can see where they were".
///
/// The **last** stretch. A pane that showed the head would be showing a user
/// where they started, which is the one place they are not.
#[test]
fn the_pane_shows_the_tail_of_a_transcript_longer_than_it_is() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript(
        (0..40)
            .map(|i| Line::new(Register::Plain, format!("{TRANSCRIPT_NONCE}-{i}")))
            .collect(),
    ));
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);
    let painted_pane = rows.join("\n");

    assert!(
        painted_pane.contains(&format!("{TRANSCRIPT_NONCE}-39")),
        "the last transcript line is not on the pane"
    );
    assert!(
        !painted_pane.contains(&format!("{TRANSCRIPT_NONCE}-0-")),
        "the first transcript line is on the pane, so this is the head rather than the tail"
    );
}

// ------------------------------------------------- ADR-0015 D2, the grammar

/// The accepting sibling of every refusal below.
///
/// A grammar that refused everything would satisfy each refusal check on its
/// own, so this asserts the whole vocabulary parses — walked from the
/// vocabulary rather than listed here, so a namespace added to the table is
/// covered without anybody remembering.
#[test]
fn every_namespace_the_vocabulary_carries_is_read_as_itself() {
    let vocabulary = StagedVocabulary;
    for namespace in vocabulary.namespaces() {
        let typed = match namespace.verbs.first() {
            Some(verb) => format!("{} {verb}", namespace.slash),
            None => namespace.slash.to_owned(),
        };
        match read(&typed, &vocabulary) {
            Typed::Command(command) if namespace.built => {
                assert_eq!(command.slash, namespace.slash);
                assert_eq!(command.verb, namespace.verbs.first().copied());
            }
            Typed::Refused(Refused::NotBuilt { slash, .. }) if !namespace.built => {
                assert_eq!(slash, namespace.slash);
            }
            other => panic!("{typed:?} read as {other:?}, which is neither of the two answers"),
        }
    }
}

/// ADR-0014 D5's nearest match, on the in-session surface.
#[test]
fn a_slash_word_that_names_no_namespace_is_refused_naming_the_nearest() {
    let Typed::Refused(refusal) = read("/sessoin list", &StagedVocabulary) else {
        panic!("a word naming no namespace was not refused");
    };
    let Refused::UnknownCommand { offered, nearest } = &refusal else {
        panic!("refused as {refusal:?} rather than as an unknown command");
    };
    assert_eq!(offered, "sessoin");
    assert_eq!(
        *nearest,
        Some("/session"),
        "the nearest to `sessoin` is not `/session`"
    );
    assert!(
        refusal.to_string().contains("/session"),
        "the refusal does not name the nearest: {refusal}"
    );
}

/// ADR-0015 D2's four unbuilt namespaces. The out-of-session surface refuses
/// these saying so rather than placing them against a nearest, "because
/// telling a user who typed `stack` that they may have meant `sessions` is a
/// worse answer than the truth", and the in-session surface is the same
/// operation.
#[test]
fn an_unbuilt_namespace_is_refused_saying_so_and_never_placed_against_a_nearest() {
    for (slash, governs) in [
        ("/stack", "AEGIS component fetch and status"),
        ("/memory", "relationship memory"),
        ("/learned", "what this session wrote to craft memory"),
        ("/inbox", "deposits"),
    ] {
        let Typed::Refused(refusal) = read(slash, &StagedVocabulary) else {
            panic!("{slash} was not refused");
        };
        assert_eq!(
            refusal,
            Refused::NotBuilt { slash, governs },
            "{slash} was refused as something other than unbuilt"
        );
        let stated = refusal.to_string();
        assert!(
            stated.contains(governs),
            "the refusal does not say what {slash} governs: {stated}"
        );
        assert!(
            !stated.contains("nearest"),
            "the refusal placed {slash} against a nearest: {stated}"
        );
    }
}

/// A verb the namespace does not take, with its own nearest.
#[test]
fn a_verb_a_namespace_does_not_take_is_refused_naming_the_nearest() {
    let Typed::Refused(Refused::UnknownVerb {
        slash,
        offered,
        nearest,
    }) = read("/session resmue 01J", &StagedVocabulary)
    else {
        panic!("an unknown verb was not refused as one");
    };
    assert_eq!(slash, "/session");
    assert_eq!(offered, "resmue");
    assert_eq!(nearest, Some("resume"));
}

/// A namespace that takes verbs and was given none says which it takes.
#[test]
fn a_namespace_that_needs_a_verb_and_was_given_none_lists_the_verbs() {
    let Typed::Refused(refusal) = read("/session", &StagedVocabulary) else {
        panic!("a bare namespace was not refused");
    };
    let stated = refusal.to_string();
    for verb in ["resume", "continue", "list", "rm"] {
        assert!(
            stated.contains(verb),
            "the refusal does not list `{verb}`: {stated}"
        );
    }
}

/// ADR-0010 D4's in-session spellings: "Inside a session the same operation is
/// `/session resume <id>` and `/session continue`."
#[test]
fn the_in_session_session_verbs_are_the_ones_adr_0010_d4_names() {
    for (typed, verb, words) in [
        ("/session resume 01JQZX", "resume", vec!["01JQZX"]),
        ("/session continue", "continue", vec![]),
    ] {
        let Typed::Command(command) = read(typed, &StagedVocabulary) else {
            panic!("{typed} was not read as a command");
        };
        assert_eq!(command.slash, "/session");
        assert_eq!(command.verb, Some(verb));
        assert_eq!(command.words, words);
    }
}

/// A namespace that takes no verb is a whole command on its own, exactly as
/// `zaru runtime` is.
#[test]
fn a_namespace_with_no_verbs_is_a_whole_command() {
    for slash in ["/runtime", "/models", "/init"] {
        let Typed::Command(command) = read(slash, &StagedVocabulary) else {
            panic!("{slash} was not read as a command");
        };
        assert_eq!(command.verb, None);
        assert!(command.words.is_empty());
    }
}

/// A leading slash is what makes a command a command, and nothing else is one.
#[test]
fn a_line_with_no_leading_slash_is_a_task() {
    let Typed::Task(task) = read("rename the widget and run the tests", &StagedVocabulary) else {
        panic!("a sentence was not read as a task");
    };
    assert_eq!(task, "rename the widget and run the tests");

    // A single word with no slash is a task too, and this is where the
    // in-session grammar deliberately differs from the out-of-session one:
    // outside, "a command is a word and a task is a sentence"; inside, the
    // user has a character to spend on saying which.
    assert_eq!(
        read("runtime", &StagedVocabulary),
        Typed::Task("runtime".to_owned())
    );
}

/// ADR-0015 D2: "A user command may not shadow a built-in namespace."
///
/// Asserted against whatever the vocabulary carries rather than against a list
/// written here, so an eleventh namespace spelled `/exit` is caught by this
/// check rather than by somebody remembering.
#[test]
fn the_shells_own_leave_word_shadows_no_namespace() {
    let taken: Vec<&'static str> = StagedVocabulary
        .namespaces()
        .into_iter()
        .map(|namespace| namespace.slash)
        .collect();
    assert!(
        !taken.contains(&LEAVE),
        "{LEAVE} shadows one of ADR-0015 D2's namespaces: {taken:?}"
    );
    assert_eq!(read(LEAVE, &StagedVocabulary), Typed::Leave);
}

// ------------------------------------------------------------------- leaving

/// Both ways out, and both exit 0.
#[test]
fn both_ways_of_leaving_exit_zero() {
    let mut typed = shell();
    assert_eq!(line(&mut typed, LEAVE), Action::Leave(Leaving::Word));

    let mut interrupted = shell();
    let action = interrupted.key(
        Input {
            key: Key::Char('c'),
            ctrl: true,
            alt: false,
            shift: false,
        },
        NOW,
        &TrieOf::new(0),
        &StagedVocabulary,
    );
    assert_eq!(action, Action::Leave(Leaving::Interrupt));

    for leaving in Leaving::ALL {
        assert_eq!(
            leaving.code(),
            0,
            "{leaving:?} does not exit 0, and ADR-0016 D5's 0 is what a user who asked to leave \
             and left got"
        );
    }
}

/// `leaves` and `Shell::key` are one rule, asserted over a keyboard rather
/// than over the one key the rule is about.
///
/// A host reads keystrokes while a turn is running, when the shell is not what
/// the turn is waiting on, and it asks this function rather than spelling
/// `ctrl` and `c` a second time. So the two have to agree on **every** input,
/// not only on the interrupt — a `leaves` that answered `Some` for `Ctrl-D`
/// would make a host leave on a key the shell hands the composer.
///
/// # Why the expectation is spelled out here rather than compared
///
/// **The first form of this check was a tautology and a mutation said so.**
/// It asserted only that `leaves` and `Shell::key` agree, and they agree by
/// construction because the second calls the first — library verification
/// lessons §11, both arms travelling through the thing being checked. Its
/// staging assertion counted how many combinations left, which is *four*
/// whether the rule reads `ctrl` or `alt`, so the count was ordinary on the
/// axis the mutant moved (§51). Swapping `ctrl` for `alt` left the check
/// green.
///
/// So one arm is now the check's own literal statement of the rule — `ctrl`
/// set, `alt` clear, the code `c` — written here and derived from nothing.
/// The agreement between `leaves` and `Shell::key` is still asserted, because
/// it is what catches `Shell::key` growing a second spelling, but it is no
/// longer what holds the rule.
#[test]
fn the_leave_rule_has_one_spelling_and_the_shell_uses_it() {
    // A keyboard, not a key: every combination of the three modifiers over a
    // handful of codes, so `c` sits in the middle of the run rather than at
    // either end of it (library verification-lessons §54).
    let codes = [
        Key::Char('a'),
        Key::Enter,
        Key::Char('c'),
        Key::Esc,
        Key::Char('d'),
        Key::Backspace,
    ];
    let mut interrupts = 0;
    let mut walked = 0;
    for code in codes {
        for ctrl in [false, true] {
            for alt in [false, true] {
                for shift in [false, true] {
                    walked += 1;
                    let input = Input {
                        key: code,
                        ctrl,
                        alt,
                        shift,
                    };

                    // The independent arm: what the rule is, said here rather
                    // than read back from the thing under test. `alt` and
                    // `shift` are free, which is what the branch this replaced
                    // already did — a terminal reports the chord several ways
                    // and no record narrows it, so widening or narrowing it
                    // here would be a behaviour decision wearing a check's
                    // clothes.
                    let expected = (ctrl && code == Key::Char('c')).then_some(Leaving::Interrupt);
                    let ruled = crate::shell::leaves(&input);
                    assert_eq!(
                        ruled, expected,
                        "`leaves` answered {ruled:?} for {code:?} with ctrl={ctrl} alt={alt} \
                         shift={shift}; the rule is ctrl set and the code `c`, with alt and \
                         shift free, so it should have answered {expected:?}"
                    );

                    let mut shell = shell();
                    let acted = shell.key(input, NOW, &TrieOf::new(0), &StagedVocabulary);
                    let acted_leave = match acted {
                        Action::Leave(leaving) => Some(leaving),
                        Action::Idle | Action::Run(_) | Action::Task(_) => None,
                    };
                    assert_eq!(
                        ruled, acted_leave,
                        "`leaves` and `Shell::key` disagree about {code:?} with ctrl={ctrl} \
                         alt={alt} shift={shift}: the rule says {ruled:?} and the shell did \
                         {acted_leave:?}, so the shell has a second spelling of this rule"
                    );
                    if ruled.is_some() {
                        interrupts += 1;
                    }
                }
            }
        }
    }
    // Assert the staging as well: a walk that reached no interrupt at all
    // would satisfy every comparison above.
    assert_eq!(
        walked,
        codes.len() * 8,
        "the walk covered {walked} combinations rather than {}",
        codes.len() * 8
    );
    assert_eq!(
        interrupts, 4,
        "the keyboard walked {walked} combinations and {interrupts} of them left; `Ctrl-C` is one \
         code with ctrl set and alt and shift free, which is four"
    );
}

/// `Ctrl-C` leaves from mid-line, not only from an empty prompt.
#[test]
fn an_interrupt_leaves_from_the_middle_of_a_line() {
    let mut shell = shell();
    typing(shell.composer_mut(), "half a task", NOW, &TrieOf::new(0));
    let action = shell.key(
        Input {
            key: Key::Char('c'),
            ctrl: true,
            alt: false,
            shift: false,
        },
        NOW,
        &TrieOf::new(0),
        &StagedVocabulary,
    );
    assert_eq!(action, Action::Leave(Leaving::Interrupt));
}

// -------------------------------------------------------------- ADR-0011 D3

/// The default is decline, asserted twice over: in the rendered prompt and in
/// the value `Enter` produces.
#[test]
fn a_confirmation_defaults_to_decline() {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "run `rm -rf build`",
        STAGED_ANSWERS,
        false,
    ));
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);
    assert!(
        rows.iter().any(|row| row.contains("[y/N]")),
        "the prompt does not render its default; the frame was {rows:#?}"
    );

    assert_eq!(key(&mut shell, Key::Enter), Action::Idle);
    assert_eq!(
        shell.answer(),
        Some(false),
        "Enter on a prompt whose default is N did not decline"
    );
    assert!(shell.asking().is_none(), "the question is still standing");
}

/// The accepting sibling: a refusal that refuses everything is not a
/// confirmation.
#[test]
fn an_explicit_yes_accepts() {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "run `rm -rf build`",
        STAGED_ANSWERS,
        false,
    ));
    assert_eq!(key(&mut shell, Key::Char('y')), Action::Idle);
    assert_eq!(shell.answer(), Some(true));
}

/// `n` and `Esc` decline as `Enter` does.
#[test]
fn an_explicit_no_and_an_escape_both_decline() {
    for pressed in [Key::Char('n'), Key::Esc] {
        let mut shell = shell();
        shell.ask(Confirmation::new(
            "run `rm -rf build`",
            STAGED_ANSWERS,
            false,
        ));
        key(&mut shell, pressed);
        assert_eq!(shell.answer(), Some(false), "{pressed:?} did not decline");
    }
}

/// A stray keystroke answers nothing.
///
/// The safe direction: a prompt whose outcome depended on a key the user did
/// not mean would make the answer a fact about their typing rather than about
/// their decision.
#[test]
fn a_key_that_is_neither_yes_nor_no_leaves_the_question_standing() {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "run `rm -rf build`",
        STAGED_ANSWERS,
        false,
    ));
    key(&mut shell, Key::Char('z'));
    assert_eq!(shell.answer(), None, "`z` answered the question");
    assert!(
        shell.asking().is_some(),
        "the question stopped standing without being answered"
    );
}

/// ADR-0011 D3: "`ask` — Prompts before any write or command." A prompt the
/// user can type past is not a prompt.
#[test]
fn a_standing_question_takes_every_key_and_the_composer_receives_none() {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "run `rm -rf build`",
        STAGED_ANSWERS,
        false,
    ));
    for ch in "hello".chars() {
        key(&mut shell, Key::Char(ch));
    }
    assert_eq!(
        shell.composer().text(),
        "",
        "keystrokes reached the composer while a question was standing"
    );
    assert!(shell.asking().is_some());
}

/// ADR-0011 D6's marking, which raises the prompt without changing what it can
/// do.
#[test]
fn a_destructive_question_renders_more_prominently_than_an_ordinary_one() {
    let mut prominent = shell();
    prominent.ask(Confirmation::new(
        "delete every file under /tmp",
        STAGED_ANSWERS,
        true,
    ));
    let (loud, _) = painted(&prominent, WIDTH, HEIGHT);

    let mut ordinary = shell();
    ordinary.ask(Confirmation::new(
        "delete every file under /tmp",
        STAGED_ANSWERS,
        false,
    ));
    let (quiet, _) = painted(&ordinary, WIDTH, HEIGHT);

    assert_ne!(
        loud, quiet,
        "a destructive question paints the same frame as an ordinary one, so D6's marking is not \
         reaching the buffer"
    );
    assert!(
        loud.iter()
            .any(|row| row.contains("! delete every file under /tmp")),
        "the prominent marking is not in the frame: {loud:#?}"
    );
}

// ------------------------------------------------------- the security corpus

/// The pane is a view of the record, and the record holds what the session
/// held.
///
/// ADR-0008's clause-6 Update puts the `Redactor` on "every path from captured
/// bytes into a **model prompt or request**", and ADR-0010's Negative section
/// says the transcript "contains whatever the session contained... Filesystem
/// permissions are the only protection". A pane that differed from the file it
/// views could not be what D2 calls replayable.
///
/// So this is the mirror of `zaru-cli`'s `redaction_from_outside.rs`: the
/// planted value **reaches the buffer**, and reaches nothing else — no
/// refusal, no `Debug` of anything that is *about* the session rather than
/// *is* its data, and nothing the shell hands its host.
#[test]
fn a_held_secret_in_a_transcript_line_reaches_the_buffer_and_nothing_else() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript(vec![Line::new(
        Register::Call,
        format!("cmd.run `curl -H 'Authorization: Bearer {SECRET_NONCE}'`"),
    )]));

    let (rows, _) = painted(&shell, 120, HEIGHT);
    assert!(
        rows.iter().any(|row| row.contains(SECRET_NONCE)),
        "the transcript's own bytes did not reach the pane, so the pane is not a view of the \
         record: {rows:#?}"
    );

    // Everything the shell says about itself, rather than shows.
    let mut said = format!("{:?}", shell.status());
    said.push_str(&format!("{:?}", Refused::Empty));
    for namespace in StagedVocabulary.namespaces() {
        said.push_str(&format!("{namespace:?}"));
    }
    let action = line(&mut shell, "/stack install");
    said.push_str(&format!("{action:?}"));
    assert!(
        !said.contains(SECRET_NONCE),
        "a value from the transcript reached something that is about the session rather than is \
         its data: {said}"
    );
}

/// The accepting sibling of the check above: the walk over `said` must be
/// capable of finding a value that is genuinely there, or its absence
/// assertion passes vacuously.
#[test]
fn the_absence_walk_finds_a_value_that_is_actually_in_what_the_shell_says() {
    let shell = Shell::open(Status::new("bare", SECRET_NONCE));
    let said = format!("{:?}", shell.status());
    assert!(
        said.contains(SECRET_NONCE),
        "the walk cannot see a value planted where it looks, so its absence assertion says nothing"
    );
}

// ---------------------------------------------------------- the shell's loop

/// A refused slash line is shown rather than swallowed, and the shell stays
/// open.
#[test]
fn a_refused_slash_line_appears_on_the_pane_and_the_shell_stays_open() {
    let mut shell = shell();
    assert_eq!(line(&mut shell, "/sessoin list"), Action::Idle);
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);
    assert!(
        rows.iter()
            .any(|row| row.contains("there is no `/sessoin`")),
        "the refusal is not on the pane: {rows:#?}"
    );
    assert_eq!(
        shell.composer().text(),
        "",
        "the composer kept the refused line"
    );
}

/// A task is handed to the host rather than acted on here.
#[test]
fn a_task_is_handed_out_and_the_composer_is_cleared() {
    let mut shell = shell();
    assert_eq!(
        line(&mut shell, "rename the widget"),
        Action::Task("rename the widget".to_owned())
    );
    assert_eq!(shell.composer().text(), "");
}

/// An empty line does nothing at all.
#[test]
fn an_empty_line_is_absorbed() {
    let mut shell = shell();
    assert_eq!(key(&mut shell, Key::Enter), Action::Idle);
    assert!(shell.pane_lines().is_empty());
}

/// A refresh replaces the transcript and keeps this session's own notices.
///
/// The two are different things: one is the record on disk and the other is
/// what this terminal said, and a refresh that lost the second would make a
/// refusal disappear the moment the loop wrote a line.
#[test]
fn a_refresh_replaces_the_transcript_and_keeps_the_notices() {
    let mut shell = shell();
    shell.notice(Line::new(Register::Failed, "a refusal"));
    shell.refresh(&StagedTranscript(vec![Line::new(Register::Plain, "first")]));
    shell.refresh(&StagedTranscript(vec![
        Line::new(Register::Plain, "first"),
        Line::new(Register::Plain, "second"),
    ]));

    let painted: Vec<String> = shell.pane_lines().iter().map(Line::painted).collect();
    assert_eq!(
        painted,
        vec![
            "  first".to_owned(),
            "  second".to_owned(),
            "✗ a refusal".to_owned()
        ]
    );
}
