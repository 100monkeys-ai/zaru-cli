// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `fs.read`: a text file, a part at a time, sized to fit.
//!
//! # What was wrong
//!
//! Until 2026-09-28 `fs.read` read the whole file into memory and handed it
//! to the output budget, which kept the first and last 16 KiB and cut out the
//! middle. The model could not see the middle, could not ask for it, and could
//! not edit what it could not see: on a real question the answer sat at line
//! 919 of a 54 KB file, and the model read the file twice, got the same two
//! ends both times, and ran out of turns. A binary file went to the model as
//! control bytes, and a file that was not UTF-8 went with its bad bytes
//! replaced.
//!
//! # What it does now
//!
//! It takes a first line and a count, both optional. It reads the file once,
//! a block at a time, keeping only the lines asked for, and it stops keeping
//! them before the answer would grow past what the budget holds. So **the
//! output budget never cuts an `fs.read` answer in the middle**: the answer is
//! sized here and says plainly what is left and where to start to read on.
//! It counts every line, so it can say how many the file has.
//!
//! Each line comes back as its number, the mark [`LINE_MARK`], and the line.
//! The header says the number and the mark are not in the file.
//!
//! A binary file, a file that is not UTF-8, an empty file, a folder, a path
//! with nothing at it and something that is not a regular file each get their
//! own plain message.
//!
//! Ruled by the coordinator at the `file-tools` arc's spawn under Jeshua's
//! directive 58, recorded on ADR-0011 D1 and D5, open to his veto.
//!
//! # The boundary is untouched
//!
//! This module is handed the path the permission decision was reached about,
//! exactly as `files::read` was, and resolves nothing again. Where the file
//! is, who is asked, and what is marked are all decided before it is called.

use crate::tools::output::{Captured, OutputBudget};
use std::io::Read;
use std::path::Path;

/// How many lines an `fs.read` returns when the call does not say.
///
/// **500, chosen from the output budget.** The budget is 32 KiB for one
/// result. A line of source code is about 40 bytes and its number and mark add
/// about 6, so 500 lines is about 23 KB and leaves room for the header and for
/// longer lines. When the lines are longer than that, the answer stops sooner
/// and says so; the count is an upper bound, the budget is the limit.
pub const DEFAULT_LINE_COUNT: usize = 500;

/// How long a line may be before it is cut, in bytes.
///
/// **2,000**, so that at least fifteen of the longest lines fit in one answer
/// and one minified line cannot fill it. The cut says how long the line was.
pub const LONGEST_LINE_BYTES: usize = 2_000;

/// How many of a folder's entries the message for a folder names.
pub const DIRECTORY_ENTRIES_SHOWN: usize = 50;

/// What separates a line's number from the line in an `fs.read` answer.
///
/// A box-drawing bar rather than a tab or a colon, because a tab and a colon
/// both start real lines of real files, and the model must never take the
/// number for part of the line.
pub const LINE_MARK: &str = "\u{2502}";

/// What the model is told `fs.read` does.
///
/// The numbers are [`DEFAULT_LINE_COUNT`] and [`LONGEST_LINE_BYTES`], written
/// out because a constant cannot be spliced into a `&'static str`. A check
/// holds the text to the constants.
pub const READ_DESCRIPTION: &str = "Read a UTF-8 text file. Optional \
    start_line (from 1) and line_count choose the lines; by default you get up \
    to 500 lines from line 1, fewer if they are long. The answer says how many \
    lines the file has, which it shows, and the start_line to read on from. \
    Each line starts with its number and \u{2502}, which are not in the file: \
    do not copy them into fs.edit. Lines over 2,000 bytes are cut. Binary and \
    non-UTF-8 files are refused.";

/// The first bytes looked at to tell a binary file from text.
const SNIFF_BYTES: usize = 8 * 1024;

/// How much of a file is read at a time.
const BLOCK_BYTES: usize = 64 * 1024;

/// Which lines a call asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lines {
    /// The first line wanted, counting from 1. `None` is line 1.
    pub start: Option<usize>,
    /// How many lines at most. `None` is [`DEFAULT_LINE_COUNT`].
    pub count: Option<usize>,
}

