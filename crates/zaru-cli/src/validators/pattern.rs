// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D3's `matches = "<regex>"`, decided over `regex`.
//!
//! # Standard output, and nothing here decided that
//!
//! D3's third row is "Stdout matches"; the port's own documentation is
//! "whether the command's standard output matches the declared pattern"; and
//! `zaru-core`'s dispatch already hands this implementation
//! `ValidatorOutput::stdout`. **So a validator cannot match standard error**,
//! which is a property of D3 rather than of this module — recorded on that
//! record for its author rather than repaired here, because the port's
//! signature is what would have to change.
//!
//! # A search, not a whole-string match
//!
//! D3 says "Stdout matches" and names no anchoring, so
//! [`Regex::is_match`](regex::Regex::is_match) is used as it stands: a search.
//! That is what "matches" means in every tool a user already knows, and it is
//! the reading that does not surprise — anchoring would make
//! `expect = { matches = "ok" }` fail on `everything ok` with nothing saying
//! why. A project that wants anchoring writes `^…$`, where `^` is the start of
//! the whole output unless the pattern opens with `(?m)`. **A reading, not a
//! clause**, recorded as a proposed Update on ADR-0009 D3 rather than settled
//! here.
//!
//! # Nothing is cached
//!
//! One compile per call. `zaru-core`'s dispatch asks once per validator per
//! iteration, and a cache keyed by pattern is state whose invalidation nobody
//! has decided.
//!
//! # What the refusal may say, which was measured before it was written
//!
//! See [`crate::validators`]: `regex::Error` renders the pattern in both its
//! `Display` and its `Debug`, so [`PatternRefused`] carries neither. What it
//! carries is the kind — a pattern that will not parse, or one whose compiled
//! program is past the caller's ceiling — plus, for the second, the ceiling,
//! which is the harness's own number and not the project's text.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators

use crate::validators::ceiling::PatternCeiling;
use core::fmt;
use core::future::Future;
use regex::RegexBuilder;
use zaru_core::iteration::PortFailure;
use zaru_core::iteration::validator::{Pattern, PatternMatch};

/// Why a declared `matches` pattern could not be used.
///
/// **No variant carries the pattern**, and that is the whole shape of this
/// type rather than an omission — see [`crate::validators`] for the
/// measurement behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternRefused {
    /// The pattern is not a regular expression this engine accepts.
    ///
    /// Which covers a syntax error and, deliberately in the same variant, a
    /// construct `regex` does not implement: look-around and backreferences,
    /// which that crate omits *because* they cannot be evaluated in bounded
    /// time. Both are the declaration being unusable, and separating them
    /// would invite a reader to think one of them might be added later.
    NotAPattern,
    /// The pattern compiles to a program larger than the caller's ceiling.
    TooLarge {
        /// The largest compiled program this caller will run.
        ceiling: usize,
    },
}

impl fmt::Display for PatternRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAPattern => f.write_str(
                "a `matches` validator's pattern is not a regular expression this harness can \
                 compile. The pattern is deliberately not quoted here, because a manifest is a \
                 file that gets committed and a refusal is text that gets pasted into a report. \
                 Note that look-around and backreferences are not supported at all: the engine \
                 omits them so that every search is bounded in time",
            ),
            Self::TooLarge { ceiling } => write!(
                f,
                "a `matches` validator's pattern compiles to a program larger than {ceiling} \
                 bytes, which is the largest this harness will run. Counted repetitions expand, \
                 so a short pattern can ask for a very large program. The pattern is \
                 deliberately not quoted here",
            ),
        }
    }
}

impl std::error::Error for PatternRefused {}

impl From<PatternRefused> for PortFailure {
    fn from(refusal: PatternRefused) -> Self {
        Self::new(refusal.to_string())
    }
}

/// [ADR-0009] D3's `matches` kind, over `regex`.
///
/// Holds the ceiling and nothing else, so it is `Sync`, which
/// [`Dispatch`](zaru_core::iteration::validator::Dispatch) requires.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patterns {
    ceiling: PatternCeiling,
}

impl Patterns {
    /// Decide `matches` validators under this ceiling.
    #[must_use]
    pub const fn new(ceiling: PatternCeiling) -> Self {
        Self { ceiling }
    }

    /// The ceiling every pattern is compiled under.
    #[must_use]
    pub const fn ceiling(&self) -> PatternCeiling {
        self.ceiling
    }

    /// Whether the pattern matches, in this implementation's own refusals.
    ///
    /// [`PatternMatch::matches`] is this, with the refusal flattened into the
    /// port's type. A caller that wants to know *which* thing was wrong calls
    /// here.
    ///
    /// # Errors
    ///
    /// [`PatternRefused`].
    pub fn decide(&self, pattern: &Pattern, stdout: &str) -> Result<bool, PatternRefused> {
        let compiled = RegexBuilder::new(pattern.as_str())
            .size_limit(self.ceiling.get())
            .build()
            .map_err(|error| match error {
                // `Error` is `#[non_exhaustive]`, so the arm is by shape
                // rather than by name and a third kind lands in the first
                // variant, which is the one that says only that the
                // declaration is unusable.
                regex::Error::CompiledTooBig(_) => PatternRefused::TooLarge {
                    ceiling: self.ceiling.get(),
                },
                _ => PatternRefused::NotAPattern,
            })?;
        Ok(compiled.is_match(stdout))
    }
}

impl PatternMatch for Patterns {
    fn matches(
        &self,
        pattern: &Pattern,
        stdout: &str,
    ) -> impl Future<Output = Result<bool, PortFailure>> + Send {
        // Decided before the future is built, so nothing that is not `Send`
        // is held across it. See the module documentation of
        // [`crate::validators`].
        let decided = self.decide(pattern, stdout).map_err(PortFailure::from);
        async move { decided }
    }
}
