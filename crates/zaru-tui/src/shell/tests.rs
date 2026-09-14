// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::composer::fixtures::{TrieOf, typing};
use crate::shell::command::{LEAVE, Refused, Typed, read};
use crate::shell::fixtures::{
    SECRET_NONCE, StagedTranscript, StagedVocabulary, TRANSCRIPT_NONCE, cells, painted,
};
use crate::shell::port::{CommandVocabulary, Confirmation, Line, Palette, Register, Row};
use crate::shell::{Action, COMPOSER_ROWS, Leaving, Segment, Shell, Status};
use core::time::Duration;
use ratatui::style::Color;
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

// ----------------------------------------- ADR-0013 clause 5, ADR-0012 clause 6

/// A status line carrying neither segment paints exactly what it did before
/// either existed.
///
/// The regression guard for widening the row: [ADR-0001] D2's line is what
/// every check in this workspace and every capture in every record quotes, and
/// a segment that contributed an empty separator would change all of them
/// while looking like nothing. Asserted byte for byte against a literal this
/// check owns, so neither arm travels through the formatter under test.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[test]
fn a_row_with_no_segments_paints_what_it_painted_before_the_segments_existed() {
    assert_eq!(
        Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X").painted(200),
        "runtime.tier = bare · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "a row carrying neither segment must be byte-identical to what it was"
    );
}

/// ADR-0013 clause 5 and ADR-0012 clause 6 both reach the painted buffer.
///
/// **The segments are nonces this check owns**, not numbers a renderer
/// produced: what this crate owes is to carry what it was handed onto the row,
/// and a check that composed a count here and then asserted the shell rendered
/// that count would be asserting about its own arithmetic
/// (\[Verification lessons\] §10 and §11). `zaru-cli`'s
/// `the_two_segments_are_the_registers_the_records_already_landed` is where
/// the wording is asserted against the records.
///
/// Read out of `TestBackend` rather than off `painted()`, because the claim is
/// that a *user* meets them: a row composed correctly and then dropped by the
/// renderer would satisfy a string comparison.
#[test]
fn both_records_numbers_reach_the_painted_row_in_the_order_the_arbitration_gives() {
    let mut shell = shell();
    shell.set_context_usage(Some("context 12.3k/1048.5k tokens".to_owned()));
    shell.set_token_usage(Some("tokens: 390 prompt + 79 completion = 469".to_owned()));

    let (rows, _) = painted(&shell, 160, HEIGHT);
    assert_eq!(
        rows[0].trim_end(),
        "runtime.tier = bare · context 12.3k/1048.5k tokens · tokens: 390 prompt + 79 \
         completion = 469 · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "both segments must reach the row, after the tier and before the session"
    );
}

/// Either segment alone contributes itself and one separator, never an empty
/// one.
///
/// The two are set independently — the token line is absent until an exchange
/// has happened while the context number exists from the session's first frame
/// — so both one-sided states are real and both are asserted. A check over the
/// pair alone would pass while a `None` printed a trailing ` · `.
#[test]
fn a_segment_that_is_absent_contributes_nothing_at_all_including_its_separator() {
    let mut only_context = Status::new("bare", "s");
    only_context.context = Some(Segment::same("context 1 tokens"));
    assert_eq!(
        only_context.painted(200),
        "runtime.tier = bare · context 1 tokens · session s",
        "an absent token segment must contribute no separator"
    );

    let mut only_tokens = Status::new("bare", "s");
    only_tokens.tokens = Some(Segment::same("tokens: 1 prompt + 2 completion = 3"));
    assert_eq!(
        only_tokens.painted(200),
        "runtime.tier = bare · tokens: 1 prompt + 2 completion = 3 · session s",
        "an absent context segment must contribute no separator"
    );
}

/// ADR-0001 D2's "at all times", at widths that cannot hold the whole row.
///
/// **This is the half the arbitration had to buy.** Two more segments make the
/// row long enough that a real terminal clips it, and D2's requirement is that
/// what survives is the tier. Nothing here elides by a rule of its own —
/// `ratatui` clips the right edge — so the property is entirely the *order*,
/// and the mutant that breaks it is a renderer that puts a number first.
///
/// Four widths, the narrowest below the tier's own spelling, so the check
/// covers the case where even `runtime.tier = bare` does not fit and the
/// prefix that survives is still the tier's.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[test]
fn the_tier_is_what_survives_a_width_too_narrow_for_the_whole_row() {
    let mut shell = shell();
    shell.set_context_usage(Some("context 786.4k/1048.5k tokens".to_owned()));
    shell.set_token_usage(Some("tokens: 390 prompt + 79 completion = 469".to_owned()));

    for width in [10_u16, 20, 44, 72] {
        let (rows, _) = painted(&shell, width, HEIGHT);
        let row = &rows[0];
        assert_eq!(
            row.chars().count(),
            usize::from(width),
            "the status row must fill the terminal's width exactly at {width}; row 0 was {row:?}"
        );
        let expected = "runtime.tier = bare";
        let head: String = expected.chars().take(usize::from(width)).collect();
        assert!(
            row.starts_with(&head),
            "at width {width} the row must still begin with the tier; row 0 was {row:?}"
        );
    }
}

// ------------------- ADR-0001 D2, ADR-0013 D6, ADR-0012 clause 6, ADR-0028 D5

/// Every field the row can carry, staged with the product's own spellings.
///
/// The two-spelling segments are staged as `Segment::new`, which is what a
/// host hands over; the widths below are a function of these exact strings and
/// nothing here recomputes them.
fn crowded() -> Shell {
    let mut shell = shell();
    shell.set_context_usage(Some(Segment::new(
        "context 1.2k/1048.5k tokens",
        "1.2k/1048.5k",
    )));
    shell.set_token_usage(Some(Segment::new(
        "tokens: 390 prompt + 79 completion = 469",
        "469 tokens",
    )));
    shell.set_elapsed(Some("12.34s".to_owned()));
    shell.describe(
        Some("gemini-3.6-flash".to_owned()),
        Some("mode ask".to_owned()),
    );
    shell
}

