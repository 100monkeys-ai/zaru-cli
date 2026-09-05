// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D3's "one trait", declared here and implemented nowhere.
//!
//! **Nothing in this crate's product tree implements [`Provider`]**, exactly
//! as nothing implements [`LayerSource`](crate::config::LayerSource), the
//! credential store's [`SecretStore`](crate::credentials::SecretStore),
//! [`ManifestSource`](crate::manifest::ManifestSource) or any of `zaru-core`'s
//! loop ports. A check implements it; the product does not, and that is why
//! **no code in this workspace can reach a provider at all**.
//!
//! # Why it is a port rather than three clients
//!
//! [ADR-0003] D2's dependency table names `rmcp`, `ratatui`, `tui-textarea`,
//! `fastembed`, `tokio`, `serde` and `reqwest`. `reqwest` is there and it has
//! **no caller anywhere in this workspace**, so it is not taken: a dependency
//! arrives in the arc that has a caller, and that arc is the one holding a
//! credential and able to prove a call. Three provider clients would also be
//! three streaming implementations and three error taxonomies, which is ADR-
//! 0012's own Negative consequence, and none of them can be written before
//! ADR-0007's provider-credential question is answered — that question is open
//! on [operations/adr-status] and this arc did not answer it.
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
