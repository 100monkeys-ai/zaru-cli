// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the filesystem acts themselves, on real files.
//!
//! Every one reads back off the disk with `std::fs` rather than asking the
//! function what it did ([Verification lessons] §10 and §11: at least one arm
//! of a comparison must not travel through the thing being checked). The
//! boundary cases — a target outside the working directory, a symlink out of
//! it — are in `tools::execute::tests`, because they are about the classified
//! path a decision was reached on and these functions never classify one.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::tools::files::{CREATED_MODE, edit, write};
use crate::tools::fixtures::nonce;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A directory this check owns, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("ft-files"));
        std::fs::create_dir_all(&base).expect("staging: the scratch directory");
        Self(base)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("the file is on disk")
        .permissions()
        .mode()
        & 0o777
}

/// A write puts the bytes there and an edit replaces exactly one occurrence.
///
/// The mutant: `replacen(old, new, 1)` widened to `replace`, which rewrites
/// every occurrence of a string the caller said was unique.
#[test]
fn a_write_puts_the_bytes_there_and_an_edit_replaces_one_occurrence() {
    let scratch = Scratch::new();
    let path = scratch.at("a.txt");
    let contents = format!("hello {}\n", nonce("written"));

    let wrote = write(&path, &contents);
    assert_eq!(wrote.exit_code, 0, "the write failed: {}", wrote.stderr);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file is on disk"),
        contents,
        "the bytes on disk are not the bytes that were asked for"
    );
    println!("fs.write -> {}", wrote.stdout);

    // Exactly one of the two lines carries the string being replaced, and it
    // is the SECOND -- so a mutant that edits the first match, or the whole
    // file, produces a different result from one that edits the right place.
    let before = "keep this line\nreplace HERE please\nkeep this too\n";
    std::fs::write(&path, before).expect("staging");
    let edited = edit(&path, "HERE", "THERE", false);
    assert_eq!(edited.exit_code, 0, "the edit failed: {}", edited.stderr);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file is on disk"),
        "keep this line\nreplace THERE please\nkeep this too\n",
        "the edit changed something other than the one occurrence"
    );
    println!("fs.edit -> {}", edited.stdout);
}

/// A replacement keeps the file's own permission bits.
///
/// **This is a measurement rather than a preference.** A rename installs the
/// temporary's mode over whatever the live file had — measured 2026-09-05, a
/// `0600` temporary renamed over a `0755` file leaves `0600` — so a constant
/// here would silently strip the executable bit off a script a model edited.
///
/// The mutant: passing a constant to `crate::atomic::write` instead of
/// `mode_for`.
#[test]
fn a_replacement_keeps_the_files_own_mode_and_a_new_file_takes_the_umasks() {
    let scratch = Scratch::new();

    // An existing file with a mode nothing here would choose.
    let script = scratch.at("script.sh");
    std::fs::write(&script, "#!/bin/sh\necho old\n").expect("staging");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("staging");
    assert_eq!(mode_of(&script), 0o755, "staging: the mode was not set");

    let wrote = write(&script, "#!/bin/sh\necho new\n");
    assert_eq!(wrote.exit_code, 0, "the write failed: {}", wrote.stderr);
    assert_eq!(
        mode_of(&script),
        0o755,
        "a rename installs the temporary's mode, so a file the model rewrote came back without \
         the executable bit its owner gave it"
    );

    let edited = edit(&script, "new", "newer", false);
    assert_eq!(edited.exit_code, 0, "the edit failed: {}", edited.stderr);
    assert_eq!(mode_of(&script), 0o755, "an edit is a replacement too");

    // A file that did not exist takes what the umask leaves of `CREATED_MODE`.
    // The expected value is read from a file `std::fs::write` created in the
    // same directory rather than computed here: a second reader that does not
    // travel through the function under test, and one that cannot disagree
    // with the machine's umask (Verification lessons §11).
    let reference = scratch.at("reference.txt");
    std::fs::write(&reference, "x").expect("staging");
    let fresh = scratch.at("fresh.txt");
    let wrote = write(&fresh, "x");
    assert_eq!(wrote.exit_code, 0, "the write failed: {}", wrote.stderr);
    assert_eq!(
        mode_of(&fresh),
        mode_of(&reference),
        "a file this surface creates carries what the process umask leaves of {CREATED_MODE:o}, \
         which is what a file the user's own editor creates carries"
    );
    println!(
        "existing {:o}; created {:o}, and std::fs::write's own is {:o}",
        0o755,
        mode_of(&fresh),
        mode_of(&reference)
    );
}

