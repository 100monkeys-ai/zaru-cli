// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the shell paints.
//!
//! # Three regions, and only the middle one changes size
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │ runtime.tier = bare · gemini-3.6-flash · ask · 1.2k/1048.5k · 4.2s │  status, 1 row
//! ├──────────────────────────────────────────────────────────────────┤
//! │ ✓ finished in 4.2s                                               │  transcript pane
//! │ ⊘ stopped at the ceiling after 3                                  │
//! ├──────────────────────────────────────────────────────────────────┤
//! │ > what should I do                                               │  composer, fixed
//! │   a page the trie matched                                        │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The status line is at the top and the composer's area is a fixed height at
//! the foot, so the input row sits at a position that is a function of the
//! terminal's size and nothing else. That is [ADR-0005] D2 in a shell — see
//! [`crate::shell`]'s own documentation for why the alternative fails.
//!
//! **The status row is one line and is not wrapped.** It is **composed** to
//! the width instead, which is the 2026-09-06 amendment to [ADR-0001] D2 and
//! is what replaced the sentence that stood here: that a row wider than the
//! terminal was clipped at the right edge by `ratatui`, and that D2's "at all
//! times" was therefore a consequence of the tier being *first* rather than of
//! an elision rule nothing states. **Nothing states it no longer**: a clip
//! protects the first field and silently drops every other clause's, so the
//! order in which fields go is now declared as [`crate::shell::Rank`] and read
//! by [`crate::shell::Status::painted`], which takes this region's width. The
//! clip survives in exactly one case — a terminal too narrow for the tier's
//! own spelling — and that is the case D2's clause-6 check already covers.
//! **The transcript pane is the opposite and wraps**, which is not an
//! inconsistency: the status row is one row by construction and its ordering
//! is what protects the clause on it, while a transcript line clipped at the
//! right edge silently loses whatever the producer put last — measured on
//! 2026-09-05 to include the values and the `← effective` marker of
//! `config explain`, and [ADR-0011] D4's out-of-tree marking. See
//! [`crate::shell::wrap`].
//!
//! The pane shows the **tail**, which is [ADR-0010] D4's "re-renders the last
//! stretch of transcript so the user can see where they were" — a tail of
//! painted rows rather than of records, so a record longer than the pane
//! shows its newest rows.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::shell::{COMPOSER_ROWS, Palette, Row, Shell};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line as TextLine;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

/// What a prominent prompt is prefixed with, per [ADR-0011] D6.
///
/// D6: "Surfacing beats forbidding." The marking raises the prompt without
/// changing what it can do, which is that clause as a rendering rather than as
/// a veto.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const PROMINENT: &str = "!";

