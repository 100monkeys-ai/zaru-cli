// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the one place this workspace starts a child process.
//!
//! **Every child here is real.** `printf`, `false`, `true`, `env`, `pwd`,
//! `sleep`, `touch` and this crate's own test binary, resolved through `PATH`
//! exactly as [ADR-0011] D1 says `cmd.run` resolves a program. Nothing is
//! staged, because a staged process cannot tell a ceiling that kills from one
//! that does not.
//!
//! A program this machine does not have produces
//! [`SpawnFailure::CouldNotStart`], which names the program — a loud refusal
//! rather than a silent pass, which is [Verification lessons] §4's rule about
//! staging.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::credentials::fixtures::{ascii_core, nonce as awkward_nonce};
use crate::process::ceiling::{CeilingIsZero, ProcessCeiling};
use crate::process::environment::{Environment, HARNESS_PREFIX, MINIMUM, NotForAChild};
use crate::process::line::{CommandLine, NotACommandLine, REFUSED_CONSTRUCTS};
use crate::process::spawn::{Ended, SIGNALLED_EXIT_BASE, Spawn, SpawnFailure};
use crate::tools::fixtures::ScratchTree;
use crate::tools::port::Subprocess as _;
use crate::tools::tree::WorkingDirectory;
use core::time::Duration;

/// A ceiling generous enough that reaching it means something is wrong.
///
/// Used by every check whose subject is not the ceiling, so that a hang is a
/// bounded red rather than a suite that never finishes.
fn generous() -> ProcessCeiling {
    ProcessCeiling::new(Duration::from_secs(30)).expect("thirty seconds is not zero")
}

// ---------------------------------------------------------------- the line

/// Every construct the ruling names is refused, and each one has an accepting
/// sibling that is the same text quoted.
///
/// The set is walked from [`REFUSED_CONSTRUCTS`] rather than retyped here, so
/// an entry removed from the product is a check that stops testing it rather
/// than a check that keeps passing — [Verification lessons] §17.
///
/// **The accepting arm is what stops this being satisfied by a splitter that
/// refuses everything.** Quoting the very same characters must produce one
/// ordinary argument carrying them.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_refused_construct_is_refused_and_the_same_text_quoted_is_not() {
    let mut accepted = Vec::new();
    let mut misnamed = Vec::new();
    let mut quoted_rejected = Vec::new();

    for construct in REFUSED_CONSTRUCTS {
        let offered = format!("printf hello {construct} goodbye");
        match CommandLine::split(&offered) {
            Err(NotACommandLine::ShellConstruct {
                construct: named, ..
            }) => {
                if named != construct {
                    misnamed.push((construct, named));
                }
            }
            Err(other) => misnamed.push((construct, refusal_name(&other))),
            Ok(line) => accepted.push((construct, line.render())),
        }

        // The accepting sibling: the same characters, quoted, are an argument.
        let quoted = format!("printf hello '{construct}' goodbye");
        match CommandLine::split(&quoted) {
            Ok(line) => {
                if line.arguments() != ["hello", construct, "goodbye"] {
                    quoted_rejected.push((construct, format!("{:?}", line.arguments())));
                }
            }
            Err(refused) => quoted_rejected.push((construct, refused.to_string())),
        }
    }

    assert!(
        accepted.is_empty(),
        "{} of {} shell constructs were accepted as ordinary arguments — (construct, what it \
         became): {:?}. A construct passed through as a literal runs a different command from \
         the one that was written and nothing says so",
        accepted.len(),
        REFUSED_CONSTRUCTS.len(),
        accepted,
    );
    assert!(
        misnamed.is_empty(),
        "{} refusals named the wrong construct — (offered, named): {:?}. A refusal a reader \
         cannot act on is a refusal that has told them there is a problem and not what it is",
        misnamed.len(),
        misnamed,
    );
    assert!(
        quoted_rejected.is_empty(),
        "{} of {} constructs were still refused when quoted, or did not survive as one argument \
         — (construct, what happened): {:?}. A splitter that refuses a quoted operator refuses a \
         legitimate argument, and a table with no accepting arm is satisfied by refusing \
         everything",
        quoted_rejected.len(),
        REFUSED_CONSTRUCTS.len(),
        quoted_rejected,
    );
}

/// How a refusal is named in a failure message.
fn refusal_name(refused: &NotACommandLine) -> &'static str {
    match refused {
        NotACommandLine::Empty => "Empty",
        NotACommandLine::ShellConstruct { .. } => "ShellConstruct",
        NotACommandLine::UnterminatedQuote { .. } => "UnterminatedQuote",
        NotACommandLine::TrailingEscape { .. } => "TrailingEscape",
    }
}