/// A capture that says the act failed.
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

/// Read `lines` of the file at `path`, in an answer that fits `budget`.
///
/// `path` is the path the permission decision was reached about. The answer
/// is at most fifteen sixteenths of the budget: the rest is room for a
/// redaction marker longer than the value it replaces, so that the budget's
/// own cut never lands in it.
#[must_use]
pub fn read(path: &Path, lines: Lines, budget: OutputBudget) -> Captured {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return failed(format!(
                "there is no file or folder at {}, so nothing was read",
                path.display()
            ));
        }
        Err(source) => return failed(format!("could not read {}: {source}", path.display())),
    };
    if metadata.is_dir() {
        return folder(path);
    }
    if !metadata.is_file() {
        return failed(format!(
            "{} is {}, not a regular file, so it was not read",
            path.display(),
            kind_of_special(&metadata)
        ));
    }

    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(source) => return failed(format!("could not read {}: {source}", path.display())),
    };

    let start = lines.start.unwrap_or(1);
    let count = lines.count.unwrap_or(DEFAULT_LINE_COUNT);
    let cap = budget.get() - budget.get() / 16;
    let room = cap.saturating_sub(700 + path.as_os_str().len());
    let mut scan = Scan::new(start, start.saturating_add(count - 1), room);

    let mut block = vec![0_u8; BLOCK_BYTES];
    let mut carry: Vec<u8> = Vec::new();
    let mut offset: u64 = 0;
    let mut sniffed = false;
    loop {
        let got = match file.read(&mut block) {
            Ok(0) => break,
            Ok(got) => got,
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(source) => return failed(format!("could not read {}: {source}", path.display())),
        };
        let fresh = &block[..got];
        if !sniffed {
            sniffed = true;
            let head = &fresh[..fresh.len().min(SNIFF_BYTES)];
            if let Some(what) = binary_kind(head) {
                return failed(binary(path, what, metadata.len()));
            }
        }
        // A zero byte past the first block is binary too. It is valid UTF-8,
        // so nothing else would stop it.
        if fresh.contains(&0) {
            return failed(binary(path, "binary data", metadata.len()));
        }

        // `data` starts where the carried bytes started in the file.
        let base = offset - carry.len() as u64;
        let mut data = std::mem::take(&mut carry);
        data.extend_from_slice(fresh);
        let valid = match std::str::from_utf8(&data) {
            Ok(_) => data.len(),
            Err(error) => {
                if error.error_len().is_some() {
                    let bad = base + error.valid_up_to() as u64;
                    scan.take(&data[..error.valid_up_to()]);
                    return failed(not_utf8(path, bad, scan.lines + 1, metadata.len()));
                }
                error.valid_up_to()
            }
        };
        scan.take(&data[..valid]);
        carry = data[valid..].to_vec();
        offset += got as u64;
    }
    if !carry.is_empty() {
        let bad = offset - carry.len() as u64;
        return failed(not_utf8(path, bad, scan.lines + 1, metadata.len()));
    }
    scan.finish();

    if scan.bytes == 0 {
        return succeeded(format!(
            "{} is empty: it has no lines and 0 bytes",
            path.display()
        ));
    }
    if start > scan.lines {
        return failed(format!(
            "{} has {} line(s), so there is no line {start}. Ask for a start_line from 1 to {}",
            path.display(),
            scan.lines,
            scan.lines
        ));
    }
    succeeded(scan.answer(path, cap))
}

/// One line kept for the answer.
struct Kept {
    number: usize,
    text: String,
}

/// The state of one pass over a file.
struct Scan {
    from: usize,
    to: usize,
    room: usize,
    kept: Vec<Kept>,
    kept_bytes: usize,
    stopped_for_room: bool,
    lines: usize,
    bytes: u64,
    crlf: usize,
    lf: usize,
    last_byte: Option<u8>,
    current: Vec<u8>,
    current_len: usize,
    current_last: Option<u8>,
    current_wanted: bool,
    at_line_start: bool,
}

