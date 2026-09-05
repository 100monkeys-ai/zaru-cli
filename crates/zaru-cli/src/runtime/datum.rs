// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0001] D2's status-line and `/runtime` datum: the tier, what it
//! engages, and what changing it would alter.
//!
//! # D2 names two surfaces and this module is neither of them
//!
//! D2: "Status line renders the tier at all times... `/runtime` prints the
//! current tier and **what changing it would alter**." The status line belongs
//! to `zaru-tui`, which has no such surface; `/runtime` is
//! [ADR-0015] D2's command namespace, which does not exist; and `--runtime`
//! additionally needs the argument parser [ADR-0003] D2 leaves undecided.
//!
//! What is here is the **datum** both would read. [`Runtime`] has no `Display`
//! that prints anywhere, no `println!`, no flag and no command — the same
//! shape [`Explanation`](crate::config::Explanation) takes for ADR-0014 D3's
//! explain block, and for the same reason: where the block goes is the
//! caller's to decide.
//!
//! # "What changing it would alter" is a diff, not a sentence
//!
//! D2 does not say what that phrase means and it would be easy to answer with
//! prose. The answer here is arithmetic over [ADR-0001] D1's own table: for
//! each *other* tier, which of D1's four columns differ and what they differ
//! to. A user asking "what do I get if I move" is answered with the record's
//! own cells rather than with a paraphrase somebody wrote, and a change to D1
//! moves the answer without anybody remembering to.
//!
//! It is also why this is data. A rendered sentence would have to be composed
//! somewhere, and the place it was composed would become a second statement of
//! D1's table.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::Layer;
use crate::runtime::resolve::ResolvedTier;
use crate::runtime::tier::{Engagement, Tier};

/// One column of [ADR-0001] D1's table that would change, and what to.
///
/// Three fields and no fourth. A check destructures it exhaustively, so a
/// column added to D1 stops that check compiling.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    /// D1's column heading — `"Membrane"`, `"Loop"`, `"Cortex"`, `"Network"`.
    pub column: &'static str,
    /// The cell at the tier the session is at, in D1's own wording.
    pub here: &'static str,
    /// The cell at the tier being compared with, in D1's own wording.
    pub there: &'static str,
}

/// What [ADR-0001] D2's status line renders and its `/runtime` prints.
///
/// Built from a [`ResolvedTier`], so a datum cannot exist for a tier no
/// session resolved.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    /// The tier this session is at. **The status line renders this at all
    /// times**, per D2, and a renderer that has this cannot fail to.
    pub tier: Tier,
    /// Which of ADR-0014 D1's layers supplied it.
    ///
    /// Not asked for by D2, and carried because a user who cannot see where
    /// their tier came from is in exactly the position ADR-0014 D3 exists to
    /// get them out of.
    pub supplied_by: Layer,
    /// D1's whole row for [`Runtime::tier`].
    pub engagement: Engagement,
    /// D2's "what changing it would alter", per other tier, in
    /// [`Tier::ALL`]'s order.
    ///
    /// Every other tier appears, including one that differs in no column at
    /// all — which would render as "nothing would change", and is a different
    /// answer from a tier that is missing from the list.
    pub would_change: Vec<(Tier, Vec<Difference>)>,
}

impl Runtime {
    /// The datum for a resolved tier.
    #[must_use]
    pub fn of(resolved: ResolvedTier) -> Self {
        let tier = resolved.tier();
        let would_change = Tier::ALL
            .into_iter()
            .filter(|other| *other != tier)
            .map(|other| (other, differences(tier, other)))
            .collect();

        Self {
            tier,
            supplied_by: resolved.supplied_by(),
            engagement: tier.engagement(),
            would_change,
        }
    }

    /// What changing to one particular tier would alter.
    ///
    /// Reads [`Runtime::would_change`] rather than recomputing, so there is
    /// one answer rather than two that can disagree.
    #[must_use]
    pub fn would_change_to(&self, other: Tier) -> Option<&[Difference]> {
        self.would_change
            .iter()
            .find(|(candidate, _)| *candidate == other)
            .map(|(_, differences)| differences.as_slice())
    }
}

/// Which of D1's four columns differ between two tiers.
///
/// The four comparisons are written out rather than looped, because
/// [`Engagement`]'s fields have four different types and a loop over them
/// would need a fifth representation of the table to iterate.
fn differences(here: Tier, there: Tier) -> Vec<Difference> {
    let (a, b) = (here.engagement(), there.engagement());
    let mut found = Vec::new();

    if a.membrane != b.membrane {
        found.push(Difference {
            column: "Membrane",
            here: a.membrane.as_str(),
            there: b.membrane.as_str(),
        });
    }
    if a.r#loop != b.r#loop {
        found.push(Difference {
            column: "Loop",
            here: a.r#loop.as_str(),
            there: b.r#loop.as_str(),
        });
    }
    if a.cortex != b.cortex {
        found.push(Difference {
            column: "Cortex",
            here: a.cortex.as_str(),
            there: b.cortex.as_str(),
        });
    }
    if a.network != b.network {
        found.push(Difference {
            column: "Network",
            here: a.network.as_str(),
            there: b.network.as_str(),
        });
    }

    found
}
