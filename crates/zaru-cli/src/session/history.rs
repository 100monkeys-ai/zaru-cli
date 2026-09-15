// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a person typed at the prompt, kept per working directory across
//! sessions.
//!
//! [ADR-0010] D1 lists what `~/.zaru/` holds and this is the sixth thing in
//! it — after [ADR-0004] D3's `node.key`, D1's own `sessions/`, [ADR-0014]
//! D1's `config.toml`, [ADR-0007]'s `credentials.json` and [ADR-0015] D3's
//! `commands/`. Accepted 2026-09-15 under directives 20, 25, 31 and 35 as a
//! delegated coordinator ruling, open to Jeshua's veto, and written on
//! [ADR-0010's amendments volume 3].
//!
//! # One file, keyed the way `--continue` keys a session
//!
//! D4 is "the most recent session **in this directory**", and
//! [`crate::session::most_recent_in`] implements it by comparing the
//! `directory` a session recorded against this process's canonical root. This
//! file is keyed the same way: the directory rides **in** the record and the
//! reader filters on it.
//!
//! **A file per directory was the other shape and it needs a path turned into
//! a filename, which has no correct answer here.** A percent-encoding
//! overruns `NAME_MAX` on a deep path; a mirrored directory tree collides the
//! moment one directory's mirror is another directory's leaf — `/a` and
//! `/a/history` want the same inode; a digest costs exactly the legibility D1
//! and D5 are about. A field compared at read has none of those problems and
//! reuses a comparison the harness already trusts.
//!
//! # D5, and why this file is built the way it is
//!
//! "The user can read every byte the harness stores about them with `cat`."
//! One typed line per line of the file, no framing, no index, no binary, and
//! the directory legible in the line rather than hidden in a name. `grep` is
//! the whole query language and that is deliberate.
//!
//! # This module designs no redaction, and the reason is the transcript's
//!
//! [ADR-0008]'s clause 6 puts one `Redactor` on every path from captured
//! bytes into a **model prompt**, and a file on disk is not one — which is
//! why [`crate::session`] applies none either, in as many words. **A masked
//! answer cannot reach this file because it cannot reach the composer**:
//! `zaru_tui::shell::Shell::key`'s secret arm consumes every keystroke before
//! the composer or `submit` sees one, the bytes live in a private field with
//! no accessor, and `take_secret` moves them out. What is asserted is
//! structural rather than filtered, which is the stronger property.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0010's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript-updates-3
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::session::store::FILE_MODE;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The file, inside `~/.zaru/`. A sixth thing under that directory.
pub const HISTORY_FILE: &str = "history.jsonl";

/// How many lines are kept for one working directory.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-15, open to
/// Jeshua's veto**, in the same shape as `STRIP_ROWS`, `QUEUED` and the
/// register glyphs: no record names a number and one is needed. The ruling
/// asked for "a measured line count you propose as a constant" and this is
/// the measurement: a submitted line on this machine's own sessions runs to
/// under eighty bytes, so a thousand of them is under 100 KB for a directory
/// somebody has worked in for a year. A person should set it or confirm it.
///
/// It bounds **one directory's** lines rather than the file, because the file
/// is shared by every directory and a global cap would let one busy checkout
/// evict another's history.
pub const HISTORY_LINES: usize = 1000;

/// One line, as the file holds it.
///
/// `directory` is [ADR-0011](https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface)
/// D4's canonical root, the same value `meta.toml` records, so the comparison
/// this file is read by is the comparison `--continue` is written by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Where it was typed.
    pub directory: PathBuf,
    /// What was typed, verbatim.
    pub line: String,
}

/// Something went wrong with the history file.
///
/// **No variant carries a typed line.** A refusal is the text that gets
/// pasted into a bug report, which is [`crate::session::transcript`]'s own
/// rule for the same reason.
#[derive(Debug)]
pub enum HistoryError {
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

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the history at {} did not parse: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "a history line could not be rendered: {detail}")
            }
        }
    }
}

impl std::error::Error for HistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// The history file.
#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