/// Quoting is what separates a splitter from `split_whitespace`.
///
/// The mutant this exists for is the zero-vocabulary fallback: splitting on
/// whitespace alone, which the coordinator's ruling of 2026-09-05 considered
/// and did not take. Under it every one of these arguments comes apart.
#[test]
fn quotes_and_escapes_make_one_argument_out_of_several_words() {
    let line = CommandLine::split(r#"printf 'a b'  "c d"  e\ f  "g\"h"  'i"j'  k"#)
        .expect("a quoted command line splits");

    assert_eq!(line.program(), "printf", "the program is the first word");
    assert_eq!(
        line.arguments(),
        ["a b", "c d", "e f", "g\"h", "i\"j", "k"],
        "quoting did not survive the split; `split_whitespace` would produce eleven arguments \
         here rather than six"
    );
}

/// A rendered command line splits back into the same command line.
///
/// This is the property that makes ADR-0010's sentence — "a rendered
/// `cmd.run` line **is** a command line" — true rather than approximately
/// true, and it is what lets a transcript entry be read back by a person or
/// by this splitter without a second grammar.
///
/// The fixtures are awkward on the axis the mutant moves: a word carrying a
/// space, a word carrying each quote, a word carrying a backslash, a word
/// carrying a refused construct, and an empty word.
#[test]
fn a_rendered_command_line_splits_back_to_itself() {
    let awkward = CommandLine::of(
        "printf",
        [
            String::from("a b"),
            String::from("has'single"),
            String::from("has\"double"),
            String::from("has\\backslash"),
            String::from("pipe|inside"),
            String::new(),
            String::from("plain"),
        ],
    )
    .expect("a program and seven arguments");

    let rendered = awkward.render();
    let round_tripped = CommandLine::split(&rendered).unwrap_or_else(|refused| {
        panic!("a rendering this module produced was refused by its own splitter: {refused}")
    });

    assert_eq!(
        round_tripped, awkward,
        "the rendering {rendered:?} did not split back into what produced it, so a transcript \
         entry and the command it names are two different things"
    );
}

/// An empty command, an unterminated quote and a trailing escape are refused.
///
/// Three shapes with nothing in common except that none of them names a
/// program and a set of arguments.
#[test]
fn a_command_line_that_cannot_be_read_is_refused_by_name() {
    let cases: [(&str, &str); 6] = [
        ("", "Empty"),
        ("   \t \n ", "Empty"),
        ("printf 'unterminated", "UnterminatedQuote"),
        ("printf \"unterminated", "UnterminatedQuote"),
        ("printf trailing\\", "TrailingEscape"),
        ("printf \"trailing\\", "UnterminatedQuote"),
    ];

    let mut wrong = Vec::new();
    for (offered, expected) in cases {
        match CommandLine::split(offered) {
            Err(refused) => {
                let named = refusal_name(&refused);
                if named != expected {
                    wrong.push((offered, expected, named.to_owned()));
                }
            }
            Ok(line) => wrong.push((offered, expected, format!("accepted as {}", line.render()))),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} unreadable commands were not refused for the reason that matters — (offered, \
         expected, got): {:?}",
        wrong.len(),
        cases.len(),
        wrong,
    );
}

/// A refusal quotes the offered command back and names the construct.
///
/// [Verification lessons] §36: a message written for the reader who meets it
/// with none of the author's context. The mutant is a refusal that says only
/// that something was wrong.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_shell_construct_refusal_names_the_construct_and_quotes_the_command() {
    let refused = CommandLine::split("cargo test | tee log").expect_err("a pipe is refused");
    let said = refused.to_string();

    assert!(
        said.contains("\"|\""),
        "the refusal does not name the construct it found: {said:?}"
    );
    assert!(
        said.contains("cargo test"),
        "the refusal does not quote the command back, so a reader cannot tell which declaration \
         was rejected: {said:?}"
    );
    assert!(
        said.contains("no shell"),
        "the refusal does not say why the construct cannot simply be passed through: {said:?}"
    );
}

// --------------------------------------------------------- the environment

/// [ADR-0014] D1's layer 4 cannot be put into a child's environment at all.
///
/// The refusal is on the constructor, so this is absence rather than a filter
/// somebody remembered to apply: there is no `Environment` value carrying a
/// `ZARU_` name for a `Spawn` to pass on.
///
/// The accepting sibling is the same call with a name that is not the
/// harness's, without which a constructor that refused everything would pass.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn the_harnesss_own_configuration_cannot_be_put_in_a_childs_environment() {
    let value = awkward_nonce("layer-four");

    let refused = Environment::empty()
        .carrying(format!("{HARNESS_PREFIX}PROVIDER_ANTHROPIC_KEY"), &value)
        .expect_err("a ZARU_-prefixed name is refused");
    assert!(
        matches!(refused, NotForAChild::HarnessOwned { .. }),
        "a layer-4 name was refused for the wrong reason: {refused}"
    );
    let said = refused.to_string();
    assert!(
        !said.contains(&value) && !said.contains(ascii_core(&value)),
        "the refusal published the value it was refusing to pass on: {said:?}"
    );

    let accepted = Environment::empty()
        .carrying("PATH", "/usr/bin")
        .expect("an ordinary name is carried");
    assert_eq!(
        accepted.pairs().collect::<Vec<_>>(),
        vec![("PATH", "/usr/bin")],
        "an ordinary name was not carried, so the refusal above proves nothing"
    );
}

