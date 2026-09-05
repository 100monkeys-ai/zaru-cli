// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `zaru` binary.
//!
//! # Boundary
//!
//! This crate is the composition root. It depends on all five library crates
//! and nothing depends on it. [ADR-0003] D7 names the installed binary `zaru`,
//! which is why the package is `zaru-cli` and the binary target is not.
//!
//! # What this binary does, which as of 2026-09-05 is something
//!
//! It reads its arguments, folds three of [ADR-0014] D1's five configuration
//! layers, and prints one of six data a landed module already produces. **This
//! is the first thing in the harness a person can run**, and it is the reason
//! several other records stopped being merely built: [ADR-0016] D5's exit codes
//! are observable on the real artefact, [ADR-0014] D3's explain block is
//! printed, [ADR-0012] D4's alias listing exists, [ADR-0001] D2's datum is
//! shown, [ADR-0010] D6's deletion is reachable, and one of [ADR-0007] D7's
//! five surfaces is built.
//!
//! What it still cannot do is **run a task**, because that needs a provider and
//! [ADR-0012] D3's trait has no implementation in any product tree. `zaru
//! --help` says so rather than leaving the user to find out.
//!
//! # This file is three things and no more
//!
//! Parse, execute, write. Everything a check could want to reach lives in
//! [`zaru_cli::cli`], because a binary target cannot be named from an
//! integration test — the same reason this crate grew a library target for the
//! credential store. What is left here is the boundary, the two writers, and
//! the exit code.
//!
//! # The session is still absent and the binary still says so
//!
//! [ADR-0010]'s lifecycle is built and this binary reads it, but it starts no
//! session: `zaru sessions list` lists what is there and `zaru --resume`
//! restores one, and neither creates `~/.zaru/sessions/<ulid>/` for a session
//! that never had a turn. So [`SessionEvidence::NoSessionExists`] is still what
//! [ADR-0016] D3's boundary is given, and it is still true. **The day that call
//! site changes is the day something reaches the loop.**
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::process::ExitCode;
use zaru_cli::cli::{Run, classify::Surface, parse_process};
use zaru_cli::failure::{Exit, Guarded, Presentation, SessionEvidence, guard};

/// Everything the binary does, inside the boundary.
///
/// Returns an [`Exit`] rather than `()` so that what the process exits with is
/// this function's answer rather than a decision `main` makes about it.
fn run() -> Exit {
    let version = env!("CARGO_PKG_VERSION");
    let report_at = env!("CARGO_PKG_REPOSITORY");

    let outcome = match parse_process() {
        Ok(line) => Run { version, report_at }.execute(&line),
        Err(refusal) => zaru_cli::cli::Outcome {
            lines: Vec::new(),
            exit: Exit::Failed(Surface::new(version, report_at).command(&refusal)),
        },
    };

    for line in &outcome.lines {
        println!("{line}");
    }

    // A failure goes to standard error, so that a shell reading `zaru models`
    // gets the listing on its pipe and the refusal on its terminal. ADR-0016
    // D5's whole argument is that this harness is wrapped by CI, and a wrapper
    // that has to parse a refusal out of the data stream is a wrapper that
    // will one day take the refusal for data.
    if let Exit::Failed(classified) = &outcome.exit {
        eprintln!("{}", Presentation::of(classified));
    }

    outcome.exit
}

fn main() -> ExitCode {
    // ADR-0016 D3's boundary, wrapping exactly one call. The version and the
    // report URL are read out of this package's own metadata rather than
    // retyped, for the same reason `zaru_cli::composition` reads the crate
    // names.
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
        let names: Vec<&str> = zaru_cli::composition()
            .iter()
            .map(|(name, _)| *name)
            .collect();
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
    }
}
