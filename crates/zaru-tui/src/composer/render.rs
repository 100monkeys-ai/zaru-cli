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
            // ADR-0015 D2's row: a command line is not a search, so the strip
            // says nothing and reclaims its rows exactly as a collapse does.
            StripContent::Command => Vec::new(),
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
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let [input, strip] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);

        frame.render_widget(self.input(), input);

        let (row, column) = self.cursor();
        frame.set_cursor_position(Position::new(
            input
                .x
                .saturating_add(u16::try_from(column).unwrap_or(u16::MAX)),
            input
                .y
                .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
        ));

        let lines: Vec<Line<'_>> = self.strip_lines().into_iter().map(Line::from).collect();
        if !lines.is_empty() {
            frame.render_widget(Paragraph::new(lines), strip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KEYWORD_ONLY;
    use crate::composer::Composer;
    use crate::composer::fixtures::{
        CountingTrie, SERVER_NONCE, TRIE_NONCE, TrieOf, painted, server_results, typing,
    };
    use crate::composer::search::SearchResponse;
    use core::time::Duration;

    const WIDTH: u16 = 40;
    const HEIGHT: u16 = 10;
    const BLANK: &str = "                                        ";

    /// ADR-0005 D2. The input's row and the cursor must not move because a
    /// search result arrived.
    ///
    /// Three strips of different heights over identical input text. The trie
    /// size is what varies, so nothing about the text or the cursor changes
    /// between the three — the only declared variable is the strip.
    #[test]
    fn the_input_row_is_byte_identical_whatever_the_strip_shows() {
        let mut painted_rows = Vec::new();
        let mut cursors = Vec::new();
        for entries in [0_usize, 1, 6] {
            let trie = TrieOf::new(entries);
            let mut composer = Composer::new();
            typing(&mut composer, "édit", Duration::ZERO, &trie);
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
            "édit",
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
}
