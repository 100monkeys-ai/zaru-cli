// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! How long a child process may run before the harness kills it.
//!
//! # The number is nobody's to invent, and this module carries none
//!
//! **No record names a wall-clock bound for a command**, so a ceiling
//! invented by the thing being bounded is not a ceiling. It arrives from the
//! caller and is refused at zero, which is the shape
//! [`OutputBudget`](crate::tools::OutputBudget), `zaru-core`'s
//! `TruncationBudget` and [`Ttl`](crate::credentials::Ttl) already use in this
//! workspace: a value refused at a boundary, carried thereafter as evidence
//! that the boundary was crossed.
//!
//! Where the numbers come from when the binary is wired is the composition's,
//! out of [ADR-0014]'s layers — and **no configuration key is declared here**,
//! because that record's Neutral consequence leaves each record its own keys
//! and [ADR-0011] names none for this.
//!
//! # Why the name is not `Ceiling`
//!
//! `Ceiling` is already [ADR-0001] D3's iteration ceiling in `zaru-core` and
//! `ToolCallCeiling` is the outer loop's, so a third bare `Ceiling` in one
//! workspace is exactly the collision
//! [Ubiquitous Language](https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language)
//! records `TokenUsage`, `ProviderEndpoint` and `ProviderCapabilities` as
//! having been renamed to avoid.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use core::fmt;
use core::time::Duration;

/// A ceiling the caller passed that cannot bound anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingIsZero;

impl fmt::Display for CeilingIsZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a wall-clock ceiling of zero is refused; a command given no time at all is one that \
             is killed before it can report anything, so what a caller would be shown carries \
             nothing of what happened",
        )
    }
}

impl std::error::Error for CeilingIsZero {}

/// How long a child process may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessCeiling(Duration);

impl ProcessCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`CeilingIsZero`] when `wall_clock` is zero.
    pub const fn new(wall_clock: Duration) -> Result<Self, CeilingIsZero> {
        if wall_clock.is_zero() {
            return Err(CeilingIsZero);
        }
        Ok(Self(wall_clock))
    }

    /// The ceiling.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl fmt::Display for ProcessCeiling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
