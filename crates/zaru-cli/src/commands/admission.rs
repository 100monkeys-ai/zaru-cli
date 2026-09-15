// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D4's record: which of a project's commands this user has
//! admitted, and when.
//!
//! # The clause
//!
//! D4: "**A cloned repository's commands and skills do not load on first
//! run.** The harness reports what the project offers and the user admits them
//! once; the decision is recorded per project. This is the direct answer to
//! the community-plugin supply-chain problem. A prompt template cannot execute
//! code, but it can instruct a model that holds tools — which is prompt
//! injection with a file extension. Admission is the gate, and it is per
//! project rather than global because a user who trusts one repository has
//! said nothing about the next."
//!
//! # One file, keyed the way `--continue` keys a session
//!
//! `~/.zaru/admissions.jsonl` is the **seventh** thing under that directory,
//! after [ADR-0004] D3's `node.key`, [ADR-0010] D1's `sessions/`, [ADR-0014]
//! D1's `config.toml`, [ADR-0007]'s `credentials.json`, [ADR-0015] D3's
//! `commands/` and [ADR-0010] D1's `history.jsonl`. Accepted 2026-09-15 under
//! directives 20, 25, 31 and 35 as a delegated coordinator ruling, open to
//! Jeshua's veto, and written on [ADR-0010's amendments volume 3].
//!
//! The directory rides **in** the record and the reader filters on it, which
//! is exactly what [`crate::session::history`] does and for the same reason:
//! a file per directory needs a path turned into a filename, and that
//! question was ruled against at `pane-navigation` — a percent-encoding
//! overruns `NAME_MAX` on a deep path, a mirrored tree collides when one
//! directory's mirror is another's leaf, and a digest costs exactly the
//! legibility [ADR-0010] D5 is about.
//!
//! # What an admission covers, and why the body is stored verbatim
//!
//! One line per **admitted command**, carrying the directory, the name, the
//! date and **the body as it stood when the user said yes**. So a new name in
//! an admitted project asks again, and a *changed body* of an admitted name
//! asks again, which is the half that matters: a command admitted today whose
//! body is rewritten by tomorrow's `git pull` is exactly the supply-chain
//! shape D4 exists to gate.
//!
//! **The body rather than a digest, decided 2026-09-15 and open to veto.** A
//! digest would need a cryptographic hash — a forgeable one is no gate at all
//! when the threat model is somebody who edits the body — and no hash is in
//! [ADR-0003] D2's table. Storing the body needs nothing, admits no collision
//! question whatever, and gives [ADR-0010] D5's `cat` its strongest reading:
//! the file says what you admitted **in the words you admitted**, rather than
//! in a digest a person cannot check. The cost is stated rather than
//! discovered: a line is as long as the body it records, bounded only by the
//! one-mebibyte ceiling that already applies to the file it was read from.
//!
//! # A decline writes nothing
//!
//! Deliberately. A "declined" row would have to be found and deleted by a
//! person who later decides to trust the repository, and D4's gate is a
//! standing question rather than a standing verdict.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0010's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript-updates-3
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::commands::document::Command;
use crate::session::store::FILE_MODE;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The file, inside `~/.zaru/`. A seventh thing under that directory.
pub const ADMISSIONS_FILE: &str = "admissions.jsonl";

/// One admitted command, as the file holds it.
///
/// `directory` is [ADR-0011](https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface)
/// D4's canonical root, the same value `meta.toml` and `history.jsonl`
/// record, so the comparison this file is read by is the comparison
/// `--continue` is written by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admission {
    /// The project it was admitted for.
    pub directory: PathBuf,
    /// The command's name, without a leading slash.
    pub name: String,
    /// `YYYY-MM-DD`, the date [ADR-0015](https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility)
    /// D6's attribution line shows.
    pub admitted: String,
    /// The body as it stood when the user said yes.
    pub body: String,
}

/// Something went wrong with the admissions file.
///
/// **No variant carries a body.** A refusal is the text that gets pasted into
/// a bug report, which is [`crate::session::history`]'s own rule for the same
/// reason.
#[derive(Debug)]
pub enum AdmissionError {
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A **complete** line did not parse.
    ///
    /// Distinct from a trailing fragment, which is the line that was in
    /// flight when a machine lost power, exactly as it is on the transcript.
    Malformed {
        /// The file.
        path: PathBuf,
        /// Which line, counting from one.
        line: usize,
        /// What the parser said. Positional; it quotes no field value.
        detail: String,
    },
    /// A line could not be rendered.
    NotSerialisable {
        /// What the serialiser said. Positional; it quotes no field value.
        detail: String,
    },
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the admissions at {} did not parse: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "an admission could not be rendered: {detail}")
            }
        }
    }
}

