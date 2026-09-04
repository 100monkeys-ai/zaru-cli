// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `zaru` binary.
//!
//! # Boundary
//!
//! This crate is the composition root. It depends on all five library crates
//! and nothing depends on it. ADR-0003 D7 names the installed binary `zaru`,
//! which is why the package is `zaru-cli` and the binary target is not.
//!
//! Three things live here rather than anywhere else, and each is here because
//! it is a property of the whole program rather than of any one part: the
//! configuration hierarchy of ADR-0014, the session lifecycle that resolves
//! the runtime tier once at session start, and the ADR-0016 error taxonomy
//! together with its mapping to exit codes. The library crates raise their own
//! errors; this crate classifies them.
//!
//! Nothing above is implemented yet. This binary prints what it is and exits.

/// The crates this binary is composed of, each reporting its own name and
/// version rather than being described by a list kept here.
///
/// A list retyped beside the binary is a list that drifts. Every entry is read
/// out of the crate itself, so an entry can only be wrong if that crate's own
/// package metadata is wrong.
fn composition() -> [(&'static str, &'static str); 5] {
    [
        (zaru_core::NAME, zaru_core::VERSION),
        (zaru_tui::NAME, zaru_tui::VERSION),
        (zaru_notes::NAME, zaru_notes::VERSION),
        (zaru_seal::NAME, zaru_seal::VERSION),
        (zaru_aegis::NAME, zaru_aegis::VERSION),
    ]
}

fn main() {
    println!("zaru {}", env!("CARGO_PKG_VERSION"));
    for (name, version) in composition() {
        println!("  {name} {version}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only -- see the note in `zaru-core`.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(env!("CARGO_PKG_NAME"), "zaru-cli");
        assert_eq!(
            env!("CARGO_PKG_VERSION").split('.').count(),
            3,
            "version is not three dot-separated components"
        );
    }

    // Asserts that all five sibling edges linked, by reading a value out of
    // each one. The mutant that makes this disagree is dropping a dependency
    // from `Cargo.toml`, which stops this crate compiling.
    #[test]
    fn every_library_crate_is_linked() {
        let names: Vec<&str> = composition().iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "zaru-core",
                "zaru-tui",
                "zaru-notes",
                "zaru-seal",
                "zaru-aegis"
            ]
        );
    }
}
