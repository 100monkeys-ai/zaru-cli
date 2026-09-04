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

#[test]
fn the_built_binary_prints_the_version_it_was_compiled_with() {
    let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
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