impl Scan {
    fn new(from: usize, to: usize, room: usize) -> Self {
        Self {
            from,
            to,
            room,
            kept: Vec::new(),
            kept_bytes: 0,
            stopped_for_room: false,
            lines: 0,
            bytes: 0,
            crlf: 0,
            lf: 0,
            last_byte: None,
            current: Vec::new(),
            current_len: 0,
            current_last: None,
            current_wanted: false,
            at_line_start: true,
        }
    }

    /// Take a run of bytes that is whole, valid UTF-8.
    fn take(&mut self, text: &[u8]) {
        for &byte in text {
            if self.at_line_start {
                self.at_line_start = false;
                let number = self.lines + 1;
                self.current_wanted =
                    !self.stopped_for_room && number >= self.from && number <= self.to;
            }
            self.bytes += 1;
            self.last_byte = Some(byte);
            if byte == b'\n' {
                self.end_line(true);
                continue;
            }
            self.current_len += 1;
            self.current_last = Some(byte);
            // Four bytes past the limit, so a character that straddles it is
            // whole and the cut can fall on its boundary.
            if self.current_wanted && self.current.len() < LONGEST_LINE_BYTES + 4 {
                self.current.push(byte);
            }
        }
    }

    /// The file has ended.
    fn finish(&mut self) {
        if !self.at_line_start {
            self.end_line(false);
        }
    }

    fn end_line(&mut self, by_newline: bool) {
        let number = self.lines + 1;
        self.lines = number;
        let carriage = self.current_last == Some(b'\r');
        if by_newline {
            if carriage {
                self.crlf += 1;
            } else {
                self.lf += 1;
            }
        }
        if self.current_wanted {
            let length = self.current_len - usize::from(carriage);
            let text = if length > LONGEST_LINE_BYTES {
                let mut end = LONGEST_LINE_BYTES;
                while end > 0 && !is_boundary(&self.current, end) {
                    end -= 1;
                }
                let head = String::from_utf8_lossy(&self.current[..end]);
                format!(
                    "{head} [cut: this line is {length} bytes long, and only its first {end} are \
                     shown]"
                )
            } else {
                String::from_utf8_lossy(&self.current[..length]).into_owned()
            };
            // Its number and mark take at most 24 bytes more.
            let size = text.len() + 24;
            if !self.kept.is_empty() && self.kept_bytes + size > self.room {
                self.stopped_for_room = true;
            } else {
                self.kept_bytes += size;
                self.kept.push(Kept { number, text });
            }
        }
        self.current.clear();
        self.current_len = 0;
        self.current_last = None;
        self.current_wanted = false;
        self.at_line_start = true;
    }

    /// The answer the model reads.
    fn answer(&self, path: &Path, cap: usize) -> String {
        let first = self.kept.first().map_or(self.from, |kept| kept.number);
        let last = self.kept.last().map_or(self.from, |kept| kept.number);
        let width = last.to_string().len();

        let mut about = vec![
            format!("{} line(s)", self.lines),
            format!("{} bytes", self.bytes),
        ];
        match (self.crlf, self.lf) {
            (0, _) => {}
            (_, 0) => about.push(String::from("lines end in CRLF (\\r\\n)")),
            (crlf, lf) => about.push(format!(
                "mixed line endings: {crlf} end in CRLF (\\r\\n) and {lf} in LF (\\n)"
            )),
        }
        if self.last_byte != Some(b'\n') {
            about.push(String::from("no newline at the end"));
        }
        let mut out = format!("{}: {}.\n", path.display(), about.join(", "));
        if first == last {
            out.push_str(&format!("Line {first} is below."));
        } else {
            out.push_str(&format!("Lines {first} to {last} are below."));
        }
        out.push_str(&format!(
            " Each begins with its number and \"{LINE_MARK}\", which are not part of the file.\n"
        ));
        for kept in &self.kept {
            out.push_str(&format!(
                "{:>width$}{LINE_MARK}{}\n",
                kept.number, kept.text
            ));
        }

        if first == 1 && last == self.lines {
            out.push_str("That is the whole file.");
            return out;
        }
        let mut notes = Vec::new();
        if first > 1 {
            notes.push(format!("Lines 1 to {} are not shown.", first - 1));
        }
        if last < self.lines {
            if self.stopped_for_room {
                notes.push(format!(
                    "The answer stopped at line {last} to stay under {cap} bytes."
                ));
            }
            notes.push(format!(
                "Lines {} to {} are not shown. To read on, call fs.read with start_line {}.",
                last + 1,
                self.lines,
                last + 1
            ));
        }
        out.push_str(&notes.join(" "));
        out
    }
}

