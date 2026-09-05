// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The outer cycle, and the driver that walks it.
//!
//! [ADR-0008] D1: "the model requests a tool, the harness executes it, the
//! result returns, the model continues."
//!
//! ```text
//!                   ┌──────────────── results ─────────────────┐
//!                   ▼                                          │
//! assemble ──▶ ask the model ──┬──▶ text     ──▶ Answered      │
//!  (once)          ▲           ├──▶ stop     ──▶ Stopped       │
//!                  │           └──▶ calls    ──▶ execute ──────┘
//!                  └── ceiling not reached ───────────────┘
//! ```
//!
//! # Assembly happens once, at the top
//!
//! [ADR-0013] D7 confines context assembly and compaction to turn boundaries.
//! A turn is one call to [`run`], so the policy is invoked once, before the
//! first exchange, and never again inside the turn. What accumulates within
//! the turn is [`ToolResult`]s, carried on the request rather than folded
//! back into the context — which is both D7's rule and the wire shape every
//! provider already has.
//!
//! # Where the inner loop attaches
//!
//! [ADR-0009] D4: "A project with no `zaru.toml` runs the tool-call loop
//! only." So the inner loop arrives as an `Option`, the branch is taken at
//! the turn boundary before anything else happens, and a caller with no
//! declared validators supplies `None` — at which point the code that would
//! run an iteration is not merely skipped, it has nothing to call.
//!
//! **ADR-0008 D1 says the loops are "nested" and says nothing more.** Which
//! point inside a turn the inner loop is entered at is a reading, and this is
//! the reading built: the turn boundary. A proposed Update on that record
//! carries it and settles nothing.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

use crate::iteration::port::{Clock, ContextPolicy, ContextRefusal, Interruption, Turn};
use crate::redaction::Redactor;
use crate::tool_call::error::{PortKind, ToolCallError};
use crate::tool_call::event::{Event, EventSink, TurnEnding};
use crate::tool_call::limits::ToolCallCeiling;
use crate::tool_call::port::{
    InnerLoop, Model, ModelRequest, ModelResponse, Ports, ToolCalling, ToolExecutor, ToolOutcome,
    ToolResult,
};
use core::time::Duration;

/// What a turn is about.
#[derive(Debug)]
pub enum Start<'a> {
    /// An ordinary turn on the caller's task.
    Task(&'a str),
    /// The first turn of a resumed session.
    ///
    /// [ADR-0010] D4's "the model is told it did not complete". It carries no
    /// task, because a resumed session is not a new instruction: the work and
    /// the conversation are what the policy restored, and the one thing the
    /// policy could not know is that a call never finished.
    ///
    /// **A resumed turn never enters the iteration loop**, whatever the
    /// caller supplied, because there is no task to iterate on. The check
    /// `a_resumed_turn_tells_the_model_and_iterates_nothing` asserts both
    /// halves.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    Resumed(&'a Interruption),
}

/// How a turn finished.
///
/// [ADR-0008] D5's rule for the inner loop, applied to this one: a turn that
/// ran out of exchanges is neither an error nor a success, and is reported as
/// itself.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The model answered the user.
    Answered {
        /// What it said.
        text: String,
        /// How many exchanges it took.
        rounds: u32,
        /// Tokens the provider reported across the whole turn. ADR-0012 D7.
        tokens: u64,
    },
    /// The model stopped without answering and without asking for a tool.
    Stopped {
        /// Why, in the provider's own words.
        reason: String,
        /// How many exchanges it took.
        rounds: u32,
        /// Tokens across the whole turn.
        tokens: u64,
    },
    /// The ceiling was reached with the model still asking for tools.
    Exhausted {
        /// How many exchanges ran, which is the ceiling.
        rounds: u32,
        /// How many tool calls were executed across them.
        calls: u32,
        /// Tokens across the whole turn.
        tokens: u64,
    },
    /// The iteration loop ran as this turn's body.
    Iterated(crate::iteration::Outcome),
}

