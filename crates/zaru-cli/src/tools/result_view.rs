// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a person is shown of a call's result.
//!
//! # Why this exists
//!
//! Until 2026-09-28 the pane said that a call ran, its exit and a byte count,
//! and never what it printed or what it changed. Measured on `c1769f6`: a
//! failing test run painted `cmd.run reported a failure · 1290 bytes` and not
//! one line of the failure, and an edit painted `fs.edit returned · 285
//! bytes` and not the change. A person approved the call and then could not
//! see what came of it.
//!
//! This composes a [`ResultView`] for each built-in call that completed:
//!
//! - a command: its exit, how much it printed on each stream, and its last
//!   lines, standard error marked;
//! - an edit or a write: the lines it changed, as a diff with line numbers
//!   and a few lines around each change; a new file: its first lines and its
//!   length;
//! - a read, a listing, a search or a fetch: one line saying what came back.
//!
//! # What is shown is what the model got, made safe to draw
//!
//! Every text passes the session's redactor first, so a stored key never
//! reaches a pane, and then [`harmless`], so a command's output cannot move
//! the cursor, clear the screen, set the title or write outside its block.
//!
//! # The row limits, and where the rest is
//!
//! A command shows at most [`OUTPUT_ROWS`] lines, a change at most
//! [`CHANGE_ROWS`] rows and a new file its first [`NEW_FILE_ROWS`] lines.
//! The numbers are set by the smallest terminal the pane is drawn for, 60 by
//! 20, whose transcript has 12 rows once the status row and the composer's 7
//! are taken. The call's own last line, the summary, a note of three rows (a
//! session's path does not fit on one row of 60 columns) and 7 rows fill it,
//! so a person sees which call a block belongs to at every size. Each row is
//! one screen row: the pane cuts a long line at its edge and marks the cut.
//! When a view leaves lines out, or has a line long enough to be cut, the
//! whole of it is kept in a file in the session directory and the note row
//! names that file.

use std::path::Path;
use zaru_core::tool_call::{Mark, ResultView, ViewRow};

/// The most lines of a command's output a view shows.
pub const OUTPUT_ROWS: usize = 7;

/// The most rows of a change a view shows.
pub const CHANGE_ROWS: usize = 7;

/// How many of a new file's first lines a view shows.
pub const NEW_FILE_ROWS: usize = 5;

/// How many unchanged lines a change shows on each side of a changed line.
pub const CONTEXT_LINES: usize = 2;

/// The largest file whose old and new text are read to show a change.
pub const CHANGE_READ_CEILING: u64 = 1024 * 1024;

/// A line longer than this, in characters, can be cut at the edge of a pane,
/// so a view with one keeps the whole of itself in a file.
pub const LONG_LINE: usize = 100;

/// How a file holding the whole of a view is named.
pub const WHOLE_PREFIX: &str = "shown-";

/// A view, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// The view, without its note on what is left out.
    pub view: ResultView,
    /// What it left out, when it left anything out or cut a long line.
    pub left_out: Option<LeftOut>,
}

/// What a view left out, and the whole it was taken from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    /// What the note row says is not shown. Empty when nothing is left out
    /// and a long line is only cut at the pane's edge.
    pub sentence: String,
    /// Whether the note goes before the rows, because they are the output's
    /// end, rather than after them, because they are a change's start.
    pub first: bool,
    /// The whole, to keep in a file.
    pub whole: String,
}

impl Composed {
    /// The view with its note: what is not shown, and where the whole is.
    #[must_use]
    pub fn finished(self, kept_at: Option<&Path>) -> ResultView {
        let Self { mut view, left_out } = self;
        let Some(left_out) = left_out else {
            return view;
        };
        let mut said = Vec::new();
        if !left_out.sentence.is_empty() {
            said.push(left_out.sentence);
        }
        if let Some(path) = kept_at {
            said.push(format!(
                "all of it: {}",
                harmless(&path.display().to_string())
            ));
        }
        if said.is_empty() {
            return view;
        }
        let note = ViewRow::new(Mark::Note, said.join(" · "));
        if left_out.first {
            view.rows.insert(0, note);
        } else {
            view.rows.push(note);
        }
        view
    }
}