/// The 2026-09-06 amendment to [ADR-0001] D2, at the five widths it names.
///
/// **The assertion is which fields survive, never their column arithmetic.**
/// A check that recomputed the join and compared it with the join would be
/// asserting about its own arithmetic (\[Verification lessons\] §10); what the
/// amendment decided is an *order*, so the order is what is asserted, one
/// width at a time, against a list this check owns.
///
/// The narrowest width is where the amendment is honest rather than complete:
/// at 40 columns [ADR-0013] D6's figure survives and [ADR-0028] D5's meter
/// does not, because the two of them plus the tier come to 43 columns.
///
/// Watched red on: the ranks reversed, so the session outlives the context;
/// the narrow retry deleted, so 100 columns keeps three fields instead of six.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn the_row_keeps_the_ranks_the_records_own_at_each_of_five_widths() {
    let shell = crowded();

    // (width, the fields that must be on the row, the fields that must not be)
    let expected: [(u16, &[&str], &[&str]); 5] = [
        (
            200,
            &[
                "runtime.tier = bare",
                "gemini-3.6-flash",
                "mode ask",
                "context 1.2k/1048.5k tokens",
                "12.34s",
                "tokens: 390 prompt + 79 completion = 469",
                "session 01JQZX8N3K4M5P6R7S8T9V0W1X",
            ],
            &[],
        ),
        (
            100,
            &[
                "runtime.tier = bare",
                "gemini-3.6-flash",
                "mode ask",
                "1.2k/1048.5k",
                "12.34s",
                "469 tokens",
            ],
            &["session 01JQZX8N3K4M5P6R7S8T9V0W1X"],
        ),
        (
            80,
            &[
                "runtime.tier = bare",
                "gemini-3.6-flash",
                "1.2k/1048.5k",
                "12.34s",
                "469 tokens",
            ],
            &["mode ask", "session 01JQZX8N3K4M5P6R7S8T9V0W1X"],
        ),
        (
            60,
            &[
                "runtime.tier = bare",
                "1.2k/1048.5k",
                "12.34s",
                "469 tokens",
            ],
            &["gemini-3.6-flash", "mode ask"],
        ),
        (
            40,
            &["runtime.tier = bare", "1.2k/1048.5k"],
            &["12.34s", "469 tokens", "gemini-3.6-flash", "mode ask"],
        ),
    ];

    for (width, kept, gone) in expected {
        let row = shell.status().painted(width);
        assert!(
            crate::shell::wrap::columns(&row) <= usize::from(width),
            "the row must fit {width} columns; it was {:?}",
            row
        );
        for field in kept {
            assert!(
                row.contains(field),
                "at {width} columns the row must keep {field:?}; it was {row:?}"
            );
        }
        for field in gone {
            assert!(
                !row.contains(field),
                "at {width} columns the row must have dropped {field:?}; it was {row:?}"
            );
        }
    }
}

/// The survivors keep **display** order, which is not `Rank` order.
///
/// A field that moved sideways because another disappeared would make the row
/// unreadable at a glance, which is the whole purpose it serves. Asserted as
/// rising positions rather than as a joined literal, so the check states the
/// property rather than restating the formatter.
///
/// Watched red on: `joined` emitting survivors in rank order instead of the
/// order `fields` built them in.
#[test]
fn the_fields_that_survive_keep_their_display_order() {
    let shell = crowded();
    let display = [
        "runtime.tier = bare",
        "gemini-3.6-flash",
        "mode ask",
        "1.2k/1048.5k",
        "12.34s",
        "469 tokens",
        "session 01JQZX8N3K4M5P6R7S8T9V0W1X",
    ];

    for width in [200_u16, 120, 100, 80, 60, 40] {
        let row = shell.status().painted(width);
        let mut previous = 0_usize;
        for field in display {
            // The full spellings appear only at 200; `find` skips whatever
            // this width dropped or narrowed, and what is left must still
            // rise.
            if let Some(at) = row.find(field) {
                assert!(
                    at >= previous,
                    "at {width} columns {field:?} is out of display order; the row was {row:?}"
                );
                previous = at;
            }
        }
    }
}

/// The narrow spelling is used only where the full one will not fit.
///
/// Both spellings are the host's and both carry every number; what this crate
/// owes is to prefer the labelled one whenever the row can hold it, because a
/// label dropped for no reason is legibility spent for nothing.
///
/// Watched red on: `painted` trying the narrow join before the full one.
#[test]
fn the_narrow_spelling_is_used_only_when_the_full_one_will_not_fit() {
    let mut shell = shell();
    shell.set_context_usage(Some(Segment::new("the full spelling", "short")));

    // 19 for the tier, 3 for the separator, 17 for the full spelling.
    let exactly_enough = shell.status().painted(39);
    assert!(
        exactly_enough.contains("the full spelling"),
        "a row with room for the full spelling must use it; it was {exactly_enough:?}"
    );

    let one_column_short = shell.status().painted(38);
    assert!(
        one_column_short.contains("short") && !one_column_short.contains("the full spelling"),
        "a row one column short must use the narrow spelling; it was {one_column_short:?}"
    );
}

/// [ADR-0028] D5's meter is on the row only while there is a turn to measure.
///
/// Taking it off must leave no separator behind, which is the same property
/// the two segments before it already have and the same mutant reaches it.
///
/// Watched red on: an absent elapsed figure contributing an empty segment.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn an_elapsed_figure_taken_off_the_row_leaves_no_separator_behind() {
    let mut shell = shell();
    shell.set_elapsed(Some("4.20s".to_owned()));
    assert_eq!(
        shell.status().painted(200),
        "runtime.tier = bare · 4.20s · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "the meter sits between the tier and the session"
    );

    shell.set_elapsed(None);
    assert_eq!(
        shell.status().painted(200),
        "runtime.tier = bare · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "a turn that ended must leave the row exactly as it opened"
    );
}

/// The model and the mode reach the row, at the ranks the amendment gives them.
///
/// Neither answers a clause, which is why both are ranked below the three that
/// do; that they are *present* is survey row 14, and that they are present
/// **after** the tier and **before** the context figure is this crate's.
///
/// Watched red on: `describe` writing only the model; the model placed ahead
/// of the tier.
#[test]
fn the_model_and_the_mode_reach_the_row_where_the_amendment_puts_them() {
    let mut shell = shell();
    shell.describe(
        Some("gemini-3.6-flash".to_owned()),
        Some("mode yolo".to_owned()),
    );
    let (rows, _) = painted(&shell, 120, HEIGHT);
    assert_eq!(
        rows[0].trim_end(),
        "runtime.tier = bare · gemini-3.6-flash · mode yolo · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "both fields must reach the painted row, after the tier"
    );
}

