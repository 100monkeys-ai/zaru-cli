// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staged implementations of the five ports, and two sinks.
//!
//! These are the test tree. Nothing here has a counterpart in the product
//! tree, and nothing here reaches a network or a process.
//!
//! Two properties are load-bearing and must survive anybody tidying this file
//! up. **Every failure text carries a nonce, a newline and a non-ASCII
//! character**, so that an implementation which hard-coded a plausible
//! failure string could not produce it — a fixture too well-behaved to
//! separate a right implementation from a wrong one is the second of the
//! three failure shapes the testing contract names. And **the clock only ever
//! moves because a staged port moved it**, so every elapsed time in the event
//! stream is a number the test chose rather than a number the machine
//! produced.

use crate::iteration::event::{Event, EventSink, ValidatorOutcome};
use crate::iteration::port::{
    Clock, ContextPolicy, ExecutionOutcome, Executor, Generated, Generator, PortFailure, Prompt,
    Turn, ValidatorReport, Validators,
};
use core::time::Duration;
use std::sync::{Arc, Mutex};

/// A nonce that no implementation could produce without carrying it.
pub(super) const NONCE: &str = "zaru-nonce-9f2a";

/// A clock that moves only when a test moves it.
#[derive(Debug, Default)]
pub(super) struct ManualClock {
    elapsed: Mutex<Duration>,
}

impl ManualClock {
    pub(super) fn advance(&self, by: Duration) {
        *self.elapsed.lock().expect("manual clock poisoned") += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self.elapsed.lock().expect("manual clock poisoned")
    }
}

/// One thing that happened, from whichever of two unrelated producers saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TraceEntry {
    /// A sink received an event, named by its variant.
    Event(&'static str),
    /// The context policy was asked to assemble a prompt.
    ContextAssembled,
}

/// A log two different producers write to, so their interleaving can be read.
#[derive(Debug, Default)]
pub(super) struct Trace(Mutex<Vec<TraceEntry>>);

impl Trace {
    pub(super) fn push(&self, entry: TraceEntry) {
        self.0.lock().expect("trace poisoned").push(entry);
    }

    pub(super) fn entries(&self) -> Vec<TraceEntry> {
        self.0.lock().expect("trace poisoned").clone()
    }
}

/// The variant name of an event, for the trace and for the projecting sink.
pub(super) const fn tag(event: &Event) -> &'static str {
    match event {
        Event::IterationStarted { .. } => "IterationStarted",
        Event::CandidateGenerated { .. } => "CandidateGenerated",
        Event::ExecutionCompleted { .. } => "ExecutionCompleted",
        Event::ValidatorEvaluated { .. } => "ValidatorEvaluated",
        Event::IterationFailed { .. } => "IterationFailed",
        Event::RefinementConstructed { .. } => "RefinementConstructed",
        Event::LoopSucceeded { .. } => "LoopSucceeded",
        Event::LoopExhausted { .. } => "LoopExhausted",
    }
}

/// A sink that keeps the events themselves.
#[derive(Debug, Default)]
pub(super) struct RecordingSink {
    pub(super) events: Vec<Event>,
}

impl EventSink for RecordingSink {
    fn emit(&mut self, event: &Event) {
        self.events.push(event.clone());
    }
}

/// A sink that keeps a flat line per event and nothing structured.
///
/// Deliberately a different kind of reader from [`RecordingSink`]: it decides
/// for itself what an event says, so two sinks agreeing is two readings rather
/// than one reading stored twice.
#[derive(Debug, Default)]
pub(super) struct ProjectingSink {
    pub(super) lines: Vec<String>,
}

