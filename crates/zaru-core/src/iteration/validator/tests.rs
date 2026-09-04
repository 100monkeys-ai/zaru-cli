// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks over ADR-0009 D2 and D3.
//!
//! Every count these assert comes from the number of declarations the check
//! staged, never from the length of what the dispatch returned — two readings
//! taken through the same code agree with each other for as long as the defect
//! lives ([Verification lessons] §11, §17).
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::iteration::event::ValidatorOutcome;
use crate::iteration::port::{ExecutionOutcome, Validators};
use crate::iteration::validator::declaration::Declared;
use crate::iteration::validator::dispatch::Dispatch;
use crate::iteration::validator::expectation::Expect;
use crate::iteration::validator::fixtures::{
    NONCE, StagedPattern, StagedRunner, StagedSchema, command_for, declared, name, output_for,
    stderr_for, stdout_for,
};
use crate::iteration::validator::name::{Name, NameRefused, Pattern, Run, SchemaPath, TextRefused};
use crate::iteration::validator::plan::{Plan, PlanRefused};
use crate::iteration::validator::port::ValidatorOutput;

/// The candidate's execution, which ADR-0009's four kinds never read.
fn candidate_execution() -> ExecutionOutcome {
    ExecutionOutcome {
        exit_code: 0,
        stdout: String::new(),
        stderr: String::new(),
    }
}

/// The names a dispatch reported, in the order it reported them.
fn reported_names(reports: &[crate::iteration::port::ValidatorReport]) -> Vec<String> {
    reports.iter().map(|report| report.name.clone()).collect()
}

/// The outcomes a dispatch reported, in the order it reported them.
fn reported_outcomes(reports: &[crate::iteration::port::ValidatorReport]) -> Vec<ValidatorOutcome> {
    reports.iter().map(|report| report.outcome).collect()
}

// --- ADR-0009 D2: the order ------------------------------------------------

#[test]
fn dependency_order_is_not_file_order() {
    // The dependent is declared FIRST on purpose. A fixture declaring these in
    // dependency order cannot tell a correct implementation from one that
    // sorts by declaration index, which is the mutant this exists to catch.
    let declarations = vec![
        declared("lint").after([name("test")]),
        declared("test").after([name("build")]),
        declared("build"),
    ];
    let plan = Plan::from_declared(declarations).expect("the dependencies resolve");

    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    assert_eq!(
        order,
        vec!["build", "test", "lint"],
        "ADR-0009 D2 orders validators by declared dependency and not by file order; these were \
         declared in exactly the reverse of their dependency order"
    );
}

#[test]
fn validators_with_no_dependency_between_them_keep_their_declaration_order() {
    // The other half of the same rule: file order is a tie-break, so an order
    // that moved between runs would make the event stream unreadable.
    let plan = Plan::from_declared(vec![declared("gamma"), declared("alpha"), declared("beta")])
        .expect("no dependencies to resolve");
    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    assert_eq!(
        order,
        vec!["gamma", "alpha", "beta"],
        "independent validators run in the order they were declared; sorting them by name would \
         be this module inventing an order ADR-0009 does not state"
    );
}

#[test]
fn a_cycle_is_refused_naming_every_member_and_not_only_the_first() {
    let refusal = Plan::from_declared(vec![
        declared("a").after([name("c")]),
        declared("b").after([name("a")]),
        declared("c").after([name("b")]),
        declared("free"),
    ])
    .expect_err("a cycle cannot be ordered");

    let PlanRefused::Cycle { members } = &refusal else {
        panic!("expected a cycle refusal, got {refusal:?}");
    };
    let named: Vec<&str> = members.iter().map(Name::as_str).collect();
    assert_eq!(
        named,
        vec!["a", "b", "c"],
        "a cycle refusal carries every member so a reader does not have to trace it by hand"
    );
    let rendered = refusal.to_string();
    for member in ["a", "b", "c"] {
        assert!(
            rendered.contains(member),
            "the refusal does not name {member:?}: {rendered}"
        );
    }
    assert!(
        !rendered.contains("free"),
        "the refusal names a validator that is not in the cycle: {rendered}"
    );
}

