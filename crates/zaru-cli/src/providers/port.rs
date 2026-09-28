// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D3's "one trait", and the two kinds that implement it.
//!
//! **This module's own documentation said the opposite until 2026-09-14, and
//! the sentences are corrected rather than left.** It read "**Nothing in this
//! crate's product tree implements [`Provider`]**" and "**no code in this
//! workspace can reach a provider at all**". Both were written on 2026-09-04,
//! when they were exactly true, and both were falsified on 2026-09-05 by the
//! `provider-client` arc — the day [`GeminiClient`](crate::providers::GeminiClient)
//! implemented this trait and `zaru <task>` stopped being refused at exit 4.
//! A doc comment that denies the existence of the type three modules down is
//! worse than no comment, because a reader who believes it looks for a seam
//! that is already built.
//!
//! # Two kinds implement it, and three do not
//!
//! [`GeminiClient`](crate::providers::GeminiClient) since 2026-09-05 and
//! [`OllamaClient`](crate::providers::OllamaClient) since 2026-09-14.
//! `anthropic`, `openai-compatible` and `aegis` have no client: D3's own
//! Negative consequence — "each addition is a maintenance surface with its own
//! streaming quirks and error taxonomy" — is why none was written blind, and
//! the two that exist were each written against a real endpoint.
//!
//! # Why it is a port rather than a client
//!
//! [ADR-0003] D2's dependency table names `rmcp`, `ratatui`, `ratatui-textarea`,
//! `fastembed`, `tokio`, `serde` and `reqwest`. Five provider clients would be
//! five streaming implementations and five error taxonomies, which is
//! ADR-0012's own Negative consequence. **What the two built so far show is
//! that the taxonomies genuinely differ rather than merely might**: `gemini`
//! classifies an unreachable endpoint as environmental because a hosted outage
//! is nobody's to fix, and `ollama` classifies the same shape as
//! user-correctable because a local server is the user's to start. One trait
//! over two clients is what lets both be true at once.
//!
//! # What D3's four concerns are, and where each one is
//!
//! D3: "The trait carries streaming, tool calling, token accounting, and a
//! capability descriptor."
//!
//! | Concern | Here |
//! | --- | --- |
//! | capability descriptor | [`Provider::capabilities`] |
//! | streaming | [`ProviderCapabilities::streaming`], which is what a provider *says*; there is nothing to stream |
//! | tool calling | [`ProviderCapabilities::tool_calling`], refused at configuration time by [`require_tool_calling`](ProviderCapabilities::require_tool_calling) |
//! | token accounting | [`Provider::usage`], paired with the descriptor's own flag |
//!
//! **There is no request method, and that is a boundary rather than an
//! omission.** A prompt-in, response-out shape is the tool-call loop's `Model`
//! port in `zaru-core`, and declaring a second one here would be two
//! statements of what a model exchange is — the rule-in-two-places that
//! [`Layer`](crate::config::Layer) was de-duplicated to remove from this
//! workspace on 2026-09-04. What this trait carries is what a provider is
//! *configured* to be, which is [`crate::providers`]'s half.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status

use crate::providers::capability::ProviderCapabilities;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::kind::ProviderKind;
use crate::providers::usage::TokenUsage;

/// A configured provider, per [ADR-0012] D3.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub trait Provider {
    /// Which of D3's four kinds this is.
    fn kind(&self) -> ProviderKind;

    /// Where it is reached.
    ///
    /// The same question for every kind, which is D5 — see
    /// [`ProviderEndpoint`].
    fn endpoint(&self) -> &ProviderEndpoint;

    /// What it says it can do.
    ///
    /// D3 calls this "load-bearing", and it is consulted at configuration time
    /// rather than mid-loop.
    fn capabilities(&self) -> ProviderCapabilities;

    /// What the last request cost, where this provider accounts at all.
    ///
    /// **This and [`ProviderCapabilities::token_accounting`] are two halves of
    /// one statement, and an implementation owes both.** A provider whose
    /// descriptor says it does not account must answer `None` here, and one
    /// that says it does must answer `Some`. The obligation is stated rather
    /// than enforced, because enforcing it would mean this crate wrapping
    /// every implementation — and there are none to wrap; what holds it today
    /// is a check over the implementations that exist, which are a check's own.
    fn usage(&self) -> Option<TokenUsage>;
}