/// **Security corpus.** An edit whose string occurs twice is refused, naming
/// every place, and never the text at those places.
///
/// Two mutants. Editing the first match anyway, which rewrites a file the
/// caller did not describe; and quoting the matching line in the refusal,
/// which publishes whatever is on it — the rule
/// `crate::config::FileRefused` already holds one layer up.
#[test]
fn an_edit_whose_string_occurs_twice_is_refused_naming_every_place() {
    let scratch = Scratch::new();
    let path = scratch.at("twice.txt");

    // Each matching line carries a token that exists nowhere else in this
    // file, so "the refusal does not quote the line" is an assertion about
    // bytes rather than about a shape. The tokens are fixed rather than
    // generated, because the columns below are counted by hand from these
    // exact strings and a generated token would make the expected column a
    // thing computed from the fixture — which is the tautology Verification
    // lessons §10 and §12 name.
    //
    // Line 1, `alpha ONLYONLINEONE TARGET`: six characters, then thirteen,
    // then a space, so `TARGET` starts at column 21.
    // Line 2, `bé ONLYONLINETWO TARGET`: three characters -- one of them two
    // bytes -- then thirteen, then a space, so `TARGET` starts at column 18
    // and at BYTE 19. The `é` is there for exactly that: it is the axis the
    // byte-versus-character mutant moves (§51), and without it the fixture is
    // awkward on no axis at all.
    let before = "alpha ONLYONLINEONE TARGET\nb\u{e9} ONLYONLINETWO TARGET\ngamma\n".to_owned();
    let first = "ONLYONLINEONE";
    let second = "ONLYONLINETWO";
    std::fs::write(&path, &before).expect("staging");

    let refused = edit(&path, "TARGET", "REPLACED", false);
    assert_eq!(
        refused.exit_code, 1,
        "an ambiguous edit must not act: {}",
        refused.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file is on disk"),
        before,
        "the file was changed by an edit that was refused"
    );

    assert!(
        refused.stderr.contains("line 1, column 21"),
        "the refusal must name the first place: {}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("line 2, column 18"),
        "the refusal must name the second place, and a column counted in bytes rather than in \
         characters puts it at 19: {}",
        refused.stderr
    );
    assert!(
        !refused.stderr.contains(first) && !refused.stderr.contains(second),
        "the refusal quoted what is on the matching lines, and the harness does not know what is \
         on them: {}",
        refused.stderr
    );
    println!("fs.edit refused: {}", refused.stderr);

    // The accepting arm. Without it every assertion above is satisfied by an
    // `edit` that refuses everything.
    let unique = edit(&path, "gamma", "delta", false);
    assert_eq!(
        unique.exit_code, 0,
        "a unique string edits: {}",
        unique.stderr
    );
    assert!(
        std::fs::read_to_string(&path)
            .expect("on disk")
            .contains("delta"),
        "the unique edit did not land"
    );
}