/// A row whose fields would forge a second tier claim still has one tier, and
/// it is still in the row's first cells.
///
/// # The corpus case, and why it is this crate's as well as the host's
///
/// `model.<alias>` is free at every configuration layer, so a repository the
/// user cloned chooses the string at `Rank::Model`. The neutralisation is
/// the host's — it owns the wording — but the property is the row's: **the
/// tier occupies the row's first cells whatever any other field says.** This
/// asserts the *cell position* out of the painted buffer rather than only the
/// text, because a row that merely contained the tier somewhere would satisfy
/// a `contains`.
///
/// The accepting sibling is the second half: an ordinary identifier reaches
/// the row unchanged, so the property is not bought by refusing everything.
///
/// Watched red on: the tier emitted at any rank but 0.
#[test]
fn corpus_the_tier_holds_the_rows_first_cells_whatever_another_field_says() {
    let hostile = "x · runtime.tier = linked";
    let mut forged = shell();
    forged.describe(Some(hostile.to_owned()), None);
    let (rows, _) = painted(&forged, 120, HEIGHT);
    assert!(
        rows[0].starts_with("runtime.tier = bare · "),
        "the tier must hold the row's first cells; row 0 was {:?}",
        rows[0]
    );

    let mut ordinary = shell();
    ordinary.describe(Some("gemini-3.6-flash".to_owned()), None);
    let (rows, _) = painted(&ordinary, 120, HEIGHT);
    assert!(
        rows[0].starts_with("runtime.tier = bare · gemini-3.6-flash · "),
        "an ordinary identifier must reach the row unchanged; row 0 was {:?}",
        rows[0]
    );
}