#[test]
fn a_validator_naming_itself_is_a_cycle_of_one() {
    let refusal = Plan::from_declared(vec![declared("build").after([name("build")])])
        .expect_err("a self-reference cannot be ordered");
    let PlanRefused::Cycle { members } = &refusal else {
        panic!("expected a cycle refusal, got {refusal:?}");
    };
    assert_eq!(members.len(), 1, "one validator is caught in it");
    assert_eq!(members[0].as_str(), "build");
}

#[test]
fn an_unknown_prerequisite_is_refused_naming_both_validators() {
    let refusal = Plan::from_declared(vec![declared("test").after([name("bulid")])])
        .expect_err("a prerequisite nothing declares cannot be resolved");
    assert_eq!(
        refusal,
        PlanRefused::UnknownPrerequisite {
            validator: name("test"),
            missing: name("bulid"),
        },
        "a prerequisite that is silently ignored is a validator that runs when D2 says it must not"
    );
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("test") && rendered.contains("bulid"),
        "{rendered}"
    );
}

#[test]
fn two_validators_declared_with_one_name_are_refused() {
    let refusal = Plan::from_declared(vec![declared("build"), declared("test"), declared("build")])
        .expect_err("`after` cannot name one of two");
    assert_eq!(
        refusal,
        PlanRefused::DuplicateName {
            name: name("build")
        }
    );
}

#[test]
fn a_prerequisite_named_twice_is_one_edge_and_not_a_cycle() {
    // The mutant is counting prerequisites rather than distinct ones, which
    // leaves an indegree that nothing can ever decrement to zero -- and it
    // reports as a cycle, which is the most confusing refusal available for an
    // input that says the same true thing twice.
    let plan = Plan::from_declared(vec![
        declared("build"),
        declared("test").after([name("build"), name("build")]),
    ])
    .expect("saying it twice says the same thing");
    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    assert_eq!(order, vec!["build", "test"]);
}

#[test]
fn a_manifest_with_no_validators_is_a_plan_rather_than_a_refusal() {
    // ADR-0009 D4's project with nothing declared is an ordinary thing, and
    // refusing it here would make D4 unreachable.
    let plan = Plan::from_declared(Vec::new()).expect("declaring nothing is legal");
    assert!(plan.is_empty());
    assert_eq!(plan.len(), 0);
}

// --- ADR-0009 D2: skipped is distinct --------------------------------------

#[tokio::test]
async fn a_validator_whose_prerequisite_failed_is_skipped_and_its_command_is_never_run() {
    let declarations = vec![
        declared("build"),
        declared("test").after([name("build")]),
        declared("lint").after([name("build")]),
    ];
    let staged = declarations.len();
    let plan = Plan::from_declared(declarations).expect("the dependencies resolve");

    // `build` fails; both of its dependents must be skipped, and neither of
    // their commands may be run.
    let runner = StagedRunner::new()
        .staging(
            &Run::new(command_for("build")).expect("a command"),
            output_for("build", 1),
        )
        .staging(
            &Run::new(command_for("test")).expect("a command"),
            output_for("test", 0),
        )
        .staging(
            &Run::new(command_for("lint")).expect("a command"),
            output_for("lint", 0),
        );
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();
    let dispatch = Dispatch::new(&plan, &runner, &pattern, &schema);

    let reports = dispatch
        .evaluate(&candidate_execution())
        .await
        .expect("the ports all answered");

    assert_eq!(
        reports.len(),
        staged,
        "every declared validator is considered and gets exactly one report; the denominator is \
         the number this check staged, not the length of what the dispatch returned"
    );
    assert_eq!(
        reported_outcomes(&reports),
        vec![
            ValidatorOutcome::Failed,
            ValidatorOutcome::Skipped,
            ValidatorOutcome::Skipped
        ],
        "ADR-0009 D2 makes `skipped` distinct from `passed`; reporting a skipped validator as \
         passed is the silent green that decision exists to prevent"
    );

    // The half that is invisible from the outcomes alone: an implementation
    // that ran the command and threw the result away reports identically.
    assert_eq!(
        runner.asked(),
        vec![command_for("build")],
        "a skipped validator's command must never be run; the runner was asked for {:?}",
        runner.asked()
    );
}

