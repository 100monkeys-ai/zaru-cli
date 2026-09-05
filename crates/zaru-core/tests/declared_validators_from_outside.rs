// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives ADR-0009's dispatch from outside the crate, through the loop.
//!
//! Every other check on the dispatch lives inside `zaru-core` and reaches its
//! subject directly. That proves the mechanism and says nothing about whether
//! the mechanism can be reached — a capability whose only callers are unit
//! tests is one nobody has been shown able to use, and the gap is invisible to
//! a green suite because there is no mutant for a declaration that was never
//! made public ([Verification lessons] §25).
//!
//! So this file builds a plan and implements the three validator ports using
//! **only what `zaru-core` exports**, hands the resulting `Dispatch` to the
//! loop as its `Validators`, and reads the outcome off the event stream. It
//! spawns no process, compiles no pattern, opens no file and reaches no
//! network.
//!
//! **This is evidence about the mechanism and not about the `zaru` binary**,
//! which reaches none of it.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use core::time::Duration;
use std::sync::Mutex;
use zaru_core::iteration::validator::{
    Declared, Dispatch, Expect, Name, Pattern, PatternMatch, Plan, Run, SchemaPath, SchemaValidate,
    ValidatorOutput, ValidatorRunner,
};
use zaru_core::iteration::{
    Ceiling, Clock, ContextPolicy, ContextRefusal, Event, EventSink, ExecutionOutcome, Executor,
    Generated, Generator, Limits, Outcome, PortFailure, Ports, Prompt, TruncationBudget, Turn,
    ValidatorOutcome, run,
};
use zaru_core::redaction::{Redacted, Redactor};

/// A nonce with an embedded newline and a non-ASCII character, so that text
/// arriving downstream can only have got there by being carried.
const NONCE: &str = "zaru-outside-nonce-b13d";

/// What the failing validator's command writes to standard output.
fn failing_stdout() -> String {
    format!("{NONCE}\ntest: 2 failed — left ≠ right\n")
}

/// What the failing validator's command writes to standard error.
///
/// Ends with a trailing space on purpose: a fixture that is awkward only in
/// its encoding is ordinary on the whitespace axis, and a mutation that
/// trimmed the captured output would survive it ([Verification lessons] §51).
fn failing_stderr() -> String {
    format!("{NONCE}-stderr\nerror: aborting due to 2 previous errors ✗ \n")
}

// --- The three validator ports, implemented from outside the crate ---------

struct StagedRunner {
    asked: Mutex<Vec<String>>,
}

impl ValidatorRunner for StagedRunner {
    async fn run(&self, command: &Run) -> Result<ValidatorOutput, PortFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push(command.as_str().to_owned());
        // `cargo build --locked` succeeds; `cargo test --all` does not. No
        // process is started: these are the bytes this check chose.
        Ok(match command.as_str() {
            "cargo build --locked" => ValidatorOutput {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
            "cargo test --all" => ValidatorOutput {
                exit_code: 101,
                stdout: failing_stdout(),
                stderr: failing_stderr(),
            },
            other => panic!("nothing staged the command {other:?}"),
        })
    }
}

struct NeverAsked;

impl PatternMatch for NeverAsked {
    async fn matches(&self, pattern: &Pattern, _stdout: &str) -> Result<bool, PortFailure> {
        panic!("no validator in this plan expects a pattern, yet one was asked: {pattern:?}")
    }
}

impl SchemaValidate for NeverAsked {
    async fn validates(&self, schema: &SchemaPath, _stdout: &str) -> Result<bool, PortFailure> {
        panic!("no validator in this plan expects a schema, yet one was asked: {schema:?}")
    }
}

// --- The loop's other four ports -------------------------------------------

struct FrozenClock;

impl Clock for FrozenClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

struct Echo;

impl ContextPolicy for Echo {
    // ADR-0013's arc gave this port its own refusal type on 2026-09-04, so a
    // context that cannot be assembled is distinguishable from a port that
    // failed. This policy refuses nothing and never returns either.
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let text = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
            Turn::Resumed { interrupted } => format!(
                "the previous session was interrupted and this call never completed: {}",
                interrupted.call()
            ),
        };
        Ok(Prompt::new(Redacted::by(&NothingHeld, &text)))
    }
}

