// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The tool-call loop's own error type, and the thing it deliberately has no
//! variant for.
//!
//! # A refusal is not in here, and that is the whole point of the file
//!
//! Under a **delegated coordinator ruling of 2026-09-04**, recorded on
//! [ADR-0011] and [ADR-0016] and open to Jeshua's veto: a user answering "no"
//! to a permission prompt **is not a failure at all**. It is not the work
//! failing, not user-correctable, not environmental, not a capability the
//! tier lacks and not a defect, and forcing it into ADR-0016 D1's five is
//! what would misclassify it.
//!
//! So [`ToolCallError`] has **no variant a refusal could reach**, and
//! [`ToolOutcome::Refused`](crate::tool_call::ToolOutcome::Refused) has
//! nowhere to go but back to the model as that call's content. "A refusal
//! becomes the next model turn's content rather than a failure" is therefore
//! a property of the types rather than a branch somebody remembered to write
//! — the same shape ADR-0011 D6 uses to make "it does not veto" checkable.
//!
//! # And a ceiling is not in here either
//!
//! [ADR-0008] D5's rule for the inner loop holds for this one: a turn that
//! ran out of exchanges succeeded at being a turn and failed to finish the
//! work, so it returns `Ok` carrying
//! [`Outcome::Exhausted`](crate::tool_call::Outcome::Exhausted). A turn whose
//! provider was unreachable never got to try, so it returns `Err`.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::iteration::port::PortFailure;
use core::fmt;

/// Which port failed.
///
/// A second `PortKind` beside [`crate::iteration::PortKind`], for the reason
/// the event stream is a second enum: these are the outer loop's four ports
/// and none of them is one of the inner loop's five.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    /// The provider. ADR-0012.
    Model,
    /// The tool surface. ADR-0011.
    Tools,
    /// The context policy. ADR-0013.
    ContextPolicy,
    /// The iteration loop, where a project declares validators. ADR-0009.
    InnerLoop,
}

impl PortKind {
    /// The port's name as a failure message should say it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Tools => "tool surface",
            Self::ContextPolicy => "context policy",
            Self::InnerLoop => "iteration loop",
        }
    }
}

/// The turn could not run to an outcome.
///
/// One variant. See the module documentation for the two things that are
/// deliberately not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallError {
    /// A port the loop called out through failed.
    Port {
        /// Which port.
        port: PortKind,
        /// Which exchange within the turn it failed on, counting from one.
        round: u32,
        /// What the port said, in its own words.
        failure: PortFailure,
    },
}

impl fmt::Display for ToolCallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port {
                port,
                round,
                failure,
            } => write!(
                f,
                "the {} port failed on exchange {} of this turn: {}",
                port.as_str(),
                round,
                failure
            ),
        }
    }
}

impl std::error::Error for ToolCallError {}
