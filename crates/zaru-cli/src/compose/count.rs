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
//! [`TokenCounter`] rather than computing
//! one, because **no tokeniser is in [ADR-0003] D2's table** — `fastembed`,
//! the only local model that table admits, is an embedder — and "a count
//! invented by the thing being counted is not a measurement".
//!
//! Adding a tokeniser is an amendment to D2 and a dependency, and it is the
//! open question `operations/adr-status-questions` carries, raised by the
//! composer arc when [ADR-0005] D5's staged token cost needed the same thing.
//! **It is still not made**, and as of 2026-09-14 the measurement below is
//! what a person deciding it has to go on.
//!
//! So the composition measures **bytes**, and says so in the type's name
//! rather than in a comment somebody may not read. A byte count is a real
//! measurement of a real quantity; what it is not is the quantity ADR-0013's
//! clauses are written in.
//!
//! # It over-counts the prompt, and until 2026-09-14 that was the whole claim
//!
//! Every tokeniser in use maps one token onto one or more bytes, so for any
//! text `bytes >= tokens`. This module said that made the refusal **sound** —
//! "`bytes > window` implies `tokens > window`, so nothing this counter admits
//! would have overflowed the provider's window on a real count".
//!
//! **That was true of the text it measures and false of the request the
//! window is read against**, and it was measured false on 2026-09-14 from the
//! release binary against a local Ollama through a logging proxy. The first
//! exchange of a session put **1,967 bytes** on the wire, of which **231**
//! were message content and the rest the seven tool declarations, and the
//! provider reported **465** prompt tokens. A count over the prompt alone was
//! 231 against a real 465 — below the provider's own number, by a factor of
//! two, which is the direction that overflows a window in silence.
//!
//! The tool surface is on every request of every exchange, for every kind
//! this workspace speaks to, and it is not in the context: the tools are the
//! loop's, declared per exchange, and putting them into layer 6 would put
//! them into the conversation the model is shown twice.
//!
//! # So the counted quantity is the request, and the claim holds again
//!
//! [`Context::reserved`](zaru_core::context::Context::reserved) carries what
//! a request spends outside the context —
//! [`ProviderClient::tool_surface_bytes`](crate::providers::ProviderClient::tool_surface_bytes),
//! measured through the answering kind's own wire mapping rather than guessed
//! — and it is added to every whole-context measurement and to no single
//! exchange's. With it:
//!
//! - **The refusal is sound again.** What is compared against the window is
//!   the bytes of everything sent, and bytes are at least tokens for all of
//!   it.
//! - **The refusal is early, and by a measured margin.** Whole-request bytes
//!   against the provider's own prompt count came to **4.05** and **3.87**
//!   bytes to the token over a three-turn session against `llama3.2:3b`
//!   (2,715 bytes for 671 tokens; 3,035 for 785). So a window is used to
//!   roughly a quarter before compaction runs.
//!
//! **Early is the honest side to be wrong on**: under-counting overflows the
//! provider's window with nothing said and the oldest of a conversation
//! dropped, and over-counting compacts sooner than it needed to and says so
//! as it happens. A per-kind ratio, or a tokeniser, is a later decision — and
//! it now has these numbers to be decided on rather than none.
//!
//! Recorded as an accepted Update on [ADR-0013] under directive 20 of
//! 2026-09-05, open to Jeshua's veto.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

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