#[tokio::test]
async fn a_skip_propagates_down_a_chain_of_prerequisites() {
    // ADR-0009 D2 says what happens to a validator whose prerequisite FAILED
    // and is silent about one whose prerequisite was SKIPPED. Under a
    // delegated coordinator ruling of 2026-09-04 a skip propagates, and this
    // is the check that pins it: `lint` comes after `test`, which was skipped
    // rather than failed.
    let declarations = vec![
        declared("build"),
        declared("test").after([name("build")]),
        declared("lint").after([name("test")]),
    ];
    let plan = Plan::from_declared(declarations).expect("the dependencies resolve");
    let runner = StagedRunner::new().staging(
        &Run::new(command_for("build")).expect("a command"),
        output_for("build", 1),
    );
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();
    let dispatch = Dispatch::new(&plan, &runner, &pattern, &schema);

    let reports = dispatch
        .evaluate(&candidate_execution())
        .await
        .expect("the ports all answered");
    assert_eq!(
        reported_outcomes(&reports),
        vec![
            ValidatorOutcome::Failed,
            ValidatorOutcome::Skipped,
            ValidatorOutcome::Skipped
        ],
        "a prerequisite that did not run cannot have passed, so its dependent is skipped too"
    );
    assert_eq!(
        runner.asked(),
        vec![command_for("build")],
        "neither validator downstream of the failure may be run"
    );
}

#[tokio::test]
async fn every_declared_validator_gets_one_report_in_the_plans_order() {
    let declarations = vec![
        declared("lint").after([name("test")]),
        declared("test").after([name("build")]),
        declared("build"),
    ];
    let staged = declarations.len();
    let plan = Plan::from_declared(declarations).expect("the dependencies resolve");
    let runner = StagedRunner::new()
        .staging(
            &Run::new(command_for("build")).expect("a command"),
            output_for("build", 0),
        )
        .staging(
            &Run::new(command_for("test")).expect("a command"),
            output_for("test", 0),
        )
        .staging(
            &Run::new(command_for("lint")).expect("a command"),
            output_for("lint", 0),
        );
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();
    let dispatch = Dispatch::new(&plan, &runner, &pattern, &schema);

    let reports = dispatch
        .evaluate(&candidate_execution())
        .await
        .expect("the ports all answered");
    assert_eq!(reports.len(), staged);
    assert_eq!(
        reported_names(&reports),
        vec!["build", "test", "lint"],
        "the loop's `Validators` port documents one entry per validator considered, in the order \
         ADR-0009 D2's declared dependencies put them"
    );
    assert_eq!(
        runner.asked(),
        vec![
            command_for("build"),
            command_for("test"),
            command_for("lint")
        ],
        "the commands run in the plan's order too, not in the order they were declared"
    );
}

// --- ADR-0009 D3: the four kinds -------------------------------------------

#[test]
fn the_expect_vocabulary_is_the_four_kinds_d3_names_and_there_is_no_fifth() {
    // The exhaustive match is the mechanism: a fifth variant stops this
    // compiling, which is louder than any assertion. The list below is D3's
    // own table order.
    let every = [
        Expect::ExitZero,
        Expect::ExitCode(3),
        Expect::Matches(Pattern::new("ok").expect("a pattern")),
        Expect::JsonSchema(SchemaPath::new("schema/output.json").expect("a path")),
    ];
    let kinds: Vec<&str> = every.iter().map(Expect::kind).collect();
    assert_eq!(kinds, Expect::KINDS.to_vec());
    assert_eq!(
        Expect::KINDS.len(),
        4,
        "ADR-0009 D3: \"Four `expect` kinds and no more, in this version\""
    );

    // Which half of the vocabulary the product can decide today, asserted
    // rather than described: two kinds need a crate ADR-0003 D2's table does
    // not name.
    let ported: Vec<&str> = every
        .iter()
        .filter(|expect| expect.needs_an_evaluator_port())
        .map(Expect::kind)
        .collect();
    assert_eq!(ported, vec!["matches", "json_schema"]);
}

