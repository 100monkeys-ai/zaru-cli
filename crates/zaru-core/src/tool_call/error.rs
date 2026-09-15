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
//! # A window that will not fit *is* in here, and it is neither of those two
//!
//! Added 2026-09-15. The two absences above are both "not a failure at all":
//! a user who answered no got what they asked for, and a turn that ran out of
//! exchanges succeeded at being a turn. **A turn whose own context will not
//! assemble is neither** — it never ran, it produced nothing, and the loop
//! has no outcome to return. So it is an `Err`, and
//! [`ToolCallError::ContextWindowExceeded`] is what it is.
//!
//! It is a variant rather than a `Port` failure because the two numbers have
//! to survive the trip. Until this day the turn's assembly refusal was folded
//! into a [`PortFailure`]'s `String` and reached `zaru-cli` as
//! [`PortKind::ContextPolicy`] with nothing to read, so it was classified as a
//! **defect** and a reader whose own `provider.<kind>.context_tokens` was too
//! small was told, in [ADR-0016] D2's own register, that it was "a bug in
//! Zaru, not something you can configure". That is D3's "never present a
//! defect as a user error" inverted, and it is the second time this workspace
//! has met that inversion.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::iteration::port::{ContextRefusal, PortFailure};
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
/// Two variants since 2026-09-15. See the module documentation for the two
/// things that are deliberately not here, and the section below it for why a
/// window that will not fit is neither of them.
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
    /// Assembling this turn's own context would not fit the window.
    ///
    /// A variant of its own rather than a [`Self::Port`] failure, and the
    /// reason is that [`PortFailure`] carries a `String` and nothing else:
    /// folding the refusal into one turns the two numbers a reader has to act
    /// on into prose, and prose is not something a classifier can read back.
    /// [ADR-0016]'s own Update already says that the `String` "rather than
    /// the absence of an implementation" is what keeps `PortFailure` unmapped.
    ///
    /// **This crate does not say what class it is.** `zaru-cli` owns
    /// [ADR-0016] D1, and the window here came from a key the reader set, so
    /// the class is theirs to state and the numbers are ours to carry.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    ContextWindowExceeded {
        /// Tokens the assembled context would have needed.
        needed: u64,
        /// Tokens the window allows.
        window: u64,
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
            // The refusal's own wording, not a second copy of it: this is the
            // sentence `ContextRefusal::WindowExceeded` writes, reached
            // through that type so the workspace holds exactly one of it. A
            // `write!` here with the same words would be a second copy that
            // can drift, which is the failure this delegation prevents.
            Self::ContextWindowExceeded { needed, window } => ContextRefusal::WindowExceeded {
                needed: *needed,
                window: *window,
            }
            .fmt(f),
        }
    }
}

impl std::error::Error for ToolCallError {}
