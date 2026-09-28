// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which projects' validators the person has approved.
//!
//! # Why this exists
//!
//! A validator is a command written in a project's `./zaru.toml`. Before
//! 2026-09-28 those commands ran the first time a task was given in the
//! project, in every permission mode and with no terminal needed. So a
//! repository somebody cloned could run its own commands on their machine the
//! moment they asked Zaru anything in it.
//!
//! Now a project's validators are untrusted until the person approves them.
//! Before the first one runs, Zaru shows every validator's name and exact
//! command and asks once. The answer is remembered for that project directory
//! **and** for that exact set of validators. If `zaru.toml` changes them, Zaru
//! asks again and says what changed. With no terminal to ask on, nothing runs
//! and the task is refused, naming `zaru validators approve`.
//!
//! The permission mode plays no part. `yolo` is about the model's tool calls;
//! this is about a file the project's author wrote. Ruled by the coordinator
//! on 2026-09-28 under Jeshua's directives 58 and 62, open to his veto.
//!
//! # Where the approvals are kept
//!
//! `~/.zaru/approved-validators.jsonl`, one line per approval, never in the
//! project. The line holds the project directory, the date, and **every
//! validator as it was parsed**: name, command, `expect` and `after`.
//!
//! The validators themselves rather than a hash of them, for two reasons.
//! No hash function is in the dependency table of ADR-0003 D2, and a
//! forgeable one would be no gate. And a hash cannot say what changed, which
//! the person is owed when Zaru asks again. `commands::admission` keeps
//! whole command files for the same reasons, and this file follows it.
//!
//! # A model cannot approve validators without being asked
//!
//! The file is outside the project, so an `fs.write` or `fs.edit` of it is
//! out of the working tree and is asked about in `ask` and `allow` mode, and
//! refused with no terminal. A `cmd.run` is asked about in `ask` mode and in
//! `allow` mode unless the person put that exact line on their allowlist. In
//! `yolo` mode nothing the model does is asked about; a model there can run
//! any command directly, so approving a validator gains it nothing it did not
//! already have.

use crate::session::store::FILE_MODE;
use crate::tools::port::{Answer, Confirm, Question};
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use zaru_core::iteration::validator::{Declared, Expect};

/// The file, inside `~/.zaru/`.
pub const APPROVALS_FILE: &str = "approved-validators.jsonl";

/// The command a person runs to approve a project's validators.
pub const APPROVE_COMMAND: &str = "zaru validators approve";

/// The command a person runs to see what is approved.
pub const LIST_COMMAND: &str = "zaru validators list";

/// One validator, as it is shown and as it is remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// The validator's name.
    pub name: String,
    /// The command it runs, exactly as `zaru.toml` wrote it.
    pub run: String,
    /// How its result is judged: `exit-zero`, `exit-code 2`, `matches <regex>`
    /// or `json_schema <file>`.
    pub expect: String,
    /// The validators it runs after.
    #[serde(default)]
    pub after: Vec<String>,
}

impl Shown {
    /// The validator as it was parsed from `zaru.toml`.
    #[must_use]
    pub fn of(declared: &Declared) -> Self {
        let expect = match &declared.expect {
            Expect::ExitZero => "exit-zero".to_owned(),
            Expect::ExitCode(code) => format!("exit-code {code}"),
            Expect::Matches(pattern) => format!("matches {}", pattern.as_str()),
            Expect::JsonSchema(path) => format!("json_schema {}", path.as_str()),
        };
        Self {
            name: declared.name.as_str().to_owned(),
            run: declared.run.as_str().to_owned(),
            expect,
            after: declared
                .after
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect(),
        }
    }

    /// The row a question or a listing shows for this validator.
    #[must_use]
    pub fn row(&self) -> String {
        format!("  {}: {}", self.name, self.run)
    }
}

/// Every validator in a set, in the order `zaru.toml` declared them.
#[must_use]
pub fn shown(declared: &[Declared]) -> Vec<Shown> {
    declared.iter().map(Shown::of).collect()
}

/// One approval, as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    /// The project directory, canonical, the same path a session records.
    pub directory: PathBuf,
    /// `YYYY-MM-DD`.
    pub approved: String,
    /// Every validator that was approved, as parsed.
    pub validators: Vec<Shown>,
}

/// Where a project's validators stand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// This exact set was approved for this directory.
    Approved {
        /// When.
        on: String,
    },
    /// Nothing was ever approved for this directory.
    NeverApproved,
    /// Something was approved here, and the validators are different now.
    Changed {
        /// One plain line per difference.
        changes: Vec<String>,
    },
}

/// Something went wrong with the approvals file.
///
/// No variant carries a validator's command. A refusal is text that gets
/// pasted into a bug report.
#[derive(Debug)]
pub enum ApprovalError {
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A complete line did not parse.
    Malformed {
        /// The file.
        path: PathBuf,
        /// Which line, counting from one.
        line: usize,
        /// What the parser said.
        detail: String,
    },
    /// A line could not be written out.
    NotSerialisable {
        /// What the serialiser said.
        detail: String,
    },
}