#[tokio::test]
async fn exit_zero_and_exit_code_pass_and_fail_with_no_port_involved() {
    // Both kinds, both answers, four cases -- and the ports are staged empty,
    // so if the dispatch reached one of them the fixture would panic rather
    // than answer.
    for (expect, exit_code, expected) in [
        (Expect::ExitZero, 0, ValidatorOutcome::Passed),
        (Expect::ExitZero, 1, ValidatorOutcome::Failed),
        (Expect::ExitCode(3), 3, ValidatorOutcome::Passed),
        (Expect::ExitCode(3), 0, ValidatorOutcome::Failed),
    ] {
        let mut one = declared("check");
        one.expect = expect.clone();
        let plan = Plan::from_declared(vec![one]).expect("one validator resolves");
        let runner = StagedRunner::new().staging(
            &Run::new(command_for("check")).expect("a command"),
            output_for("check", exit_code),
        );
        let pattern = StagedPattern::new();
        let schema = StagedSchema::new();
        let dispatch = Dispatch::new(&plan, &runner, &pattern, &schema);

        let reports = dispatch
            .evaluate(&candidate_execution())
            .await
            .expect("the runner answered");
        assert_eq!(
            reports[0].outcome,
            expected,
            "{} against exit code {exit_code} should be {expected:?}",
            expect.kind()
        );
        assert!(
            pattern.asked().is_empty() && schema.asked().is_empty(),
            "{} is decided with std and must reach no evaluator port",
            expect.kind()
        );
    }
}

#[tokio::test]
async fn matches_and_json_schema_pass_and_fail_through_their_own_ports() {
    let pattern_text = Pattern::new("^ok$").expect("a pattern");
    let schema_path = SchemaPath::new("schema/output.json").expect("a path");

    for answer in [true, false] {
        let expected = if answer {
            ValidatorOutcome::Passed
        } else {
            ValidatorOutcome::Failed
        };

        let mut matching = declared("shape");
        matching.expect = Expect::Matches(pattern_text.clone());
        let plan = Plan::from_declared(vec![matching]).expect("one validator resolves");
        let runner = StagedRunner::new().staging(
            &Run::new(command_for("shape")).expect("a command"),
            output_for("shape", 0),
        );
        let pattern = StagedPattern::new().answering(&pattern_text, answer);
        let schema = StagedSchema::new();
        let reports = Dispatch::new(&plan, &runner, &pattern, &schema)
            .evaluate(&candidate_execution())
            .await
            .expect("the ports answered");
        assert_eq!(reports[0].outcome, expected, "matches answering {answer}");
        // The port is handed the command's own standard output rather than the
        // candidate's, which is what D3's "Stdout matches" is about.
        assert_eq!(
            pattern.asked(),
            vec![(pattern_text.as_str().to_owned(), stdout_for("shape"))],
            "the pattern port is handed the validator command's standard output"
        );

        let mut validating = declared("shape");
        validating.expect = Expect::JsonSchema(schema_path.clone());
        let plan = Plan::from_declared(vec![validating]).expect("one validator resolves");
        let runner = StagedRunner::new().staging(
            &Run::new(command_for("shape")).expect("a command"),
            output_for("shape", 0),
        );
        let pattern = StagedPattern::new();
        let schema = StagedSchema::new().answering(&schema_path, answer);
        let reports = Dispatch::new(&plan, &runner, &pattern, &schema)
            .evaluate(&candidate_execution())
            .await
            .expect("the ports answered");
        assert_eq!(
            reports[0].outcome, expected,
            "json_schema answering {answer}"
        );
        assert_eq!(
            schema.asked(),
            vec![(schema_path.as_str().to_owned(), stdout_for("shape"))],
            "the schema port is handed the validator command's standard output"
        );
    }
}

