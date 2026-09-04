// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives [ADR-0016]'s taxonomy through its public
//! door, and reads the `zaru` binary's own exit code off the built artefact.
//!
//! # Two different kinds of evidence, and they must not be quoted for each
//! other
//!
//! The checks here that build a failure and read its class, its remedy and its
//! exit code are evidence **about the mechanism**: they use only what the
//! crate exports, from outside it, which is the reachability
//! [Verification lessons] §25 asks for and which the crate's own `mod tests`
//! cannot give. They say nothing whatever about the `zaru` binary.
//!
//! The last check is the other kind. It runs the artefact Cargo built and
//! reads its process status, which is the only place a D5 exit code is
//! observable at all. **Today it can observe exactly one of the six**: `zaru`
//! takes no arguments, so nothing a user can do makes it fail. Making a second
//! observable would need a flag or a `ZARU_*` variable, and both belong to
//! other records — [ADR-0015]'s namespaces and [ADR-0014]'s layer 5 — so
//! neither was added and the record says so rather than a probe being smuggled
//! in for the sake of a green.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::process::Command;
use std::sync::{Mutex, PoisonError};
use zaru_cli::failure::{
    Action, Class, Classified, Exit, Expected, Guarded, Partial, Presentation, Remedy,
    SessionEvidence, Statement, StepName, guard,
};
use zaru_cli::tools::Tier;

fn said(text: &str) -> Statement {
    Statement::new(text).expect("the fixtures here carry no control character")
}

/// Held for the whole body of every check in this file, and not merely around
/// the calls to [`guard`].
///
/// **Found by running rather than by reading.** The first version of this file
/// guarded nothing, and a mutation's red arrived with no failure message at
/// all: the checks run as threads in one process, `guard` replaces the
/// process-wide panic hook for its duration, and an assertion failing in a
/// neighbouring thread during that window has its message captured by the
/// guard's sink instead of printed. The verdict said FAILED and the reason was
/// gone — a red nobody can read, which is [Verification lessons] §3 arriving
/// from a direction nothing in this file was looking.
///
/// Serialising only the `guard` calls is not enough, because the panic that
/// gets swallowed is a *neighbour's*. The whole body is held.
///
/// The general rule this file now carries: **anything that calls
/// [`guard`] in a test binary serialises the whole check, not just the call.**
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
static ONE_CHECK_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone<T>(body: impl FnOnce() -> T) -> T {
    let _held = ONE_CHECK_AT_A_TIME
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    body()
}

/// One failure of each class, built and read from outside the crate.
///
/// The classes come from `Class::ALL` rather than a list retyped here, so a
/// sixth class arrives as a missing arm rather than as silent coverage.
#[test]
fn a_caller_outside_the_crate_builds_a_failure_of_every_class_and_reads_it() {
    alone(|| {
        println!("-- ADR-0016 D1, one failure of each class, from outside the crate --");

        let mut seen = Vec::new();
        for class in Class::ALL {
            let classified = match class {
                Class::Expected => {
                    Classified::Expected(Expected::new(said("iteration 3 of 5 did not pass")))
                }
                Class::UserCorrectable => Classified::UserCorrectable {
                    statement: said("no credential for alias `default`"),
                    remedy: Remedy::one(Action::described(said(
                        "add the token to the credential store under that alias",
                    ))),
                },
                Class::Environmental => Classified::Environmental {
                    statement: said("the provider is rate limiting"),
                    wait: zaru_cli::failure::Wait::NoWaitWillHelp(said(
                        "the limit resets on the account rather than on this run",
                    )),
                },
                Class::Capability => Classified::Capability {
                    statement: said("a membrane is not available at this tier"),
                    offered_by: Tier::Contained,
                },
                Class::Defect => match guard(
                    env!("CARGO_PKG_VERSION"),
                    "https://github.com/100monkeys-ai/zaru-cli",
                    SessionEvidence::NoSessionExists,
                    || panic!("a deliberate defect, raised from outside the crate"),
                ) {
                    Guarded::Defected(caught) => Classified::Defect(caught.report().clone()),
                    Guarded::Ran(()) => panic!("the boundary did not catch a panic under it"),
                },
            };

            assert_eq!(classified.class(), class);
            let shown = Presentation::of(&classified);
            println!(
                "[{}] exit {} | error register {} | {}",
                class,
                Exit::Failed(classified.clone()).code(),
                shown.is_the_error_register(),
                shown
            );
            seen.push((class, shown.class, Exit::Failed(classified).code()));
        }

        // The five codes D5 gives, as literals this check owns.
        assert_eq!(
            seen.iter().map(|(_, _, code)| *code).collect::<Vec<u8>>(),
            vec![1, 2, 3, 4, 70],
            "ADR-0016 D5's exit codes, read from outside the crate"
        );
        assert_eq!(
            Exit::Succeeded.code(),
            0,
            "and D5's table opens with 0 for a run that did what was asked"
        );
    });
}

