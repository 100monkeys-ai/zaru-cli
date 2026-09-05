// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the shell paints.
//!
//! # Three regions, and only the middle one changes size
//!
//! ```text
//! ┌───────────────────────────────────────┐
//! │ runtime.tier = bare · session 01J…     │  status, 1 row, always
//! ├───────────────────────────────────────┤
//! │ ✓ finished in 4.2s                     │  transcript pane, the rest
//! │ ⊘ stopped at the ceiling after 3        │
//! ├───────────────────────────────────────┤
//! │ > what should I do                     │  composer, COMPOSER_ROWS, fixed
//! │   a page the trie matched              │
//! └───────────────────────────────────────┘
//! ```
//!
//! The status line is at the top and the composer's area is a fixed height at
//! the foot, so the input row sits at a position that is a function of the
//! terminal's size and nothing else. That is [ADR-0005] D2 in a shell — see
//! [`crate::shell`]'s own documentation for why the alternative fails.
//!
//! The pane shows the **tail**, which is [ADR-0010] D4's "re-renders the last
//! stretch of transcript so the user can see where they were".
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::shell::{COMPOSER_ROWS, Shell};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line as TextLine;
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

    /// The transcript lines the pane can show in `height` rows, oldest first.
    ///
    /// The **tail**: a pane shorter than the transcript shows the end of it,
    /// because that is where the user was.
    #[must_use]
    pub fn visible(&self, height: u16) -> Vec<String> {
        let lines = self.pane_lines();
        let height = usize::from(height);
        let start = lines.len().saturating_sub(height);
        lines[start..]
            .iter()
            .map(crate::shell::Line::painted)
            .collect()
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

    /// Paint the whole shell into `area`.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        let [status, pane, composer] = Self::regions(area);

        frame.render_widget(
            Paragraph::new(TextLine::from(self.status().painted())),
            status,
        );

        let visible: Vec<TextLine<'_>> = self
            .visible(pane.height)
            .into_iter()
            .map(TextLine::from)
            .collect();
        if !visible.is_empty() {
            frame.render_widget(Paragraph::new(visible), pane);
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
