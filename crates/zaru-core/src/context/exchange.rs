// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The unit of layer 6: one conversational exchange.
//!
//! ADR-0013 D3's announcement counts these and calls them turns. This module
//! calls them exchanges, because [Ubiquitous Language] reserves *turn* as an
//! anti-term for an iteration and a type named `Turn` in a crate that already
//! has [`crate::iteration::Turn`] would make the two indistinguishable at
//! every call site. The word survives where D3 put it: on the announcement's
//! field, which is what a renderer prints.
//!
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

use serde::{Deserialize, Serialize};

/// Whether an exchange is what happened or a summary of what happened.
///
/// Informational rather than load-bearing. ADR-0013 D2 replaces the oldest
/// span with a summary and says nothing about summaries being exempt from a
/// later compaction — and exempting them would eventually leave compaction
/// with nothing it is allowed to free, which routes straight to D7's
/// exhaustion. So a summary is compacted like anything else, oldest first,
/// and this kind exists for a renderer that wants to mark one rather than for
/// the selection to filter on. Ruled 2026-09-04 as a delegated coordinator
/// reading and recorded on ADR-0013 as an open question the record's author
/// can reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeKind {
    /// What was actually said, as it was said.
    Verbatim,
    /// A summary that replaced a span of older exchanges.
    Summary,
}

/// One exchange in layer 6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    text: String,
    kind: ExchangeKind,
}

impl Exchange {
    /// Record what was said.
    #[must_use]
    pub fn verbatim(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ExchangeKind::Verbatim,
        }
    }

    /// Record a summary that replaced older exchanges.
    #[must_use]
    pub fn summary(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ExchangeKind::Summary,
        }
    }

    /// Record one whole turn: what was asked, what the tools returned, and
    /// what the model answered.
    ///
    /// # Three parts, because ADR-0013 D1's layer 6 is three things
    ///
    /// D1 names layer 6 "conversation **and tool results**", and a turn is
    /// all of it: the task the user typed, the rendered line of every tool
    /// call the model made on the way, and the answer it ended with. An
    /// exchange holding only the first and the last would drop the middle,
    /// and the middle is where a coding session's facts are — the file that
    /// was read, the command that failed.
    ///
    /// # They are composed into one text rather than kept apart
    ///
    /// Every consumer of layer 6 wants the whole of it: [`Self::as_str`]
    /// feeds the render, the token count and the summarisation, and none of
    /// them has a use for the parts separately. Keeping three fields *and* a
    /// rendered whole would be the same content twice, which is how the two
    /// drift; keeping only the parts would make `as_str` allocate on every
    /// count. What needs the parts separately is [ADR-0010] D2's transcript,
    /// and that already holds each of them in its own record, at full
    /// fidelity, written as it happened.
    ///
    /// The separator is the blank line every other join in this crate's
    /// context module uses, so a reader of an assembled context meets one
    /// convention rather than two. An empty part
    /// contributes nothing rather than a blank stretch — a turn with no tool
    /// calls is the ordinary case, not a turn with an empty tool section.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn of_turn(task: &str, tool_results: &[String], answer: &str) -> Self {
        let mut text = String::new();
        for part in core::iter::once(task)
            .chain(tool_results.iter().map(String::as_str))
            .chain(core::iter::once(answer))
            .filter(|part| !part.is_empty())
        {
            if !text.is_empty() {
                text.push_str(crate::context::prefix::SEPARATOR);
            }
            text.push_str(part);
        }
        Self {
            text,
            kind: ExchangeKind::Verbatim,
        }
    }

    /// The exchange's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether this is what happened or a summary of it.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }
}