// --- ADR-0009 D5: what a failing validator carries -------------------------

#[tokio::test]
async fn a_failing_validator_carries_both_captured_streams_verbatim() {
    let plan = Plan::from_declared(vec![declared("test")]).expect("one validator resolves");
    let runner = StagedRunner::new().staging(
        &Run::new(command_for("test")).expect("a command"),
        output_for("test", 1),
    );
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();
    let reports = Dispatch::new(&plan, &runner, &pattern, &schema)
        .evaluate(&candidate_execution())
        .await
        .expect("the runner answered");

    let detail = &reports[0].detail;
    assert!(
        detail.contains(&stdout_for("test")),
        "ADR-0009 D5 sends the captured standard output into refinement verbatim; it is not in \
         {detail:?}"
    );
    assert!(
        detail.contains(&stderr_for("test")),
        "ADR-0009 D5 says stdout AND stderr; the standard error is not in {detail:?}"
    );
    assert!(
        detail.contains(NONCE),
        "the detail carries no nonce, so it could have been produced without carrying anything"
    );
    // The staged standard error ends with a trailing space on purpose; a
    // dispatch that trimmed the captured output would lose it, and a fixture
    // awkward only on its encoding would not have noticed.
    assert!(
        detail.ends_with(" \n"),
        "the captured output was trimmed; ADR-0008 D4 carries failure text verbatim: {detail:?}"
    );
}

#[tokio::test]
async fn a_passing_or_skipped_validator_carries_no_detail() {
    let plan = Plan::from_declared(vec![
        declared("build"),
        declared("test").after([name("build")]),
    ])
    .expect("the dependencies resolve");
    let runner = StagedRunner::new().staging(
        &Run::new(command_for("build")).expect("a command"),
        output_for("build", 0),
    );
    // `test` is staged to pass too, so both non-failing outcomes are covered.
    let runner = runner.staging(
        &Run::new(command_for("test")).expect("a command"),
        output_for("test", 0),
    );
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();
    let reports = Dispatch::new(&plan, &runner, &pattern, &schema)
        .evaluate(&candidate_execution())
        .await
        .expect("the runner answered");
    for report in &reports {
        assert_eq!(report.outcome, ValidatorOutcome::Passed);
        assert!(
            report.detail.is_empty(),
            "a passing validator carries no failure text; the loop only reads a failing one's"
        );
    }
}

// --- Ports fail as ports, not as failing validators -------------------------

#[tokio::test]
async fn a_command_that_could_not_be_run_is_a_port_failure_and_not_a_failing_validator() {
    let command = Run::new(command_for("build")).expect("a command");
    let plan = Plan::from_declared(vec![declared("build")]).expect("one validator resolves");
    let runner = StagedRunner::new().failing_for(&command);
    let pattern = StagedPattern::new();
    let schema = StagedSchema::new();

    let outcome = Dispatch::new(&plan, &runner, &pattern, &schema)
        .evaluate(&candidate_execution())
        .await;
    let failure = outcome.expect_err(
        "a command that could not be run at all is the harness failing, not the validator",
    );
    assert!(
        failure.to_string().contains(NONCE),
        "the port's own wording is carried out unchanged: {failure}"
    );
}

#[tokio::test]
async fn an_unusable_pattern_or_schema_is_a_port_failure_and_not_a_failing_validator() {
    for (label, expect) in [
        (
            "matches",
            Expect::Matches(Pattern::new("[unclosed").expect("a pattern")),
        ),
        (
            "json_schema",
            Expect::JsonSchema(SchemaPath::new("schema/missing.json").expect("a path")),
        ),
    ] {
        let mut one = declared("shape");
        one.expect = expect;
        let plan = Plan::from_declared(vec![one]).expect("one validator resolves");
        let runner = StagedRunner::new().staging(
            &Run::new(command_for("shape")).expect("a command"),
            output_for("shape", 0),
        );
        let pattern = StagedPattern::failing();
        let schema = StagedSchema::failing();
        let outcome = Dispatch::new(&plan, &runner, &pattern, &schema)
            .evaluate(&candidate_execution())
            .await;
        assert!(
            outcome.is_err(),
            "{label}: a declaration the evaluator cannot use is the manifest being wrong, which \
             is not the same thing as the validator failing"
        );
    }
}

