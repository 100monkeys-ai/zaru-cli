// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every process a check starts, owned until it is gone.
//!
//! # The defect this exists for
//!
//! Measured on 2026-09-28 by the `harness-orphans-and-reader-panic` arc. The
//! machine's watchdog sent SIGTERM to eight `zaru` processes running from the
//! `target/debug/` of a worktree that had been deleted five hours earlier.
//! They came from a **passing** run of the workspace suite started under
//! `nohup`, and the same eight — same count, same clustering of process ids —
//! were reproduced here from a passing run of `terminal_from_outside.rs` alone.
//!
//! Three ways a child outlived the check that started it, all measured:
//!
//! - **It was not the child.** `script(1)` puts the command it runs in a
//!   session of its own, so the `zaru` under it is `script`'s grandchild in
//!   another session and another process group. The check killed and reaped
//!   `script` and nothing else. Whether `zaru` then ended depended on a
//!   `SIGHUP` reaching it, which it does not when the shell leading that
//!   session ignores the signal, as everything under `nohup` does.
//! - **The check never ran its `Drop`.** SIGTERM or SIGKILL to the test binary
//!   alone — the watchdog, `timeout`, an out-of-memory killer — ends it with no
//!   unwinding, and `script` and `zaru` both lived on, `script` reparented to
//!   the machine's reaper, polling a pipe nobody would ever write to again.
//! - **The check reaped a direct child only on the paths it thought of.** A
//!   `std::process::Child` is not killed when it is dropped.
//!
//! # What owning a process means here
//!
//! - **It dies with the thread that started it.** Every program is started
//!   through util-linux's `setpriv --pdeathsig KILL`, which sets the kernel's
//!   parent-death signal and then `exec`s the program in place, so the process
//!   id is the program's and nothing else about it changes. When the thread
//!   that spawned it ends — the check returning, panicking, or its whole
//!   process being killed — the kernel sends it SIGKILL. `setpriv` is on every
//!   Linux this harness supports and on CI's runner, as `script` is; a machine
//!   without it fails every check that starts a process, by name.
//! - **It leads a process group of its own**, so what it starts in that group
//!   goes with it.
//! - **Dropping the handle kills everything below it and reaps it.** The tree
//!   is read from `/proc` while it is still whole — a grandchild in another
//!   session is found by its parent, not by its group — and every process in
//!   it, and the group, is sent SIGKILL before the child is reaped.
//!
//! **The parent-death signal is the thread's, not the process's.** A check
//! that starts a process on a thread of its own and lets that thread end has
//! ended the process too. Every check here starts its processes on the thread
//! the test runs on.
//!
//! # Why a type of its own rather than `std::process::Command`
//!
//! [`Owning`] carries the builder methods a check uses and three that start
//! the process, and every one of those returns something this module owns. A
//! check cannot reach `std`'s `spawn` through it, so the only way to start a
//! process nothing owns is to name `std::process::Command` again, which
//! `corpus_every_process_a_check_starts_is_owned` in `files_from_outside.rs`
//! walks `tests/` for.

#![allow(
    dead_code,
    reason = "every test binary includes this file, and each uses the part its checks need"
)]

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::sync::OnceLock;

/// Where util-linux's `setpriv` is, found once on this process's `PATH`.
fn setpriv() -> &'static Path {
    static FOUND: OnceLock<PathBuf> = OnceLock::new();
    FOUND.get_or_init(|| {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .map(|directory| directory.join("setpriv"))
            .find(|candidate| candidate.is_file())
            .unwrap_or_else(|| {
                panic!(
                    "util-linux `setpriv` is not on this machine's PATH ({path:?}). Every \
                     process a check starts is started through it so that the process dies \
                     with the check, and a check that cannot promise that does not start one"
                )
            })
    })
}

/// A command whose process the check will own. See the module documentation.
///
/// `program` is looked up by `setpriv` on the child's own `PATH`, or on
/// `/bin:/usr/bin` when the check hands the child none.
pub fn command(program: impl AsRef<OsStr>) -> Owning {
    let mut command = Command::new(setpriv());
    command
        .args(["--pdeathsig", "KILL", "--"])
        .arg(program.as_ref());
    Owning { command }
}

/// A command whose process the check will own, being built.
pub struct Owning {
    command: Command,
}

impl Owning {
    /// As [`Command::arg`].
    pub fn arg(&mut self, argument: impl AsRef<OsStr>) -> &mut Self {
        self.command.arg(argument);
        self
    }

