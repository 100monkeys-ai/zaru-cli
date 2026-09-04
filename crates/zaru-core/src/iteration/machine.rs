// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The state machine and the driver that walks it.
//!
//! ADR-0008 D1's states, and the transitions between them:
//!
//! ```text
//! Generate ──▶ Execute ──▶ Evaluate ──┬──▶ Succeeded
//!     ▲                               ├──▶ Exhausted
//!     └────────── Refine ◀────────────┘
//! ```
//!
//! `Refine` is a distinct state and not a branch back to `Generate`, because
//! constructing the next prompt from the previous failure is the mechanism
//! that separates an iteration from a retry.
//!
//! `Exhausted` is a terminal state beside `Succeeded`. D1's diagram names
//! only `Succeeded`, but D5 makes exhaustion a distinct outcome that is
//! neither a success nor an error, and a machine with no state for it cannot
//! report it as itself. Recorded as a proposed Update on ADR-0008 D1.
//!
//! The ceiling is checked on the transition out of `Evaluate`, so a failing
//! iteration at the ceiling goes straight to `Exhausted` and no refinement is
//! constructed for a candidate that will never be generated.
//!
//! **Two routes reach `Exhausted`.** The ceiling is one. ADR-0013 D7 is the
//! other: a context policy that would exceed the window refuses, and the
//! refusal is exhaustion rather than an error, because the loop worked and
//! the window did not fit. That route leaves `Generate` before anything is
//! generated, so it is the one transition into a terminal state that does not
//! come out of `Evaluate`.

use crate::iteration::error::{IterationError, PortKind};
use crate::iteration::event::{Event, EventSink, ExhaustionReason, ValidatorOutcome};
use crate::iteration::limits::Limits;
use crate::iteration::port::{
    Clock, ContextPolicy, ContextRefusal, Executor, Generator, Ports, Turn, ValidatorReport,
    Validators,
};
use crate::iteration::refinement::{self, RefinementInput, RefinementPrompt};
use core::time::Duration;

/// A state of the iteration loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A candidate is being produced.
    Generate,
    /// A candidate's effect is being made real.
    Execute,
    /// The declared validators are reporting on the execution.
    Evaluate,
    /// The next prompt is being constructed from this iteration's failure.
    Refine,
    /// Every validator passed. Terminal.
    Succeeded,
    /// The loop stopped without succeeding. Terminal.
    Exhausted,
}

impl State {
    /// Every state.
    ///
    /// A hand-written list, guarded by the exhaustive match in
    /// `every_state_the_enum_declares_is_visited_across_the_two_runs`: adding
    /// a variant to [`State`] fails to compile there, which is the signal to
    /// add it here and to stage a run that reaches it. A list nothing forces
    /// you to revisit is a list that goes stale.
    pub const ALL: [Self; 6] = [
        Self::Generate,
        Self::Execute,
        Self::Evaluate,
        Self::Refine,
        Self::Succeeded,
        Self::Exhausted,
    ];
}

impl Event {
    /// The state the loop was in when it emitted this event.
    ///
    /// ADR-0008 D3 has the loop emit what a consumer needs rather than
    /// letting the consumer reach in, and which state an event belongs to is
    /// exactly the sort of thing a narrative renderer would otherwise
    /// reconstruct from a mapping of its own.
    #[must_use]
    pub const fn state(&self) -> State {
        match self {
            Self::IterationStarted { .. } | Self::CandidateGenerated { .. } => State::Generate,
            Self::ExecutionCompleted { .. } => State::Execute,
            Self::ValidatorEvaluated { .. } | Self::IterationFailed { .. } => State::Evaluate,
            Self::RefinementConstructed { .. } => State::Refine,
            Self::LoopSucceeded { .. } => State::Succeeded,
            Self::LoopExhausted { .. } => State::Exhausted,
        }
    }
}

/// How the loop finished.
///
/// ADR-0008 D5: exhaustion is not an error and is not a success. It is a
/// third thing, and it is reported as itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every validator passed.
    Succeeded {
        /// How many iterations ran.
        iterations: u32,
        /// How long the whole loop took, in the caller's clock.
        total_elapsed: Duration,
    },
    /// The loop stopped without succeeding.
    Exhausted {
        /// How many iterations ran to an evaluation.
        iterations: u32,
        /// Why it stopped.
        reason: ExhaustionReason,
        /// The last iteration's failing validators' output, verbatim, or
        /// `None` when no iteration reached an evaluation.
        last_failure: Option<String>,
    },
}

