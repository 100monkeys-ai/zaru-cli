// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `zaru` binary, driven the way a person drives it.
//!
//! # Why this file exists rather than more unit checks
//!
//! Every other record's Status tracking in this workspace carries the same
//! sentence: "that capture is evidence about the mechanism and **must not be
//! quoted as evidence about the binary**". This file is the other kind. It
//! runs the artefact Cargo built, with arguments, and reads its standard
//! output, its standard error and its process status.
//!
//! # The scratch home is what makes it honest
//!
//! `SessionStore::default_root` and `CredentialStore::default_root` both go
//! through `std::env::home_dir`, which on this platform reads `$HOME` —
//! measured rather than assumed, by running a probe binary with `HOME` set and
//! with it unset. So each check gives the child its own `HOME` and the child
//! writes to the paths the product actually writes to, inside it. That is
//! [Testing]'s "what a test's own root is here", applied to a process.
//!
//! The environment is **cleared** rather than inherited, and that is not
//! tidiness. [ADR-0014] D5 refuses any `ZARU_*` variable that maps to no
//! declared key, so a developer with one exported would see every check here
//! fail for a reason that has nothing to do with the check.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A scratch `$HOME` that removes itself.
struct Home {
    path: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-cli-from-outside-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch home under the temporary directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// What one run of the binary produced.
struct Ran {
    stdout: String,
    stderr: String,
    code: i32,
}

impl Ran {
    fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }
}

/// Run the built binary with a scratch home and a cleared environment.
fn zaru(home: &Home, arguments: &[&str]) -> Ran {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .output()
        .expect("failed to execute the built zaru binary");

    let ran = Ran {
        stdout: String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("zaru printed invalid UTF-8 on stderr"),
        code: output
            .status
            .code()
            .expect("the binary was killed by a signal rather than exiting"),
    };

    println!("-- zaru {} --", arguments.join(" "));
    for line in ran.stdout.lines() {
        println!("   {line}");
    }
    for line in ran.stderr.lines() {
        println!(" ! {line}");
    }
    println!("   exit {}", ran.code);
    ran
}

/// ADR-0016 D5's `0` and `2`, on the artefact rather than on the mapping.
///
/// **This is the clause moving.** That record's Status tracking has said since
/// 2026-09-04 that "from the real artefact only `0` is observable" and that
/// "`zaru` takes no arguments, so nothing a user can do makes it fail". Both
/// halves change here, and neither `1`, `3`, `4` nor `70` does: there is still
/// no loop to exhaust, no network to be unreachable, no tier that withholds
/// anything, and no honest way to make the binary panic.
#[test]
fn the_built_binary_reaches_adr_0016_d5s_zero_and_two_and_no_other_code() {
    let home = Home::new("exit-codes");

    let succeeded: Vec<(&[&str], i32)> = vec![
        (&["--help"], 0),
        (&["--version"], 0),
        (&["runtime"], 0),
        (&["models"], 0),
        (&["config", "explain", "runtime.tier"], 0),
    ];
    let refused: Vec<(&[&str], i32)> = vec![
        (&["runtim"], 2),
        (&["stack"], 2),
        (&["--runtime", "sandboxed", "runtime"], 2),
        (&["config", "explain", "runtime.nonsense"], 2),
        (&["--runtime", "bare", "--runtime", "linked"], 2),
    ];

    let mut wrong = Vec::new();
    for (arguments, expected) in succeeded.iter().chain(refused.iter()) {
        let ran = zaru(&home, arguments);
        if ran.code != *expected {
            wrong.push((arguments.join(" "), ran.code, *expected));
        }
    }

    assert!(
        wrong.is_empty(),
        "the built binary exited with something other than ADR-0016 D5's code for what it did \
         (command, got, wanted): {wrong:?}"
    );
}

/// A refusal goes to standard error and the data goes to standard output.
///
/// D5's argument is that CI wraps this harness. A wrapper reading `zaru
/// models` off a pipe must not one day take a refusal for a row.
#[test]
fn what_a_wrapper_reads_off_the_pipe_is_never_a_refusal() {
    let home = Home::new("streams");

    let listing = zaru(&home, &["models"]);
    assert_eq!(listing.code, 0);
    assert!(
        listing.stderr.is_empty(),
        "a successful listing wrote to standard error: {:?}",
        listing.stderr
    );
    assert_eq!(
        listing.lines().len(),
        5,
        "ADR-0012 D2's five aliases, one line each: {:?}",
        listing.lines()
    );

    let refusal = zaru(&home, &["runtim"]);
    assert_eq!(refusal.code, 2);
    assert!(
        refusal.stdout.is_empty(),
        "a refusal reached standard output, where a wrapper reads its data: {:?}",
        refusal.stdout
    );
    assert!(
        refusal.stderr.contains("zaru runtime"),
        "the refusal must name what to run instead: {:?}",
        refusal.stderr
    );
}

