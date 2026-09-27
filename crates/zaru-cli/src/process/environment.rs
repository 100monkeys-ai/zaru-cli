// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a child process is allowed to see of the harness's own environment.
//!
//! # An allowlist, because a denylist is silent about what nobody thought of
//!
//! [`Spawn`](super::Spawn) clears the environment and then sets exactly what
//! an [`Environment`] holds. It never removes a list of names from an
//! inherited one, and the difference is the whole point: a denylist naming
//! `ZARU_*` is silent about `GITHUB_TOKEN`, `AWS_SECRET_ACCESS_KEY` and
//! `ANTHROPIC_API_KEY`, and it stays silent about whatever is invented next.
//! An allowlist is wrong in the direction that produces a command which fails
//! loudly, which is the safe direction to be wrong in on a boundary.
//!
//! # `ZARU_*` cannot be put in one at all
//!
//! [ADR-0014] D1's layer 4 is the `ZARU_*` environment — **the harness's own
//! configuration, never a child's**. [`Environment::carrying`] refuses any
//! name with that prefix, so no caller, product or check, can build an
//! environment that leaks one. That is absence rather than a filter somebody
//! remembered to apply, and it matters more the moment [ADR-0007] D3's
//! environment fallback holds key material: a variable that is the harness's
//! key is one a model-driven `cmd.run` must not be able to read back by
//! running `env`.
//!
//! Recorded on ADR-0014 as an accepted reading of D1 under the coordinator's
//! ruling of 2026-09-05, open to Jeshua's veto.
//!
//! # The five names, and why each one
//!
//! [`Environment::inherited_minimum`] is the one place the set is written.
//! It is a **constructor and not a default**: a caller may build any
//! environment it likes with [`Environment::carrying`], and this is what the
//! composition calls when it has no reason to differ.
//!
//! | Name | Why |
//! | --- | --- |
//! | `PATH` | [ADR-0011] D1's `cmd.run` executes what `PATH` finds, and without it a bare program name resolves to nothing at all |
//! | `HOME` | a validator's `run` is a project's own command, and `cargo`, `git` and `npm` each fail without it in a way that reads as a harness bug |
//! | `LANG`, `LC_ALL` | a command whose output encoding differs between the user's shell and the harness produces a capture the user cannot reconcile with what they see |
//! | `TMPDIR` | a build that writes scratch files puts them where the user's system says, not where the harness's cleared environment implies |
//!
//! Each is carried **only when the harness's own process has it**, so an
//! absent `PATH` produces a `CouldNotStart` naming the program rather than an
//! empty string that resolves surprisingly.
//!
//! The set is the coordinator's ruling of 2026-09-05 transcribed and is
//! recorded on ADR-0011 D2 as an accepted Update. Nothing here chose it.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use core::fmt;
use std::collections::BTreeMap;

/// The prefix [ADR-0014] D1's layer 4 owns, which no child may be given.
///
/// One constant, read by the refusal and by the check that plants one, so
/// that "the harness's own configuration does not reach a child" cannot be
/// true in one place and false in the other.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const HARNESS_PREFIX: &str = "ZARU_";

/// The names [`Environment::inherited_minimum`] carries, in the order the
/// ruling names them.
///
/// A hand-written list, and a check asserts that an environment built from it
/// holds exactly the subset the harness's own process has — so a sixth name
/// added here without a reason on the record is a visible change.
pub const MINIMUM: [&str; 5] = ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"];

/// Why a name or a value is not something a child may be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotForAChild {
    /// The name is [ADR-0014] D1's layer 4, which is the harness's own.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    HarnessOwned {
        /// The name that was offered, escaped.
        offered: String,
    },
    /// The name was empty.
    EmptyName,
    /// The name carried `=` or a NUL, neither of which an environment can
    /// express.
    ///
    /// Refused rather than passed on, because `A=B` as a *name* is a second
    /// assignment smuggled through the first, and a NUL truncates whatever
    /// reads it.
    UnnameableName {
        /// The name that was offered, escaped.
        offered: String,
    },
    /// The value carried a NUL.
    UnnameableValue {
        /// The name whose value was refused.
        name: String,
    },
}

impl fmt::Display for NotForAChild {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HarnessOwned { offered } => write!(
                f,
                // ADR-0014 D1's layer 4 is the prefixed environment.
                "the environment variable {offered} belongs to the harness: \
                 {HARNESS_PREFIX}-prefixed configuration is read by Zaru itself, and handing it to \
                 a child would let a command the model chose read the harness's own settings back \
                 out with `env`",
            ),
            Self::EmptyName => f.write_str(
                "an environment variable with no name cannot be set, and nothing could read it",
            ),
            Self::UnnameableName { offered } => write!(
                f,
                "the environment variable name {offered} carries a `=` or a NUL, which an \
                 environment cannot express; a `=` inside a name is a second assignment smuggled \
                 through the first",
            ),
            Self::UnnameableValue { name } => write!(
                f,
                "the value offered for {name:?} carries a NUL, which truncates whatever reads it",
            ),
        }
    }
}

impl std::error::Error for NotForAChild {}

/// Exactly what a child process is given, and nothing else.
///
/// Ordered, because two runs of one command should hand the child the same
/// bytes in the same order — a `BTreeMap` makes that a property of the type
/// rather than of whatever order a caller happened to build it in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    held: BTreeMap<String, String>,
}

impl Environment {
    /// An environment holding nothing.
    ///
    /// The starting point for every other constructor, so "cleared, then set"
    /// is the only shape there is.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Carry one name and value.
    ///
    /// # Errors
    ///
    /// [`NotForAChild`], which is where the [`HARNESS_PREFIX`] rule lives.
    pub fn carrying(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, NotForAChild> {
        let name = name.into();
        let value = value.into();
        if name.is_empty() {
            return Err(NotForAChild::EmptyName);
        }
        if name.starts_with(HARNESS_PREFIX) {
            return Err(NotForAChild::HarnessOwned {
                offered: name.escape_debug().to_string(),
            });
        }
        if name.contains('=') || name.contains('\0') {
            return Err(NotForAChild::UnnameableName {
                offered: name.escape_debug().to_string(),
            });
        }
        if value.contains('\0') {
            return Err(NotForAChild::UnnameableValue { name });
        }
        self.held.insert(name, value);
        Ok(self)
    }

    /// The five names of [`MINIMUM`] the harness's own environment has.
    ///
    /// A name the harness does not itself have is not carried, rather than
    /// carried empty: an empty `PATH` resolves differently from an absent one
    /// and neither is what the user's shell would do.
    ///
    /// **The harness's own environment is `variables`, since 2026-09-27**,
    /// and was the process's, read here with `std::env::var` — so a caller
    /// holding other variables still handed a child the developer's `PATH`
    /// and `HOME`. It is what the binary's `main` read once. See
    /// [`crate::config::Variables`].
    ///
    /// # Errors
    ///
    /// [`NotForAChild`] when one of the harness's own values cannot be passed
    /// on — a NUL in a value, which the operating system should not produce
    /// and which is reported rather than dropped.
    pub fn inherited_minimum(variables: &crate::config::Variables) -> Result<Self, NotForAChild> {
        let mut environment = Self::empty();
        for name in MINIMUM {
            if let Some(value) = variables.get(name) {
                environment = environment.carrying(name, value)?;
            }
        }
        Ok(environment)
    }

    /// Every name and value, in name order.
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.held
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    /// How many names are carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether nothing is carried.
    ///
    /// An empty environment is a real thing to want — a command that must see
    /// nothing at all — so it is a value rather than a refusal.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}