/// `text` with nothing a terminal would act on.
///
/// Every control character is written out as its escape, `\u{1b}` for the
/// escape that begins a terminal sequence, so the person sees that the output
/// held one and the terminal does nothing with it. A tab becomes spaces to
/// the next multiple of eight. The characters that reverse the direction text
/// is drawn in are written out too, because they change what a row appears to
/// say. A line break is written out, so a row stays one row.
#[must_use]
pub fn harmless(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0_usize;
    for character in text.chars() {
        if character == '\t' {
            let to = (column / 8 + 1) * 8;
            out.extend(core::iter::repeat_n(' ', to - column));
            column = to;
        } else if character.is_control() {
            let escaped: String = character.escape_debug().collect();
            column += escaped.chars().count();
            out.push_str(&escaped);
        } else if is_direction_control(character) {
            let escaped: String = character.escape_unicode().collect();
            column += escaped.chars().count();
            out.push_str(&escaped);
        } else {
            column += 1;
            out.push(character);
        }
    }
    out
}

/// The characters that change the direction text is drawn in.
const fn is_direction_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
}

/// The lines of `text`: no empty line after a final newline, and no carriage
/// return at the end of a line that ended in CRLF.
fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    body.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// `n` with the noun it counts, one or many.
fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Whether any line is long enough to be cut at a pane's edge.
fn has_a_long_line<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    lines
        .into_iter()
        .any(|line| line.chars().count() > LONG_LINE)
}

/// A command's view: its exit, how much it printed, and its last lines.
///
/// `stdout` and `stderr` are what the model was given, already redacted.
#[must_use]
pub fn command(exit_code: i32, stdout: &str, stderr: &str) -> Composed {
    let out = lines_of(stdout);
    let err = lines_of(stderr);
    let printed = match (out.len(), err.len()) {
        (0, 0) => String::from("printed nothing"),
        (n, 0) => format!("{} on standard output", counted(n, "line", "lines")),
        (0, n) => format!("{} on standard error", counted(n, "line", "lines")),
        (a, b) => format!(
            "{} on standard output, {b} on standard error",
            counted(a, "line", "lines")
        ),
    };
    let summary = format!("exit {exit_code} · {printed}");

    // Standard error keeps at least half the rows when both streams printed,
    // because that is where most programs say what went wrong.
    let err_rows = if out.is_empty() {
        err.len().min(OUTPUT_ROWS)
    } else {
        err.len()
            .min((OUTPUT_ROWS / 2).max(OUTPUT_ROWS.saturating_sub(out.len())))
    };
    let out_rows = out.len().min(OUTPUT_ROWS - err_rows);

    let mut rows: Vec<ViewRow> = Vec::new();
    rows.extend(
        out[out.len() - out_rows..]
            .iter()
            .map(|line| ViewRow::new(Mark::Output, harmless(line))),
    );
    rows.extend(
        err[err.len() - err_rows..]
            .iter()
            .map(|line| ViewRow::new(Mark::Error, harmless(line))),
    );

    let hidden_out = out.len() - out_rows;
    let hidden_err = err.len() - err_rows;
    let sentence = match (hidden_out, hidden_err) {
        (0, 0) => String::new(),
        (n, 0) | (0, n) => format!("{} not shown", counted(n, "earlier line", "earlier lines")),
        (a, b) => format!(
            "{} not shown: {a} of standard output and {b} of standard error",
            counted(a + b, "earlier line", "earlier lines")
        ),
    };
    let long = has_a_long_line(out.iter().chain(err.iter()).copied());
    let left_out = (!sentence.is_empty() || long).then(|| LeftOut {
        sentence,
        first: true,
        whole: format!(
            "exit code: {exit_code}\n--- standard output ---\n{}\n--- standard error ---\n{}\n",
            out.iter()
                .map(|line| harmless(line))
                .collect::<Vec<_>>()
                .join("\n"),
            err.iter()
                .map(|line| harmless(line))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    });
    Composed {
        view: ResultView { summary, rows },
        left_out,
    }
}

/// What a file held before a write or an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Before {
    /// There was no file.
    Absent,
    /// It held this text, already redacted.
    Text(String),
    /// Its text is not shown, for this reason.
    NotShown(&'static str),
}

impl Before {
    /// What the file at `path` holds now, read for showing a change later.
    ///
    /// Read only when it is a regular file no larger than
    /// [`CHANGE_READ_CEILING`] and is UTF-8 text with no zero byte.
    #[must_use]
    pub fn of(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Err(_) => Self::Absent,
            Ok(metadata) if !metadata.is_file() => Self::NotShown("it is not a regular file"),
            Ok(metadata) if metadata.len() > CHANGE_READ_CEILING => {
                Self::NotShown("the file is larger than 1 MiB")
            }
            Ok(_) => match std::fs::read(path) {
                Err(_) => Self::NotShown("the file could not be read"),
                Ok(bytes) if bytes.contains(&0) => Self::NotShown("the file is not text"),
                Ok(bytes) => String::from_utf8(bytes)
                    .map_or(Self::NotShown("the file is not UTF-8 text"), Self::Text),
            },
        }
    }

    /// The same, with its text passed through `redact`.
    #[must_use]
    pub fn redacted(self, redact: impl Fn(&str) -> String) -> Self {
        match self {
            Self::Text(text) => Self::Text(redact(&text)),
            other => other,
        }
    }
}

