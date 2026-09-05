// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the loop is asserted to do.
//!
//! Two rules govern the numbers in this file, both of them written against a
//! failure shape the testing contract names.
//!
//! **Counts are asserted against what the fixture was staged with**, never
//! against another reading taken through the loop. The event stream's
//! iteration count and the returned outcome's iteration count come from one
//! counter, so they agree even when that counter is wrong; the only arm worth
//! having is the number the test itself chose.
//!
//! **Failure text carries a nonce, a newline and a non-ASCII character**, so
//! that no implementation can produce it without carrying it. A fixture whose
//! failure text was a plausible constant would let a hard-coded prompt pass
//! the one assertion that makes iteration different from retry.

use super::fixtures::{
    ManualClock, PassThroughContext, Plan, ProjectingSink, RecordingSink, STAGED_NEEDED,
    STAGED_WINDOW, StagedExecutor, StagedGenerator, StagedValidators, Trace, TraceEntry,
    TracingSink, tag,
};
// `run` is reached here through its own module rather than through the
// crate's re-export, so the re-export's only consumer is the integration test
// that drives the loop from outside. That is what leaves
// `tests/headless_loop.rs` guarding reachability on its own: were these
// imports to go through the re-export, removing it would redden nineteen
// checks and the one that is actually about it would be lost in them.
use crate::iteration::machine::run;
use crate::iteration::{
    Ceiling, Event, EventSink, ExhaustionReason, IterationError, Limits, Outcome, PortKind, Ports,
    State, TruncationBudget,
};
use crate::redaction::Redactor;
use crate::redaction::fixtures::{HoldingOne, NothingHeld, ascii_core};
use core::time::Duration;
use std::sync::Arc;

const TASK: &str = "make the tests pass";

const GENERATE_COST: Duration = Duration::from_millis(7);
const EXECUTE_COST: Duration = Duration::from_millis(30);
const EVALUATE_COST: Duration = Duration::from_millis(5);
/// What one whole iteration costs in the staged clock: 7 + 30 + 5.
const ITERATION_COST: Duration = Duration::from_millis(42);

/// A budget large enough that nothing in these fixtures is truncated.
const ROOMY: usize = 4096;

fn limits(ceiling: u32, budget: usize) -> Limits {
    Limits {
        ceiling: Ceiling::new(ceiling).expect("ceiling"),
        budget: TruncationBudget::new(budget).expect("budget"),
    }
}

/// Everything one run needs, staged.
struct Rig {
    clock: Arc<ManualClock>,
    trace: Arc<Trace>,
    generator: StagedGenerator,
    executor: StagedExecutor,
    validators: StagedValidators,
    context: PassThroughContext,
    /// ADR-0008 clause 6's port. `NothingHeld` unless a check stages a
    /// secret, so that every existing check drives the loop with the port
    /// present and the identity behind it -- which is the arm that
    /// discriminates a redactor from one that erases everything.
    redactor: Box<dyn Redactor>,
}

impl Rig {
    fn new(plans: Vec<Plan>) -> Self {
        let clock = Arc::new(ManualClock::default());
        let trace = Arc::new(Trace::default());
        Self {
            generator: StagedGenerator::new(&clock, GENERATE_COST),
            executor: StagedExecutor::new(&clock, EXECUTE_COST),
            validators: StagedValidators::new(&clock, EVALUATE_COST, plans),
            context: PassThroughContext::new(&trace),
            redactor: Box::new(NothingHeld),
            clock,
            trace,
        }
    }

    /// Stage a redactor that holds `value` under `alias`.
    fn holding(mut self, value: impl Into<String>, alias: &str) -> Self {
        self.redactor = Box::new(HoldingOne::new(value, alias));
        self
    }

    async fn drive(
        &self,
        limits: Limits,
        sinks: &mut [&mut dyn EventSink],
    ) -> Result<Outcome, IterationError> {
        run(
            TASK,
            limits,
            Ports {
                generator: &self.generator,
                executor: &self.executor,
                validators: &self.validators,
                context: &self.context,
                clock: self.clock.as_ref(),
                redactor: self.redactor.as_ref(),
            },
            sinks,
        )
        .await
    }

