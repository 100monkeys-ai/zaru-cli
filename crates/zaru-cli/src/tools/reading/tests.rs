// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for `fs.read` on real files.
//!
//! Each file is written here with `std::fs` and each expected line is built
//! from the same numbers the file was written from, never from what the read
//! returned.

use super::{
    DEFAULT_LINE_COUNT, DIRECTORY_ENTRIES_SHOWN, LINE_MARK, LONGEST_LINE_BYTES, Lines,
    READ_DESCRIPTION, read,
};
use crate::tools::fixtures::nonce;
use crate::tools::output::{Captured, ELISION_PREFIX, OutputBudget};
use std::path::PathBuf;

/// A directory this check owns, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("ft-reading"));
        std::fs::create_dir_all(&base).expect("staging: the scratch directory");
        Self(base)
    }

    fn file(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("staging: a file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The budget the binary passes.
fn budget() -> OutputBudget {
    crate::cli::layers::output_budget()
}

fn all() -> Lines {
    Lines::default()
}

fn from(start: usize, count: usize) -> Lines {
    Lines {
        start: Some(start),
        count: Some(count),
    }
}

/// The lines of an answer that carry a file line, as (number, text).
fn numbered(answer: &Captured) -> Vec<(usize, String)> {
    answer
        .stdout
        .lines()
        .filter_map(|line| {
            let (number, text) = line.split_once(LINE_MARK)?;
            Some((number.trim().parse().ok()?, text.to_owned()))
        })
        .collect()
}

/// The `start_line` an answer says to go on from, if it says one.
fn read_on_from(answer: &Captured) -> Option<usize> {
    let (_, after) = answer.stdout.split_once("call fs.read with start_line ")?;
    after
        .trim_end_matches('.')
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

/// A short file comes back whole, every line numbered, and says so.
#[test]
fn a_hundred_line_file_comes_back_whole_and_numbered() {
    let scratch = Scratch::new();
    let body: String = (1..=100)
        .map(|n| format!("line {n} of a hundred\n"))
        .collect();
    let path = scratch.file("hundred.txt", &body);

    let answer = read(&path, all(), budget());
    println!("{}", answer.stdout);
    assert_eq!(answer.exit_code, 0, "{}", answer.stderr);
    let lines = numbered(&answer);
    let expected: Vec<(usize, String)> = (1..=100)
        .map(|n| (n, format!("line {n} of a hundred")))
        .collect();
    assert_eq!(
        lines, expected,
        "the hundred lines are not all there, in order"
    );
    assert!(answer.stdout.contains("100 line(s)"), "{}", answer.stdout);
    assert!(
        answer.stdout.contains("That is the whole file."),
        "{}",
        answer.stdout
    );
}

/// A file longer than the default comes back from line 1 to the default
/// count, and says how many lines it has and where to read on.
#[test]
fn a_two_thousand_line_file_comes_back_to_the_default_and_says_where_to_go_on() {
    let scratch = Scratch::new();
    let body: String = (1..=2_000)
        .map(|n| format!("    value_{n} = compute({n})\n"))
        .collect();
    let path = scratch.file("two-thousand.py", &body);

    let answer = read(&path, all(), budget());
    let lines = numbered(&answer);
    assert_eq!(lines.len(), DEFAULT_LINE_COUNT, "{}", answer.stdout);
    assert_eq!(lines[0], (1, String::from("    value_1 = compute(1)")));
    assert!(answer.stdout.contains("2000 line(s)"), "{}", answer.stdout);
    assert_eq!(
        read_on_from(&answer),
        Some(DEFAULT_LINE_COUNT + 1),
        "the answer does not say where to read on: {}",
        answer.stdout
    );
    assert!(
        answer.stdout.len() < budget().get(),
        "the answer is over the budget"
    );
}

/// **A 50,000-line file read in three calls covers every line exactly once**,
/// each call starting where the one before said to.
///
/// The budget here is 720 KiB so that three calls cover it; at the binary's
/// 32 KiB the same walk takes about thirty calls. What is checked is that the
/// hint each answer gives is the right one: no line is skipped and none comes
/// twice.
#[test]
fn a_fifty_thousand_line_file_read_in_three_calls_covers_every_line_once() {
    let scratch = Scratch::new();
    let body: String = (1..=50_000).map(|n| format!("row {n}\n")).collect();
    let path = scratch.file("fifty-thousand.txt", &body);
    let roomy = OutputBudget::new(720 * 1024).expect("not zero");

    let mut seen: Vec<usize> = Vec::new();
    let mut start = 1;
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls <= 3, "more than three calls were needed");
        let answer = read(&path, from(start, 1_000_000), roomy);
        assert_eq!(answer.exit_code, 0, "{}", answer.stderr);
        for (number, text) in numbered(&answer) {
            assert_eq!(
                text,
                format!("row {number}"),
                "line {number} is not what was written"
            );
            seen.push(number);
        }
        match read_on_from(&answer) {
            Some(next) => start = next,
            None => break,
        }
    }
    println!("{calls} calls, {} lines", seen.len());
    assert_eq!(calls, 3, "the file was not read in three calls");
    let expected: Vec<usize> = (1..=50_000).collect();
    assert_eq!(
        seen, expected,
        "the three calls did not cover every line exactly once"
    );
}

/// Asked for more lines than fit, the answer stops early, says so, and is
/// never cut by the output budget.
#[test]
fn an_answer_is_sized_to_fit_the_budget_and_never_cut_in_the_middle() {
    let scratch = Scratch::new();
    let long = "y".repeat(190);
    let body: String = (1..=2_000).map(|n| format!("{n:05} {long}\n")).collect();
    let path = scratch.file("wide.txt", &body);

    let answer = read(&path, from(1, 2_000), budget());
    let lines = numbered(&answer);
    assert!(
        lines.len() > 50 && lines.len() < 2_000,
        "{} lines came back",
        lines.len()
    );
    let last = lines.last().expect("some lines").0;
    assert!(
        answer.stdout.contains(&format!("stopped at line {last}")),
        "the answer does not say it stopped early: {}",
        answer.stdout
    );
    assert_eq!(read_on_from(&answer), Some(last + 1));

    // What the model is shown, through the budget and the redactor.
    let presented = answer
        .present(budget(), &crate::redaction::HeldSecrets::none(), None)
        .expect("an answer that fits needs no overflow sink");
    assert!(
        !presented.stdout.as_str().contains(ELISION_PREFIX),
        "the output budget cut an fs.read answer in the middle"
    );
}

/// A line of a mebibyte is cut, and the cut says how long it was.
#[test]
fn a_line_of_a_mebibyte_is_cut_and_says_how_long_it_was() {
    let scratch = Scratch::new();
    let path = scratch.file("one-line.txt", format!("{}\n", "x".repeat(1 << 20)));

    let answer = read(&path, all(), budget());
    let lines = numbered(&answer);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].1.starts_with(&"x".repeat(LONGEST_LINE_BYTES)),
        "the first {LONGEST_LINE_BYTES} bytes are not kept"
    );
    assert!(
        lines[0].1.contains("this line is 1048576 bytes long"),
        "the cut does not say how long the line was: {}",
        &lines[0].1[LONGEST_LINE_BYTES..]
    );
    assert!(answer.stdout.len() < 4_096, "{} bytes", answer.stdout.len());
}

