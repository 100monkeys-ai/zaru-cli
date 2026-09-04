// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives the loop from outside the crate, through the door a caller uses.
//!
//! Every other check on this loop lives inside `zaru-core` and reaches its
//! subject directly. That proves the mechanism and says nothing about whether
//! the mechanism can be reached: a capability whose only callers are unit
//! tests is a capability nobody has been shown able to use, and the missing
//! piece is invisible to a green suite because there is no mutant for a
//! declaration that was never made public.
//!
//! So this file implements the five ports using only what `zaru-core`
//! exports, with no terminal and nothing that touches a network — which is
//! also ADR-0008 D2's headless requirement stated as something a stranger can
//! reproduce.

use core::time::Duration;
use std::sync::Mutex;
use zaru_core::iteration::{
    Ceiling, Clock, ContextPolicy, Event, EventSink, ExecutionOutcome, Executor, Generated,
    Generator, Limits, Outcome, PortFailure, Ports, Prompt, State, TruncationBudget, Turn,
    ValidatorOutcome, ValidatorReport, Validators, run,
};

struct FrozenClock;

impl Clock for FrozenClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

struct Echo;

impl ContextPolicy for Echo {
    fn assemble(
        &self,
        turn: &Turn<'_>,
    ) -> impl Future<Output = Result<Prompt, PortFailure>> + Send {
        let text = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        async move { Ok(Prompt::new(text)) }
    }
}

struct Counting(Mutex<u32>);

impl Counting {
    fn bump(&self) -> u32 {
        let mut n = self.0.lock().expect("counter poisoned");
        *n += 1;
        *n
    }
}

impl Generator for Counting {
    type Candidate = String;

    async fn generate(&self, _prompt: &Prompt) -> Result<Generated<Self::Candidate>, PortFailure> {
        Ok(Generated {
            candidate: format!("candidate {}", self.bump()),
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

/// Fails once, then passes.
struct FailsOnce(Mutex<u32>);

impl Validators for FailsOnce {
    async fn evaluate(
        &self,
        _execution: &ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        let n = {
            let mut calls = self.0.lock().expect("counter poisoned");
            *calls += 1;
            *calls
        };
        let outcome = if n == 1 {
            ValidatorOutcome::Failed
        } else {
            ValidatorOutcome::Passed
        };
        Ok(vec![ValidatorReport {
            name: "test".to_owned(),
            outcome,
            detail: "one failure, carried verbatim".to_owned(),
        }])
    }
}

#[derive(Default)]
struct Collect(Vec<Event>);

impl EventSink for Collect {
    fn emit(&mut self, event: &Event) {
        self.0.push(event.clone());
    }
}

#[tokio::test]
async fn a_caller_outside_this_crate_can_drive_the_loop_to_an_outcome() {
    let generator = Counting(Mutex::new(0));
    let validators = FailsOnce(Mutex::new(0));
    let mut events = Collect::default();

    let outcome = run(
        "make the tests pass",
        Limits {
            ceiling: Ceiling::new(4).expect("a ceiling of four is usable"),
            budget: TruncationBudget::new(512).expect("a budget of 512 bytes is usable"),
        },
        Ports {
            generator: &generator,
            executor: &Inert,
            validators: &validators,
            context: &Echo,
            clock: &FrozenClock,
        },
        &mut [&mut events],
    )
    .await
    .expect("no port fails in this run, so the loop should reach an outcome");

    assert_eq!(
        outcome,
        Outcome::Succeeded {
            iterations: 2,
            total_elapsed: Duration::ZERO,
        },
        "one failing evaluation then a passing one is two iterations"
    );
    assert!(
        events.0.iter().any(|event| event.state() == State::Refine),
        "a run with a failure in it must pass through Refine, and this one's states were: {:?}",
        events.0.iter().map(Event::state).collect::<Vec<_>>()
    );
}
