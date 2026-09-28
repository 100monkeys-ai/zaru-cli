// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a person is shown of one call's result.
//!
//! # Why this exists
//!
//! Until 2026-09-28 a person saw that a call ran, its exit and a byte count,
//! and never what it printed or what it changed. They approved a call and
//! then could not see what came of it, so they could not follow the work or
//! catch a mistake. This is what the executing surface composes for them: a
//! line saying what the call did, and for a command or a change, the rows
//! that show it.
//!
//! # It is for the person and never for the model
//!
//! It travels on [`ToolOutcome::Completed`](crate::tool_call::ToolOutcome)
//! and on [`Event::ToolShown`](crate::tool_call::Event), and on no
//! [`Message`](crate::conversation::Message). The conversation a model is
//! sent is built from messages alone, so nothing here reaches a model as if
//! someone had said it.
//!
//! # What is in it has passed the redactor
//!
//! The surface that composes it redacts the text first and makes every
//! control character visible, because this is drawn on a terminal and kept in
//! the transcript. This crate cannot check either and does not claim to: it
//! holds text somebody else made safe, the way
//! [`ToolDecision`](crate::tool_call::ToolDecision) holds a sentence somebody
//! else composed.

use serde::{Deserialize, Serialize};

/// What a person is shown of one call's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultView {
    /// One line saying what the call did or returned: which lines of which
    /// file, how many matches, a command's exit and how much it printed.
    pub summary: String,
    /// The rows under it, in order: a command's last lines, or an edit's
    /// changed lines with a few lines around them. Empty for a call a line
    /// says enough about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<ViewRow>,
}

/// One row of a [`ResultView`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewRow {
    /// What kind of row this is. The renderer marks it by this, with a
    /// character, so it never depends on colour.
    pub mark: Mark,
    /// The line's number in its file, for a row of a change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<usize>,
    /// The row's text: one line, with no line break in it.
    pub text: String,
}

/// What kind of row a [`ViewRow`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    /// A line a command wrote to standard output.
    Output,
    /// A line a command wrote to standard error.
    Error,
    /// A line a change removed.
    Removed,
    /// A line a change added.
    Added,
    /// A line near a change that the change left as it was.
    Context,
    /// A break between two parts of a change that are far apart in the file.
    Gap,
    /// A line about the rows rather than one of them: how many are not
    /// shown and where the whole is kept, or that a command printed nothing.
    Note,
}

impl ViewRow {
    /// A row with no line number.
    #[must_use]
    pub fn new(mark: Mark, text: impl Into<String>) -> Self {
        Self {
            mark,
            number: None,
            text: text.into(),
        }
    }

    /// A row of a change, with the line's number in its file.
    #[must_use]
    pub fn numbered(mark: Mark, number: usize, text: impl Into<String>) -> Self {
        Self {
            mark,
            number: Some(number),
            text: text.into(),
        }
    }
}
