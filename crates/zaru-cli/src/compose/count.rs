// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the harness measures a context against, which is bytes and is not
//! tokens.
//!
//! # This is not a tokeniser, and the name is the whole of the honesty
//!
//! [ADR-0013] measures a context in tokens: D3's announcement carries "real
//! before-and-after counts", D6's status line carries usage, and D7 refuses an
//! iteration whose assembly would exceed the window. `zaru-core` takes every
//! one of those numbers through
//! [`TokenCounter`](zaru_core::context::TokenCounter) rather than computing
//! one, because **no tokeniser is in [ADR-0003] D2's table** — `fastembed`,
//! the only local model that table admits, is an embedder — and "a count
//! invented by the thing being counted is not a measurement".
//!
//! Adding a tokeniser is an amendment to D2 and a dependency, and it is the
//! open question `operations/adr-status` already carries, raised by the
//! composer arc when [ADR-0005] D5's staged token cost needed the same thing.
//! This arc has no measurement to offer for that amendment and does not make
//! it.
//!
//! So the composition measures **bytes**, and says so in the type's name
//! rather than in a comment somebody may not read. A byte count is a real
//! measurement of a real quantity; what it is not is the quantity ADR-0013's
//! clauses are written in.
//!
//! # It over-counts, and the direction is the reason it is safe
//!
//! Every tokeniser in use maps one token onto one or more bytes, so for any
//! text `bytes >= tokens`. Two consequences, and they are opposite:
//!
//! - **The refusal is sound.** [`Context::assemble`](zaru_core::context::Context::assemble)
//!   refuses when the count exceeds the window, and `bytes > window` implies
//!   `tokens > window` — so nothing this counter admits would have overflowed
//!   the provider's window on a real count. It never lets a prompt through
//!   that would not have fitted.
//! - **The refusal is early.** The converse does not hold: a prompt whose
//!   bytes exceed the window while its tokens would not is refused here and
//!   would have been accepted by the provider. On a boundary whose failure
//!   mode is a provider rejecting a whole turn, refusing early is the
//!   direction to be wrong in, and it is stated rather than left for a user
//!   to discover from behaviour.
//!
//! The margin is bounded and is worth stating too: [`CONTEXT_WINDOW_TOKENS`]
//! is 1,048,576 and English prose runs near four bytes to the token, so a
//! turn is refused somewhere around a quarter of the window it could have
//! used. That is a real cost, and it is the cost of not choosing a dependency
//! this arc has no caller-measurement for.
//!
//! Recorded as an accepted Update on [ADR-0013] under directive 20 of
//! 2026-09-05, open to Jeshua's veto.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [`CONTEXT_WINDOW_TOKENS`]: crate::cli::layers::CONTEXT_WINDOW_TOKENS

use zaru_core::context::TokenCounter;

/// Counts a context in bytes, against a window stated in tokens.
///
/// See the module documentation: this is an over-count, deliberately, and it
/// is named for what it measures rather than for what it stands in for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ByteCounter;

impl TokenCounter for ByteCounter {
    /// The text's length in bytes.
    ///
    /// `len()` rather than `chars().count()`: a character count would be a
    /// *different* wrong number, and one that can under-count against a
    /// tokeniser for text outside the basic multilingual plane — which would
    /// lose the soundness the module documentation is built on.
    fn count(&self, text: &str) -> u64 {
        text.len() as u64
    }
}
