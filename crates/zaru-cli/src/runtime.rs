// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0001]'s runtime tiers: the axis, what each tier engages, how one is
//! resolved, and the iteration ceilings that follow from it.
//!
//! # Why this module exists rather than the type staying where it was
//!
//! [`Tier`] was declared in [`crate::tools::mode`] because the tool surface
//! needed it first — [ADR-0011] D2's enforcement differs per tier — and that
//! left one file holding three records' rules: ADR-0001's tier, ADR-0011's
//! permission mode, and [ADR-0014] D1's layers. **A rule that lives in two
//! places diverges**, and this one already had: `runtime.tier` was declared
//! three times across the tree on 2026-09-04, with two different reasons for
//! why a project may not set it.
//!
//! So the tier lives here, with the record that owns it, and `tools::mode`
//! re-exports it — the same shape that module already uses for
//! [`Layer`](crate::config::Layer), under the delegated ruling of 2026-09-04
//! that inside one crate a rule lives in one place. Nothing that imported
//! `crate::tools::Tier` had to change.
//!
//! # What is built, and what waits
//!
//! | ADR-0001 | Built here |
//! | --- | --- |
//! | D1 — the axis, three tiers, four columns | yes, as one exhaustive derivation per tier |
//! | D2 — the config key, immutability for a session | the key and the resolution; immutability as a type with no mutation surface |
//! | D2 — `--runtime`, the status line, `/runtime` | **no**; the datum they render, with no renderer |
//! | D3 — per-tier, per-provider iteration defaults | yes, as the one source of truth in code |
//! | D4 — onboarding leads with sovereignty | **no**; it is user-facing prose, not code |
//!
//! **A fourth tier fails to compile in two places**: [`Tier::ALL`]'s annotated
//! length, and the wildcard-free match in [`Tier::engagement`]. D3's table adds
//! a third, because every one of its twelve cells is spelled.
//!
//! # Nothing here renders and nothing here is a command
//!
//! D2 puts the tier in a status line and behind `/runtime`. The status line is
//! `zaru-tui`'s and [ADR-0015] D2 owns the command namespace; neither exists.
//! What is here is [`Runtime`], the datum both would read, with no `Display`
//! that prints anywhere and no flag. `--runtime` additionally needs the
//! argument parser [ADR-0003] D2 leaves undecided.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

pub mod defaults;
pub mod resolve;
pub mod tier;

pub use defaults::{Inference, InferenceRefused, Placement, ceiling, iterations};
pub use resolve::{KEY, PROJECT_REFUSAL, ResolvedTier, TierRefused, field, key};
pub use tier::{Cortex, Engagement, Loop, Membrane, Network, Tier};

#[cfg(test)]
mod tests;