/// A cut falls between characters, never inside one.
#[test]
fn a_cut_falls_between_characters() {
    let scratch = Scratch::new();
    // One byte, then two-byte characters: the limit falls inside one.
    let path = scratch.file("wide-chars.txt", format!("a{}\n", "\u{e9}".repeat(3_000)));
    let answer = read(&path, all(), budget());
    let lines = numbered(&answer);
    let kept = lines[0].1.split(" [cut:").next().expect("a head");
    assert_eq!(
        kept.len(),
        LONGEST_LINE_BYTES - 1,
        "the cut is not on a boundary"
    );
    assert!(lines[0].1.contains("6001 bytes long"), "{}", lines[0].1);
}

/// A character split across two blocks of the read is not taken for a bad
/// byte.
#[test]
fn a_character_split_across_two_blocks_is_still_text() {
    let scratch = Scratch::new();
    // 65,535 bytes of ASCII, then a two-byte character across the first
    // 65,536-byte block's end.
    let mut body = "a".repeat(65_535);
    body.push('\u{e9}');
    body.push('\n');
    let path = scratch.file("straddle.txt", &body);
    let answer = read(&path, all(), budget());
    assert_eq!(answer.exit_code, 0, "{}", answer.stderr);
}

/// A binary file is refused, naming what it is and how large, and none of it
/// is sent.
#[test]
fn a_binary_file_is_refused_with_what_it_is_and_its_size() {
    let scratch = Scratch::new();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend((0..=255_u8).cycle().take(10_000));
    let path = scratch.file("logo.png", &png);
    let answer = read(&path, all(), budget());
    println!("{}", answer.stderr);
    assert_eq!(answer.exit_code, 1);
    assert!(answer.stdout.is_empty(), "some of the file was sent");
    assert!(answer.stderr.contains("a PNG image"), "{}", answer.stderr);
    assert!(answer.stderr.contains("10008 bytes"), "{}", answer.stderr);

    // No signature, a zero byte past the first block.
    let mut late = "text\n".repeat(20_000).into_bytes();
    late.push(0);
    let path = scratch.file("late.bin", &late);
    let answer = read(&path, all(), budget());
    assert_eq!(answer.exit_code, 1);
    assert!(answer.stderr.contains("binary data"), "{}", answer.stderr);
}