/// ADR-0016 D3's boundary, driven from outside the crate.
#[test]
fn a_caller_outside_the_crate_drives_a_panic_to_a_defect_report() {
    alone(|| {
        let caught = match guard(
            env!("CARGO_PKG_VERSION"),
            "https://github.com/100monkeys-ai/zaru-cli",
            SessionEvidence::NoSessionExists,
            || panic!("a deliberate defect, raised from outside the crate"),
        ) {
            Guarded::Defected(caught) => caught,
            Guarded::Ran(()) => panic!("the boundary did not catch a panic under it"),
        };

        println!("-- ADR-0016 D3, a caught defect --");
        println!("{caught}");

        let rendered = caught.to_string();
        assert!(rendered.contains("not something you can configure"));
        assert!(rendered.contains("there is no session and no transcript was written"));
        assert!(
            !rendered.contains("a deliberate defect"),
            "the panic's own words must not be in the report: {rendered:?}"
        );
        assert_eq!(
            caught.own_words().as_str(),
            "a deliberate defect, raised from outside the crate",
            "and they must still be captured, for ADR-0010's transcript to be handed"
        );
    });
}

/// ADR-0016 D6, from outside the crate.
#[test]
fn a_caller_outside_the_crate_reports_three_of_five_steps() {
    alone(|| {
        let step = |name: &str| StepName::new(name).expect("a plain name is renderable");
        let partial = Partial::new(
            vec![step("fmt"), step("clippy"), step("build")],
            vec![step("test"), step("boundaries")],
        )
        .expect("three of five is a partial report");

        println!("-- ADR-0016 D6 --");
        println!("{partial}");

        assert_eq!(partial.of(), 5);
        assert!(partial.to_string().contains("3 of 5"));
    });
}

/// The one place a D5 exit code is observable: the artefact Cargo built.
///
/// **This is not evidence about the taxonomy**, it is evidence about the
/// binary. The two sides are deliberately different readers — the left is the
/// process status of an executed artefact, the right is D5's number written
/// out here.
#[test]
fn the_built_binary_exits_with_adr_0016_d5s_code_for_what_it_did() {
    alone(|| {
        let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
            .output()
            .expect("failed to execute the built zaru binary");

        let code = output
            .status
            .code()
            .expect("the binary was killed by a signal rather than exiting");
        println!("-- the built artefact --");
        println!("zaru exited {code}");

        assert_eq!(
            code,
            0,
            "ADR-0016 D5's `0   success`: the binary prints its composition and does nothing that \
             can fail. stderr was {:?}",
            String::from_utf8_lossy(&output.stderr)
        );

        // The arm that discriminates: 0 is the code for *success* rather than the
        // code the binary always returns, and the mapping it goes through says so.
        //
        // The widening is worth a line. `ExitStatus::code` is an `i32` because it
        // is the operating system's number; `Exit::code` is a `u8` because that is
        // what `std::process::ExitCode::from` takes and therefore all a process can
        // actually carry. The conversion is here, at the one place the two meet.
        assert_eq!(i32::from(Exit::Succeeded.code()), code);
        assert_ne!(
            i32::from(Class::Defect.exit_code()),
            code,
            "if these were equal the assertion above would hold for a binary that had crashed"
        );
    });
}
