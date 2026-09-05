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
//! 4. Standard output and standard error are separate pipes, each **drained
//!    by its own thread**, while the calling thread polls for the child and
//!    for the ceiling.
//!
//! # The two reader threads are not a style choice
//!
//! `Child::wait_with_output` has no timeout, so it cannot honour a ceiling.
//! Polling `Child::try_wait` *without* draining deadlocks the moment the
//! child fills a pipe buffer — which is every build that logs — because the
//! child blocks in `write` and therefore never exits, and the poll waits
//! forever for a child that is waiting for the poll. One thread per stream is
//! the smallest shape that reads both while the ceiling runs, and it needs
//! nothing outside `std`.
//!
//! # A limit this type does not close, stated rather than discovered
//!
//! A child that leaves a grandchild holding the pipes keeps them open after it
//! is killed, and the reader threads then wait past the ceiling for an end of
//! file that has not come. **Nothing here contains a grandchild**, because at
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
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::process::ceiling::ProcessCeiling;
use crate::process::environment::Environment;
use crate::process::line::CommandLine;
use crate::tools::tree::WorkingDirectory;
use core::fmt;
use core::time::Duration;
use std::process::{Child, ExitStatus, Stdio};
use std::time::Instant;

/// How often the calling thread asks whether the child has finished.
///
/// The ceiling's precision is this interval, and a sleep of this length costs
/// one syscall — which is why it is short enough to make the ceiling mean
/// what it says rather than long enough to save a measurable amount of
/// anything.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

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
    /// Blocking, and the calling thread is the one that waits. See the module
    /// documentation on the two reader threads.
    ///
    /// # Errors
    ///
    /// [`SpawnFailure::CouldNotStart`] when the program will not start, and
    /// [`SpawnFailure::Lost`] when the harness cannot follow a child it did
    /// start. A command that ran and failed is `Ok`, carrying its [`Ended`].
    pub fn execute(&self, line: &CommandLine) -> Result<Outcome, SpawnFailure> {
        let mut command = std::process::Command::new(line.program());
        command
            .args(line.arguments())
            .current_dir(self.working_directory.root())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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

        // Taken before the poll begins, because a pipe nobody is reading is
        // a child nobody can wait for.
        let out = child.stdout.take().expect("stdout was piped above");
        let err = child.stderr.take().expect("stderr was piped above");
        let reading_out = std::thread::spawn(move || drain(out));
        let reading_err = std::thread::spawn(move || drain(err));

        let ended = self.wait(&mut child, line.program(), started)?;
        let took = started.elapsed();

        let stdout = joined(reading_out, line.program(), "standard output")?;
        let stderr = joined(reading_err, line.program(), "standard error")?;

        Ok(Outcome {
            ended,
            stdout,
            stderr,
            took,
        })
    }

    /// Wait for the child, killing it when the ceiling is reached.
    fn wait(
        &self,
        child: &mut Child,
        program: &str,
        started: Instant,
    ) -> Result<Ended, SpawnFailure> {
        let lost = |detail: String| SpawnFailure::Lost {
            program: program.to_owned(),
            detail,
        };
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(ending(&status)),
                Ok(None) => {}
                Err(source) => return Err(lost(format!("it could not be waited for: {source}"))),
            }
            if started.elapsed() >= self.ceiling.get() {
                child
                    .kill()
                    .map_err(|source| lost(format!("it could not be killed: {source}")))?;
                let status = child.wait().map_err(|source| {
                    lost(format!(
                        "it could not be waited for after the kill: {source}"
                    ))
                })?;
                let after = started.elapsed();
                // A child that exited on its own in the moment between the
                // ceiling firing and the kill landing reports its own code,
                // because that is what happened. Only a signalled status is
                // this harness's doing.
                return Ok(match ending(&status) {
                    Ended::Exited { code } => Ended::Exited { code },
                    Ended::Signalled { signal } | Ended::KilledAtTheCeiling { signal, .. } => {
                        Ended::KilledAtTheCeiling { after, signal }
                    }
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// Read one stream to end of file, keeping whatever arrived if it fails.
///
/// A read error mid-stream is reported as the bytes so far rather than as
/// nothing: a capture that lost its tail is still evidence, and a capture
/// replaced by an error message is not.
fn drain(mut stream: impl std::io::Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    bytes
}

/// Join one reader thread and decode what it read.
///
/// Lossy, for the reason `tools::execute`'s file reader is: a command's output
/// is bytes and a model is given text, and refusing a capture because a build
/// emitted one invalid sequence would lose the whole diagnosis over a byte.
fn joined(
    reader: std::thread::JoinHandle<Vec<u8>>,
    program: &str,
    which: &str,
) -> Result<String, SpawnFailure> {
    match reader.join() {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => Err(SpawnFailure::Lost {
            program: program.to_owned(),
            detail: format!("the thread reading its {which} panicked"),
        }),
    }
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
