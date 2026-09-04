// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D6: "Work that completed three of five steps reports three of
//! five, names what completed, and names what did not. It does not report
//! failure and discard the record of what was accomplished."
//!
//! # Partial is not a class
//!
//! D6 is about *reporting*, and D1's five classes are about what kind of
//! failure something is. A partially completed run may end in any of them or
//! in none, so [`Partial`] is a report a caller carries beside a
//! [`Classified`](crate::failure::Classified) rather than a sixth variant of
//! it. Making it one would have been the misclassification D1's Negative
//! consequence warns about.
//!
//! # No step vocabulary is invented
//!
//! **No record in this catalogue names a step**, so a [`StepName`] is whatever
//! its caller passed. [ADR-0009]'s validators, [ADR-0011]'s tool calls and
//! [ADR-0015]'s skills are three different things that could be steps and each
//! belongs to its own record.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::remedy::{Statement, StatementRefused};
use core::fmt;

/// One step's name, as whoever ran it calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepName(String);

impl StepName {
    /// Take a step's name.
    ///
    /// # Errors
    ///
    /// [`StatementRefused`] for an empty name or one carrying a control
    /// character — D6's report is rendered, and an unnameable step cannot be
    /// named.
    pub fn new(name: impl Into<String>) -> Result<Self, StatementRefused> {
        Statement::new(name).map(|statement| Self(statement.as_str().to_owned()))
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StepName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a report is not a partial one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialRefused {
    /// Nothing completed.
    ///
    /// That is a failure rather than a partial success, and reporting it as
    /// partial claims an accomplishment there is no record of — the inverse of
    /// the discard D6 forbids, and just as untrue.
    NothingCompleted,
    /// Nothing was left outstanding.
    ///
    /// That is a success. Reporting it as partial tells a reader to go looking
    /// for work that is not there.
    NothingOutstanding,
}

impl fmt::Display for PartialRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingCompleted => f.write_str(
                "a partial report completed nothing; ADR-0016 D6 is about work that completed \
                 some of its steps, and a run that completed none of them failed",
            ),
            Self::NothingOutstanding => f.write_str(
                "a partial report left nothing outstanding; ADR-0016 D6 is about work that \
                 completed some of its steps, and a run that completed all of them succeeded",
            ),
        }
    }
}

impl std::error::Error for PartialRefused {}

/// ADR-0016 D6's report: what completed, by name, and what did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partial {
    completed: Vec<StepName>,
    not_completed: Vec<StepName>,
}

impl Partial {
    /// Report a run that got part of the way.
    ///
    /// # Errors
    ///
    /// [`PartialRefused`] when either list is empty — see that type for why
    /// neither case is partial.
    pub fn new(
        completed: Vec<StepName>,
        not_completed: Vec<StepName>,
    ) -> Result<Self, PartialRefused> {
        if completed.is_empty() {
            return Err(PartialRefused::NothingCompleted);
        }
        if not_completed.is_empty() {
            return Err(PartialRefused::NothingOutstanding);
        }
        Ok(Self {
            completed,
            not_completed,
        })
    }

    /// What completed, by name.
    #[must_use]
    pub fn completed(&self) -> &[StepName] {
        &self.completed
    }

    /// What did not, by name.
    #[must_use]
    pub fn not_completed(&self) -> &[StepName] {
        &self.not_completed
    }

    /// D6's "of five" — every step there was.
    ///
    /// The sum of the two lists the caller staged, rather than a total carried
    /// separately. A denominator kept beside the lists is a third number that
    /// can disagree with them.
    #[must_use]
    pub fn of(&self) -> usize {
        self.completed.len() + self.not_completed.len()
    }
}

impl fmt::Display for Partial {
    /// D6's report as plain text. Colour, width and glyphs are `zaru-tui`'s.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{} of {} steps completed",
            self.completed.len(),
            self.of()
        )?;
        for step in &self.completed {
            writeln!(f, "  completed:      {step}")?;
        }
        for (index, step) in self.not_completed.iter().enumerate() {
            let last = index + 1 == self.not_completed.len();
            if last {
                write!(f, "  did not run:    {step}")?;
            } else {
                writeln!(f, "  did not run:    {step}")?;
            }
        }
        Ok(())
    }
}