/// A name or a value an environment cannot express is refused.
#[test]
fn an_unnameable_name_or_value_is_refused() {
    assert!(matches!(
        Environment::empty()
            .carrying("", "x")
            .expect_err("an empty name"),
        NotForAChild::EmptyName
    ));
    assert!(matches!(
        Environment::empty()
            .carrying("A=B", "x")
            .expect_err("a name carrying an assignment"),
        NotForAChild::UnnameableName { .. }
    ));
    assert!(matches!(
        Environment::empty()
            .carrying("A\0B", "x")
            .expect_err("a name carrying a NUL"),
        NotForAChild::UnnameableName { .. }
    ));
    assert!(matches!(
        Environment::empty()
            .carrying("A", "x\0y")
            .expect_err("a value carrying a NUL"),
        NotForAChild::UnnameableValue { .. }
    ));
}

/// The inherited minimum is exactly the named five, and no more.
///
/// Asserted against [`MINIMUM`] rather than against a list retyped here, and
/// against the harness's own process for which of them exist — so the check
/// measures the rule rather than this machine.
#[test]
fn the_inherited_minimum_is_the_named_five_and_nothing_else() {
    let environment = Environment::inherited_minimum().expect("the harness's own values pass on");

    // The five are written here as literals rather than read back out of
    // `MINIMUM`. [Verification lessons] §11: at least one arm of a comparison
    // must not travel through the thing being checked, and a sixth name added
    // to the product's own list would otherwise appear on both sides at once
    // and agree with itself. That mutation survived until this was written
    // this way.
    let five = ["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR"];
    assert_eq!(
        MINIMUM.to_vec(),
        five.to_vec(),
        "ADR-0011 D2's Update names these five and no others"
    );

    let carried: Vec<&str> = environment.pairs().map(|(name, _)| name).collect();
    let outside: Vec<&&str> = carried
        .iter()
        .filter(|name| !five.contains(*name))
        .collect();
    assert!(
        outside.is_empty(),
        "the inherited minimum carried {outside:?}, which ADR-0011 D2's five do not name; the \
         five are {five:?}"
    );

    // Compared as sets: an `Environment` is ordered by name and the five are
    // in the ruling's own order, so comparing the two sequences would assert
    // the sort rather than the membership.
    let mut expected: Vec<&str> = five
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some())
        .collect();
    expected.sort_unstable();
    let mut carried_sorted = carried.clone();
    carried_sorted.sort_unstable();
    assert_eq!(
        carried_sorted, expected,
        "the inherited minimum is not the named five this process actually has"
    );
    // The staging: without a name to carry, every assertion above is vacuous.
    assert!(
        carried.contains(&"PATH"),
        "this process has no PATH, so nothing above was measured. ADR-0011 D1's `cmd.run` \
         resolves a program through PATH and a check with none cannot exercise it"
    );
}

// ------------------------------------------------------------- the ceiling

/// A ceiling of zero is refused, and any other is taken.
#[test]
fn a_ceiling_of_zero_is_refused_and_carries_its_reason() {
    let refused = ProcessCeiling::new(Duration::ZERO).expect_err("zero is refused");
    assert_eq!(refused, CeilingIsZero);
    assert!(
        refused.to_string().contains("killed before it can report"),
        "the refusal does not say what a zero ceiling costs: {refused}"
    );
    assert_eq!(
        ProcessCeiling::new(Duration::from_millis(1))
            .expect("one millisecond is not zero")
            .get(),
        Duration::from_millis(1),
        "a ceiling did not carry the duration it was given"
    );
}

// --------------------------------------------------------------- the child

/// A child starts in ADR-0011 D4's boundary root and nowhere else.
///
/// Asserted with `pwd`, which reports the directory the kernel gave the
/// process rather than anything the harness told it — one arm of the
/// comparison does not travel through the code under test
/// ([Verification lessons] §11). The environment is cleared, so there is no
/// `PWD` for `pwd` to report instead of the real one.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn a_child_starts_at_the_boundarys_root() {
    let tree = ScratchTree::new();
    // Deliberately the symlinked spelling: a root that was not canonicalised
    // at construction would put the child somewhere else with the same name.
    let working = WorkingDirectory::at(tree.project_by_link()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let outcome = spawn
        .execute(&CommandLine::split("pwd").expect("`pwd` is a command line"))
        .await
        .unwrap_or_else(|failure| panic!("`pwd` did not run: {failure}"));

    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "`pwd` did not exit cleanly; stderr was {:?}",
        outcome.stderr
    );
    assert_eq!(
        outcome.stdout.trim_end(),
        working.root().display().to_string(),
        "the child started somewhere other than the working directory ADR-0011 D4 measures \
         against"
    );
}

