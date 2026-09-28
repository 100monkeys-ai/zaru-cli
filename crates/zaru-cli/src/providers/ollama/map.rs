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
//! finding. Since 2026-09-28 the loop hands every request the whole
//! conversation, the model's own messages included, for this turn and every
//! earlier one, so this client keeps nothing between two requests.
//!
//! The other half of that defect **does** apply and is built in from the first
//! line: each result is named for the **tool** rather than for the call's id.
//! Ollama's field is `tool_name` and takes a name, so the shape that cost the
//! `gemini` client a defect is not reachable here.

use super::endpoint::NUM_THREAD;
use super::failure::OllamaFailure;
use super::wire;
use serde_json::Value;
use zaru_core::conversation::Message;
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

/// The role the system text carries.
pub const ROLE_SYSTEM: &str = "system";

/// Build the request body for one exchange.
///
/// # The whole conversation, every time
///
/// `/api/chat` is stateless, so every request carries the conversation: the
/// system text as a `system` message, then every earlier turn, then this
/// turn's task, then this turn's own messages — a person's message as a `user`
/// message, the model's as an `assistant` message carrying its `tool_calls`,
/// and each result as a `tool` message naming its tool by `tool_name`.
/// Nothing is kept between two requests.
///
/// # Errors
///
/// [`OllamaFailure::ToolSchemaUnreadable`] when a descriptor's parameters are
/// not JSON.
pub fn request_from(
    request: &ModelRequest<'_>,
    model: &str,
    context_tokens: u64,
) -> Result<wire::Request, OllamaFailure> {
    let task = Message::User {
        text: request.prompt.task().to_owned(),
    };
    let mut messages = Vec::with_capacity(2 + request.prompt.history().len() + request.turn.len());
    if let Some(system) = request.prompt.system() {
        messages.push(wire::Message {
            role: ROLE_SYSTEM.to_owned(),
            content: system.to_owned(),
            tool_calls: Vec::new(),
            tool_name: None,
        });
    }
    for message in request
        .prompt
        .history()
        .iter()
        .chain(core::iter::once(&task))
        .chain(request.turn.iter())
    {
        messages.push(match message {
            Message::User { text } => wire::Message {
                role: ROLE_USER.to_owned(),
                content: text.clone(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
            Message::Assistant { text, calls, .. } => wire::Message {
                role: ROLE_ASSISTANT.to_owned(),
                content: text.clone(),
                tool_calls: calls
                    .iter()
                    .map(|call| wire::ToolCall {
                        id: (!call.id.is_empty()).then(|| call.id.clone()),
                        function: wire::CalledFunction {
                            name: call.name.clone(),
                            // Ollama takes the arguments as an **object**.
                            // Text that is not JSON is sent as a string
                            // rather than dropped.
                            arguments: serde_json::from_str(&call.arguments)
                                .unwrap_or_else(|_| Value::String(call.arguments.clone())),
                            index: None,
                        },
                    })
                    .collect(),
                tool_name: None,
            },
            Message::Tool { name, content, .. } => wire::Message {
                role: ROLE_TOOL.to_owned(),
                content: content.clone(),
                tool_calls: Vec::new(),
                // **The tool's name, not the call's id.** See the module
                // documentation: this is the half of the `gemini-read-loop`
                // defect that does apply to this API.
                tool_name: Some(name.clone()),
            },
        });
    }

    let tools = tools_of(request.tools)?;

    Ok(wire::Request {
        model: model.to_owned(),
        messages,
        tools,
        stream: true,
        options: wire::Options {
            num_thread: NUM_THREAD,
            num_ctx: context_tokens,
        },
        truncate: false,
        shift: false,
    })
}

/// This kind's wire shape for a set of tool descriptors.
///
/// **One spelling, called twice**: by [`request_from`] above, which sends
/// them, and by
/// [`OllamaClient::tool_surface_bytes`](super::OllamaClient::tool_surface_bytes),
/// which measures what they cost. Two spellings would be a measurement of a
/// request nobody sends.
///
/// The schema is offered whole. Unlike the `gemini` client, this one narrows
/// nothing: Ollama hands the schema to the model's own template and imposes
/// no subset, so there is no keyword to drop and no narrowing to justify.
///
/// # Errors
///
/// [`OllamaFailure::ToolSchemaUnreadable`], naming the tool.
pub fn tools_of(
    descriptors: &[zaru_core::tool_call::ToolDescriptor],
) -> Result<Vec<wire::Tool>, OllamaFailure> {
    let mut tools = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
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
    Ok(tools)
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
        return Ok(ModelResponse::Calls {
            calls,
            text: message.content.clone(),
            echo: None,
            tokens,
        });
    }

    let reason = answer.done_reason.as_deref().unwrap_or_default();
    if reason == DONE_STOP && !message.content.is_empty() {
        return Ok(ModelResponse::Text {
            echo: None,
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