/// An identifier longer than any terminal cannot displace the tier.
///
/// Watched red on: the tier dropped like any other field once the row
/// overflows.
#[test]
fn corpus_an_over_long_field_cannot_displace_the_tier() {
    let mut shell = shell();
    shell.describe(Some("m".repeat(4_000)), None);
    for width in [40_u16, 80, 200] {
        let (rows, _) = painted(&shell, width, HEIGHT);
        let expected = "runtime.tier = bare";
        let head: String = expected.chars().take(usize::from(width)).collect();
        assert!(
            rows[0].starts_with(&head),
            "at {width} columns the row must still begin with the tier; row 0 was {:?}",
            rows[0]
        );
        assert_eq!(
            rows.len(),
            usize::from(HEIGHT),
            "the status row must not have become more than one row"
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

/// The same clause for a paste, which is the other door into the composer.
///
/// # Why this is its own check and not an arm of the one above
///
/// `zaru-cli`'s `PaneConfirm` absorbs a paste at its own read, **and** this
/// guard absorbs one here, so the property has two enforcing sites and no
/// single-site mutation can redden a check driven through that adapter —
/// library verification lessons §68, which is a measurement of redundancy
/// rather than of a check. **Both are kept**: that one guards the synchronous
/// read a tool is awaiting, and this one is the shell's own rule, which has to
/// hold for every caller and not only for the adapter that exists today. This
/// check is the one that can falsify *this* site, and it is why the redundancy
/// is recorded rather than argued for.
///
/// Its accepting sibling is immediately below: with no question standing the
/// same paste does reach the composer, so the guard refuses rather than the
/// method doing nothing at all.
#[test]
fn a_standing_question_absorbs_a_paste_and_the_composer_receives_none() {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "run `rm -rf build`",
        STAGED_ANSWERS,
        false,
    ));
    shell.pasted("y\ny\ny", Duration::ZERO, &TrieOf::new(0));
    assert_eq!(
        shell.composer().text(),
        "",
        "a paste reached the composer while a question was standing; it holds {:?}",
        shell.composer().text()
    );
    assert!(
        shell.asking().is_some(),
        "the paste answered the question, which only y, n, Esc and Enter may do"
    );
}

/// The sibling: with nothing standing, the same paste reaches the composer
/// whole, newlines and all.
#[test]
fn a_paste_reaches_the_composer_whole_when_no_question_stands() {
    let mut shell = shell();
    shell.pasted("y\ny\ny", Duration::ZERO, &TrieOf::new(0));
    assert_eq!(
        shell.composer().text(),
        "y\ny\ny",
        "the paste did not reach the composer as its own bytes; it holds {:?}",
        shell.composer().text()
    );
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

// ---------------------------------------------------- the streamed answer

// The answer grows on the pane as it arrives, which is the whole point of a
// stream to a person waiting. Read out of `TestBackend` rather than off
// `pane_lines`, because the claim is about what a reader sees.
//
// **The mutant is `stream_delta` replacing rather than appending** -- a pane
// that shows only the newest piece, which looks like it is working right up
// until an answer arrives in more than one frame.
#[test]
fn a_streamed_answer_grows_on_the_pane_as_it_arrives() {
    let mut shell = shell();
    let mut widths = Vec::new();
    let mut frames = Vec::new();

    for delta in ["One", "\nTwo", "\nThree"] {
        shell.stream_delta(delta);
        let (rows, _) = painted(&shell, WIDTH, HEIGHT);
        let painted_text = rows.join("\n");
        widths.push(
            shell
                .streaming()
                .expect("something is being streamed")
                .len(),
        );
        frames.push(painted_text);
    }

    assert!(
        widths.windows(2).all(|pair| pair[1] > pair[0]),
        "the streamed answer did not grow on every delta: {widths:?}"
    );
    assert!(
        frames[2].contains("Three"),
        "the newest piece did not reach the buffer"
    );
    assert!(
        frames[2].contains("One"),
        "the earliest piece left the buffer, so the pane is replacing rather than accumulating"
    );
}

// The provisional line is cleared at the end of the turn and the authoritative
// answer is added like every other line, so the answer's bytes come from one
// place. The mutant keeps the provisional line, which paints the answer twice.
#[test]
fn the_streamed_line_is_cleared_so_the_answer_is_painted_once() {
    let mut shell = shell();
    shell.stream_delta("the answer");

    let (during, _) = painted(&shell, WIDTH, HEIGHT);
    assert_eq!(
        during.join("\n").matches("the answer").count(),
        1,
        "the answer appeared more than once while it was still arriving"
    );

    // The turn ends: the provisional line goes, the rendered line arrives.
    shell.clear_streaming();
    shell.notice(Line::new(Register::Plain, "the answer"));

    let (after, _) = painted(&shell, WIDTH, HEIGHT);
    assert_eq!(
        after.join("\n").matches("the answer").count(),
        1,
        "the answer is painted twice: the provisional line was kept as well as the rendered one"
    );
    assert_eq!(
        shell.streaming(),
        None,
        "the shell still believes something is streaming after the turn ended"
    );
}

// Clearing what was never started is not an error: a turn whose model asked
// for a tool and never spoke streams nothing, and the driver clears anyway.
#[test]
fn clearing_a_stream_that_never_started_changes_nothing() {
    let mut shell = shell();
    let (before, _) = painted(&shell, WIDTH, HEIGHT);
    shell.clear_streaming();
    let (after, _) = painted(&shell, WIDTH, HEIGHT);
    assert_eq!(before, after);
    assert_eq!(shell.streaming(), None);
}

// ------------------------------------- the pane's own text handling, 2026-09-06

/// The rows of the transcript pane, as the buffer holds them.
///
/// The status row is row 0 and the composer's area is the last
/// [`COMPOSER_ROWS`]; everything between is the pane. Trailing padding is
/// removed because `TestBackend` fills every cell, so a row's own trailing
/// spaces and the backend's padding are the same bytes — every fixture below
/// is written without trailing spaces for that reason.
fn pane_rows(shell: &Shell, width: u16, height: u16) -> Vec<String> {
    let (rows, _) = painted(shell, width, height);
    let last = rows.len() - usize::from(COMPOSER_ROWS);
    rows[1..last]
        .iter()
        .map(|row| row.trim_end().to_owned())
        .collect()
}

/// Every row `visible` hands the widget is byte-identical to the row the
/// buffer ends up holding.
///
/// # What this pins, and why it is not a tautology
///
/// `Line::rows` returned `Vec<String>` until 2026-09-14 and now returns
/// [`Row`], which carries the marker column and the text apart so the pane can
/// colour the first and leave the second alone. Nothing about what reaches the
/// buffer was meant to change, and "was meant to" is exactly the claim a
/// commit message cannot make good.
///
/// **One arm is the buffer**, read out of `TestBackend` cell by cell, and the
/// other is [`Row::joined`]. A comparison whose two sides both travelled
/// through `Row` would agree with itself for the life of any defect
/// ([Verification lessons] §11); the buffer is a reader that shares no code
/// with the split.
///
/// Three widths and three registers, because the split's two halves differ
/// between the first row of a line and its continuations, and a width that
/// never wraps would exercise only the first.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_rows_joined_form_is_what_the_pane_painted_before() {
    let long = "bare tier has no membrane and a prompt is a question rather than a barrier, \
                which is a sentence long enough to wrap at every width below";

    for width in [30_u16, 40, 100] {
        for register in [Register::Plain, Register::Call, Register::Setback] {
            let shell = shell_showing(register, long);
            let from_model: Vec<String> = shell
                .visible(24 - 1 - COMPOSER_ROWS, width)
                .iter()
                .map(Row::joined)
                .map(|row| row.trim_end().to_owned())
                .filter(|row| !row.is_empty())
                .collect();
            let from_buffer: Vec<String> = pane_rows(&shell, width, 24)
                .into_iter()
                .filter(|row| !row.trim().is_empty())
                .collect();

            assert!(
                !from_buffer.is_empty(),
                "nothing was painted at {width} columns for {register:?}, so this check \
                 asserted nothing"
            );
            assert_eq!(
                from_model, from_buffer,
                "at {width} columns the rows {register:?} hands the widget are not the rows \
                 the buffer holds"
            );
        }
    }
}

// ------------------------------------------- ADR-0028 D2's remaining word

/// No two registers share a colour, and only `Plain` has none.
///
/// The colour analogue of `no_two_registers_share_a_glyph`, and it holds the
/// same property that check's own argument rests on: a marker that two
/// registers share says nothing about which of them it is. `Plain` is the one
/// exception and it is asserted rather than excused — its colour is
/// [`Color::Reset`], which is the absence of a colour, exactly as its glyph is
/// the absence of a marker.
#[test]
fn no_two_registers_share_a_colour() {
    let mut seen: Vec<(Color, Register)> = Vec::new();
    for register in Register::ALL {
        let colour = register.colour();
        if register == Register::Plain {
            assert_eq!(
                colour,
                Color::Reset,
                "Plain carries a colour, so every line of ordinary narration is now tinted \
                 and the palette is a theme rather than a register distinction"
            );
            continue;
        }
        assert_ne!(
            colour,
            Color::Reset,
            "{register:?} has no colour of its own, so ADR-0028 D2's \"coloured\" does not \
             reach it"
        );
        if let Some((_, other)) = seen.iter().find(|(taken, _)| *taken == colour) {
            panic!("{register:?} and {other:?} are both painted {colour:?}");
        }
        seen.push((colour, register));
    }
    assert_eq!(seen.len(), Register::ALL.len() - 1);
}

/// Every register's colour is one of the terminal's sixteen, which is what the
/// ruling of 2026-09-14 recorded on ADR-0028 D2 asks for.
///
/// # The list is this check's own
///
/// Enumerated here rather than derived from `ratatui`'s enum, because a check
/// that asked the library which colours exist would accept whatever the
/// library grew next ([Verification lessons] §11 — one arm of a comparison
/// must not travel through the thing being checked). `Color::Rgb` and
/// `Color::Indexed` are the two the ruling refuses by name, and they are the
/// two a later author reaches for first when a hue looks slightly wrong.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_register_colour_is_one_of_the_sixteen_ansi_colours() {
    const SIXTEEN: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];

    for register in Register::ALL {
        let colour = register.colour();
        assert!(
            !matches!(colour, Color::Rgb(..) | Color::Indexed(_)),
            "{register:?} is painted {colour:?}, and the ruling of 2026-09-14 on ADR-0028 D2 \
             takes the terminal's sixteen and no truecolour or 256-colour index"
        );
        assert!(
            colour == Color::Reset || SIXTEEN.contains(&colour),
            "{register:?} is painted {colour:?}, which is neither one of the sixteen this \
             check names nor the absence of a colour"
        );
    }
}