// --- The refusals on the declaration's own text -----------------------------

#[test]
fn a_validator_name_refuses_what_after_and_the_event_stream_cannot_carry() {
    assert_eq!(Name::new(""), Err(NameRefused::Empty));
    assert!(matches!(
        Name::new("bu\u{7}ild"),
        Err(NameRefused::Control { .. })
    ));
    assert!(matches!(
        Name::new(" build"),
        Err(NameRefused::SurroundingWhitespace { .. })
    ));
    assert!(
        Name::new("build").is_ok(),
        "an ordinary name is taken; a check that only ever refuses says nothing about a rule"
    );
    assert!(
        Name::new("ビルド").is_ok(),
        "there is no character allowlist: refusing a non-Latin name is a restriction no record \
         states"
    );
}

#[test]
fn an_empty_run_pattern_or_schema_path_is_refused_and_a_multi_line_command_is_not() {
    assert_eq!(Run::new("   "), Err(TextRefused::EmptyRun));
    assert_eq!(Pattern::new(""), Err(TextRefused::EmptyPattern));
    assert_eq!(SchemaPath::new(" "), Err(TextRefused::EmptySchemaPath));

    // The permitted side, which is what makes the refusals mean anything: TOML
    // can express a multi-line string and a multi-line shell command is an
    // ordinary thing to declare, so refusing a newline here would be inventing
    // a restriction no record states.
    assert!(
        Run::new("set -e\ncargo build --locked\n").is_ok(),
        "a multi-line command is a legal declaration"
    );
    assert!(Pattern::new("^ok$").is_ok());
    assert!(SchemaPath::new("schema/output.json").is_ok());
}

#[test]
fn an_empty_pattern_is_refused_because_it_would_pass_whatever_happened() {
    // Not a taste question: an empty regular expression matches every possible
    // standard output, so the validator is green by construction.
    let refusal = Pattern::new("").expect_err("an empty pattern is refused");
    assert!(
        refusal.to_string().contains("every standard output"),
        "the refusal has to say why, or a reader will delete it: {refusal}"
    );
}

// --- The candidate's execution is not read ----------------------------------

#[tokio::test]
async fn the_dispatch_reports_the_same_thing_whatever_the_candidates_execution_was() {
    // ADR-0009 D3's four kinds are all statements about the validator
    // COMMAND's output, so the candidate's execution is not an input. Recorded
    // as a finding on ADR-0008 and ADR-0009; asserted here so that a future
    // implementation reading it is a visible change rather than a silent one.
    //
    // **The staged validator FAILS**, and that is the whole check rather than
    // a detail. Its first version staged a passing one, so the branch that
    // builds a failing validator's detail never ran, and the mutation that
    // appends the candidate's own standard output to that detail survived --
    // Verification lessons §9, found by running rather than by reading.
    let mut seen = Vec::new();
    for execution in [
        ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
        ExecutionOutcome {
            exit_code: 127,
            stdout: format!("{NONCE}-candidate-stdout"),
            stderr: format!("{NONCE}-candidate-stderr"),
        },
    ] {
        let plan = Plan::from_declared(vec![declared("build")]).expect("one validator resolves");
        let runner = StagedRunner::new().staging(
            &Run::new(command_for("build")).expect("a command"),
            output_for("build", 1),
        );
        let pattern = StagedPattern::new();
        let schema = StagedSchema::new();
        let reports = Dispatch::new(&plan, &runner, &pattern, &schema)
            .evaluate(&execution)
            .await
            .expect("the runner answered");
        seen.push(reports);
    }
    assert_eq!(
        seen[0], seen[1],
        "the candidate's execution reached a report, which ADR-0009 D3 does not describe"
    );
}

// --- The output shape is not the loop's -------------------------------------

