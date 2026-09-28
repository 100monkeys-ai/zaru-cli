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
//! So this file implements the six ports using only what `zaru-core`
//! exports, with no terminal and nothing that touches a network — which is
//! also ADR-0008 D2's headless requirement stated as something a stranger can
//! reproduce.

use core::time::Duration;
use std::sync::Mutex;
use zaru_core::iteration::{
    Ceiling, Clock, ContextPolicy, ContextRefusal, Event, EventSink, ExecutionOutcome, Executor,
    Generated, Generator, Limits, Outcome, PortFailure, Ports, Prompt, State, TruncationBudget,
    Turn, ValidatorOutcome, ValidatorReport, Validators, run,
};
use zaru_core::redaction::{Redacted, Redactor};

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
    ) -> impl Future<Output = Result<Prompt, ContextRefusal>> + Send {
        let text = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        async move { Ok(Prompt::new(Redacted::by(&NothingHeld, &text))) }
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

/// Take a future only if it is `Send`.
///
/// This is the whole instrument for `EventSink`'s `Send` bound, and it is a
/// **compile-time** one: if the bound is removed, `run`'s future stops being
/// `Send` because it holds `&mut [&mut dyn EventSink]` across every await,
/// this file stops compiling, and the message names the trait and the slice.
/// There is no runtime assertion that could say it instead — a future either
/// is `Send` or the program does not build.
///
/// It is here rather than in the crate's own tests because
/// [`InnerLoop`](zaru_core::tool_call::InnerLoop) is what needs it, and an
/// outside caller is the only thing in this workspace that can be a stand-in
/// for one without depending on `zaru-cli`.
fn only_if_send<F: core::future::Future + Send>(future: F) -> F {
    future
}

#[tokio::test]
async fn a_caller_outside_this_crate_can_drive_the_loop_to_an_outcome() {
    let generator = Counting(Mutex::new(0));
    let validators = FailsOnce(Mutex::new(0));
    let mut events = Collect::default();

    let outcome = only_if_send(run(
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
            redactor: &NothingHeld,
        },
        &mut [&mut events],
    ))
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