    /// Drive the loop and keep the events.
    async fn record(&self, limits: Limits) -> (Result<Outcome, IterationError>, Vec<Event>) {
        let mut sink = RecordingSink::default();
        let outcome = self.drive(limits, &mut [&mut sink]).await;
        (outcome, sink.events)
    }
}

/// The states an event stream passed through, with consecutive repeats
/// collapsed. The mapping is the loop's own, so a state the loop stops
/// entering disappears from here.
fn states(events: &[Event]) -> Vec<State> {
    let mut out: Vec<State> = Vec::new();
    for event in events {
        let state = event.state();
        if out.last() != Some(&state) {
            out.push(state);
        }
    }
    out
}

fn tags(events: &[Event]) -> Vec<&'static str> {
    events.iter().map(tag).collect()
}

// --- D1: the states and the transitions ------------------------------------

#[tokio::test]
async fn succeeds_on_the_first_passing_evaluation() {
    let rig = Rig::new(vec![Plan::Pass]);
    let (outcome, events) = rig.record(limits(3, ROOMY)).await;

    assert_eq!(
        outcome,
        Ok(Outcome::Succeeded {
            iterations: 1,
            total_elapsed: ITERATION_COST,
        }),
        "a passing evaluation on the first iteration should finish the loop"
    );
    assert_eq!(
        states(&events),
        vec![
            State::Generate,
            State::Execute,
            State::Evaluate,
            State::Succeeded
        ],
        "a run that never fails must not enter Refine"
    );
}

#[tokio::test]
async fn a_failing_evaluation_below_the_ceiling_enters_refine_and_regenerates() {
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let (outcome, events) = rig.record(limits(3, ROOMY)).await;

    assert_eq!(
        states(&events),
        vec![
            State::Generate,
            State::Execute,
            State::Evaluate,
            State::Refine,
            State::Generate,
            State::Execute,
            State::Evaluate,
            State::Succeeded,
        ],
        "ADR-0008 D1 makes Refine a distinct state, not a branch back to Generate"
    );
    assert_eq!(
        outcome,
        Ok(Outcome::Succeeded {
            iterations: 2,
            total_elapsed: ITERATION_COST * 2,
        })
    );
    assert_eq!(
        rig.executor.candidates(),
        vec![
            StagedGenerator::candidate_for(1),
            StagedGenerator::candidate_for(2)
        ],
        "the second iteration must execute a newly generated candidate"
    );
}

#[tokio::test]
async fn every_state_the_enum_declares_is_visited_across_the_two_runs() {
    // Exhaustive over `State`: a seventh state fails to compile here, which is
    // the signal to add it to `State::ALL` and to stage a run that reaches it.
    const fn accounted_for(state: State) -> bool {
        match state {
            State::Generate
            | State::Execute
            | State::Evaluate
            | State::Refine
            | State::Succeeded
            | State::Exhausted => true,
        }
    }
    assert!(State::ALL.iter().copied().all(accounted_for));

    let (_, succeeding) = Rig::new(vec![Plan::Fail, Plan::Pass])
        .record(limits(3, ROOMY))
        .await;
    let (_, exhausting) = Rig::new(vec![Plan::Fail]).record(limits(2, ROOMY)).await;

    let mut visited: Vec<State> = states(&succeeding);
    visited.extend(states(&exhausting));

    let missing: Vec<State> = State::ALL
        .iter()
        .copied()
        .filter(|state| !visited.contains(state))
        .collect();
    assert!(
        missing.is_empty(),
        "these declared states were never entered by either staged run: {missing:?}"
    );
}

// --- D4: failure text is data ---------------------------------------------

#[tokio::test]
async fn validator_failure_text_reaches_the_refinement_prompt_byte_for_byte() {
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let (outcome, _) = rig.record(limits(3, ROOMY)).await;
    outcome.expect("the staged run should reach an outcome");

    let prompts = rig.generator.prompts();
    assert_eq!(
        prompts.len(),
        2,
        "the fixture stages exactly two generations"
    );

    let carried = StagedValidators::failure_detail_for(1);
    assert!(
        prompts[1].contains(&carried),
        "the refinement prompt does not carry the validator's failure text verbatim.\n\
         looked for: {carried:?}\n\
         prompt was: {:?}",
        prompts[1]
    );
}