    /// As [`Command::args`].
    pub fn args<I, S>(&mut self, arguments: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(arguments);
        self
    }

    /// As [`Command::env`].
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.command.env(key, value);
        self
    }

    /// As [`Command::envs`].
    pub fn envs<I, K, V>(&mut self, variables: I) -> &mut Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.command.envs(variables);
        self
    }

    /// As [`Command::env_remove`].
    pub fn env_remove(&mut self, key: impl AsRef<OsStr>) -> &mut Self {
        self.command.env_remove(key);
        self
    }

    /// As [`Command::env_clear`].
    pub fn env_clear(&mut self) -> &mut Self {
        self.command.env_clear();
        self
    }

    /// As [`Command::current_dir`].
    pub fn current_dir(&mut self, directory: impl AsRef<Path>) -> &mut Self {
        self.command.current_dir(directory);
        self
    }

    /// As [`Command::stdin`].
    pub fn stdin(&mut self, stdio: impl Into<Stdio>) -> &mut Self {
        self.command.stdin(stdio);
        self
    }

    /// As [`Command::stdout`].
    pub fn stdout(&mut self, stdio: impl Into<Stdio>) -> &mut Self {
        self.command.stdout(stdio);
        self
    }

    /// As [`Command::stderr`].
    pub fn stderr(&mut self, stdio: impl Into<Stdio>) -> &mut Self {
        self.command.stderr(stdio);
        self
    }

    /// What the child's environment will be told, as [`Command::get_envs`]
    /// reports it: a read, which starts nothing.
    pub fn get_envs(&self) -> impl Iterator<Item = (OsString, Option<OsString>)> + '_ {
        self.command
            .get_envs()
            .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
    }

    /// Start the process, owned. Standard streams are inherited unless the
    /// check said otherwise, as [`Command::spawn`] has them.
    ///
    /// # Errors
    ///
    /// When `setpriv` cannot be started. A program `setpriv` cannot find is
    /// `setpriv`'s own exit, 127, with its reason on the child's standard
    /// error.
    pub fn spawn(&mut self) -> io::Result<Owned> {
        self.command.process_group(0);
        let child = self.command.spawn()?;
        let leader = Identity::of(child.id());
        Ok(Owned {
            child,
            reaped: false,
            leader,
            adopted: Vec::new(),
        })
    }

    /// Run the process to its end and collect what it wrote, as
    /// [`Command::output`] does: standard input empty, both outputs captured.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`], or when waiting on the child fails.
    pub fn output(&mut self) -> io::Result<Output> {
        self.command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.spawn()?.wait_with_output()
    }

    /// Run the process to its end, as [`Command::status`] does: every stream
    /// inherited unless the check said otherwise.
    ///
    /// # Errors
    ///
    /// As [`Self::spawn`], or when waiting on the child fails.
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait()
    }
}

/// A process this check started, and everything below it.
///
/// Dropping it kills the lot and reaps the child. See the module
/// documentation.
pub struct Owned {
    child: Child,
    reaped: bool,
    /// The child as it was when it started, so nothing is ever signalled by a
    /// process id the system has since given to someone else.
    leader: Option<Identity>,
    /// Descendants recorded while the tree was whole, for the moment it no
    /// longer is. See [`Self::kill_the_child_alone`].
    adopted: Vec<Identity>,
}

