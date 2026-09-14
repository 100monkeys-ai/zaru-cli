// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D2's append-only transcript: one event per line, and what a
//! kill costs.
//!
//! D2: "Append-only means a crash loses at most the event in flight. It also
//! means the transcript is a replayable record: re-rendering it reproduces
//! what the user saw, which is what makes 'show the struggle' durable rather
//! than momentary."
//!
//! # The discipline, stated because each clause of it is load-bearing
//!
//! The file is opened `append`. Each record is serialised, a newline is
//! appended to the **same buffer**, and the whole line goes out in exactly
//! one [`write_all`](std::io::Write::write_all). Then
//! [`flush`](std::io::Write::flush), then
//! [`sync_data`](std::fs::File::sync_data). **No buffered writer at any
//! layer.**
//!
//! - *One `write_all` per line* is what stops a kill splitting a record. A
//!   `BufWriter` flushes on its own buffer boundary, which falls in the
//!   middle of whichever line crosses it — measured on 2026-09-04, a buffered
//!   child left a torn trailing fragment in 6 of 30 kills while the
//!   unbuffered one left none in 60.
//! - *`sync_data` per line* is what makes the line survive the machine losing
//!   power rather than the process being killed. **No check here exercises
//!   it**: a `SIGKILL` cannot split an unbuffered `write_all` to a regular
//!   file, so the kill check below stays green with the sync removed. That is
//!   recorded rather than glossed, because a green whose subject is narrower
//!   than the claim is [Verification lessons] §21.
//!
//! # A reader tolerates a trailing fragment, and never a gap
//!
//! [`Reading`] separates the complete lines from whatever bytes follow the
//! last newline. A fragment is the event that was in flight, which is exactly
//! what D2 says a crash costs; a *complete* line that does not parse is a
//! different thing and is reported as [`TranscriptError::Malformed`].
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::session::record::Record;
use crate::session::store::FILE_MODE;
use core::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Something went wrong with the transcript on disk.
///
/// **No variant carries a record's contents.** A transcript holds whatever
/// the session held — ADR-0010's own Negative section says so — and a refusal
/// is the text that gets pasted into a bug report.
#[derive(Debug)]
pub enum TranscriptError {
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A record could not be rendered.
    NotSerialisable {
        /// What the serialiser said. Positional; it quotes no field value.
        detail: String,
    },
    /// A **complete** line did not parse.
    ///
    /// Distinct from a trailing fragment, which is the event that was in
    /// flight and is what D2 says a crash costs.
    Malformed {
        /// The file.
        path: PathBuf,
        /// Which line, counting from one.
        line: usize,
        /// What the parser said. Positional; it quotes no field value.
        detail: String,
    },
}

impl fmt::Display for TranscriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
            Self::NotSerialisable { detail } => write!(
                f,
                "a transcript record could not be rendered: {detail}. The transcript is the \
                 loop's own event stream, so a record that cannot be written is \
                 an event a consumer will never see"
            ),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the transcript at {} did not parse: {detail}. A trailing \
                 fragment is the event that was in flight and is what D2 says a crash costs; a \
                 complete line that does not parse is not that",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TranscriptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NotSerialisable { .. } | Self::Malformed { .. } => None,
        }
    }
}

/// What reading a transcript back found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// Every complete line, in the order it was appended.
    pub records: Vec<Record>,
    /// The same lines as the bytes they are on disk, one per record.
    ///
    /// **Carried rather than re-serialised**, and that is the point. D1's
    /// argument for a directory of plain files is that "a harness that shows
    /// its work should not store the record of that work somewhere only it can
    /// read", so what a caller showing a transcript shows is the file. Handing
    /// it the parsed records and letting it write them back out would be a
    /// second rendering of one line, and the two would agree until a field was
    /// added.
    ///
    /// Parallel to [`Reading::records`] by construction: both are pushed in
    /// the same loop, so an index into one is an index into the other.
    pub lines: Vec<String>,
    /// How many bytes followed the last newline, if any did.
    ///
    /// `Some` is the event that was in flight when the process died — D2's
    /// "at most the event in flight". It is deliberately a **length** rather
    /// than the bytes: a fragment is arbitrary captured text and a reader of
    /// this type has no reason to see it.
    pub fragment: Option<usize>,
}