/// The work's exit code is the work's, and it is an `i32`.
#[tokio::test]
async fn a_childs_exit_code_is_the_works_own() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let succeeded = spawn
        .execute(&CommandLine::split("true").expect("a command line"))
        .await
        .expect("`true` ran");
    let failed = spawn
        .execute(&CommandLine::split("false").expect("a command line"))
        .await
        .expect("`false` ran");

    assert_eq!(succeeded.ended.exit_code(), 0, "`true` did not report zero");
    assert_eq!(failed.ended.exit_code(), 1, "`false` did not report one");
    assert!(
        !succeeded.ended.was_killed_at_the_ceiling() && !failed.ended.was_killed_at_the_ceiling(),
        "a child that exited on its own was reported as killed at the ceiling"
    );
}

/// Both streams are captured, separately, and neither is merged into the
/// other.
#[tokio::test]
async fn stdout_and_stderr_are_captured_separately() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    // `printf` with a bad format writes to stderr and still writes what it
    // can to stdout, so one command exercises both streams.
    let outcome = spawn
        .execute(&CommandLine::split("printf 'out-marker'").expect("a command line"))
        .await
        .expect("`printf` ran");
    assert_eq!(
        outcome.stdout, "out-marker",
        "standard output was not captured"
    );
    assert!(
        outcome.stderr.is_empty(),
        "standard error carried standard output's bytes: {:?}",
        outcome.stderr
    );

    let failing = spawn
        .execute(&CommandLine::split("cat /nonexistent-for-this-check").expect("a command line"))
        .await
        .expect("`cat` ran");
    assert!(
        failing.stdout.is_empty(),
        "standard output carried standard error's bytes: {:?}",
        failing.stdout
    );
    assert!(
        !failing.stderr.is_empty(),
        "standard error was not captured at all"
    );
}

/// A capture larger than a pipe buffer completes rather than deadlocking.
///
/// **This is the check the concurrent readers exist for.** A `Spawn` that
/// waited for the child without draining would block it in `write` at the
/// first full pipe — 64 KiB on Linux — and neither would ever move, so the
/// ceiling would kill a child that had done nothing wrong. One megabyte is
/// sixteen buffers. It was two reader threads until 2026-09-05 and is two
/// futures in a `select!` now; the property is the same and so is the mutant.
#[tokio::test]
async fn a_capture_larger_than_a_pipe_buffer_completes() {
    const BYTES: usize = 1_000_000;
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let outcome = spawn
        .execute(&CommandLine::split(&format!("printf %0{BYTES}d 0")).expect("a command line"))
        .await
        .expect("`printf` ran");

    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "a large capture did not finish on its own — it was {}",
        outcome.ended
    );
    assert_eq!(
        outcome.stdout.len(),
        BYTES,
        "the capture is not the size the child wrote, so something between them dropped bytes"
    );
}

/// The ceiling kills a child that would otherwise outlast it.
///
/// **Two mutants, both bounded and both deterministic** ([Verification
/// lessons] §57). Reporting the kill as an ordinary exit reddens on
/// `was_killed_at_the_ceiling`. Removing the kill itself leaves the wait
/// running until the child ends on its own, which reddens on the elapsed
/// assertion — which is why the child sleeps for a fixed span it can be
/// measured against rather than forever.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn the_ceiling_kills_a_child_that_outlasts_it() {
    const CHILD_SLEEPS: Duration = Duration::from_secs(20);
    let ceiling = ProcessCeiling::new(Duration::from_millis(200)).expect("not zero");
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        ceiling,
    );

    let outcome = spawn
        .execute(&CommandLine::split("sleep 20").expect("a command line"))
        .await
        .expect("`sleep` ran");

    assert!(
        outcome.ended.was_killed_at_the_ceiling(),
        "a child that outlasted its ceiling was reported as {}, so a caller cannot tell the \
         harness's ceiling from the work's own failure",
        outcome.ended
    );
    assert_eq!(
        outcome.ended.exit_code(),
        SIGNALLED_EXIT_BASE + 9,
        "a ceiling kill did not report the shell convention's code for SIGKILL"
    );
    assert!(
        outcome.took < CHILD_SLEEPS / 4,
        "the call took {:?}, and the child asked to run for {CHILD_SLEEPS:?} — so nothing \
         stopped it and the ceiling only described what happened afterwards",
        outcome.took,
    );
}