impl Owned {
    /// The child's process id, which is the program's: `setpriv` `exec`s it.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// The child's standard input, which the check piped.
    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.child
            .stdin
            .as_mut()
            .expect("the check piped the child's standard input")
    }

    /// The child's standard input, taken, so that dropping it closes the pipe.
    pub fn take_stdin(&mut self) -> ChildStdin {
        self.child
            .stdin
            .take()
            .expect("the check piped the child's standard input")
    }

    /// The child's standard output, taken.
    pub fn take_stdout(&mut self) -> ChildStdout {
        self.child
            .stdout
            .take()
            .expect("the check piped the child's standard output")
    }

    /// Wait for the child to end, and reap it.
    ///
    /// # Errors
    ///
    /// When waiting fails.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.child.wait()?;
        self.reaped = true;
        Ok(status)
    }

    /// Wait for the child to end, reap it, and collect what it wrote.
    ///
    /// # Errors
    ///
    /// When reading its output or waiting fails.
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        let stdout = self.child.stdout.take();
        let stderr = self.child.stderr.take();
        // Standard input is closed first, so a child reading it to its end
        // sees the end.
        drop(self.child.stdin.take());
        let drain = |stream: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut stream) = stream {
                    stream.read_to_end(&mut bytes).map(|_| bytes)
                } else {
                    Ok(bytes)
                }
            })
        };
        let out = drain(stdout.map(|stream| Box::new(stream) as Box<dyn Read + Send>));
        let err = drain(stderr.map(|stream| Box::new(stream) as Box<dyn Read + Send>));
        let status = self.wait()?;
        let stdout = out
            .join()
            .map_err(|_| io::Error::other("the thread reading standard output panicked"))??;
        let stderr = err
            .join()
            .map_err(|_| io::Error::other("the thread reading standard error panicked"))??;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    /// Kill the child and everything below it, and reap the child.
    pub fn kill(&mut self) {
        let mut targets: Vec<String> = Vec::new();
        if !self.reaped {
            // The group's id is the child's, and it cannot have been given to
            // anyone else while the child is unreaped.
            targets.push(format!("-{}", self.child.id()));
            self.adopt_the_tree();
        }
        targets.extend(
            self.adopted
                .iter()
                .filter(|identity| identity.is_still_running())
                .map(|identity| identity.pid.to_string()),
        );
        signal_kill(&targets);
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.wait();
        }
    }

    /// Kill the child and nothing else, and reap it, having first recorded
    /// everything below it so that dropping this handle still reaches them.
    ///
    /// For a check about what the processes below a child do when the child
    /// dies — which is the only moment the child's descendants stop being
    /// findable from it, because they are handed to the machine's reaper.
    pub fn kill_the_child_alone(&mut self) {
        self.adopt_the_tree();
        let _ = self.child.kill();
        let _ = self.wait();
    }

    /// Record every descendant of the child as it stands now.
    fn adopt_the_tree(&mut self) {
        for pid in descendants_of(self.child.id()) {
            if let Some(identity) = Identity::of(pid)
                && !self.adopted.contains(&identity)
            {
                self.adopted.push(identity);
            }
        }
    }

    /// The descendants recorded so far.
    pub fn adopted(&self) -> Vec<u32> {
        self.adopted.iter().map(|identity| identity.pid).collect()
    }

    /// Whether the child, as it was when it started, is still running.
    pub fn leader_is_running(&self) -> bool {
        self.leader.as_ref().is_some_and(Identity::is_still_running)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A process as `/proc` knows it: its id and the moment it started, which
/// together name one process for the life of the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The process id.
    pub pid: u32,
    started: u64,
}

impl Identity {
    /// The process `pid` is now, if there is one.
    pub fn of(pid: u32) -> Option<Self> {
        let (_, _, started) = stat(pid)?;
        Some(Self { pid, started })
    }

    /// Whether the process this names is still there and not a zombie.
    pub fn is_still_running(&self) -> bool {
        matches!(stat(self.pid), Some((state, _, started)) if started == self.started && state != 'Z')
    }
}

/// A process's state, parent and start time, from `/proc/<pid>/stat`.
///
/// The command name sits in parentheses and may itself hold spaces and
/// parentheses, so the fields are read after the last `)`.
fn stat(pid: u32) -> Option<(char, u32, u64)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = &text[text.rfind(')')? + 1..];
    let fields: Vec<&str> = after.split_whitespace().collect();
    // After the name: state (3), ppid (4), … starttime (22).
    let state = fields.first()?.chars().next()?;
    let parent = fields.get(1)?.parse().ok()?;
    let started = fields.get(19)?.parse().ok()?;
    Some((state, parent, started))
}

/// Every process below `root`, read from `/proc` by parent.
fn descendants_of(root: u32) -> Vec<u32> {
    let mut parents: Vec<(u32, u32)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if let Some((_, parent, _)) = stat(pid) {
                parents.push((pid, parent));
            }
        }
    }
    let mut found = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for (pid, _) in parents.iter().filter(|(_, of)| *of == parent) {
            if !found.contains(pid) {
                found.push(*pid);
                frontier.push(*pid);
            }
        }
    }
    found
}

/// SIGKILL to each target, a process id or a negated process group id.
///
/// Through `kill(1)`, because the workspace denies `unsafe_code` and the
/// standard library signals only its own children. A target already gone is
/// `kill`'s complaint about that one target and not a reason to skip the rest,
/// so its status is not read.
fn signal_kill(targets: &[String]) {
    if targets.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .args(targets)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