impl Shell {
    /// The three regions, top to bottom.
    ///
    /// Public so a check can assert where the input row is without inferring
    /// it from a painted frame, and so a host can size a terminal against it.
    #[must_use]
    pub fn regions(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(COMPOSER_ROWS),
        ])
        .areas(area)
    }

    /// The rows the pane paints in a `height` by `width` area, oldest first.
    ///
    /// The **tail**: a pane shorter than the transcript shows the end of it,
    /// because that is where the user was.
    ///
    /// # The tail is a tail of rows, not of records
    ///
    /// It counted records until 2026-09-06, which was the same number only
    /// while every record was one row. A thirty-line answer is thirty rows,
    /// and a tail taken over records would have handed `ratatui` thirty rows
    /// for a pane with room for ten and let the widget keep the first ten —
    /// showing a user the beginning of the answer they had just watched
    /// arrive. Counting rows keeps [ADR-0010] D4's "the last stretch"
    /// true **inside** one record as well as across several.
    ///
    /// `width` is taken because a row count is a function of it. That is the
    /// signature change wrapping forces, and it is the honest one: how much
    /// of the transcript fits genuinely depends on how wide the terminal is.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn visible(&self, height: u16, width: u16) -> Vec<Row> {
        let rows: Vec<Row> = self
            .pane_lines()
            .iter()
            .flat_map(|line| line.rows(width))
            .collect();
        let start = rows.len().saturating_sub(usize::from(height));
        rows[start..].to_vec()
    }

    /// What the composer's area shows: the prompt, or a standing question.
    ///
    /// The answers line is the one `zaru-cli`'s plain prompt writes, handed
    /// across rather than spelled again — see [`Confirmation::answers`].
    ///
    /// [`Confirmation::answers`]: crate::shell::Confirmation::answers
    #[must_use]
    pub fn prompt_lines(&self) -> Vec<String> {
        match self.asking() {
            None => Vec::new(),
            Some(question) => {
                let statement = if question.prominent {
                    format!("{PROMINENT} {}", question.statement)
                } else {
                    question.statement.clone()
                };
                vec![statement, question.answers.clone()]
            }
        }
    }

    /// The pane's own area, and the row a queued task is pinned to.
    ///
    /// # Why the queued row is pinned rather than added to the pane's lines
    ///
    /// [`Shell::pane_lines`] is the transcript, then this session's notices,
    /// then the answer being streamed — in that order — and [`Shell::visible`]
    /// shows the **tail**. So a notice sits *above* an answer that is still
    /// growing, and a long answer pushes it out of the visible tail within a
    /// beat or two. That is measurable rather than theoretical: the
    /// look-and-feel survey's row 12 recorded "nothing is refused, and no line
    /// says why" of a build that had carried a refusal notice for fifty
    /// commits. It had been painted and then scrolled away.
    ///
    /// A queued task that vanished behind a long answer would be useless in
    /// exactly the case a person most needs it, so it takes a reserved row at
    /// the foot of the pane's own region, which no amount of scrolling can
    /// reach.
    ///
    /// **The composer's area is untouched.** [ADR-0005] D2's one input row and
    /// its six reserved strip rows are unchanged, and the input row is still a
    /// function of the terminal's size alone — the row comes out of the pane,
    /// above the composer, not out of the composer.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    fn pane_and_queue(&self, pane: Rect) -> (Rect, Option<Rect>) {
        if self.queued().is_none() {
            return (pane, None);
        }
        let [above, pinned] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(pane);
        (above, Some(pinned))
    }

    /// Paint the whole shell into `area`.
    ///
    /// `palette` decides whether the registers' colours are painted, and it
    /// is an argument rather than shell state because the terminal is what
    /// knows — see [`Palette`]. **It reaches exactly one thing**: a
    /// transcript row's marker column. The status line, the composer, the
    /// hint strip and a standing question are painted the same way at either
    /// value, which is `no_register_colour_reaches_the_status_line_the_\
    /// composer_or_the_hint_strip`.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, palette: Palette) {
        let [status, pane, composer] = Self::regions(area);
        let (pane, queued) = self.pane_and_queue(pane);

        frame.render_widget(
            Paragraph::new(TextLine::from(self.status().painted(status.width))),
            status,
        );

        // No `Wrap` on this paragraph, and that is deliberate: `visible`
        // has already broken every row to `pane.width`, and a widget
        // re-wrapping them would measure a continuation's indent as content
        // and break it again one column early.
        let visible: Vec<TextLine<'_>> = self
            .visible(pane.height, pane.width)
            .into_iter()
            // Two spans rather than one joined string: the marker column
            // carries the register's colour and the producer's words carry
            // nothing. `Paragraph` sets a style per grapheme, so the style
            // reaches the cells `lead` paints and not the blanks past the end
            // of the row.
            //
            // It is also what lets `a_rows_joined_form_is_what_the_pane_\
            // painted_before` compare `Row::joined` against the buffer
            // without both arms travelling through the same function.
            .map(|row| {
                TextLine::from(vec![
                    Span::styled(row.lead, palette.marker(row.register)),
                    Span::raw(row.text),
                ])
            })
            .collect();
        if !visible.is_empty() {
            frame.render_widget(Paragraph::new(visible), pane);
        }

        // The queued task, on its own row immediately above the composer. It
        // goes through `Line::rows` so the register, its glyph and the width
        // measurement are the pane's own and nothing is authored here beyond
        // the one word `QUEUED` carries.
        if let (Some(area), Some(task)) = (queued, self.queued()) {
            // `Row::joined` rather than the two spans the pane uses, and the
            // register stays `Plain`: this row sits **below** the pane, in the
            // composer's own area, and the ruling of 2026-09-13 23:58Z puts a
            // colour on a transcript line's marker and nowhere else. `Plain`'s
            // colour is `Color::Reset` either way, so the two spellings paint
            // the same cells -- what `joined` says is that the choice is
            // deliberate rather than incidental.
            let row =
                crate::shell::port::Line::new(crate::shell::port::Register::Plain, task.painted())
                    .rows(area.width)
                    .first()
                    .map(crate::shell::port::Row::joined)
                    .unwrap_or_default();
            frame.render_widget(Paragraph::new(TextLine::from(row)), area);
        }

        // A standing question takes the composer's area whole. The input is
        // not usable while one stands -- ADR-0011 D3's prompt is not something
        // a user types past -- and painting both would offer a surface that
        // does nothing.
        let question: Vec<TextLine<'_>> = self
            .prompt_lines()
            .into_iter()
            .map(TextLine::from)
            .collect();
        if question.is_empty() {
            self.composer().render(frame, composer);
        } else {
            frame.render_widget(Paragraph::new(question), composer);
        }
    }
}
