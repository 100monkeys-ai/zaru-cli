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

/// `matches` cut to the `rows` the strip paints, with [`continues`] last where
/// anything was left out.
///
/// # One pager, because two copies of this rule is what the defect was
///
/// The command picker landed on 2026-09-15 carrying this arithmetic inside
/// `Composer::strip`'s own `Intent::Command` arm, and the three corpora beside
/// it had none. So the trie handed [`Composer::strip_lines`] up to
/// [`MATCH_LIMIT`] rows — nine with [`KEYWORD_ONLY`] — the shell gave the
/// strip [`STRIP_ROWS`] of them, `Paragraph` carried no `Wrap`, and rows seven
/// and eight were painted into no cell with **nothing on the screen saying a
/// match had been dropped**. Neither constant was wrong and nobody had
/// reconciled them. The rule therefore lives here, once, and every arm goes
/// through it: inside one crate a rule lives in one place.
///
/// # Trailers are reserved, never paged
///
/// [`KEYWORD_ONLY`] is ADR-0005 D8's honest degradation and is not a match.
/// Appending it to the matches and paging the result would make it the row the
/// budget drops — the one row on the strip whose whole purpose is to say
/// something is wrong. So a trailer's rows come off the budget first and the
/// matches page into what is left.
///
/// # The overflow row is last
///
/// It is the row a reader acts on, and what it says is what to type. That is
/// also where the command picker already put it, and this function exists so
/// that surface's behaviour is unchanged while the others acquire it.
///
/// `paint` formats the matches that survived, and is handed them rather than
/// the whole set, because the command picker pads its column to the widest
/// **shown** spelling: padding to the widest in the narrowed set would indent
/// every row by the width of a spelling nobody can see.
///
/// [`MATCH_LIMIT`]: crate::composer::MATCH_LIMIT
/// [`STRIP_ROWS`]: crate::shell::STRIP_ROWS
fn fitted<T>(
    matches: Vec<T>,
    trailers: Vec<String>,
    rows: usize,
    paint: impl FnOnce(Vec<T>) -> Vec<String>,
) -> Vec<String> {
    if matches.len() + trailers.len() <= rows {
        let mut lines = paint(matches);
        lines.extend(trailers);
        return lines;
    }
    // The overflow row takes one row and the trailers keep theirs; the
    // matches get whatever is left. A budget too small to hold even the
    // trailers and the overflow row cannot arise from `STRIP_ROWS`, and is
    // answered by showing fewer of them rather than by a branch no caller
    // reaches and no check could redden.
    let kept = trailers.len().min(rows.saturating_sub(1));
    let shown = rows.saturating_sub(kept).saturating_sub(1);
    let beyond = matches.len() - shown;
    let mut lines = paint(matches.into_iter().take(shown).collect());
    lines.extend(trailers.into_iter().take(kept));
    lines.push(continues(beyond));
    lines
}

/// `lines`, or `absence` when there are none and a sentence was handed in.
///
/// # A blank strip is what this exists to stop
///
/// Before the fast tier had an implementation, a user typing into `zaru` saw
/// nothing below the input and was told nothing about why — the largest
/// missing piece of this surface, and the kind of silent degradation
/// [Operating Principles]' "legibility beats smoothness" is written against. A
/// user with no Nuclear Notes token has a reason to see nothing, and the
/// reason is worth one line. A user whose working directory offers nothing has
/// a different reason, and it is worth a different line.
///
/// It appends rather than replaces so that it cannot hide a match: it is
/// reached only when there is nothing else to show.
///
/// # It takes the sentence rather than reading a field, since 2026-09-15
///
/// It was a method on [`Composer`] reading `self.absence`, which was right
/// while one corpus could be absent. There are two — the cortex and the
/// working directory — they fail independently, and a session with a cortex
/// and an empty tree must be able to say the second thing without claiming the
/// first. Passing the sentence is what makes each arm name the corpus it is
/// about. See [`Composer::set_absence`] and [`Composer::set_path_absence`] for
/// why either line is handed in rather than composed here.
///
/// [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
fn or_absence(absence: Option<&String>, lines: Vec<String>) -> Vec<String> {
    match (lines.is_empty(), absence) {
        (true, Some(absence)) => vec![absence.clone()],
        _ => lines,
    }
}

