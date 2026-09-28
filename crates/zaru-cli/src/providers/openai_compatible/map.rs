// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Between [`zaru_core::tool_call`]'s shapes and [`super::wire`]'s.
//!
//! # The fold is why this kind needed its own client
//!
//! [ADR-0012] D3 gives `openai-compatible` "everything OpenAI-shaped", and the
//! `ollama-client` arc recorded on 2026-09-14 that it stays a separate arc
//! because "the two differ at the wire, a tool call's arguments arriving as an
//! object where the compatible kind's arrive as a string". **Measured the
//! same day, that understates it**: the string may arrive whole in one frame
//! or accumulated across thirteen, and both are this one kind.
//!
//! | Server | One `get_weather` call |
//! | --- | --- |
//! | Ollama's `/v1/chat/completions` | `arguments` complete in one frame: `{"city":"Paris","unit":"c"}` |
//! | `llama-server --jinja`, same install, same weights | thirteen frames: `{`, `"`, `city`, `":`, ` "`, `Paris`, `",`, ` "`, `unit`, `":`, ` "`, `c`, `"}` |
//!
//! The difference is the flags: Ollama runs the same `llama-server` binary
//! with `--no-jinja --chat-template chatml` and parses the call on its Go
//! side, so a client that had only ever seen Ollama's surface would conclude
//! the arguments are always whole and be wrong about every other server of
//! this kind.
//!
//! **[`fold`] handles both with no branch on which**, because it appends where
//! a client might assign. Appending thirteen fragments gives the whole string;
//! appending one whole string gives the same string. That single choice is
//! what makes one client serve both, and it is the only line in this module a
//! mutation can redden against one fixture and not the other — which is why
//! both fixtures are recorded.
//!
//! # Correlation is by `index` for a call's fragments and by id for a result
//!
//! **Which fragments belong to which call** is [`wire::CallDelta::index`], the
//! one field present on every fragment. A fold that pushed in arrival order
//! would report thirteen calls where a real stream sent one.
//!
//! **Which result answers which call** is the call's id, carried: each result
//! the loop hands this client is a message naming the call it answers, so the
//! `tool_call_id` this API wants is the id the server issued, never one this
//! client made up.
//!
//! # A client of a stateless API owes it the model's own turns
//!
//! The `gemini-read-loop` arc measured this six ways against a live API after
//! a client that gave no assistant turns back made the model re-read one file
//! until the turn's ceiling. Since 2026-09-28 the loop hands every request the
//! whole conversation, the model's own messages included, for this turn and
//! every earlier one, so this client keeps nothing between two requests.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use super::failure::OpenAiCompatibleFailure;
use super::wire;
use serde_json::Value;
use std::collections::BTreeMap;
use zaru_core::conversation::Message;
use zaru_core::tool_call::{ModelRequest, ModelResponse, TokenUsage, ToolRequest};

/// The role an assistant turn carries.
pub const ROLE_ASSISTANT: &str = "assistant";
/// The role a person's message carries.
pub const ROLE_USER: &str = "user";
/// The role a tool result carries.
pub const ROLE_TOOL: &str = "tool";
/// The `type` every tool and tool call carries.
pub const TOOL_FUNCTION: &str = "function";
/// The finish reason of an answer that ended on its own.
pub const FINISH_STOP: &str = "stop";
/// The sentinel that ends a stream of this shape. **Not the SSE reader's
/// business** — see [`crate::providers::sse`]; it arrives as a payload and is
/// discarded here, before anything tries to parse it as JSON.
pub const DONE: &str = "[DONE]";

/// The role the system text carries.
pub const ROLE_SYSTEM: &str = "system";