// --- ADR-0008 trigger clause 6, decided 2026-09-05 -------------------------

/// Everything one of the two clause-6 checks below asserts about one prompt.
///
/// A helper rather than two copies, because the two differ only in which
/// captured stream the held value came out of, and a check whose body is
/// copied is a check whose two copies diverge.
fn assert_redacted(prompt: &str, held: &str, what: &str) {
    let core = ascii_core(held);
    assert!(
        !core.is_empty() && core != held,
        "{what}: the staged value must have an ASCII core distinct from \
         itself, or the escaped-form arm asserts nothing: {held:?}"
    );
    assert!(
        !prompt.contains(held),
        "{what}: a held value reached the model's prompt: {prompt:?}"
    );
    assert!(
        !prompt.contains(core),
        "{what}: a held value's ASCII core reached the model's prompt, so an \
         escaping renderer would publish it: {prompt:?}"
    );
    assert!(
        prompt.contains("<redacted: work>"),
        "{what}: nothing marks where the value was, and a redactor that \
         erased its whole input would satisfy the two assertions above on \
         its own: {prompt:?}"
    );
}

#[tokio::test]
async fn a_held_secret_in_validator_output_is_absent_from_the_next_prompt() {
    // The path ADR-0008 D4 describes, driven end to end and read out of the
    // *generator* -- one layer beyond the policy that built the prompt, so
    // neither arm of the comparison travels back through the code under test.
    //
    // The discriminating arm is not staged here: it is
    // `validator_failure_text_reaches_the_refinement_prompt_byte_for_byte`
    // above, which drives the same rig with nothing held and asserts these
    // exact bytes are present. Both must pass, and only a real redaction
    // makes that possible.
    let held = StagedValidators::failure_detail_for(1);
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]).holding(held.clone(), "work");
    let (outcome, _) = rig.record(limits(3, ROOMY)).await;
    outcome.expect("the staged run should reach an outcome");

    let prompts = rig.generator.prompts();
    assert_eq!(
        prompts.len(),
        2,
        "the fixture stages exactly two generations"
    );
    assert_redacted(&prompts[1], &held, "the validator's failure text");
}

#[tokio::test]
async fn a_held_secret_in_the_executions_stdout_is_absent_from_the_next_prompt() {
    // The finding this arc raised and the coordinator ruled in on
    // 2026-09-05: `construct` embeds the execution's two streams as well as
    // the failure text, and the identity seam that was replaced sat on the
    // failure text alone. A secret printed by the candidate's own execution
    // reached the prompt through a part nothing was documented as covering.
    let held = StagedExecutor::stdout_for(1);
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]).holding(held.clone(), "work");
    let (outcome, _) = rig.record(limits(3, ROOMY)).await;
    outcome.expect("the staged run should reach an outcome");

    let prompts = rig.generator.prompts();
    assert_redacted(&prompts[1], &held, "the execution's stdout");

    // The discriminating arm, staged here because no existing check asserts
    // the stdout's presence: the same run with nothing held must carry it.
    let carried = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let (outcome, _) = carried.record(limits(3, ROOMY)).await;
    outcome.expect("the staged run should reach an outcome");
    assert!(
        carried.generator.prompts()[1].contains(&held),
        "with nothing held the execution's stdout must reach the prompt \
         unaltered, or the assertions above are not about redaction: {:?}",
        carried.generator.prompts()[1]
    );
}

// --- D5: exhaustion, and the ceiling the caller passes ---------------------

#[tokio::test]
async fn exhaustion_returns_ok_and_carries_the_last_failure_not_the_first() {
    let rig = Rig::new(vec![Plan::Fail]);
    let (outcome, events) = rig.record(limits(3, ROOMY)).await;

    assert_eq!(
        outcome,
        Ok(Outcome::Exhausted {
            iterations: 3,
            reason: ExhaustionReason::CeilingReached,
            last_failure: Some(StagedValidators::failure_text_for(3)),
        }),
        "exhaustion is neither an error nor a success, and it carries the LAST failure"
    );
    assert!(
        !tags(&events).contains(&"LoopSucceeded"),
        "an exhausted loop must not also report success"
    );
    // The ceiling is checked on the way out of Evaluate, so the final failing
    // iteration builds no refinement for a candidate nobody will generate.
    assert_eq!(
        tags(&events)
            .iter()
            .filter(|t| **t == "RefinementConstructed")
            .count(),
        2,
        "three iterations at a ceiling of three should construct two refinements"
    );
}

