// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one number the tool-call loop takes from its caller, and the boundary
//! that refuses a useless one.
//!
//! # No record carries this number, and the loop does not invent one
//!
//! [ADR-0001] D3's table is **iteration** ceilings, per tier and per
//! provider, and iterations are the inner loop's. Nothing in the catalogue
//! bounds how many times the outer loop may ask a model that keeps requesting
//! tools — and an unbounded outer loop does not terminate, which is not a
//! property a harness may acquire by omission.
//!
//! So the bound arrives as a parameter and is refused at zero, in the shape
//! [`Ceiling`](crate::iteration::Ceiling),
//! [`TruncationBudget`](crate::iteration::TruncationBudget), ADR-0007's `Ttl`
//! and ADR-0011's `OutputBudget` already use here. A budget invented by the
//! thing being budgeted is not a budget. That this record's table has no row
//! for it is recorded as a proposed Update on [ADR-0008] rather than filled
//! in by whoever wrote the loop.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop

use core::fmt;

/// A bound the caller passed that the loop cannot work with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingIsZero;

impl fmt::Display for CeilingIsZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a tool-call ceiling of 0 is refused; the loop needs at least one exchange with the \
             model to report anything, and a turn that asked nothing is indistinguishable from a \
             turn whose ceiling was reached",
        )
    }
}

impl std::error::Error for CeilingIsZero {}

/// How many times one turn may ask the model before it is exhausted.
///
/// It bounds **exchanges with the model**, not tool calls, because that is
/// the thing that repeats: a model may ask for five tools in one answer and
/// the loop runs all five before asking again. Bounding the calls instead
/// would make a turn's budget depend on how the provider happened to batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCallCeiling(u32);

impl ToolCallCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`CeilingIsZero`] when `exchanges` is zero.
    pub const fn new(exchanges: u32) -> Result<Self, CeilingIsZero> {
        if exchanges == 0 {
            return Err(CeilingIsZero);
        }
        Ok(Self(exchanges))
    }

    /// The ceiling as a count of exchanges.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CeilingIsZero, ToolCallCeiling};

    #[test]
    fn a_ceiling_of_zero_is_refused_at_the_boundary() {
        assert_eq!(ToolCallCeiling::new(0), Err(CeilingIsZero));
        assert!(
            CeilingIsZero.to_string().contains('0'),
            "the refusal should name the value that was refused: {}",
            CeilingIsZero
        );
        assert!(
            ToolCallCeiling::new(1).is_ok(),
            "one exchange is a usable ceiling"
        );
        assert_eq!(ToolCallCeiling::new(3).expect("three").get(), 3);
    }
}
