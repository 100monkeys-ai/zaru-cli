// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Painting the composer, and the one rule that constrains it more than it
//! reads.
//!
//! ADR-0005 D2: "The strip renders below the input and its height changes
//! never reflow the text the user is composing. The cursor does not move
//! because a search result arrived." The record's own Status tracking calls
//! this the clause that constrains the implementation more than it looks, and
//! names the failure it prevents as the most irritating one available in a
//! terminal.
//!
//! It is held here by anchoring the input to the top of the composer's area
//! and letting the strip occupy whatever is below it. The input's row is then
//! a function of the area alone, and no strip height can move it. Under the
//! coordinator's ruling of 2026-09-04 a collapsed strip **reclaims** its rows
//! rather than reserving them — [`Composer::height`] reports one row when the
//! strip is collapsed — which is D1 row 3's "collapses to zero height" and
//! D2's "reserving space rather than growing into it" reconciled: nothing is
//! reserved because nothing above it can move.

use crate::composer::search::SearchState;
use crate::composer::{Composer, StripContent};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

/// What D8 puts on the strip when semantic ranking is unavailable.
///
/// "`search.global` returns `semanticAvailable`; when false the strip says
/// `keyword only` rather than silently serving worse results." The words are
/// the record's and are spelled here exactly once.
pub const KEYWORD_ONLY: &str = "keyword only";

/// The row a command picker gives to the namespaces its rows cannot hold.
///
/// **The one authored string in the command picker, drafted under a delegated
/// coordinator ruling of 2026-09-14 23:48:04Z and 2026-09-15 00:12:17Z and
/// open to Jeshua's veto**, in the same shape as the six register glyphs,
/// `STRIP_ROWS` and [`crate::composer::NEWLINE`]: no record supplies a line
/// and one is needed, so it is named once here with its reasoning rather than
/// typed at a call site. It is recorded on [ADR-0005's amendments page] with
/// the two alternatives that were rejected.
///
/// # Why there is a line at all
///
/// [ADR-0015] D2 names twelve namespaces and the strip paints six rows, so a
/// bare `/` cannot show them all. The tempting option is to paint the first
/// six and say nothing, and **the strip already does exactly that** on the
/// other corpus — the trie's budget is eight matches against six painted rows,
/// with no wrapping, so two of eight reach a person nowhere and nothing tells
/// them. Reproducing that in a new surface is the silent degradation
/// [Operating Principles]' "legibility beats smoothness" is written against.
/// The other option was a scrolling list with a selection cursor, refused
/// because it gives `Up` and `Down` a second meaning inside the composer.
///
/// # Why it says what to do rather than only how many
///
/// [ADR-0016] D2: a message whose reader cannot act "is a stack trace with
/// better grammar". Typing is what narrows the list, so that is what the line
/// says. It is twenty-five columns at the counts this vocabulary can produce,
/// which fits the forty-column frame the absence lines are short for.
///
/// [ADR-0005's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
#[must_use]
pub fn continues(beyond: usize) -> String {
    format!("… {beyond} more · type to narrow")
}

impl Composer {
    /// The lines the strip is showing, top to bottom.
    ///
    /// Empty when the strip is collapsed, which is what makes a collapse
    /// reclaim its rows rather than paint blank ones.
    #[must_use]
    pub fn strip_lines(&self) -> Vec<String> {
        match self.strip() {
            StripContent::Collapsed => Vec::new(),
            StripContent::Deposits { count } => {
                vec![format!("{count} pending · /inbox")]
            }
            StripContent::Tip { text } => vec![text],
            // ADR-0015 D2's row: a command line is not a search, so what the
            // strip shows is the command namespaces rather than the hint
            // tiers' matches — and it carries no absence line, because the
            // absence line is about a corpus this row is not showing.
            StripContent::Command { matches, beyond } => {
                // The spellings are padded to the widest row shown, the way
                // `--help` pads its own, so the descriptions line up. Padding
                // to the widest in the *vocabulary* instead would indent every
                // narrowed list by the width of `/providers`, which is a
                // column of blanks a person has no use for.
                let width = matches
                    .iter()
                    .map(|namespace| namespace.slash.chars().count())
                    .max()
                    .unwrap_or(0);
                let mut lines: Vec<String> = matches
                    .into_iter()
                    .map(|namespace| {
                        let slash = namespace.slash;
                        format!("{slash:width$}  {}", namespace.governs)
                    })
                    .collect();
                if beyond > 0 {
                    lines.push(continues(beyond));
                }
                lines
            }
            // A picker never carries the absence line. An open picker with no
            // matches is what a miss looks like, and the picker's own sigil is
            // already on the screen saying what is being picked.
            StripContent::Picker { matches, .. } => {
                matches.into_iter().map(|entry| entry.title).collect()
            }
            StripContent::Trie { matches } => {
                self.or_absence(matches.into_iter().map(|entry| entry.title).collect())
            }
            StripContent::Merged { entries, search } => {
                let mut lines: Vec<String> = entries.into_iter().map(|entry| entry.title).collect();
                if search
                    == (SearchState::Returned {
                        semantic_available: false,
                    })
                {
                    lines.push(KEYWORD_ONLY.to_owned());
                }
                self.or_absence(lines)
            }
        }
    }

