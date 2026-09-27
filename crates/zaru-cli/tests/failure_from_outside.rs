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
        // A home this check owns: a bare `zaru` reads nothing from one today,
        // and a child that inherited `HOME` would pass by that accident rather
        // than by construction. See `corpus_every_spawned_zaru_is_handed_a_home`.
        let home = std::env::temp_dir().join(format!("zaru-failure-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).expect("a scratch home");
        let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
            .env_clear()
            .env("HOME", &home)
            .output()
            .expect("failed to execute the built zaru binary");
        let _ = std::fs::remove_dir_all(&home);

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

/// **One place in the terminal turns a classified failure into pane lines.**
///
/// [ADR-0016] D2 says a correctable failure carries what to do about it. Until
/// 2026-09-14 four of the five terminal sites that rendered a failure read
/// `Presentation::of(...).headline` and threw the projection's `lines` away,
/// so inside a session a refusal said what went wrong and never what to do.
/// Each site was fixed and each has a frame check; this walk is what stops a
/// *sixth* site from being written the same way, which no per-site check can
/// do because a check cannot fail for a site nobody has written yet.
///
/// # Why a walk and not a type
///
/// `Presentation`'s two fields are public data with three legitimate readers
/// that correctly take them apart: `main.rs` renders through `Display`,
/// `session::record::FailureLine::of` stores them separately by design, and
/// `compose`'s checks compare the headline alone against the transcript.
/// Making the fields unreachable would break three correct consumers to
/// constrain one. The walk constrains the surface that actually had the
/// defect, and leaves the three alone.
///
/// # What it exempts, and why `tests.rs` is not the product
///
/// `terminal/tests.rs` reads both needles, necessarily: the four frame checks
/// take their expectation from the refusal's own `Presentation`, so the file
/// that *asserts* the rule names what the rule forbids. What must have one
/// answer is what the binary does, which is the same line
/// `corpus_one_thing_decides_a_working_directory` draws and for the same
/// reason. `fixtures.rs` is **not** exempted -- it is test support the product
/// modules compile against, and nothing stops a rendering site being written
/// there.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn corpus_one_place_in_the_terminal_renders_a_classified_failure() {
    /// The one module that may project a classified failure into pane lines.
    const RENDERER: &str = "src/terminal/vocabulary.rs";
    /// Reading either of these is what turns a `Classified` into shown text.
    const NEEDLES: [&str; 2] = ["Presentation::of", ".headline"];

    let terminal = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/terminal");
    let mut scanned = 0usize;
    let mut lines = 0usize;
    let mut offences: Vec<String> = Vec::new();

    let mut stack = vec![terminal.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the terminal's source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable source file");
            scanned += 1;
            lines += text.lines().count();
            let relative = path
                .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            for (number, line) in text.lines().enumerate() {
                // A doc comment naming the call is prose about the rule, not an
                // instance of it -- and `vocabulary.rs`'s own comments say why
                // it is the one caller.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if NEEDLES.iter().any(|needle| line.contains(needle)) && relative != RENDERER {
                    offences.push(format!("{relative}:{}: {}", number + 1, line.trim()));
                }
            }
        }
    }

    // Liveness. A walk that read the wrong directory, or one whose exemption
    // list grew until it excused everything, passes vacuously and says nothing
    // -- the shape `Verification lessons` §8 names. The terminal is seven files
    // and some thousands of lines; a floor well under that still catches a scan
    // that found nothing.
    println!("scanned {scanned} terminal file(s), {lines} line(s)");
    assert!(
        scanned >= 5 && lines > 3_000,
        "this scan read {scanned} terminal file(s) and {lines} line(s), which is too few to have \
         asserted anything about where a classified failure is rendered",
    );

    // The accepting sibling, without which the walk is satisfied by a terminal
    // that renders no failure at all.
    let renderer =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(RENDERER))
            .expect("the one renderer is there");
    for needle in NEEDLES {
        assert!(
            renderer.contains(needle),
            "{RENDERER} is exempted as the one place that projects a classified failure, and it \
             does not read {needle}",
        );
    }

    assert!(
        offences.is_empty(),
        "a classified failure becomes pane lines in {RENDERER} and nowhere else, so that ADR-0016 \
         D2's remedy cannot be dropped by a site written later. {} other place(s) read it: \n  {}",
        offences.len(),
        offences.join("\n  "),
    );
}