impl History {
    /// The history at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The history under a `~/.zaru`-equivalent `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(HISTORY_FILE))
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every line in the file, oldest first, with the fragment separated.
    ///
    /// **An absent file is an empty history rather than a fault**, which is
    /// the distinction `MetaFile::read_if_present` already draws: a machine
    /// that has never run this harness has nothing to report.
    ///
    /// # Errors
    ///
    /// [`HistoryError::Io`] when the file is there and cannot be read, and
    /// [`HistoryError::Malformed`] when a complete line does not parse.
    pub fn entries(&self) -> Result<Vec<Entry>, HistoryError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(HistoryError::Io {
                    action: "read the history",
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
                serde_json::from_str(line).map_err(|error| HistoryError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// What was typed in `directory`, oldest first, the last
    /// [`HISTORY_LINES`] of it.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn lines_in(&self, directory: &Path) -> Result<Vec<String>, HistoryError> {
        let mut lines: Vec<String> = self
            .entries()?
            .into_iter()
            .filter(|entry| entry.directory == directory)
            .map(|entry| entry.line)
            .collect();
        if lines.len() > HISTORY_LINES {
            lines.drain(..lines.len() - HISTORY_LINES);
        }
        Ok(lines)
    }

    /// Append one submitted line.
    ///
    /// The discipline is [`crate::session::transcript`]'s, and it is the same
    /// discipline rather than a second description of it: the file is opened
    /// `append`, the line and its newline go out in **one** `write_all` so a
    /// kill cannot split them, then `flush`, then `sync_data`.
    ///
    /// # Errors
    ///
    /// [`HistoryError::Io`] when the file cannot be opened or written, and
    /// [`HistoryError::NotSerialisable`] when the line cannot be rendered.
    pub fn append(&self, directory: &Path, line: &str) -> Result<(), HistoryError> {
        let entry = Entry {
            directory: directory.to_path_buf(),
            line: line.to_owned(),
        };
        let mut rendered =
            serde_json::to_string(&entry).map_err(|error| HistoryError::NotSerialisable {
                detail: error.to_string(),
            })?;
        rendered.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| HistoryError::Io {
                action: "open the history for appending",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(rendered.as_bytes())
            .map_err(|source| HistoryError::Io {
                action: "append to the history",
                path: self.path.clone(),
                source,
            })?;
        file.flush().map_err(|source| HistoryError::Io {
            action: "flush the history",
            path: self.path.clone(),
            source,
        })?;
        file.sync_data().map_err(|source| HistoryError::Io {
            action: "sync the history",
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }

    /// Drop every directory's lines past [`HISTORY_LINES`], keeping the
    /// newest, and say whether anything was dropped.
    ///
    /// **Called at session open and never while a session runs.** A rewrite
    /// races an append, and the window this leaves is the moment a session
    /// opens rather than every line a person types. A session that appends
    /// after this has returned appends to the rewritten file.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s, and [`HistoryError::Io`] when the rewrite fails.
    pub fn compact(&self) -> Result<bool, HistoryError> {
        let entries = self.entries()?;
        let mut kept: Vec<Entry> = Vec::with_capacity(entries.len());
        for directory in directories_of(&entries) {
            let mut theirs: Vec<(usize, &Entry)> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.directory == directory)
                .collect();
            if theirs.len() > HISTORY_LINES {
                theirs.drain(..theirs.len() - HISTORY_LINES);
            }
            kept.extend(theirs.into_iter().map(|(_, entry)| entry.clone()));
        }
        if kept.len() == entries.len() {
            return Ok(false);
        }
        // Re-ordered to the file's own order, so compaction changes which
        // lines are there and never which order they were typed in.
        kept.sort_by_key(|entry| {
            entries
                .iter()
                .position(|original| original == entry)
                .unwrap_or(usize::MAX)
        });
        let mut rendered = String::new();
        for entry in &kept {
            let line =
                serde_json::to_string(entry).map_err(|error| HistoryError::NotSerialisable {
                    detail: error.to_string(),
                })?;
            rendered.push_str(&line);
            rendered.push('\n');
        }
        crate::atomic::write(&self.path, rendered.as_bytes(), FILE_MODE).map_err(|failed| {
            HistoryError::Io {
                action: "rewrite the history",
                path: failed.path.clone(),
                source: failed.source,
            }
        })?;
        Ok(true)
    }
}

/// Every directory in `entries`, in the order they first appear.
fn directories_of(entries: &[Entry]) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for entry in entries {
        if !seen.contains(&entry.directory) {
            seen.push(entry.directory.clone());
        }
    }
    seen
}