/// An edit refuses a file that is not UTF-8 rather than rewriting it lossily.
///
/// The mutant: `String::from_utf8_lossy`, which is what `fs.read` correctly
/// uses and what this path must not — a lossy decode replaces every invalid
/// sequence with U+FFFD and the rewrite writes the replacement back over what
/// was there.
#[test]
fn an_edit_refuses_a_file_that_is_not_utf8_rather_than_rewriting_it() {
    let scratch = Scratch::new();
    let path = scratch.at("binary.dat");
    let before: Vec<u8> = b"keep TARGET \xff\xfe and this".to_vec();
    std::fs::write(&path, &before).expect("staging");

    let refused = edit(&path, "TARGET", "REPLACED", false);
    assert_eq!(
        refused.exit_code, 1,
        "a file that is not UTF-8 must not be rewritten: {}",
        refused.stdout
    );
    assert!(
        refused.stderr.contains("offset 12"),
        "the refusal names where the decode stopped: {}",
        refused.stderr
    );
    assert_eq!(
        std::fs::read(&path).expect("the file is on disk"),
        before,
        "the file was rewritten by an edit that was refused, and the bytes that are not UTF-8 are \
         exactly the ones a lossy decode would have destroyed"
    );
    println!("fs.edit refused: {}", refused.stderr);

    // The accepting arm: the same edit on the same bytes made valid.
    let valid = scratch.at("text.txt");
    std::fs::write(&valid, "keep TARGET and this").expect("staging");
    let edited = edit(&valid, "TARGET", "REPLACED", false);
    assert_eq!(edited.exit_code, 0, "a UTF-8 file edits: {}", edited.stderr);
}

/// A write never creates a directory, so D1's set stays seven.
///
/// The mutant: `create_dir_all` on the parent, which is `fs.create_dir` — one
/// of the five platform tools ADR-0011 D1 deliberately does not have —
/// arriving through a side door.
#[test]
fn a_write_creates_a_file_and_never_a_directory() {
    let scratch = Scratch::new();
    let missing = scratch.at("not-there");
    let path = missing.join("a.txt");

    let refused = write(&path, "hello");
    assert_eq!(
        refused.exit_code, 1,
        "a write into a directory that does not exist must be refused: {}",
        refused.stdout
    );
    assert!(
        !missing.exists(),
        "the write created {}, which is fs.create_dir arriving as an eighth built-in",
        missing.display()
    );
    assert!(
        !path.exists(),
        "the file exists and its directory does not, which is not a state a filesystem has"
    );
    println!("fs.write refused: {}", refused.stderr);

    // The accepting arm: the same write once the directory is there.
    std::fs::create_dir(&missing).expect("staging");
    let wrote = write(&path, "hello");
    assert_eq!(wrote.exit_code, 0, "the write failed: {}", wrote.stderr);
    assert_eq!(
        std::fs::read_to_string(&path).expect("on disk"),
        "hello",
        "the write did not land once its directory existed"
    );

    // And a directory is not a file to write into.
    let onto_directory = write(&missing, "hello");
    assert_eq!(
        onto_directory.exit_code, 1,
        "a directory is not something to write bytes into"
    );
    println!("fs.write refused: {}", onto_directory.stderr);
}

/// An edit that would change nothing is refused rather than rewriting a file.
///
/// The mutant: dropping either guard, which turns `fs.edit` into a way of
/// replacing a file with itself — a rename, a new inode, and a `.rewriting`
/// sibling risked, for no change.
#[test]
fn an_edit_that_would_change_nothing_is_refused() {
    let scratch = Scratch::new();
    let path = scratch.at("same.txt");
    std::fs::write(&path, "alpha beta gamma\n").expect("staging");

    let same = edit(&path, "beta", "beta", false);
    assert_eq!(
        same.exit_code, 1,
        "a no-op edit is refused: {}",
        same.stdout
    );
    assert!(
        same.stderr.contains("change nothing"),
        "the refusal says what is wrong: {}",
        same.stderr
    );

    let empty = edit(&path, "", "anything", false);
    assert_eq!(
        empty.exit_code, 1,
        "an empty string occurs everywhere, so it names no occurrence: {}",
        empty.stdout
    );
    println!("{}\n{}", same.stderr, empty.stderr);

    // The accepting arm, and it is the same file: a real replacement lands.
    let real = edit(&path, "beta", "delta", false);
    assert_eq!(
        real.exit_code, 0,
        "a real edit still works: {}",
        real.stderr
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("on disk"),
        "alpha delta gamma\n"
    );
}

