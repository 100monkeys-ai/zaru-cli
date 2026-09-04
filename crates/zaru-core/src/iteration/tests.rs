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
    ManualClock, PassThroughContext, Plan, ProjectingSink, RecordingSink, StagedExecutor,
    StagedGenerator, StagedValidators, Trace, TraceEntry, TracingSink, tag,
};
use crate::iteration::{
    Ceiling, Event, EventSink, ExhaustionReason, IterationError, Limits, Outcome, PortKind, Ports,
    State, TruncationBudget, run,
};
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
            clock,
            trace,
        }
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
            last_failure: StagedValidators::failure_text_for(3),
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
                last_failure: StagedValidators::failure_text_for(staged),
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