    /// `lines`, or the absence line when there are none and one was handed in.
    ///
    /// # A blank strip is what this exists to stop
    ///
    /// Before the fast tier had an implementation, a user typing into `zaru`
    /// saw nothing below the input and was told nothing about why — the largest
    /// missing piece of this surface, and the kind of silent degradation
    /// [Operating Principles]' "legibility beats smoothness" is written
    /// against. A user with no Nuclear Notes token has a reason to see nothing,
    /// and the reason is worth one line.
    ///
    /// It appends rather than replaces so that it cannot hide a match: it is
    /// reached only when there is nothing else to show. See
    /// [`Composer::set_absence`] for why the line is handed in.
    ///
    /// [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
    fn or_absence(&self, lines: Vec<String>) -> Vec<String> {
        match (lines.is_empty(), &self.absence) {
            (true, Some(absence)) => vec![absence.clone()],
            _ => lines,
        }
    }

    /// How many rows the composer needs at this moment: the input, plus the
    /// strip.
    ///
    /// One when the strip is collapsed. A host lays out around this, which is
    /// what "reclaims its rows" means concretely.
    #[must_use]
    pub fn height(&self) -> u16 {
        let strip = u16::try_from(self.strip_lines().len()).unwrap_or(u16::MAX);
        strip.saturating_add(1)
    }

    /// Paint the composer into `area`.
    ///
    /// The input is anchored to the top and occupies exactly one row, so its
    /// position is a function of `area` and nothing else. D2.
    ///
    /// # The row is composed rather than handed to the text area's widget
    ///
    /// Since 2026-09-13, and [`Composer::input_row`] carries the whole of why:
    /// a prompt may hold newlines now, the `tui-textarea` widget has no way to
    /// paint one as a glyph, and with several lines in it that widget paints
    /// the cursor's line alone with nothing saying the rest exist.
    ///
    /// **The caret's row is now always the input row.** It was `input.y` plus
    /// the cursor's *row* until then, which was harmless while the prompt
    /// could only ever hold one line and would have put the caret on the first
    /// strip row the moment one held two. [`Composer::input_row`] folds the
    /// row into a column, so there is nothing left to add.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let [input, strip] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

        let (row, column) = self.input_row(input.width);
        frame.render_widget(Paragraph::new(Line::from(row)), input);
        frame.set_cursor_position(Position::new(input.x.saturating_add(column), input.y));

