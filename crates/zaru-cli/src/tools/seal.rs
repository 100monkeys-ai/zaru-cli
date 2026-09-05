// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a SEAL verdict gates a tool call, as a seam with no implementation.
//!
//! [ADR-0004] D2 gives `contained` and `linked` a membrane enforced by the
//! orchestrator, and D6 requires that "**every** SEAL verdict is rendered" —
//! allowed and denied alike, with the context name and the reason. This
//! module is the place a verdict would be asked for. **Nothing in any product
//! tree implements it**, and ADR-0004 is blocked upstream: its own Status
//! tracking says conformance vectors do not exist in the `seal-protocol`
//! repository, and "a third independent implementation without them is three
//! implementations that drift silently".
//!
//! # The shape, and what choosing it does and does not settle
//!
//! Two-valued plus a reason, under a **delegated coordinator ruling of
//! 2026-09-04** open to Jeshua's veto. It is not an invention: D6's own
//! rendered example is a tick, a cross, and a reason drawn from the error-code
//! registry —
//!
//! ```text
//! ✓ fs.read   src/main.rs              ctx zaru-local
//! ✗ fs.read   /etc/passwd              ctx zaru-local
//!   PATH_NOT_ALLOWED — not in ["./**"]
//! ```
//!
//! — so allowed-or-denied-with-a-reason is the record's own shape read back.
//! **No reason code is authored here.** D6 says the strings "come from the
//! existing SEAL error-code registry (ADR-088 A5), so the harness renders
//! what the gateway already produces rather than inventing a second
//! vocabulary", and inventing one would be the security-vocabulary authoring
//! [Autonomous Development] puts on the human side of the boundary.
//!
//! # A denial is an expected failure, and the mode cannot suppress it
//!
//! [ADR-0016] D1 row 1 names "denied verdict" among the expected failures —
//! "**Not an error — this is the loop working.**" So a denial is presented in
//! that register and never the error one.
//!
//! And [ADR-0011] D3: at `contained` and above the mode governs "prompting
//! only — SEAL enforcement is not affected by it. A user in `yolo` inside a
//! membrane is still inside the membrane." The executor asks for a verdict
//! before it consults the mode at all, and [`Mode`](crate::tools::Mode) is
//! not an input to [`Verdicts::verdict`]. There is nothing there for a mode
//! to reach, which is absence rather than a branch somebody remembered.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::failure::{Classified, Expected, Statement};
use crate::tools::decision::Invocation;

/// What the membrane said about one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The call may proceed.
    Allowed,
    /// The call may not, and this is the registry's reason for it.
    Denied {
        /// The code, from ADR-088 A5's registry. **Authored there, not here.**
        code: String,
        /// The reason, in the registry's own words.
        reason: String,
    },
}

impl Verdict {
    /// Present a denial as [ADR-0016] D1 row 1's expected failure.
    ///
    /// `None` for an allowed call, which is not a failure of any kind.
    ///
    /// This is the whole of "a refusing verdict is presented as expected
    /// failure, not error": the only [`Classified`] this type can produce is
    /// an [`Expected`], because there is no constructor here for any other,
    /// and [`Class::is_the_error_register`](crate::failure::Class::is_the_error_register)
    /// is `false` for that class by construction.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn as_expected_failure(&self) -> Option<Classified> {
        match self {
            Self::Allowed => None,
            Self::Denied { code, reason } => Some(Classified::Expected(Expected::new(
                Statement::sanitised(format!("{code} — {reason}")),
            ))),
        }
    }
}

/// Asks the membrane about a call. [ADR-0004] owns what is behind this.
///
/// **No product implementation**, and the record is blocked upstream on
/// conformance vectors — see the module documentation.
///
/// [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
pub trait Verdicts {
    /// What the membrane says about this call.
    ///
    /// Synchronous, and deliberately: at `contained` the orchestrator is a
    /// local process and at `bare` there is nothing to ask. Whether it
    /// becomes asynchronous is ADR-0004's to decide when it has a transport,
    /// and making it so now would be a shape chosen with no implementation
    /// behind it.
    ///
    /// **Takes no [`Mode`](crate::tools::Mode)**, so a permission mode cannot
    /// reach enforcement. ADR-0011 D3.
    fn verdict(&self, invocation: &Invocation<'_>) -> Verdict;
}

/// The verdict a tier with no membrane gives.
///
/// [ADR-0001] D1 gives `bare` no membrane at all, so there is nothing to ask
/// and every call is allowed **by this port** — which says nothing about
/// whether ADR-0011's permission decision allows it, and that decision is
/// made separately and afterwards.
///
/// This is not a product implementation of enforcement. It is the honest
/// answer at a tier where the record says there is none, and it exists so
/// that a caller at `bare` is not forced to write one and accidentally write
/// a policy while doing it.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMembrane;

impl Verdicts for NoMembrane {
    fn verdict(&self, _invocation: &Invocation<'_>) -> Verdict {
        Verdict::Allowed
    }
}
