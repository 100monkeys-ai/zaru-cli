// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Fitting what a question is about into the width a terminal has.
//!
//! # What a person must see before they answer
//!
//! A permission question asks about one thing: a file, a command, a URL. The
//! part of it a person cannot answer without is the part a plain cut takes
//! first. A path's end is the file's name; a command's start is the program; a
//! URL's start is the host. So each is shortened where it can lose the least:
//!
//! - a path in the **middle**, so its start and the file's name both show;
//! - a command after the last argument that fits, saying how many are not
//!   shown;
//! - a URL in the middle of its path, so the host and the path's end both
//!   show.
//!
//! Measured on `e5b9240`, before this existed: at 80 by 24, 60 by 20 and 120
//! by 40 alike, a write of 200 lines to a deep path showed the content's last
//! lines and the answers, and the line naming the file was not on the screen.
//!
//! # The words are this crate's, and there are three
//!
//! [`ELLIPSIS`], [`more_arguments`] and [`not_shown`] say that something was
//! left out and how much. They are drafted here, beside [`crate::shell::below`]
//! which says the same kind of thing about the pane, under a delegated
//! coordinator ruling of 2026-09-28 open to Jeshua's veto. Everything else a
//! question says is handed to this crate by `zaru-cli`.

use crate::shell::port::Shown;
use crate::shell::wrap::{columns, rows};

/// What stands where something was left out of a path, a URL or a line.
pub const ELLIPSIS: &str = "…";

/// What follows a command whose last `hidden` arguments did not fit.
#[must_use]
pub fn more_arguments(hidden: usize) -> String {
    if hidden == 1 {
        "… and 1 more argument".to_owned()
    } else {
        format!("… and {hidden} more arguments")
    }
}

/// What stands in a question's content for the `hidden` lines not shown.
#[must_use]
pub fn not_shown(hidden: usize) -> String {
    if hidden == 1 {
        "… 1 more line not shown".to_owned()
    } else {
        format!("… {hidden} more lines not shown")
    }
}

/// The rows a question's head takes at `width`: the lead and the subject,
/// then `end`.
///
/// On one row when the whole of it fits. Otherwise the lead is wrapped on its
/// own rows and the subject is fitted to the next row with `end` after it, so
/// the subject is never broken across rows and never cut off at its end.
#[must_use]
pub fn head(lead: &str, subject: &Shown, end: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let whole = format!("{lead} {}{end}", spelled(subject));
    if columns(&whole) <= width {
        return vec![whole];
    }
    let mut head = rows(lead, width);
    let room = width.saturating_sub(columns(end)).max(1);
    head.push(format!("{}{end}", fitted(subject, room)));
    head
}

/// The subject written out whole.
fn spelled(subject: &Shown) -> String {
    match subject {
        Shown::Path(text) | Shown::Text(text) => text.clone(),
        Shown::Command(words) => words.join(" "),
        Shown::Url { origin, rest } => format!("{origin}{rest}"),
    }
}

/// The subject in at most `width` columns.
#[must_use]
pub fn fitted(subject: &Shown, width: usize) -> String {
    let whole = spelled(subject);
    if columns(&whole) <= width {
        return whole;
    }
    match subject {
        Shown::Path(path) => path_in(path, width),
        Shown::Command(words) => command_in(words, width),
        Shown::Url { origin, rest } => url_in(origin, rest, width),
        Shown::Text(text) => start_of(text, width.saturating_sub(1)) + ELLIPSIS,
    }
}

/// A path shortened in the middle: as much of its start as fits, then
/// [`ELLIPSIS`], then its last component. If the last component alone does not
/// fit, the end of it does.
fn path_in(path: &str, width: usize) -> String {
    if columns(path) <= width {
        return path.to_owned();
    }
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    let tail = if name.len() < path.len() {
        format!("/{name}")
    } else {
        name.to_owned()
    };
    let for_the_start = width.saturating_sub(columns(ELLIPSIS) + columns(&tail));
    if for_the_start == 0 {
        return format!(
            "{ELLIPSIS}{}",
            end_of(&tail, width.saturating_sub(columns(ELLIPSIS)))
        );
    }
    format!("{}{ELLIPSIS}{tail}", start_of(path, for_the_start))
}

/// A command's words, as many as fit after the program, then how many more.
fn command_in(words: &[String], width: usize) -> String {
    let Some((program, arguments)) = words.split_first() else {
        return String::new();
    };
    let note = |hidden: usize| {
        if hidden == 0 {
            String::new()
        } else {
            format!(" {}", more_arguments(hidden))
        }
    };
    // The most arguments that fit with the note saying how many do not.
    for kept in (0..=arguments.len()).rev() {
        let mut shown = program.clone();
        for argument in &arguments[..kept] {
            shown.push(' ');
            shown.push_str(argument);
        }
        shown.push_str(&note(arguments.len() - kept));
        if columns(&shown) <= width {
            return shown;
        }
    }
    // Not even the program fits beside the note, which is what says the
    // command is incomplete, so the program is shortened in its middle.
    let note = note(arguments.len());
    format!(
        "{}{note}",
        path_in(program, width.saturating_sub(columns(&note)))
    )
}

/// A URL shortened in the middle of what follows its host.
fn url_in(origin: &str, rest: &str, width: usize) -> String {
    let room = width.saturating_sub(columns(origin) + columns(ELLIPSIS));
    if room == 0 {
        return start_of(origin, width.saturating_sub(1)) + ELLIPSIS;
    }
    format!("{origin}{ELLIPSIS}{}", end_of(rest, room))
}

/// The longest start of `text` in at most `width` columns.
fn start_of(text: &str, width: usize) -> String {
    let mut out = String::new();
    for character in text.chars() {
        let mut next = out.clone();
        next.push(character);
        if columns(&next) > width {
            break;
        }
        out = next;
    }
    out
}

/// The longest end of `text` in at most `width` columns.
fn end_of(text: &str, width: usize) -> String {
    let mut kept: Vec<char> = Vec::new();
    for character in text.chars().rev() {
        kept.push(character);
        let candidate: String = kept.iter().rev().collect();
        if columns(&candidate) > width {
            kept.pop();
            break;
        }
    }
    kept.iter().rev().collect()
}