/// A program that does not exist is user-correctable and names the command.
#[tokio::test]
async fn a_program_that_will_not_start_names_itself() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );
    let missing = awkward_nonce("no-such-program");

    let failure = spawn
        .execute(&CommandLine::of(&missing, []).expect("a program with no arguments"))
        .await
        .expect_err("a program that does not exist cannot start");

    assert!(
        matches!(failure, SpawnFailure::CouldNotStart { .. }),
        "a program that does not exist was not reported as one that could not start: {failure}"
    );
    assert!(
        failure.to_string().contains(ascii_core(&missing)),
        "the failure does not name the program, so a reader cannot tell which command to fix: {failure}"
    );
}

/// A missing `cmd.run` program is a failed tool result, not a failed turn.
///
/// The model can read the command and PATH remedy from stderr and try a
/// different command; carrying it as a tools-port failure would instead make
/// the outer classifier call it a harness defect and stop the turn.
#[tokio::test]
async fn a_missing_cmd_run_program_stays_in_the_tool_result_channel() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );
    let missing = awkward_nonce("no-such-command");

    let captured = spawn
        .run(&CommandLine::of(&missing, []).expect("a command line"))
        .await
        .expect("a missing program is reported to the tool caller");

    assert_eq!(
        captured.exit_code, 127,
        "a missing program did not carry the shell command-not-found status"
    );
    assert!(
        captured.stdout.is_empty(),
        "a program that never started wrote stdout"
    );
    assert!(
        captured.stderr.contains(ascii_core(&missing)) && captured.stderr.contains("PATH"),
        "the tool result did not name the missing command and its remedy: {:?}",
        captured.stderr
    );
}

/// Nothing contains a child at `bare`, and this check documents that rather
/// than claiming otherwise.
///
/// ADR-0011 D2: "the harness is not a sandbox and says so." A `cmd.run` that
/// was permitted can write anywhere the user can, and the working directory
/// is where it *starts*, not a wall. Asserting the file **exists** is the
/// honest form: a check asserting it does not would be asserting containment
/// this tier has never had, and would redden the day somebody read D2
/// correctly.
///
/// At `contained` the membrane is ADR-0004's, and it is the answer that does
/// not depend on a process being well behaved.
#[tokio::test]
async fn nothing_contains_a_child_at_bare_and_the_check_says_so() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let outside = tree
        .base()
        .join(format!("{}.escaped", awkward_nonce("child")));
    assert!(
        !outside.exists(),
        "the staging is wrong: the file the child is about to create already exists"
    );
    assert!(
        !outside.starts_with(working.root()),
        "the staging is wrong: the target is inside the working directory, so this check would \
         say nothing about leaving it"
    );

    let outcome = spawn
        .execute(
            &CommandLine::of("touch", [outside.display().to_string()])
                .expect("a program and one argument"),
        )
        .await
        .unwrap_or_else(|failure| panic!("`touch` did not run: {failure}"));

    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "`touch` did not succeed, so nothing was learned: stderr was {:?}",
        outcome.stderr
    );
    assert!(
        outside.exists(),
        "a child wrote nothing outside the working directory. That would be a *stronger* \
         property than ADR-0011 D2 claims at `bare`, and if it is now true the record and this \
         check both want rewriting rather than this assertion relaxing"
    );
}

// ------------------------------------- the harness's own environment, planted

/// The environment variable the parent plants in the child test binary.
const PLANTED_NAME: &str = "ZARU_PROVIDER_ANTHROPIC_KEY";

/// Where the parent tells the re-invoked child to root its working directory.
const CHILD_ROOT: &str = "PR_CHILD_ROOT";

/// How the re-invoked child marks a line of its grandchild's environment.
const ENV_MARKER: &str = "PR-ENV ";

