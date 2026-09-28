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
/// # It says whether it replaced a file
///
/// Since 2026-09-28 the answer says whether the file was created or
/// replaced, and how large the old one was, so a model that meant to create
/// a file learns it has overwritten one.
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
                 the built-in set is seven and has no fs.create_dir, and a write that \
                 made its own parent would be an eighth built-in",
                parent.display()
            ));
        }
        _ => {}
    }

    // Read before the write, so the answer can say what was replaced.
    let replaced = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let bytes = contents.as_bytes();
    match crate::atomic::write(path, bytes, mode_for(path)) {
        Ok(()) => succeeded(match replaced {
            Some(old) => format!(
                "replaced {}: the file that was there had {old} byte(s), and it now has {} \
                 byte(s) in {} line(s)",
                path.display(),
                bytes.len(),
                line_count(contents)
            ),
            None => format!(
                "created {} with {} byte(s) in {} line(s)",
                path.display(),
                bytes.len(),
                line_count(contents)
            ),
        }),
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

/// How many lines `text` has: one per newline, and one more for a last line
/// with no newline after it.
fn line_count(text: &str) -> usize {
    text.matches('\n').count() + usize::from(!text.is_empty() && !text.ends_with('\n'))
}

/// How many lines of the nearest match an absent edit shows, at most.
const NEAREST_LINES_SHOWN: usize = 40;

/// How many changed places an edit's answer lists by line, at most.
const CHANGES_LISTED: usize = 20;

/// Replace exact text within a file. `fs.edit`.
///
/// # The read is strict
///
/// A lossy decode replaces every invalid sequence with U+FFFD, and writing
/// the result back would **destroy those bytes** in a file the user owns. A
/// file that is not UTF-8 is refused, naming the offset of the first invalid
/// byte. A file with a zero byte is refused as binary.
///
/// # One occurrence unless the call says every one, and never a fuzzy match
///
/// [ADR-0011] D1's row is "Replace an exact string within a file". Zero
/// occurrences is a refusal. More than one is a refusal naming how many and
/// **every** place as a line and a column, and never the text there — unless
/// the call sets `all`, which replaces every one. Nothing is normalised: not
/// whitespace, not case.
///
/// # Line endings and the final newline are the file's
///
/// Since 2026-09-28. A model writes `\n`. In a file whose lines all end in
/// CRLF, the text to replace and its replacement are given CRLF endings, so a
/// match written with `\n` is found and the file stays CRLF throughout. In a
/// file with mixed endings the text is matched as written first, and with
/// CRLF endings only if that finds nothing. A file that ended with a newline
/// still does after the edit, and a file that did not still does not; the
/// answer says when either had to be restored.
///
/// # When the text is not there, the answer shows where it nearly is
///
/// Since 2026-09-28, the refusal for absent text shows the lines of the
/// nearest match, numbered as `fs.read` numbers them: the lines whose first
/// line matches the text's first line once spaces and case are ignored. It
/// quotes the file, where the refusal for text that occurs twice does not: a
/// model that gets text wrong needs to see what is there to get it right, it
/// could read those lines with `fs.read` anyway, and the answer passes the
/// same redaction as any other result.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub(crate) fn edit(path: &Path, old: &str, new: &str, all: bool) -> Captured {
    if old.is_empty() {
        return failed(String::from(
            "the string to replace is empty, which occurs everywhere in every file. fs.edit \
             replaces exact text, so it needs text to find",
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
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return failed(format!(
                "there is no file at {}, so nothing was changed. fs.write creates a file",
                path.display()
            ));
        }
        Err(source) => return failed(format!("could not read {}: {source}", path.display())),
    };
    if bytes.contains(&0) {
        return failed(format!(
            "{} is binary data, not text: it has {} bytes. fs.edit changes text only, so nothing \
             was changed",
            path.display(),
            bytes.len()
        ));
    }
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

    let endings = Endings::of(&text);
    let crlf = |s: &str| s.replace("\r\n", "\n").replace('\n', "\r\n");
    let (old, new, given_crlf) = if endings == Endings::Crlf {
        (
            crlf(old),
            crlf(new),
            old.contains('\n') || new.contains('\n'),
        )
    } else if endings == Endings::Mixed
        && !text.contains(old)
        && old.contains('\n')
        && text.contains(&crlf(old))
    {
        (crlf(old), crlf(new), true)
    } else {
        (old.to_owned(), new.to_owned(), false)
    };

    let places: Vec<usize> = text.match_indices(old.as_str()).map(|(at, _)| at).collect();
    if places.is_empty() {
        return failed(absent(path, &text, &old));
    }
    if places.len() > 1 && !all {
        let named: Vec<String> = places
            .iter()
            .map(|at| Position::of(&text, *at).to_string())
            .collect();
        return failed(format!(
            "the string to replace occurs {} times in {}, at {}. Nothing was changed. fs.edit \
             replaces one occurrence unless all is true: include more of the lines around it so it \
             occurs once, or set all to true to replace every one. What is at each place is not \
             shown, because a refusal that quoted it would publish whatever is on those lines",
            places.len(),
            path.display(),
            named.join("; ")
        ));
    }

    let mut replaced = if all {
        text.replace(old.as_str(), &new)
    } else {
        text.replacen(old.as_str(), &new, 1)
    };
    let newline = if endings == Endings::Crlf {
        "\r\n"
    } else {
        "\n"
    };
    let mut restored = None;
    if text.ends_with('\n') && !replaced.is_empty() && !replaced.ends_with('\n') {
        replaced.push_str(newline);
        restored = Some("The file ended with a newline, so one was kept at its end.");
    } else if !text.ends_with('\n') && replaced.ends_with('\n') {
        let cut = if replaced.ends_with("\r\n") { 2 } else { 1 };
        replaced.truncate(replaced.len() - cut);
        restored = Some("The file did not end with a newline, so the one at its end was removed.");
    }

    let changes = changed_lines(&text, &replaced, &places, old.len(), new.len());
    let written = replaced.as_bytes();
    match crate::atomic::write(path, written, mode_for(path)) {
        Ok(()) => {
            let mut said = format!(
                "replaced {} occurrence(s) in {}: {}. The file now has {} line(s) and {} byte(s).",
                places.len(),
                path.display(),
                changes,
                line_count(&replaced),
                written.len()
            );
            if given_crlf {
                said.push_str(" Its lines end in CRLF, so the new text was given CRLF endings.");
            }
            if let Some(restored) = restored {
                said.push(' ');
                said.push_str(restored);
            }
            succeeded(said)
        }
        Err(failure) => failed(format!(
            "could not {} for {}: {}",
            failure.action,
            failure.path.display(),
            failure.source
        )),
    }
}

/// How a file's lines end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endings {
    /// Every newline is a bare LF, or there is none.
    Lf,
    /// Every newline is CRLF.
    Crlf,
    /// Some of each.
    Mixed,
}

