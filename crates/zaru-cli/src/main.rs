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
//! # What this binary does, which is still almost nothing
//!
//! It prints what it is composed of and exits. What is new as of 2026-09-04 is
//! that it exits through [ADR-0016] D5's mapping rather than through `()`, and
//! that it runs inside D3's defect boundary — so a panic anywhere under `run`
//! is reported as a defect with the version and where to report it, and the
//! process exits 70 instead of Rust's 101 with a banner.
//!
//! **Nothing else is reachable from here.** The configuration hierarchy, the
//! credential store and the local tool surface are all built and none of them
//! is called: reaching them needs a command surface, which is ADR-0015's, and
//! a session, which is ADR-0010's.
//!
//! # The session is absent and the binary says so rather than pretending
//!
//! ADR-0010 is not started, so there is no `~/.zaru/sessions/<ulid>/` and no
//! transcript. This binary passes
//! [`SessionEvidence::NoSessionExists`],
//! which is the whole reason that variant exists: D3 says a defect's message
//! says the transcript is on disk, and claiming one that was never written
//! would be worse than admitting there is none. **This is the one call site
//! that changes the day ADR-0010 lands.**
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::process::ExitCode;
use zaru_cli::failure::{Exit, Guarded, SessionEvidence, guard};

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

/// Everything the binary does, inside the boundary.
///
/// Returns an [`Exit`] rather than `()` so that what the process exits with is
/// this function's answer rather than a decision `main` makes about it.
fn run() -> Exit {
    println!("zaru {}", env!("CARGO_PKG_VERSION"));
    for (name, version) in composition() {
        println!("  {name} {version}");
    }
    Exit::Succeeded
}

fn main() -> ExitCode {
    // ADR-0016 D3's boundary, wrapping exactly one call. The version and the
    // report URL are read out of this package's own metadata rather than
    // retyped, for the same reason `composition` reads the crate names.
    let guarded = guard(
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_REPOSITORY"),
        SessionEvidence::NoSessionExists,
        run,
    );

    // Two arms and no third: a caught defect hands back no value, so there is
    // nothing here that could carry on past a corrupted state.
    let exit = match guarded {
        Guarded::Ran(exit) => exit,
        Guarded::Defected(caught) => {
            eprintln!("{caught}");
            Exit::Failed(zaru_cli::failure::Classified::Defect(
                caught.report().clone(),
            ))
        }
    };

    ExitCode::from(exit.code())
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

    /// ADR-0016 D3 needs somewhere to send a bug report, and D5 needs the
    /// binary to have an exit code at all.
    ///
    /// Both come out of this package's own metadata, so this check is about
    /// the metadata being there to read rather than about a string somebody
    /// typed. The mutant: removing `repository` from the workspace's
    /// `[workspace.package]`, which stops this compiling.
    #[test]
    fn the_binary_knows_its_own_version_and_where_to_report_a_defect() {
        assert!(
            env!("CARGO_PKG_REPOSITORY").starts_with("https://"),
            "a defect report has to name somewhere a person can actually reach"
        );
        assert_eq!(
            run(),
            Exit::Succeeded,
            "printing the composition is not a failure, so the binary exits 0"
        );
        assert_eq!(run().code(), 0, "ADR-0016 D5: 0 is success");
    }
}