/// A `ZARU_*` variable of the harness's own process never reaches a child.
///
/// # Why this needs a second process
///
/// The rule is about a variable the **harness itself** holds, and planting one
/// means changing this process's environment — which is `unsafe` in edition
/// 2024 and is racy against every other check in this binary. So the check
/// re-invokes this crate's own test binary with the variable set on *that*
/// command, which is safe and touches nothing shared, and the child runs `env`
/// through a real [`Spawn`] and reports what its grandchild saw.
///
/// It is the same shape `session::tests`'s kill check already uses, and the
/// parent prints a newline before spawning so that its own progress framing
/// cannot be glued to the child's first line ([Verification lessons] §60).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_harnesss_own_configuration_never_reaches_a_child() {
    let tree = ScratchTree::new();
    let planted = awkward_nonce("layer-four-secret");

    println!();
    let output = std::process::Command::new(
        std::env::current_exe().expect("the test binary knows where it is"),
    )
    .args([
        "--exact",
        "process::tests::the_environment_checks_child_reports_what_its_grandchild_saw",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(PLANTED_NAME, &planted)
    .env(CHILD_ROOT, tree.project())
    .output()
    .expect("could not spawn this crate's own test binary");

    let said = String::from_utf8_lossy(&output.stdout).into_owned();
    let seen: Vec<&str> = said
        .lines()
        .filter_map(|line| line.strip_prefix(ENV_MARKER))
        .collect();

    // The staging, before any absence is believed: the child ran and reported.
    assert!(
        !seen.is_empty(),
        "the re-invoked child reported no environment at all, so nothing below was measured. It \
         printed: {said:?} / {:?}",
        String::from_utf8_lossy(&output.stderr),
    );

    let names: Vec<&str> = seen
        .iter()
        .map(|line| line.split_once('=').map_or(*line, |(name, _)| name))
        .collect();

    // The accepting arm: an environment that reached the grandchild at all.
    assert!(
        names.contains(&"PATH"),
        "the grandchild saw no PATH, so an implementation passing nothing would pass this check. \
         It saw: {names:?}"
    );
    // The rule.
    assert!(
        !names.iter().any(|name| name.starts_with(HARNESS_PREFIX)),
        "a {HARNESS_PREFIX}-prefixed variable of the harness's own process reached a child: \
         {names:?}"
    );
    let core = ascii_core(&planted);
    assert!(
        !seen
            .iter()
            .any(|line| line.contains(&planted) || line.contains(core)),
        "the planted value reached the grandchild's environment. Asserted on the raw value and \
         on its ASCII core, because an escaping renderer can publish every byte of a value in a \
         form the raw comparison does not recognise"
    );
    // And the environment is an allowlist rather than an inheritance: every
    // name the grandchild saw is one of the five.
    let outside: Vec<&&str> = names.iter().filter(|n| !MINIMUM.contains(*n)).collect();
    assert!(
        outside.is_empty(),
        "the grandchild saw {outside:?}, which ADR-0011 D2's five do not name — so the \
         environment was inherited rather than cleared and set"
    );
}

/// The child half of the check above. Never run on its own.
#[tokio::test]
#[ignore = "re-invoked by `the_harnesss_own_configuration_never_reaches_a_child`"]
async fn the_environment_checks_child_reports_what_its_grandchild_saw() {
    let root = std::env::var(CHILD_ROOT)
        .unwrap_or_else(|_| panic!("{CHILD_ROOT} names the working directory this child uses"));
    assert!(
        std::env::var(PLANTED_NAME).is_ok(),
        "the parent did not plant {PLANTED_NAME}, so this child cannot say anything about it"
    );

    let working = WorkingDirectory::at(&root).expect("the working directory resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );
    let outcome = spawn
        .execute(&CommandLine::split("env").expect("a command line"))
        .await
        .unwrap_or_else(|failure| panic!("`env` did not run: {failure}"));

    for line in outcome.stdout.lines() {
        println!("{ENV_MARKER}{line}");
    }
}

// -------------------------------------------- the runtime, while a child runs

/// How many polls of the loop below a condition is given before the check
/// refuses.
///
/// **A bound on failure, never a wait.** Every condition here is satisfied on
/// the first poll or two; this exists so that a mutant that never satisfies it
/// prints a sentence rather than hanging the suite. [Verification lessons] §20
/// — the wait is on the condition, the number only bounds the refusal.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
const POLL_BUDGET: usize = 100_000;

/// How many times the scripted children below look for their gate before
/// giving up, and the code they exit with when they do.
///
/// A child that spun forever would turn a red into a hang, and a hang is not
/// an instrument.
const GIVES_UP_AFTER: usize = 300;

/// The code a scripted child exits with when its gate never appeared.
const GAVE_UP: i32 = 9;

/// Write a shell script into the tree and give back the command line that runs
/// it.
///
/// `/bin/sh` by absolute path with one argument, so nothing here goes near
/// [`CommandLine::split`]'s refused constructs: the script's own `while`, `[`
/// and `$((…))` are the file's business and never text this workspace splits.
fn scripted(tree: &ScratchTree, name: &str, body: &str) -> CommandLine {
    let path = tree.project().join(name);
    std::fs::write(&path, body).unwrap_or_else(|why| panic!("staging: {name}: {why}"));
    CommandLine::of("/bin/sh", [path.display().to_string()])
        .expect("`/bin/sh` and one path are a program and one argument")
}

/// A child that waits for a file to appear, then exits zero.
fn waits_for(gate: &std::path::Path) -> String {
    format!(
        "n=0\nwhile [ ! -e '{}' ]; do\n  n=$((n+1))\n  if [ \"$n\" -gt {GIVES_UP_AFTER} ]; then \
         exit {GAVE_UP}; fi\n  sleep 0.01\ndone\nexit 0\n",
        gate.display()
    )
}

/// Whether a process id is still in the process table.
///
/// Linux, like everything else in this module that reads a signal number off
/// an exit status. A **zombie still has a `/proc` entry**, so absence here is
/// the stronger property: the child is dead *and* it has been reaped.
fn in_the_process_table(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// The runtime keeps being polled for the whole of a child.
///
/// # The instrument is a gate, not a clock
///
/// The child spins until a file appears and the file is written **from the
/// other branch of the same `select!`**, so the child can only finish if the
/// runtime polled something other than [`Spawn::execute`] while the child was
/// running. The tick is [`tokio::task::yield_now`], which reads no clock
/// ([Verification lessons] §57): what is asserted is the child's exit code,
/// which is `0` when the gate was written and [`GAVE_UP`] when it was not.
///
/// **The mutant is the shape this replaced.** Inserting
/// `while child.try_wait().ok().flatten().is_none() { std::thread::sleep(…) }`
/// at the top of `Spawn::wait` — the pre-2026-09-05 blocking poll — stops the
/// gate ever being written, and the child exits [`GAVE_UP`] on **every** run
/// rather than on some of them.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn the_runtime_is_still_polled_while_a_child_runs() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let gate = tree.project().join(awkward_nonce("gate"));
    assert!(
        !gate.exists(),
        "the staging is wrong: the gate the child is waiting for already exists, so this check \
         would pass without the runtime being polled at all"
    );
    let line = scripted(&tree, "waits-for-a-gate.sh", &waits_for(&gate));
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let running = spawn.execute(&line);
    tokio::pin!(running);
    let mut polls = 0_usize;
    let mut opened = false;
    let outcome = loop {
        tokio::select! {
            biased;

            outcome = &mut running => break outcome,

            () = tokio::task::yield_now() => {
                polls += 1;
                if !opened {
                    std::fs::write(&gate, b"").expect("the gate is written");
                    opened = true;
                }
                assert!(
                    polls < POLL_BUDGET,
                    "the child never finished in {POLL_BUDGET} polls, having been given its gate \
                     on poll 1. Either the runtime is not polling this loop while the child runs, \
                     or the child cannot see the gate"
                );
            }
        }
    };

    let outcome =
        outcome.unwrap_or_else(|failure| panic!("the scripted child did not run: {failure}"));
    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "the child exited {}, which is what it does when its gate never appeared — so the runtime \
         was not polled while it ran and nothing beside `execute` could make progress. It ran for \
         {:?}",
        outcome.ended.exit_code(),
        outcome.took,
    );
    assert!(
        polls > 0 && opened,
        "the loop never reached its other branch, so the assertion above says nothing"
    );
}

/// Dropping the future ends the child, and a child left alone reports instead.
///
/// # Both arms, because "the pid is gone" is true of a child that just exited
///
/// The first arm interrupts: the future is dropped while the child is spinning
/// on a gate that is never opened, and the child leaves the process table. The
/// second arm is its **accepting sibling** — the same script, the same
/// `execute`, the gate opened — and asserts that an outcome comes back at all.
/// Without it, a `Spawn` that killed every child the instant it started would
/// pass the first arm.
///
/// **The mutant is `kill_on_drop(false)`**, measured before this check was
/// written: the child then *survives* the runtime being dropped, so the wait
/// below exhausts its budget on every run.
#[tokio::test]
async fn an_interrupt_during_a_child_ends_the_child() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let never = tree.project().join(awkward_nonce("gate-that-never-opens"));
    let pidfile = tree.project().join(awkward_nonce("pid"));
    let body = format!("echo $$ > '{}'\n{}", pidfile.display(), waits_for(&never));
    let line = scripted(&tree, "reports-its-pid.sh", &body);
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let pid = {
        let running = spawn.execute(&line);
        tokio::pin!(running);
        let mut polls = 0_usize;
        // Wait on the condition — the child having reported its own pid —
        // rather than on any span of time.
        let pid = loop {
            tokio::select! {
                biased;

                outcome = &mut running => panic!(
                    "the child ended before it could be interrupted: {outcome:?}"
                ),

                () = tokio::task::yield_now() => {
                    polls += 1;
                    if let Ok(held) = std::fs::read_to_string(&pidfile)
                        && let Ok(pid) = held.trim().parse::<u32>()
                    {
                        break pid;
                    }
                    assert!(
                        polls < POLL_BUDGET,
                        "the child never wrote its process id in {POLL_BUDGET} polls, so killing \
                         it would say nothing about what an interrupt costs"
                    );
                }
            }
        };
        assert!(
            in_the_process_table(pid),
            "the staging is wrong: the child was already gone before it was interrupted"
        );
        pid
        // `running` is dropped here, which is exactly what a mid-turn
        // `Ctrl-C` does to the turn's future.
    };

    let mut polls = 0_usize;
    while in_the_process_table(pid) {
        polls += 1;
        assert!(
            polls < POLL_BUDGET,
            "the child {pid} is still in the process table after the future that owned it was \
             dropped, so an interrupt leaves a command running on the user's machine with nothing \
             left to stop it"
        );
        tokio::task::yield_now().await;
    }

    // The accepting sibling: the same script, uninterrupted, reports.
    let opens = tree.project().join(awkward_nonce("gate-that-opens"));
    std::fs::write(&opens, b"").expect("the gate is written");
    let line = scripted(&tree, "finishes.sh", &waits_for(&opens));
    let outcome = spawn
        .execute(&line)
        .await
        .unwrap_or_else(|failure| panic!("the uninterrupted child did not run: {failure}"));
    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "a child that was never interrupted did not report its own outcome, so the arm above \
         cannot distinguish an interrupt from a `Spawn` that kills everything"
    );
}