impl EventSink for ProjectingSink {
    fn emit(&mut self, event: &Event) {
        let detail = match event {
            Event::IterationStarted { n, of } => format!("{n}/{of}"),
            Event::CandidateGenerated { tokens, .. } => format!("tokens={tokens}"),
            Event::ExecutionCompleted { exit_code, .. } => format!("exit={exit_code}"),
            Event::ValidatorEvaluated { name, outcome, .. } => format!("{name}={outcome:?}"),
            Event::IterationFailed { n, .. } => format!("n={n}"),
            Event::RefinementConstructed { n, .. } => format!("n={n}"),
            Event::LoopSucceeded { iterations, .. } | Event::LoopExhausted { iterations, .. } => {
                format!("iterations={iterations}")
            }
        };
        self.lines.push(format!("{} {detail}", tag(event)));
    }
}

/// A sink that writes into a shared trace, so its ordering can be compared
/// against another producer's.
#[derive(Debug)]
pub(super) struct TracingSink(pub(super) Arc<Trace>);

impl EventSink for TracingSink {
    fn emit(&mut self, event: &Event) {
        self.0.push(TraceEntry::Event(tag(event)));
    }
}

/// Produces candidates whose text is distinct per iteration.
#[derive(Debug)]
pub(super) struct StagedGenerator {
    clock: Arc<ManualClock>,
    cost: Duration,
    tokens: u64,
    fails_on: Option<u32>,
    calls: Mutex<u32>,
    prompts: Mutex<Vec<String>>,
}

