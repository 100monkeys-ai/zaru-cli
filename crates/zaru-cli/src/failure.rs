// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016]'s error taxonomy: five classes, what each one owes its reader,
//! and the mapping to D5's exit codes.
//!
//! # Why `failure` and not `error`
//!
//! D1's first row says an expected failure is **not an error** — "this is the
//! loop working" — and colouring it like a crash "teaches users to fear the
//! thing that makes the product work". A module called `error` would hold, as
//! its first and most important member, a thing the record says is not one.
//! The record's own title is "Error taxonomy and failure **presentation**",
//! and this module is the second half.
//!
//! # The class is not a field anybody sets
//!
//! [`Class`] is closed: five variants, no sixth, and every match on it is
//! exhaustive so a sixth fails to compile rather than travelling. But nothing
//! constructs a [`Classified`] by naming a class. The class is *which
//! constructor was used*, and each one demands exactly what D1 and D2 oblige
//! that class to say:
//!
//! | Class | What it must carry | So that |
//! | --- | --- | --- |
//! | `Expected` | its statement, and nothing else | D1: nothing is wrong, so there is nothing to remedy |
//! | `UserCorrectable` | a [`Remedy`] | D2: "Says exactly what to change" |
//! | `Environmental` | a [`Wait`] | D2: "Says whether to wait and how long" |
//! | `Capability` | a [`Tier`](crate::tools::Tier) | D1: "Says which tier does" |
//! | `Defect` | a [`DefectReport`] | D3: the session, the version, and where to report |
//!
//! **This is what makes trigger clause 3 a property of the type rather than
//! of a check.** There is exactly one way to build a
//! [`Classified::UserCorrectable`] and it takes a [`Remedy`] positionally, and
//! a `Remedy` cannot be empty by construction. "Every user-correctable error
//! carries a remedy" is therefore not something a test could discover to be
//! false; it is something the compiler will not let anybody write. The
//! clause's *exhaustive enumeration* is discharged by [`classify`] being a
//! wildcard-free match over every mapped enum, so a new variant anywhere in
//! the workspace fails to compile here rather than being misclassified —
//! which D1's own Negative consequence calls worse than being unclassified.
//!
//! # D2's "admits there is not one" is typed, not remembered
//!
//! D2: "Where there genuinely is no action, say that." Each class's payload is
//! the admission as much as the remedy. [`Wait::NoWaitWillHelp`] is an
//! environmental failure saying outright that waiting will not fix it;
//! [`DefectReport`] renders D3's "this is a bug in Zaru, not something you can
//! configure" as part of its own text; and an [`Expected`] carries no remedy
//! because nothing is wrong. No class can be constructed without saying its
//! half of D2.
//!
//! # What this module does not do
//!
//! **It renders nothing.** [`Presentation`] is a projection — a class, a
//! headline and lines — carrying no colour, no glyph, no width and no frame.
//! The terminal is `zaru-tui`'s, and `zaru-tui` depends only on `zaru-core`,
//! so it cannot see this type: a renderer reaches it through a port its own
//! crate declares, the dependency inversion [ADR-0005]'s composer already
//! uses. **Nothing is added to `zaru-core` and no [ADR-0003] D8 edge moves.**
//!
//! **It classifies nothing whose class no record states.** Four error enums in
//! this workspace are deliberately unmapped and every unreadable variant is
//! named on ADR-0016's own Update — see [`classify`].
//!
//! **It carries no configuration key, no command, no flag and no environment
//! variable.** [ADR-0015]'s namespaces and [ADR-0014]'s layer 5 are untouched;
//! `zaru` still takes no arguments.
//!
//! # This exit code is the harness's, and there is another one
//!
//! `zaru_core::iteration::ExecutionOutcome::exit_code` and
//! [`tools::Presented::exit_code`](crate::tools::Presented::exit_code) are
//! both `i32` and are **the work's** — what a command the harness ran
//! reported. [`Class::exit_code`] is `u8` and is **the harness's own** process
//! status. The types differ so that one cannot be passed for the other, and
//! ADR-0016 D5 is about the second: "CI wraps this harness. A wrapper that
//! cannot distinguish 'the tests genuinely fail' from 'the API key is
//! missing' cannot make the right decision."
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

pub mod class;
pub mod classified;
pub mod defect;
pub mod guard;
pub mod partial;
pub mod present;
pub mod remedy;
pub mod wait;

pub use class::{Class, Exit, SUCCESS};
pub use classified::{Classified, Expected};
pub use defect::{DefectReport, Location, SessionEvidence, SessionId, SessionIdRefused};
pub use guard::{Caught, Guarded, OwnWords, guard};
pub use partial::{Partial, PartialRefused, StepName};
pub use present::{Line, Presentation};
pub use remedy::{Action, Remedy, Statement, StatementRefused};
pub use wait::{Backoff, RETRY_LABEL, RetryCeiling, RetryRecord, Wait, WaitRefused};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
