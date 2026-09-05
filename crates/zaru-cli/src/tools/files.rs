// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The filesystem half of [ADR-0011] D1, acting on `std::fs` inside D4's
//! boundary.
//!
//! # Every path here arrives classified, and none is resolved twice
//!
//! Each function takes the resolved path out of the [`Target`] the permission
//! decision was reached about. **Nothing here calls
//! [`WorkingDirectory::classify`]**, because a second resolution is D4's rule
//! in two places and the second one is the one nothing prompted about.
//!
//! # A failure of the act is the work's, never the harness's
//!
//! Every function returns a [`Captured`] and never a `Result`: a read that
//! failed is a non-zero exit code with the operating system's own words on
//! standard error, exactly as a command that failed is. That is deliberate
//! rather than convenient. [ADR-0016] D1 has no row a filesystem error can be
//! read into without a clause the record does not carry — its own Update says
//! so, twice: "a port failure's class belongs to the port's implementation",
//! and `StoreError::Io`'s class "is the `io::ErrorKind`: a permission is the
//! user's, a disk fault is neither's", over a `#[non_exhaustive]` enum no
//! wildcard-free match can cover. So no class is claimed here, and D1's
//! missing clause is left to be written rather than pre-empted.
//!
//! # No refusal renders what is in a file
//!
//! `fs.edit`'s refusals name a **line and a column** and never the text there,
//! for the reason [`FileRefused`](crate::config::FileRefused) already refuses
//! to quote a malformed configuration line: a refusal is exactly the text that
//! gets pasted into a bug report, and the harness does not know what is on the
//! line it is refusing to edit.
//!
//! # What a write is, and the window it does not close
//!
//! A whole-file replacement goes through [`crate::atomic::write`] and nothing
//! else: a sibling `<path>.rewriting`, written, synced, and renamed over the
//! live file. A reader — a person, an editor, a build, a backup — sees the
//! whole old document or the whole new one and never anything in between.
//!
//! **Two things that follow are stated here rather than left to be
//! discovered.**
//!
//! On a crash between the sibling and the rename the live file is untouched
//! and the sibling is left behind — and for the first time that sibling is in
//! **the user's own project tree** rather than under `~/.zaru`, where it can be
//! committed, matched by a build glob, or picked up by a watcher. `atomic`'s
//! own note that "the next write truncates it" holds only if that same path is
//! written again. No cleanup-on-start is done, because a sweep that deleted
//! files matching a suffix in a user's tree is a worse hazard than the residue;
//! it is recorded on ADR-0011 for the record's author.
//!
//! And **a rename installs the temporary's mode over whatever the live file
//! had** — measured 2026-09-05, a `0600` temporary renamed over a `0755` file
//! leaves `0600`. `atomic` was written for files the harness owns, where that
//! is the property it wanted; here it would strip the executable bit off a
//! script a model edited. So the mode is read off the existing file first, by
//! a private `mode_for`, and the setuid, setgid and sticky bits are
//! deliberately not carried across.
//!
//! # The boundary is a check at a moment, and a write is where that costs most
//!
//! Between the instant a path is resolved and the instant the sibling is
//! opened, a segment that did not exist can be created as a symlink out of the
//! tree, and `std` offers no `openat2(RESOLVE_BENEATH)` to close it. **No
//! containment is claimed against a concurrent attacker.** At `bare` that is
//! inside what [ADR-0011] D2 already says out loud — "the harness is not a
//! sandbox and says so" — and at `contained` [ADR-0004]'s membrane is the
//! answer that does not depend on winning a race.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`Target`]: crate::tools::tree::Target
//! [`WorkingDirectory::classify`]: crate::tools::tree::WorkingDirectory::classify

use crate::config::Position;
use crate::tools::output::Captured;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The mode a file this surface creates is opened at.
///
/// `0o666` rather than a narrower constant, so the **process umask** decides —
/// which is what `std::fs::write` does, and therefore what a file the user's
/// own editor creates gets. A model-created source file that nothing else on
/// the machine can read would be a permission the harness invented.
pub const CREATED_MODE: u32 = 0o666;

/// A capture that says the act failed, in the operating system's own words.
fn failed(detail: String) -> Captured {
    Captured {
        exit_code: 1,
        stdout: String::new(),
        stderr: detail,
    }
}

/// A capture that says the act succeeded.
fn succeeded(text: String) -> Captured {
    Captured {
        exit_code: 0,
        stdout: text,
        stderr: String::new(),
    }
}

/// The mode a replacement of `path` is written at.
///
/// The file's own permission bits where there is a file, and [`CREATED_MODE`]
/// where there is not.
///
/// # The setuid, setgid and sticky bits are deliberately not carried
///
/// The mask is `0o777`. A file that was setuid and is rewritten by a
/// model-driven action comes back without it, which is a permission the user
/// granted to the *original* file and not to the rewrite. Losing it is the
/// direction to be wrong in on this surface, and it is recorded on ADR-0011
/// rather than left for a reader to find by `stat`.
fn mode_for(path: &Path) -> u32 {
    std::fs::symlink_metadata(path).map_or(CREATED_MODE, |metadata| {
        metadata.permissions().mode() & 0o777
    })
}