/// A change's view: the lines a write or an edit changed.
///
/// `shown` is the file as the person knows it, relative to the working
/// directory where it is inside it. `verb` is what the call did, such as
/// "changed" or "replaced". `before` and `after` are already redacted.
#[must_use]
pub fn change(shown: &str, verb: &str, before: &Before, after: &Before) -> Composed {
    let shown = harmless(shown);
    let (before, after) = match (before, after) {
        (_, Before::NotShown(reason)) | (Before::NotShown(reason), _) => {
            return Composed {
                view: ResultView {
                    summary: format!("{verb} {shown} · the change is not shown: {reason}"),
                    rows: Vec::new(),
                },
                left_out: None,
            };
        }
        (_, Before::Absent) => {
            return Composed {
                view: ResultView {
                    summary: format!("{verb} {shown} · the file is not there now"),
                    rows: Vec::new(),
                },
                left_out: None,
            };
        }
        (Before::Absent, Before::Text(after)) => return created(&shown, after),
        (Before::Text(before), Before::Text(after)) => (before, after),
    };

    let old = lines_of(before);
    let new = lines_of(after);
    let ops = diff(&old, &new);
    let removed = ops.iter().filter(|op| matches!(op, Op::Delete(_))).count();
    let added = ops.iter().filter(|op| matches!(op, Op::Insert(_))).count();
    if removed == 0 && added == 0 {
        return Composed {
            view: ResultView {
                summary: format!("{verb} {shown} · no line changed"),
                rows: Vec::new(),
            },
            left_out: None,
        };
    }
    let summary = format!(
        "{verb} {shown} · {} removed, {} added",
        counted(removed, "line", "lines"),
        added
    );

    let every = hunks(&ops)
        .into_iter()
        .enumerate()
        .flat_map(|(k, hunk)| {
            let gap = (k > 0).then(|| ViewRow::new(Mark::Gap, String::new()));
            gap.into_iter()
                .chain(hunk.into_iter().map(|op| match op {
                    Op::Equal(_, j) => ViewRow::numbered(Mark::Context, j + 1, harmless(new[j])),
                    Op::Delete(i) => ViewRow::numbered(Mark::Removed, i + 1, harmless(old[i])),
                    Op::Insert(j) => ViewRow::numbered(Mark::Added, j + 1, harmless(new[j])),
                }))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let rows: Vec<ViewRow> = every.iter().take(CHANGE_ROWS).cloned().collect();
    let hidden = every.len() - rows.len();
    let long = has_a_long_line(rows.iter().map(|row| row.text.as_str()));
    let left_out = (hidden > 0 || long).then(|| LeftOut {
        sentence: if hidden > 0 {
            format!(
                "{} of the change not shown",
                counted(hidden, "more row", "more rows")
            )
        } else {
            String::new()
        },
        first: false,
        whole: whole_of(&every),
    });
    Composed {
        view: ResultView { summary, rows },
        left_out,
    }
}

/// A new file's view: its first lines and its length.
fn created(shown: &str, after: &str) -> Composed {
    let lines = lines_of(after);
    let summary = format!(
        "created {shown} · {}, {}",
        counted(lines.len(), "line", "lines"),
        counted(after.len(), "byte", "bytes")
    );
    let rows: Vec<ViewRow> = lines
        .iter()
        .take(NEW_FILE_ROWS)
        .enumerate()
        .map(|(at, line)| ViewRow::numbered(Mark::Added, at + 1, harmless(line)))
        .collect();
    let hidden = lines.len() - rows.len();
    let long = has_a_long_line(rows.iter().map(|row| row.text.as_str()));
    let left_out = (hidden > 0 || long).then(|| LeftOut {
        sentence: if hidden > 0 {
            format!("{} not shown", counted(hidden, "more line", "more lines"))
        } else {
            String::new()
        },
        first: false,
        whole: whole_of(
            &lines
                .iter()
                .enumerate()
                .map(|(at, line)| ViewRow::numbered(Mark::Added, at + 1, harmless(line)))
                .collect::<Vec<_>>(),
        ),
    });
    Composed {
        view: ResultView { summary, rows },
        left_out,
    }
}

/// How wide a view's line-number column is: the widest number it shows.
#[must_use]
pub fn number_width(rows: &[ViewRow]) -> usize {
    rows.iter()
        .filter_map(|row| row.number)
        .map(|number| number.to_string().len())
        .max()
        .unwrap_or(1)
}

/// One row as text: its mark, then what it says.
///
/// **The mark is a character, never a colour alone**: `out` and `err` before
/// a command's lines, `-` and `+` before a change's, and the line's number.
/// `ascii` spells the separators with ASCII only, for output that is not a
/// terminal and for the file that keeps the whole: a pipe carries it and a
/// reader of the file sees the same marks.
#[must_use]
pub fn row_text(row: &ViewRow, width: usize, ascii: bool) -> String {
    let (bar, gap, more) = if ascii {
        ("|", "...", "...")
    } else {
        ("\u{2502}", "\u{22ee}", "\u{2026}")
    };
    let number = row.number.map_or_else(String::new, |n| n.to_string());
    match row.mark {
        Mark::Output => format!("out {bar} {}", row.text),
        Mark::Error => format!("err {bar} {}", row.text),
        Mark::Removed => format!("{number:>width$} - {bar} {}", row.text),
        Mark::Added => format!("{number:>width$} + {bar} {}", row.text),
        Mark::Context => format!("{number:>width$}   {bar} {}", row.text),
        Mark::Gap => format!("{:>width$}   {gap}", ""),
        Mark::Note => format!("{more} {}", row.text),
    }
}

/// A view as plain lines: the summary, then each row, ASCII marks only.
///
/// For `zaru "<task>"`, whose output is not a terminal session and may be a
/// pipe. The text is made safe again here, for the reason the pane's own
/// rendering does it.
#[must_use]
pub fn plain_lines(view: &ResultView) -> Vec<String> {
    let width = number_width(&view.rows);
    let mut lines = vec![format!("  {}", harmless(&view.summary))];
    lines.extend(view.rows.iter().map(|row| {
        let safe = ViewRow {
            text: harmless(&row.text),
            ..row.clone()
        };
        format!("  {}", row_text(&safe, width, true))
    }));
    lines
}

/// Every row of `rows` as ASCII text, one per line, for the file that keeps
/// the whole of a view.
fn whole_of(rows: &[ViewRow]) -> String {
    let width = number_width(rows);
    rows.iter()
        .map(|row| row_text(row, width, true))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// One step of a diff between two lists of lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Line `i` of the old text is line `j` of the new.
    Equal(usize, usize),
    /// Line `i` of the old text was removed.
    Delete(usize),
    /// Line `j` of the new text was added.
    Insert(usize),
}

/// The most edits the diff searches for before it shows the changed middle
/// as removed and added whole.
const MOST_EDITS: usize = 500;

/// A shortest diff of two lists of lines.
///
/// The lines both share at the start and at the end are matched first. The
/// middle is diffed with Myers' algorithm, which finds the fewest lines
/// removed and added; past [`MOST_EDITS`] it is shown as removed and added
/// whole, which is still a true diff, only a longer one.
fn diff(old: &[&str], new: &[&str]) -> Vec<Op> {
    let head = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = old[head..]
        .iter()
        .rev()
        .zip(new[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (a, b) = (&old[head..old.len() - tail], &new[head..new.len() - tail]);

    let mut ops: Vec<Op> = (0..head).map(|k| Op::Equal(k, k)).collect();
    let middle = myers(a, b).unwrap_or_else(|| {
        (0..a.len())
            .map(Op::Delete)
            .chain((0..b.len()).map(Op::Insert))
            .collect()
    });
    ops.extend(middle.into_iter().map(|op| match op {
        Op::Equal(i, j) => Op::Equal(i + head, j + head),
        Op::Delete(i) => Op::Delete(i + head),
        Op::Insert(j) => Op::Insert(j + head),
    }));
    ops.extend((0..tail).map(|k| Op::Equal(old.len() - tail + k, new.len() - tail + k)));
    ops
}

/// Myers' diff of `a` and `b`, or `None` when it needs more than
/// [`MOST_EDITS`] edits.
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "indices are bounded by the two lists' lengths, which are far below isize::MAX"
)]
fn myers(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let (n, m) = (a.len() as isize, b.len() as isize);
    let max = (a.len() + b.len()).min(MOST_EDITS);
    let offset = max as isize + 1;
    let width = 2 * max + 3;
    let mut v = vec![0_isize; width];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = false;
    for d in 0..=max as isize {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let at = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[at - 1] < v[at + 1]) {
                v[at + 1]
            } else {
                v[at - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[at] = x;
            if x >= n && y >= m {
                found = true;
                break;
            }
            k += 2;
        }
        if found {
            trace.push(v.clone());
            break;
        }
    }
    if !found {
        return None;
    }

    // Walk back from the end through the kept frontiers.
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..trace.len() - 1).rev() {
        let d = d as isize;
        let v = &trace[d as usize];
        let k = x - y;
        let at = |k: isize| (k + offset) as usize;
        let previous_k = if k == -d || (k != d && v[at(k - 1)] < v[at(k + 1)]) {
            k + 1
        } else {
            k - 1
        };
        let previous_x = v[at(previous_k)];
        let previous_y = previous_x - previous_k;
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            ops.push(Op::Equal(x as usize, y as usize));
        }
        if x == previous_x {
            y -= 1;
            ops.push(Op::Insert(y as usize));
        } else {
            x -= 1;
            ops.push(Op::Delete(x as usize));
        }
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        ops.push(Op::Equal(x as usize, y as usize));
    }
    ops.reverse();
    Some(ops)
}

