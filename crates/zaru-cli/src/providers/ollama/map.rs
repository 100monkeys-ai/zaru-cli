// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Between `zaru-core`'s ports and Ollama's `/api/chat` shapes.
//!
//! # The frames are folded and the mapping is called once
//!
//! **Measured on 2026-09-14, and it is the reason this module has a fold at
//! all**: a tool call arrives complete in one frame — its id, its name and its
//! finished `arguments` object together, never split — and the `done_reason`
//! that ends the same exchange arrives in a **different** frame. A reader
//! mapping frames as they arrived would therefore answer one question twice,
//! as [`ModelResponse::Calls`] and then [`ModelResponse::Stopped`], and the
//! second answer would win.
//!
//! So [`fold`] reduces every frame to one [`wire::Response`] and
//! [`response_from`] is called once, on the fold. **This is the same shape the
//! `gemini` client reached for its own API**, and it is reached here by
//! measuring this one rather than by copying that one — which is the only way
//! the agreement is worth anything. If Ollama ever splits a call across frames
//! it stops loudly at the `serde` boundary rather than quietly producing
//! truncated arguments.
//!
//! # Text is a delta and usage is not
//!
//! Two facts that pull in opposite directions, both measured across the
//! recorded fixtures. A frame's `content` is a **delta** to be concatenated —
//! six frames spelling "Hello there, friend." — so folding text means
//! appending. A frame's `prompt_eval_count` and `eval_count` appear on the
//! **terminal frame only** and are absent everywhere else, so folding usage
//! means taking the last that carried any.
//!
//! **The `gemini` client takes the last frame's usage too, and for a different
//! reason**: its counts are cumulative on every frame, so summing would
//! multiply the prompt count. Here nothing is cumulative and nothing else
//! carries a count at all. The rule is the same and the justification is not,
//! which is recorded so that this fold does not appear to rest on a property
//! this API does not have.
//!
//! # A client of a stateless API owes it the model's own turns
//!
//! ADR-0012's amendments page records this as the `gemini-read-loop` arc's
//! finding, ending "**whoever writes the second client should expect to keep
//! the same state**". This client keeps it: [`Answered`] holds the assistant
//! turns of the turn now in flight, scoped to one turn and reset by the act of
//! building a request whose `results` are empty.
//!
//! **Measured here rather than inherited, and the measurement came out
//! differently.** Replaying a recorded second round against the live endpoint
//! five times at temperature zero, with the assistant turn **omitted**,
//! answered correctly five times out of five and never re-called the tool — so
//! `llama3.2:3b` does not reproduce the defect that made the Gemini client
//! re-read a file to its ceiling. **The state is kept anyway**, for two
//! reasons that do not depend on that result: the conversation this client
//! sends is otherwise a false record of what happened, with a result appearing
//! for a call the transcript never shows being made; and a measurement over
//! one model at one temperature is not a property of the kind. Keeping it
//! costs one vector and is what the record told the second client to expect.
//!
//! The other half of that defect **does** apply and is built in from the first
//! line: each result is named for the **tool** rather than for the call's id.
//! Ollama's field is `tool_name` and takes a name, so the shape that cost the
//! `gemini` client a defect is not reachable here.

use super::endpoint::NUM_THREAD;
use super::failure::OllamaFailure;
use super::wire;
use serde_json::Value;
use zaru_core::tool_call::{ModelRequest, ModelResponse, TokenUsage, ToolRequest};

/// The role an assistant turn carries.
pub const ROLE_ASSISTANT: &str = "assistant";
/// The role a user turn carries.
pub const ROLE_USER: &str = "user";
/// The role a tool result carries.
pub const ROLE_TOOL: &str = "tool";
/// The `done_reason` of an exchange that ended normally.
pub const DONE_STOP: &str = "stop";
/// The `type` every declared tool carries.
pub const TOOL_FUNCTION: &str = "function";

/// The assistant turns of the turn now in flight.
///
/// See the module documentation for why a provider client that remembers
/// anything is worth announcing, and why this one does.
#[derive(Debug, Default)]
pub struct Answered {
    rounds: Vec<Round>,
}

/// One round of a turn: the assistant message, and the calls inside it.
#[derive(Debug, Clone)]
struct Round {
    /// The assistant turn, as this client parsed it.
    message: wire::Message,
    /// The calls inside it, in the order the model asked.
    calls: Vec<wire::ToolCall>,
}

impl Answered {
    /// Drop everything remembered if this request begins a new turn.
    ///
    /// # The boundary lives here, not at the call site
    ///
    /// [`ModelRequest::results`] is "What the tools returned so far in this
    /// turn" and is "Empty on the first exchange", so an empty one *is* a turn
    /// beginning — it is the only signal the port gives. Reading it inside
    /// [`request_from`] rather than in the client's exchange is what makes the
    /// rule structural: there is no call site that can forget to reset,
    /// because building a request is the reset.
    ///
    /// It covers the iteration loop too, and that is not incidental:
    /// `compose::iterate` sends `results: &[]` on every iteration because "an
    /// iteration's exchange is a first exchange", so an iteration cannot
    /// inherit a turn's model history.
    fn at_turn_boundary(&mut self, request: &ModelRequest<'_>) {
        if request.results.is_empty() {
            self.rounds.clear();
        }
    }