impl Composer {
    /// The lines the strip is showing, top to bottom.
    ///
    /// Empty when the strip is collapsed, which is what makes a collapse
    /// reclaim its rows rather than paint blank ones.
    #[must_use]
    pub fn strip_lines(&self) -> Vec<String> {
        // The rendering budget, read once here and handed to every arm, so
        // that no corpus can be painted against a number of its own. See
        // [`fitted`].
        let rows = usize::from(crate::shell::STRIP_ROWS);
        let titles = |matches: Vec<crate::composer::Entry>| -> Vec<String> {
            matches.into_iter().map(|entry| entry.title).collect()
        };
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
            StripContent::Command {
                matches,
                extensions,
            } => {
                // **Across both corpora**, since 2026-09-15: a command's row
                // and a namespace's row sit in one list, so a column that
                // lined up only within each half would read as two tables.
                let pairs: Vec<(String, String)> = matches
                    .into_iter()
                    .map(|namespace| (namespace.slash.to_owned(), namespace.governs.to_owned()))
                    .chain(
                        extensions
                            .into_iter()
                            .map(|extension| (extension.slash, extension.governs)),
                    )
                    .collect();
                fitted(pairs, Vec::new(), rows, |shown| {
                    // The spellings are padded to the widest row shown, the
                    // way `--help` pads its own, so the descriptions line up.
                    // Padding to the widest in the *vocabulary* instead would
                    // indent every narrowed list by the width of `/providers`,
                    // which is a column of blanks a person has no use for —
                    // which is why the padding is computed here, on what
                    // survived the paging, and not on the set handed in.
                    let width = shown
                        .iter()
                        .map(|(slash, _)| slash.chars().count())
                        .max()
                        .unwrap_or(0);
                    shown
                        .into_iter()
                        .map(|(slash, governs)| format!("{slash:width$}  {governs}"))
                        .collect()
                })
            }
            // **A picker carries the absence line, since 2026-09-15.** It did
            // not until then, on the reasoning that "an open picker with no
            // matches is what a miss looks like, and the picker's own sigil is
            // already on the screen saying what is being picked" — and that
            // reasoning is about a **miss**, which this line is not. The
            // absence is `Some` only where the host says the corpus holds
            // nothing at all, and a `[[` over a session with no Nuclear Notes
            // token painted six blank rows and said nothing about why:
            // measured on the release binary at `3c1bf0a`, the same silence
            // the trie arm has had a line for since 2026-09-05. A miss is
            // still silent, because `or_absence` is reached only when there is
            // nothing else to show and the host handed a sentence in.
            //
            // It pages like every other corpus: an explicit picker over a
            // large cortex is exactly where eight matches meet six rows.
            StripContent::Picker { matches, .. } => or_absence(
                self.absence.as_ref(),
                fitted(matches, Vec::new(), rows, titles),
            ),
            // The third corpus, paged by the same one pager and carrying its
            // own absence line — the working directory and the cortex fail
            // independently, so the sentence is a different one.
            StripContent::Paths { matches, .. } => or_absence(
                self.path_absence.as_ref(),
                fitted(matches, Vec::new(), rows, |shown| {
                    shown
                        .into_iter()
                        .map(|entry| entry.spelling().to_owned())
                        .collect()
                }),
            ),
            StripContent::Trie { matches } => or_absence(
                self.absence.as_ref(),
                fitted(matches, Vec::new(), rows, titles),
            ),
            StripContent::Merged { entries, search } => {
                // `keyword only` is a **trailer**: it is D8's statement about
                // the ranking rather than a match, and paging it with the
                // matches would make the honest line the row the budget drops.
                let trailers = if search
                    == (SearchState::Returned {
                        semantic_available: false,
                    }) {
                    vec![KEYWORD_ONLY.to_owned()]
                } else {
                    Vec::new()
                };
                or_absence(
                    self.absence.as_ref(),
                    fitted(entries, trailers, rows, titles),
                )
            }
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
    /// a prompt may hold newlines now, the text area's widget has no way to
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

        // **The elision is here and never in `strip_lines`.** A row wider than
        // the frame was clipped by the widget, so the honest no-corpus
        // sentence lost its error code at a hundred columns and almost all of
        // itself at forty, where a pane row wraps. `wrap::elided` ends it with
        // the tree's own marker instead, measured in the columns the buffer
        // paints with; truncating at the paint site rather than in the
        // sentence keeps the whole of it available to anything that reads
        // `strip_lines`. It is not wrapped, for the row budget's sake --
        // `wrap::elided` carries the whole of why.
        let lines: Vec<Line<'_>> = self
            .strip_lines()
            .into_iter()
            .map(|line| Line::from(crate::shell::wrap::elided(&line, usize::from(strip.width))))
            .collect();
        if !lines.is_empty() {
            frame.render_widget(Paragraph::new(lines), strip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KEYWORD_ONLY;
    use crate::composer::fixtures::{
        CountingTrie, NoPaths, PathsOf, SERVER_NONCE, TRIE_NONCE, TrieOf, VocabularyOf, painted,
        press, server_results, typing, typing_paths, typing_with,
    };
    use crate::composer::search::SearchResponse;
    use crate::composer::{Composer, NEWLINE};
    use crate::shell::fixtures::StagedVocabulary;
    use core::time::Duration;
    use ratatui_textarea::Key;

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
            composer.paste(prompt, Duration::ZERO, &trie, &StagedVocabulary, &NoPaths);
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
        composer.paste(
            "óne\ntwo\nthree",
            Duration::ZERO,
            &trie,
            &StagedVocabulary,
            &NoPaths,
        );

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
        composer.paste(
            "a\n\u{23ce}b",
            Duration::ZERO,
            &trie,
            &StagedVocabulary,
            &NoPaths,
        );

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
        composer.paste(&block, Duration::ZERO, &trie, &StagedVocabulary, &NoPaths);

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

    /// The trie's matches page against the rows the strip paints, and the row
    /// that says so is [`continues`](super::continues) — the command picker's
    /// own line, reused.
    ///
    /// **This is the check that would have caught the defect.** At `c915001`
    /// a composer typed into against a trie of eight answered
    /// `strip_lines().len() == 8` while the shell gave the strip six rows and
    /// `Paragraph` carried no `Wrap`, so rows seven and eight were painted
    /// into no cell and nothing on the screen said a match had been dropped.
    /// The count is asserted against `STRIP_ROWS` rather than against a
    /// literal six, so a shell that changes its budget cannot leave this
    /// passing while the strip drops rows again.
    #[test]
    fn a_strip_with_more_matches_than_rows_paints_the_overflow_row() {
        let rows = usize::from(crate::shell::STRIP_ROWS);
        let trie = TrieOf::new(crate::composer::MATCH_LIMIT);
        let mut composer = Composer::new();
        typing(&mut composer, "tí", Duration::ZERO, &trie);

        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            rows,
            "the strip was handed {} lines for {rows} rows, so {} of them reach a person nowhere; \
             the lines were {lines:?}",
            lines.len(),
            lines.len().saturating_sub(rows)
        );
        for (n, line) in lines.iter().take(rows - 1).enumerate() {
            assert_eq!(
                line,
                &format!("títle-{n}·{TRIE_NONCE} ✦"),
                "row {n} is not the {n}th match; the lines were {lines:?}"
            );
        }
        assert_eq!(
            lines[rows - 1],
            super::continues(crate::composer::MATCH_LIMIT - (rows - 1)),
            "the last row does not say how many matches were left out, so the strip drops them \
             silently; the lines were {lines:?}"
        );
    }

    /// The accepting sibling: a strip whose matches fit carries no overflow
    /// row at all.
    ///
    /// Without this, `continues` pushed unconditionally — even at a count of
    /// zero — would pass the check above while putting `… 0 more · type to
    /// narrow` under every short list on the surface.
    #[test]
    fn a_strip_that_fits_paints_no_overflow_row() {
        let trie = TrieOf::new(4);
        let mut composer = Composer::new();
        typing(&mut composer, "tí", Duration::ZERO, &trie);

        let lines = composer.strip_lines();
        assert_eq!(lines.len(), 4, "four matches are four rows; got {lines:?}");
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("more · type to narrow")),
            "a list that fits was given an overflow row anyway; the lines were {lines:?}"
        );
    }

