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
//! the turn is the turn's own conversation — the model's messages and the
//! result of every call — carried on the request rather than folded back into
//! the context, which is both D7's rule and the wire shape every provider
//! already has.
//!
//! # Every message is emitted as it is made
//!
//! The person's message, each of the model's messages and each result are
//! emitted as [`Event::Message`] at the moment they join the conversation.
//! That one emission is what the transcript records and what the next turn's
//! history is rebuilt from, so what a later turn is sent is what this turn
//! sent, rather than a second description of it.
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

use crate::conversation::Message;
use crate::iteration::port::{Clock, ContextPolicy, ContextRefusal, Turn};
use crate::redaction::Redactor;
use crate::tool_call::error::{PortKind, ToolCallError};
use crate::tool_call::event::{Event, EventSink, TurnEnding};
use crate::tool_call::limits::ToolCallCeiling;
use crate::tool_call::port::{
    InnerLoop, Model, ModelRequest, ModelResponse, Ports, ToolCalling, ToolExecutor, ToolOutcome,
};
use core::time::Duration;

/// What a turn is about.
///
/// One variant today. It is an enum because what starts a turn is a fact the
/// loop branches on, and a second kind of start is a new arm rather than a
/// flag.
#[derive(Debug)]
pub enum Start<'a> {
    /// An ordinary turn on the caller's task.
    Task(&'a str),
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
            of: ceiling.limit(),
        },
    );

    let Start::Task(task) = start;

    // --- The person's message, first of this turn's conversation ---------
    //
    // Emitted before the branch, so a turn that iterates is recorded as
    // having been asked exactly as one that calls tools is.
    emit(sinks, &Event::Message(Message::user(ports.redactor, task)));

    // --- ADR-0009 D4's branch, at the turn boundary ----------------------
    //
    // Taken before anything else, so a project with declared validators
    // never asks a model in this loop and a project without never reaches
    // the iteration loop.
    if let Some(inner) = inner {
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
    let turn = Turn::Initial { task };
    let prompt = ports
        .context
        .assemble(&turn)
        .await
        .map_err(|refusal| match refusal {
            // A policy that broke is a port failure like any other.
            ContextRefusal::Failed(failure) => ToolCallError::Port {
                port: PortKind::ContextPolicy,
                round: 1,
                failure,
            },
            // ADR-0013 D7's window-pressure route belongs to the *iteration*
            // loop's exhaustion, which this loop does not have, so it travels as
            // itself and `zaru-cli` classifies it. That much was already true;
            // what was not is that it travelled as a **string**. Until 2026-09-15
            // this arm read `PortFailure::new(other.to_string())`, which flattened
            // the two numbers into prose, and the classifier — with nothing left
            // to read but a port kind — reported the reader's own configuration as
            // a defect in the harness. The numbers travel now. Recorded on
            // ADR-0013 as the answer to the question that arc did not answer:
            // what a turn does when its own assembly will not fit.
            ContextRefusal::WindowExceeded { needed, window } => {
                ToolCallError::ContextWindowExceeded { needed, window }
            }
        })?;

    let descriptors = ports.tools.descriptors().to_vec();
    let mut conversation: Vec<Message> = Vec::new();
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
                turn: &conversation,
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
            ModelResponse::Text { text, echo, .. } => {
                emit(
                    sinks,
                    &Event::Message(Message::assistant(ports.redactor, &text, &[], echo)),
                );
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
            ModelResponse::Calls {
                calls, text, echo, ..
            } => {
                let said = Message::assistant(ports.redactor, &text, &calls, echo);
                emit(sinks, &Event::Message(said.clone()));
                conversation.push(said);
                calls
            }
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
                ToolOutcome::Completed { result, view, .. } => {
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
                    // What the person is shown, after the line that says the
                    // call returned and before the result joins the
                    // conversation. It is not a message, so the model is
                    // never sent it.
                    if let Some(view) = view {
                        emit(
                            sinks,
                            &Event::ToolShown {
                                round,
                                call,
                                name: request.name.clone(),
                                view: view.clone(),
                            },
                        );
                    }
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
            let result = Message::result(&request.name, &outcome.for_the_model(ports.redactor));
            emit(sinks, &Event::Message(result.clone()));
            conversation.push(result);
        }

        if ceiling.limit().is_some_and(|limit| round >= limit) {
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
