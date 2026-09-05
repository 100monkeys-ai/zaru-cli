// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What bounds one retrieval: how many bytes, how long, and how many
//! redirects.
//!
//! # None of the three numbers is this module's
//!
//! [ADR-0011] names no size for a body, no wall clock for a fetch and no
//! redirect count, so every one of them arrives from the caller — the shape
//! [`OutputBudget`](crate::tools::OutputBudget),
//! [`ProcessCeiling`](crate::process::ProcessCeiling),
//! [`SizeCeiling`](crate::config::SizeCeiling) and
//! [`Ttl`](crate::credentials::Ttl) already use in this workspace. **No
//! configuration key is declared here**, because [ADR-0014]'s Neutral
//! consequence leaves each record its own keys and this one names none. The
//! binary's values are [`crate::cli::layers`]'s, in one place, where a check
//! can read what the binary chose.
//!
//! # Two of the three are refused at zero and one is not
//!
//! Zero bytes and zero time are refused, because a retrieval given neither
//! carries nothing of what happened — the argument [`OutputBudget`] and
//! [`ProcessCeiling`] each make in their own words.
//!
//! **[`RedirectLimit`] admits zero, and the difference is the point.** "Follow
//! no redirect" is a coherent policy a caller might want; refusing it would be
//! a restriction nobody chose, which is the same thing as a number nobody
//! chose. So the type that could most easily have copied its neighbours does
//! not, and says why here rather than leaving a reader to find it by trying.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [`OutputBudget`]: crate::tools::OutputBudget
//! [`ProcessCeiling`]: crate::process::ProcessCeiling

use core::fmt;
use core::time::Duration;

/// A bound the caller passed that cannot bound anything.
///
/// One type for both bounds that refuse zero, carrying which one it was, so a
/// caller that mixed up two arguments is told which it got wrong rather than
/// being told that something was zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundIsZero {
    /// A body ceiling of zero bytes.
    Body,
    /// A timeout of zero.
    Timeout,
}

impl fmt::Display for BoundIsZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Body => f.write_str(
                "a body ceiling of zero is refused; a retrieval allowed no bytes at all is one \
                 that is refused before it can report anything, so what a caller would be shown \
                 carries nothing of what happened",
            ),
            Self::Timeout => f.write_str(
                "a fetch timeout of zero is refused; a retrieval given no time at all is one \
                 that is abandoned before it can report anything, so what a caller would be \
                 shown carries nothing of what happened",
            ),
        }
    }
}

impl std::error::Error for BoundIsZero {}

/// The largest response body a retrieval will accept.
///
/// # Over it is a **refusal**, not a truncation
///
/// This is the one bound on this surface that does not clip. [ADR-0011] D5
/// promises that when output is truncated "the full text \[is\] written to the
/// session directory, with the path shown", and says why: "a truncation the
/// user cannot notice is how a diagnosis gets built on a fragment." A
/// retrieval that stopped reading at this ceiling would hand
/// [`Overflow::preserve`](crate::tools::Overflow::preserve) a body that was
/// **already cut**, so what reached the session would be a prefix and D5's
/// promise would be false for this one tool.
///
/// The tree has met that exact shape twice and answered it the same way both
/// times:
/// [`ThereWasNowhereToKeepTheRest`](crate::tools::PresentationRefused::ThereWasNowhereToKeepTheRest)
/// refuses rather than clipping when the promise cannot be kept, and
/// `fs.search` **names an oversized file as skipped** rather than reading a
/// prefix of it. So a body over this ceiling is refused whole, naming the
/// ceiling, and **D5's budget stays the only truncation on this path** — one
/// truncation rule, in one place.
///
/// A delegated coordinator ruling of 2026-09-05 under Jeshua's directive of
/// that day, open to his veto, recorded on ADR-0011 D5's Status tracking.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BodyCeiling(u64);

impl BodyCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`BoundIsZero::Body`] when `bytes` is zero.
    pub const fn new(bytes: u64) -> Result<Self, BoundIsZero> {
        if bytes == 0 {
            return Err(BoundIsZero::Body);
        }
        Ok(Self(bytes))
    }

    /// The ceiling in bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for BodyCeiling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} byte(s)", self.0)
    }
}

/// How long one retrieval may take, start to finished body.
///
/// Applied once, where the client is built, and it bounds the whole exchange
/// **including reading the body** — so a server that answers its headers
/// promptly and then dribbles bytes forever is bounded by this and not only by
/// [`BodyCeiling`]. That is the reason it is a whole-request timeout rather
/// than a connect timeout, said here because the difference is invisible at
/// the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FetchTimeout(Duration);

impl FetchTimeout {
    /// Take a timeout from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`BoundIsZero::Timeout`] when `wall_clock` is zero.
    pub const fn new(wall_clock: Duration) -> Result<Self, BoundIsZero> {
        if wall_clock.is_zero() {
            return Err(BoundIsZero::Timeout);
        }
        Ok(Self(wall_clock))
    }

    /// The timeout.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl fmt::Display for FetchTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// How many redirects **within one host** a retrieval will follow.
///
/// A redirect that leaves the host is never followed whatever this says — see
/// [`crate::web::client`] — so this bounds only the case where the server that
/// was asked is the server that answers.
///
/// # Zero is accepted
///
/// See the module documentation. "Follow none" is a policy; zero bytes and
/// zero time are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RedirectLimit(usize);

impl RedirectLimit {
    /// Take a limit from the caller. Zero is a limit.
    #[must_use]
    pub const fn new(hops: usize) -> Self {
        Self(hops)
    }

    /// The limit.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

impl fmt::Display for RedirectLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} redirect(s) within one host", self.0)
    }
}

/// The three bounds together.
///
/// Bundled for the reason [`Executor`](crate::tools::Executor)'s ports are: a
/// constructor taking three unlabelled scalars is a constructor whose argument
/// order is a thing to get wrong, and two of these three are a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchBounds {
    /// The largest body accepted, over which a retrieval is refused.
    pub body: BodyCeiling,
    /// How long one retrieval may take.
    pub timeout: FetchTimeout,
    /// How many within-host redirects are followed.
    pub redirects: RedirectLimit,
}