/// Read a file. `fs.read`.
///
/// The contents are decoded **lossily**, because this is the path that shows a
/// model text and a file it cannot read is more useful shown with replacement
/// characters than refused. [`edit`] does the opposite for the opposite reason.
pub(crate) fn read(path: &Path) -> Captured {
    match std::fs::read(path) {
        Ok(bytes) => succeeded(String::from_utf8_lossy(&bytes).into_owned()),
        Err(source) => failed(format!("could not read {}: {source}", path.display())),
    }
}

/// List a directory. `fs.list`.
///
/// Entries are sorted, because a directory's own order is a property of the
/// filesystem rather than of the directory, and a listing that changes order
/// between two identical calls is a listing a model cannot reason about.
pub(crate) fn list(path: &Path) -> Captured {
    let reading = match std::fs::read_dir(path) {
        Ok(reading) => reading,
        Err(source) => return failed(format!("could not list {}: {source}", path.display())),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in reading {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(source) => {
                return failed(format!("could not list {}: {source}", path.display()));
            }
        }
    }
    names.sort();
    succeeded(names.join("\n"))
}

/// Create or overwrite a file. `fs.write`.
///
/// # It never creates a directory, and that is the built-in set staying closed
///
/// [ADR-0011] D1's set is seven and the platform's is twelve; `fs.create_dir`
/// is one of the five this record deliberately does not have. A write that
/// made its own parent would be that eighth built-in arriving through a side
/// door — "small is the security posture, not an ergonomic compromise". So a
/// missing parent is refused, naming it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub(crate) fn write(path: &Path, contents: &str) -> Captured {
    if path.is_dir() {
        return failed(format!(
            "{} is a directory, so there is nothing to write there",
            path.display()
        ));
    }
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() && !parent.is_dir() => {
            return failed(format!(
                "the directory {} does not exist. fs.write creates a file and never a directory: \
                 ADR-0011 D1's built-in set is seven and has no fs.create_dir, and a write that \
                 made its own parent would be an eighth built-in",
                parent.display()
            ));
        }
        _ => {}
    }

    let bytes = contents.as_bytes();
    match crate::atomic::write(path, bytes, mode_for(path)) {
        Ok(()) => succeeded(format!(
            "wrote {} byte(s) to {}",
            bytes.len(),
            path.display()
        )),
        // The failure's own wording and never the contents: this function was
        // handed a whole file, and a message that quoted it would publish it.
        Err(failure) => failed(format!(
            "could not {} for {}: {}",
            failure.action,
            failure.path.display(),
            failure.source
        )),
    }
}

/// Replace an exact string within a file, once. `fs.edit`.
///
/// # The read is strict, and `fs.read`'s is not
///
/// [`read`] decodes lossily because it shows a model text. This one must not:
/// a lossy decode replaces every invalid sequence with U+FFFD, and writing the
/// result back would **destroy those bytes** in a file the user owns. A file
/// that is not UTF-8 is refused, naming the offset of the first invalid byte.
///
/// # Exactly one occurrence, and never a fuzzy match
///
/// [ADR-0011] D1's row is "Replace an exact string within a file". Zero
/// occurrences is a refusal; more than one is a refusal naming **every** place
/// as a line and a column, so the caller can make the string unique. Nothing
/// is normalised — not whitespace, not line endings, not case — because a
/// match the caller did not ask for is a rewrite of a file they did not
/// intend.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub(crate) fn edit(path: &Path, old: &str, new: &str) -> Captured {
    if old.is_empty() {
        return failed(String::from(
            "the string to replace is empty, which occurs everywhere in every file. fs.edit \
             replaces one exact occurrence, so it needs a string to find",
        ));
    }
    if old == new {
        return failed(format!(
            "the string to replace and its replacement are the same, so this edit of {} would \
             rewrite the file and change nothing",
            path.display()
        ));
    }

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) => return failed(format!("could not read {}: {source}", path.display())),
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            return failed(format!(
                "{} is not UTF-8: the first byte that is not is at offset {}. fs.edit rewrites the \
                 whole file, and decoding it lossily would replace what could not be read and then \
                 write the replacement back over what was there",
                path.display(),
                error.utf8_error().valid_up_to()
            ));
        }
    };

    let places: Vec<Position> = text
        .match_indices(old)
        .map(|(at, _)| Position::of(&text, at))
        .collect();
    if places.is_empty() {
        return failed(format!(
            "the string to replace does not occur in {}. fs.edit replaces an exact string and \
             never a fuzzy match, so nothing was changed",
            path.display()
        ));
    }
    if places.len() > 1 {
        let named: Vec<String> = places.iter().map(Position::to_string).collect();
        return failed(format!(
            "the string to replace occurs {} times in {}, at {}. fs.edit replaces exactly one \
             occurrence, so nothing was changed; make the string unique by including more of what \
             surrounds it. What is at each place is not shown, because a refusal that quoted it \
             would publish whatever is on those lines",
            places.len(),
            path.display(),
            named.join("; ")
        ));
    }

    let replaced = text.replacen(old, new, 1);
    let bytes = replaced.as_bytes();
    match crate::atomic::write(path, bytes, mode_for(path)) {
        Ok(()) => succeeded(format!(
            "replaced one occurrence in {}, which is now {} byte(s)",
            path.display(),
            bytes.len()
        )),
        Err(failure) => failed(format!(
            "could not {} for {}: {}",
            failure.action,
            failure.path.display(),
            failure.source
        )),
    }
}

#[cfg(test)]
mod tests;