#[tokio::test]
async fn the_ceiling_comes_from_the_caller_and_nothing_else() {
    for staged in [1_u32, 2, 5] {
        let rig = Rig::new(vec![Plan::Fail]);
        let (outcome, events) = rig.record(limits(staged, ROOMY)).await;

        assert_eq!(
            outcome,
            Ok(Outcome::Exhausted {
                iterations: staged,
                reason: ExhaustionReason::CeilingReached,
                last_failure: Some(StagedValidators::failure_text_for(staged)),
            }),
            "a ceiling of {staged} should run exactly {staged} iterations"
        );
        let started: Vec<u32> = events
            .iter()
            .filter_map(|event| match event {
                Event::IterationStarted { of, .. } => Some(*of),
                _ => None,
            })
            .collect();
        assert_eq!(
            started,
            vec![staged; staged as usize],
            "every IterationStarted should report the ceiling the caller passed"
        );
    }
}

// --- ADR-0013 D7: the second route into Exhausted --------------------------

#[tokio::test]
async fn a_context_window_exceedance_is_exhaustion_and_carries_the_numbers_it_was_given() {
    // Staged: the policy refuses the third assembly. Two iterations reach an
    // evaluation and fail; the third never generates. The two numbers come
    // from the fixture and travel nowhere else, so an assertion on them is an
    // assertion that the loop carried them rather than that it recomputed
    // something from its own state.
    let rig = {
        let mut rig = Rig::new(vec![Plan::Fail]);
        rig.context = PassThroughContext::new(&rig.trace).exceeding_on(3);
        rig
    };
    let (outcome, events) = rig.record(limits(9, ROOMY)).await;

    assert_eq!(
        outcome,
        Ok(Outcome::Exhausted {
            iterations: 2,
            reason: ExhaustionReason::ContextWindowExceeded {
                needed: STAGED_NEEDED,
                window: STAGED_WINDOW,
            },
            last_failure: Some(StagedValidators::failure_text_for(2)),
        }),
        "ADR-0013 D7: an iteration that would exceed the window fails as exhausted with a clear \
         reason. Two of the nine permitted iterations reached an evaluation, the third could not \
         assemble, and the ceiling was nowhere near"
    );
    assert!(
        matches!(
            events.last(),
            Some(Event::LoopExhausted {
                iterations: 2,
                reason: ExhaustionReason::ContextWindowExceeded { .. },
                last_failure: Some(_),
            })
        ),
        "the event stream must carry the same outcome the return value does, and its last event \
         was {:?}",
        events.last()
    );
    assert_eq!(
        tags(&events)
            .iter()
            .filter(|t| **t == "CandidateGenerated")
            .count(),
        2,
        "the refused iteration must not generate: the refusal happens before the generator is \
         reached, and a third candidate would mean the loop assembled anyway"
    );
}

#[tokio::test]
async fn an_exceedance_before_any_evaluation_reports_no_iterations_and_no_failure() {
    // The first assembly is refused, so nothing has been validated and there
    // is no failure text in existence. `Some(String::new())` here would be a
    // claim that the validators ran and said nothing.
    let rig = {
        let mut rig = Rig::new(vec![Plan::Fail]);
        rig.context = PassThroughContext::new(&rig.trace).exceeding_on(1);
        rig
    };
    let (outcome, events) = rig.record(limits(5, ROOMY)).await;

    assert_eq!(
        outcome,
        Ok(Outcome::Exhausted {
            iterations: 0,
            reason: ExhaustionReason::ContextWindowExceeded {
                needed: STAGED_NEEDED,
                window: STAGED_WINDOW,
            },
            last_failure: None,
        }),
        "no iteration reached an evaluation, so none ran and none produced a failure"
    );
    assert_eq!(
        tags(&events),
        vec!["IterationStarted", "LoopExhausted"],
        "the iteration began and then could not proceed; nothing else happened"
    );
}