/// The diff cut into the parts that changed, each with up to
/// [`CONTEXT_LINES`] unchanged lines on each side. Two parts whose context
/// would touch are one part.
fn hunks(ops: &[Op]) -> Vec<Vec<Op>> {
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| !matches!(op, Op::Equal(..)))
        .map(|(at, _)| at)
        .collect();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for at in changed {
        let from = at.saturating_sub(CONTEXT_LINES);
        let to = (at + CONTEXT_LINES + 1).min(ops.len());
        match spans.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => spans.push((from, to)),
        }
    }
    spans
        .into_iter()
        .map(|(from, to)| ops[from..to].to_vec())
        .collect()
}

/// A read's view: which lines of which file came back.
///
/// `answer` is the text the model was given, whose first two lines
/// [`crate::tools::reading`] writes as the file's line count and the lines
/// that follow.
#[must_use]
pub fn read(shown: &str, answer: &str) -> Composed {
    let shown = harmless(shown);
    let mut lines = answer.lines();
    let total = lines.next().and_then(|first| {
        let before = first.split(" line(s)").next()?;
        before.rsplit(": ").next()?.parse::<usize>().ok()
    });
    let span = lines.next().and_then(|second| {
        let sentence = second.split('.').next()?;
        if let Some(rest) = sentence.strip_prefix("Lines ") {
            let (first, last) = rest.split_once(" to ")?;
            let last = last.split(' ').next()?;
            Some((first.parse::<usize>().ok()?, last.parse::<usize>().ok()?))
        } else {
            let one = sentence.strip_prefix("Line ")?.split(' ').next()?;
            let one = one.parse::<usize>().ok()?;
            Some((one, one))
        }
    });
    let summary = match (span, total) {
        (Some((first, last)), Some(total)) if first == last => {
            format!("read {shown} · line {first} of {total}")
        }
        (Some((first, last)), Some(total)) => {
            format!("read {shown} · lines {first} to {last} of {total}")
        }
        _ => format!(
            "read {shown} · {} returned",
            counted(answer.len(), "byte", "bytes")
        ),
    };
    one_line(summary)
}