    /// ADR-0005 D8's `keyword only` is a **trailer**: its row comes off the
    /// budget before the matches page into what is left, so it can never be
    /// the row the budget drops.
    ///
    /// The failure this is written against is the tempting shape — append the
    /// line to the matches and page the result — under which the one row on
    /// the strip whose whole purpose is to say something is wrong is the first
    /// casualty of there being too much to show.
    #[test]
    fn the_merged_strip_reserves_the_keyword_only_row_before_it_pages() {
        let rows = usize::from(crate::shell::STRIP_ROWS);
        let trie = TrieOf::new(crate::composer::MATCH_LIMIT);
        let mut composer = Composer::new();
        typing(&mut composer, "títle", Duration::ZERO, &trie);
        composer.deliver(SearchResponse {
            results: Vec::new(),
            semantic_available: false,
        });

        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            rows,
            "the merged strip was handed {} lines for {rows} rows; they were {lines:?}",
            lines.len()
        );
        assert!(
            lines.iter().any(|line| line == KEYWORD_ONLY),
            "D8's degradation line was the row the budget dropped, which is the one row that \
             exists to say something is wrong; the lines were {lines:?}"
        );
        assert_eq!(
            lines[rows - 1],
            super::continues(crate::composer::MATCH_LIMIT - (rows - 2)),
            "the overflow row is not last, or it is not counting the matches the trailer left \
             room for; the lines were {lines:?}"
        );
    }

    /// An explicit picker pages against the same budget as everything else.
    ///
    /// D1 row 6's picker is the surface most likely to meet a large cortex —
    /// `[[` over every page and atom — and it had no paging at all.
    #[test]
    fn an_open_picker_pages_against_the_same_budget() {
        let rows = usize::from(crate::shell::STRIP_ROWS);
        let trie = TrieOf::new(crate::composer::MATCH_LIMIT);
        let mut composer = Composer::new();
        typing(&mut composer, "[[tí", Duration::ZERO, &trie);

        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            rows,
            "an open picker was handed {} lines for {rows} rows; they were {lines:?}",
            lines.len()
        );
        assert_eq!(
            lines[rows - 1],
            super::continues(crate::composer::MATCH_LIMIT - (rows - 1)),
            "the picker drops matches without saying so; the lines were {lines:?}"
        );
    }

    /// The command picker's rows are byte-identical across the move of its
    /// paging into [`fitted`](super::fitted).
    ///
    /// The regression guard on the refactor: this surface was already correct
    /// and the arc's whole claim is that the other three acquired its
    /// behaviour without it acquiring theirs. The expected rows are literals
    /// written here, taken from the frame the release binary painted at
    /// `c915001`.
    #[test]
    fn the_command_pickers_rows_are_byte_identical_across_the_move() {
        let rows = usize::from(crate::shell::STRIP_ROWS);
        let vocabulary = VocabularyOf::new(6).and_commands(4);
        let trie = TrieOf::new(0);
        let mut composer = Composer::new();
        typing_with(&mut composer, "/sé", Duration::ZERO, &trie, &vocabulary);

        let lines = composer.strip_lines();
        assert_eq!(
            lines,
            vec![
                "/séance  a staged namespace".to_owned(),
                "/sédan   a staged namespace".to_owned(),
                "/sédge   a staged namespace".to_owned(),
                "/sédum   a staged namespace".to_owned(),
                "/séism   a staged namespace".to_owned(),
                super::continues(10 - (rows - 1)),
            ],
            "the picker's rows changed when its paging moved — the column's padding is computed \
             on the rows shown, not on the set handed in"
        );
    }

    /// No match the strip holds reaches a person nowhere, read out of the
    /// painted buffer rather than out of `strip_lines`.
    ///
    /// The two are different subjects. `strip_lines` is what the composer
    /// says it will paint; this is what `ratatui` put in the cells of an area
    /// the size the shell gives it. The original defect lived exactly in the
    /// gap between them, so the frame is where it is asserted gone.
    #[test]
    fn no_match_the_strip_holds_reaches_a_person_nowhere() {
        let held = crate::composer::MATCH_LIMIT;
        let trie = TrieOf::new(held);
        let mut composer = Composer::new();
        typing(&mut composer, "tí", Duration::ZERO, &trie);

        // The shell's own geometry: one input row and `STRIP_ROWS` below it.
        let (rows, _) = painted(&composer, 60, crate::shell::COMPOSER_ROWS);
        let painted_text = rows.join("\n");

        let mut shown = 0;
        for n in 0..held {
            if painted_text.contains(&format!("títle-{n}·{TRIE_NONCE} ✦")) {
                shown += 1;
            }
        }
        assert!(
            painted_text.contains("more · type to narrow"),
            "{} of {held} matches are on the frame and no row says the rest exist; the frame was \
             {rows:?}",
            shown
        );
        assert_eq!(
            shown + (held - shown),
            held,
            "the arithmetic below is only meaningful if every match is either painted or counted"
        );
        assert!(
            painted_text.contains(&super::continues(held - shown)),
            "the overflow row's count is not the number of matches missing from the frame: {shown} \
             are painted out of {held}, so the row should read {:?}; the frame was {rows:?}",
            super::continues(held - shown)
        );
    }

    /// The retrieval budget exceeds the rendering budget, at run time as well
    /// as at compile time.
    ///
    /// The `const _` beside `MATCH_LIMIT` is the real gate and a change that
    /// breaks it does not compile. This states the same property where a
    /// reader of the checks will find it, and says why it matters: a retrieval
    /// budget equal to the row budget can be exhausted without the strip being
    /// able to say so.
    #[test]
    fn the_retrieval_budget_exceeds_the_row_budget() {
        assert!(
            crate::composer::MATCH_LIMIT > usize::from(crate::shell::STRIP_ROWS),
            "MATCH_LIMIT is {} and STRIP_ROWS is {}; with no gap between them a full strip cannot \
             say that anything was left out",
            crate::composer::MATCH_LIMIT,
            crate::shell::STRIP_ROWS
        );
    }

    /// The register's own sentence, at the three widths it was measured at.
    ///
    /// `notes unreachable · the server refused pages.list: You are not a
    /// member of that workspace. (code -32002)` is one hundred and four
    /// columns. At 150 it is whole and its error code is readable; at 100 and
    /// at 40 it is cut, and after this change the cut says so. **The accepting
    /// sibling is inside the check**, because the three widths this arc was
    /// given bracket the defect rather than all exhibiting it — a check that
    /// only asserted the marker would pass against a composer that elided
    /// every row.
    #[test]
    fn a_row_wider_than_the_frame_ends_with_the_elision_glyph() {
        const SENTENCE: &str = "notes unreachable · the server refused pages.list: You are not a \
                                member of that workspace. (code -32002)";
        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(SENTENCE.to_owned()));
        typing(&mut composer, "édit", Duration::ZERO, &empty);

        for width in [40_u16, 100] {
            let (rows, _) = painted(&composer, width, crate::shell::COMPOSER_ROWS);
            let row = &rows[1];
            assert_eq!(
                crate::shell::wrap::columns(row.trim_end()),
                usize::from(width),
                "at {width} columns the strip's row is not filling the frame; it was {row:?}"
            );
            assert!(
                row.trim_end().ends_with('…'),
                "at {width} columns the row was cut with nothing saying so, which is what a \
                 person reading it cannot see; it was {row:?}"
            );
        }

        let (rows, _) = painted(&composer, 150, crate::shell::COMPOSER_ROWS);
        assert_eq!(
            rows[1].trim_end(),
            SENTENCE,
            "at 150 columns the sentence fits and must be painted byte for byte, error code and \
             all; it was {:?}",
            rows[1]
        );
    }

    /// The sentence the row carries is not truncated — only the painted row
    /// is.
    ///
    /// The elision is a property of the frame and never of the text, so
    /// `strip_lines` answers the whole sentence at every width. That is what
    /// keeps the truncation out of anything that later reads the strip's rows,
    /// and it is the half a check on the buffer alone cannot see.
    #[test]
    fn the_sentence_the_row_carries_is_not_truncated() {
        const SENTENCE: &str = "notes unreachable · the server refused pages.list: You are not a \
                                member of that workspace. (code -32002)";
        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(SENTENCE.to_owned()));
        typing(&mut composer, "édit", Duration::ZERO, &empty);

        for width in [40_u16, 100, 150] {
            let (_, _) = painted(&composer, width, crate::shell::COMPOSER_ROWS);
            assert_eq!(
                composer.strip_lines(),
                vec![SENTENCE.to_owned()],
                "painting at {width} columns changed what the strip says it holds; the elision \
                 belongs to the frame and not to the sentence"
            );
        }
    }

    /// An elided row never cuts a wide character in half, and never overflows
    /// the frame by the column such a cut would cost.
    ///
    /// The tempting implementation counts `char`s. A row of CJK at an odd
    /// budget is where that and the buffer's own `unicode-width` measurement
    /// disagree, and disagreeing by one column on the last cell is exactly the
    /// bug the elision exists to remove.
    #[test]
    fn an_elided_row_never_cuts_a_wide_character_in_half() {
        const WIDE: &str = "広い行広い行広い行広い行広い行広い行広い行広い行";
        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(WIDE.to_owned()));
        typing(&mut composer, "édit", Duration::ZERO, &empty);

        for width in [11_u16, 21, 31] {
            // The buffer is read cell by cell and a wide character occupies
            // two, so the reconstructed string is not the row's own text and
            // is not measured as if it were. What the frame can say is that
            // the marker reached the last cell that was written; that the
            // *text* never exceeds the budget is `elided`'s own property and
            // is asserted on the function, in `shell::tests`, at the odd
            // budgets where a character count and a column count disagree.
            let (rows, _) = painted(&composer, width, crate::shell::COMPOSER_ROWS);
            let row = rows[1].trim_end();
            assert!(
                row.ends_with('…'),
                "at {width} columns a row of wide characters was cut with nothing saying so; it \
                 was {row:?}"
            );
            let text = crate::shell::wrap::elided(WIDE, usize::from(width));
            assert!(
                crate::shell::wrap::columns(&text) <= usize::from(width),
                "at {width} columns the row this frame was painted from measures {} and would \
                 run past the frame; it was {text:?}",
                crate::shell::wrap::columns(&text)
            );
        }
    }

    /// The accepting sibling: a row that fits is painted byte for byte.
    ///
    /// Without it, appending the marker unconditionally would satisfy every
    /// check above while putting an ellipsis on the end of every short row on
    /// the surface.
    #[test]
    fn a_row_that_fits_is_painted_byte_for_byte() {
        const SHORT: &str = "nothing cached · ✦";
        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(SHORT.to_owned()));
        typing(&mut composer, "édit", Duration::ZERO, &empty);

        let (rows, _) = painted(&composer, 40, crate::shell::COMPOSER_ROWS);
        assert_eq!(
            rows[1].trim_end(),
            SHORT,
            "a row that fits was altered on the way to the frame"
        );
        assert!(
            !rows[1].contains('…'),
            "a row that fits was given an elision marker; it was {:?}",
            rows[1]
        );
    }

    /// ADR-0005 clause 5's fifth producer: the input row and its caret are
    /// byte-identical whether the strip's rows are elided, whole, or carrying
    /// the overflow row.
    ///
    /// The first four are the strip's zero, one and six entries, the pasted
    /// three-line block, the picker's rows, and the pane's window. This is the
    /// same clause over the two states this arc adds, at the width where an
    /// elision happens and the width where it does not.
    #[test]
    fn the_input_row_is_byte_identical_whether_the_strip_is_elided_or_not() {
        const LONG: &str = "notes unreachable · the server refused pages.list: You are not a \
                            member of that workspace. (code -32002)";
        const TYPED: &str = "édit";

        for width in [40_u16, 150] {
            let empty = TrieOf::new(0);
            let mut whole = Composer::new();
            whole.set_absence(Some("short · ✦".to_owned()));
            typing(&mut whole, TYPED, Duration::ZERO, &empty);
            let (whole_rows, whole_cursor) = painted(&whole, width, crate::shell::COMPOSER_ROWS);

            let mut cut = Composer::new();
            cut.set_absence(Some(LONG.to_owned()));
            typing(&mut cut, TYPED, Duration::ZERO, &empty);
            let (cut_rows, cut_cursor) = painted(&cut, width, crate::shell::COMPOSER_ROWS);

            let full = TrieOf::new(crate::composer::MATCH_LIMIT);
            let mut paged = Composer::new();
            typing(&mut paged, TYPED, Duration::ZERO, &full);
            let (paged_rows, paged_cursor) = painted(&paged, width, crate::shell::COMPOSER_ROWS);

            assert_eq!(
                (whole_rows[0].as_str(), whole_cursor),
                (cut_rows[0].as_str(), cut_cursor),
                "at {width} columns an elided strip row moved the input row or its caret"
            );
            assert_eq!(
                (whole_rows[0].as_str(), whole_cursor),
                (paged_rows[0].as_str(), paged_cursor),
                "at {width} columns a strip carrying the overflow row moved the input row or its \
                 caret"
            );
        }
    }

    /// ADR-0005 clause 5's **sixth** producer: the input row and its caret are
    /// byte-identical whatever the path corpus shows.
    ///
    /// The first five are the strip's zero, one and six entries, the pasted
    /// three-line block, the command picker's rows, the pane's window, and the
    /// elided and overflowing strip. This is the same clause over the third
    /// corpus, at the width an elision happens and the width it does not, and
    /// across a corpus of nothing, of one row and of a full page — the only
    /// variable that moves is how many rows the strip holds.
    #[test]
    fn the_input_row_is_byte_identical_whatever_the_path_corpus_shows() {
        const TYPED: &str = "@sé";

        for width in [40_u16, 150] {
            let trie = TrieOf::new(0);

            let mut bare = Composer::new();
            typing_paths(
                &mut bare,
                TYPED,
                Duration::ZERO,
                &trie,
                &PathsOf::new([] as [&str; 0]),
            );
            let (bare_rows, bare_cursor) = painted(&bare, width, crate::shell::COMPOSER_ROWS);

            let one = PathsOf::new(["séance.txt"]);
            let mut single = Composer::new();
            typing_paths(&mut single, TYPED, Duration::ZERO, &trie, &one);
            let (single_rows, single_cursor) = painted(&single, width, crate::shell::COMPOSER_ROWS);

            let many = PathsOf::new(
                (0..crate::composer::MATCH_LIMIT)
                    .map(|i| format!("séance/dossier-{i}·✦.md"))
                    .collect::<Vec<_>>(),
            );
            let mut paged = Composer::new();
            typing_paths(&mut paged, TYPED, Duration::ZERO, &trie, &many);
            let (paged_rows, paged_cursor) = painted(&paged, width, crate::shell::COMPOSER_ROWS);

            assert_eq!(
                (bare_rows[0].as_str(), bare_cursor),
                (single_rows[0].as_str(), single_cursor),
                "at {width} columns one path row moved the input row or its caret"
            );
            assert_eq!(
                (bare_rows[0].as_str(), bare_cursor),
                (paged_rows[0].as_str(), paged_cursor),
                "at {width} columns a full page of path rows moved the input row or its caret"
            );
        }
    }

    /// The path corpus pages through the one pager, overflow row and all.
    ///
    /// The fixture answers more spellings than the strip has rows, so an arm
    /// that paged on its own — or did not page at all, which is what every
    /// corpus but the command picker did before 2026-09-15 — would paint rows
    /// into no cell and say nothing about it.
    #[test]
    fn the_path_corpus_pages_through_the_one_pager() {
        let trie = TrieOf::new(0);
        let many = PathsOf::new(
            (0..crate::composer::MATCH_LIMIT)
                .map(|i| format!("séance/dossier-{i}.md"))
                .collect::<Vec<_>>(),
        );
        let mut composer = Composer::new();
        typing_paths(&mut composer, "@sé", Duration::ZERO, &trie, &many);

        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            usize::from(crate::shell::STRIP_ROWS),
            "the strip paints the rows it has and no more: {lines:?}"
        );
        assert_eq!(
            lines[lines.len() - 1],
            super::continues(
                crate::composer::MATCH_LIMIT - (usize::from(crate::shell::STRIP_ROWS) - 1)
            ),
            "the last row says how many were left out and what to do: {lines:?}"
        );
        assert!(
            lines[0].starts_with("séance/dossier-0"),
            "the rows are the corpus's spellings, in its own order: {lines:?}"
        );
    }

    /// A path row wider than the frame is elided rather than clipped, because
    /// the elision is at the paint site and every arm reaches it.
    #[test]
    fn a_path_row_wider_than_the_frame_is_elided() {
        let trie = TrieOf::new(0);
        let long = PathsOf::new(["séance/dossier/très-long-nom-de-fichier-évident.md"]);
        let mut composer = Composer::new();
        typing_paths(&mut composer, "@sé", Duration::ZERO, &trie, &long);

        let (narrow, _) = painted(&composer, 20, crate::shell::COMPOSER_ROWS);
        assert!(
            narrow[1].trim_end().ends_with('…'),
            "a path row wider than the frame was clipped rather than elided; it was {:?}",
            narrow[1]
        );
        assert_eq!(
            crate::shell::wrap::columns(narrow[1].trim_end()),
            20,
            "the elided path row does not fill the frame; it was {:?}",
            narrow[1]
        );
    }

    /// The path corpus says so when the working directory has nothing to
    /// offer, on a painted frame rather than only in the model.
    ///
    /// The accepting sibling is the same corpus with a spelling in it: without
    /// it, an implementation that painted the absence line unconditionally
    /// would pass the first half.
    #[test]
    fn the_path_corpus_says_so_when_there_is_nothing_to_offer() {
        const NOTHING: &str = "nothing here to offer · ✦";
        let trie = TrieOf::new(0);

        let mut bare = Composer::new();
        bare.set_path_absence(Some(NOTHING.to_owned()));
        typing_paths(
            &mut bare,
            "@sé",
            Duration::ZERO,
            &trie,
            &PathsOf::new([] as [&str; 0]),
        );
        let (rows, _) = painted(&bare, 40, crate::shell::COMPOSER_ROWS);
        assert_eq!(
            rows[1].trim_end(),
            NOTHING,
            "a working directory with nothing to offer says so on the frame; the row was {:?}",
            rows[1]
        );

        let mut offering = Composer::new();
        offering.set_path_absence(Some(NOTHING.to_owned()));
        typing_paths(
            &mut offering,
            "@sé",
            Duration::ZERO,
            &trie,
            &PathsOf::new(["séance.txt"]),
        );
        let (rows, _) = painted(&offering, 40, crate::shell::COMPOSER_ROWS);
        assert_eq!(
            rows[1].trim_end(),
            "séance.txt",
            "a corpus with something in it paints the something; the row was {:?}",
            rows[1]
        );
    }

    /// The command picker's own rows are elided too, which is the half a fix
    /// inside any one content arm would have missed.
    ///
    /// Measured at `c915001` from the release binary at 40 columns: the strip
    /// painted `/stack    AEGIS component fetch and stat` — the picker's own
    /// row, clipped at the frame with nothing saying so. The elision sits at
    /// the paint site, so every arm gets it in one place.
    #[test]
    fn the_command_pickers_own_rows_are_elided_at_forty_columns() {
        let vocabulary = VocabularyOf::new(2);
        let trie = TrieOf::new(0);
        let mut composer = Composer::new();
        typing_with(&mut composer, "/sé", Duration::ZERO, &trie, &vocabulary);

        let (narrow, _) = painted(&composer, 20, crate::shell::COMPOSER_ROWS);
        assert!(
            narrow[1].trim_end().ends_with('…'),
            "a picker row wider than the frame was clipped rather than elided; it was {:?}",
            narrow[1]
        );
        assert_eq!(
            crate::shell::wrap::columns(narrow[1].trim_end()),
            20,
            "the elided picker row does not fill the frame; it was {:?}",
            narrow[1]
        );

        let (wide, _) = painted(&composer, 60, crate::shell::COMPOSER_ROWS);
        assert_eq!(
            wide[1].trim_end(),
            "/séance  a staged namespace",
            "a picker row that fits was altered; it was {:?}",
            wide[1]
        );
    }
}