struct Counting(Mutex<u32>);

impl Generator for Counting {
    type Candidate = String;

    async fn generate(&self, prompt: &Prompt) -> Result<Generated<Self::Candidate>, PortFailure> {
        let mut seen = self.0.lock().expect("counter poisoned");
        *seen += 1;
        Ok(Generated {
            candidate: format!(
                "candidate {} from a {}-byte prompt",
                *seen,
                prompt.as_str().len()
            ),
            tokens: 1,
        })
    }
}

struct Inert;

impl Executor for Inert {
    type Candidate = String;

    async fn execute(&self, candidate: &Self::Candidate) -> Result<ExecutionOutcome, PortFailure> {
        Ok(ExecutionOutcome {
            exit_code: 0,
            stdout: candidate.clone(),
            stderr: String::new(),
        })
    }
}

#[derive(Default)]
struct Recording {
    events: Vec<Event>,
}

impl EventSink for Recording {
    fn emit(&mut self, event: &Event) {
        self.events.push(event.clone());
    }
}

/// ADR-0009 D1's own worked manifest, minus the two `expect` kinds whose
/// evaluators are ports with no implementation, as declarations.
fn adr_0009_d1s_validators() -> Vec<Declared> {
    vec![
        // Declared in the record's order, which is also dependency order --
        // and `lint` is added out of order so the plan has something to do.
        Declared::new(
            Name::new("lint").expect("a name"),
            Run::new("cargo clippy --workspace").expect("a command"),
            Expect::ExitZero,
        )
        .after([Name::new("test").expect("a name")]),
        Declared::new(
            Name::new("build").expect("a name"),
            Run::new("cargo build --locked").expect("a command"),
            Expect::ExitZero,
        ),
        Declared::new(
            Name::new("test").expect("a name"),
            Run::new("cargo test --all").expect("a command"),
            Expect::ExitZero,
        )
        .after([Name::new("build").expect("a name")]),
    ]
}

#[tokio::test]
async fn a_caller_outside_this_crate_declares_validators_and_the_loop_reports_on_each() {
    let declared = adr_0009_d1s_validators();
    let staged = declared.len();
    let plan = Plan::from_declared(declared).expect("ADR-0009 D1's dependencies resolve");

    // The order is derived, and it is not the order they were declared in.
    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    assert_eq!(
        order,
        vec!["build", "test", "lint"],
        "ADR-0009 D2 orders by declared dependency; these were declared lint, build, test"
    );

    let runner = StagedRunner {
        asked: Mutex::new(Vec::new()),
    };
    let dispatch = Dispatch::new(&plan, &runner, &NeverAsked, &NeverAsked);

    let generator = Counting(Mutex::new(0));
    let mut recording = Recording::default();
    let sinks: &mut [&mut dyn EventSink] = &mut [&mut recording];

    let outcome = run(
        "make the suite pass",
        Limits {
            ceiling: Ceiling::new(1).expect("one iteration"),
            budget: TruncationBudget::new(4096).expect("a budget"),
        },
        Ports {
            generator: &generator,
            executor: &Inert,
            validators: &dispatch,
            context: &Echo,
            clock: &FrozenClock,
            redactor: &NothingHeld,
        },
        sinks,
    )
    .await
    .expect("no port failed");

    // One event per validator the check staged -- the denominator comes from
    // the declarations rather than from the stream it is being compared to.
    let evaluated: Vec<(&str, ValidatorOutcome)> = recording
        .events
        .iter()
        .filter_map(|event| match event {
            Event::ValidatorEvaluated { name, outcome, .. } => Some((name.as_str(), *outcome)),
            _ => None,
        })
        .collect();
    assert_eq!(evaluated.len(), staged);
    assert_eq!(
        evaluated,
        vec![
            ("build", ValidatorOutcome::Passed),
            ("test", ValidatorOutcome::Failed),
            ("lint", ValidatorOutcome::Skipped),
        ],
        "ADR-0009 D2's three outcomes reach ADR-0008 D3's event stream, one event each, in the \
         order the declared dependencies put them"
    );

    // `lint` was skipped, so its command was never run. The runner's own list
    // is the only reading that separates that from a result thrown away.
    assert_eq!(
        runner.asked(),
        vec!["cargo build --locked", "cargo test --all"],
        "a skipped validator's command must never be run"
    );

    // Exhaustion at a ceiling of one, carrying the failing validator's output.
    let Outcome::Exhausted { last_failure, .. } = &outcome else {
        panic!("one iteration with a failing validator is exhausted, not {outcome:?}");
    };
    // `last_failure` became an `Option` on 2026-09-04 when ADR-0013's second
    // exhaustion route landed: a run that stopped because the context window
    // would be exceeded has no validator failure to carry. Asserting `Some`
    // here is therefore a stronger claim than the previous one, and it is the
    // claim ADR-0009 D5 is about -- exhaustion by CEILING carries the failing
    // validator's own output.
    let carried = last_failure
        .as_deref()
        .expect("exhaustion at the ceiling carries the failing validator's output");
    assert!(
        carried.contains(&failing_stdout()) && carried.contains(&failing_stderr()),
        "ADR-0009 D5 sends the captured stdout AND stderr into the loop's failure text: \
         {carried:?}"
    );
}