    /// Remember one assistant turn, exactly as it arrived.
    ///
    /// Called only when the response was [`ModelResponse::Calls`]: a turn that
    /// answers or stops sends nothing further, so there is nothing for a later
    /// round to resend.
    pub fn remember(&mut self, message: wire::Message, calls: Vec<wire::ToolCall>) {
        self.rounds.push(Round { message, calls });
    }

    /// How many calls this turn has asked for across every round.
    #[must_use]
    pub fn calls_asked(&self) -> usize {
        self.rounds.iter().map(|round| round.calls.len()).sum()
    }
}

/// Build the request body for one exchange.
///
/// # Errors
///
/// [`OllamaFailure::ToolSchemaUnreadable`] when a descriptor's parameters are
/// not JSON, and [`OllamaFailure::ResultsDoNotMatchCalls`] when the loop's
/// accumulated results and this client's remembered calls differ in number —
/// because they are paired **by position**, so a mismatch would name a result
/// for the wrong tool.
pub fn request_from(
    request: &ModelRequest<'_>,
    answered: &mut Answered,
    model: &str,
    context_tokens: u64,
) -> Result<wire::Request, OllamaFailure> {
    answered.at_turn_boundary(request);

    let mut messages = Vec::with_capacity(1 + 2 * answered.rounds.len());

    // The prompt. `Prompt` can only be built from `Redacted`, which can only
    // be built by a `Redactor` -- so ADR-0008 clause 6's guarantee reaches
    // this line through the type system rather than through a call somebody
    // remembered to make. Nothing here redacts, and nothing here needs to.
    messages.push(wire::Message {
        role: ROLE_USER.to_owned(),
        content: request.prompt.as_str().to_owned(),
        tool_calls: Vec::new(),
        tool_name: None,
    });

    // Every round this turn has already had, as the API's own conversation:
    // the assistant's turn exactly as it arrived, then the results of the
    // calls it made, in the order it made them.
    //
    // **Correlation is by position, not by id.** That is
    // `ModelRequest::results`' own documented contract -- "What the tools
    // returned so far in this turn, oldest first" -- and `tool_call::machine`
    // appends each round's outcomes in call order, so position is exact even
    // for a provider that sends no id at all.
    if request.results.len() != answered.calls_asked() {
        return Err(OllamaFailure::ResultsDoNotMatchCalls {
            results: request.results.len(),
            calls: answered.calls_asked(),
        });
    }
    let mut results = request.results.iter();
    for round in &answered.rounds {
        messages.push(round.message.clone());
        for call in &round.calls {
            let Some(result) = results.next() else {
                // Unreachable while the count above holds; written as a
                // refusal rather than an `expect` because a request built on
                // a broken pairing is exactly what must not be sent.
                return Err(OllamaFailure::ResultsDoNotMatchCalls {
                    results: request.results.len(),
                    calls: answered.calls_asked(),
                });
            };
            messages.push(wire::Message {
                role: ROLE_TOOL.to_owned(),
                // `ToolResult::content` is `Redacted`, which is the second
                // half of ADR-0008 clause 6's type gate -- a tool's output
                // reaches a model through here and not through a prompt.
                content: result.content.as_str().to_owned(),
                tool_calls: Vec::new(),
                // **The tool's name, not the call's id.** See the module
                // documentation: this is the half of the `gemini-read-loop`
                // defect that does apply to this API, built in rather than
                // discovered.
                tool_name: Some(call.function.name.clone()),
            });
        }
    }

    let mut tools = Vec::with_capacity(request.tools.len());
    for descriptor in request.tools {
        // The schema is offered whole. Unlike the `gemini` client, this one
        // narrows nothing: Ollama hands the schema to the model's own
        // template and imposes no subset, so there is no keyword to drop and
        // no narrowing to justify.
        let parameters: Value = serde_json::from_str(&descriptor.parameters).map_err(|error| {
            OllamaFailure::ToolSchemaUnreadable {
                tool: descriptor.name.clone(),
                parser: error.to_string(),
            }
        })?;
        tools.push(wire::Tool {
            kind: TOOL_FUNCTION.to_owned(),
            function: wire::DeclaredFunction {
                name: descriptor.name.clone(),
                description: descriptor.description.clone(),
                parameters,
            },
        });
    }

    Ok(wire::Request {
        model: model.to_owned(),
        messages,
        tools,
        stream: true,
        options: wire::Options {
            num_thread: NUM_THREAD,
            num_ctx: context_tokens,
        },
    })
}

