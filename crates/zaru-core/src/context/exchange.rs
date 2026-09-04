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
