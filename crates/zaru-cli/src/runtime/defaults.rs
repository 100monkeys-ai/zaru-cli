// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0001] D3's iteration defaults: **the one source of truth in code.**
//!
//! # The record asked for this in as many words
//!
//! D3's Negative consequence: "Per-tier, per-provider iteration defaults are a
//! two-dimensional table that has to stay consistent across the CLI, the docs,
//! and the system prompt. **It will drift unless there is one source of truth
//! in code.**" [`iterations`] is that source, and D3's twelve cells are
//! transcribed into it and nowhere else in this workspace.
//!
//! D3's table:
//!
//! ```text
//! | Tier        | Local model         | BYO frontier key     |
//! | bare        | 1                   | 1                    |
//! | contained   | 3                   | 5                    |
//! | linked      | 3 local, 8 offloaded| 5 local, 12 offloaded|
//! ```
//!
//! That is three axes rather than two: the tier, D3's column, and — in the
//! `linked` row — whether the work runs locally or is offloaded.
//!
//! # Every cell is spelled, so nothing arrives by default
//!
//! [`iterations`] matches on the whole triple with **no wildcard arm**, so all
//! twelve cells are written out. A fourth [`Tier`], a third [`Inference`] or a
//! third [`Placement`] is a compile error rather than a value silently taking
//! a neighbour's number.
//!
//! # The unavailable cells are checked against D1, not derived from it
//!
//! Eight cells hold a number and four hold nothing: nothing offloads at `bare`
//! or at `contained`, which is [ADR-0001] D1's Loop column — "none", "local",
//! "local, offloadable". Those four arms return [`None`] and they are
//! **written out here** rather than computed from
//! [`Engagement::r#loop`](super::Engagement).
//!
//! That is deliberate. Deriving one table from the other would make them one
//! source, and a check comparing them would be a mirror with a verdict
//! attached ([Verification lessons] §11). Written twice from the record's two
//! tables, they are two transcriptions that a check can hold against each
//! other, and the mutant that changes either one reddens it.
//!
//! # `bare` is one by definition, and the number is not chosen here
//!
//! D3: "`bare` has no loop, so its iteration count is one by definition —
//! there is no validator to refine against." That is why both of `bare`'s
//! cells are 1, and it is the only cell in the table with a stated derivation.
//!
//! # The axis question, decided
//!
//! D3's columns are "Local model" and "BYO frontier key". [ADR-0012] D3's
//! provider kinds are `anthropic`, `openai-compatible`, `ollama` and `aegis`,
//! and **they are not the same axis**: `openai-compatible` covers a local vLLM
//! and a hosted gateway alike, so no mapping exists between the two without
//! inventing one.
//!
//! Under Jeshua's directive of 2026-09-05 the axis is decided rather than
//! inferred: [`Inference`] is **declared per alias in configuration**, at
//! `inference.<alias>` — a sibling of `model.<alias>` rather than a child, so
//! that one key is never both a value and a table. [`Placement`] is
//! [`Placement::Local`] unless ADR-0012 D3's `aegis` kind is the resolved
//! provider kind. **Both types are [`crate::providers`]', re-exported here**;
//! this module indexes D3's table by them and declares neither, so there is
//! one spelling of each and one place that decides what a provider kind
//! implies.
//!
//! # Nothing here names a tier to `zaru-core`
//!
//! [`ceiling`] hands back `zaru-core`'s own
//! [`Ceiling`], which is a number and carries
//! no tier. [ADR-0008] D5 says the loop "takes one and never chooses one", and
//! its `limits` module says ADR-0001 D3 owns the numbers. This is the boundary
//! where that ownership is exercised.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::runtime::tier::Tier;
use zaru_core::iteration::Ceiling;

/// [ADR-0001] D3's column axis, and where the work runs.
///
/// **Declared once, in [`crate::providers`], and re-exported here.** Both this
/// module and that one need them: D3's table is indexed by them and ADR-0012's
/// resolution produces them. This module declared its own until the
/// `provider-aliases` arc landed on 2026-09-05, at which point there were two
/// of each — the one-rule-in-two-places shape [Verification lessons] §27 names
/// and the very thing this module's own header records `Tier` moving to avoid.
///
/// The declarations that stay are `providers`', because they are typed over
/// [`ProviderKind`](crate::providers::ProviderKind) — `Inference::of` and
/// `Placement::of` take a resolved kind, which is stronger than the text this
/// module was matching, and `Inference::key` takes a
/// [`ModelAlias`](crate::providers::ModelAlias) rather than any string. This is
/// a **delegated coordinator ruling of 2026-09-05**, the same shape as `Tier`'s
/// and `Layer`'s, recorded on ADR-0001's Status tracking and open to Jeshua's
/// veto.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub use crate::providers::{Inference, InferenceRefused, Placement};

/// [ADR-0001] D3's iteration default, as a plain count.
///
/// `None` where the tier cannot offload, which is four of the twelve cells.
/// See the module documentation for why those four are written out rather than
/// derived from D1's Loop column.
///
/// **Every cell is spelled and there is no wildcard arm.**
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[must_use]
pub const fn iterations(tier: Tier, inference: Inference, placement: Placement) -> Option<u32> {
    match (tier, inference, placement) {
        // `bare` — D3: "no loop, so its iteration count is one by definition".
        (Tier::Bare, Inference::Local, Placement::Local)
        | (Tier::Bare, Inference::Frontier, Placement::Local) => Some(1),

        // `contained` — 3 on a local model, 5 on a frontier key.
        (Tier::Contained, Inference::Local, Placement::Local) => Some(3),
        (Tier::Contained, Inference::Frontier, Placement::Local) => Some(5),

        // `linked` — "3 local, 8 offloaded" and "5 local, 12 offloaded".
        (Tier::Linked, Inference::Local, Placement::Local) => Some(3),
        (Tier::Linked, Inference::Local, Placement::Offloaded) => Some(8),
        (Tier::Linked, Inference::Frontier, Placement::Local) => Some(5),
        (Tier::Linked, Inference::Frontier, Placement::Offloaded) => Some(12),

        // Nothing offloads below `linked`. D1's Loop column gives `bare`
        // "none" and `contained` "local"; only `linked` is "local,
        // offloadable".
        (Tier::Bare, Inference::Local, Placement::Offloaded)
        | (Tier::Bare, Inference::Frontier, Placement::Offloaded)
        | (Tier::Contained, Inference::Local, Placement::Offloaded)
        | (Tier::Contained, Inference::Frontier, Placement::Offloaded) => None,
    }
}

/// The same cell, as the bound `zaru-core`'s loop takes.
///
/// This is the boundary [ADR-0008] D5 describes from the other side: the loop
/// "takes one and never chooses one", and the number comes from ADR-0001 D3.
/// **`zaru-core` is handed a count and never a tier.**
///
/// `None` for the four cells [`iterations`] has none for. Every number D3
/// prints is at least one, so no cell of this table can produce the zero
/// `Ceiling::new` refuses — which is asserted rather than assumed.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[must_use]
pub fn ceiling(tier: Tier, inference: Inference, placement: Placement) -> Option<Ceiling> {
    iterations(tier, inference, placement).map(|count| {
        Ceiling::new(count).expect("ADR-0001 D3 prints no ceiling of zero; see the check")
    })
}