/// A file that is not UTF-8 is refused, naming where and how large.
#[test]
fn a_file_that_is_not_utf8_is_refused_naming_where_and_its_size() {
    let scratch = Scratch::new();
    let path = scratch.file("latin1.txt", b"first line\ncaf\xe9 cr\xe8me\n");
    let answer = read(&path, all(), budget());
    println!("{}", answer.stderr);
    assert_eq!(answer.exit_code, 1);
    assert!(answer.stdout.is_empty());
    assert!(
        answer.stderr.contains("offset 14, on line 2"),
        "{}",
        answer.stderr
    );
    assert!(answer.stderr.contains("22 bytes"), "{}", answer.stderr);

    // A character cut off by the end of the file.
    let path = scratch.file("truncated.txt", b"ok\n\xc3");
    let answer = read(&path, all(), budget());
    assert_eq!(answer.exit_code, 1);
    assert!(answer.stderr.contains("offset 3"), "{}", answer.stderr);
}

/// An empty file, a folder, a missing path, a device and a line past the
/// end each get their own plain message.
#[test]
fn an_empty_file_a_folder_a_missing_path_and_a_device_each_say_what_they_are() {
    let scratch = Scratch::new();

    let empty = read(&scratch.file("empty.txt", ""), all(), budget());
    assert_eq!(empty.exit_code, 0);
    assert!(empty.stdout.contains("is empty"), "{}", empty.stdout);

    let folder = scratch.0.join("folder");
    std::fs::create_dir(&folder).expect("staging");
    std::fs::create_dir(folder.join("a-folder")).expect("staging");
    for n in 0..60 {
        std::fs::write(folder.join(format!("f{n:02}.txt")), "x").expect("staging");
    }
    let answer = read(&folder, all(), budget());
    println!("{}", answer.stderr);
    assert_eq!(answer.exit_code, 1);
    assert!(answer.stderr.contains("is a folder, not a file"));
    assert!(
        answer
            .stderr
            .contains("It holds 61 entries: a-folder/, f00.txt, f01.txt"),
        "{}",
        answer.stderr
    );
    assert!(
        answer
            .stderr
            .contains(&format!("and {} more", 61 - DIRECTORY_ENTRIES_SHOWN)),
        "{}",
        answer.stderr
    );

    let missing = read(&scratch.0.join("not-there.txt"), all(), budget());
    assert_eq!(missing.exit_code, 1);
    assert!(
        missing.stderr.contains("there is no file or folder at"),
        "{}",
        missing.stderr
    );

    let device = read(std::path::Path::new("/dev/null"), all(), budget());
    assert_eq!(device.exit_code, 1);
    assert!(device.stderr.contains("is a device"), "{}", device.stderr);

    let short = scratch.file("short.txt", "one\ntwo\n");
    let past = read(&short, from(5, 1), budget());
    assert_eq!(past.exit_code, 1);
    assert!(
        past.stderr.contains("has 2 line(s), so there is no line 5"),
        "{}",
        past.stderr
    );
}

/// A range from the middle comes back with the right numbers, and says what
/// is not shown on both sides.
#[test]
fn a_range_from_the_middle_says_what_is_not_shown_on_both_sides() {
    let scratch = Scratch::new();
    let body: String = (1..=5_000).map(|n| format!("entry {n}\n")).collect();
    let path = scratch.file("five-thousand.txt", &body);
    let answer = read(&path, from(2_498, 5), budget());
    println!("{}", answer.stdout);
    let lines = numbered(&answer);
    let expected: Vec<(usize, String)> =
        (2_498..=2_502).map(|n| (n, format!("entry {n}"))).collect();
    assert_eq!(lines, expected);
    assert!(answer.stdout.contains("Lines 1 to 2497 are not shown."));
    assert!(answer.stdout.contains("Lines 2503 to 5000 are not shown."));
    assert_eq!(read_on_from(&answer), Some(2_503));
}

/// A file whose lines end in CRLF says so, and the carriage returns are not
/// in the lines shown. A file with no final newline says so.
#[test]
fn line_endings_and_a_missing_final_newline_are_named() {
    let scratch = Scratch::new();
    let crlf = read(&scratch.file("crlf.txt", "one\r\ntwo\r\n"), all(), budget());
    assert!(crlf.stdout.contains("lines end in CRLF"), "{}", crlf.stdout);
    assert_eq!(
        numbered(&crlf),
        vec![(1, String::from("one")), (2, String::from("two"))]
    );
    let bare = read(&scratch.file("bare.txt", "one\ntwo"), all(), budget());
    assert!(
        bare.stdout.contains("no newline at the end"),
        "{}",
        bare.stdout
    );
    assert!(!crlf.stdout.contains("no newline at the end"));
}

/// The description the model is sent quotes the numbers the tool uses.
#[test]
fn the_description_quotes_the_numbers_the_tool_uses() {
    assert!(READ_DESCRIPTION.contains(&format!("up to {DEFAULT_LINE_COUNT} lines")));
    assert!(READ_DESCRIPTION.contains("over 2,000 bytes"));
    assert_eq!(LONGEST_LINE_BYTES, 2_000);
    assert!(READ_DESCRIPTION.contains(LINE_MARK));
}
