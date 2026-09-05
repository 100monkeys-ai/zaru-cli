// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two ports one [`Spawn`] answers, and the one thing each of them loses.
//!
//! # Two ports, one process, and that is the point
//!
//! [`ValidatorRunner`] is [ADR-0009] D3's — a command a **project declared**,
//! in a file the user can read. [`Subprocess`] is [ADR-0011] D1's `cmd.run` —
//! a command **a model chose**, decided by that record's permission model
//! first. `zaru-core`'s own port module is explicit that they are not one
//! port and must not become one, and nothing here merges them: they are two
//! trait implementations over one private `execute`, so the *decision* stays
//! in two places and only the *mechanism* is shared.
//!
//! # Both futures are already finished
//!
//! Each port's method returns `impl Future + Send`, and each implementation
//! here does the whole thing synchronously and hands back a future that is
//! already resolved. **So the calling thread blocks for as long as the child
//! runs, up to the ceiling.** `zaru-cli`'s product tree carries no async
//! runtime — `tokio` is a dev-dependency — and taking one to make this
//! genuinely asynchronous is a reactor in the binary, which is a decision with
//! a record's name on it rather than an import. Recorded on [ADR-0008]'s and
//! [ADR-0009] D3's Status tracking under the coordinator's ruling of
//! 2026-09-05 rather than left for a reader to discover from a stall.
//!
//! # What both output types cannot say, which is a finding
//!
//! [`Ended`](super::Ended) distinguishes a child the harness killed at its
//! ceiling from one the work's own tooling killed. **Neither
//! [`ValidatorOutput`] nor [`Captured`] has anywhere to put that**: both carry
//! one `i32`, and a ceiling kill and a `SIGKILL` from anything else are both
//! `137`. Appending a sentence of the harness's own to a captured stream was
//! refused, because [ADR-0009] D5 sends those bytes into refinement "verbatim"
//! and [ADR-0011] D5 sends them to the model, and a harness sentence inside
//! them is the paraphrase [ADR-0008] D4 forbids arriving through a side door.
//! **Raised on ADR-0009 and ADR-0011 for their authors** — a refinement prompt
//! that cannot tell "your tests hung" from "your tests were killed" is a
//! prompt that teaches the model the wrong thing, and the room for it has to
//! be made in the record's own shape.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::process::line::CommandLine;
use crate::process::spawn::{Outcome, Spawn};
use crate::tools::output::Captured;
use crate::tools::port::Subprocess;
use core::future::Future;
use zaru_core::iteration::PortFailure;
use zaru_core::iteration::validator::{Run, ValidatorOutput, ValidatorRunner};

impl Spawn<'_> {
    /// Run a declared `run` command, splitting it first.
    ///
    /// A split refusal is a [`PortFailure`] because that is what
    /// [`ValidatorRunner`]'s contract calls **the declaration being
    /// unusable** — the same register as a pattern that is not a regular
    /// expression — rather than a validator that failed.
    fn declared(&self, command: &Run) -> Result<ValidatorOutput, PortFailure> {
        let line = CommandLine::split(command.as_str())
            .map_err(|refused| PortFailure::new(refused.to_string()))?;
        let outcome = self
            .execute(&line)
            .map_err(|failure| PortFailure::new(failure.to_string()))?;
        Ok(ValidatorOutput {
            exit_code: outcome.ended.exit_code(),
            stdout: outcome.stdout,
            stderr: outcome.stderr,
        })
    }
}

impl ValidatorRunner for Spawn<'_> {
    fn run(
        &self,
        command: &Run,
    ) -> impl Future<Output = Result<ValidatorOutput, PortFailure>> + Send {
        core::future::ready(self.declared(command))
    }
}

impl Subprocess for Spawn<'_> {
    fn run(
        &self,
        line: &CommandLine,
    ) -> impl Future<Output = Result<Captured, PortFailure>> + Send {
        core::future::ready(
            self.execute(line)
                .map(captured)
                .map_err(|failure| PortFailure::new(failure.to_string())),
        )
    }
}

/// One outcome as [ADR-0011] D5's three parts.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
fn captured(outcome: Outcome) -> Captured {
    Captured {
        exit_code: outcome.ended.exit_code(),
        stdout: outcome.stdout,
        stderr: outcome.stderr,
    }
}