/// The register's colour is on the marker column and the producer's words
/// carry none.
///
/// # Two arms, and the second is the one that matters
///
/// `Line`'s own documentation says the shell "chooses the glyph and nothing
/// else". So the first cell of a row — the register's glyph — carries the
/// register's colour, and the first cell of the *text* carries
/// [`Color::Reset`]. A check that asserted only the first would pass on a
/// renderer that tinted the whole row, which is the reading this arc
/// deliberately did not take.
///
/// Read out of `TestBackend`'s cells rather than off `Palette::marker`, so
/// neither arm travels through the function under test.
#[test]
fn the_pane_paints_each_register_glyph_in_its_own_colour() {
    for register in Register::ALL {
        let shell = shell_showing(register, "a line this check owns");
        let (rows, _) = cells(&shell, 40, 24, Palette::Coloured);
        let row = &rows[1];

        assert_eq!(
            row[0].0,
            register.glyph(),
            "the pane's first cell is not {register:?}'s glyph, so the row below is about \
             something else"
        );
        assert_eq!(
            row[0].1,
            register.colour(),
            "{register:?}'s glyph is painted {:?} rather than {:?}",
            row[0].1,
            register.colour()
        );
        assert_eq!(
            row[2].1,
            Color::Reset,
            "the first character of the text is painted {:?}, so the colour reached the \
             producer's words and not only the marker",
            row[2].1
        );
    }
}

/// Under `NO_COLOR` nothing carries a colour, and the frame is otherwise the
/// same frame.
///
/// # An absence assertion needs an instrument that could have found something
///
/// The first arm is the absence: every cell of a frame carrying all seven
/// registers is [`Color::Reset`]. On its own that arm is satisfied by a
/// renderer that paints nothing at all, by a staging that produced no rows,
/// and by a palette that was never consulted — so the second arm is the
/// **control**: the same shell under [`Palette::Coloured`] has at least one
/// cell that is not `Reset` ([Verification lessons] §8).
///
/// The third arm is what makes a capture taken under `NO_COLOR` comparable
/// with every capture taken before a colour existed: the two frames' symbols
/// are identical cell for cell, so the glyphs stay and only the colour goes.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn no_cell_carries_a_colour_under_no_color() {
    let mut shell = shell();
    for register in Register::ALL {
        shell.notice(Line::new(
            register,
            format!("{register:?} · {TRANSCRIPT_NONCE}"),
        ));
    }

    let (dark, _) = cells(&shell, 40, 24, Palette::Monochrome);
    let (lit, _) = cells(&shell, 40, 24, Palette::Coloured);

    assert!(
        lit.iter()
            .flatten()
            .any(|(_, colour)| *colour != Color::Reset),
        "the control frame carries no colour anywhere, so the absence asserted below is a \
         fact about a renderer that paints none rather than about NO_COLOR"
    );
    for (y, row) in dark.iter().enumerate() {
        for (x, (symbol, colour)) in row.iter().enumerate() {
            assert_eq!(
                *colour,
                Color::Reset,
                "under NO_COLOR the cell at ({x}, {y}), holding {symbol:?}, is painted \
                 {colour:?}"
            );
        }
    }
    let symbols = |frame: &Vec<Vec<(String, Color)>>| {
        frame
            .iter()
            .map(|row| row.iter().map(|(s, _)| s.clone()).collect::<String>())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        symbols(&dark),
        symbols(&lit),
        "NO_COLOR changed what the frame says and not only how it is coloured"
    );
}

/// No register's colour reaches the status line, the composer or the hint
/// strip.
///
/// # Why this asserts the absence of the seven and not of every colour
///
/// The ruling of 2026-09-14 puts the colour on the register glyphs only, and
/// what this check holds is that **this arc added none elsewhere**. It is
/// deliberately not phrased as "no colour at all": `tui-textarea` 0.7 carries
/// its own defaults — a placeholder foreground and a selection background —
/// which nothing here reaches today but which are that crate's values rather
/// than this workspace's, and a check phrased over every colour would one day
/// redden for a reason that has nothing to do with a register.
///
/// The pane arm is the control: at least one cell there does carry one, so a
/// renderer that painted no colour anywhere cannot satisfy this.
#[test]
fn no_register_colour_reaches_the_status_line_the_composer_or_the_hint_strip() {
    let mut shell = shell();
    shell
        .composer_mut()
        .set_standing(3, Some("a standing tip".to_owned()));
    shell.refresh(&StagedTranscript::three_outcomes());
    shell.notice(Line::new(Register::Setback, "iteration 1 failed: greets"));

    let height = 24_u16;
    let (frame, _) = cells(&shell, 100, height, Palette::Coloured);
    let pane_last = usize::from(height - COMPOSER_ROWS);
    let of_a_register = |colour: &Color| {
        *colour != Color::Reset && Register::ALL.iter().any(|r| r.colour() == *colour)
    };

    assert!(
        frame[1..pane_last]
            .iter()
            .flatten()
            .any(|(_, colour)| of_a_register(colour)),
        "no cell of the pane carries a register's colour, so the rows below assert nothing"
    );
    for (y, row) in frame.iter().enumerate() {
        if (1..pane_last).contains(&y) {
            continue;
        }
        for (x, (symbol, colour)) in row.iter().enumerate() {
            assert!(
                !of_a_register(colour),
                "the cell at ({x}, {y}), holding {symbol:?}, carries a register's colour \
                 {colour:?}, and row {y} is the status line or the composer's area"
            );
        }
    }
}

/// A shell whose pane holds exactly one line.
fn shell_showing(register: Register, text: &str) -> Shell {
    let mut shell = shell();
    shell.notice(Line::new(register, text));
    shell
}

/// The premise `Line::indent` rests on, asserted rather than assumed.
///
/// The continuation of a wrapped row is indented by the glyph's width plus
/// one. A register given a two-column glyph would still wrap correctly —
/// `indent` measures — but the mutant this catches is the reverse: somebody
/// replacing the measurement with a literal `2` after a wide glyph arrived.
#[test]
fn every_register_glyph_occupies_one_column() {
    for register in Register::ALL {
        assert_eq!(
            crate::shell::wrap::columns(register.glyph()),
            1,
            "{register:?}'s glyph {:?} is not one column wide, so a wrapped line's \
             continuation would not align under its first row",
            register.glyph()
        );
    }
}

