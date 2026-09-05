// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D3's boundary: where a panic stops being a Rust panic and starts
//! being a defect report.
//!
//! D3: "Panics and internal invariant violations are caught at the session
//! boundary and presented as what they are."
//!
//! # The boundary is one call, and it is narrow because there is nowhere to
//! continue to
//!
//! ADR-0016's own Negative consequence is the constraint: "Catching panics at
//! the session boundary risks masking a corrupted state that should have
//! terminated the process. **The boundary must be narrow.**"
//!
//! [`guard`] wraps exactly one call and nothing else, and [`Guarded`] has no
//! arm that both reports a defect and hands back a value. So a caller cannot
//! carry on with a half-built result: the only thing it can do with a
//! [`Caught`] is report it and exit. That is narrowness held by the type
//! rather than by a rule somebody remembers — the widening that would break it
//! is guarding each statement instead of the body, and the check
//! `nothing_after_a_caught_panic_runs` is what that widening reddens.
//!
//! # The default panic banner has to be replaced, not merely caught
//!
//! `catch_unwind` alone does not stop the default hook printing Rust's own
//! `thread 'main' panicked at ...` banner first, and a defect that arrives as
//! a raw panic is exactly the failure this record's Context names: "a product
//! whose deliberate failures render beautifully while its infrastructure
//! failures render as a Rust panic has undermined its own thesis at the worst
//! moment". So [`guard`] installs its own hook for the duration and restores
//! the previous one afterwards. Measured in a scratch probe on 2026-09-04:
//! with the hook replaced, nothing is printed by the runtime and the message
//! and location are captured instead.
//!
//! # `panic::set_hook` is process-wide, so this is called once
//!
//! The hook is global state and `take_hook`/`set_hook` is not atomic, so two
//! concurrent guards could interleave. The product calls [`guard`] exactly
//! once, from the binary's `main`, and the checks on it hold a lock for the
//! duration so the question never arises. Two probes on 2026-09-04 failed to
//! reproduce interference, and that is not evidence that it cannot happen.
//!
//! # The panic's own words are not in the report
//!
//! Under a **delegated coordinator ruling of 2026-09-04**, open to Jeshua's
//! veto: the message travels beside the report in [`OwnWords`] rather than
//! inside it, so no [`Presentation`] can show it
//! — it is not in the value a presentation is built from. It is the one field
//! on this path that can carry arbitrary captured text, which is the surface
//! [ADR-0008]'s open trigger clause 6 blocks on, and it belongs to ADR-0010's
//! transcript when that exists. **Nothing here redacts, filters or inspects
//! it.**
//!
//! # A structural guard this arc did not have to write
//!
//! `catch_unwind` does nothing under `panic = "abort"`, and the root
//! `Cargo.toml` carries no `[profile]` section, so unwinding is in force. A
//! later profile that changed it would disable this boundary silently — except
//! that the checks below would abort the test process rather than fail it, so
//! the disabling is loud without a separate gate.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::classified::Classified;
use crate::failure::defect::{DefectReport, Location, SessionEvidence};
use crate::failure::present::Presentation;
use core::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};

/// What the panic itself said.
///
/// **Never presented.** See the module documentation: it is held beside the
/// report rather than inside it, so no rendering can reach it, and it is
/// ADR-0010's transcript that will take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnWords(String);

impl OwnWords {
    /// What the panic said, for whatever writes the transcript.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A defect the boundary caught.
///
/// Carries D3's report and, separately, the panic's own words. There is no
/// constructor that puts the second inside the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caught {
    report: DefectReport,
    own_words: OwnWords,
}

impl Caught {
    /// D3's report.
    #[must_use]
    pub const fn report(&self) -> &DefectReport {
        &self.report
    }

    /// What the panic said. Not part of the report and not presented.
    #[must_use]
    pub const fn own_words(&self) -> &OwnWords {
        &self.own_words
    }

    /// What a renderer shows for this defect.
    ///
    /// Built from the report alone, so the panic's own words cannot reach it.
    #[must_use]
    pub fn presentation(&self) -> Presentation {
        Presentation::of(&Classified::Defect(self.report.clone()))
    }
}

impl fmt::Display for Caught {
    /// The plain-text projection of the report. Colour and layout are
    /// `zaru-tui`'s.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.presentation())
    }
}

/// What came back from a guarded call.
///
/// **There is no arm carrying both a defect and a value.** That is what makes
/// the boundary narrow: a caller holding a [`Caught`] has nothing to carry on
/// with, so a corrupted state cannot be resumed past.
#[derive(Debug)]
pub enum Guarded<T> {
    /// The body ran to completion and this is what it returned.
    Ran(T),
    /// The body panicked. ADR-0016 D3.
    Defected(Caught),
}

/// Run `body` behind ADR-0016 D3's boundary.
///
/// `version` and `report_at` come from the caller's own package metadata
/// rather than being retyped here — a list retyped beside the binary is a list
/// that drifts. `session` is what ADR-0010 will supply and today is
/// [`SessionEvidence::NoSessionExists`].
///
/// Call this **once**, at the process boundary. See the module documentation
/// for why.
pub fn guard<T>(
    version: &str,
    report_at: &str,
    session: SessionEvidence,
    body: impl FnOnce() -> T,
) -> Guarded<T> {
    let captured: Arc<Mutex<Option<(Location, String)>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&captured);

    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(Location::unknown, |at| Location {
                file: at.file().to_owned(),
                line: at.line(),
                column: at.column(),
            });
        let said = info.payload().downcast_ref::<&str>().map_or_else(
            || {
                info.payload()
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_default()
            },
            |said| (*said).to_owned(),
        );
        *sink.lock().unwrap_or_else(PoisonError::into_inner) = Some((location, said));
    }));

    let outcome = panic::catch_unwind(AssertUnwindSafe(body));
    panic::set_hook(previous);

    match outcome {
        Ok(value) => Guarded::Ran(value),
        Err(_) => {
            let (location, said) = captured
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .unwrap_or_else(|| (Location::unknown(), String::new()));
            Guarded::Defected(Caught {
                report: DefectReport::new(version, report_at, location, session),
                own_words: OwnWords(said),
            })
        }
    }
}