// --- D3: the event stream --------------------------------------------------

#[tokio::test]
async fn two_dissimilar_subscribers_receive_the_same_events_from_one_emission() {
    // Staged: two failing iterations then a passing one. A failing iteration
    // emits IterationStarted, CandidateGenerated, ExecutionCompleted, three
    // ValidatorEvaluated, IterationFailed and RefinementConstructed — eight.
    // A passing one emits IterationStarted, CandidateGenerated,
    // ExecutionCompleted, two ValidatorEvaluated and LoopSucceeded — six.
    const STAGED_EVENTS: usize = 8 + 8 + 6;

    let rig = Rig::new(vec![Plan::Fail, Plan::Fail, Plan::Pass]);
    let mut recording = RecordingSink::default();
    let mut projecting = ProjectingSink::default();
    rig.drive(limits(5, ROOMY), &mut [&mut recording, &mut projecting])
        .await
        .expect("the staged run should reach an outcome");

    assert_eq!(
        recording.events.len(),
        STAGED_EVENTS,
        "the staging says how many events there should be; the loop agreeing with itself does not"
    );
    assert_eq!(
        projecting.lines.len(),
        STAGED_EVENTS,
        "the second subscriber received a different number of events from the first"
    );
    for (index, event) in recording.events.iter().enumerate() {
        assert!(
            projecting.lines[index].starts_with(tag(event)),
            "at index {index} the two subscribers disagree: {:?} against {:?}",
            tag(event),
            projecting.lines[index]
        );
    }
}

#[tokio::test]
async fn no_event_is_emitted_after_a_terminal_outcome() {
    for (plans, ceiling, terminal) in [
        (vec![Plan::Fail, Plan::Pass], 3_u32, "LoopSucceeded"),
        (vec![Plan::Fail], 2, "LoopExhausted"),
    ] {
        let (_, events) = Rig::new(plans).record(limits(ceiling, ROOMY)).await;
        let tags = tags(&events);
        assert_eq!(
            tags.last().copied(),
            Some(terminal),
            "the stream should end at {terminal}, and ended: {tags:?}"
        );
        assert_eq!(
            tags.iter()
                .filter(|t| **t == "LoopSucceeded" || **t == "LoopExhausted")
                .count(),
            1,
            "exactly one terminal event per run: {tags:?}"
        );
    }
}

#[tokio::test]
async fn validator_names_and_outcomes_reach_the_stream_one_event_each() {
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let (_, events) = rig.record(limits(3, ROOMY)).await;

    let evaluated: Vec<(String, crate::iteration::ValidatorOutcome)> = events
        .iter()
        .filter_map(|event| match event {
            Event::ValidatorEvaluated { name, outcome, .. } => Some((name.clone(), *outcome)),
            _ => None,
        })
        .collect();

    use crate::iteration::ValidatorOutcome::{Failed, Passed, Skipped};
    assert_eq!(
        evaluated,
        vec![
            ("build".to_owned(), Passed),
            ("test".to_owned(), Failed),
            ("lint".to_owned(), Skipped),
            ("build".to_owned(), Passed),
            ("test".to_owned(), Passed),
        ],
        "the fixture stages three validators then two; each gets its own event, \
         and a skipped one is not reported as a pass"
    );
}

// --- D6: elapsed time ------------------------------------------------------

#[tokio::test]
async fn elapsed_comes_from_the_callers_clock() {
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let (outcome, events) = rig.record(limits(3, ROOMY)).await;

    for event in &events {
        match event {
            Event::CandidateGenerated { elapsed, .. } => assert_eq!(
                *elapsed, GENERATE_COST,
                "generation elapsed should be exactly what the staged clock was advanced by"
            ),
            Event::ExecutionCompleted { elapsed, .. } => assert_eq!(*elapsed, EXECUTE_COST),
            Event::IterationFailed { elapsed, .. } => assert_eq!(*elapsed, ITERATION_COST),
            Event::LoopSucceeded {
                elapsed,
                total_elapsed,
                ..
            } => {
                assert_eq!(*elapsed, ITERATION_COST);
                assert_eq!(*total_elapsed, ITERATION_COST * 2);
            }
            _ => {}
        }
    }
    assert_eq!(
        outcome,
        Ok(Outcome::Succeeded {
            iterations: 2,
            total_elapsed: ITERATION_COST * 2,
        })
    );
}

