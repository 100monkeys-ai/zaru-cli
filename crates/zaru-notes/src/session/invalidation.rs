// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0007] D6's three reasons a cached tool scope is stale.
//!
//! # This crate produces the signals; it does not hold the cache
//!
//! D6 puts the cache in the entry: "The harness calls `tools/list` once per
//! token at attach and caches the result in the entry." That entry is
//! `zaru-cli`'s, and this crate may not depend on it. So the contract across
//! the two is:
//!
//! - **`zaru-notes` produces** each of the three signals, from the cause D6
//!   names for it, and asserts that each cause produces its own and no other.
//! - **`zaru-cli` owns the cache** and refreshes it with one `tools/list` per
//!   signal, and asserts that.
//!
//! # Nothing retries blind, and that is structural
//!
//! D6: "Refresh, then surface the failure — never retry blind." There is no
//! retry method on this type, on [`Session`](super::Session), or anywhere in
//! this crate — so a blind retry is not something a caller could write by
//! accident. And [`Invalidation::claimed`] does not consume a failure it
//! declines: it hands the failure back unchanged, so a caller cannot reach a
//! state where a refusal was swallowed by a staleness check.
//!
//! # Where the numbers come from
//!
//! The TTL window and both clock readings are the **caller's**. D6 calls a TTL
//! "a backstop for a missed notification" and gives no number, and `zaru-cli`'s
//! `Ttl` already validates one and refuses zero. Readings are monotonic offsets
//! rather than instants for the reason `zaru-core`'s `Clock` gives: an instant
//! cannot be constructed at a chosen value, and a clock a check cannot set is a
//! clock a check cannot assert on. **This crate reads no clock at all.**
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::session::error::CallRefused;
use core::fmt;
use core::time::Duration;

/// A reason a caller's cached tool scope is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalidation {
    /// The server sent `notifications/tools/list_changed`, unsolicited.
    ///
    /// D6's first and best signal: the server said so.
    ListChanged,
    /// The caller's window elapsed. D6's backstop for a missed notification.
    Expired {
        /// The window the caller chose.
        window: Duration,
        /// How much of the caller's own clock had passed.
        elapsed: Duration,
    },
    /// A tool the caller said its cache claimed was refused by the server.
    ///
    /// Carries the refusal, so that surfacing it needs no second lookup and a
    /// caller that refreshes cannot lose what actually failed.
    Claimed(CallRefused),
}

impl Invalidation {
    /// D6's TTL backstop, over the caller's clock.
    ///
    /// Returns `None` while the window still holds. Both readings are offsets
    /// from the same monotonic origin; a `now` before `cached_at` yields no
    /// elapsed time rather than a panic, because a caller's clock is a caller's
    /// business.
    #[must_use]
    pub fn expired(cached_at: Duration, now: Duration, window: Duration) -> Option<Self> {
        let elapsed = now.saturating_sub(cached_at);
        (elapsed >= window).then_some(Self::Expired { window, elapsed })
    }

    /// D6's refresh-on-refusal rule, over a scope the caller claimed.
    ///
    /// # Errors
    ///
    /// The refusal itself, unchanged, when the tool is not one the caller's
    /// cache claimed — because then the cache said nothing that turned out to
    /// be wrong and the failure is simply a failure.
    ///
    /// # This gate is deliberately wider than D6's, and that is recorded rather than hidden
    ///
    /// D6 fires on "a forbidden or method-not-found result". Method-not-found
    /// is a JSON-RPC code and is read exactly. **`forbidden` is not** — the
    /// MCP error codes `rmcp` names carry no such member, and what Nuclear
    /// Notes puts on the wire for one is unmeasured, because measuring it needs
    /// a token this arc does not have. So this fires on **any** refusal the
    /// server itself returned for a claimed tool, which is a superset of what
    /// D6 asks for and settles nothing about what `forbidden` looks like.
    /// Firing wider is the safe direction: the cache is refreshed more often
    /// than needed and the original failure is surfaced either way. The
    /// narrower gate waits on a measurement.
    ///
    /// It does **not** fire on a transport failure or a timeout, which are not
    /// the server refusing anything and say nothing about a scope.
    pub fn claimed(refused: CallRefused, claimed: &[String]) -> Result<Self, CallRefused> {
        if claimed.contains(&refused.tool) {
            Ok(Self::Claimed(refused))
        } else {
            Err(refused)
        }
    }
}

impl fmt::Display for Invalidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ListChanged => f.write_str(
                "the server sent notifications/tools/list_changed, so the cached tool scope is \
                 stale",
            ),
            Self::Expired { window, elapsed } => write!(
                f,
                "the cached tool scope is {elapsed:?} old against a {window:?} window, so the \
                 backstop fired; a missed list_changed is what this window exists to bound"
            ),
            Self::Claimed(refused) => write!(
                f,
                "the cache claimed {} and the server refused it, so the cache is stale: {}",
                refused.tool, refused
            ),
        }
    }
}
