// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What each request does, and what the process exits with.
//!
//! # This module writes nothing
//!
//! It returns lines and an [`Exit`], and the binary writes them. That keeps
//! the whole of [ADR-0015]'s out-of-session surface reachable from a check
//! that is an ordinary caller, and it keeps the one `println!` in this crate
//! in `main.rs` where the composition root already is. The real evidence for
//! this surface is `tests/cli_from_outside.rs`, which runs the built binary;
//! this shape is what makes the *unit* half possible at all.
//!
//! # Every exit code the binary can reach comes from here
//!
//! [ADR-0016] D5's mapping was already built and, until 2026-09-05, only `0`
//! was observable from the real artefact — "`zaru` takes no arguments, so
//! nothing a user can do makes it fail". Something can now. What became
//! observable is `0` and `2`; `1`, `3`, `4` and `70` did not, because there
//! is still no loop to exhaust, no network to be unreachable, no tier that
//! withholds anything, and no honest way to make the binary panic.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::classify::Surface;
use crate::cli::invocation::{CommandLine, Overrides, Request};
use crate::cli::{help, layers, render};
use crate::config::{Key, Resolution};
use crate::failure::{Classified, Exit, SessionEvidence};
use crate::providers::ModelTable;
use crate::runtime::{ResolvedTier, Runtime};

/// What one run produced.
///
/// `lines` is standard output; `exit` is [ADR-0016] D5's code. A failed run may
/// still have lines — a partial listing is worth more than nothing — though
/// nothing this surface does produces both today.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub struct Outcome {
    /// What to write to standard output.
    pub lines: Vec<String>,
    /// What the process exits with.
    pub exit: Exit,
}

impl Outcome {
    /// A run that did what was asked.
    fn printed(lines: Vec<String>) -> Self {
        Self {
            lines,
            exit: Exit::Succeeded,
        }
    }

    /// A run that did not.
    fn failed(classified: Classified) -> Self {
        Self {
            lines: Vec::new(),
            exit: Exit::Failed(classified),
        }
    }
}

/// Everything one invocation of the binary needs from outside itself.
///
/// The version and the report URL are read out of the binary's own package
/// metadata by its `main` rather than by this module's `env!`, for the reason
/// `composition()` already reads the crate names out of the crates: a value
/// retyped beside the binary is a value that drifts.
pub struct Run<'a> {
    /// The harness version.
    pub version: &'a str,
    /// Where a defect is reported.
    pub report_at: &'a str,
}

impl Run<'_> {
    /// Do what the command line asked.
    #[must_use]
    pub fn execute(&self, line: &CommandLine) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        match &line.request {
            Request::Help => Outcome::printed(help::lines(self.version)),
            Request::Version => Outcome::printed(version_lines(self.version)),
            Request::Runtime => {
                self.configured(
                    &line.overrides,
                    |resolution| match ResolvedTier::from_configuration(resolution) {
                        Ok(resolved) => Outcome::printed(render::runtime(&Runtime::of(resolved))),
                        Err(refusal) => Outcome::failed(surface.tier(&refusal)),
                    },
                )
            }
            Request::Models => {
                self.configured(
                    &line.overrides,
                    |resolution| match ModelTable::from_configuration(resolution) {
                        Ok(table) => Outcome::printed(render::models(&table)),
                        Err(refusal) => Outcome::failed(Classified::from(refusal)),
                    },
                )
            }
            Request::ConfigExplain { key } => {
                self.configured(&line.overrides, |resolution| explain(resolution, key))
            }
            Request::SessionsList
            | Request::SessionsRemove { .. }
            | Request::NotesTokens
            | Request::Resume { .. }
            | Request::Continue
            | Request::Task { .. } => Outcome::printed(vec![
                "not reached in this commit; the surrounding arms land with their own checks"
                    .to_owned(),
            ]),
        }
    }

    /// Fold the configuration, then do something with it.
    ///
    /// One place, so every command that needs configuration reads the same
    /// three layers in the same order and a refusal from the fold has one
    /// classification rather than one per command.
    fn configured(
        &self,
        overrides: &Overrides,
        then: impl FnOnce(&Resolution) -> Outcome,
    ) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        match layers::resolve_from_process(overrides) {
            Ok(resolution) => then(&resolution),
            Err(failure) => {
                Outcome::failed(surface.load(&failure, SessionEvidence::NoSessionExists))
            }
        }
    }
}

/// What `--version` prints.
///
/// The composition list, which a bare `zaru` printed until 2026-09-05. It is
/// behind the flag now because "what am I made of" is a question a user asks
/// deliberately, and because a bare `zaru` had to answer the question a user
/// asks by typing the name of a program and nothing else.
fn version_lines(version: &str) -> Vec<String> {
    let mut lines = vec![format!("zaru {version}")];
    for (name, crate_version) in crate::composition() {
        lines.push(format!("  {name} {crate_version}"));
    }
    lines
}

/// [ADR-0014] D3's block for one key, or a refusal for a key nothing declares.
///
/// **A key the schema does not carry is refused rather than explained.**
/// Explaining it would print five `(not set)` rows, which reads as "this key
/// exists and nobody has set it" — D5's silent-typo failure with a block on
/// top, and the failure that clause exists to prevent. Ruled 2026-09-05.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn explain(resolution: &Resolution, key: &Key) -> Outcome {
    let schema = layers::schema();
    if schema.field(key).is_none() {
        return Outcome::failed(Surface::undeclared_key(key, &schema));
    }
    Outcome::printed(render::explanation(&resolution.explain(key)))
}
