// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two numbers the loop takes from its caller, and the boundary that
//! refuses a useless one.
//!
//! Neither number is decided here. ADR-0001 D3 owns the iteration ceilings,
//! per tier and per provider; no record carries a truncation budget at all.
//! A budget invented by the thing being budgeted is not a budget, so both
//! arrive as parameters and `zaru-cli` owns where they come from.
//!
//! This is a boundary in the sense [Operating Principles] means: values
//! crossing into the loop from outside are validated here and nowhere else.
//!
//! [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles

use core::fmt;

/// A value the caller passed that the loop cannot work with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationError {
    /// An iteration ceiling of zero was passed.
    ///
    /// Refused rather than accepted, because a loop that runs no iterations
    /// still has to report something, and the only honest thing it could
    /// report is that it stopped having generated nothing — which is
    /// indistinguishable, to every consumer of the event stream, from a
    /// ceiling that was reached.
    CeilingIsZero,
    /// A truncation budget of zero was passed.
    ///
    /// ADR-0008 D4 requires that truncation keep the head and the tail of the
    /// output and mark the elision. A budget of zero can keep neither, so the
    /// refinement prompt would carry nothing of the failure it exists to
    /// carry.
    BudgetIsZero,
}

impl fmt::Display for ConfigurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CeilingIsZero => f.write_str(
                "iteration ceiling is 0; the loop needs at least one iteration to report anything",
            ),
            Self::BudgetIsZero => f.write_str(
                "truncation budget is 0; a refinement prompt cannot carry a failure in zero bytes",
            ),
        }
    }
}

impl std::error::Error for ConfigurationError {}

/// How many iterations the loop may run before it is exhausted.
///
/// ADR-0008 D5: ceilings are per-tier and per-provider and ADR-0001 D3 owns
/// the numbers. The loop takes one and never chooses one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceiling(u32);

impl Ceiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`ConfigurationError::CeilingIsZero`] when `iterations` is zero.
    pub const fn new(iterations: u32) -> Result<Self, ConfigurationError> {
        if iterations == 0 {
            return Err(ConfigurationError::CeilingIsZero);
        }
        Ok(Self(iterations))
    }

    /// The ceiling as a count of iterations.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// How many bytes of any one variable-length part a prompt may carry.
///
/// Applied independently to the previous candidate, the execution's standard
/// output, its standard error, and the validators' failure text, so that one
/// number bounds each part rather than four numbers nobody has chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruncationBudget(usize);

impl TruncationBudget {
    /// Take a budget from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`ConfigurationError::BudgetIsZero`] when `bytes` is zero.
    pub const fn new(bytes: usize) -> Result<Self, ConfigurationError> {
        if bytes == 0 {
            return Err(ConfigurationError::BudgetIsZero);
        }
        Ok(Self(bytes))
    }

    /// The budget in bytes.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Everything the caller bounds the loop with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How many iterations may run.
    pub ceiling: Ceiling,
    /// How many bytes of any one part a prompt may carry.
    pub budget: TruncationBudget,
}

#[cfg(test)]
mod tests {
    use super::{Ceiling, ConfigurationError, TruncationBudget};

    #[test]
    fn a_ceiling_of_zero_is_refused_at_the_boundary() {
        assert_eq!(Ceiling::new(0), Err(ConfigurationError::CeilingIsZero));
        assert!(
            ConfigurationError::CeilingIsZero.to_string().contains('0'),
            "the refusal should name the value that was refused"
        );
        assert!(Ceiling::new(1).is_ok(), "one iteration is a usable ceiling");

        assert_eq!(
            TruncationBudget::new(0),
            Err(ConfigurationError::BudgetIsZero)
        );
        assert!(TruncationBudget::new(1).is_ok());
    }
}
