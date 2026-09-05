// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Replacing a file the harness owns, without a window in which it is half a
//! document.
//!
//! # Why this is one function and not two copies
//!
//! Writing in place truncates the file and then fills it, so there is a window
//! in which it is empty or partial — to a reader, to a backup, and to whatever
//! is left behind if the process dies inside it. Writing to a sibling
//! temporary, syncing it, and renaming over the live file closes that window
//! entirely: a reader sees the whole old document or the whole new one and
//! never anything between.
//!
//! [ADR-0010] D3's checkpoint had this discipline from the day it landed. The
//! credential store did not: it truncated and rewrote, which was survivable
//! while its file carried only metadata and stopped being survivable the
//! moment that file began carrying **the only copy of every sealed secret** —
//! a torn write there is not a lost turn, it is every credential the user
//! has. So the discipline is lifted here rather than copied, because the two
//! copies would have been a rule in two places, and a rule in two places is a
//! rule that diverges ([Verification lessons] §30's shape, one level up).
//!
//! It lives at the crate root rather than in `session` or in `credentials`
//! because it belongs to neither: making `credentials` depend on `session` to
//! borrow a file-writing discipline would be an edge invented for a
//! convenience.
//!
//! # The sibling is in the same directory, and that is not incidental
//!
//! `rename` is atomic within a filesystem, and a same-directory sibling is the
//! only placement that guarantees the two are on one filesystem regardless of
//! where the user's mounts are.
//!
//! # The mode is set on the temporary, not fixed up afterwards
//!
//! A rename carries the source's mode — measured 2026-09-04 by the session
//! arc, and re-measured here for a `0600` credential store: a `0600` temporary
//! renamed over a live file leaves `0600`. Creating the temporary at the mode
//! is what stops the replacement file from ever existing, even briefly, at
//! anything wider.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The suffix the sibling temporary carries.
///
/// A leftover from a killed process is never mistaken for the live file,
/// because it does not have the live file's name, and the next write truncates
/// it.
pub const TEMPORARY_SUFFIX: &str = ".rewriting";

/// A step of the replacement failed, and which one.
///
/// Carries no contents: this function is handed the bytes of a checkpoint or of
/// a credential store, and a failure that quoted them would publish whichever
/// it was.
#[derive(Debug)]
pub struct WriteFailed {
    /// What was being attempted, for a caller that wraps this in its own words.
    pub action: &'static str,
    /// The path it was attempted on.
    pub path: PathBuf,
    /// What the operating system said.
    pub source: std::io::Error,
}

/// The sibling `path` is replaced through.
#[must_use]
pub fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(TEMPORARY_SUFFIX);
    PathBuf::from(name)
}

/// Replace `path` with `contents`, at `mode`, atomically.
///
/// # Errors
///
/// [`WriteFailed`] for the open, the write, the sync or the rename, naming
/// which.
pub fn write(path: &Path, contents: &[u8], mode: u32) -> Result<(), WriteFailed> {
    let temporary = temporary_path(path);

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(mode)
        .open(&temporary)
        .map_err(|source| WriteFailed {
            action: "open the sibling temporary for writing",
            path: temporary.clone(),
            source,
        })?;
    file.write_all(contents).map_err(|source| WriteFailed {
        action: "write the sibling temporary",
        path: temporary.clone(),
        source,
    })?;
    // Sync before the rename, not after: a rename that reached the directory
    // before the bytes reached the file is a live file pointing at a hole.
    file.sync_all().map_err(|source| WriteFailed {
        action: "sync the sibling temporary",
        path: temporary.clone(),
        source,
    })?;
    drop(file);

    fs::rename(&temporary, path).map_err(|source| WriteFailed {
        action: "rename the sibling temporary over the file",
        path: path.to_path_buf(),
        source,
    })
}