/// An answer's own newlines are the answer's.
///
/// **The mutant**: `wrap::rows` stops splitting on `\n` and hands the whole
/// text back as one piece. Measured from the binary at `8179f8a` on
/// 2026-09-05, that is what shipped: asked to count from 1 to 30 one per
/// line, the pane painted
/// `123456789101112131415161718192021222324252627282930` on a single row,
/// while `context.json` held the newlines. Read out of the buffer rather than
/// out of `visible`, because the buffer is the consequence.
#[test]
fn a_thirty_line_answer_paints_thirty_rows() {
    let answer = (1..=30)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let shell = shell_showing(Register::Plain, &answer);

    let rows = pane_rows(&shell, 100, 40);
    let painted: Vec<&str> = rows
        .iter()
        .map(|row| row.trim())
        .filter(|row| !row.is_empty())
        .collect();

    assert_eq!(
        painted.len(),
        30,
        "a thirty-line answer painted {} row(s): {painted:#?}",
        painted.len()
    );
    assert_eq!(painted.first().copied(), Some("1"));
    assert_eq!(painted.last().copied(), Some("30"));
}

/// The accepting sibling: a record with no newline gains no row.
///
/// **The mutant**: `wrap::rows` appends a blank row per piece, which a
/// renderer could easily do while satisfying the thirty-line check above and
/// which would give a reader a pane of double-spaced narrative.
///
/// Two records rather than one, and asserted as **adjacent** buffer rows. A
/// single record cannot see this: the pane's unused rows are blank anyway, so
/// one spurious blank row after the only record is indistinguishable from the
/// empty pane beneath it. The gap between two records is where it shows.
#[test]
fn two_single_line_records_paint_on_adjacent_rows() {
    let mut shell = shell();
    shell.notice(Line::new(Register::Succeeded, "turn 1 answered"));
    shell.notice(Line::new(Register::Plain, "turn 2, up to 8 exchange(s)"));

    let rows = pane_rows(&shell, 100, 40);

    assert_eq!(rows[0], "✓ turn 1 answered");
    assert_eq!(
        rows[1],
        "  turn 2, up to 8 exchange(s)",
        "the second record is not on the row after the first; the pane was {:#?}",
        &rows[..4]
    );
}

/// The status row is one row, at every width, now that the pane wraps.
///
/// **The mutant**: `Shell::regions` gives the status `Constraint::Length(2)`.
/// That is the mutation this can actually see, and finding it out is worth
/// recording: giving the status paragraph the pane's `Wrap` **does not**
/// redden anything, because `regions` hands it a one-row `Rect` and the
/// overflow is clipped vertically rather than growing into the pane. So the
/// property [ADR-0001] D2 leans on is held by the **layout** rather than by
/// the absence of a `Wrap`, and this check is written against the layout.
///
/// The sibling is the existing
/// `the_tier_is_what_survives_a_width_too_narrow_for_the_whole_row`, which
/// asserts what the one row holds; this asserts that the pane still starts on
/// the row after it once transcript lines are allowed to occupy more than one.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[test]
fn the_status_row_is_one_row_at_every_width_while_the_pane_wraps() {
    let mut shell = shell();
    shell.set_context_usage(Some("context 786.4k/1048.5k tokens".to_owned()));
    shell.set_token_usage(Some("tokens: 390 prompt + 79 completion = 469".to_owned()));
    // Three characters, so the pane's own line cannot wrap even at ten
    // columns and the only thing that can push it down is the status row.
    shell.notice(Line::new(Register::Plain, "zzz"));

    for width in [10_u16, 20, 40, 72, 100] {
        let (rows, _) = painted(&shell, width, HEIGHT);
        assert_eq!(
            rows[1].trim_end(),
            "  zzz",
            "at width {width} the pane's first row is {:?} rather than the transcript's \
             only line, so the status row took more than one row",
            rows[1]
        );
    }
}

/// A line longer than the pane wraps, and no character is lost.
///
/// **The mutant**: `Line::rows` returns the text as one row, which is what
/// `ratatui` then clips at the right edge — the behaviour measured at
/// `8179f8a`, where `/config explain runtime.tier` inside a session printed
/// five layer rows whose values and whose `← effective` marker were past the
/// edge, so the command answered nothing.
///
/// The fixture carries no space, so it is also the hard-split case: a single
/// "word" wider than any row. It carries no trailing space either, which is
/// what makes stripping the backend's padding safe and lets this assert the
/// **exact** text rather than a proxy for it.
#[test]
fn a_line_wider_than_the_pane_wraps_and_loses_no_character() {
    let long: String = (0..300)
        .map(|n| char::from(b'a' + (n % 26) as u8))
        .collect();
    let shell = shell_showing(Register::Plain, &long);

    let rows: Vec<String> = pane_rows(&shell, 40, 24)
        .into_iter()
        .filter(|row| !row.trim().is_empty())
        .collect();

    assert!(
        rows.len() > 1,
        "a 300-character line at 40 columns painted {} row(s), so it did not wrap",
        rows.len()
    );
    for row in &rows {
        assert!(
            crate::shell::wrap::columns(row) <= 40,
            "a painted row is {} columns wide against a pane of 40: {row:?}",
            crate::shell::wrap::columns(row)
        );
    }

    let rejoined: String = rows.iter().map(|row| row[2..].to_owned()).collect();
    assert_eq!(
        rejoined, long,
        "the wrapped rows do not reproduce the line; the pane lost or reordered text"
    );
}

/// The accepting sibling: a line that fits is byte-identical to `painted`.
#[test]
fn a_line_that_fits_is_painted_exactly_as_it_always_was() {
    let line = Line::new(Register::Call, "fs.write ./note.txt");
    let mut shell = shell();
    shell.notice(line.clone());

    let rows: Vec<String> = pane_rows(&shell, 40, 24)
        .into_iter()
        .filter(|row| !row.trim().is_empty())
        .collect();

    assert_eq!(rows, vec![line.painted()]);
}

/// A wrap breaks between words and never inside one.
///
/// **The mutant**: the wrap splits at the budget regardless of where a word
/// ends, which reads as a hyphenless hyphenation and makes a path or an
/// identifier unsearchable by eye.
#[test]
fn a_wrapped_line_breaks_between_words_and_never_inside_one() {
    let text = "bare tier has no membrane and a prompt is a question rather than a barrier";
    let shell = shell_showing(Register::Plain, text);

    let rows = pane_rows(&shell, 30, 24);
    let painted = rows.join("\n");
    for word in text.split(' ') {
        assert!(
            rows.iter()
                .any(|row| row.split(' ').any(|shown| shown == word)),
            "the word {word:?} is on no painted row whole; the pane was:\n{painted}"
        );
    }
}

