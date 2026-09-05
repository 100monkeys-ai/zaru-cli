// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! How large a compiled pattern may be before it is refused.
//!
//! # The number is nobody's to invent here, and the crate asked for one
//!
//! [ADR-0009] D3 names no bound and neither does any other record, so the
//! ceiling arrives from the caller and is refused at zero — the shape
//! [`ProcessCeiling`](crate::process::ProcessCeiling) and
//! [`SizeCeiling`](crate::config::SizeCeiling) already use in this crate.
//!
//! **That there is a ceiling at all is the engine's own advice.** `regex`'s
//! crate documentation, under "Untrusted patterns": *"Advice for those using
//! untrusted regexes: limit the pattern length to something small and expand
//! it as needed. Configure `RegexBuilder::size_limit` to something small and
//! then expand it as needed."* A validator's pattern is exactly an untrusted
//! regex — it arrives in a file from a repository the user cloned. The
//! *search* is already bounded by that crate's `O(m * n)` guarantee; what this
//! bounds is `m`, which counted repetitions expand: `a{1000}{1000}{1000}` is
//! nineteen characters and a billion states.
//!
//! # Why the name is not `Ceiling`
//!
//! For [`ProcessCeiling`](crate::process::ProcessCeiling)'s reason: `Ceiling`
//! is already [ADR-0001] D3's iteration ceiling in `zaru-core`, and a third
//! bare `Ceiling` in one workspace is the collision
//! [Ubiquitous Language](https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language)
//! records other types as having been renamed to avoid.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators

use core::fmt;

/// A ceiling the caller passed that cannot bound anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternCeilingIsZero;

impl fmt::Display for PatternCeilingIsZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a compiled-pattern ceiling of zero is refused; every pattern compiles to at least \
             one state, so a ceiling of zero refuses every pattern a project could declare and \
             is a way of turning the `matches` kind off rather than of bounding it",
        )
    }
}

impl std::error::Error for PatternCeilingIsZero {}

/// How large a compiled pattern may be, in bytes of compiled program.
///
/// **No default.** See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PatternCeiling(usize);

impl PatternCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`PatternCeilingIsZero`] when `bytes` is zero.
    pub const fn new(bytes: usize) -> Result<Self, PatternCeilingIsZero> {
        if bytes == 0 {
            return Err(PatternCeilingIsZero);
        }
        Ok(Self(bytes))
    }

    /// The ceiling, in bytes.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}
