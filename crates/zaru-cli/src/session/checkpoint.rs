// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D3's checkpoint: `context.json`, rewritten rather than
//! appended.
//!
//! D3: "`context.json` holds what the model needs to continue... It is
//! overwritten each turn. Conflating the two is the mistake that makes resume
//! unreliable: compaction discards, and a store that both discards and is
//! expected to be a complete record cannot be either."
//!
//! # The contents are ADR-0013's and this module does not read them
//!
//! What is in the checkpoint is [ADR-0013]'s layering and compaction, and
//! since 2026-09-05 the type that knows its shape is
//! [`SessionContext`](crate::compose::SessionContext): `checkpoint()` produces
//! the document and `restored()` reads it back, so layer 6's spelling has one
//! home rather than a writer here and a reader there.
//!
//! The value here stays an opaque [`serde_json::Value`]: this module writes it
//! whole, reads it whole, and interprets no field. **Nothing here compacts
//! anything** — that is the boundary's, one crate module away, and the
//! separation is D3's own: "compaction discards, and a store that both
//! discards and is expected to be a complete record cannot be either".
//!
//! # The rewrite is a rename, and the reason is not a crash
//!
//! Writing in place truncates the file and then fills it, so there is a
//! window in which `context.json` is empty or half a document — to a reader,
//! to a backup, and to whatever is left behind if the process dies inside it.
//! Writing to a sibling temporary file, syncing it, and renaming over the
//! live one closes the window entirely: a reader sees the whole old document
//! or the whole new one and never anything between.
//!
//! **The sibling is in the same directory**, not merely on the same
//! filesystem. `rename` is atomic within a filesystem and a same-directory
//! sibling is the only placement that guarantees that regardless of where the
//! user's mounts are.
//!
//! The temporary file is created at [`FILE_MODE`] rather than fixed up
//! afterwards, because a rename carries the source's mode: measured
//! 2026-09-04, a `0600` temporary renamed over a live file leaves `0600`.
//!
//! **The discipline itself now lives in [`crate::atomic`]**, because the
//! credential store needs exactly the same one and two copies of it would be a
//! rule in two places. What stays here is why a checkpoint wants it.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

use crate::session::store::FILE_MODE;
use core::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The suffix the sibling temporary file carries.
///
/// Re-exported from [`crate::atomic`] rather than declared again: the
/// credential store replaces its own file through the same discipline, and a
/// suffix declared in two places is a suffix that diverges. A session
/// directory has one writer, so one name is enough, and a leftover temporary
/// from a killed process is never mistaken for the checkpoint because it is
/// not called `context.json`.
pub use crate::atomic::TEMPORARY_SUFFIX;

/// The checkpoint could not be rewritten or read.
///
/// **No variant carries the checkpoint's contents.** D3 makes this the
/// conversation state, which is whatever the session held.
#[derive(Debug)]
pub enum CheckpointError {
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The checkpoint did not parse, or could not be rendered.
    Malformed {
        /// The file.
        path: PathBuf,
        /// What the parser said. Positional; it quotes no field value.
        detail: String,
    },
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
            Self::Malformed { path, detail } => write!(
                f,
                "the checkpoint at {} did not parse: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } => None,
        }
    }
}

/// [ADR-0010] D3's `context.json`.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    path: PathBuf,
}

impl Checkpoint {
    /// The checkpoint at a path.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The sibling the rewrite goes through.
    #[must_use]
    pub fn temporary_path(&self) -> PathBuf {
        crate::atomic::temporary_path(&self.path)
    }

    /// Rewrite the checkpoint, atomically.
    ///
    /// A concurrent reader sees the whole previous document or the whole new
    /// one, never anything between, and a process that dies inside this call
    /// leaves the previous one intact.
    ///
    /// # Errors
    ///
    /// [`CheckpointError::Malformed`] when the state cannot be rendered, and
    /// [`CheckpointError::Io`] for the write, the sync or the rename.
    pub fn write(&self, state: &serde_json::Value) -> Result<(), CheckpointError> {
        let rendered = serde_json::to_vec(state).map_err(|error| CheckpointError::Malformed {
            path: self.path.clone(),
            detail: error.to_string(),
        })?;
        crate::atomic::write(&self.path, &rendered, FILE_MODE).map_err(|failure| {
            CheckpointError::Io {
                action: failure.action,
                path: failure.path,
                source: failure.source,
            }
        })
    }

    /// Read the checkpoint back, whole.
    ///
    /// `None` for a session that has not checkpointed yet, which is not an
    /// error: D3 overwrites it each turn and a session with no turns has had
    /// none.
    ///
    /// # Errors
    ///
    /// [`CheckpointError::Io`] and [`CheckpointError::Malformed`].
    pub fn read(&self) -> Result<Option<serde_json::Value>, CheckpointError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(CheckpointError::Io {
                    action: "read the checkpoint",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| CheckpointError::Malformed {
                path: self.path.clone(),
                detail: error.to_string(),
            })
    }
}