/// A bare `zaru` prints the help, and its first line is still its version.
///
/// The version line is load-bearing twice: `tests/version.rs` reads it for the
/// release checklist's reason, and a user typing the name of a program and
/// nothing else is owed an answer to the question they asked.
#[test]
fn a_bare_zaru_prints_the_help_and_the_composition_moved_behind_a_flag() {
    let home = Home::new("bare");

    let bare = zaru(&home, &[]);
    assert_eq!(bare.code, 0);
    assert_eq!(
        bare.lines().first().copied(),
        Some(concat!("zaru ", env!("CARGO_PKG_VERSION"))),
        "the artefact's first line is its own version"
    );
    assert!(
        bare.stdout.contains("usage:"),
        "a bare `zaru` prints the help: {}",
        bare.stdout
    );
    assert!(
        !bare.stdout.contains("zaru-core"),
        "the composition list is `--version`'s now, not a bare `zaru`'s: {}",
        bare.stdout
    );

    let version = zaru(&home, &["--version"]);
    assert_eq!(version.code, 0);
    assert!(
        version.stdout.contains("zaru-core") && version.stdout.contains("zaru-aegis"),
        "`--version` prints what the binary is composed of: {}",
        version.stdout
    );
}

/// ADR-0014 D3's block, printed by the binary, over the layers it can read.
///
/// The effective row is read out of the **rendered text** rather than from the
/// explanation's own accessor, so the two sides of the assertion do not travel
/// through one code path.
#[test]
fn adr_0014_d3s_block_is_printed_and_a_flag_moves_the_effective_row() {
    let home = Home::new("explain");

    let built_in = zaru(&home, &["config", "explain", "runtime.tier"]);
    assert_eq!(built_in.code, 0);
    let effective: Vec<&str> = built_in
        .lines()
        .into_iter()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(
        effective.len(),
        1,
        "D3 marks exactly one row, and it is the highest layer that *set* the key: {:?}",
        built_in.lines()
    );
    assert!(
        effective[0].contains("built-in") && effective[0].contains("bare"),
        "with nothing configured the effective row is layer 1: {:?}",
        effective[0]
    );
    assert_eq!(
        built_in.lines().len(),
        6,
        "D3's block is the key's line and one row per layer, always five: {:?}",
        built_in.lines()
    );

    let flagged = zaru(
        &home,
        &[
            "--runtime",
            "contained",
            "config",
            "explain",
            "runtime.tier",
        ],
    );
    let effective: Vec<&str> = flagged
        .lines()
        .into_iter()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(effective.len(), 1);
    assert!(
        effective[0].contains("flag") && effective[0].contains("contained"),
        "a flag is layer 5 and wins, and the block says which layer supplied it: {:?}",
        effective[0]
    );
}

/// ADR-0012 D4's listing, printed by the binary, naming the supplying layer.
///
/// Both arms: an alias nothing set renders as D3's `(not set)`, and one a flag
/// set names the layer. The unresolved arm alone would pass against a listing
/// that never resolves anything.
#[test]
fn adr_0012_d4s_listing_names_what_each_alias_resolved_to_and_which_layer() {
    let home = Home::new("models");

    let nothing = zaru(&home, &["models"]);
    assert_eq!(nothing.code, 0);
    for line in nothing.lines() {
        assert!(
            line.contains("(not set)"),
            "with nothing configured every alias is unresolved, in D3's own spelling: {line:?}"
        );
    }

    let flagged = zaru(&home, &["--model", "a-model-identifier", "models"]);
    assert_eq!(flagged.code, 0);
    let default: Vec<&str> = flagged
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("  default"))
        .collect();
    assert_eq!(default.len(), 1, "one row per alias: {:?}", flagged.lines());
    assert!(
        default[0].contains("a-model-identifier") && default[0].contains("flag"),
        "D4 prints what the alias resolved to and which layer supplied it: {:?}",
        default[0]
    );
    assert_eq!(
        flagged
            .lines()
            .into_iter()
            .filter(|line| line.contains("(not set)"))
            .count(),
        4,
        "`--model` sets `model.default` and nothing else: {:?}",
        flagged.lines()
    );
}

/// ADR-0001 D2's datum, printed, with D1's cells and the diff to each tier.
#[test]
fn adr_0001_d2s_datum_is_printed_with_d1s_own_cells() {
    let home = Home::new("runtime");

    let ran = zaru(&home, &["--runtime", "contained", "runtime"]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.lines()[0].contains("runtime.tier = contained (from flag)"),
        "the first line is the tier and where it came from: {:?}",
        ran.lines()[0]
    );
    assert!(
        ran.stdout.contains("local containers"),
        "D1's Membrane cell for `contained`, in the record's own wording: {}",
        ran.stdout
    );

    let mut altered: Vec<&str> = ran
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("changing to "))
        .collect();
    altered.sort_unstable();
    assert_eq!(
        altered,
        vec![
            "changing to bare would alter",
            "changing to linked would alter"
        ],
        "D2's \"what changing it would alter\" is answered for every other tier, and only for the \
         others: {:?}",
        ran.lines()
    );
}

/// The binary creates nothing in `~/.zaru` merely by being asked a question.
///
/// ADR-0010's own inode consequence, and ADR-0014's "a loader that created a
/// directory in order to find nothing in it would be creating state to read
/// state". The control is that the scratch home exists throughout, so a check
/// that reported absence for everything would fail on it.
#[test]
fn asking_the_binary_a_question_writes_nothing_to_the_users_home() {
    let home = Home::new("no-writes");

    for arguments in [
        vec!["--help"],
        vec!["runtime"],
        vec!["models"],
        vec!["config", "explain", "runtime.tier"],
    ] {
        let ran = zaru(&home, &arguments);
        assert_eq!(ran.code, 0);
    }

    assert!(
        home.path().is_dir(),
        "the scratch home itself must survive, or this check reports absence for everything"
    );
    let zaru_home = home.path().join(".zaru");
    assert!(
        !zaru_home.exists(),
        "reading configuration created {}, which is creating state in order to read state",
        zaru_home.display()
    );
}
