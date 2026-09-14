// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D4's retry policy, as data.
//!
//! # The word is `retry`, and it is never `iteration`
//!
//! D4: "The retry is visible, counted, and bounded — and it is labelled
//! `retry`, never `iteration`. Conflating the two in the display is the same
//! conflation [ADR-0008] D1 forbids in the code... A retry repeats hoping for
//! a different outcome; an iteration changes the attempt because of the
//! failure."
//!
//! [Ubiquitous Language] lists *retry* as an anti-term **for an iteration**,
//! which is the opposite obligation: it forbids calling an iteration a retry,
//! and D4 requires calling a retry a retry. So this module uses the word
//! deliberately and uses the other one nowhere at all — a check asserts that
//! nothing this module renders contains it.
//!
//! # No number is invented here
//!
//! D4 says retries are bounded and names no bound; nothing in this catalogue
//! carries a backoff. [ADR-0001] D3 owns iteration ceilings and they are a
//! different quantity. So both numbers arrive from the caller and both are
//! refused at zero, in the shape `zaru-core`'s `Ceiling` and
//! `TruncationBudget`, [ADR-0007]'s `Ttl` and [ADR-0011]'s `OutputBudget`
//! already use. **A budget invented by the thing being budgeted is not a
//! budget.**
//!
//! # Nothing retries
//!
//! D4's second half — "the harness obeys it visibly" — needs something to
//! retry, and there is no provider, no network and no socket anywhere in this
//! workspace. What is built is the policy and the count as data; obeying it
//! arrives with [ADR-0012]'s provider.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

use crate::failure::remedy::Statement;
use core::fmt;
use core::time::Duration;

/// The word ADR-0016 D4 requires, in the one place it is spelled.
///
/// A constant rather than a literal at each site, so that a renderer and a
/// transcript cannot label one repeat two ways.
pub const RETRY_LABEL: &str = "retry";

/// A value the caller passed that cannot bound anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitRefused {
    /// A retry ceiling of zero was passed.
    ///
    /// D4 requires retries be bounded. A bound of zero is not a bound on
    /// retrying, it is the decision not to retry, and the two render
    /// identically to anyone reading a count of zero.
    CeilingIsZero,
    /// A backoff of zero was passed.
    ///
    /// D4 has rate limits and transient network failures "retry with backoff".
    /// A backoff of zero is retrying without one, which turns a rate limit
    /// into a tighter rate limit.
    BackoffIsZero,
    /// More retries were reported than the ceiling allows.
    ///
    /// D4's "counted, and bounded" is one clause rather than two: a count
    /// past the bound means the bound was not obeyed, and reporting it as a
    /// policy that was followed would be the invisible retry the record
    /// exists to prevent.
    MoreRetriesThanTheCeiling {
        /// How many were reported.
        made: u32,
        /// What the policy allowed.
        ceiling: u32,
    },
}

impl fmt::Display for WaitRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CeilingIsZero => f.write_str(
                // ADR-0016 D4 requires retries be bounded.
                "a retry ceiling of 0 is refused; a bound of zero is the decision not to retry \
                 rather than a bound on retrying",
            ),
            Self::BackoffIsZero => f.write_str(
                // ADR-0016 D4 has transient failures retry with backoff.
                "a retry backoff of 0 is refused; retrying without one turns a rate limit into a \
                 tighter rate limit",
            ),
            Self::MoreRetriesThanTheCeiling { made, ceiling } => write!(
                f,
                // ADR-0016 D4 says counted and bounded in one clause.
                "{made} retries were reported against a ceiling of {ceiling}; a count past the \
                 bound is a bound that was not obeyed"
            ),
        }
    }
}

impl std::error::Error for WaitRefused {}

/// How many times the harness may repeat before it stops. ADR-0016 D4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryCeiling(u32);

impl RetryCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`WaitRefused::CeilingIsZero`] when `retries` is zero.
    pub const fn new(retries: u32) -> Result<Self, WaitRefused> {
        if retries == 0 {
            return Err(WaitRefused::CeilingIsZero);
        }
        Ok(Self(retries))
    }

    /// The ceiling as a count of retries.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// How long the harness waits before repeating. ADR-0016 D4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff(Duration);

impl Backoff {
    /// Take a backoff from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`WaitRefused::BackoffIsZero`] when `wait` is zero.
    pub const fn new(wait: Duration) -> Result<Self, WaitRefused> {
        if wait.is_zero() {
            return Err(WaitRefused::BackoffIsZero);
        }
        Ok(Self(wait))
    }

    /// The wait before the next retry.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// What ADR-0016 D4 says an environmental failure retries under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// How many retries are allowed.
    pub ceiling: RetryCeiling,
    /// How long to wait before each.
    pub backoff: Backoff,
}

/// A policy and how much of it has been used.
///
/// D4's three words in one type: **visible** because it is data a consumer is
/// handed rather than a number kept inside a retry helper, **counted** because
/// `made` is a field, and **bounded** because it cannot exceed the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryRecord {
    policy: RetryPolicy,
    made: u32,
}

impl RetryRecord {
    /// Report a run under a policy.
    ///
    /// # Errors
    ///
    /// [`WaitRefused::MoreRetriesThanTheCeiling`] when `made` exceeds the
    /// policy's ceiling.
    pub const fn new(policy: RetryPolicy, made: u32) -> Result<Self, WaitRefused> {
        if made > policy.ceiling.get() {
            return Err(WaitRefused::MoreRetriesThanTheCeiling {
                made,
                ceiling: policy.ceiling.get(),
            });
        }
        Ok(Self { policy, made })
    }

    /// The policy this ran under.
    #[must_use]
    pub const fn policy(&self) -> RetryPolicy {
        self.policy
    }

    /// How many retries have happened.
    #[must_use]
    pub const fn made(&self) -> u32 {
        self.made
    }

    /// How many the policy still allows.
    #[must_use]
    pub const fn remaining(&self) -> u32 {
        self.policy.ceiling.get() - self.made
    }

    /// How long the reader is being asked to wait for.
    #[must_use]
    pub const fn backoff(&self) -> Duration {
        self.policy.backoff.get()
    }
}

/// ADR-0016 D2's obligation on an environmental failure: "Says whether to wait
/// and how long."
///
/// Two variants, and the second is D2's other half — "Where there genuinely is
/// no action, say that." An environmental failure that waiting cannot fix has
/// to say so rather than leaving the reader to wait forever, and the type is
/// what makes saying so unavoidable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wait {
    /// Waiting is the answer, under this policy and this far into it.
    Retrying(RetryRecord),
    /// Waiting is not the answer, and this says why.
    NoWaitWillHelp(Statement),
}

impl Wait {
    /// Whether waiting is being offered as the answer.
    #[must_use]
    pub const fn is_worth_waiting(&self) -> bool {
        matches!(self, Self::Retrying(_))
    }
}
