// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two ports context assembly calls out through.
//!
//! Both are traits declared here and implemented elsewhere. **Nothing in this
//! crate's product tree implements either**, which is the same property
//! [`crate::iteration::port`] holds and for the same reason: it makes
//! ADR-0008 D2's headless requirement a fact about the code rather than a
//! claim about it.
//!
//! # Why counting is a port
//!
//! ADR-0013 D3's announcement carries "real before-and-after counts" and D6's
//! status line carries usage. Both are token counts, and **no tokeniser is in
//! ADR-0003 D2's table** — `fastembed`, the only local model D2 admits, is an
//! embedder. `operations/adr-status` already carries the open question, raised
//! by the composer arc when ADR-0005 D5's staged token cost needed the same
//! thing. So a count is a measurement this crate takes from a caller through
//! [`TokenCounter`], and adding a tokeniser is an amendment to D2 rather than
//! an import.
//!
//! [`TokenCounter`] is synchronous. Counting is local and pure, and a port
//! returning a future would put a runtime under every check on assembly and
//! make each measurement depend on a poll order — the same argument
//! `zaru-tui`'s `Entries` makes for its own fast tier.
//!
//! # Why summarising is a port, and asynchronous
//!
//! ADR-0013's own Negative consequence: "Summarisation costs a model call at
//! the moment the session is already under pressure". A model call is
//! ADR-0012's, it crosses a network, and its port is asynchronous for the
//! same reason the loop's are.

use crate::context::exchange::Exchange;
use crate::iteration::port::PortFailure;
use core::future::Future;
use serde::{Deserialize, Serialize};

/// Measures how much of the window a piece of text occupies.
pub trait TokenCounter {
    /// How many tokens `text` costs.
    fn count(&self, text: &str) -> u64;
}

/// The span of layer 6 that a compaction replaced.
///
/// ADR-0013 D2: "the raw span stays in the transcript. History is preserved
/// on disk; only the model's view is compacted." This is the value that goes
/// to the transcript. **Writing it is ADR-0010's**, whose D2 makes the
/// transcript append-only and whose D3 keeps it apart from the checkpoint;
/// this crate produces the datum and persists nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    exchanges: Vec<Exchange>,
}

impl Span {
    /// Take a span.
    #[must_use]
    pub const fn new(exchanges: Vec<Exchange>) -> Self {
        Self { exchanges }
    }

    /// The exchanges the span replaced, oldest first, exactly as they were.
    #[must_use]
    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    /// How many exchanges the span covers. This is the number ADR-0013 D3's
    /// announcement calls `turns`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.exchanges.len()
    }

    /// Whether the span covers nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.exchanges.is_empty()
    }
}

/// Turns a span of layer 6 into the summary that replaces it.
///
/// ADR-0012 owns what is behind this. Nothing here detects a bad summary —
/// ADR-0013's own Status tracking names that as an open question and sends it
/// to the backlog, and this port is deliberately not the place it gets
/// settled.
pub trait Summariser {
    /// Summarise one span.
    fn summarise(&self, span: &Span) -> impl Future<Output = Result<String, PortFailure>> + Send;
}