/// Build the request body for one exchange.
///
/// # The whole conversation, every time
///
/// The API is stateless, so every request carries the conversation: the
/// system text as a `system` message, then every earlier turn, then this
/// turn's task, then this turn's own messages — a person's message as a
/// `user` message, the model's as an `assistant` message carrying its
/// `tool_calls`, and each result as a `tool` message naming the call it
/// answers by `tool_call_id`. Nothing is kept between two requests.
///
/// # Errors
///
/// [`OpenAiCompatibleFailure::ToolSchemaUnreadable`] when a descriptor's
/// parameters are not JSON.
pub fn request_from(
    request: &ModelRequest<'_>,
    model: &str,
) -> Result<wire::Request, OpenAiCompatibleFailure> {
    let task = Message::User {
        text: request.prompt.task().to_owned(),
    };
    let mut messages = Vec::with_capacity(2 + request.prompt.history().len() + request.turn.len());
    if let Some(system) = request.prompt.system() {
        messages.push(wire::Message {
            role: ROLE_SYSTEM.to_owned(),
            content: Some(system.to_owned()),
            tool_calls: Vec::new(),
            tool_call_id: None,
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
                content: Some(text.clone()),
                tool_calls: Vec::new(),
                tool_call_id: None,
            },
            Message::Assistant { text, calls, .. } => wire::Message {
                role: ROLE_ASSISTANT.to_owned(),
                // **`None` rather than an empty string** for a message that is
                // all tool calls, because that is what the servers themselves
                // send -- `llama-server`'s opening frame is `"content":null`.
                content: (!text.is_empty()).then(|| text.clone()),
                tool_calls: calls
                    .iter()
                    .map(|call| wire::ToolCall {
                        // Carried, never generated. An id a server never
                        // issued is the failure `ToolRequest::id` is
                        // documented against.
                        id: (!call.id.is_empty()).then(|| call.id.clone()),
                        kind: TOOL_FUNCTION.to_owned(),
                        function: wire::CalledFunction {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        },
                    })
                    .collect(),
                tool_call_id: None,
            },
            Message::Tool { id, content, .. } => wire::Message {
                role: ROLE_TOOL.to_owned(),
                content: Some(content.clone()),
                tool_calls: Vec::new(),
                // **The call's id, carried**, so the server knows which call
                // this answers.
                tool_call_id: (!id.is_empty()).then(|| id.clone()),
            },
        });
    }

    let tools = tools_of(request.tools)?;

    Ok(wire::Request {
        model: model.to_owned(),
        messages,
        tools,
        stream: true,
        stream_options: wire::StreamOptions {
            include_usage: true,
        },
    })
}

/// One turn's worth of frames, folded.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Every `delta.content` fragment, in order.
    pub text: String,
    /// The calls, keyed by the `index` that correlates their fragments.
    pub calls: Vec<Call>,
    /// The last finish reason any frame carried.
    pub finish_reason: Option<String>,
    /// The last usage any frame carried.
    pub usage: Option<wire::Usage>,
}

/// One accumulated tool call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Call {
    /// The provider's id, from the frame that carried one.
    pub id: Option<String>,
    /// The tool's name, from the frame that carried one.
    pub name: Option<String>,
    /// Every argument fragment, appended in arrival order.
    pub arguments: String,
}

/// Fold a stream's frames into one answer.
///
/// # The two lines a mutation reddens, and the fixture each needs
///
/// **`arguments.push_str` rather than an assignment.** Against
/// `recorded/delta-arguments.sse` an assignment leaves `"}"` — the last
/// fragment — and the check fails. Against `recorded/whole-arguments.sse` an
/// assignment is *indistinguishable* from an append, because there is one
/// fragment and it is the whole. Only one of the two recorded servers can see
/// that mutant, which is the argument for recording both.
///
/// **Keying by [`wire::CallDelta::index`] rather than pushing in arrival
/// order.** Against `recorded/delta-arguments.sse` a push gives thirteen calls
/// where the server sent one; against `recorded/two-calls.sse` it gives twelve
/// where there are two, and in an interleaved order that no positional scheme
/// can recover.
///
/// # Usage is the last frame that carried any, and is not a sum
///
/// Both measured servers send exactly one usage-bearing frame, on which the
/// last and the sum are equal — so **a mutation that summed here could not be
/// reddened by any recorded fixture**, and none is claimed. The rule is stated
/// because the API's own documentation makes the terminal frame cumulative
/// rather than incremental, and a later provider that sent two would otherwise
/// be double-counted.
#[must_use]
pub fn fold(frames: &[wire::Chunk]) -> Answer {
    let mut text = String::new();
    // `BTreeMap` rather than a `HashMap`: the calls are handed to the loop in
    // an order the user sees, and `index` is what the provider says that order
    // is. A hash map would make the order of two calls depend on a hasher.
    let mut calls: BTreeMap<u32, Call> = BTreeMap::new();
    let mut finish_reason: Option<String> = None;
    let mut usage: Option<wire::Usage> = None;

    for frame in frames {
        for choice in &frame.choices {
            if let Some(content) = choice.delta.content.as_ref() {
                // Text is a **delta**: appended, never replaced.
                text.push_str(content);
            }
            for fragment in &choice.delta.tool_calls {
                let call = calls.entry(fragment.index).or_default();
                // **Set from the frame that carries one, and never unset.**
                // Every frame after the first omits all three, so an
                // assignment from `None` would erase the identity the first
                // frame established.
                if fragment.id.is_some() {
                    call.id.clone_from(&fragment.id);
                }
                if let Some(function) = fragment.function.as_ref() {
                    if function.name.is_some() {
                        call.name.clone_from(&function.name);
                    }
                    // **The line.** See this function's documentation.
                    call.arguments.push_str(&function.arguments);
                }
            }
            if choice.finish_reason.is_some() {
                finish_reason.clone_from(&choice.finish_reason);
            }
        }
        if frame.usage.is_some() {
            usage = frame.usage;
        }
    }

    Answer {
        text,
        calls: calls.into_values().collect(),
        finish_reason,
        usage,
    }
}

