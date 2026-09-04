// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The configuration hierarchy: [ADR-0014]'s five layers and how they resolve.
//!
//! # What this module owns, and what it deliberately does not
//!
//! ADR-0014's Neutral section draws the line: "**Nothing here specifies the
//! schema. Each record owns its own keys; this one owns how they resolve.**"
//! So this module holds precedence, merging, explanation and the refusals,
//! and it enumerates **no configuration key at all**. The known keys arrive
//! as a [`Schema`] a caller builds — see that type for why writing
//! [ADR-0001]'s `runtime`, [ADR-0009]'s `[project]` or [ADR-0011]'s
//! permission mode here would settle five other records' spellings inside a
//! sixth record's implementation.
//!
//! # Where the credential store is, and is not
//!
//! **The store is not a layer of this hierarchy.** It sits *beside* D1's
//! layer 2 at `~/.zaru/credentials.json`, next to `~/.zaru/config.toml`, and
//! reaching for it as a sixth layer would reopen exactly the hole D4 closes.
//! D4: "Configuration holds a *reference* to a credential, never a
//! credential … the design decision that prevents it is refusing to have a
//! field to put one in."
//!
//! Here that refusal is two things rather than a rule somebody remembers. A
//! key that names a credential holds a
//! [`CredentialRef`], which has one field and it is an
//! [`Alias`](crate::credentials::Alias) — there is no string form and a
//! second field would not compile. And a value shaped like a bearer token is
//! refused at load, by asking [ADR-0007]'s own
//! [`Secret`](crate::credentials::Secret) rather than by a second list of
//! prefixes that could drift from the first.
//!
//! # What is built, and what waits
//!
//! | ADR-0014 | Built here |
//! | --- | --- |
//! | D1 — five layers, higher wins | yes, as an ordered enum with no field a pin could occupy |
//! | D2 — merge per key; arrays replace wholesale | yes |
//! | D3 — every setting is explainable | the explanation, as data; the command surface is [ADR-0015]'s and does not exist |
//! | D4 — secrets never live in config files | yes, both structurally and at load |
//! | D5 — unknown keys are an error naming the nearest match | yes, against a caller-supplied schema |
//! | D6 — project config cannot raise a security posture | the mechanism, over declared policies |
//! | D7 — tier resolved once, immutable after | the invariant half: a resolution with no mutation surface |
//!
//! Layers 2, 3 and 5 are read through [`LayerSource`], which **nothing in
//! this crate implements**: a TOML parser and an argument parser are two
//! dependencies and [ADR-0003] D2's table names neither. Layer 4 needs no
//! dependency and is built. See [`port`] for the whole of that reasoning.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

pub mod credential;
pub mod environment;
pub mod explain;
pub mod home;
pub mod key;
pub mod layer;
pub mod port;
pub mod refusal;
pub mod resolve;
pub mod schema;
pub mod value;

pub use credential::CredentialRef;
pub use explain::{Explanation, ExplanationRow};
pub use home::{HomeFailure, ensure};
pub use key::{Key, KeyRefused};
pub use layer::{Contribution, Layer, Source};
pub use port::{LayerSource, SourceFailure, gather};
pub use refusal::ConfigRefused;
pub use resolve::Resolution;
pub use schema::{CoercionFailure, Field, FieldKind, ProjectPolicy, Schema};
pub use value::{Table, Value};

// `pub(crate)` rather than private, for the reason `credentials::fixtures`
// already is: `crate::failure`'s checks drive the real load to produce
// ADR-0014 D4's refusal and then assert that classifying it publishes
// neither the planted bearer value nor its ASCII core. Staging that load a
// second time beside those checks would be one fixture in two places, which
// is a fixture that diverges.
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