/// A child that ignores the polite signals is ended anyway.
///
/// `trap '' TERM INT` in the child, and it still leaves the process table —
/// which is what makes the signal the same `SIGKILL` the ceiling already
/// sends rather than something a well-behaved process can decline. Its
/// accepting sibling is the second arm of the check above.
#[tokio::test]
async fn a_child_that_ignores_the_signal_is_ended_anyway() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let never = tree.project().join(awkward_nonce("gate-that-never-opens"));
    let pidfile = tree.project().join(awkward_nonce("pid"));
    let body = format!(
        "trap '' TERM INT\necho $$ > '{}'\n{}",
        pidfile.display(),
        waits_for(&never)
    );
    let line = scripted(&tree, "ignores-the-signal.sh", &body);
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );

    let pid = {
        let running = spawn.execute(&line);
        tokio::pin!(running);
        let mut polls = 0_usize;
        // The loop is this block's tail, so `running` -- and with it the child
        // -- is dropped exactly here. That drop is the interrupt.
        loop {
            tokio::select! {
                biased;
                outcome = &mut running => panic!("the child ended too early: {outcome:?}"),
                () = tokio::task::yield_now() => {
                    polls += 1;
                    if let Ok(held) = std::fs::read_to_string(&pidfile)
                        && let Ok(pid) = held.trim().parse::<u32>()
                    {
                        break pid;
                    }
                    assert!(polls < POLL_BUDGET, "the child never wrote its process id");
                }
            }
        }
    };

    let mut polls = 0_usize;
    while in_the_process_table(pid) {
        polls += 1;
        assert!(
            polls < POLL_BUDGET,
            "a child that ignores `TERM` and `INT` outlived the interrupt, so the signal an \
             interrupt sends is one a process can decline"
        );
        tokio::task::yield_now().await;
    }
}