        let lines: Vec<Line<'_>> = self.strip_lines().into_iter().map(Line::from).collect();
        if !lines.is_empty() {
            frame.render_widget(Paragraph::new(lines), strip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KEYWORD_ONLY;
    use crate::composer::fixtures::{
        CountingTrie, SERVER_NONCE, TRIE_NONCE, TrieOf, VocabularyOf, painted, press,
        server_results, typing, typing_with,
    };
    use crate::composer::search::SearchResponse;
    use crate::composer::{Composer, NEWLINE};
    use crate::shell::fixtures::StagedVocabulary;
    use core::time::Duration;
    use tui_textarea::Key;

    const WIDTH: u16 = 40;
    const HEIGHT: u16 = 10;
    const BLANK: &str = "                                        ";

    /// ADR-0005 D2. The input's row and the cursor must not move because a
    /// search result arrived.
    ///
    /// Three strips of different heights over identical input text. The trie
    /// size is what varies, so nothing about the text or the cursor changes
    /// between the three — the only declared variable is the strip.
    ///
    /// **A pasted block is the second producer**, added 2026-09-13: a prompt
    /// that holds newlines is still one row, so D2's clause holds over it
    /// exactly as it holds over a typed line, and a composer that grew a row
    /// for the block would move the strip and redden here.
    #[test]
    fn the_input_row_is_byte_identical_whatever_the_strip_shows() {
        for prompt in ["édit", "édit\nagain\nand again"] {
            input_row_is_fixed_whatever_the_strip_shows(prompt);
        }
    }

    /// The body of the check above, run once per prompt shape.
    fn input_row_is_fixed_whatever_the_strip_shows(prompt: &str) {
        let mut painted_rows = Vec::new();
        let mut cursors = Vec::new();
        for entries in [0_usize, 1, 6] {
            let trie = TrieOf::new(entries);
            let mut composer = Composer::new();
            composer.paste(prompt, Duration::ZERO, &trie, &StagedVocabulary);
            let (rows, cursor) = painted(&composer, WIDTH, HEIGHT);
            assert_eq!(
                rows.len() - 1,
                HEIGHT as usize - 1,
                "the frame should be {HEIGHT} rows tall"
            );
            painted_rows.push(rows[0].clone());
            cursors.push(cursor);
        }

        assert_eq!(
            painted_rows[0], painted_rows[1],
            "the input row moved between a strip of nothing and a strip of one line: {:?} then \
             {:?} — read out of the test backend's buffer, not out of the composer",
            painted_rows[0], painted_rows[1]
        );
        assert_eq!(
            painted_rows[0], painted_rows[2],
            "the input row moved between a strip of nothing and a strip of six lines: {:?} then \
             {:?}",
            painted_rows[0], painted_rows[2]
        );
        assert_eq!(
            cursors[0], cursors[1],
            "the cursor moved because a result arrived, which D2 forbids"
        );
        assert_eq!(
            cursors[0], cursors[2],
            "the cursor moved because six results arrived, which D2 forbids"
        );
        assert_eq!(
            painted_rows[0].trim_end(),
            prompt.replace('\n', NEWLINE),
            "the input row should hold what was composed"
        );
    }

    /// ADR-0005 clause 5, over the command picker: the input's row and cursor
    /// are byte-identical across pickers of zero, one and six rows.
    ///
    /// **A third producer for that clause, beside the pasted block and the
    /// queued task's row, rather than the clause moving.** On a command line
    /// the strip is a function of the text, so what varies here is the
    /// *vocabulary* — the six staged namespaces all answer to the same prefix,
    /// so the text and the cursor stay fixed while the picker is zero, one and
    /// six rows tall.
    #[test]
    fn the_input_row_is_byte_identical_whatever_the_picker_shows() {
        let trie = TrieOf::new(0);
        let mut painted_rows = Vec::new();
        let mut cursors = Vec::new();
        for namespaces in [0_usize, 1, 6] {
            let mut composer = Composer::new();
            typing_with(
                &mut composer,
                "/sé",
                Duration::ZERO,
                &trie,
                &VocabularyOf::new(namespaces),
            );
            let (rows, cursor) = painted(&composer, WIDTH, HEIGHT);
            assert_eq!(
                composer.strip_lines().len(),
                namespaces,
                "the staging is wrong: a vocabulary of {namespaces} should paint {namespaces} \
                 picker rows, and it painted {:?}",
                composer.strip_lines()
            );
            painted_rows.push(rows[0].clone());
            cursors.push(cursor);
        }

        assert_eq!(
            painted_rows[0], painted_rows[1],
            "the input row moved between a picker of nothing and a picker of one row: {:?} then \
             {:?}",
            painted_rows[0], painted_rows[1]
        );
        assert_eq!(
            painted_rows[0], painted_rows[2],
            "the input row moved between a picker of nothing and a picker of six rows: {:?} then \
             {:?}",
            painted_rows[0], painted_rows[2]
        );
        assert_eq!(
            cursors[0], cursors[1],
            "the cursor moved because one picker row appeared, which D2 forbids"
        );
        assert_eq!(
            cursors[0], cursors[2],
            "the cursor moved because six picker rows appeared, which D2 forbids"
        );
        assert_eq!(
            painted_rows[0].trim_end(),
            "/sé",
            "the input row should hold what was typed"
        );
    }

    /// ADR-0005 D1 row 3: "Empty, neither — nothing — the strip collapses to
    /// zero height."
    ///
    /// Two clauses about two different subjects, asserted apart. The painted
    /// rows are a fact about a viewport; the height is a fact about the
    /// mechanism, and it is what "reclaims its rows" means to a host laying
    /// out around the composer.
    #[test]
    fn an_empty_prompt_with_neither_deposits_nor_a_tip_paints_no_strip_rows() {
        let composer = Composer::new();
        let (rows, _) = painted(&composer, WIDTH, HEIGHT);

        for (n, row) in rows.iter().enumerate() {
            assert_eq!(
                row, BLANK,
                "row {n} of the painted frame is not blank, and an empty prompt with no deposits \
                 and no tip has nothing to say"
            );
        }

        assert_eq!(
            composer.height(),
            1,
            "a collapsed strip reclaims its rows, so the composer needs the input row and no more"
        );
    }

    /// ADR-0005 D1: "Deposits outrank tips." Asserted in the viewport as well
    /// as in the model, because a strip that carried both in its content and
    /// painted only one would satisfy the model check alone.
    #[test]
    fn a_deposit_count_outranks_a_standing_tip_in_the_same_frame() {
        let mut composer = Composer::new();
        composer.set_standing(3, Some(format!("try /learned — {SERVER_NONCE}")));
        let (rows, _) = painted(&composer, WIDTH, HEIGHT);
        let painted_text = rows.join("\n");

        assert!(
            painted_text.contains('3') && painted_text.contains("/inbox"),
            "the deposit count and where to open it should both be on the strip; the frame was \
             {rows:?}"
        );
        assert!(
            !painted_text.contains(SERVER_NONCE),
            "the tip was painted beside the deposits; user intent wins and the tip waits. The \
             frame was {rows:?}"
        );
    }

    /// ADR-0005 D1: "Typing dismisses a tip instantly — no fade, no delay. The
    /// first keystroke switches the strip to search."
    #[test]
    fn the_first_keystroke_replaces_a_tip_with_trie_matches_in_the_same_frame() {
        let trie = TrieOf::new(2);
        let mut composer = Composer::new();
        composer.set_standing(0, Some(format!("try /learned — {SERVER_NONCE}")));

        let (before, _) = painted(&composer, WIDTH, HEIGHT);
        assert!(
            before.join("\n").contains(SERVER_NONCE),
            "with an empty prompt the tip is what the strip has to show; the frame was {before:?}"
        );

        typing(&mut composer, "é", Duration::ZERO, &trie);
        let (after, _) = painted(&composer, WIDTH, HEIGHT);
        let after_text = after.join("\n");

        assert!(
            !after_text.contains(SERVER_NONCE),
            "one keystroke did not dismiss the tip, and D1 gives it no fade and no delay; the \
             frame was {after:?}"
        );
        assert!(
            after_text.contains(TRIE_NONCE),
            "one keystroke did not put trie matches on the strip; the frame was {after:?}"
        );
    }

    /// The absence line is painted where `ab000df` painted nothing, and the
    /// input row does not move because it appeared.
    ///
    /// Two subjects asserted apart. The first is that a user sees the sentence
    /// at all, read out of the buffer. The second is ADR-0005 D2 applied to the
    /// new row: a strip that grew from zero rows to one must not move the input,
    /// and that is compared against the same composer with no line handed in.
    #[test]
    fn the_absence_line_is_painted_and_the_input_row_does_not_move_for_it() {
        // The check owns its literal, and deliberately a short one: what is
        // asserted here is that the composer paints the line it was handed,
        // whatever it says. The words a user actually reads are `zaru-cli`'s
        // and are asserted at the shell's own width, because this frame is 40
        // columns and a real sentence about Nuclear Notes does not fit in it —
        // which is itself worth knowing and is why the wording is short.
        const ABSENCE: &str = "nothing to search · ✦";
        let empty = TrieOf::new(0);

        let mut silent = Composer::new();
        typing(&mut silent, "édit", Duration::ZERO, &empty);
        let (before, before_cursor) = painted(&silent, WIDTH, HEIGHT);
        assert!(
            before[1].trim().is_empty(),
            "with no line handed in the strip's first row is blank, which is what this arc found              and what the row below replaces; it was {:?}",
            before[1]
        );

        let mut speaking = Composer::new();
        speaking.set_absence(Some(ABSENCE.to_owned()));
        typing(&mut speaking, "édit", Duration::ZERO, &empty);
        let (after, after_cursor) = painted(&speaking, WIDTH, HEIGHT);

        assert_eq!(
            after[1].trim_end(),
            ABSENCE,
            "the absence line should be the strip's first row, read out of the test backend's              buffer; the frame was {after:?}"
        );
        assert_eq!(
            before[0], after[0],
            "the input row moved because the absence line appeared, which is D2's reflow: {:?}              then {:?}",
            before[0], after[0]
        );
        assert_eq!(
            before_cursor, after_cursor,
            "and so did the cursor, which D2 forbids"
        );
    }

    /// ADR-0005 D8: "`search.global` returns `semanticAvailable`; when false
    /// the strip says `keyword only` rather than silently serving worse
    /// results."
    ///
    /// Both sides are asserted. A check that only looked at the unavailable
    /// case could not tell the words being rendered from the words being
    /// rendered always.
    #[test]
    fn semantic_ranking_unavailable_renders_the_words_keyword_only() {
        for semantic_available in [true, false] {
            let trie = CountingTrie::staged();
            let mut composer = Composer::new();
            typing(&mut composer, "édi", Duration::ZERO, &trie);
            composer
                .step(Duration::from_millis(250))
                .expect("the request is due");
            composer.deliver(SearchResponse {
                results: server_results(),
                semantic_available,
            });

            let (rows, _) = painted(&composer, WIDTH, HEIGHT);
            let painted_text = rows.join("\n");
            assert_eq!(
                painted_text.contains(KEYWORD_ONLY),
                !semantic_available,
                "with semantic_available = {semantic_available} the strip should {} say \
                 {KEYWORD_ONLY:?}; the frame was {rows:?}",
                if semantic_available { "not" } else { "" }
            );
        }
    }
    /// The premise every measurement of the composed row rests on.
    ///
    /// `Line::indent` asserts the same thing about the six register glyphs and
    /// for the same reason: a two-column marker would put the caret a column
    /// out for every newline before it, and the row would be measured against
    /// a budget it does not occupy. A wider glyph reddens here rather than
    /// skewing every frame.
    #[test]
    fn the_newline_marker_occupies_one_column() {
        assert_eq!(
            crate::shell::wrap::columns(NEWLINE),
            1,
            "the newline marker {NEWLINE:?} occupies {} columns, and the one-row composer is \
             measured as though it occupied one",
            crate::shell::wrap::columns(NEWLINE)
        );
    }

    /// ADR-0005 D1 and D2, 2026-09-13: a pasted block stays in the one row and
    /// each of its newlines paints as one marker glyph.
    ///
    /// Both arms are literals written here rather than values the composer
    /// produced, so neither side of the comparison travels through the thing
    /// under test.
    #[test]
    fn a_pasted_block_paints_its_newlines_as_one_marker_each_in_one_row() {
        let trie = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.paste("óne\ntwo\nthree", Duration::ZERO, &trie, &StagedVocabulary);

        let (rows, cursor) = painted(&composer, WIDTH, HEIGHT);
        assert_eq!(
            rows[0].trim_end(),
            "óne\u{23ce}two\u{23ce}three",
            "a three-line paste should paint in one row with two markers; the row reads {:?}",
            rows[0]
        );
        assert_eq!(
            rows[1], BLANK,
            "the paste reached a second row, which ADR-0005 D2's one-row input forbids: {:?}",
            rows[1]
        );
        assert_eq!(
            cursor.y, 0,
            "the caret left the input row for row {}, which is the strip's",
            cursor.y
        );
        assert_eq!(
            cursor.x, 13,
            "the caret should sit past `óne⏎two⏎three`, which is 13 columns; it is at {}",
            cursor.x
        );
        assert_eq!(
            composer.text(),
            "óne\ntwo\nthree",
            "the composer stored the marker rather than the newline; it holds {:?}",
            composer.text()
        );
    }

    /// The marker is a rendering and never a storage form.
    ///
    /// A block carrying **both** a real newline and a literal U+23CE is the
    /// only shape that can tell the two apart: an implementation that stored
    /// the marker would make the two indistinguishable, and one that read the
    /// marker back as a newline would submit text the person never pasted.
    #[test]
    fn a_pasted_marker_glyph_survives_as_itself_beside_a_pasted_newline() {
        let trie = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.paste("a\n\u{23ce}b", Duration::ZERO, &trie, &StagedVocabulary);

        assert_eq!(
            composer.text(),
            "a\n\u{23ce}b",
            "the round trip altered the pasted bytes; the composer holds {:?} where the paste was \
             {:?}",
            composer.text(),
            "a\n\u{23ce}b"
        );
        let (rows, _) = painted(&composer, WIDTH, HEIGHT);
        assert_eq!(
            rows[0].trim_end(),
            "a\u{23ce}\u{23ce}b",
            "the newline and the pasted marker should paint as two markers side by side; the row \
             reads {:?}",
            rows[0]
        );
    }

    /// A block wider than the frame paints its visible tail, and the caret
    /// stays on the input row wherever it is.
    ///
    /// Two readings, deliberately: with the caret at the end the window is the
    /// block's tail, and with the caret moved back past the left edge the
    /// window follows it. A window anchored at zero passes the first and fails
    /// the second.
    #[test]
    fn a_block_wider_than_the_frame_paints_the_window_the_caret_is_in() {
        let trie = TrieOf::new(0);
        let mut composer = Composer::new();
        let block: String = (0..6)
            .map(|n| format!("líne-{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        composer.paste(&block, Duration::ZERO, &trie, &StagedVocabulary);

        // Six pieces of six columns each and five markers: 41 columns, one
        // past a 40-column frame, so two columns go to leave room for the caret.
        let (rows, cursor) = painted(&composer, WIDTH, HEIGHT);
        assert_eq!(
            rows[0].trim_end(),
            "ne-0\u{23ce}líne-1\u{23ce}líne-2\u{23ce}líne-3\u{23ce}líne-4\u{23ce}líne-5",
            "the tail of the block is not what the row paints: {:?}",
            rows[0]
        );
        assert_eq!(
            (cursor.x, cursor.y),
            (39, 0),
            "the caret should sit at the right edge of the input row; it is at {cursor:?}"
        );

        // Twelve presses of Left leave the caret twelve columns back, still
        // inside the window -- so the window must not move.
        for _ in 0..12 {
            press(&mut composer, Key::Left, &trie);
        }
        let (moved, caret) = painted(&composer, WIDTH, HEIGHT);
        assert_eq!(
            (caret.x, caret.y),
            (27, 0),
            "the caret left the window when it moved back; it is at {caret:?}"
        );
        assert_eq!(
            moved, rows,
            "the window moved although the caret was still inside it: {:?} then {:?}",
            rows[0], moved[0]
        );
    }

    /// The single-line frames this arc inherited are byte-identical, which is
    /// what pins the horizontal windowing the composer took over from
    /// `tui-textarea`.
    ///
    /// Four widths and two caret positions each — at the end of the text, and
    /// back inside it — with both arms literals written here. A one-column
    /// shift in either direction reddens.
    #[test]
    fn a_single_line_prompt_paints_where_it_always_painted() {
        let trie = TrieOf::new(0);
        // Twelve columns of text, so 40 and 20 hold it whole and 10 and 6 do
        // not.
        let text = "édit-a-líne";
        for (width, at_end, moved_back) in [
            (40_u16, ("édit-a-líne", 11_u16), ("édit-a-líne", 6_u16)),
            (20, ("édit-a-líne", 11), ("édit-a-líne", 6)),
            (10, ("it-a-líne", 9), ("it-a-líne", 4)),
            (6, ("-líne", 5), ("-líne", 0)),
        ] {
            let mut composer = Composer::new();
            typing(&mut composer, text, Duration::ZERO, &trie);
            let (rows, cursor) = painted(&composer, width, HEIGHT);
            assert_eq!(
                (rows[0].trim_end(), cursor.x),
                at_end,
                "at width {width} with the caret at the end the input row and caret read {:?} and \
                 {}",
                rows[0],
                cursor.x
            );

            for _ in 0..5 {
                press(&mut composer, Key::Left, &trie);
            }
            let (rows, cursor) = painted(&composer, width, HEIGHT);
            assert_eq!(
                (rows[0].trim_end(), cursor.x),
                moved_back,
                "at width {width} with the caret five back the input row and caret read {:?} and \
                 {}",
                rows[0],
                cursor.x
            );
        }
    }
}