impl StagedGenerator {
    pub(super) fn new(clock: &Arc<ManualClock>, cost: Duration) -> Self {
        Self {
            clock: Arc::clone(clock),
            cost,
            tokens: 11,
            fails_on: None,
            calls: Mutex::new(0),
            prompts: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn failing_on(mut self, iteration: u32) -> Self {
        self.fails_on = Some(iteration);
        self
    }

    /// The prompt text this generator was handed, per call.
    pub(super) fn prompts(&self) -> Vec<String> {
        self.prompts.lock().expect("prompts poisoned").clone()
    }

    /// The candidate text this generator produces for iteration `n`.
    pub(super) fn candidate_for(n: u32) -> String {
        format!("{NONCE}-candidate-{n}")
    }
}

impl Generator for StagedGenerator {
    type Candidate = String;

    async fn generate(&self, prompt: &Prompt) -> Result<Generated<Self::Candidate>, PortFailure> {
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(prompt.as_str().to_owned());
        let n = {
            let mut calls = self.calls.lock().expect("calls poisoned");
            *calls += 1;
            *calls
        };
        self.clock.advance(self.cost);
        if self.fails_on == Some(n) {
            return Err(PortFailure::new(format!("{NONCE} provider unreachable")));
        }
        Ok(Generated {
            candidate: Self::candidate_for(n),
            tokens: self.tokens,
        })
    }
}

/// Makes a candidate's effect "real" by returning staged streams.
#[derive(Debug)]
pub(super) struct StagedExecutor {
    clock: Arc<ManualClock>,
    cost: Duration,
    exit_code: i32,
    fails_on: Option<u32>,
    calls: Mutex<u32>,
    candidates: Mutex<Vec<String>>,
}

impl StagedExecutor {
    pub(super) fn new(clock: &Arc<ManualClock>, cost: Duration) -> Self {
        Self {
            clock: Arc::clone(clock),
            cost,
            exit_code: 1,
            fails_on: None,
            calls: Mutex::new(0),
            candidates: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn failing_on(mut self, iteration: u32) -> Self {
        self.fails_on = Some(iteration);
        self
    }

    /// The candidates this executor was handed, in order.
    pub(super) fn candidates(&self) -> Vec<String> {
        self.candidates.lock().expect("candidates poisoned").clone()
    }

    pub(super) fn stdout_for(n: u32) -> String {
        format!("{NONCE}-stdout-{n}")
    }

    pub(super) fn stderr_for(n: u32) -> String {
        format!("{NONCE}-stderr-{n}")
    }
}

impl Executor for StagedExecutor {
    type Candidate = String;

    async fn execute(&self, candidate: &Self::Candidate) -> Result<ExecutionOutcome, PortFailure> {
        self.candidates
            .lock()
            .expect("candidates poisoned")
            .push(candidate.clone());
        let n = {
            let mut calls = self.calls.lock().expect("calls poisoned");
            *calls += 1;
            *calls
        };
        self.clock.advance(self.cost);
        if self.fails_on == Some(n) {
            return Err(PortFailure::new(format!("{NONCE} sandbox refused")));
        }
        Ok(ExecutionOutcome {
            exit_code: self.exit_code,
            stdout: Self::stdout_for(n),
            stderr: Self::stderr_for(n),
        })
    }
}

/// What the validators report for one iteration.
#[derive(Debug, Clone)]
pub(super) enum Plan {
    /// Every declared validator passes.
    Pass,
    /// `build` passes, `test` fails, `lint` is skipped because it came after.
    Fail,
    /// The port itself fails.
    PortFails,
}

/// Runs the declared validators, from a plan per iteration.
#[derive(Debug)]
pub(super) struct StagedValidators {
    clock: Arc<ManualClock>,
    cost: Duration,
    plans: Vec<Plan>,
    calls: Mutex<u32>,
}

impl StagedValidators {
    /// The last plan repeats once the list is exhausted, so a run at any
    /// ceiling is staged by naming only what differs.
    pub(super) fn new(clock: &Arc<ManualClock>, cost: Duration, plans: Vec<Plan>) -> Self {
        assert!(!plans.is_empty(), "a validators fixture needs a plan");
        Self {
            clock: Arc::clone(clock),
            cost,
            plans,
            calls: Mutex::new(0),
        }
    }

    /// The detail the failing validator produces on iteration `n`.
    ///
    /// Carries the nonce, an embedded newline and a non-ASCII character, so
    /// that text arriving anywhere downstream can only have got there by
    /// being carried.
    pub(super) fn failure_detail_for(n: u32) -> String {
        format!("{NONCE}\niteration {n}: assertion failed — left ≠ right\n")
    }

    /// The whole failure text the loop assembles for iteration `n`.
    pub(super) fn failure_text_for(n: u32) -> String {
        format!("test:\n{}", Self::failure_detail_for(n))
    }
}

impl Validators for StagedValidators {
    async fn evaluate(
        &self,
        _execution: &ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        let n = {
            let mut calls = self.calls.lock().expect("calls poisoned");
            *calls += 1;
            *calls
        };
        self.clock.advance(self.cost);
        let index = (n as usize - 1).min(self.plans.len() - 1);
        match self.plans[index] {
            Plan::Pass => Ok(vec![
                report("build", ValidatorOutcome::Passed, String::new()),
                report("test", ValidatorOutcome::Passed, String::new()),
            ]),
            Plan::Fail => Ok(vec![
                report("build", ValidatorOutcome::Passed, String::new()),
                report(
                    "test",
                    ValidatorOutcome::Failed,
                    Self::failure_detail_for(n),
                ),
                report("lint", ValidatorOutcome::Skipped, String::new()),
            ]),
            Plan::PortFails => Err(PortFailure::new(format!("{NONCE} validators crashed"))),
        }
    }
}

fn report(name: &str, outcome: ValidatorOutcome, detail: String) -> ValidatorReport {
    ValidatorReport {
        name: name.to_owned(),
        outcome,
        detail,
    }
}

/// Assembles the prompt by passing the turn through unchanged, and records
/// that it was asked.
#[derive(Debug)]
pub(super) struct PassThroughContext {
    trace: Arc<Trace>,
    fails: bool,
}

impl PassThroughContext {
    pub(super) fn new(trace: &Arc<Trace>) -> Self {
        Self {
            trace: Arc::clone(trace),
            fails: false,
        }
    }

    pub(super) fn failing(mut self) -> Self {
        self.fails = true;
        self
    }
}

impl ContextPolicy for PassThroughContext {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, PortFailure> {
        let text = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        self.trace.push(TraceEntry::ContextAssembled);
        if self.fails {
            return Err(PortFailure::new(format!("{NONCE} context unavailable")));
        }
        Ok(Prompt::new(text))
    }
}