/// Turn a folded answer into the response the loop reads.
///
/// # Calls are read before the reason, and the ordering is load-bearing
///
/// Both measured servers end a tool-calling stream with
/// `finish_reason: "tool_calls"`, so reading the reason first would be
/// harmless here — and that is exactly why the ordering is written down rather
/// than left to luck. The `gemini` client's recorded tool-call exchange ends
/// `finishReason: "STOP"` and the `ollama` client's ends `done_reason: "stop"`,
/// and a client of either that read the reason first would report a stop and
/// silently lose the call. The same shape, guarded the same way, before a
/// server of this kind produces it.
///
/// # Errors
///
/// [`OpenAiCompatibleFailure::Unreadable`] when the stream carried no choices
/// at all, which the API does not document as a successful shape.
pub fn response_from(
    answer: &Answer,
    bytes: usize,
    saw_a_choice: bool,
) -> Result<ModelResponse, OpenAiCompatibleFailure> {
    let tokens = usage_from(answer);

    if !saw_a_choice {
        return Err(OpenAiCompatibleFailure::Unreadable {
            bytes,
            parser: "the stream carried no choices, which the API does not document as a \
                     successful shape"
                .to_owned(),
        });
    }

    let calls: Vec<ToolRequest> = answer
        .calls
        .iter()
        .map(|call| ToolRequest {
            // Carried, never generated. An id the provider did not send becomes
            // an empty one rather than a number this client made up:
            // renumbering is the failure `ToolRequest::id` is documented
            // against.
            id: call.id.clone().unwrap_or_default(),
            name: call.name.clone().unwrap_or_default(),
            // **A pass-through, not a re-serialisation.** The arguments arrived
            // as text and `zaru-core`'s port documents them opaque, so nothing
            // here parses them -- which is the one place this client is simpler
            // than the `ollama` one, whose arguments arrive as an object and
            // must be rendered back to text.
            arguments: call.arguments.clone(),
        })
        .collect();

    if !calls.is_empty() {
        return Ok(ModelResponse::Calls {
            calls,
            text: answer.text.clone(),
            echo: None,
            tokens,
        });
    }

    let reason = answer.finish_reason.as_deref().unwrap_or_default();
    if reason == FINISH_STOP && !answer.text.is_empty() {
        return Ok(ModelResponse::Text {
            echo: None,
            text: answer.text.clone(),
            tokens,
        });
    }

    Ok(ModelResponse::Stopped {
        // The provider's own word, whatever it is -- `finish_reason` is treated
        // as an open set, so a value a gateway invents tomorrow reaches the
        // user as itself rather than as this client's guess about it.
        reason: if reason.is_empty() {
            "the provider reported no reason for stopping".to_owned()
        } else {
            reason.to_owned()
        },
        tokens,
    })
}

/// What the exchange cost, or zero where the server reported nothing.
fn usage_from(answer: &Answer) -> TokenUsage {
    match answer.usage {
        Some(usage) => TokenUsage {
            prompt: usage.prompt_tokens,
            completion: usage.completion_tokens,
        },
        // A server that ignores `stream_options` sends no usage frame --
        // measured against Ollama's `/v1` with the field omitted. Zero is
        // reported rather than a number this client would have to invent.
        None => TokenUsage {
            prompt: 0,
            completion: 0,
        },
    }
}

/// This kind's wire shape for a set of tool descriptors.
///
/// **One spelling, called twice**: by [`request_from`], which sends them, and
/// by [`OpenAiCompatibleClient::tool_surface_bytes`](super::OpenAiCompatibleClient::tool_surface_bytes),
/// which measures what they cost.
///
/// # Errors
///
/// [`OpenAiCompatibleFailure::ToolSchemaUnreadable`], naming the tool.
pub fn tools_of(
    descriptors: &[zaru_core::tool_call::ToolDescriptor],
) -> Result<Vec<wire::Tool>, OpenAiCompatibleFailure> {
    let mut tools = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let parameters: Value = serde_json::from_str(&descriptor.parameters).map_err(|error| {
            OpenAiCompatibleFailure::ToolSchemaUnreadable {
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