impl Endings {
    fn of(text: &str) -> Self {
        let newlines = text.matches('\n').count();
        let crlf = text.matches("\r\n").count();
        match (crlf, newlines) {
            (0, _) => Self::Lf,
            (crlf, newlines) if crlf == newlines => Self::Crlf,
            _ => Self::Mixed,
        }
    }
}

/// The line, counting from 1, that byte `at` of `text` is on, from the
/// offsets of its newlines.
fn line_at(newlines: &[usize], at: usize) -> usize {
    newlines.partition_point(|newline| *newline < at) + 1
}

/// Which lines each replacement took and which it became, in plain words.
fn changed_lines(
    before: &str,
    after: &str,
    places: &[usize],
    old_len: usize,
    new_len: usize,
) -> String {
    let newlines_before: Vec<usize> = before.match_indices('\n').map(|(at, _)| at).collect();
    let newlines_after: Vec<usize> = after.match_indices('\n').map(|(at, _)| at).collect();
    let span = |first: usize, last: usize| {
        if first == last {
            format!("line {first}")
        } else {
            format!("lines {first} to {last}")
        }
    };
    let mut said: Vec<String> = Vec::new();
    for (k, at) in places.iter().enumerate() {
        if said.len() == CHANGES_LISTED {
            said.push(format!("and {} more", places.len() - CHANGES_LISTED));
            break;
        }
        let was = span(
            line_at(&newlines_before, *at),
            line_at(&newlines_before, at + old_len - 1),
        );
        // Where this replacement starts in the new text: every earlier one
        // moved it by the difference in length.
        let moved = at - k * old_len + k * new_len;
        let became = if new_len == 0 {
            format!(
                "removed, so the text around it now meets on line {}",
                line_at(&newlines_after, moved)
            )
        } else {
            format!(
                "became {}",
                span(
                    line_at(&newlines_after, moved),
                    line_at(&newlines_after, moved + new_len - 1),
                )
            )
        };
        said.push(format!("{was} {became}"));
    }
    said.join("; ")
}

/// The refusal for text that does not occur, with the nearest lines.
fn absent(path: &Path, text: &str, old: &str) -> String {
    let mut said = format!(
        "the string to replace does not occur in {}, so nothing was changed. fs.edit needs the \
         exact text, with the same spaces, indentation and line breaks, and without the line \
         numbers fs.read shows.",
        path.display()
    );
    let squeeze = |line: &str| {
        line.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let wanted: Vec<&str> = old.split('\n').collect();
    let Some((skip, probe)) = wanted
        .iter()
        .enumerate()
        .map(|(n, line)| (n, squeeze(line)))
        .find(|(_, line)| !line.is_empty())
    else {
        said.push_str(" The text to replace is only spaces and line breaks.");
        return said;
    };
    // A newline ends a line; it does not start one, so the empty piece after
    // the last newline is not a line of the file.
    let lines: Vec<&str> = text
        .strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect();
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| squeeze(line) == probe)
        .map(|(n, _)| n.saturating_sub(skip))
        .collect();
    let Some(first) = starts.first() else {
        said.push_str(
            " No line of the file matches the text's first line, even when spaces and case are \
             ignored. Read the file with fs.read to see what is there.",
        );
        return said;
    };
    let shown = wanted.len().clamp(1, NEAREST_LINES_SHOWN);
    let last = (first + shown).min(lines.len());
    if starts.len() == 1 {
        said.push_str(&format!(
            " The nearest match starts at line {}, where the first line matches once spaces and \
             case are ignored.",
            first + 1
        ));
    } else {
        let named: Vec<String> = starts
            .iter()
            .take(10)
            .map(|n| (n + 1).to_string())
            .collect();
        said.push_str(&format!(
            " {} places nearly match, starting at lines {}; the first is shown.",
            starts.len(),
            named.join(", ")
        ));
    }
    said.push_str(&format!(
        " Lines {} to {} of the file are:\n",
        first + 1,
        last
    ));
    let width = last.to_string().len();
    for (n, line) in lines[*first..last].iter().enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let mut end = line.len().min(crate::tools::reading::LONGEST_LINE_BYTES);
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        said.push_str(&format!(
            "{:>width$}{}{}\n",
            first + n + 1,
            crate::tools::reading::LINE_MARK,
            &line[..end]
        ));
    }
    said.push_str("Copy the text from there, without the line numbers.");
    said
}

#[cfg(test)]
mod tests;
