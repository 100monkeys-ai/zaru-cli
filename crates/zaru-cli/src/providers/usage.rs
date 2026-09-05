// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D7's accounting, as a datum with no arithmetic in it.
//!
//! D7: "Every request records prompt tokens, completion tokens, and — where
//! the provider publishes pricing — cost. Per turn in the status line, per
//! session on exit."
//!
//! # Nothing here computes a cost, and that is the whole design
//!
//! "**Where the provider publishes pricing**" is a conditional, and no
//! provider exists in this workspace to publish any. So [`Cost`] is a value a
//! caller *reports*, never one this module derives: there is no rate, no
//! multiplication, no rounding and no currency. Every one of those would be a
//! decision — which currency, what scale, whether a fractional token is
//! rounded up — that no record makes, and a number invented here would be
//! rendered to a user as though somebody had chosen it.
//!
//! The unit travels with the amount for the same reason. A bare number is a
//! number in a currency the reader has to guess, so [`Cost::reported`] takes
//! the unit its caller means and this module picks none.
//!
//! # There is no total, and no sum across requests
//!
//! D7 names three things and a total is not one of them, so there is no
//! `total_tokens`: a method that exists is a method a renderer will show, and
//! a total nobody asked for is a fourth quantity to keep consistent. D7's "per
//! session on exit" needs a sum across requests, which needs a session — and
//! [ADR-0010]'s session lifecycle carries no provider request, because there
//! is no provider. That half of D7 waits rather than being approximated.
//!
//! # What is missing, and where it is
//!
//! D7 wants these in the status line. **There is no status line**: `zaru-tui`
//! renders nothing and depends only on `zaru-core`, so it cannot see this
//! type. That is the same shape [ADR-0013] D6's context usage and [ADR-0016]
//! D1's presentation are both in, and it is why this is a datum rather than a
//! rendering.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use core::fmt;

/// Why a reported cost was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostRefused {
    /// The unit was empty, or was nothing but whitespace.
    ///
    /// A number with no unit is a number the reader has to guess the meaning
    /// of, and D7 puts this in front of a user on every turn.
    UnitMissing,
    /// The unit carried a control character.
    ///
    /// It is rendered into a status line, where one can move the cursor or
    /// erase a neighbouring row.
    UnitControl {
        /// The unit as it was offered, escaped.
        offered: String,
    },
}

impl fmt::Display for CostRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnitMissing => f.write_str(
                "a reported cost has no unit; ADR-0012 D7 puts cost in front of a user on every \
                 turn, and a bare number is one they have to guess the currency of",
            ),
            Self::UnitControl { offered } => write!(
                f,
                "the cost unit {offered:?} carries a control character; it is rendered into a \
                 status line, where one can erase or overwrite a neighbouring row",
            ),
        }
    }
}

impl std::error::Error for CostRefused {}

/// What a request cost, as the provider reported it.
///
/// **Reported, never computed.** See the module documentation: no rate lives
/// here, and neither does a currency.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cost {
    amount: u64,
    unit: String,
}

impl Cost {
    /// Report what a provider said a request cost, in the unit it said it in.
    ///
    /// # Errors
    ///
    /// [`CostRefused`] when the unit is missing or cannot be rendered.
    pub fn reported(amount: u64, unit: &str) -> Result<Self, CostRefused> {
        if unit.trim().is_empty() {
            return Err(CostRefused::UnitMissing);
        }
        if unit.chars().any(char::is_control) {
            return Err(CostRefused::UnitControl {
                offered: unit.escape_debug().to_string(),
            });
        }
        Ok(Self {
            amount,
            unit: unit.to_owned(),
        })
    }

    /// The amount, in [`Cost::unit`] and in nothing else.
    #[must_use]
    pub const fn amount(&self) -> u64 {
        self.amount
    }

    /// The unit the caller reported the amount in.
    #[must_use]
    pub fn unit(&self) -> &str {
        &self.unit
    }
}

impl fmt::Display for Cost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.unit)
    }
}

/// [ADR-0012] D7's three quantities, for one request.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    cost: Option<Cost>,
}

impl TokenUsage {
    /// What a request used, with no cost reported.
    ///
    /// The ordinary case: D7 makes cost conditional on the provider publishing
    /// pricing, and most do not publish it on the response.
    #[must_use]
    pub const fn counted(prompt_tokens: u64, completion_tokens: u64) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            cost: None,
        }
    }

    /// The same usage, with what the provider said it cost.
    #[must_use]
    pub fn priced(mut self, cost: Cost) -> Self {
        self.cost = Some(cost);
        self
    }

    /// Tokens the prompt occupied.
    #[must_use]
    pub const fn prompt_tokens(&self) -> u64 {
        self.prompt_tokens
    }

    /// Tokens the completion occupied.
    #[must_use]
    pub const fn completion_tokens(&self) -> u64 {
        self.completion_tokens
    }

    /// What it cost, where the provider published pricing.
    #[must_use]
    pub const fn cost(&self) -> Option<&Cost> {
        self.cost.as_ref()
    }
}