/// The tail is a tail of **rows**, not of records.
///
/// **The mutant**: `visible` slices `pane_lines` before the rows are built,
/// which is what it did until 2026-09-06. One forty-line record on a pane
/// with room for ten then hands the widget forty rows and the widget keeps
/// the first ten — showing the user the beginning of an answer instead of its
/// end, which is the exact inverse of [ADR-0010] D4's "the last stretch".
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn the_tail_of_one_record_longer_than_the_pane_is_its_newest_rows() {
    let answer = (1..=40)
        .map(|n| format!("{TRANSCRIPT_NONCE}-{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let shell = shell_showing(Register::Plain, &answer);

    // One status row and COMPOSER_ROWS at the foot leave ten for the pane.
    let rows = pane_rows(&shell, 100, 11 + COMPOSER_ROWS);
    let painted = rows.join("\n");

    assert!(
        painted.contains(&format!("{TRANSCRIPT_NONCE}-40")),
        "the last row of the record is not on the pane:\n{painted}"
    );
    assert!(
        !painted.contains(&format!("{TRANSCRIPT_NONCE}-1\n")) && !painted.ends_with("-1"),
        "the first row of the record is on the pane, so this is the head:\n{painted}"
    );
}

/// **Security corpus.** A held value that a wrap breaks in two still reaches
/// the buffer whole, across the two rows.
///
/// **The mutant**: the wrap consumes the space it breaks at, or normalises
/// what it carries. Either would cut a planted value into pieces, and the
/// absence assertions this suite makes elsewhere are written against the
/// value's own bytes — so a wrap that quietly elided a character would make
/// those assertions pass for a reason nobody intended, which is the inverse
/// of [Verification lessons] §65's warning about redacting after truncating.
///
/// The sibling is `a_held_secret_in_a_transcript_line_reaches_the_buffer_and_nothing_else`
/// above, which plants the same value in a line that fits. Both must pass:
/// the pane is a view of the record at every width, which is [ADR-0010] D2's
/// 2026-09-05 Update.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-3
#[test]
fn corpus_a_held_secret_split_across_a_wrap_still_reaches_the_buffer_whole() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript(vec![Line::new(
        Register::Call,
        format!("cmd.run `curl -H 'Authorization: Bearer {SECRET_NONCE}'`"),
    )]));

    // Narrow enough that the value itself cannot fit on one row, so the break
    // lands **inside** it. At thirty columns the word wrap keeps it whole,
    // which is right and is why the width here is fourteen: the property
    // being asserted is about a break through a held value, not about a
    // wrapped line that happens to carry one.
    let rows = pane_rows(&shell, 14, 24);
    assert!(
        rows.iter().filter(|row| !row.trim().is_empty()).count() > 1,
        "the line did not wrap at 14 columns, so this asserts nothing about a break: {rows:#?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains(SECRET_NONCE)),
        "the value is whole on one row, so the break did not land inside it and this \
         asserts nothing: {rows:#?}"
    );

    let rejoined: String = rows
        .iter()
        .filter(|row| !row.trim().is_empty())
        .map(|row| row[2..].to_owned())
        .collect();
    assert!(
        rejoined.contains(SECRET_NONCE),
        "the wrap cut the value into pieces the buffer no longer holds; rejoined: {rejoined:?}"
    );
}

/// ADR-0015's 2026-09-13 amendment: the queued task is painted on its own row
/// immediately above the composer, and it cannot be scrolled away.
///
/// # The mutant this is written against is the shape that was already there
///
/// Appending the row to `Shell::pane_lines` reads simpler and is the defect:
/// `pane_lines` is transcript, then notices, then the answer being streamed,
/// and `visible` shows the tail — so a growing answer pushes anything above it
/// off the screen. This stages exactly that, a pane of transcript with a
/// streamed answer long enough to fill it, and asserts the queued row survives.
///
/// Three clauses, asserted apart: the row is immediately above the composer,
/// the input row is byte-identical to the frame without a queue (ADR-0005 D2
/// met by a second producer), and the row is gone once the queue is drained.
#[test]
fn a_queued_task_paints_above_the_composer_and_survives_a_streaming_answer() {
    let mut shell = shell();
    shell.refresh(&StagedTranscript::three_outcomes());
    for n in 0..40 {
        shell.notice(Line::new(Register::Plain, format!("filler line {n}")));
    }
    shell.stream_delta(&"an answer that keeps arriving. ".repeat(20));

    let (before, cursor_before) = painted(&shell, WIDTH, HEIGHT);
    let input_row = usize::from(HEIGHT - COMPOSER_ROWS);

    shell.queue(crate::shell::Queued::of("réad src/main.rs"));
    let (after, cursor_after) = painted(&shell, WIDTH, HEIGHT);

    assert_eq!(
        after[input_row - 1].trim_end(),
        "  queued réad src/main.rs",
        "the row immediately above the composer is not the queued task; it reads {:?}",
        after[input_row - 1]
    );
    assert_eq!(
        after[input_row], before[input_row],
        "the input row moved because a task was queued, which ADR-0005 D2 forbids: {:?} then {:?}",
        before[input_row], after[input_row]
    );
    assert_eq!(
        cursor_after, cursor_before,
        "the cursor moved because a task was queued"
    );

    shell.take_queued().expect("a task was queued");
    let (drained, _) = painted(&shell, WIDTH, HEIGHT);
    assert_eq!(
        drained,
        before,
        "draining the queue did not give the row back: {:?} then {:?}",
        before[input_row - 1],
        drained[input_row - 1]
    );
}

/// Taking the queued task is what empties it, so nothing can run it twice.
///
/// Its accepting sibling is the first half: a task queued is a task the shell
/// hands over once.
#[test]
fn a_second_enter_replaces_the_queued_task_and_taking_it_empties_the_queue() {
    let mut shell = shell();
    shell.queue(crate::shell::Queued::of("the first thing"));
    shell.queue(crate::shell::Queued::of("the second thing"));
    assert_eq!(
        shell.queued().map(|task| task.task.as_str()),
        Some("the second thing"),
        "a second queued task did not replace the first; the shell holds {:?}",
        shell.queued()
    );

    assert_eq!(
        shell.take_queued().map(|task| task.task),
        Some("the second thing".to_owned())
    );
    assert_eq!(
        shell.take_queued(),
        None,
        "the queue handed the same task over twice, so a turn could run it twice"
    );
}