impl Reading {
    /// The last `n` records, which D4's resume re-renders.
    #[must_use]
    pub fn tail(&self, n: usize) -> &[Record] {
        let from = self.records.len().saturating_sub(n);
        &self.records[from..]
    }

    /// The same `n` records as the bytes they are on disk.
    #[must_use]
    pub fn tail_lines(&self, n: usize) -> &[String] {
        let from = self.lines.len().saturating_sub(n);
        &self.lines[from..]
    }
}

/// [ADR-0010] D2's append-only transcript.
///
/// The file handle is held open rather than reopened per record, and it is
/// **not** wrapped in a buffered writer. See the module documentation for
/// what each half of that buys.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug)]
pub struct Transcript {
    path: PathBuf,
    file: File,
}

impl Transcript {
    /// Open a transcript for appending, creating it at [`FILE_MODE`].
    ///
    /// The mode is set again after opening, because `OpenOptions::mode`
    /// applies only when the call creates the file and a transcript that
    /// already existed with the wrong mode is exactly what D5 says is the
    /// only protection there is.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Io`].
    pub fn append_to(path: impl Into<PathBuf>) -> Result<Self, TranscriptError> {
        let path = path.into();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&path)
            .map_err(|source| TranscriptError::Io {
                action: "open the transcript for appending",
                path: path.clone(),
                source,
            })?;
        fs::set_permissions(&path, fs::Permissions::from_mode(FILE_MODE)).map_err(|source| {
            TranscriptError::Io {
                action: "set 0600 on the transcript",
                path: path.clone(),
                source,
            }
        })?;
        Ok(Self { path, file })
    }

    /// The file this transcript appends to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record, and do not return until it is on disk.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::NotSerialisable`] and [`TranscriptError::Io`].
    pub fn record(&mut self, record: &Record) -> Result<(), TranscriptError> {
        // The newline joins the record in **one** buffer, so the line leaves
        // this process in one `write_all`. Two writes is the torn line.
        let mut line = serde_json::to_string(record)
            .map_err(|error| TranscriptError::NotSerialisable {
                detail: error.to_string(),
            })?
            .into_bytes();
        line.push(b'\n');

        self.file
            .write_all(&line)
            .map_err(|source| TranscriptError::Io {
                action: "append to the transcript",
                path: self.path.clone(),
                source,
            })?;
        self.file.flush().map_err(|source| TranscriptError::Io {
            action: "flush the transcript",
            path: self.path.clone(),
            source,
        })?;
        self.file.sync_data().map_err(|source| TranscriptError::Io {
            action: "sync the transcript",
            path: self.path.clone(),
            source,
        })
    }

    /// Read a transcript back, separating complete lines from a fragment.
    ///
    /// An absent file reads as empty rather than as an error: a session that
    /// has recorded nothing has not gone wrong.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Io`] and [`TranscriptError::Malformed`].
    pub fn read(path: &Path) -> Result<Reading, TranscriptError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(source) => {
                return Err(TranscriptError::Io {
                    action: "read the transcript",
                    path: path.to_path_buf(),
                    source,
                });
            }
        };

        // Split at the last newline: everything before it is complete lines,
        // everything after it is the event that was in flight.
        let (complete, fragment) = match bytes.iter().rposition(|byte| *byte == b'\n') {
            Some(last) => (&bytes[..=last], bytes.len() - last - 1),
            None => (&bytes[..0], bytes.len()),
        };

        let mut records = Vec::new();
        let mut lines = Vec::new();
        for (index, line) in complete
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .enumerate()
        {
            let record =
                serde_json::from_slice(line).map_err(|error| TranscriptError::Malformed {
                    path: path.to_path_buf(),
                    line: index + 1,
                    detail: error.to_string(),
                })?;
            records.push(record);
            // Lossy is right here and unreachable in practice: the line has
            // just parsed as JSON, which `serde_json` only does for valid
            // UTF-8. Converting fallibly would add an error variant for a
            // state the parse above has already excluded.
            lines.push(String::from_utf8_lossy(line).into_owned());
        }

        Ok(Reading {
            records,
            lines,
            fragment: (fragment > 0).then_some(fragment),
        })
    }
}