#[tokio::test]
async fn a_failing_validators_captured_output_reaches_the_refinement_prompt_byte_for_byte() {
    // ADR-0009 trigger clause 4, asserted jointly with ADR-0008 D4 as that
    // clause requires: the bytes a validator's command produced travel through
    // the dispatch, through the loop's failure text, into the refinement
    // prompt, and into the prompt the next generation is handed. Nothing on
    // that path may paraphrase, and no new path was added for it.
    let plan =
        Plan::from_declared(adr_0009_d1s_validators()).expect("ADR-0009 D1's dependencies resolve");
    let runner = StagedRunner {
        asked: Mutex::new(Vec::new()),
    };
    let dispatch = Dispatch::new(&plan, &runner, &NeverAsked, &NeverAsked);
    let generator = Counting(Mutex::new(0));
    let mut recording = Recording::default();
    let sinks: &mut [&mut dyn EventSink] = &mut [&mut recording];

    // Two iterations, so a refinement is actually constructed.
    let outcome = run(
        "make the suite pass",
        Limits {
            ceiling: Ceiling::new(2).expect("two iterations"),
            budget: TruncationBudget::new(4096).expect("a budget"),
        },
        Ports {
            generator: &generator,
            executor: &Inert,
            validators: &dispatch,
            context: &Echo,
            clock: &FrozenClock,
            redactor: &NothingHeld,
        },
        sinks,
    )
    .await
    .expect("no port failed");
    assert!(matches!(outcome, Outcome::Exhausted { .. }));

    let excerpts: Vec<&str> = recording
        .events
        .iter()
        .filter_map(|event| match event {
            Event::RefinementConstructed {
                failure_excerpt, ..
            } => Some(failure_excerpt.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        excerpts.len(),
        1,
        "a run of two iterations at a ceiling of two constructs one refinement"
    );

    let excerpt = excerpts[0];
    for carried in [failing_stdout(), failing_stderr()] {
        assert!(
            excerpt.contains(&carried),
            "the refinement prompt does not carry the validator's captured output verbatim; it is \
             missing {carried:?} and holds {excerpt:?}"
        );
    }
    assert!(
        excerpt.contains("test:"),
        "the loop names the validator whose output this is: {excerpt:?}"
    );
    assert!(
        !excerpt.contains("build") && !excerpt.contains("lint"),
        "a passing and a skipped validator contribute nothing to the failure text: {excerpt:?}"
    );
}

impl StagedRunner {
    /// The commands this runner was asked for, in order.
    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("asked poisoned").clone()
    }
}

/// A redactor holding nothing, which is therefore the identity.
///
/// Every outside caller has to supply one, because a `Prompt` can only be
/// built from text that has passed [ADR-0008] clause 6's port — which is the
/// whole point of that type. It is declared here rather than shared because
/// an integration test cannot see another crate's test tree and [ADR-0003] D8
/// forbids the dependency that would let it, the same cost `zaru-cli`'s
/// Nuclear Notes fixture server already pays.
///
/// Holding nothing is also the **discriminating** arm: a check asserting that
/// a value is absent from a prompt is worthless unless the same run with
/// nothing held carries that value through byte for byte.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}