/// A listing's view: how many entries came back.
#[must_use]
pub fn list(shown: &str, answer: &str) -> Composed {
    one_line(format!(
        "listed {} · {}",
        harmless(shown),
        counted(lines_of(answer).len(), "entry", "entries")
    ))
}

/// A search's view: how many lines matched, in how many files.
///
/// Read from the answer's first line, which counts every matching line,
/// shown to the model or not.
#[must_use]
pub fn search(shown: &str, needle: &str, found: &str) -> Composed {
    let tally = crate::tools::searching::tally(found).unwrap_or_default();
    let mut what = if tally.lines > 0 {
        format!(
            "{} found in {}",
            counted(tally.lines, "line", "lines"),
            counted(tally.files, "file", "files")
        )
    } else if tally.declarations > 0 {
        format!(
            "{} found",
            counted(tally.declarations, "declaration", "declarations")
        )
    } else if tally.meanings > 0 {
        String::new()
    } else {
        String::from("nothing found")
    };
    // Retrieval by meaning adds its count, and only when it found something,
    // so a search with it off shows the line it always did.
    if tally.meanings > 0 {
        let by_meaning = format!(
            "{} found by meaning",
            counted(tally.meanings, "place", "places")
        );
        what = if what.is_empty() {
            by_meaning
        } else {
            format!("{what}, {by_meaning}")
        };
    }
    one_line(format!(
        "searched {} for \"{}\" · {what}",
        harmless(shown),
        harmless(needle)
    ))
}