/// **The binary has one ending, and it goes through the writing.**
///
/// [ADR-0016] D2's remedy and D5's exit code are built for every refusal this
/// harness raises, and until 2026-09-15 one path threw the first away:
/// `main.rs` answered the terminal branch with `Some(exit) => return exit`,
/// which left `run()` above the two writers below it. Measured from the
/// release binary at `6bdf080` over a pseudo-terminal, five refusal kinds
/// each exited with their code and wrote **zero bytes** — including a
/// checkpoint this harness did not write, which is D1's `Defect` at D5's `70`
/// and whose whole presentation is "this is a bug in Zaru" plus where to
/// report it.
///
/// # Why a walk, and why it is the only instrument that reaches this
///
/// `main.rs` is a binary target and cannot be named from an integration test,
/// which is the reason this crate has a library target at all. So the
/// property "every ending writes" is asserted twice, from the two sides a
/// check can stand on: `Outcome::written`'s own corpus asserts what the
/// writing *does*, over two `Vec<u8>`, for the class of every refusal; this
/// asserts that the binary has no ending that goes past it. Neither alone
/// would have caught the defect — the first because the writer was correct
/// all along, the second because a walk cannot say what bytes come out.
///
/// It is the same instrument, drawn for the same reason, as
/// `corpus_one_place_in_the_terminal_renders_a_classified_failure` above: a
/// rule that stops a *second* site being written the same way cannot be a
/// per-site check, because a check cannot fail for a site nobody has written
/// yet.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn corpus_the_binary_has_no_ending_that_goes_past_the_writing() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    let text = std::fs::read_to_string(&path).expect("the binary's own source");

    /// A line of prose about the rule is not an instance of it. Both the
    /// module documentation and the branch's own comment name the `return`
    /// this walk forbids, because they record why it is forbidden.
    fn is_prose(line: &str) -> bool {
        let trimmed = line.trim_start();
        trimmed.starts_with("//") || trimmed.starts_with('*')
    }

    let code: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_prose(line))
        .map(|(number, line)| (number + 1, line))
        .collect();

    // Liveness first. A walk over a file it failed to read, or over a file
    // that stopped being the composition root, passes vacuously and says
    // nothing -- `Verification lessons` §8. `main.rs` is some hundreds of
    // lines and a floor well under that still catches an empty read.
    println!("scanned src/main.rs: {} line(s) of code", code.len());
    assert!(
        code.len() > 40,
        "this walk read {} line(s) of code from src/main.rs, which is too few to have asserted \
         anything about how the binary ends",
        code.len()
    );

    // The rule. `run()` returns the value of one expression and the process
    // exits with it; an early `return` is an ending that skips the writing,
    // which is exactly the shape the terminal path had.
    let early: Vec<String> = code
        .iter()
        .filter(|(_, line)| line.contains("return ") || line.trim_end().ends_with("return;"))
        .map(|(number, line)| format!("src/main.rs:{number}: {}", line.trim()))
        .collect();
    assert!(
        early.is_empty(),
        "an ending that returns out of `run` goes past `Outcome::written`, and a refusal taking \
         it exits with its code and says nothing at all -- which is what ADR-0016 D2 calls an \
         error whose reader cannot act, with the grammar removed too:\n{}",
        early.join("\n")
    );

    // The other half of the same rule: the data writer moved with the refusal
    // writer, so a second `println!` here would be a second answer to where
    // standard output comes from. `eprintln!` stays for exactly one caller --
    // ADR-0016 D3's boundary, which reports a caught defect and is not an
    // `Outcome` at all.
    let printers: Vec<String> = code
        .iter()
        .filter(|(_, line)| line.contains("println!(") && !line.contains("eprintln!("))
        .map(|(number, line)| format!("src/main.rs:{number}: {}", line.trim()))
        .collect();
    assert!(
        printers.is_empty(),
        "standard output has one writer and it is `Outcome::written`:\n{}",
        printers.join("\n")
    );

    // The accepting sibling, without which every assertion above is satisfied
    // by a `main.rs` that writes nothing and ends nowhere.
    assert!(
        code.iter()
            .any(|(_, line)| line.contains("outcome.written(")),
        "the binary's one ending is `Outcome::written`, and this file does not call it -- so the \
         two assertions above hold vacuously"
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
