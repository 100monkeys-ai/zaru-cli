// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staged inputs for the shell's checks.
//!
//! # The vocabulary here is a transcription and it is not the product's
//!
//! `zaru-cli` implements [`CommandVocabulary`] over [ADR-0015] D2's own closed
//! enum, and a check in this crate cannot see that crate. So the fixture below
//! transcribes D2's table, and what it establishes is that the **grammar**
//! reads a vocabulary correctly — never that the vocabulary is right.
//! `crates/zaru-cli/tests/shell_from_outside.rs` is the check that drives the
//! product's own adapter, and that is where the table's content is asserted.
//! Said here so that a green check in this file is not read as more than it is
//! ([Verification Lessons](https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons)
//! §1).
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::shell::port::{CommandVocabulary, Line, Namespace, Palette, Register, TranscriptSource};
use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Position;
use ratatui::style::Color;

/// A value planted in a staged transcript, so a check can look for something
/// that could only have come from the line it planted.
pub(crate) const TRANSCRIPT_NONCE: &str = "transcript-6b1d";

/// A value planted as a held secret, for the corpus check.
pub(crate) const SECRET_NONCE: &str = "nn_live_7a41c3e9";

/// [ADR-0015] D2's table, transcribed. See the module documentation.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub(crate) struct StagedVocabulary;

const NAMESPACES: [(&str, &str, bool, &[&str]); 11] = [
    ("/runtime", "tier and membrane", true, &[]),
    ("/stack", "AEGIS component fetch and status", false, &[]),
    (
        "/notes",
        "Nuclear Notes tokens, workspace, search",
        true,
        &["tokens"],
    ),
    (
        "/config",
        "configuration and explanation",
        true,
        &["explain"],
    ),
    ("/memory", "relationship memory", false, &[]),
    (
        "/learned",
        "what this session wrote to craft memory",
        false,
        &[],
    ),
    ("/inbox", "deposits", false, &[]),
    (
        "/session",
        "resume, list, remove",
        true,
        &["resume", "continue", "list", "rm"],
    ),
    ("/models", "alias resolution", true, &[]),
    ("/init", "the project manifest", true, &[]),
    ("/providers", "provider credentials", true, &["keys"]),
];

impl CommandVocabulary for StagedVocabulary {
    fn namespaces(&self) -> Vec<Namespace> {
        NAMESPACES
            .into_iter()
            .map(|(slash, governs, built, verbs)| Namespace {
                slash,
                governs,
                built,
                verbs,
            })
            .collect()
    }

    fn nearest(&self, offered: &str) -> Option<&'static str> {
        NAMESPACES
            .into_iter()
            .map(|(slash, ..)| slash)
            .min_by_key(|slash| distance(slash.trim_start_matches('/'), offered))
    }

    fn nearest_verb(&self, slash: &str, offered: &str) -> Option<&'static str> {
        NAMESPACES
            .into_iter()
            .find(|(spelling, ..)| *spelling == slash)?
            .3
            .iter()
            .copied()
            .min_by_key(|verb| distance(verb, offered))
    }
}

/// Levenshtein, for the fixture only.
///
/// The product's answer is `zaru_cli::config::nearest`, which is the one place
/// [ADR-0014] D5's rule lives. This is a stand-in so the grammar can be
/// exercised without it, and it is deliberately the simplest thing that
/// orders candidates.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (row, left_char) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = usize::from(left_char != *right_char);
            current[column + 1] = (previous[column] + substitution)
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        core::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// A staged transcript, which is what stands in for a loop that does not run.
pub(crate) struct StagedTranscript(pub(crate) Vec<Line>);

impl TranscriptSource for StagedTranscript {
    fn lines(&self) -> Vec<Line> {
        self.0.clone()
    }
}

impl StagedTranscript {
    /// One line in each of the three registers [ADR-0008] clause 4 requires to
    /// be distinguishable, each carrying an elapsed time for clause 5.
    ///
    /// The wording is a producer's rather than the shell's, which is D3's
    /// "rendering never reads loop internals" — a check that composed these
    /// here and then asserted the shell rendered them would be asserting
    /// nothing.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    pub(crate) fn three_outcomes() -> Self {
        Self(vec![
            Line::new(Register::Succeeded, "succeeded after 2 iterations · 4.20s"),
            Line::new(
                Register::Exhausted,
                "exhausted at the ceiling after 3 iterations · 9.10s",
            ),
            Line::new(
                Register::Failed,
                "provider: no credential for alias `default`",
            ),
        ])
    }
}

/// Paint a shell and read the buffer back as rows, with the cursor.
///
/// Both arms of every frame assertion read cells out of `TestBackend` and
/// compare them against literals the check owns, so neither side travels back
/// through the shell's own formatter.
///
/// **The palette is [`Palette::Coloured`], which is what the product passes
/// when `NO_COLOR` is unset**, and it is named here rather than left implicit
/// because a fixture's default is a value somebody chose for a different
/// caller. Every check in this crate that predates 2026-09-14 reads symbols,
/// which no palette changes; the checks that read a colour say which palette
/// they painted under by calling [`painted_in`].
pub(crate) fn painted(
    shell: &crate::shell::Shell,
    width: u16,
    height: u16,
) -> (Vec<String>, Position) {
    painted_in(shell, width, height, Palette::Coloured)
}

/// The same, under a palette the check chose.
pub(crate) fn painted_in(
    shell: &crate::shell::Shell,
    width: u16,
    height: u16,
    palette: Palette,
) -> (Vec<String>, Position) {
    let (rows, cursor) = cells(shell, width, height, palette);
    (
        rows.into_iter()
            .map(|row| row.into_iter().map(|(symbol, _)| symbol).collect())
            .collect(),
        cursor,
    )
}

/// Paint a shell and read every cell back as its symbol **and its foreground
/// colour**.
///
/// A colour is a property of a cell rather than of a row, so a check about one
/// reads the cell rather than the row's bytes. **What a terminal would
/// actually be sent is a different measurement and is not taken here**:
/// `ratatui`'s crossterm backend is behind a feature this crate deliberately
/// does not take, so the byte-level count lives in `zaru-cli`, where
/// `a_monochrome_frame_writes_no_colour_sequence` takes it.
/// Paint a shell and read every cell back as its symbol, its foreground
/// colour **and its modifier**.
///
/// The third of the three properties a cell can carry that this workspace
/// asserts about. It is separate from [`cells`] rather than replacing it
/// because every check written before 2026-09-15 reads two, and widening the
/// tuple they destructure would be an edit to a hundred checks that are not
/// about a modifier.
pub(crate) fn styled_cells(
    shell: &crate::shell::Shell,
    width: u16,
    height: u16,
    palette: Palette,
) -> Vec<Vec<(String, Color, ratatui::style::Modifier)>> {
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
                    (cell.symbol().to_owned(), cell.fg, cell.modifier)
                })
                .collect()
        })
        .collect()
}

pub(crate) fn cells(
    shell: &crate::shell::Shell,
    width: u16,
    height: u16,
    palette: Palette,
) -> (Vec<Vec<(String, Color)>>, Position) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), palette))
        .expect("draw");
    let cursor = terminal
        .backend_mut()
        .get_cursor_position()
        .expect("the test backend records the cursor");
    let buffer = terminal.backend().buffer();
    let rows = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| {
                    let cell = &buffer[(x, y)];
                    (cell.symbol().to_owned(), cell.fg)
                })
                .collect()
        })
        .collect();
    (rows, cursor)
}