/// An edit keeps what the file had: CRLF endings in a mixed file where the
/// text was written with LF, and a final newline the replacement dropped. A
/// binary file, a missing file and text with no near match are each refused
/// saying which, and nothing is written.
#[test]
fn an_edit_keeps_endings_and_the_final_newline_and_refuses_plainly() {
    let scratch = Scratch::new();

    let mixed = scratch.at("mixed.txt");
    std::fs::write(&mixed, "a\r\nb\nc\r\n").expect("staging");
    let edited = edit(&mixed, "a\nb", "x\ny", false);
    assert_eq!(edited.exit_code, 0, "{}", edited.stderr);
    assert_eq!(
        std::fs::read(&mixed).expect("on disk"),
        b"x\r\ny\nc\r\n",
        "in a mixed file, text written with LF is matched with CRLF and replaced with CRLF"
    );

    let ended = scratch.at("ended.txt");
    std::fs::write(&ended, "one\ntwo\n").expect("staging");
    let edited = edit(&ended, "two\n", "three", false);
    assert_eq!(edited.exit_code, 0, "{}", edited.stderr);
    assert_eq!(
        std::fs::read_to_string(&ended).expect("on disk"),
        "one\nthree\n",
        "a file that ended with a newline lost it"
    );
    assert!(
        edited.stdout.contains("so one was kept"),
        "{}",
        edited.stdout
    );

    let only = edit(&ended, "one", "ONE", true);
    assert_eq!(only.exit_code, 0, "all with one occurrence replaces it");
    assert!(
        only.stdout.contains("replaced 1 occurrence(s)"),
        "{}",
        only.stdout
    );

    let binary = scratch.at("binary.bin");
    std::fs::write(&binary, b"text\0more").expect("staging");
    let refused = edit(&binary, "text", "TEXT", false);
    assert_eq!(refused.exit_code, 1);
    assert!(refused.stderr.contains("binary data"), "{}", refused.stderr);
    assert_eq!(std::fs::read(&binary).expect("on disk"), b"text\0more");

    let missing = edit(&scratch.at("nope.txt"), "a", "b", false);
    assert_eq!(missing.exit_code, 1);
    assert!(
        missing.stderr.contains("there is no file at"),
        "{}",
        missing.stderr
    );

    // The nearest lines stop at the file's last line.
    let near = scratch.at("near.py");
    std::fs::write(&near, "def f():\n    return 1\n").expect("staging");
    let absent = edit(&near, "def f():\n  return 1\n", "x", false);
    assert_eq!(absent.exit_code, 1);
    assert!(
        absent.stderr.contains("Lines 1 to 2 of the file are:")
            && !absent.stderr.contains("3\u{2502}"),
        "the nearest lines went past the end of the file: {}",
        absent.stderr
    );

    let far = edit(&ended, "nothing like this", "x", false);
    assert_eq!(far.exit_code, 1);
    assert!(
        far.stderr.contains("No line of the file matches"),
        "{}",
        far.stderr
    );
}

/// A refused write leaves no sibling temporary behind.
///
/// The residue this arc records on the record is the one a **crash** between
/// the sibling and the rename leaves. A refusal is not that, and a refusal
/// that left one would put a stray file in a user's tree on every mistake.
#[test]
fn a_refused_write_leaves_no_sibling_behind() {
    let scratch = Scratch::new();
    let path = scratch.at("gone");
    std::fs::create_dir(&path).expect("staging: a directory where a file was asked for");

    let refused = write(&path, "hello");
    assert_eq!(refused.exit_code, 1, "a directory is not a file");

    let leftovers: Vec<String> = std::fs::read_dir(&scratch.0)
        .expect("the scratch directory is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.contains(crate::atomic::TEMPORARY_SUFFIX))
        .collect();
    assert!(
        leftovers.is_empty(),
        "a refused write left a sibling temporary in the tree: {leftovers:?}"
    );
}
