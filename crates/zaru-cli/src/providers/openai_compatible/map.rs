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
//! # Correlation is by `index` for a call and by position for a result
//!
//! Two different questions with two different answers, and conflating them is
//! the defect this paragraph exists to prevent.
//!
//! **Which fragments belong to which call** is [`wire::CallDelta::index`], the
//! one field present on every fragment. A fold that pushed in arrival order
//! would report thirteen calls where a real stream sent one.
//!
//! **Which result answers which call** is *position*, exactly as
//! [`crate::providers::ollama::map`] argues: [`ModelRequest::results`]' own
//! contract is "what the tools returned so far in this turn, oldest first",
//! and `tool_call::machine` appends each round's outcomes in call order. The
//! `tool_call_id` this API wants is then **carried from the remembered call**
//! rather than generated — the half of the `gemini-read-loop` defect that does
//! apply here, built in rather than discovered.
//!
//! # A client of a stateless API owes it the model's own turns
//!
//! The `gemini-read-loop` arc measured this six ways against a live API after
//! a client that gave no assistant turns back made the model re-read one file
//! until the turn's ceiling. Its closing sentence — "the second client should
//! expect to keep the same state" — is why [`Answered`] is here, and it is the
//! third client to keep it.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`ModelRequest::results`]: zaru_core::tool_call::ModelRequest::results

use super::failure::OpenAiCompatibleFailure;
use super::wire;
use serde_json::Value;
use std::collections::BTreeMap;
use zaru_core::tool_call::{ModelRequest, ModelResponse, TokenUsage, ToolRequest};

/// The role an assistant turn carries.
pub const ROLE_ASSISTANT: &str = "assistant";
/// The role the prompt carries.
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

/// What this turn has already asked for, so it can be given back.
#[derive(Debug, Default)]
pub struct Answered {
    rounds: Vec<Round>,
}

/// One round: what the model said, and what it asked for.
#[derive(Debug, Clone)]
struct Round {
    message: wire::Message,
    calls: Vec<wire::ToolCall>,
}

impl Answered {
    /// Forget everything when a new turn starts.
    ///
    /// A request arriving with no results is the first round of a turn, which
    /// is the only boundary this client can see: the port hands it one
    /// [`ModelRequest`] at a time and nothing says "a turn began".
    fn at_turn_boundary(&mut self, request: &ModelRequest<'_>) {
        if request.results.is_empty() {
            self.rounds.clear();
        }
    }

    /// Remember an assistant turn that asked for tools.
    pub fn remember(&mut self, message: wire::Message, calls: Vec<wire::ToolCall>) {
        self.rounds.push(Round { message, calls });
    }

    /// How many calls this turn has asked for so far.
    #[must_use]
    pub fn calls_asked(&self) -> usize {
        self.rounds.iter().map(|round| round.calls.len()).sum()
    }
}

/// Build the request body for one exchange.
///
/// # Errors
///
/// [`OpenAiCompatibleFailure::ToolSchemaUnreadable`] when a descriptor's
/// parameters are not JSON, and
/// [`OpenAiCompatibleFailure::ResultsDoNotMatchCalls`] when this turn's own
/// bookkeeping disagrees with itself.
pub fn request_from(
    request: &ModelRequest<'_>,
    answered: &mut Answered,
    model: &str,
) -> Result<wire::Request, OpenAiCompatibleFailure> {
    answered.at_turn_boundary(request);

    let mut messages = Vec::with_capacity(1 + 2 * answered.rounds.len());

    // The prompt. `Prompt` can only be built from `Redacted`, which can only
    // be built by a `Redactor` -- so ADR-0008 clause 6's guarantee reaches this
    // line through the type system rather than through a call somebody
    // remembered to make. Nothing here redacts, and nothing here needs to.
    messages.push(wire::Message {
        role: ROLE_USER.to_owned(),
        content: Some(request.prompt.as_str().to_owned()),
        tool_calls: Vec::new(),
        tool_call_id: None,
    });

    if request.results.len() != answered.calls_asked() {
        return Err(OpenAiCompatibleFailure::ResultsDoNotMatchCalls {
            results: request.results.len(),
            calls: answered.calls_asked(),
        });
    }
    let mut results = request.results.iter();
    for round in &answered.rounds {
        messages.push(round.message.clone());
        for call in &round.calls {
            let Some(result) = results.next() else {
                // Unreachable while the count above holds; written as a refusal
                // rather than an `expect` because a request built on a broken
                // pairing is exactly what must not be sent.
                return Err(OpenAiCompatibleFailure::ResultsDoNotMatchCalls {
                    results: request.results.len(),
                    calls: answered.calls_asked(),
                });
            };
            messages.push(wire::Message {
                role: ROLE_TOOL.to_owned(),
                // `ToolResult::content` is `Redacted`, which is the second half
                // of ADR-0008 clause 6's type gate -- a tool's output reaches a
                // model through here and not through a prompt.
                content: Some(result.content.as_str().to_owned()),
                tool_calls: Vec::new(),
                // **The call's id, carried.** An id this client invented would
                // be an id the server never issued, which is the failure
                // `ToolRequest::id` is documented against; an id omitted would
                // leave the server to guess which call a result answers.
                tool_call_id: call.id.clone(),
            });
        }
    }

    let mut tools = Vec::with_capacity(request.tools.len());
    for descriptor in request.tools {
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
        return Ok(ModelResponse::Calls { calls, tokens });
    }

    let reason = answer.finish_reason.as_deref().unwrap_or_default();
    if reason == FINISH_STOP && !answer.text.is_empty() {
        return Ok(ModelResponse::Text {
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

/// The assistant turn to remember, built from what was folded.
///
/// Kept beside [`fold`] rather than inside it because folding is about frames
/// and this is about the conversation: the same fold serves a text answer,
/// which is remembered nowhere.
#[must_use]
pub fn assistant_turn(answer: &Answer) -> (wire::Message, Vec<wire::ToolCall>) {
    let calls: Vec<wire::ToolCall> = answer
        .calls
        .iter()
        .map(|call| wire::ToolCall {
            id: call.id.clone(),
            kind: TOOL_FUNCTION.to_owned(),
            function: wire::CalledFunction {
                name: call.name.clone().unwrap_or_default(),
                arguments: call.arguments.clone(),
            },
        })
        .collect();
    let message = wire::Message {
        role: ROLE_ASSISTANT.to_owned(),
        // **`None` rather than an empty string**, because that is what the
        // servers themselves send for an assistant turn that is all tool calls
        // -- `llama-server`'s opening frame is `"content":null` -- and a turn
        // sent back in a shape the server does not produce is a shape nobody
        // has tested the far side against.
        content: if answer.text.is_empty() {
            None
        } else {
            Some(answer.text.clone())
        },
        tool_calls: calls.clone(),
        tool_call_id: None,
    };
    (message, calls)
}
