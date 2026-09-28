// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D1's five classes and D5's exit codes.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::classified::Classified;
use core::fmt;

/// ADR-0016 D5's exit code for a run that did what was asked.
///
/// Not one of [`Class`]'s five, and deliberately not on that enum: success is
/// the *absence* of a failure rather than a kind of one. D5's table opens with
/// it, so [`Exit`] is where the whole of that table lives.
pub const SUCCESS: u8 = 0;

/// The exit code for a session a signal ended: `128 + n`, the status a POSIX
/// shell reports for a process that signal `n` killed.
///
/// **Not a sixth class and not a row D5's table authored.** A signal is not a
/// failure of the run, so no class describes it. Before 2026-09-27 the
/// harness took no signal at all, and the default action killed it, so a
/// person's `$?` read `128 + n`. `terminal::open` now catches three signals so
/// that it can give the terminal back first, and then exits with this code, so
/// what a shell or a CI wrapper reads is unchanged. `SIGKILL` cannot be caught
/// and still reaches the default action.
#[must_use]
pub const fn signalled(signal: u8) -> u8 {
    128 + signal
}

/// One of ADR-0016 D1's five classes.
///
/// **Closed.** Five variants and no sixth; every match on it below is
/// exhaustive with no wildcard arm, so adding one is a compile error here
/// rather than a silent default somewhere. D1's Negative consequence is why
/// that matters: "a misclassified error is worse than an unclassified one
/// because the presentation actively misleads".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// D1 row 1 — the work's. "Iteration failure, denied verdict. **Not an
    /// error — this is the loop working.**"
    Expected,
    /// D1 row 2 — the user's. "Missing key, unreachable endpoint, bad config,
    /// no manifest. Says exactly what to change."
    UserCorrectable,
    /// D1 row 3 — neither's. "Rate limit, network, provider outage. Says
    /// whether to wait and how long."
    Environmental,
    /// D1 row 4 — neither's. "The tier does not offer this. Says which tier
    /// does."
    Capability,
    /// D1 row 5 — ours. "A bug. Says so plainly, and how to report it."
    Defect,
}

impl Class {
    /// Every class D1 names, in the record's own order.
    ///
    /// The length is annotated, so a sixth variant fails to compile here as
    /// well as in every exhaustive match below.
    pub const ALL: [Self; 5] = [
        Self::Expected,
        Self::UserCorrectable,
        Self::Environmental,
        Self::Capability,
        Self::Defect,
    ];

    /// What D1's first column calls this class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Expected => "expected",
            Self::UserCorrectable => "user-correctable",
            Self::Environmental => "environmental",
            Self::Capability => "capability",
            Self::Defect => "defect",
        }
    }

    /// The class a stored spelling names, if it names one.
    ///
    /// The reverse of [`Self::as_str`], walked over [`Self::ALL`] rather than
    /// written as a second match arm, so the two spellings cannot drift apart
    /// and a sixth class needs no edit here. A transcript keeps the class as
    /// text ([ADR-0010] D2), and the pane has to get back to the register D1
    /// gives it when that transcript is replayed.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn named(spelling: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|class| class.as_str() == spelling)
    }

    /// ADR-0016 D5's exit code for this class.
    ///
    /// A **total function over the enum with no wildcard arm**, so a sixth
    /// class cannot arrive without a number being chosen for it here.
    ///
    /// `Expected` maps to D5's `1`, and that pairing reads oddly enough to be
    /// worth stating: D5 calls `1` "the work failed (loop exhausted, validator
    /// never satisfied)" while D1 calls the same row "not an error — this is
    /// the loop working". Both are true and they are different sentences. The
    /// loop working is the *mechanism* operating; the work failing is the
    /// *outcome* the caller gets, and a wrapping CI job needs the second.
    ///
    /// The codes are D5's verbatim and none is invented: 1, 2, 3, 4, 70.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Expected => 1,
            Self::UserCorrectable => 2,
            Self::Environmental => 3,
            Self::Capability => 4,
            Self::Defect => 70,
        }
    }

    /// Whether a failure of this class renders in the error register.
    ///
    /// ADR-0016 D1: "**Expected failures never render as errors.** An
    /// iteration that fails is the mechanism operating, and colouring it like
    /// a crash teaches users to fear the thing that makes the product work."
    ///
    /// This is the whole of trigger clause 2 that this crate can hold: the
    /// register is *derived* from the class rather than being a field
    /// somewhere, so a `Class::Expected` in the error register is not a state
    /// any code can construct. What a renderer then does with it is
    /// `zaru-tui`'s.
    #[must_use]
    pub const fn is_the_error_register(self) -> bool {
        match self {
            Self::Expected => false,
            Self::UserCorrectable | Self::Environmental | Self::Capability | Self::Defect => true,
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the harness's own process ended, and therefore what it exits with.
///
/// Named `Exit` rather than `Outcome` because `zaru_core::iteration::Outcome`
/// is a different thing — whether the *loop* succeeded or was exhausted — and
/// a second `Outcome` one crate away is how a word stops carrying one.
///
/// This is where the whole of D5's table lives, because [`SUCCESS`] is the
/// absence of a failure rather than a sixth class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    /// The run did what was asked. D5's `0`.
    Succeeded,
    /// The run did not, and this is what kind of not.
    Failed(Classified),
    /// A signal ended the run, and this is its number. [`signalled`] is the
    /// status: `128 + n`, what a shell reports for a process that signal
    /// ended. Not a failure of the run, so it has no class.
    ///
    /// A session ends this way when its terminal goes away or a signal
    /// arrives, once what it was running has been stopped and the session
    /// written down. Added 2026-09-28: until then the signal listener exited
    /// the process where it stood, and a command a turn was running outlived
    /// it.
    Signalled(u8),
}

impl Exit {
    /// The process status this ending exits with.
    ///
    /// Returns a `u8` rather than a [`std::process::ExitCode`] deliberately,
    /// and this is load-bearing: **`ExitCode` has no accessor**, so a mapping
    /// written to it could not be asserted at all except by spawning a
    /// process. `ExitCode::from` is applied at exactly one place, the `zaru`
    /// binary's return.
    #[must_use]
    pub fn code(&self) -> u8 {
        match self {
            Self::Succeeded => SUCCESS,
            Self::Failed(classified) => classified.class().exit_code(),
            Self::Signalled(number) => signalled(*number),
        }
    }

    /// The class of the failure, or `None` for a run that succeeded.
    #[must_use]
    pub fn class(&self) -> Option<Class> {
        match self {
            Self::Succeeded | Self::Signalled(_) => None,
            Self::Failed(classified) => Some(classified.class()),
        }
    }
}
