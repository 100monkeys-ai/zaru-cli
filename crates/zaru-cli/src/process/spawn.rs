// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one place this workspace starts a child process.
//!
//! # What it does, in the order it does it
//!
//! 1. The working directory is [ADR-0011] D4's boundary root, which
//!    [`WorkingDirectory`] canonicalised once at construction. **That is the
//!    only containment this type claims**, and at `bare` it is not
//!    containment at all — see below.
//! 2. The environment is **cleared** and then set from an [`Environment`], so
//!    what a child sees is a list somebody wrote rather than whatever the
//!    harness happened to be started with.
//! 3. Standard input is `/dev/null`. **No record names stdin**, and the
//!    reason it is closed is that a child reading an inherited terminal
//!    blocks the whole harness waiting for input the user does not know is
//!    wanted; a read of `/dev/null` returns end of file, which every program
//!    already handles. Recorded on ADR-0011 D2 under the coordinator's ruling
//!    of 2026-09-05.
//! 4. Standard output and standard error are separate pipes, each **read by a
//!    future of its own**, beside a future waiting for the child and a future
//!    holding the ceiling. All of them are polled by the runtime the session
//!    already has, and **this call creates no thread at all**.
//!
//! # Nothing here blocks the runtime, and that is the whole point of the shape
//!
//! Until 2026-09-05 this was a `std::process::Command`, `try_wait` in a loop
//! with a one-millisecond sleep, and two reader threads joined at the end. It
//! is reached from two asynchronous ports — [`Subprocess`](crate::tools::Subprocess)
//! and [`ValidatorRunner`](zaru_core::iteration::validator::ValidatorRunner) —
//! so for the whole of a `cmd.run` or a declared validator's command the
//! session's one current-thread runtime was blocked: no beat fired, no
//! keystroke was read, and a `Ctrl-C` was not seen until the child returned or
//! the ceiling killed it. That gap was recorded on [ADR-0011] and [ADR-0009]
//! by the arc that found it and is closed here.
//!
//! **The readers still run beside the wait, for the reason they always did.**
//! `Child::wait_with_output` has no timeout, so it cannot honour a ceiling;
//! waiting *without* draining deadlocks the moment the child fills a pipe
//! buffer — which is every build that logs — because the child blocks in
//! `write` and therefore never exits, and the wait waits forever for a child
//! that is waiting for it. What changed is that the three are futures in one
//! `select!` rather than two threads and a sleeping poll.
//!
//! # An interrupt ends the child, because the child is a value the future owns
//!
//! `kill_on_drop(true)`, so dropping this future — which is what a mid-turn
//! `Ctrl-C` does, [ADR-0010] D2's "at most the event in flight" arriving
//! without a crash — sends the child `SIGKILL` and hands it to the runtime's
//! own reaper. **The signal is the one the ceiling already sends**: `kill` is
//! `SIGKILL` here as it is there, and measured against a child running
//! `trap '' TERM INT` both leave the process table and neither becomes a
//! zombie. Without the flag the child *survives* the runtime being dropped,
//! which is a `sleep 30` still running after the shell has gone. No record
//! names a signal for an interrupt and none names a wait after one; this
//! follows the ceiling's own precedent and waits for nothing, recorded as an
//! amendment on [ADR-0011] and [ADR-0009] rather than decided here.
//!
//! # A limit this type does not close, stated rather than discovered
//!
//! A child that leaves a grandchild holding the pipes keeps them open after it
//! is killed, and the readers then wait past the ceiling for an end of file
//! that has not come. **That is unchanged**: the ceiling bounds the child and
//! not the capture, exactly as the two joins after the wait did. What is new
//! is that the wait is now something an interrupt can drop, where nothing
//! could reach it before. **Nothing here contains a grandchild**, because at
//! `bare` nothing contains anything: [ADR-0011] D2 — "the harness is not a
//! sandbox and says so". At `contained` the membrane is [ADR-0004]'s and is
//! the answer that does not depend on a process being well behaved. Raised on
//! ADR-0011 as a finding rather than papered over, in the same shape as that
//! record's existing note about the boundary being a check at a moment.
//!
//! # What is the work's and what is the harness's
//!
//! A non-zero exit is **the work's**, and it is an `i32`
//! ([`Ended::exit_code`]). [`Class::exit_code`](crate::failure::Class::exit_code)
//! is the *harness's* own `u8` and is a different quantity with a different
//! width so that one cannot be passed for the other — [ADR-0016] D5. A
//! command that could not be started at all is neither: it is
//! [`SpawnFailure::CouldNotStart`], which D1 row 2 makes user-correctable and
//! which names the program.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::process::ceiling::ProcessCeiling;
use crate::process::environment::Environment;
use crate::process::line::CommandLine;
use crate::tools::tree::WorkingDirectory;
use core::fmt;
use core::time::Duration;
use std::process::{ExitStatus, Stdio};
use std::time::Instant;
use tokio::io::AsyncReadExt as _;
use tokio::process::Child;