/// Run the iteration loop to an outcome.
///
/// Headless by construction: nothing here renders, and every effect leaves
/// through a port the caller supplies.
///
/// # Errors
///
/// [`IterationError::Port`] when a port fails. That is not the loop failing —
/// see the module documentation on [`crate::iteration::error`].
pub async fn run<G, X, V, P, K>(
    task: &str,
    limits: Limits,
    ports: Ports<'_, G, X, V, P, K>,
    sinks: &mut [&mut dyn EventSink],
) -> Result<Outcome, IterationError>
where
    G: Generator,
    X: Executor<Candidate = G::Candidate>,
    V: Validators,
    P: ContextPolicy,
    K: Clock,
{
    let ceiling = limits.ceiling.get();
    let loop_started = ports.clock.now();
    let mut refinement: Option<RefinementPrompt> = None;
    // The failure of the last iteration that reached an evaluation. Kept
    // beside the refinement rather than read back out of it, because the
    // refinement carries a *truncated* excerpt and ADR-0008 D5 has the
    // exhausted outcome carry the failure verbatim.
    let mut last_failure: Option<String> = None;
    let mut n: u32 = 1;

    loop {
        emit(sinks, &Event::IterationStarted { n, of: ceiling });
        let iteration_started = ports.clock.now();

        // --- Generate -------------------------------------------------
        //
        // The context policy is invoked here, at the iteration boundary, and
        // nowhere else. ADR-0013 D7 forbids compaction between generate and
        // evaluate: a context rewritten partway through a cycle changes the
        // model's view of what it was doing.
        let turn = match refinement.as_ref() {
            None => Turn::Initial { task },
            Some(built) => Turn::Refinement { refinement: built },
        };
        let prompt = match ports.context.assemble(&turn).await {
            Ok(prompt) => prompt,
            Err(ContextRefusal::Failed(failure)) => {
                return Err(IterationError::Port {
                    port: PortKind::ContextPolicy,
                    iteration: n,
                    failure,
                });
            }
            // ADR-0013 D7's second route. `n` iterations were started and
            // `n - 1` reached an evaluation, so that is the count reported:
            // this iteration never generated anything, and saying it ran
            // would credit the loop with work it did not do.
            Err(ContextRefusal::WindowExceeded { needed, window }) => {
                let reason = ExhaustionReason::ContextWindowExceeded { needed, window };
                let iterations = n - 1;
                emit(
                    sinks,
                    &Event::LoopExhausted {
                        iterations,
                        reason,
                        last_failure: last_failure.clone(),
                    },
                );
                return Ok(Outcome::Exhausted {
                    iterations,
                    reason,
                    last_failure,
                });
            }
        };

        let before = ports.clock.now();
        let generated =
            ports
                .generator
                .generate(&prompt)
                .await
                .map_err(|failure| IterationError::Port {
                    port: PortKind::Generator,
                    iteration: n,
                    failure,
                })?;
        emit(
            sinks,
            &Event::CandidateGenerated {
                tokens: generated.tokens,
                elapsed: ports.clock.now() - before,
            },
        );

        // --- Execute --------------------------------------------------
        let before = ports.clock.now();
        let execution = ports
            .executor
            .execute(&generated.candidate)
            .await
            .map_err(|failure| IterationError::Port {
                port: PortKind::Executor,
                iteration: n,
                failure,
            })?;
        emit(
            sinks,
            &Event::ExecutionCompleted {
                exit_code: execution.exit_code,
                stdout_bytes: execution.stdout.len(),
                stderr_bytes: execution.stderr.len(),
                elapsed: ports.clock.now() - before,
            },
        );

        // --- Evaluate -------------------------------------------------
        let reports = ports
            .validators
            .evaluate(&execution)
            .await
            .map_err(|failure| IterationError::Port {
                port: PortKind::Validators,
                iteration: n,
                failure,
            })?;
        for report in &reports {
            emit(
                sinks,
                &Event::ValidatorEvaluated {
                    name: report.name.clone(),
                    outcome: report.outcome,
                    detail: report.detail.clone(),
                },
            );
        }

        let Some(failure) = failure_text(&reports) else {
            let now = ports.clock.now();
            let total_elapsed = now - loop_started;
            emit(
                sinks,
                &Event::LoopSucceeded {
                    iterations: n,
                    elapsed: now - iteration_started,
                    total_elapsed,
                },
            );
            return Ok(Outcome::Succeeded {
                iterations: n,
                total_elapsed,
            });
        };

        emit(
            sinks,
            &Event::IterationFailed {
                n,
                reason: failure.clone(),
                elapsed: ports.clock.now() - iteration_started,
            },
        );
        last_failure = Some(failure.clone());

        if n >= ceiling {
            emit(
                sinks,
                &Event::LoopExhausted {
                    iterations: n,
                    reason: ExhaustionReason::CeilingReached,
                    last_failure: Some(failure.clone()),
                },
            );
            return Ok(Outcome::Exhausted {
                iterations: n,
                reason: ExhaustionReason::CeilingReached,
                last_failure: Some(failure),
            });
        }

        // --- Refine ---------------------------------------------------
        let built = refinement::construct(
            &RefinementInput {
                iteration: n,
                previous_candidate: generated.candidate.as_ref(),
                execution: &execution,
                failure_text: &failure,
            },
            limits.budget,
        );
        emit(
            sinks,
            &Event::RefinementConstructed {
                n,
                failure_excerpt: built.failure_excerpt().to_owned(),
            },
        );
        refinement = Some(built);
        n += 1;
    }
}

/// Hand one event to every sink.
///
/// The event is constructed once by the caller and borrowed here, so two
/// consumers cannot be given two different values for one thing that happened.
fn emit(sinks: &mut [&mut dyn EventSink], event: &Event) {
    for sink in sinks.iter_mut() {
        sink.emit(event);
    }
}

/// Every failing validator's own output, or `None` if none failed.
///
/// A skipped validator is not a failure: under ADR-0009 D2 it did not run
/// because something it depended on failed, and that something is already
/// here.
fn failure_text(reports: &[ValidatorReport]) -> Option<String> {
    let mut text = String::new();
    for report in reports
        .iter()
        .filter(|report| report.outcome == ValidatorOutcome::Failed)
    {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&report.name);
        text.push_str(":\n");
        text.push_str(&report.detail);
    }
    (!text.is_empty()).then_some(text)
}
