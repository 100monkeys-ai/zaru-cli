// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Reads the binary's identity back out of the built artefact.
//!
//! The release checklist requires the version string be read out of the
//! shipped artefact rather than taken from the value the build was told to
//! set: elsewhere in this ecosystem a version property was set before an inner
//! build step, silently failed to reach the exported binary, and shipped wrong
//! for thirteen releases while every build reported success.
//!
//! So the two sides of this assertion are deliberately different readers. The
//! left-hand side is stdout from actually executing the binary Cargo built;
//! the right-hand side is this test crate's compile-time package metadata. The
//! mutant that makes them disagree is `main` printing anything other than the
//! version it was compiled with.

use std::process::Command;

/// A home this check owns, removed when it ends.
///
/// A bare `zaru` reads nothing from its home today, and that is exactly why
/// it is handed one: a child that inherits `HOME` passes by the accident of
/// what the binary happens not to read, and the first change that reads
/// configuration at start-up would make this check read the person's own
/// `~/.zaru`. `corpus_every_spawned_zaru_is_handed_a_home` holds every spawn
/// in this workspace to the same shape.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_built_binary_prints_the_version_it_was_compiled_with() {
    let home =
        Scratch(std::env::temp_dir().join(format!("zaru-version-home-{}", std::process::id())));
    std::fs::create_dir_all(&home.0).expect("a scratch home");
    let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .env_clear()
        .env("HOME", &home.0)
        .output()
        .expect("failed to execute the built zaru binary");

    assert!(
        output.status.success(),
        "zaru exited with {:?}; stderr was {:?}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8");
    let first_line = stdout.lines().next().unwrap_or("");

    assert_eq!(
        first_line,
        format!("zaru {}", env!("CARGO_PKG_VERSION")),
        "the built binary's first line of output is not its own version"
    );
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