/// A queued task's own text cannot produce a row a reader reads as a transcript
/// line.
///
/// The queued text is painted verbatim so a person sees what will run, and the
/// text is whatever they typed or pasted — so a block beginning with a
/// register's glyph is reachable. What stops it being read as that register is
/// the prefix word before it and `Register::Plain`'s own glyph, both at column
/// zero.
///
/// **Its accepting sibling is the second half**: a real `Register::Failed`
/// line does begin with `✗ ` at column zero, so the check discriminates rather
/// than refusing everything.
#[test]
fn corpus_a_queued_task_cannot_paint_a_row_a_reader_reads_as_a_failure() {
    let forged = "✗ the program \"cargo\" could not be started";

    let mut queueing = shell();
    queueing.queue(crate::shell::Queued::of(forged));
    let (rows, _) = painted(&queueing, WIDTH, HEIGHT);
    let input_row = usize::from(HEIGHT - COMPOSER_ROWS);
    assert!(
        rows[input_row - 1].starts_with("  queued ✗ the program"),
        "the queued row does not mark itself as queued before the text; it reads {:?}",
        rows[input_row - 1]
    );
    assert!(
        !rows.iter().any(|row| row.starts_with("✗ ")),
        "a queued task produced a row a reader would read as a failure: {:?}",
        rows.iter().find(|row| row.starts_with("✗ "))
    );

    let mut real = shell();
    real.notice(Line::new(
        Register::Failed,
        "the program could not be started",
    ));
    let (genuine, _) = painted(&real, WIDTH, HEIGHT);
    assert!(
        genuine.iter().any(|row| row.starts_with("✗ ")),
        "a genuine failure does not begin with the failure glyph at column zero either, so the \
         check above discriminates nothing: {genuine:?}"
    );
}

/// A pasted block cannot forge the status row, however it is spelled.
///
/// # The neighbouring finding this is written against
///
/// The `live-status` arc found that adding the model made the row "the first
/// surface a repository the user cloned can write to", because `model.<alias>`
/// is free at every configuration layer — and that neutralising the separator
/// was not enough, because a forged **tier** claim is still a forged claim
/// even when it is not a forged field. A pasted block is bytes the person
/// chose, arriving at a surface that had no way to carry arbitrary text
/// before, so the same question has to be asked of it.
///
/// The answer is structural: [`Status`]'s fields have two setters that reach
/// two segments, the composer is not one of them, and the row is composed from
/// the fields alone. So the assertion is byte identity of the row across a
/// paste carrying both the separator and the tier's own prefix.
///
/// **Its accepting sibling is the second half**: `describe` does change the
/// row, so the check discriminates rather than asserting that the row never
/// changes at all.
#[test]
fn corpus_a_pasted_block_cannot_forge_a_field_on_the_status_row() {
    let forgery = format!(
        "{sep}{prefix}linked{sep}a second claim",
        sep = crate::shell::SEPARATOR,
        prefix = crate::shell::TIER_PREFIX,
    );

    let mut shell = shell();
    let before = shell.status().painted(WIDTH);
    shell.pasted(&forgery, Duration::ZERO, &TrieOf::new(0));
    let after = shell.status().painted(WIDTH);
    assert_eq!(
        before, after,
        "a pasted block changed the status row, so the composer reaches a field the records own: \
         {before:?} then {after:?}"
    );
    assert_eq!(
        after.matches(crate::shell::TIER_PREFIX).count(),
        1,
        "the row carries more than one membrane claim, which is the one thing ADR-0001 D2 exists \
         to make unambiguous: {after:?}"
    );
    let (rows, _) = painted(&shell, WIDTH, HEIGHT);
    assert_eq!(
        rows[0],
        {
            let mut expected = before.clone();
            expected
                .push_str(&" ".repeat(usize::from(WIDTH) - crate::shell::wrap::columns(&before)));
            expected
        },
        "the painted status row is not the row the shell composed: {:?}",
        rows[0]
    );

    // The sibling: a field a record does put there does move the row.
    shell.describe(Some("gemini-3.6-flash".to_owned()), Some("ask".to_owned()));
    assert_ne!(
        shell.status().painted(WIDTH),
        after,
        "the row did not change when a model and a mode were set, so the check above asserts \
         nothing"
    );
}

/// The one row windows the text and the submission does not.
///
/// Library verification lessons §65 read the other way: where a value is both
/// shortened and handed on, the shortening must not be what is handed on. Here
/// the *rendering* is a window onto a block that may be far wider than the
/// terminal, and what `Enter` submits is the whole of it.
///
/// **Its accepting sibling is the short case**, where the window and the text
/// coincide — so an implementation that submitted the row would pass that one
/// and fail this.
#[test]
fn corpus_a_block_wider_than_the_frame_is_submitted_whole() {
    let trie = TrieOf::new(0);
    let block = "abcdefghij".repeat(50);
    assert_eq!(block.chars().count(), 500);

    let mut shell = shell();
    shell.pasted(&block, Duration::ZERO, &trie);
    let (row, caret) = shell.composer().input_row(WIDTH);
    // One column short of the frame, because the caret sits at the right edge
    // and needs a cell — the same arithmetic `tui-textarea`'s own viewport
    // did. What matters here is only that the row is a window: 59 columns
    // against 500 characters.
    assert_eq!(
        crate::shell::wrap::columns(&row),
        usize::from(WIDTH) - 1,
        "the painted row is not a window onto the block, so this check is not comparing a window \
         with a whole: {row:?}"
    );
    assert_eq!(caret, WIDTH - 1, "the caret is not at the right edge");

    let submitted = match key(&mut shell, Key::Enter) {
        Action::Task(task) => task,
        other => panic!("a pasted block was not read as a task: {other:?}"),
    };
    assert_eq!(
        submitted,
        block,
        "what was submitted is not what was pasted: {} characters against {}",
        submitted.chars().count(),
        block.chars().count()
    );

    // The sibling: a block that fits is submitted whole too, so the assertion
    // above is not satisfied by an implementation that submits the row.
    let mut short = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));
    short.pasted("a short one", Duration::ZERO, &trie);
    let (row, _) = short.composer().input_row(WIDTH);
    assert_eq!(row.trim_end(), "a short one");
    assert_eq!(
        key(&mut short, Key::Enter),
        Action::Task("a short one".to_owned())
    );
}
