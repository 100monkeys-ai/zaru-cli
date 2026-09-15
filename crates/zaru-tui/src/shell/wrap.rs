// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! How one transcript line becomes the rows a pane of a given width paints.
//!
//! # Why the pane wraps rather than letting `ratatui` do it
//!
//! `Paragraph` has a `Wrap` and it works, but the pane also has to know **how
//! many rows** a line became, because it shows the tail: [`Shell::visible`]
//! takes the last `height` rows and a widget that wraps at paint time has
//! already been given its area by then. `Paragraph::line_count` answers
//! exactly that question and is gated behind `ratatui`'s
//! `unstable-rendered-line-info` feature, which the workspace manifest does
//! not take — see the `[workspace.dependencies]` comment on how a dependency,
//! and a feature of one, arrives.
//!
//! So the rows are produced here, the tail is taken over rows, and the
//! `Paragraph` is handed lines that already fit and is given no `Wrap` at
//! all. That also keeps the two continuation columns out of the wrapping: a
//! widget re-wrapping a row this module already indented would measure the
//! indent as content.
//!
//! **No new dependency.** Display width is measured through
//! [`ratatui::text::Span::width`], which is public and is the same
//! `unicode-width` measurement the buffer itself uses when it paints. A
//! second measurement — `str::len`, or counting `char`s — would disagree with
//! the buffer about a wide character and wrap to the wrong column.
//!
//! # Nothing is dropped, including a space
//!
//! Every byte of the text reaches exactly one row, in order. A row is broken
//! **after** the spaces that end a word rather than by consuming them, so
//! concatenating the rows reproduces the text — which is what lets a check
//! assert the loss-free property directly instead of asserting a proxy for
//! it. Trailing spaces on a row are invisible on a terminal, so the fidelity
//! costs nothing a reader can see.
//!
//! That property is load-bearing beyond tidiness. [ADR-0010] D2 requires the
//! pane to show what the transcript file holds, unaltered, and [ADR-0011] D4
//! puts its out-of-tree marking at the **end** of a rendered call line, after
//! a resolved absolute path — so a wrap that dropped or elided anything at a
//! break would be able to drop exactly that marking.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [`Shell::visible`]: crate::shell::Shell::visible

use ratatui::text::Span;

/// The display width of `text`, measured as the buffer measures it.
#[must_use]
pub fn columns(text: &str) -> usize {
    Span::raw(text).width()
}

/// The display width of one character, without allocating for it.
fn char_columns(character: char) -> usize {
    let mut buffer = [0_u8; 4];
    columns(character.encode_utf8(&mut buffer))
}

/// What a row that did not fit ends with. U+2026, HORIZONTAL ELLIPSIS.
///
/// **Not a new authored glyph and nothing to batch.** It is already this
/// tree's elision marker in two places a person reads —
/// `composer::continues`' `… N more · type to narrow` and `shell::below`'s
/// `… N more below · End` — so naming it here records a convention rather
/// than inventing one. One column wide: its East Asian Width is Neutral, and
/// [`elided`] measures it through [`columns`] rather than assuming, so a font
/// substitution cannot silently make a row one column too wide.
const ELISION: char = '\u{2026}';

