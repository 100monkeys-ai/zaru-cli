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
//! # What this binary does, which as of 2026-09-05 includes running a task
//!
//! It reads its arguments, folds all five of [ADR-0014] D1's configuration
//! layers, prints what a landed module already produces — and **runs one
//! turn**. `zaru <task>` starts a session, asks a model, executes the tool
//! calls it asks for under [ADR-0011]'s permission model, writes every event
//! to [ADR-0010] D2's transcript as it happens, and exits through
//! [ADR-0016] D5.
//!
//! What it cannot do is run a task against four of [ADR-0012] D3's five
//! provider kinds, which have no client, or run [ADR-0008] D1's **inner** loop
//! over a project that declares validators, which has no `Generator` and no
//! `Executor`. Both are refused naming what is missing rather than left for a
//! user to find out.
//!
//! # This file is three things and one branch
//!
//! Parse, execute, write — and, since 2026-09-05, one branch before the
//! execute: `--resume` and `--continue` open [ADR-0005]'s terminal when
//! standard output is a terminal, and fall through to the print when it is
//! not. That branch is [`zaru_cli::terminal::take_over`], which carries the
//! decision and its reason; what is here is the call and nothing else.
//!
//! Everything a check could want to reach lives in
//! [`zaru_cli::cli`], because a binary target cannot be named from an
//! integration test — the same reason this crate grew a library target for the
//! credential store. What is left here is the boundary, the two handles the
//! writing is done on, and the exit code.
//!
//! **The writing itself left on 2026-09-15, and the sentence above said "the
//! two writers" until then.** It was two macros here, and a `return` in the
//! branch below went past both of them: a session that would not open at a
//! terminal exited with its code and printed nothing at all. The rule is
//! [`zaru_cli::cli::Outcome::written`] now — one call, on writers this file
//! passes — because a rule that lives in a binary target is a rule no check
//! can read the bytes back from, which is what this paragraph already says
//! about everything else in this file.
//!
//! # The binary starts a session, and this is the day that changed
//!
//! [ADR-0010]'s Status tracking has carried one sentence since 2026-09-04:
//! "**The day that call site changes is the day something reaches the loop.**"
//! It changed on 2026-09-05. `zaru <task>` starts a session, writes D1's three
//! files including the first `meta.toml` any product path has ever written,
//! runs one turn of [ADR-0008] D1's outer loop, and exits through
//! [ADR-0016] D5.
//!
//! So [`SessionEvidence::NoSessionExists`] is **still what this boundary is
//! given, and it is still true of this call**: `guard` wraps the whole
//! process, including the parse, which happens before any session could exist.
//! What changed is that the paths *inside* it that have a session now hand
//! their own evidence to their own refusals — see [`zaru_cli::cli::run`]. A
//! defect caught out here is a defect in the harness before it had a session
//! to name, which is what this arm says.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::process::ExitCode;
use zaru_cli::cli::{Run, classify::Surface, parse_process};
use zaru_cli::config::{Home, Variables};
use zaru_cli::failure::{Exit, Guarded, SessionEvidence, guard};

/// Everything the binary does, inside the boundary.
///
/// Returns an [`Exit`] rather than `()` so that what the process exits with is
/// this function's answer rather than a decision `main` makes about it.
fn run() -> Exit {
    let version = env!("CARGO_PKG_VERSION");
    let report_at = env!("CARGO_PKG_REPOSITORY");
    // `~/.zaru`, asked of the environment **here and nowhere else**, and
    // handed to everything that reads it. See `zaru_cli::config::Home` for
    // the seventeen places that each asked for themselves until 2026-09-27,
    // and why a check could not point them anywhere.
    let home = Home::of_this_user();
    // The environment, read **here and nowhere else**, for the same reason
    // and by the same shape. See `zaru_cli::config::Variables` for the four
    // readers that each asked the process for themselves until 2026-09-27.
    let variables = Variables::of_this_process();

    let outcome = match parse_process() {
        // ADR-0010 D4's two readings, one per reader. A person at a terminal
        // is left inside the session; a pipe is handed the transcript and
        // nothing else. See `zaru_cli::terminal::open`, which carries the
        // decision and the reason. This is the only branch in this file that
        // is not parse, execute, write.
        Ok(line) => {
            match zaru_cli::terminal::take_over(&line, &home, &variables, version, report_at) {
                // **The terminal path's ending is an `Outcome` like every other,
                // and until 2026-09-15 it was a `return` that went past the
                // writing below.** A person whose session would not open -- no
                // session to continue, a session that does not exist, a
                // `zaru.toml` refused by name, a runtime tier that names none, a
                // checkpoint this harness did not write -- got the exit code and
                // not one byte of the sentence, which is ADR-0016 D2's "an error
                // message whose reader cannot act" with the grammar removed too.
                //
                // There are no lines, because what the terminal path had to show
                // it painted itself. `terminal::open` gives the terminal back
                // before it hands this up -- `guard.restore_now()` on both of its
                // exits -- so the refusal is written to the screen the person is
                // looking at rather than into an alternate screen that is about
                // to be discarded with it.
                Some(exit) => zaru_cli::cli::Outcome {
                    lines: Vec::new(),
                    exit,
                },
                None => Run {
                    version,
                    report_at,
                    home: &home,
                    variables: &variables,
                }
                .execute(&line),
            }
        }
        Err(refusal) => zaru_cli::cli::Outcome {
            lines: Vec::new(),
            exit: Exit::Failed(Surface::new(version, report_at).command(&refusal)),
        },
    };

    // A failure goes to standard error, so that a shell reading `zaru models`
    // gets the listing on its pipe and the refusal on its terminal. ADR-0016
    // D5's whole argument is that this harness is wrapped by CI, and a wrapper
    // that has to parse a refusal out of the data stream is a wrapper that
    // will one day take the refusal for data.
    //
    // **The rule is `Outcome::written` and not two macros here**, because a
    // rule written in a binary target is a rule no check can name: this file
    // cannot be reached from an integration test, which is the same reason
    // this crate has a library target at all. What it asserts -- data to one
    // writer, the refusal to the other, for every class -- is asserted over
    // two `Vec<u8>` there.
    outcome.written(&mut std::io::stdout(), &mut std::io::stderr())
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
