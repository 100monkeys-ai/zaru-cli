// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staged implementations of the three validator ports.
//!
//! These are the test tree. Nothing here has a counterpart in the product
//! tree, **nothing here spawns a process**, compiles a pattern or opens a
//! file, and every answer is one a check staged.
//!
//! Three properties are load-bearing and must survive anybody tidying this
//! file up.
//!
//! **Every staged stream carries a nonce, a newline and a non-ASCII
//! character**, so text arriving anywhere downstream can only have got there
//! by being carried — an implementation that hard-coded a plausible failure
//! string could not produce it ([Verification lessons] §9).
//!
//! **The staged standard error ends with a trailing space.** That is not an
//! accident and it is not tidiable. A fixture whose awkwardness is all in its
//! encoding is awkward on the encoding axis and perfectly ordinary on the
//! whitespace axis, so a mutation that trims the captured output would survive
//! it — [Verification lessons] §51, which was paid for on 2026-09-04 by
//! exactly that mutation surviving exactly that fixture.
//!
//! **A runner asked for a command nothing staged panics** rather than
//! returning a failure. A fixture that answered anyway would let a check pass
//! over a dispatch asking for the wrong command, and a staging failure that
//! renders as a port failure is [Verification lessons] §4's vacuous pass.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::iteration::port::PortFailure;
use crate::iteration::validator::name::{Pattern, Run, SchemaPath};
use crate::iteration::validator::port::{
    PatternMatch, SchemaValidate, ValidatorOutput, ValidatorRunner,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A nonce no implementation could produce without carrying it.
pub(super) const NONCE: &str = "zaru-validator-nonce-4c7e";

/// The standard output a command staged for `label` produces.
pub(super) fn stdout_for(label: &str) -> String {
    format!("{NONCE}\n{label}: assertion failed — left ≠ right\n")
}

/// The standard error a command staged for `label` produces.
///
/// **Ends with a trailing space, deliberately.** See the module documentation.
pub(super) fn stderr_for(label: &str) -> String {
    format!("{NONCE}-stderr\n{label}: 1 error emitted — ✗ \n")
}

/// A staged command result carrying both streams and an exit code.
pub(super) fn output_for(label: &str, exit_code: i32) -> ValidatorOutput {
    ValidatorOutput {
        exit_code,
        stdout: stdout_for(label),
        stderr: stderr_for(label),
    }
}

/// Runs commands from a table a check staged, and records what it was asked.
#[derive(Debug, Default)]
pub(super) struct StagedRunner {
    outputs: BTreeMap<String, ValidatorOutput>,
    fails_for: Option<String>,
    asked: Mutex<Vec<String>>,
}

impl StagedRunner {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Stage what one command produces.
    pub(super) fn staging(mut self, command: &Run, output: ValidatorOutput) -> Self {
        self.outputs.insert(command.as_str().to_owned(), output);
        self
    }

    /// Make one command fail to run at all.
    pub(super) fn failing_for(mut self, command: &Run) -> Self {
        self.fails_for = Some(command.as_str().to_owned());
        self
    }

    /// The commands this runner was asked for, in order.
    ///
    /// **This is the reading that separates a skipped validator from one whose
    /// result was thrown away.** Both report `Skipped`; only one of them is
    /// absent from this list.
    pub(super) fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("asked poisoned").clone()
    }
}

impl ValidatorRunner for StagedRunner {
    async fn run(&self, command: &Run) -> Result<ValidatorOutput, PortFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push(command.as_str().to_owned());
        if self.fails_for.as_deref() == Some(command.as_str()) {
            return Err(PortFailure::new(format!(
                "{NONCE} could not run {:?}",
                command.as_str()
            )));
        }
        self.outputs.get(command.as_str()).cloned().map_or_else(
            || {
                panic!(
                    "the dispatch asked this runner for {:?}, which no check staged; staged: {:?}",
                    command.as_str(),
                    self.outputs.keys().collect::<Vec<_>>()
                )
            },
            Ok,
        )
    }
}

/// Answers `matches` from a table a check staged.
#[derive(Debug, Default)]
pub(super) struct StagedPattern {
    answers: BTreeMap<String, bool>,
    fails: bool,
    asked: Mutex<Vec<(String, String)>>,
}

impl StagedPattern {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn answering(mut self, pattern: &Pattern, matched: bool) -> Self {
        self.answers.insert(pattern.as_str().to_owned(), matched);
        self
    }

    pub(super) fn failing() -> Self {
        Self {
            fails: true,
            ..Self::default()
        }
    }

    /// The pattern and the standard output this port was handed, per call.
    pub(super) fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().expect("asked poisoned").clone()
    }
}

impl PatternMatch for StagedPattern {
    async fn matches(&self, pattern: &Pattern, stdout: &str) -> Result<bool, PortFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push((pattern.as_str().to_owned(), stdout.to_owned()));
        if self.fails {
            return Err(PortFailure::new(format!(
                "{NONCE} {:?} is not a usable pattern",
                pattern.as_str()
            )));
        }
        self.answers.get(pattern.as_str()).copied().map_or_else(
            || {
                panic!(
                    "the dispatch asked this port about {:?}, which no check staged",
                    pattern.as_str()
                )
            },
            Ok,
        )
    }
}

/// Answers `json_schema` from a table a check staged.
#[derive(Debug, Default)]
pub(super) struct StagedSchema {
    answers: BTreeMap<String, bool>,
    fails: bool,
    asked: Mutex<Vec<(String, String)>>,
}

impl StagedSchema {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn answering(mut self, schema: &SchemaPath, validated: bool) -> Self {
        self.answers.insert(schema.as_str().to_owned(), validated);
        self
    }

    pub(super) fn failing() -> Self {
        Self {
            fails: true,
            ..Self::default()
        }
    }

    /// The schema path and the standard output this port was handed, per call.
    pub(super) fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().expect("asked poisoned").clone()
    }
}

impl SchemaValidate for StagedSchema {
    async fn validates(&self, schema: &SchemaPath, stdout: &str) -> Result<bool, PortFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push((schema.as_str().to_owned(), stdout.to_owned()));
        if self.fails {
            return Err(PortFailure::new(format!(
                "{NONCE} the schema at {:?} could not be read",
                schema.as_str()
            )));
        }
        self.answers.get(schema.as_str()).copied().map_or_else(
            || {
                panic!(
                    "the dispatch asked this port about {:?}, which no check staged",
                    schema.as_str()
                )
            },
            Ok,
        )
    }
}

/// A validator named `name` running `run <name>` and expecting exit zero.
pub(super) fn declared(name: &str) -> crate::iteration::validator::declaration::Declared {
    use crate::iteration::validator::declaration::Declared;
    use crate::iteration::validator::expectation::Expect;
    use crate::iteration::validator::name::Name;
    Declared::new(
        Name::new(name).expect("a fixture name"),
        Run::new(command_for(name)).expect("a fixture command"),
        Expect::ExitZero,
    )
}

/// The command a fixture validator named `name` runs.
pub(super) fn command_for(name: &str) -> String {
    format!("run-{name}")
}

/// A fixture validator's name, for building an `after` list.
pub(super) fn name(name: &str) -> crate::iteration::validator::name::Name {
    crate::iteration::validator::name::Name::new(name).expect("a fixture name")
}
