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
//! # Both futures are real, since 2026-09-05
//!
//! Each port's method returns `impl Future + Send`, and each implementation
//! here used to do the whole thing synchronously and hand back a future that
//! was already resolved — so the calling thread blocked for as long as the
//! child ran, up to the ceiling, and the session's one current-thread runtime
//! went with it. **Neither does now**: [`Spawn::execute`] is `async` and the
//! child, its two pipes and the ceiling are futures the runtime polls, so a
//! turn racing this against a terminal keeps repainting and stays
//! interruptible while `cmd.run` or a declared validator's command runs.
//! **Neither trait declaration moved**, which is the measure of how much of
//! this was a body rather than a contract.
//!
//! The reason recorded for not doing it earlier was that closing the gap
//! needed `tokio::task::spawn_blocking` and therefore "a runtime with a
//! blocking pool, which is a second runtime". **Measured on 2026-09-05, that
//! is wrong twice over.** A current-thread runtime *has* a blocking pool —
//! `Builder::new_current_thread().enable_all()` runs a blocking closure on a
//! `tokio-rt-worker` thread while the beat keeps firing — so no second runtime
//! was ever needed. And `spawn_blocking` is nonetheless the wrong shape, for a
//! reason nothing had stated: **a blocking task is not cancellable.** Dropping
//! its handle, which is exactly what dropping the turn's future does, does not
//! stop the closure; measured against a three-second child, the interrupt
//! returned in 334 ms with the child still running, and `Runtime::drop` then
//! blocked for **2.67 s** waiting for it — which against this workspace's
//! two-minute process ceiling is a `Ctrl-C` that leaves the build running and
//! hangs the harness on exit. The corrections are on [ADR-0011], [ADR-0009]
//! and [ADR-0008]'s D2 amendment in those records' own words.
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
use crate::process::spawn::{Outcome, Spawn, SpawnFailure};
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
    async fn declared(&self, command: &Run) -> Result<ValidatorOutput, PortFailure> {
        let line = CommandLine::split(command.as_str())
            .map_err(|refused| PortFailure::new(refused.to_string()))?;
        let outcome = self
            .execute(&line)
            .await
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
        self.declared(command)
    }
}

impl Subprocess for Spawn<'_> {
    /// `async fn` against a trait that declares `impl Future + Send`, which is
    /// the same signature: the compiler checks the `Send` bound on the future
    /// this produces, so nothing is weakened by spelling it the short way.
    async fn run(&self, line: &CommandLine) -> Result<Captured, PortFailure> {
        match self.execute(line).await {
            Ok(outcome) => Ok(captured(outcome)),
            // A missing program is a property of the command the model chose,
            // not an infrastructure failure. Keep it in the ordinary tool
            // result channel so the model sees the actionable PATH/install
            // explanation and can choose another command on its next exchange.
            // `127` is the shell convention for a command that was not found.
            Err(failure @ SpawnFailure::CouldNotStart { .. }) => Ok(Captured {
                exit_code: 127,
                stdout: String::new(),
                stderr: failure.to_string(),
            }),
            Err(failure @ SpawnFailure::Lost { .. }) => Err(PortFailure::new(failure.to_string())),
        }
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