#[tokio::test]
async fn every_iteration_ending_event_carries_its_own_elapsed() {
    let staged_iterations = 3_u32;
    let rig = Rig::new(vec![Plan::Fail]);
    let (_, events) = rig.record(limits(staged_iterations, ROOMY)).await;

    let ending: Vec<Duration> = events
        .iter()
        .filter_map(|event| match event {
            Event::IterationFailed { elapsed, .. } | Event::LoopSucceeded { elapsed, .. } => {
                Some(*elapsed)
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        ending,
        vec![ITERATION_COST; staged_iterations as usize],
        "ADR-0008 D6 puts elapsed time on every iteration, including the ones that failed"
    );
}

// --- Ports: failures, and when the context policy is asked -----------------

#[tokio::test]
async fn a_port_failure_is_an_error_and_neither_a_success_nor_an_exhaustion() {
    for (rig, port, iteration) in [
        (
            {
                let mut rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
                rig.generator = StagedGenerator::new(&rig.clock, GENERATE_COST).failing_on(2);
                rig
            },
            PortKind::Generator,
            2_u32,
        ),
        (
            {
                let mut rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
                rig.executor = StagedExecutor::new(&rig.clock, EXECUTE_COST).failing_on(2);
                rig
            },
            PortKind::Executor,
            2,
        ),
        (
            Rig::new(vec![Plan::Fail, Plan::PortFails]),
            PortKind::Validators,
            2,
        ),
        (
            {
                let mut rig = Rig::new(vec![Plan::Pass]);
                rig.context = PassThroughContext::new(&rig.trace).failing();
                rig
            },
            PortKind::ContextPolicy,
            1,
        ),
    ] {
        let (outcome, events) = rig.record(limits(5, ROOMY)).await;
        match outcome {
            Err(IterationError::Port {
                port: reported,
                iteration: on,
                ..
            }) => {
                assert_eq!(reported, port, "the error names the wrong port");
                assert_eq!(on, iteration, "the error names the wrong iteration");
            }
            other => panic!("a failing {port:?} port should be an error, and was {other:?}"),
        }
        let tags = tags(&events);
        assert!(
            !tags.contains(&"LoopSucceeded") && !tags.contains(&"LoopExhausted"),
            "a port failure must not also report an outcome: {tags:?}"
        );
    }
}

#[tokio::test]
async fn the_context_policy_is_called_only_at_an_iteration_boundary() {
    let rig = Rig::new(vec![Plan::Fail, Plan::Pass]);
    let mut sink = TracingSink(Arc::clone(&rig.trace));
    rig.drive(limits(3, ROOMY), &mut [&mut sink])
        .await
        .expect("the staged run should reach an outcome");

    // Both clauses accumulate rather than returning on the first. An earlier
    // clause that returns makes every later clause unwatchable by the same
    // mutation, and the check then ships having been seen to fail on only one
    // of its arms.
    let entries = rig.trace.entries();
    let mut assemblies = 0_usize;
    let mut misplaced: Vec<Option<TraceEntry>> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if *entry == TraceEntry::ContextAssembled {
            assemblies += 1;
            let preceded_by = index.checked_sub(1).and_then(|i| entries.get(i)).cloned();
            if preceded_by != Some(TraceEntry::Event("IterationStarted")) {
                misplaced.push(preceded_by);
            }
        }
    }

    assert!(
        misplaced.is_empty(),
        "ADR-0013 D7: the context policy is invoked at an iteration boundary and never between \
         generate and evaluate. {} of {assemblies} invocations followed something else: \
         {misplaced:?}",
        misplaced.len()
    );
    assert_eq!(
        assemblies, 2,
        "the fixture stages two iterations, so the policy is asked twice"
    );
}