impl fmt::Display for ApprovalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of {} could not be read: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "an approval could not be written: {detail}")
            }
        }
    }
}

impl std::error::Error for ApprovalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// The approvals file.
#[derive(Debug, Clone)]
pub struct Approvals {
    path: PathBuf,
}

impl Approvals {
    /// The approvals at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The approvals under a `~/.zaru`-equivalent `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(APPROVALS_FILE))
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every approval in the file, oldest first.
    ///
    /// An absent file is no approvals. A last line with no newline is a write
    /// a power cut interrupted, and is not counted.
    ///
    /// # Errors
    ///
    /// [`ApprovalError::Io`] when the file is there and cannot be read, and
    /// [`ApprovalError::Malformed`] when a complete line does not parse.
    pub fn entries(&self) -> Result<Vec<Approval>, ApprovalError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(ApprovalError::Io {
                    action: "read the approved validators",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let complete = raw.rfind('\n').map_or("", |at| &raw[..=at]);
        complete
            .lines()
            .enumerate()
            .map(|(at, line)| {
                serde_json::from_str(line).map_err(|error| ApprovalError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// The latest approval for each directory, in the order they were last
    /// approved.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn latest(&self) -> Result<Vec<Approval>, ApprovalError> {
        let mut latest: Vec<Approval> = Vec::new();
        for entry in self.entries()? {
            latest.retain(|kept| kept.directory != entry.directory);
            latest.push(entry);
        }
        Ok(latest)
    }

    /// Where `declared` stands in `directory`.
    ///
    /// The set is compared whole: a validator added, removed, renamed or with
    /// a different command, `expect` or `after` is a different set.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn standing(
        &self,
        directory: &Path,
        declared: &[Declared],
    ) -> Result<Standing, ApprovalError> {
        let offered = shown(declared);
        let entries = self.entries()?;
        if let Some(match_) = entries
            .iter()
            .rev()
            .find(|entry| entry.directory == directory && entry.validators == offered)
        {
            return Ok(Standing::Approved {
                on: match_.approved.clone(),
            });
        }
        Ok(
            match entries
                .iter()
                .rev()
                .find(|entry| entry.directory == directory)
            {
                None => Standing::NeverApproved,
                Some(previous) => Standing::Changed {
                    changes: changes(&previous.validators, &offered),
                },
            },
        )
    }

    /// Record `declared` as approved in `directory`, today.
    ///
    /// Appended as one line in one write, then flushed and synced, which is
    /// the discipline every other file under `~/.zaru/` follows. The file is
    /// created readable by its owner alone.
    ///
    /// # Errors
    ///
    /// [`ApprovalError::Io`] when the file cannot be opened or written, and
    /// [`ApprovalError::NotSerialisable`] when the line cannot be rendered.
    pub fn approve(
        &self,
        directory: &Path,
        declared: &[Declared],
        today: &str,
    ) -> Result<(), ApprovalError> {
        let entry = Approval {
            directory: directory.to_path_buf(),
            approved: today.to_owned(),
            validators: shown(declared),
        };
        let mut line =
            serde_json::to_string(&entry).map_err(|error| ApprovalError::NotSerialisable {
                detail: error.to_string(),
            })?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| ApprovalError::Io {
                action: "open the approved validators for writing",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(line.as_bytes())
            .and_then(|()| file.flush())
            .and_then(|()| file.sync_data())
            .map_err(|source| ApprovalError::Io {
                action: "write to the approved validators",
                path: self.path.clone(),
                source,
            })
    }
}

/// What differs between the set approved before and the set declared now.
///
/// One plain line each, in the order the validators are declared now, then
/// the ones that were removed.
#[must_use]
pub fn changes(before: &[Shown], now: &[Shown]) -> Vec<String> {
    let mut said = Vec::new();
    for validator in now {
        match before.iter().find(|old| old.name == validator.name) {
            None => said.push(format!("  added {}: {}", validator.name, validator.run)),
            Some(old) if old.run != validator.run => said.push(format!(
                "  changed {}: it ran {} and now runs {}",
                validator.name, old.run, validator.run
            )),
            Some(old) if old != validator => said.push(format!(
                "  changed {}: same command, different expect or after",
                validator.name
            )),
            Some(_) => {}
        }
    }
    for old in before {
        if !now.iter().any(|validator| validator.name == old.name) {
            said.push(format!("  removed {}: {}", old.name, old.run));
        }
    }
    said
}

/// The first line of the question when nothing was approved here before.
pub const ASK_FIRST: &str = "This project's zaru.toml declares validators. They are commands \
                             Zaru runs on your machine to check the model's work. Allow these \
                             commands to run in this project?";

/// The first line of the question when the validators changed.
pub const ASK_AGAIN: &str = "The validators in this project's zaru.toml changed since you \
                             approved them. Allow these commands to run in this project?";

/// The question a person is asked before a project's validators first run.
///
/// Every validator's name and exact command, one per row. When the set
/// changed, what changed comes first. The answers are `y` or `N`: an approval
/// is kept on disk, so there is no "for this session" answer.
#[must_use]
pub fn question(declared: &[Declared], standing: &Standing) -> Question {
    let mut detail = Vec::new();
    let statement = match standing {
        Standing::Changed { changes } => {
            detail.push("What changed:".to_owned());
            detail.extend(changes.iter().cloned());
            detail.push("The validators now:".to_owned());
            ASK_AGAIN
        }
        Standing::NeverApproved | Standing::Approved { .. } => ASK_FIRST,
    };
    detail.extend(shown(declared).iter().map(Shown::row));
    Question {
        statement: statement.to_owned(),
        detail,
        prominent: true,
        answers: crate::tools::prompt::Answers::Admission,
    }
}

/// Why a project's validators may not run.
#[derive(Debug)]
pub enum NotApproved {
    /// There was no terminal to ask on.
    NobodyToAsk {
        /// Whether they were approved before and have changed since.
        changed: bool,
    },
    /// The person was asked and said no.
    Declined,
    /// The approvals file could not be read or written.
    File(ApprovalError),
}

/// Ask about `declared` in `directory` if it is not already approved, and
/// remember a yes.
///
/// Returns `Ok(true)` when the person approved them just now and `Ok(false)`
/// when they were already approved. The permission mode is not an argument:
/// `yolo` does not skip this.
///
/// # Errors
///
/// [`NotApproved`] when the validators may not run.
pub fn gate(
    approvals: &Approvals,
    directory: &Path,
    declared: &[Declared],
    confirmer: Option<&dyn Confirm>,
    today: &str,
) -> Result<bool, NotApproved> {
    let Some(standing) = standing_unapproved(approvals, directory, declared)? else {
        return Ok(false);
    };
    let answered = confirmer.map(|confirmer| confirmer.confirm(&question(declared, &standing)));
    answer_the_gate(approvals, directory, declared, &standing, answered, today)
}

/// [`gate`], asked at the start of a turn, where `Ctrl-C` stops the turn.
///
/// The question is asked through [`Confirm::ask`], so the program goes on
/// running while the person reads the commands, and its answers line says
/// what `Esc` and `Ctrl-C` do there:
/// [`Answers::Validators`](crate::tools::prompt::Answers::Validators). A turn
/// stopped while it stands drops this future and approves nothing.
///
/// # Errors
///
/// [`NotApproved`] when the validators may not run.
pub async fn gate_in_a_turn(
    approvals: &Approvals,
    directory: &Path,
    declared: &[Declared],
    confirmer: Option<&(dyn Confirm + Sync)>,
    today: &str,
) -> Result<bool, NotApproved> {
    let Some(standing) = standing_unapproved(approvals, directory, declared)? else {
        return Ok(false);
    };
    let asked = Question {
        answers: crate::tools::prompt::Answers::Validators,
        ..question(declared, &standing)
    };
    let answered = match confirmer {
        Some(confirmer) => Some(confirmer.ask(&asked).await),
        None => None,
    };
    answer_the_gate(approvals, directory, declared, &standing, answered, today)
}

/// Where the gate stands, or `None` when this exact set is approved already.
fn standing_unapproved(
    approvals: &Approvals,
    directory: &Path,
    declared: &[Declared],
) -> Result<Option<Standing>, NotApproved> {
    let standing = approvals
        .standing(directory, declared)
        .map_err(NotApproved::File)?;
    Ok((!matches!(standing, Standing::Approved { .. })).then_some(standing))
}

/// What the gate does with what the person answered: the one mapping both
/// ways of asking share.
fn answer_the_gate(
    approvals: &Approvals,
    directory: &Path,
    declared: &[Declared],
    standing: &Standing,
    answered: Option<Result<Answer, crate::tools::port::ConfirmFailure>>,
    today: &str,
) -> Result<bool, NotApproved> {
    let changed = matches!(standing, Standing::Changed { .. });
    let Some(answered) = answered else {
        return Err(NotApproved::NobodyToAsk { changed });
    };
    match answered {
        Err(_) => Err(NotApproved::NobodyToAsk { changed }),
        // `h` is not offered at this question, so it cannot arrive; if it
        // did, it is not a yes.
        Ok(Answer::No | Answer::ForThisHost) => Err(NotApproved::Declined),
        Ok(Answer::Once | Answer::ForThisSession) => {
            approvals
                .approve(directory, declared, today)
                .map_err(NotApproved::File)?;
            Ok(true)
        }
    }
}

/// What a listing of the approved validators prints.
#[must_use]
pub fn listing(latest: &[Approval]) -> Vec<String> {
    if latest.is_empty() {
        return vec!["No project's validators have been approved on this machine.".to_owned()];
    }
    let mut lines = Vec::new();
    for approval in latest {
        lines.push(format!(
            "{} (approved {})",
            approval.directory.display(),
            approval.approved
        ));
        if approval.validators.is_empty() {
            lines.push("  no validators".to_owned());
        }
        lines.extend(approval.validators.iter().map(Shown::row));
    }
    lines
}

#[cfg(test)]
mod tests;