/// `text`, or as much of it as fits in `budget` display columns with the
/// elision marker as the last one.
///
/// # Why a strip row is truncated where a pane row wraps
///
/// The pane wraps, by [`rows`] above, because a pane row that was clipped
/// "silently loses whatever the producer put last" — the `pane-text` arc's
/// own words on 2026-09-06. The strip was never given the same treatment, so
/// the honest no-corpus sentence `notes unreachable · the server refused
/// pages.list: You are not a member of that workspace. (code -32002)` is one
/// hundred and four columns and lost its error code at a hundred, measured
/// from the release binary over a pseudo-terminal by `pane-navigation` and
/// again here.
///
/// **It is truncated and not wrapped, and the reason is the row budget.** The
/// strip paints [`STRIP_ROWS`] rows and `composer::render::fitted` pages every
/// corpus against that number; a row allowed to become two would eat the row a
/// later match was going to use, and the overflow row's count would then have
/// to be of *rows* rather than of matches — a number that changes with the
/// terminal's width. The reason it is **not** is worth recording because it
/// was believed: wrapping here would not move the input row. `Shell::render`
/// gives the composer `Constraint::Length(COMPOSER_ROWS)` at every terminal
/// size and `Composer::render` anchors the input to the top of it, so the
/// input row is a function of the area alone; `Composer::height` exists and no
/// shell code calls it.
///
/// # Only the painted row is cut
///
/// This is applied in `Composer::render`, at the paint site, and never in
/// `Composer::strip_lines`. So the sentence the row carries survives whole for
/// anything that reads it — which is what keeps a truncation a property of the
/// frame rather than of the text, the same separation `rows` above keeps for
/// the pane.
///
/// A `budget` of zero returns nothing, because there is no column to put the
/// marker in. A budget of one returns the marker alone.
///
/// [`STRIP_ROWS`]: crate::shell::STRIP_ROWS
/// [`COMPOSER_ROWS`]: crate::shell::COMPOSER_ROWS
#[must_use]
pub fn elided(text: &str, budget: usize) -> String {
    if columns(text) <= budget {
        return text.to_owned();
    }
    if budget == 0 {
        return String::new();
    }
    // One column is the marker's. Characters are taken through `columns`, the
    // measurement the buffer itself paints with, so a wide character that
    // would straddle the last kept column is left out rather than cut in half
    // -- which would put the row one column past the frame.
    let room = budget - 1;
    let mut kept = String::new();
    let mut width = 0_usize;
    for character in text.chars() {
        let character_width = char_columns(character);
        if width + character_width > room {
            break;
        }
        kept.push(character);
        width += character_width;
    }
    kept.push(ELISION);
    kept
}

/// Break `text` into rows no wider than `budget` display columns.
///
/// `text`'s own newlines are honoured first — a newline in an answer is the
/// answer's — and each piece between them is then word-wrapped. A word wider
/// than the whole budget is split at the last character that fits rather than
/// being dropped or allowed to overflow, because a budget nothing can satisfy
/// still has to render the characters.
///
/// A `budget` of zero would make every row empty and the loop non-terminating,
/// so it is raised to one. That is a terminal one or two columns wide, where
/// nothing is readable and the only property worth keeping is that the
/// function returns.
#[must_use]
pub fn rows(text: &str, budget: usize) -> Vec<String> {
    let budget = budget.max(1);
    let mut rows = Vec::new();
    for piece in text.split('\n') {
        wrap_piece(piece, budget, &mut rows);
    }
    rows
}

/// Word-wrap one newline-free piece, appending its rows.
///
/// An empty piece contributes an empty row rather than nothing, so a blank
/// line inside an answer is a blank line on the pane. `split('\n')` on text
/// with no newline yields one piece, so a single-line record still arrives
/// here and leaves as exactly one row.
fn wrap_piece(piece: &str, budget: usize, rows: &mut Vec<String>) {
    if columns(piece) <= budget {
        rows.push(piece.to_owned());
        return;
    }

    let mut current = String::new();
    let mut width = 0_usize;

    for unit in units(piece) {
        // A run of spaces is placed wherever it falls and never starts a row
        // of its own. Spaces at the end of a row are invisible, and moving
        // them to the next row would indent a continuation by however much
        // padding the producer happened to use -- which is exactly what the
        // layer rows of `config explain` are made of.
        if unit.starts_with(' ') {
            current.push_str(unit);
            width += columns(unit);
            continue;
        }

        let unit_width = columns(unit);
        if width > 0 && width + unit_width > budget {
            rows.push(core::mem::take(&mut current));
            width = 0;
        }

        if unit_width <= budget {
            current.push_str(unit);
            width += unit_width;
            continue;
        }

        // Wider than any row can be. Fill the current row to the budget and
        // keep going; the remainder becomes whole rows of its own.
        for character in unit.chars() {
            let character_width = char_columns(character);
            if width + character_width > budget && width > 0 {
                rows.push(core::mem::take(&mut current));
                width = 0;
            }
            current.push(character);
            width += character_width;
        }
    }

    rows.push(current);
}

/// `piece` as alternating runs of spaces and runs of non-spaces.
///
/// Breaking between these is what makes the wrap a *word* wrap. Only the
/// space character separates units: a tab or any other whitespace stays
/// inside a word, because its display width on a terminal is not something
/// this module can know and guessing one would put the break in the wrong
/// column.
fn units(piece: &str) -> impl Iterator<Item = &str> {
    let mut rest = piece;
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let spaces = rest.starts_with(' ');
        let end = rest
            .find(|character: char| (character == ' ') != spaces)
            .unwrap_or(rest.len());
        let (unit, remainder) = rest.split_at(end);
        rest = remainder;
        Some(unit)
    })
}