/// Reduce every frame of one exchange to a single response.
///
/// See the module documentation for the measurement that makes this necessary
/// rather than tidy: a call and the reason that ends its exchange arrive in
/// different frames.
///
/// A stream of one frame folds to that frame, which is what keeps the
/// non-streamed shape meaningful as a stream of length one.
#[must_use]
pub fn fold(frames: &[wire::Response]) -> wire::Response {
    let mut folded = wire::Response {
        model: String::new(),
        message: None,
        done: false,
        done_reason: None,
        prompt_eval_count: None,
        eval_count: None,
    };
    let mut text = String::new();
    let mut calls: Vec<wire::ToolCall> = Vec::new();
    let mut seen_message = false;

    for frame in frames {
        if !frame.model.is_empty() {
            folded.model.clone_from(&frame.model);
        }
        if let Some(message) = frame.message.as_ref() {
            seen_message = true;
            // Text is a **delta**: appended, never replaced. Replacing would
            // leave only the last frame's fragment, which on the recorded
            // six-frame answer is a single full stop.
            text.push_str(&message.content);
            calls.extend(message.tool_calls.iter().cloned());
        }
        folded.done |= frame.done;
        if frame.done_reason.is_some() {
            folded.done_reason.clone_from(&frame.done_reason);
        }
        // Usage is the **last frame that carried any**, which on this API is
        // the terminal one and no other. Not a sum: see the module
        // documentation for why the same rule holds here for a different
        // reason than it does for the `gemini` client.
        if frame.prompt_eval_count.is_some() {
            folded.prompt_eval_count = frame.prompt_eval_count;
        }
        if frame.eval_count.is_some() {
            folded.eval_count = frame.eval_count;
        }
    }

    if seen_message {
        folded.message = Some(wire::Message {
            role: ROLE_ASSISTANT.to_owned(),
            content: text,
            tool_calls: calls,
            tool_name: None,
        });
    }
    folded
}

/// Turn one folded response into the port's three arms.
///
/// # Errors
///
/// [`OllamaFailure::Unreadable`] when the fold carries no message at all. A
/// successful status with no message is a shape the API does not document, and
/// guessing at it — an empty `Text`, a `Stopped` with an invented reason —
/// would put words in the provider's mouth.
pub fn response_from(
    answer: &wire::Response,
    bytes: usize,
) -> Result<ModelResponse, OllamaFailure> {
    let tokens = usage_from(answer);

    let Some(message) = answer.message.as_ref() else {
        return Err(OllamaFailure::Unreadable {
            bytes,
            parser: "the response carried no message, which the API does not document as a \
                     successful shape"
                .to_owned(),
        });
    };

    // **Calls are read before the reason, and the ordering is load-bearing.**
    // The recorded tool-call exchange ends `done_reason: "stop"`, exactly as
    // the `gemini` client's recorded one ends `finishReason: "STOP"`, so a
    // client reading the reason first would report a stop and lose the call.
    let calls: Vec<ToolRequest> = message
        .tool_calls
        .iter()
        .map(|call| ToolRequest {
            // Carried, never generated. An id the provider did not send
            // becomes an empty one rather than a number this client made up:
            // renumbering is the failure `ToolRequest::id` is documented
            // against.
            id: call.id.clone().unwrap_or_default(),
            name: call.function.name.clone(),
            // The arguments are opaque to `zaru-core`, so they travel as the
            // JSON text they arrived as. Ollama sends an **object**, so this
            // is a re-serialisation rather than a pass-through of a string --
            // see `wire::CalledFunction::arguments`.
            arguments: call.function.arguments.to_string(),
        })
        .collect();

    if !calls.is_empty() {
        return Ok(ModelResponse::Calls { calls, tokens });
    }

    let reason = answer.done_reason.as_deref().unwrap_or_default();
    if reason == DONE_STOP && !message.content.is_empty() {
        return Ok(ModelResponse::Text {
            text: message.content.clone(),
            tokens,
        });
    }

    Ok(ModelResponse::Stopped {
        // The provider's own word, whatever it is. `done_reason` is treated as
        // an open set -- see `wire::Response` -- so a value Ollama adds
        // tomorrow reaches the user as itself rather than as this client's
        // guess about it. An empty reason is reported as such rather than
        // dressed up.
        reason: if reason.is_empty() {
            "the provider reported no reason for stopping".to_owned()
        } else {
            reason.to_owned()
        },
        tokens,
    })
}

/// ADR-0012 D7's two quantities, or zero when the provider reported none.
///
/// A response with no counts reports zeroes rather than refusing, which is the
/// one place this module lets a missing datum become a number. It is bounded
/// for the reason the `gemini` client's equivalent is: [`TokenUsage`] has no
/// way to express "unreported", and inventing one would be widening
/// `zaru-core`'s port from inside a provider client, which is a stop.
/// `Provider::usage` answers `None` before the first exchange, which is where
/// "nothing has been reported" is expressible.
///
/// **There is no third quantity here.** The `gemini` client adds thinking
/// tokens to the completion count because that API bills them and reports them
/// separately; `llama3.2:3b` emits none and Ollama carries no field for them
/// on this path, so D7's two quantities are the whole accounting and this
/// client decides nothing about the open question. A reasoning model would
/// bring the question back.
fn usage_from(answer: &wire::Response) -> TokenUsage {
    TokenUsage {
        prompt: answer.prompt_eval_count.unwrap_or_default(),
        completion: answer.eval_count.unwrap_or_default(),
    }
}