/// No thread is created to read a child, asserted over the source itself.
///
/// # Why the source rather than `/proc/self/task`
///
/// The count of live threads belongs to the **test binary**, which runs many
/// checks at once, so an assertion over it would be a property of whoever else
/// was running — the probabilistic instrument [Verification lessons] §57
/// refuses. The property is about this module, so it is asserted about this
/// module: `spawn.rs` names no thread at all.
///
/// The **liveness arm** is what stops a scan that read the wrong file from
/// passing vacuously (§18): the source must contain the `select!` this
/// property is about.
///
/// Watched red on a planted `std::thread::spawn`.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_child_is_read_by_futures_rather_than_by_threads() {
    let source = include_str!("spawn.rs");
    assert!(
        source.contains("tokio::select!"),
        "this scan read something that is not the spawn module, so it asserts nothing"
    );
    for named in ["std::thread::spawn", "thread::sleep", "JoinHandle"] {
        assert!(
            !source.contains(named),
            "`process::spawn` names `{named}`, so a child is read or waited for on a thread \
             again — which is the thread that used to outlive the ceiling when a grandchild held \
             the pipes, and the reason a turn could not be interrupted while a child ran"
        );
    }
}

/// The runtime the session actually builds can run a child.
///
/// # This is a property of `compose::turn::runtime`, not of this module
///
/// `tokio::process` needs the io driver: on a runtime built without it the
/// first child panics with *"A Tokio 1.x context was found, but IO is
/// disabled. Call `enable_io` on the runtime builder to enable IO."* Measured
/// rather than read. `compose::turn::runtime` builds `enable_all`, so it is
/// satisfied — and that is a rule holding by construction only while somebody
/// checks it, which is what this does. Narrowing that builder to `enable_time`
/// reddens this deterministically, with the panic above as its sentence.
#[test]
fn the_sessions_own_runtime_can_run_a_child() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        Environment::inherited_minimum().expect("the five"),
        generous(),
    );
    let runtime = crate::compose::turn::runtime().expect("the session's own runtime");
    let outcome = runtime
        .block_on(spawn.execute(&CommandLine::split("true").expect("a command line")))
        .unwrap_or_else(|failure| panic!("`true` did not run on the session's runtime: {failure}"));
    assert_eq!(
        outcome.ended,
        Ended::Exited { code: 0 },
        "the runtime every turn is polled on could not run a child"
    );
}