/// Run one turn of the tool-call loop.
///
/// `n` is the turn's position in the session and is the caller's, because a
/// session spans many calls to this function and a number invented here would
/// restart at one every turn.
///
/// `tool_calling` is proof the model was asked whether it can call tools —
/// see [`ToolCalling`]. A model that cannot cannot reach this function.
///
/// `inner` is [ADR-0009] D4's branch: `Some` where a project declares
/// validators, `None` where it does not.
///
/// # Errors
///
/// [`ToolCallError::Port`] when a port fails. Neither a refusal nor a reached
/// ceiling is a port failure — see [`crate::tool_call::error`].
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub async fn run<M, X, P, K, R, I>(
    n: u32,
    start: Start<'_>,
    ceiling: ToolCallCeiling,
    tool_calling: ToolCalling,
    ports: Ports<'_, M, X, P, K, R>,
    inner: Option<&I>,
    sinks: &mut [&mut dyn EventSink],
) -> Result<Outcome, ToolCallError>
where
    M: Model,
    X: ToolExecutor,
    P: ContextPolicy,
    K: Clock,
    R: Redactor + ?Sized,
    I: InnerLoop,
{
    // Taken by value and deliberately unused past this line. It is evidence
    // that `ToolCalling::required` was called, which is the whole of
    // ADR-0012 clause 3's "not mid-loop": there is no capability check here
    // to forget, because the check already happened or this call does not
    // compile.
    let ToolCalling { .. } = tool_calling;

    let turn_started = ports.clock.now();
    emit(
        sinks,
        &Event::TurnStarted {
            n,
            of: ceiling.get(),
        },
    );

    // --- ADR-0009 D4's branch, at the turn boundary ----------------------
    //
    // Taken before anything else, so a project with declared validators
    // never asks a model in this loop and a project without never reaches
    // the iteration loop. A resumed turn is not a task and takes the
    // right-hand path whatever the caller supplied.
    if let (Some(inner), Start::Task(task)) = (inner, &start) {
        let outcome = inner
            .iterate(task)
            .await
            .map_err(|failure| ToolCallError::Port {
                port: PortKind::InnerLoop,
                round: 0,
                failure,
            })?;
        let succeeded = matches!(outcome, crate::iteration::Outcome::Succeeded { .. });
        let iterations = match &outcome {
            crate::iteration::Outcome::Succeeded { iterations, .. }
            | crate::iteration::Outcome::Exhausted { iterations, .. } => *iterations,
        };
        emit(
            sinks,
            &Event::TurnEnded {
                n,
                ending: TurnEnding::Iterated {
                    iterations,
                    succeeded,
                },
                rounds: 0,
                elapsed: ports.clock.now() - turn_started,
            },
        );
        return Ok(Outcome::Iterated(outcome));
    }

    // --- Assemble, once, at the turn boundary (ADR-0013 D7) --------------
    let turn = match &start {
        Start::Task(task) => Turn::Initial { task },
        Start::Resumed(interrupted) => Turn::Resumed { interrupted },
    };
    let prompt = ports.context.assemble(&turn).await.map_err(|refusal| {
        // ADR-0013 D7's window-pressure route belongs to the *iteration*
        // loop's exhaustion, which this loop does not have. Carrying it out
        // as a port failure would be the honest reading only if the two were
        // the same thing, and they are not — so the refusal's own wording
        // travels and `zaru-cli` classifies it, which is what
        // `iteration::port`'s documentation already says happens to a
        // `PortFailure`. Recorded on ADR-0013 as a question this arc did not
        // answer: what a turn does when its own assembly will not fit.
        ToolCallError::Port {
            port: PortKind::ContextPolicy,
            round: 1,
            failure: match refusal {
                ContextRefusal::Failed(failure) => failure,
                other => crate::iteration::PortFailure::new(other.to_string()),
            },
        }
    })?;

    let descriptors = ports.tools.descriptors().to_vec();
    let mut results: Vec<ToolResult> = Vec::new();
    let mut tokens: u64 = 0;
    let mut calls_executed: u32 = 0;
    let mut round: u32 = 1;

    loop {
        // --- Ask the model ----------------------------------------------
        let before = ports.clock.now();
        let response = ports
            .model
            .respond(&ModelRequest {
                prompt: &prompt,
                tools: &descriptors,
                results: &results,
            })
            .await
            .map_err(|failure| ToolCallError::Port {
                port: PortKind::Model,
                round,
                failure,
            })?;
        tokens += response.tokens().total();
        let calls = match &response {
            ModelResponse::Calls { calls, .. } => calls.len(),
            ModelResponse::Text { .. } | ModelResponse::Stopped { .. } => 0,
        };
        emit(
            sinks,
            &Event::ModelResponded {
                round,
                tokens: response.tokens().total(),
                calls,
                elapsed: ports.clock.now() - before,
            },
        );

        let requests = match response {
            ModelResponse::Text { text, .. } => {
                return Ok(finish(
                    sinks,
                    n,
                    TurnEnding::Answered,
                    round,
                    ports.clock.now() - turn_started,
                    Outcome::Answered {
                        text,
                        rounds: round,
                        tokens,
                    },
                ));
            }
            ModelResponse::Stopped { reason, .. } => {
                return Ok(finish(
                    sinks,
                    n,
                    TurnEnding::Stopped,
                    round,
                    ports.clock.now() - turn_started,
                    Outcome::Stopped {
                        reason,
                        rounds: round,
                        tokens,
                    },
                ));
            }
            ModelResponse::Calls { calls, .. } => calls,
        };

        // --- Execute what it asked for ----------------------------------
        //
        // Results replace rather than append across rounds only in the sense
        // that they accumulate: a provider correlates by id, and every
        // result the turn has produced stays on the request.
        for (index, request) in requests.iter().enumerate() {
            let call = u32::try_from(index + 1).unwrap_or(u32::MAX);
            emit(
                sinks,
                &Event::ToolRequested {
                    round,
                    call,
                    name: request.name.clone(),
                },
            );

            let before = ports.clock.now();
            let outcome =
                ports
                    .tools
                    .execute(request)
                    .await
                    .map_err(|failure| ToolCallError::Port {
                        port: PortKind::Tools,
                        round,
                        failure,
                    })?;
            let elapsed = ports.clock.now() - before;

            let decision = outcome.decision();
            emit(
                sinks,
                &Event::ToolPermissionDecided {
                    round,
                    call,
                    statement: decision.statement.clone(),
                    permitted: decision.permitted,
                },
            );

            match &outcome {
                ToolOutcome::Completed { result, .. } => {
                    calls_executed += 1;
                    emit(
                        sinks,
                        &Event::ToolCompleted {
                            round,
                            call,
                            name: request.name.clone(),
                            failed: result.failed,
                            content_bytes: result.content.len(),
                            elapsed,
                        },
                    );
                }
                ToolOutcome::Refused { because, .. } => {
                    emit(
                        sinks,
                        &Event::ToolRefused {
                            round,
                            call,
                            name: request.name.clone(),
                            because: because.clone(),
                            elapsed,
                        },
                    );
                }
            }

            // The single path out of `ToolOutcome`. A refusal becomes this
            // turn's next content exactly as a completion does, and there is
            // no arm anywhere that turns one into an error.
            results.push(outcome.for_the_model(ports.redactor));
        }

        if round >= ceiling.get() {
            return Ok(finish(
                sinks,
                n,
                TurnEnding::CeilingReached,
                round,
                ports.clock.now() - turn_started,
                Outcome::Exhausted {
                    rounds: round,
                    calls: calls_executed,
                    tokens,
                },
            ));
        }
        round += 1;
    }
}

/// Emit the turn's last event and hand back its outcome.
///
/// One function so that no path can return an outcome without having said so
/// on the stream, and so that the ending on the event and the outcome
/// returned are constructed at one place.
fn finish(
    sinks: &mut [&mut dyn EventSink],
    n: u32,
    ending: TurnEnding,
    rounds: u32,
    elapsed: Duration,
    outcome: Outcome,
) -> Outcome {
    emit(
        sinks,
        &Event::TurnEnded {
            n,
            ending,
            rounds,
            elapsed,
        },
    );
    outcome
}

/// Hand one event to every sink.
///
/// The event is constructed once by the caller and borrowed here, so two
/// consumers cannot be given two different values for one thing that
/// happened — [ADR-0008] D3's one-emission requirement, stated for this
/// stream.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
fn emit(sinks: &mut [&mut dyn EventSink], event: &Event) {
    for sink in sinks.iter_mut() {
        sink.emit(event);
    }
}
