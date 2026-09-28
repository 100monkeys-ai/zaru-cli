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

use crate::conversation::Message;
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

/// One exchange in layer 6: one whole turn of the conversation.
///
/// # A turn is kept whole, and that is what compaction relies on
///
/// An exchange holds every message of one turn — the person's task, each of
/// the model's messages with the calls it asked for, and the result of every
/// call — in order. [`crate::context::Context::compact`] takes whole
/// exchanges, oldest first, so a compaction can never keep a tool call and
/// drop its result, or the reverse: both are in the same exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    kind: ExchangeKind,
    messages: Vec<Message>,
}

impl Exchange {
    /// Record one whole turn, as the messages it was.
    ///
    /// # Why the messages rather than a rendering of them
    ///
    /// ADR-0013 D1 names layer 6 "conversation **and tool results**". Until
    /// 2026-09-28 a turn was kept here as one text: the task, a rendered line
    /// for each tool call, and the answer. That text is what a person was
    /// shown, and a line such as "`cmd.run reported a failure · 1256 bytes`"
    /// tells a model that a command failed and nothing of what it printed. So
    /// the next turn's model could not see what it had read or run one turn
    /// earlier. The messages are what the model was actually sent, and they
    /// are sent again in the roles a provider defines for them.
    #[must_use]
    pub const fn of_turn(messages: Vec<Message>) -> Self {
        Self {
            kind: ExchangeKind::Verbatim,
            messages,
        }
    }

    /// Record a summary that replaced older exchanges.
    ///
    /// A summary is sent as a message from the person's side, because every
    /// provider requires the conversation to open with one and a summary
    /// replaces the turns that did.
    #[must_use]
    pub fn summary(text: impl Into<String>) -> Self {
        Self {
            kind: ExchangeKind::Summary,
            messages: vec![Message::User { text: text.into() }],
        }
    }

    /// The exchange's messages, in order.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// The exchange as one text, for counting.
    ///
    /// Each message's [`Message::rendered`], separated by the blank line every
    /// other join in this module uses. Not what a provider is sent.
    #[must_use]
    pub fn rendered(&self) -> String {
        let mut text = String::new();
        for message in &self.messages {
            let part = message.rendered();
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push_str(crate::context::prefix::SEPARATOR);
            }
            text.push_str(&part);
        }
        text
    }

    /// Whether this is what happened or a summary of it.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }
}
