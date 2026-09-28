// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One failure, classified — and classified by which constructor was used
//! rather than by a field anybody set.
//!
//! See the module documentation of [`failure`](crate::failure) for the table
//! of what each class owes its reader, and for why that makes [ADR-0016]
//! trigger clause 3 a property of the type.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::class::Class;
use crate::failure::defect::DefectReport;
use crate::failure::remedy::{Remedy, Statement};
use crate::failure::wait::Wait;
use crate::tools::Tier;

/// ADR-0016 D1 row 1 — the work's own failure.
///
/// "Iteration failure, denied verdict. **Not an error — this is the loop
/// working.**"
///
/// It carries its statement and, where the statement quotes text that spans
/// lines, those lines under it. It carries no remedy, and the absence is the
/// design: nothing is wrong, and a type with a field for one would invite a
/// raising site to fill it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    statement: Statement,
    detail: Vec<Statement>,
}

impl Expected {
    /// Report something the mechanism did on purpose.
    #[must_use]
    pub const fn new(statement: Statement) -> Self {
        Self {
            statement,
            detail: Vec::new(),
        }
    }

    /// Add lines under the sentence, each escaped on its own.
    ///
    /// For text the harness did not write that spans lines, such as what a
    /// validator printed. Put into the sentence itself, its newlines were
    /// escaped with the rest and a person read them as `\n`; each line here
    /// keeps its own row. Added 2026-09-28.
    #[must_use]
    pub fn showing(mut self, detail: Vec<Statement>) -> Self {
        self.detail = detail;
        self
    }

    /// What happened.
    #[must_use]
    pub const fn statement(&self) -> &Statement {
        &self.statement
    }

    /// The lines under the sentence.
    #[must_use]
    pub fn detail(&self) -> &[Statement] {
        &self.detail
    }
}

/// A failure, in one of ADR-0016 D1's five classes.
///
/// **Nothing constructs one of these by naming a class.** Each variant's
/// payload is exactly what D1 and D2 oblige that class to say, so a class that
/// does not say it is not a value this crate can build:
///
/// - a [`Classified::UserCorrectable`] cannot exist without a [`Remedy`], and
///   a `Remedy` cannot be empty;
/// - a [`Classified::Environmental`] cannot exist without a [`Wait`], whose
///   second variant is D2's "admits there is not one";
/// - a [`Classified::Capability`] cannot exist without naming the
///   [`Tier`] that offers the thing;
/// - a [`Classified::Defect`] cannot exist without D3's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    /// D1 row 1. Renders in its own register and never as an error.
    Expected(Expected),
    /// D1 row 2 + D2: "Says exactly what to change."
    UserCorrectable {
        /// What happened.
        statement: Statement,
        /// What the reader can do about it. **Never empty.**
        remedy: Remedy,
    },
    /// D1 row 3 + D2: "Says whether to wait and how long."
    Environmental {
        /// What happened.
        statement: Statement,
        /// Whether waiting is the answer, and for how long.
        wait: Wait,
    },
    /// D1 row 4 + D2: "Says which tier does."
    Capability {
        /// What was asked for and is not available here.
        statement: Statement,
        /// The tier that offers it. ADR-0001 D1 names the three.
        offered_by: Tier,
    },
    /// D1 row 5 + D3.
    Defect(DefectReport),
}

impl Classified {
    /// Which of D1's five this is.
    ///
    /// **Exhaustive, with no wildcard arm**, so a sixth variant fails to
    /// compile here rather than taking somebody else's class.
    #[must_use]
    pub const fn class(&self) -> Class {
        match self {
            Self::Expected(_) => Class::Expected,
            Self::UserCorrectable { .. } => Class::UserCorrectable,
            Self::Environmental { .. } => Class::Environmental,
            Self::Capability { .. } => Class::Capability,
            Self::Defect(_) => Class::Defect,
        }
    }

    /// ADR-0016 D5's exit code for this failure.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        self.class().exit_code()
    }

    /// What happened, for every class.
    ///
    /// A defect has no caller-written statement — D3 decides what a defect
    /// says about itself — so this returns `None` for one, and
    /// [`Presentation`](crate::failure::Presentation) composes its headline
    /// from the report instead.
    #[must_use]
    pub const fn statement(&self) -> Option<&Statement> {
        match self {
            Self::Expected(expected) => Some(expected.statement()),
            Self::UserCorrectable { statement, .. }
            | Self::Environmental { statement, .. }
            | Self::Capability { statement, .. } => Some(statement),
            Self::Defect(_) => None,
        }
    }

    /// The remedy, where D2 obliges one and the class carries it.
    ///
    /// Only [`Classified::UserCorrectable`] does. The other four discharge D2
    /// through their own payloads rather than through a `Remedy`, which is why
    /// this returns `None` for them rather than an empty one — an empty remedy
    /// is not a value this crate can build.
    #[must_use]
    pub const fn remedy(&self) -> Option<&Remedy> {
        match self {
            Self::UserCorrectable { remedy, .. } => Some(remedy),
            Self::Expected(_)
            | Self::Environmental { .. }
            | Self::Capability { .. }
            | Self::Defect(_) => None,
        }
    }
}