/// What the shell convention adds to a signal number to make an exit code.
///
/// Transcribed rather than invented: it is what `$?` reports for a signalled
/// child, so a user running the same command in their own shell sees the same
/// number the harness reports. Recorded on [ADR-0009] D3, whose
/// `expect = "exit-code = N"` compares against it.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const SIGNALLED_EXIT_BASE: i32 = 128;

/// How a child process finished.
///
/// **Three variants and not two.** A child the user's own build killed and a
/// child this harness killed both terminate by signal, and a type that could
/// not tell them apart would report the harness's ceiling as the work's own
/// failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// It exited on its own, with this code.
    Exited {
        /// What it reported.
        code: i32,
    },
    /// It was terminated by a signal that this harness did not send.
    Signalled {
        /// Which signal.
        signal: i32,
    },
    /// The ceiling was reached and the harness killed it.
    ///
    /// **An outcome, never an error.** The mechanism did exactly what the
    /// caller's ceiling asked for, and [ADR-0016] D1 row 1 puts that in the
    /// expected register.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    KilledAtTheCeiling {
        /// How long it had been running when the ceiling was reached.
        after: Duration,
        /// The signal the kill actually delivered, read off the status
        /// rather than assumed.
        signal: i32,
    },
}

impl Ended {
    /// The work's exit code, as an `i32`.
    ///
    /// A signalled child reports [`SIGNALLED_EXIT_BASE`] plus its signal,
    /// which is the shell convention and is what `$?` gives. A ceiling kill
    /// is a signalled child, so it reports that too — the distinction the
    /// enum keeps is deliberately not carried into the number, because
    /// [`ValidatorOutput`](zaru_core::iteration::validator::ValidatorOutput)
    /// and [`Captured`](crate::tools::Captured) both have one `i32` and no
    /// room for it.
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Exited { code } => *code,
            Self::Signalled { signal } | Self::KilledAtTheCeiling { signal, .. } => {
                SIGNALLED_EXIT_BASE + *signal
            }
        }
    }

    /// Whether the harness stopped it rather than the work finishing.
    #[must_use]
    pub const fn was_killed_at_the_ceiling(&self) -> bool {
        matches!(self, Self::KilledAtTheCeiling { .. })
    }
}

impl fmt::Display for Ended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited { code } => write!(f, "exited {code}"),
            Self::Signalled { signal } => {
                write!(
                    f,
                    "terminated by signal {signal} (exit code {})",
                    self.exit_code()
                )
            }
            Self::KilledAtTheCeiling { after, signal } => write!(
                f,
                "killed after {after:?} because the wall-clock ceiling was reached; the kill \
                 delivered signal {signal}, so the exit code is {}",
                self.exit_code()
            ),
        }
    }
}

/// A child process could not be run, or could not be followed once it was.
///
/// Two variants and both are mapped in [`classify`](crate::failure::classify),
/// because [ADR-0016]'s Status tracking makes a partial mapping "a hole with a
/// comment on it".
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub enum SpawnFailure {
    /// The program could not be started.
    ///
    /// **The user's**, whatever the operating system said. No match over
    /// `io::ErrorKind` is made — that type is `#[non_exhaustive]` and no
    /// wildcard-free match over it is possible — and none is needed, because
    /// the thing the user can change is the same in every case: the command
    /// they declared or the model asked for names a program that did not
    /// start.
    CouldNotStart {
        /// The program, exactly as the command line spelled it.
        program: String,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The child started and the harness then lost track of it.
    ///
    /// **Ours.** Waiting on a child, killing one, or reading a pipe the
    /// harness itself opened are things the harness arranged, so a failure in
    /// any of them is a defect rather than anything the user did.
    Lost {
        /// The program, exactly as the command line spelled it.
        program: String,
        /// What went wrong, in the operating system's or the thread's own
        /// words.
        detail: String,
    },
}

impl fmt::Display for SpawnFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CouldNotStart { program, source } => write!(
                f,
                "the program {program:?} could not be started: {source}. ADR-0011 D1 names no \
                 allowlist of programs, so a bare name is whatever PATH finds — check that \
                 {program:?} is installed and that PATH reaches it",
            ),
            Self::Lost { program, detail } => write!(
                f,
                "the harness started {program:?} and then lost track of it: {detail}",
            ),
        }
    }
}

