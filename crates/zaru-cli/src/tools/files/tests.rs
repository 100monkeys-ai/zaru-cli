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

use crate::config::SizeCeiling;
use crate::tools::files::{CREATED_MODE, NAME_MATCH_PREFIX, edit, search, write};
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
    let edited = edit(&path, "HERE", "THERE");
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

    let edited = edit(&script, "new", "newer");
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

    let refused = edit(&path, "TARGET", "REPLACED");
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
    let unique = edit(&path, "gamma", "delta");
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

    let refused = edit(&path, "TARGET", "REPLACED");
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
    let edited = edit(&valid, "TARGET", "REPLACED");
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

    let same = edit(&path, "beta", "beta");
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

    let empty = edit(&path, "", "anything");
    assert_eq!(
        empty.exit_code, 1,
        "an empty string occurs everywhere, so it names no occurrence: {}",
        empty.stdout
    );
    println!("{}\n{}", same.stderr, empty.stderr);

    // The accepting arm, and it is the same file: a real replacement lands.
    let real = edit(&path, "beta", "delta");
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

/// A ceiling this file's checks pass, large enough that nothing is skipped
/// for its size unless a check meant it to be.
fn roomy() -> SizeCeiling {
    SizeCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

/// A search finds a literal in a file's contents and in a file's name.
///
/// ADR-0011 D1's row is "Content and filename search", which is two answers,
/// so both are asserted — and each with a sibling the other rule would not
/// find, so a search that only ever did one of the two reddens.
///
/// The mutant: dropping either half.
#[tokio::test]
async fn a_search_answers_on_contents_and_on_filenames() {
    let scratch = Scratch::new();
    std::fs::write(
        scratch.at("plain.txt"),
        "alpha\nthe NEEDLE is here\nomega\n",
    )
    .expect("staging: a content match");
    std::fs::write(scratch.at("NEEDLE-in-the-name.txt"), "nothing to find\n")
        .expect("staging: a name match");
    std::fs::write(scratch.at("quiet.txt"), "neither one nor the other\n")
        .expect("staging: a file that matches neither");

    let found = search(&scratch.0, "NEEDLE", roomy()).await;
    assert_eq!(found.exit_code, 0, "a search that ran: {}", found.stderr);
    println!("{}", found.stdout);

    assert!(
        found.stdout.contains("plain.txt:2: the NEEDLE is here"),
        "the content match is missing, with its line number and its line: {}",
        found.stdout
    );
    assert!(
        found.stdout.contains(&format!(
            "{NAME_MATCH_PREFIX}{}",
            scratch.at("NEEDLE-in-the-name.txt").display()
        )),
        "the filename match is missing: {}",
        found.stdout
    );
    assert!(
        !found.stdout.contains("quiet.txt"),
        "a file matching neither rule was reported, so the search matches everything: {}",
        found.stdout
    );
}

/// A conceptual query reaches a declaration even when it is not a literal
/// substring of one line. The result remains grounded: it is a path, a source
/// line, and the declaration the caller can immediately read with `fs.read`.
#[test]
fn a_search_falls_back_to_bounded_structural_code_retrieval() {
    let scratch = Scratch::new();
    std::fs::write(
        scratch.at("turn_clock.rs"),
        "/// Keeps the elapsed display moving during tool calls.\n\
         pub async fn refreshTurnClock() {}\n",
    )
    .expect("staging: a Rust declaration");
    std::fs::write(scratch.at("notes.txt"), "tool calls have no syntax tree\n")
        .expect("staging: ordinary text remains searchable");

    let found = search(&scratch.0, "turn_clock tool calls", roomy());
    assert_eq!(found.exit_code, 0, "retrieval ran: {}", found.stderr);
    assert!(
        found.stdout.contains("symbol:")
            && found
                .stdout
                .contains("turn_clock.rs:2: function refreshTurnClock"),
        "a structural hit needs a citable declaration, not an opaque score: {}",
        found.stdout
    );
    assert!(
        found
            .stdout
            .contains("elapsed display moving during tool calls"),
        "the bounded local context which made the conceptual match is absent: {}",
        found.stdout
    );
    assert!(
        !found.stdout.contains("notes.txt"),
        "an unsupported text file was misrepresented as a code declaration: {}",
        found.stdout
    );
}

/// **Security corpus.** A file over the caller's ceiling is named as skipped
/// and its contents are never read.
///
/// The mutant: reading the file and then checking its length, which is a
/// ceiling that bounds the report rather than the memory; and dropping the
/// skipped notice, which is how a model concludes a string is absent from a
/// tree nobody looked at all of.
#[tokio::test]
async fn a_file_over_the_ceiling_is_named_as_skipped_and_never_read() {
    let scratch = Scratch::new();
    let ceiling = SizeCeiling::new(64).expect("sixty-four is not zero");

    // The interesting file is neither first nor last in the sorted walk, and
    // there is a match on each side of it -- so "took the last one" and "took
    // the first one" are rules this staging separates from the one it is
    // named for (Verification lessons §54).
    std::fs::write(scratch.at("a-small.txt"), "NEEDLE early\n").expect("staging");
    std::fs::write(
        scratch.at("m-huge.txt"),
        format!("NEEDLE {}\n", "x".repeat(200)),
    )
    .expect("staging: a file over the ceiling");
    std::fs::write(scratch.at("z-small.txt"), "NEEDLE late\n").expect("staging");

    let found = search(&scratch.0, "NEEDLE", ceiling).await;
    println!("stdout:\n{}\nstderr:\n{}", found.stdout, found.stderr);

    assert!(
        !found.stdout.contains("m-huge.txt:"),
        "the oversized file's CONTENTS were reported, so it was read: {}",
        found.stdout
    );
    assert!(
        found.stderr.contains("m-huge.txt") && found.stderr.contains("over this search's ceiling"),
        "an oversized file must be named as skipped; a search that quietly did not look is how a \
         model concludes a string is absent: {}",
        found.stderr
    );
    assert!(
        !found.stdout.contains(&"x".repeat(200)),
        "the oversized file's bytes reached the caller: {}",
        found.stdout
    );

    // The accepting arms, on both sides of the skipped file. Without them a
    // search that skipped everything satisfies every assertion above.
    assert!(
        found.stdout.contains("a-small.txt:1: NEEDLE early"),
        "the small file before it was not searched: {}",
        found.stdout
    );
    assert!(
        found.stdout.contains("z-small.txt:1: NEEDLE late"),
        "the small file after it was not searched: {}",
        found.stdout
    );
}

/// **Security corpus.** The walk never follows a symbolic link, so everything
/// it opens is below the root the decision was reached about.
///
/// D4 classifies the root. Nothing re-classifies each file, because nothing
/// needs to — as long as the traversal cannot leave. This is that property,
/// asserted on the out-of-tree file's **own contents** rather than on its
/// path.
///
/// The mutant: `metadata` in place of `symlink_metadata`, which follows the
/// link and reports it as an ordinary directory.
#[tokio::test]
async fn a_search_never_follows_a_symbolic_link_out_of_its_root() {
    let scratch = Scratch::new();
    let inside = scratch.at("inside");
    let outside = scratch.at("outside");
    std::fs::create_dir(&inside).expect("staging");
    std::fs::create_dir(&outside).expect("staging");

    let sentinel = nonce("only-behind-the-link");
    std::fs::write(outside.join("secret.txt"), format!("NEEDLE {sentinel}\n")).expect("staging");
    std::fs::write(inside.join("ordinary.txt"), "NEEDLE in the tree\n").expect("staging");
    std::os::unix::fs::symlink(&outside, inside.join("escape")).expect("staging: the link");
    std::os::unix::fs::symlink(outside.join("secret.txt"), inside.join("shortcut.txt"))
        .expect("staging: a link to a file");

    let found = search(&inside, "NEEDLE", roomy()).await;
    println!("stdout:\n{}\nstderr:\n{}", found.stdout, found.stderr);

    assert!(
        !found.stdout.contains(&sentinel),
        "the walk followed a link out of its root and read what was behind it: {}",
        found.stdout
    );
    assert!(
        found.stderr.contains("escape") && found.stderr.contains("not followed"),
        "a link that was not followed is named as skipped rather than passed over: {}",
        found.stderr
    );

    // The accepting arm: the ordinary file in the same directory is found.
    assert!(
        found.stdout.contains("ordinary.txt:1: NEEDLE in the tree"),
        "the search found nothing at all, so the assertions above are about a walk that does not \
         walk: {}",
        found.stdout
    );

    // And a root that IS a link is refused outright rather than followed.
    let refused = search(&inside.join("escape"), "NEEDLE", roomy()).await;
    assert_eq!(
        refused.exit_code, 1,
        "a search rooted at a link is refused: {}",
        refused.stdout
    );
    assert!(
        !refused.stdout.contains(&sentinel),
        "a search rooted at a link read what was behind it: {}",
        refused.stdout
    );
    println!("{}", refused.stderr);
}

/// A file that is not UTF-8 is named as skipped and still matched by name.
///
/// The mutant: decoding lossily, which puts undecodable bytes into a model's
/// prompt as replacement characters and reports matches in text nobody wrote.
#[tokio::test]
async fn a_file_that_is_not_utf8_is_named_as_skipped_and_still_matched_by_name() {
    let scratch = Scratch::new();
    std::fs::write(scratch.at("NEEDLE.bin"), b"NEEDLE \xff\xfe rest").expect("staging");
    std::fs::write(scratch.at("ordinary.txt"), "NEEDLE here\n").expect("staging");

    let found = search(&scratch.0, "NEEDLE", roomy()).await;
    println!("stdout:\n{}\nstderr:\n{}", found.stdout, found.stderr);

    assert!(
        !found.stdout.contains("NEEDLE.bin:1:"),
        "the contents of a file that is not UTF-8 were reported: {}",
        found.stdout
    );
    assert!(
        found.stderr.contains("NEEDLE.bin") && found.stderr.contains("not UTF-8"),
        "a file whose contents were not searched is named: {}",
        found.stderr
    );
    assert!(
        found.stdout.contains(&format!(
            "{NAME_MATCH_PREFIX}{}",
            scratch.at("NEEDLE.bin").display()
        )),
        "its NAME still matches, which is the half that does not need the contents: {}",
        found.stdout
    );
    // The accepting arm.
    assert!(
        found.stdout.contains("ordinary.txt:1: NEEDLE here"),
        "the UTF-8 sibling was not searched: {}",
        found.stdout
    );
}

/// A search answers the same way twice, and an empty needle is refused.
///
/// The mutant: dropping the sort, which makes a search's answer a property of
/// the filesystem's own directory order rather than of the tree.
#[tokio::test]
async fn a_search_answers_in_a_stable_order_and_refuses_an_empty_needle() {
    let scratch = Scratch::new();
    for name in ["c.txt", "a.txt", "b.txt", "d.txt"] {
        std::fs::write(scratch.at(name), "NEEDLE\n").expect("staging");
    }
    let first = search(&scratch.0, "NEEDLE", roomy()).await;
    let second = search(&scratch.0, "NEEDLE", roomy()).await;
    assert_eq!(
        first.stdout, second.stdout,
        "two identical searches answered differently, so the order is the filesystem's"
    );
    let lines: Vec<&str> = first.stdout.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(
        lines, sorted,
        "the answer is not in a stable order: {lines:?}"
    );
    println!("{}", first.stdout);

    let refused = search(&scratch.0, "", roomy()).await;
    assert_eq!(
        refused.exit_code, 1,
        "an empty needle occurs everywhere, so it names no match: {}",
        refused.stdout
    );
    println!("{}", refused.stderr);
}