/// A fetch's view: the status, the size and the page's title.
///
/// `retrieved` is what the model was given: a heading line, then the body.
#[must_use]
pub fn fetch(shown: &str, retrieved: &str) -> Composed {
    let (heading, body) = retrieved.split_once('\n').unwrap_or((retrieved, ""));
    let status = heading
        .split(" — ")
        .nth(1)
        .filter(|status| status.parse::<u16>().is_ok());
    let mut parts = vec![format!("fetched {}", harmless(shown))];
    if let Some(status) = status {
        parts.push(format!("status {status}"));
    }
    parts.push(counted(body.len(), "byte", "bytes"));
    if let Some(title) = title_of(body) {
        parts.push(format!("title \"{}\"", harmless(&title)));
    }
    one_line(parts.join(" · "))
}

/// The text of an HTML page's `<title>`, spaces collapsed, at most eighty
/// characters.
fn title_of(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let title: String = body
        .get(start..end)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return None;
    }
    Some(if title.chars().count() > 80 {
        title
            .chars()
            .take(79)
            .chain(core::iter::once('…'))
            .collect()
    } else {
        title
    })
}

/// A call that failed: what it said, first line as the summary.
#[must_use]
pub fn failed(said: &str) -> Composed {
    let lines = lines_of(said);
    let Some((first, rest)) = lines.split_first() else {
        return one_line(String::from("failed, and said nothing"));
    };
    let rows: Vec<ViewRow> = rest
        .iter()
        .take(OUTPUT_ROWS)
        .map(|line| ViewRow::new(Mark::Error, harmless(line)))
        .collect();
    let hidden = rest.len() - rows.len();
    let left_out = (hidden > 0).then(|| LeftOut {
        sentence: format!("{} not shown", counted(hidden, "more line", "more lines")),
        first: false,
        whole: lines
            .iter()
            .map(|line| harmless(line))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    });
    Composed {
        view: ResultView {
            summary: format!("failed: {}", harmless(first)),
            rows,
        },
        left_out,
    }
}

/// A view that is one line.
fn one_line(summary: String) -> Composed {
    Composed {
        view: ResultView {
            summary,
            rows: Vec::new(),
        },
        left_out: None,
    }
}

#[cfg(test)]
mod tests;