impl std::error::Error for SpawnFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CouldNotStart { source, .. } => Some(source),
            Self::Lost { .. } => None,
        }
    }
}

/// What one child process produced.
///
/// Deliberately neither [`Captured`](crate::tools::Captured) nor
/// [`ValidatorOutput`](zaru_core::iteration::validator::ValidatorOutput):
/// this type carries the [`Ended`] the other two have no room for, and
/// [`Spawn`] converts to each of them at its own port. The duplication those
/// two already record as the cost of a boundary is paid here, once, in the
/// one place that has both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// How it finished.
    pub ended: Ended,
    /// Everything it wrote to standard output.
    pub stdout: String,
    /// Everything it wrote to standard error.
    pub stderr: String,
    /// How long it ran, measured by the harness's own clock.
    pub took: Duration,
}

/// Runs a child process, inside the working directory, under a ceiling.
///
/// It is `Sync`, which both ports require: everything it holds is either a
/// shared reference to an immutable path or an owned map of strings.
#[derive(Debug, Clone)]
pub struct Spawn<'a> {
    working_directory: &'a WorkingDirectory,
    environment: Environment,
    ceiling: ProcessCeiling,
}

impl<'a> Spawn<'a> {
    /// Run children in this directory, with this environment, under this
    /// ceiling.
    ///
    /// Every one of the three is the caller's: the boundary root because
    /// [ADR-0011] D4 makes it the thing every call is measured against, the
    /// environment because deciding what a model-driven command may read is
    /// deciding what a model-driven action may reach, and the ceiling because
    /// no record names a number.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub const fn new(
        working_directory: &'a WorkingDirectory,
        environment: Environment,
        ceiling: ProcessCeiling,
    ) -> Self {
        Self {
            working_directory,
            environment,
            ceiling,
        }
    }

    /// The directory every child starts in.
    #[must_use]
    pub const fn working_directory(&self) -> &WorkingDirectory {
        self.working_directory
    }

    /// The ceiling every child runs under.
    #[must_use]
    pub const fn ceiling(&self) -> ProcessCeiling {
        self.ceiling
    }

    /// What a child would be given.
    #[must_use]
    pub const fn environment(&self) -> &Environment {
        &self.environment
    }

    /// Run one command line and report what it produced.
    ///
    /// **Nothing here blocks the thread it is polled on.** The wait, the two
    /// reads and the ceiling are four futures in one `select!`, so a caller
    /// racing this against a terminal — which is what
    /// [`crate::terminal::driver::race`] does — keeps repainting and keeps
    /// reading keys for as long as the child runs. Dropping this future ends
    /// the child; see the module documentation.
    ///
    /// # Errors
    ///
    /// [`SpawnFailure::CouldNotStart`] when the program will not start, and
    /// [`SpawnFailure::Lost`] when the harness cannot follow a child it did
    /// start. A command that ran and failed is `Ok`, carrying its [`Ended`].
    pub async fn execute(&self, line: &CommandLine) -> Result<Outcome, SpawnFailure> {
        let mut command = tokio::process::Command::new(line.program());
        command
            .args(line.arguments())
            .current_dir(self.working_directory.root())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The whole of what an interrupt is. See the module
            // documentation: no record names a signal, and this is the one
            // the ceiling below already sends.
            .kill_on_drop(true);
        for (name, value) in self.environment.pairs() {
            command.env(name, value);
        }

        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|source| SpawnFailure::CouldNotStart {
                program: line.program().to_owned(),
                source,
            })?;

        // Taken before the wait begins, because a pipe nobody is reading is
        // a child nobody can wait for.
        let mut out = child.stdout.take().expect("stdout was piped above");
        let mut err = child.stderr.take().expect("stderr was piped above");

        let (ended, captured) = {
            let draining = async move {
                let mut on_out = Vec::new();
                let mut on_err = Vec::new();
                // Concurrently, not one after the other: a child that fills
                // its error pipe while this read its output would block in
                // `write` and never reach end of file on either.
                let _ = tokio::join!(out.read_to_end(&mut on_out), err.read_to_end(&mut on_err));
                (on_out, on_err)
            };
            tokio::pin!(draining);

            let (ended, taken) = self
                .wait(&mut child, line.program(), started, draining.as_mut())
                .await?;

            // The readers run to end of file after the child has ended,
            // exactly as the two threads were joined after the wait: the
            // ceiling bounds the child and never the capture. A grandchild
            // holding the pipes is what that costs, and the module
            // documentation says so. A `draining` that already finished
            // beside the wait is never polled again, which is what `taken`
            // carries out of it.
            let captured = match taken {
                Some(captured) => captured,
                None => draining.await,
            };
            (ended, captured)
        };
        let took = started.elapsed();

        Ok(Outcome {
            ended,
            stdout: decoded(&captured.0),
            stderr: decoded(&captured.1),
            took,
        })
    }

    /// Wait for the child and drive the readers, killing it at the ceiling.
    ///
    /// `draining` is polled here as well as by the caller, because a child
    /// that fills a pipe buffer cannot exit until somebody empties it — which
    /// is the deadlock the two reader threads used to exist to prevent.
    async fn wait(
        &self,
        child: &mut Child,
        program: &str,
        started: Instant,
        mut draining: core::pin::Pin<&mut impl Future<Output = Captured>>,
    ) -> Result<(Ended, Option<Captured>), SpawnFailure> {
        let lost = |detail: String| SpawnFailure::Lost {
            program: program.to_owned(),
            detail,
        };
        let deadline = tokio::time::Instant::from_std(started + self.ceiling.get());
        let mut taken = None;

        // `biased`, so the polling order is a decision rather than a coin: the
        // child first, then the readers, then the ceiling. A child that has
        // already exited is not killed because a deadline in the same wake-up
        // was also ready.
        let status = loop {
            tokio::select! {
                biased;

                status = child.wait() => {
                    break Some(
                        status.map_err(|source| {
                            lost(format!("it could not be waited for: {source}"))
                        })?,
                    );
                }

                captured = &mut draining, if taken.is_none() => {
                    taken = Some(captured);
                }

                () = tokio::time::sleep_until(deadline) => break None,
            }
        };

        let Some(status) = status else {
            // The borrow the `select!` held on the child ends above, which is
            // what lets it be killed here rather than inside a branch.
            child
                .kill()
                .await
                .map_err(|source| lost(format!("it could not be killed: {source}")))?;
            let status = child.wait().await.map_err(|source| {
                lost(format!(
                    "it could not be waited for after the kill: {source}"
                ))
            })?;
            let after = started.elapsed();
            // A child that exited on its own in the moment between the
            // ceiling firing and the kill landing reports its own code,
            // because that is what happened. Only a signalled status is
            // this harness's doing.
            return Ok((
                match ending(&status) {
                    Ended::Exited { code } => Ended::Exited { code },
                    Ended::Signalled { signal } | Ended::KilledAtTheCeiling { signal, .. } => {
                        Ended::KilledAtTheCeiling { after, signal }
                    }
                },
                taken,
            ));
        };
        Ok((ending(&status), taken))
    }
}

/// What the two readers produced: standard output, then standard error.
type Captured = (Vec<u8>, Vec<u8>);

/// Decode one captured stream.
///
/// Lossy, for the reason `tools::execute`'s file reader is: a command's output
/// is bytes and a model is given text, and refusing a capture because a build
/// emitted one invalid sequence would lose the whole diagnosis over a byte.
fn decoded(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// How a status finished, as an [`Ended`].
///
/// Never [`Ended::KilledAtTheCeiling`]: only the waiting loop knows whether
/// the harness was the one that sent the signal, so this function reports what
/// the status says and the caller supplies that distinction.
fn ending(status: &ExitStatus) -> Ended {
    use std::os::unix::process::ExitStatusExt as _;
    status.code().map_or_else(
        // On Unix exactly one of the two is `Some`, so the fallback is
        // unreachable; it is written rather than unwrapped because a
        // classifier that panics on a status a child produced is a denial of
        // service with extra steps.
        || Ended::Signalled {
            signal: status.signal().unwrap_or_default(),
        },
        |code| Ended::Exited { code },
    )
}
