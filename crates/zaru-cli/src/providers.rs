// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012]'s provider abstraction: the fixed alias set, the four provider
//! kinds, and where a model identifier is allowed to exist.
//!
//! # What this module owns, and what it deliberately does not
//!
//! It owns the *invariant* half of ADR-0012 — the vocabulary, the resolution
//! as data, the capability descriptor, the disagreement and the accounting.
//! **No provider is called from anywhere in this workspace**, nothing opens a
//! socket, and the provider trait has no implementation in any product tree,
//! for the reason [`crate::config::port`] gives at length about [ADR-0014]'s
//! file layers: a provider client is a dependency and [ADR-0003] D2's table
//! names none. `reqwest` is in that table and has **no caller here**, so it is
//! not taken; a dependency arrives in the arc that has a caller.
//!
//! # The names, and the four they had to avoid
//!
//! [Ubiquitous Language] gives "Alias" to [ADR-0007]: "a token's local name —
//! the handle in CLI, transcript, and tool namespace", which is
//! [`crate::credentials::Alias`]. ADR-0012 uses the same English word for a
//! *model* alias, and a second `Alias` in one crate is exactly the collision
//! that page exists to prevent. So the type here is [`ModelAlias`], and three
//! more names are chosen the same way: `TokenUsage` because
//! `zaru_core::context::Usage` is context *occupancy*; [`ProviderEndpoint`]
//! because `zaru_notes::session::Endpoint` is a *transport port*; and
//! `ProviderCapabilities` because
//! [`Class::Capability`](crate::failure::Class::Capability) is [ADR-0016]'s
//! *error class*. Rows for all five were added to that page on 2026-09-05
//! under a delegated coordinator ruling, open to Jeshua's veto.
//!
//! # What is built, and what waits
//!
//! | ADR-0012 | Built here |
//! | --- | --- |
//! | D1 — configuration names aliases, never models | a model identifier no module but the resolution table can construct |
//! | D2 — the alias set is fixed and shared with AEGIS | the four, closed; **the two sets already differ — see the Update on the record** |
//! | D3 — four provider kinds, one trait | the kinds closed; the trait a port with no implementation |
//! | D4 — resolution is layered and every layer is inspectable | through ADR-0014's own fold and its own explanation, never a second one |
//! | D5 — local endpoints are first-class | as a missing field: [`ProviderEndpoint`] has no local-versus-hosted variant |
//! | D6 — disagreement is surfaced, never reconciled | the value, with reconciliation made a compile error |
//! | D7 — cost and tokens are always visible | the datum; there is no status line to put it in |
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod alias;
pub mod capability;
pub mod endpoint;
pub mod inference;
pub mod kind;
pub mod resolution;

pub use alias::ModelAlias;
pub use capability::{CapabilityRefused, ProviderCapabilities};
pub use endpoint::{EndpointRefused, ProviderEndpoint};
pub use inference::{Inference, InferenceRefused, Placement};
pub use kind::ProviderKind;
pub use resolution::{
    ModelId, ModelIdRefused, ModelTable, ResolvedModel, TableRefused, declare, endpoint_of, fields,
    inference_of,
};

#[cfg(test)]
mod tests;