#[test]
fn a_validator_output_is_its_own_shape_and_not_the_loops_execution_outcome() {
    // Three coinciding field names, two different things: one is what making
    // the candidate's effect real produced, the other is what evaluating it
    // cost. ADR-0008 D1 keeps the two loops apart and this is that separation
    // in the types. The mutant is a `From` impl between them, which would make
    // the two interchangeable at every call site.
    let output = ValidatorOutput {
        exit_code: 2,
        stdout: stdout_for("build"),
        stderr: stderr_for("build"),
    };
    let execution = ExecutionOutcome {
        exit_code: 2,
        stdout: stdout_for("build"),
        stderr: stderr_for("build"),
    };
    assert_eq!(output.exit_code, execution.exit_code);
    assert_eq!(output.stdout, execution.stdout);
    assert_eq!(output.stderr, execution.stderr);
}

// --- Nothing in the product tree implements a validator port ----------------

#[test]
fn no_product_source_in_this_crate_implements_a_validator_port() {
    // The same claim `zaru-core`'s module docs make, as a mechanism rather
    // than a sentence. It scans this crate's product sources -- everything
    // under `src` except the two files that are the test tree -- and refuses
    // an implementation of any of the three ports.
    //
    // It prints the number of files it scanned and fails on zero, because an
    // instrument that could not have found anything is not evidence
    // (Verification lessons §8).
    //
    // **The needle is `<Port> for` rather than `impl <Port> for`**, and that
    // is not a stylistic choice. Its first version looked for the second, and
    // the mutation that planted an implementation survived because the planted
    // line spelled the trait through its full path -- `impl
    // crate::iteration::validator::port::ValidatorRunner for Planted`. Agent
    // lessons §44: when a rule is enforced by matching source text, the
    // matching is part of the rule, and a caller the predicate misses is a
    // caller the rule quietly does not apply to. Comment lines are stripped
    // first, so a doc comment naming a port is not an offender.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources = product_sources(&root);
    let lines: usize = sources.iter().map(|(_, body)| body.lines().count()).sum();
    assert!(
        sources.len() >= 8 && lines >= 500,
        "scanned {} product source file(s) and {lines} line(s) under {}, which is less than this \
         crate has; the walk is broken rather than the crate clean",
        sources.len(),
        root.display()
    );

    let mut offenders = Vec::new();
    for (path, body) in &sources {
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for port in ["ValidatorRunner", "PatternMatch", "SchemaValidate"] {
            if code.contains(&format!("{port} for ")) {
                offenders.push(format!("{} implements {port}", path.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "nothing in this crate's product tree may implement a validator port; found {} across {} \
         file(s) and {lines} line(s): {:?}",
        offenders.len(),
        sources.len(),
        offenders
    );
}

/// Every `.rs` file under `root` that is not part of the test tree.
///
/// `fixtures.rs` and `tests.rs` are this workspace's convention for a module's
/// test tree, and they are excluded by name rather than by a pattern over the
/// contents, so a product file cannot exempt itself by mentioning one.
fn product_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut found = Vec::new();
    let mut frontier = vec![root.to_path_buf()];
    while let Some(here) = frontier.pop() {
        let entries = std::fs::read_dir(&here)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", here.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                frontier.push(path);
                continue;
            }
            if path.extension().is_some_and(|extension| extension == "rs")
                && !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("fixtures.rs" | "tests.rs")
                )
            {
                let body = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
                found.push((path, body));
            }
        }
    }
    found
}

#[test]
fn a_declared_validator_carries_the_four_fields_adr_0009_d1_names() {
    // Destructured exhaustively: a fifth field, or a renamed one, stops this
    // compiling rather than passing.
    let one = declared("test").after([name("build")]);
    let Declared {
        name: declared_name,
        run,
        expect,
        after,
    } = one;
    assert_eq!(declared_name.as_str(), "test");
    assert_eq!(run.as_str(), command_for("test"));
    assert_eq!(expect, Expect::ExitZero);
    assert_eq!(after, vec![name("build")]);
}