impl std::error::Error for AdmissionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// The admissions file.
#[derive(Debug, Clone)]
pub struct Admissions {
    path: PathBuf,
}

impl Admissions {
    /// The admissions at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The admissions under a `~/.zaru`-equivalent `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(ADMISSIONS_FILE))
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every admission in the file, oldest first, with the fragment
    /// separated.
    ///
    /// **An absent file is no admissions rather than a fault**, which is the
    /// distinction `MetaFile::read_if_present` and the history both draw: a
    /// machine that has never admitted anything has nothing to report.
    ///
    /// # Errors
    ///
    /// [`AdmissionError::Io`] when the file is there and cannot be read, and
    /// [`AdmissionError::Malformed`] when a complete line does not parse.
    pub fn entries(&self) -> Result<Vec<Admission>, AdmissionError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(AdmissionError::Io {
                    action: "read the admissions",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        // Everything before the last newline is complete; whatever follows it
        // is the line that was in flight, which is what a power cut costs and
        // is never counted -- `session::transcript`'s own rule.
        let complete = raw.rfind('\n').map_or("", |at| &raw[..=at]);
        complete
            .lines()
            .enumerate()
            .map(|(at, line)| {
                serde_json::from_str(line).map_err(|error| AdmissionError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// The date `name` was admitted in `directory` with exactly this `body`,
    /// or `None` where it was not.
    ///
    /// **The latest such line wins**, because a re-admission after a change
    /// is an append rather than a rewrite.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn admitted_on(
        &self,
        directory: &Path,
        name: &str,
        body: &str,
    ) -> Result<Option<String>, AdmissionError> {
        Ok(self
            .entries()?
            .into_iter()
            .rfind(|entry| entry.directory == directory && entry.name == name && entry.body == body)
            .map(|entry| entry.admitted))
    }

    /// Whether every one of `offered` is already admitted in `directory`,
    /// body and all.
    ///
    /// An empty offer is covered: a project with no commands asks nothing,
    /// which is [ADR-0002](https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output)
    /// D1 — the harness says nothing the user did not cause.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn covers(&self, directory: &Path, offered: &[Command]) -> Result<bool, AdmissionError> {
        let entries = self.entries()?;
        Ok(offered.iter().all(|command| {
            entries.iter().any(|entry| {
                entry.directory == directory
                    && entry.name == command.name()
                    && entry.body == command.body()
            })
        }))
    }

    /// Record every one of `offered` as admitted in `directory`, today.
    ///
    /// The discipline is [`crate::session::transcript`]'s, and it is the same
    /// discipline rather than a second description of it: the file is opened
    /// `append`, each line and its newline go out in **one** `write_all` so a
    /// kill cannot split them, then `flush`, then `sync_data`.
    ///
    /// An already-admitted command with an unchanged body is appended again
    /// rather than skipped, and that is deliberate: the file is a log of what
    /// the user was asked and answered, and `covers` reads the set rather
    /// than the count.
    ///
    /// # Errors
    ///
    /// [`AdmissionError::Io`] when the file cannot be opened or written, and
    /// [`AdmissionError::NotSerialisable`] when a line cannot be rendered.
    pub fn admit(
        &self,
        directory: &Path,
        offered: &[Command],
        today: &str,
    ) -> Result<(), AdmissionError> {
        if offered.is_empty() {
            return Ok(());
        }
        let mut rendered = String::new();
        for command in offered {
            let entry = Admission {
                directory: directory.to_path_buf(),
                name: command.name().to_owned(),
                admitted: today.to_owned(),
                body: command.body().to_owned(),
            };
            let line =
                serde_json::to_string(&entry).map_err(|error| AdmissionError::NotSerialisable {
                    detail: error.to_string(),
                })?;
            rendered.push_str(&line);
            rendered.push('\n');
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| AdmissionError::Io {
                action: "open the admissions for appending",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(rendered.as_bytes())
            .map_err(|source| AdmissionError::Io {
                action: "append to the admissions",
                path: self.path.clone(),
                source,
            })?;
        file.flush().map_err(|source| AdmissionError::Io {
            action: "flush the admissions",
            path: self.path.clone(),
            source,
        })?;
        file.sync_data().map_err(|source| AdmissionError::Io {
            action: "sync the admissions",
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }
}
