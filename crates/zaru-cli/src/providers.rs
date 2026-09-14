// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012]'s provider abstraction: the fixed alias set, the four provider
//! kinds, and where a model identifier is allowed to exist.
//!
//! # What this module owns, and what it deliberately does not
//!
//! It owns the *invariant* half of ADR-0012 — the vocabulary, the resolution
//! as data, the capability descriptor, the disagreement and the accounting —
//! and, since 2026-09-05, **one implementation of D3's trait**: [`gemini`],
//! the first thing in this workspace that can reach a model at all.
//!
//! The sentences that stood here until then said no provider is called from
//! anywhere in this workspace and that the provider trait has no
//! implementation in any product tree. Both were true and neither is, and
//! they are rewritten rather than qualified. What is still true is narrower
//! and worth saying exactly: **four of D3's five kinds have no client**, and
//! nothing here is wired to a loop — `zaru <task>` runs no task.
//!
//! `reqwest` arrived with that client, which is what ADR-0003 clause 7 means
//! by a dependency having a caller; the table named it from the start and
//! this workspace had nobody to use it.
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
//! | D7 — cost and tokens are always visible | the datum, and since 2026-09-05 the row it goes on: `cli::render::usage` composes the line and `zaru_tui::shell::Status` carries it |
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod alias;
pub mod capability;
pub mod client;
pub mod endpoint;
pub mod gemini;
pub mod inference;
pub mod kind;
pub mod negotiation;
pub mod ollama;
pub mod port;
pub mod resolution;
pub mod selection;
pub mod usage;

pub use alias::ModelAlias;
pub use capability::{CapabilityRefused, ProviderCapabilities};
pub use client::{ProviderClient, ProviderFailure};
pub use endpoint::{EndpointRefused, ProviderEndpoint};
pub use gemini::{GeminiClient, GeminiFailure};
pub use inference::{Inference, InferenceRefused, Placement};
pub use kind::ProviderKind;
pub use negotiation::{
    AliasNegotiation, Disagreement, NegotiationFailure, RemoteModelId, disagreements,
};
pub use ollama::{OllamaClient, OllamaFailure};
pub use port::Provider;
pub use resolution::{
    ModelId, ModelIdRefused, ModelTable, ResolvedModel, TableRefused, declare, endpoint_of, fields,
    inference_of,
};
pub use selection::{NoKindSelected, Requirement, kind_key, select};
pub use usage::{Cost, CostRefused, TokenUsage};

#[cfg(test)]
mod tests;
