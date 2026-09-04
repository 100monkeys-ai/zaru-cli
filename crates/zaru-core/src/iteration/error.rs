// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This crate's own error type.
//!
//! Each crate in the workspace owns its error enum; `zaru-cli` owns ADR-0016's
//! taxonomy and the mapping to exit codes, and classifies what is raised here.
//! Nothing in this module is that taxonomy.
//!
//! The distinction this type exists to hold is ADR-0008 D5's. A loop that ran
//! out of iterations succeeded at being a loop and failed to solve the
//! problem, so it returns `Ok` carrying
//! [`Outcome::Exhausted`](crate::iteration::Outcome::Exhausted). A loop whose
//! provider was unreachable never got to try, so it returns `Err`. Collapsing
//! the two would report an environmental failure as the mechanism working.

use crate::iteration::port::PortFailure;
use core::fmt;

/// Which port failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortKind {
    /// The generation port. ADR-0012.
    Generator,
    /// The execution port.
    Executor,
    /// The declared validators. ADR-0009.
    Validators,
    /// The context policy. ADR-0013.
    ContextPolicy,
}

impl PortKind {
    /// The port's name as a failure message should say it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Generator => "generation",
            Self::Executor => "execution",
            Self::Validators => "validators",
            Self::ContextPolicy => "context policy",
        }
    }
}

/// The loop could not run to an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterationError {
    /// A port the loop called out through failed.
    Port {
        /// Which port.
        port: PortKind,
        /// Which iteration it failed on, counting from one.
        iteration: u32,
        /// What the port said, in its own words.
        failure: PortFailure,
    },
}

impl fmt::Display for IterationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Port {
                port,
                iteration,
                failure,
            } => write!(
                f,
                "the {} port failed on iteration {}: {}",
                port.as_str(),
                iteration,
                failure
            ),
        }
    }
}

impl std::error::Error for IterationError {}
