// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cutting a file into the pieces the index holds.
//!
//! # The rule
//!
//! - Where the syntax parser reads the file, a piece is one declaration: a
//!   function, a type, a constant and so on, with the comment lines and
//!   attributes just above it. A module or an `impl` block is not a piece of
//!   its own, because the declarations inside it are.
//! - A declaration that holds another one keeps only the lines before the
//!   first one it holds: a class's head and fields, not its methods again.
//! - Lines no declaration covers become pieces of at most [`WINDOW_LINES`]
//!   lines, when they hold at least [`LEAST_LINES`] lines that are not blank.
//! - A file the parser does not read, or reads with an error, is cut into
//!   windows of [`WINDOW_LINES`] lines.
//! - No piece is longer than [`WINDOW_LINES`] lines: a longer declaration is
//!   cut into windows of that size.
//! - Tests are left out: a module named `tests` or `test`, and a declaration
//!   whose attributes say `#[test]`, `#[tokio::test]` or `#[cfg(test)]`. The
//!   walk already leaves out test files; this is the tests inside a source
//!   file.
//! - A piece of fewer than [`SMALL_LINES`] lines that are not blank is joined
//!   to the piece next to it, when only blank lines lie between them and the
//!   two together are at most [`WINDOW_LINES`] lines: a run of one-line
//!   constants is one piece. A very short text is near many questions in
//!   meaning, so a lone one-line piece crowded out the pieces a question was
//!   about (measured 2026-09-28 on the seal gateway).
//!
//! The model reads at most 512 tokens of a piece. Forty lines of code are
//! about 400 to 600 tokens, so a piece is rarely cut short by the model.

use crate::tools::codebase::{self, Symbol};
use std::path::Path;

/// The most lines a piece holds.
pub const WINDOW_LINES: usize = 40;

/// The fewest lines that are not blank a stretch between declarations needs
/// to be a piece.
pub const LEAST_LINES: usize = 3;

/// Pieces with fewer lines than this that are not blank are joined to the
/// piece next to them.
pub const SMALL_LINES: usize = 4;

/// Attributes that mark a test.
const TEST_MARKS: [&str; 3] = ["#[test]", "#[tokio::test", "#[cfg(test)]"];

/// Which rule cut the pieces. An index cut by another rule is built again.
pub const RULE: u32 = 2;

/// One piece of a file: its lines, from 1, both ends included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// The first line.
    pub start: usize,
    /// The last line.
    pub end: usize,
    /// The lines, as they are in the file.
    pub text: String,
}

/// Lines that mark a comment or an attribute above a declaration.
const ABOVE_MARKS: [&str; 8] = ["//", "#", "*", "/*", "\"\"\"", "--", "@", "#["];

/// Cut `text`, the contents of `path`, into pieces.
#[must_use]
pub fn chunks(path: &Path, text: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut symbols: Vec<Symbol> = Vec::new();
    codebase::collect(path, text, &mut symbols);
    // Tests inside the file: a `tests` module, or a declaration marked as one.
    let tests: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|symbol| symbol.is_declaration())
        .filter_map(|symbol| {
            let start = first_line_above(&lines, symbol.line);
            let marked = lines[start - 1..symbol.line.min(lines.len())]
                .iter()
                .any(|line| TEST_MARKS.iter().any(|mark| line.trim().starts_with(mark)));
            let named = symbol.kind == "module" && matches!(symbol.name.as_str(), "tests" | "test");
            (marked || named).then_some((start, symbol.end.min(lines.len())))
        })
        .collect();
    let mut spans: Vec<(usize, usize)> = symbols
        .iter()
        .filter(|symbol| symbol.is_declaration())
        .filter(|symbol| !matches!(symbol.kind, "module" | "implementation"))
        .map(|symbol| {
            (
                first_line_above(&lines, symbol.line),
                symbol.end.min(lines.len()),
            )
        })
        .filter(|(start, end)| start <= end)
        .collect();
    spans.sort_by(|left, right| left.0.cmp(&right.0).then(right.1.cmp(&left.1)));
    spans.dedup();

    // A declaration that holds another keeps the lines before the first one.
    let mut cut: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (at, (start, end)) in spans.iter().enumerate() {
        let inner = spans
            .iter()
            .enumerate()
            .filter(|(other, (s, e))| {
                *other != at && s >= start && e <= end && (s, e) != (start, end)
            })
            .map(|(_, (s, _))| *s)
            .min();
        let end = match inner {
            Some(first) if first > *start => first - 1,
            Some(_) => continue,
            None => *end,
        };
        cut.push((*start, end));
    }
    cut.sort_unstable();

    // Pieces for declarations, then windows for the stretches between them.
    let mut pieces: Vec<(usize, usize)> = Vec::new();
    let mut next_uncovered = 1;
    for (start, end) in cut {
        if start < next_uncovered {
            // Overlaps a piece already taken: keep only what is new.
            if end < next_uncovered {
                continue;
            }
            windows(&lines, next_uncovered, end, &mut pieces, 1);
            next_uncovered = end + 1;
            continue;
        }
        windows(&lines, next_uncovered, start - 1, &mut pieces, LEAST_LINES);
        windows(&lines, start, end, &mut pieces, 1);
        next_uncovered = end + 1;
    }
    windows(
        &lines,
        next_uncovered,
        lines.len(),
        &mut pieces,
        LEAST_LINES,
    );

    pieces.retain(|(start, end)| !tests.iter().any(|(s, e)| s <= start && end <= e));
    let pieces = joined(&lines, &pieces);

    pieces
        .into_iter()
        .map(|(start, end)| Chunk {
            start,
            end,
            text: lines[start - 1..end].join("\n"),
        })
        .collect()
}

/// Join each small piece to the piece next to it, where only blank lines lie
/// between them and the two fit in a window.
fn joined(lines: &[&str], pieces: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let filled = |(start, end): (usize, usize)| {
        lines[start - 1..end]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count()
    };
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(pieces.len());
    for piece in pieces {
        if let Some(last) = out.last_mut() {
            let between_blank = lines
                .get(last.1..piece.0.saturating_sub(1))
                .is_none_or(|between| between.iter().all(|line| line.trim().is_empty()));
            let small = filled(*last) < SMALL_LINES || filled(*piece) < SMALL_LINES;
            if small && between_blank && piece.1 + 1 - last.0 <= WINDOW_LINES {
                last.1 = piece.1;
                continue;
            }
        }
        out.push(*piece);
    }
    out
}

/// The first line of the comment and attribute block just above `line`.
fn first_line_above(lines: &[&str], line: usize) -> usize {
    let mut first = line;
    while first > 1 {
        let above = lines[first - 2].trim();
        if ABOVE_MARKS.iter().any(|mark| above.starts_with(mark)) {
            first -= 1;
        } else {
            break;
        }
    }
    first
}

/// Cut lines `start..=end` into windows of at most [`WINDOW_LINES`], each
/// kept when it holds at least `least` lines that are not blank.
fn windows(
    lines: &[&str],
    start: usize,
    end: usize,
    pieces: &mut Vec<(usize, usize)>,
    least: usize,
) {
    let mut from = start;
    while from <= end && from >= 1 {
        let to = (from + WINDOW_LINES - 1).min(end);
        let filled = lines[from - 1..to]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .count();
        if filled >= least {
            pieces.push((from, to));
        }
        from = to + 1;
    }
}