/// Whether `at` is the start of a character in `bytes`, or its end.
fn is_boundary(bytes: &[u8], at: usize) -> bool {
    at >= bytes.len() || (bytes[at] & 0b1100_0000) != 0b1000_0000
}

/// What a file that starts with `head` is, when it is plainly not text.
fn binary_kind(head: &[u8]) -> Option<&'static str> {
    const KNOWN: [(&[u8], &str); 9] = [
        (b"\x89PNG\r\n\x1a\n", "a PNG image"),
        (b"\xff\xd8\xff", "a JPEG image"),
        (b"GIF87a", "a GIF image"),
        (b"GIF89a", "a GIF image"),
        (b"%PDF-", "a PDF document"),
        (b"PK\x03\x04", "a ZIP archive, or a file packed like one"),
        (b"\x1f\x8b", "a gzip-compressed file"),
        (b"\x7fELF", "a compiled program (ELF)"),
        (b"\0asm", "a WebAssembly module"),
    ];
    for (magic, what) in KNOWN {
        if head.starts_with(magic) {
            return Some(what);
        }
    }
    head.contains(&0).then_some("binary data")
}

/// The message for a file that is not text.
fn binary(path: &Path, what: &str, size: u64) -> String {
    format!(
        "{} is {what}, not text. It has {size} bytes. fs.read shows text only, so nothing was \
         read",
        path.display()
    )
}

/// The message for a file that is not UTF-8.
fn not_utf8(path: &Path, offset: u64, line: usize, size: u64) -> String {
    format!(
        "{} is not UTF-8 text: the byte at offset {offset}, on line {line}, is not valid UTF-8. \
         It has {size} bytes. It may be in another encoding, such as Latin-1, or be binary. \
         fs.read shows UTF-8 text only, so nothing was read",
        path.display()
    )
}

/// What a path that is neither a file nor a folder is.
fn kind_of_special(metadata: &std::fs::Metadata) -> &'static str {
    use std::os::unix::fs::FileTypeExt;
    let kind = metadata.file_type();
    if kind.is_fifo() {
        "a named pipe"
    } else if kind.is_socket() {
        "a socket"
    } else if kind.is_block_device() || kind.is_char_device() {
        "a device"
    } else {
        "something other than a file or a folder"
    }
}

/// The message for a folder: what it holds, up to [`DIRECTORY_ENTRIES_SHOWN`].
fn folder(path: &Path) -> Captured {
    let reading = match std::fs::read_dir(path) {
        Ok(reading) => reading,
        Err(source) => return failed(format!("could not list {}: {source}", path.display())),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in reading {
        match entry {
            Ok(entry) => {
                let mut name = entry.file_name().to_string_lossy().into_owned();
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    name.push('/');
                }
                names.push(name);
            }
            Err(source) => {
                return failed(format!("could not list {}: {source}", path.display()));
            }
        }
    }
    names.sort();
    let held = if names.is_empty() {
        String::from("It is empty.")
    } else {
        let shown = &names[..names.len().min(DIRECTORY_ENTRIES_SHOWN)];
        let more = names.len() - shown.len();
        let mut held = format!(
            "It holds {} entr{}: {}",
            names.len(),
            if names.len() == 1 { "y" } else { "ies" },
            shown.join(", ")
        );
        if more > 0 {
            held.push_str(&format!(", and {more} more"));
        }
        held.push('.');
        held
    };
    failed(format!(
        "{} is a folder, not a file, so there is nothing to read. {held} Call fs.read on a file \
         in it, or fs.list to see all of it",
        path.display()
    ))
}

#[cfg(test)]
mod tests;
