// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The mapping between `zaru-core`'s ports and Gemini's wire shapes, both
//! ways.
//!
//! # Why this is its own module with no I/O in it
//!
//! Everything here is a function from values to values, and nothing here
//! opens a socket, reads a clock or touches a file. That is what lets a
//! recorded exchange — a real response body captured once and scrubbed —
//! drive both directions offline, on a CI runner with no key and no network.
//! [Testing] forbids a check calling a provider, and a mapping that could
//! only be exercised through a socket would be a mapping nothing checks.
//!
//! [`request_from`] keeps nothing between two requests: the loop hands it
//! the whole conversation every time, and a model message that must go back
//! to Google exactly travels in that message's own `echo`.
//!
//! # A round is a pair of turns, and the model's half is not optional
//!
//! The conversation this module builds has, for every round, the model's own
//! turn followed by the results of the calls it made. The model's half was
//! missing until 2026-09-05, and its absence is the whole of the
//! `gemini-read-loop` defect — see [`wire::FunctionResponse::name`] for what
//! it cost and how it was measured. Since 2026-09-28 the same holds for every
//! earlier turn of a session, not only the turn in flight.
//!
//! # The three arms, and what decides between them
//!
//! [`ModelResponse`] has exactly three: text, tool calls, or a stop. The
//! decision is read off the candidate rather than invented:
//!
//! - any `functionCall` part → [`ModelResponse::Calls`], **carrying Gemini's
//!   own id**;
//! - otherwise, `finishReason` of `STOP` with text → [`ModelResponse::Text`];
//! - anything else → [`ModelResponse::Stopped`], carrying the provider's own
//!   `finishReason` verbatim.
//!
//! Calls are checked for **before** the finish reason, because a turn that
//! asks for a tool is a turn that continues, whatever word the provider ends
//! it with. Reading the finish reason first would turn every tool call into a
//! stop for any `finishReason` this client has not been told about — and the
//! full enum is not documented, which is exactly the case that ordering
//! protects.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use super::failure::GeminiFailure;
use super::wire;
use serde_json::Value;
use zaru_core::conversation::Message;
use zaru_core::tool_call::{ModelRequest, ModelResponse, TokenUsage, ToolRequest};

/// Build a request body from what the loop handed the model.
///
/// # The whole conversation, in Google's shapes
///
/// `generateContent` is stateless and its function-calling guide says what a
/// caller owes it: "you must pass the full history of the conversation in the
/// input field of each subsequent request. This history must include: 1. The
/// initial user_input step. 2. All model-generated steps returned in Turn 1
/// (including thought and function_call steps) exactly as received. 3. The
/// function_result step containing the output of your executed function."
///
/// So every request is built from the whole conversation the loop hands it
/// and nothing is kept between two requests:
///
/// - the system text becomes `systemInstruction`, Google's place for
///   instructions that are not the person's;
/// - every earlier turn, then this turn's task, then this turn's messages,
///   each in its role: a person's message as a text part of a `user` turn,
///   the model's message as a `model` turn, and each call's result as a
///   `functionResponse` part of a `user` turn, named for the tool;
/// - turns alternate: messages of one role side by side are joined into one
///   turn, their parts in order;
/// - a model message that arrived with parts this client must give back
///   exactly — a `thoughtSignature`, or a part it does not model — carries
///   them in its `echo`, and they are sent as they arrived.
///
/// # Errors
///
/// [`GeminiFailure::ToolSchemaUnreadable`] when a tool descriptor's parameter
/// schema is not JSON. ADR-0011 D1 declares no argument shapes, so the schema
/// is whichever surface owns the tool — and a schema that is not JSON is that
/// surface's defect rather than the user's or the provider's.
pub fn request_from(request: &ModelRequest<'_>) -> Result<wire::Request, GeminiFailure> {
    let task = Message::User {
        text: request.prompt.task().to_owned(),
    };
    let conversation = request
        .prompt
        .history()
        .iter()
        .chain(core::iter::once(&task))
        .chain(request.turn.iter());

    let mut contents: Vec<wire::Content> = Vec::new();
    for message in conversation {
        let (role, parts) = match message {
            Message::User { text } => (
                wire::ROLE_USER,
                vec![wire::Part::Text {
                    text: text.clone(),
                    thought_signature: None,
                }],
            ),
            Message::Assistant { text, calls, echo } => (
                wire::ROLE_MODEL,
                echoed(echo.as_deref()).unwrap_or_else(|| parts_of(text, calls)),
            ),
            Message::Tool {
                id,
                name,
                content,
                failed,
            } => (
                wire::ROLE_USER,
                vec![wire::Part::FunctionResponse {
                    function_response: wire::FunctionResponse {
                        // Google asks for it: "Include this exact `id` in your
                        // `functionResponse` so the model can accurately map
                        // your result back". An empty one is not an id.
                        id: (!id.is_empty()).then(|| id.clone()),
                        // The tool's own name. See `wire::FunctionResponse::name`
                        // for what sending the id here did.
                        name: name.clone(),
                        response: serde_json::json!({
                            "content": content,
                            "failed": failed,
                        }),
                    },
                }],
            ),
        };
        if parts.is_empty() {
            continue;
        }
        // **Turns alternate.** Two messages of one role side by side -- the
        // results of one model message, a result followed by the person's
        // next task, or two tasks with no answer between them -- are joined
        // into one turn with their parts in order, so Google is never sent
        // two `user` or two `model` turns in a row.
        match contents.last_mut() {
            Some(last) if last.role == role => last.parts.extend(parts),
            _ => contents.push(wire::Content {
                role: role.to_owned(),
                parts,
            }),
        }
    }

    Ok(wire::Request {
        system_instruction: request.prompt.system().map(|system| wire::Content {
            role: String::new(),
            parts: vec![wire::Part::Text {
                text: system.to_owned(),
                thought_signature: None,
            }],
        }),
        contents,
        tools: tools_of(request.tools)?,
    })
}

/// The parts a model message arrived with, if it kept them.
///
/// `None` for a message with no echo, and for an echo this client cannot
/// read, which is then rebuilt from the text and the calls.
fn echoed(echo: Option<&str>) -> Option<Vec<wire::Part>> {
    serde_json::from_str::<Vec<wire::Part>>(echo?).ok()
}

/// A model message's parts, built from its text and its calls.
///
/// Used where the message carried no parts of its own to give back: nothing
/// in it needed to go back exactly, or it came from another provider.
fn parts_of(text: &str, calls: &[ToolRequest]) -> Vec<wire::Part> {
    let mut parts = Vec::with_capacity(1 + calls.len());
    if !text.is_empty() {
        parts.push(wire::Part::Text {
            text: text.to_owned(),
            thought_signature: None,
        });
    }
    for call in calls {
        parts.push(wire::Part::FunctionCall {
            function_call: wire::FunctionCall {
                id: (!call.id.is_empty()).then(|| call.id.clone()),
                name: call.name.clone(),
                // The arguments as the model sent them. Text that is not JSON
                // is sent as a string rather than dropped.
                args: serde_json::from_str(&call.arguments)
                    .unwrap_or_else(|_| Value::String(call.arguments.clone())),
            },
            thought_signature: None,
        });
    }
    parts
}

/// What a model message must carry back to Google exactly, if anything.
///
/// The parts as they arrived, when any of them holds a `thoughtSignature` —
/// which Google refuses a function call without — or is a part this client
/// does not model. `None` otherwise: the text and the calls rebuild the
/// message exactly.
fn echo_of(parts: &[wire::Part]) -> Option<String> {
    let must_return = parts.iter().any(|part| match part {
        wire::Part::FunctionCall {
            thought_signature, ..
        }
        | wire::Part::Text {
            thought_signature, ..
        } => thought_signature.is_some(),
        wire::Part::Other(_) => true,
        wire::Part::FunctionResponse { .. } => false,
    });
    if must_return {
        serde_json::to_string(parts).ok()
    } else {
        None
    }
}

/// This kind's wire shape for a set of tool descriptors.
///
/// **One spelling, called twice**: by [`request_from`], which sends them, and
/// by [`GeminiClient::tool_surface_bytes`](super::GeminiClient::tool_surface_bytes),
/// which measures what they cost. Two spellings would be a measurement of a
/// request nobody sends -- and this kind narrows each schema to Google's own
/// subset, so the difference between the two would be real.
///
/// One `Tool` entry carrying every declaration, rather than one entry per
/// tool. Both are accepted; one entry is what Google's own examples show, and
/// a client that sent N entries would be making a choice the documentation
/// does not.
///
/// # Errors
///
/// [`GeminiFailure::ToolSchemaUnreadable`], naming the tool.
pub fn tools_of(
    descriptors: &[zaru_core::tool_call::ToolDescriptor],
) -> Result<Vec<wire::Tool>, GeminiFailure> {
    let mut declarations = Vec::with_capacity(descriptors.len());
    for tool in descriptors {
        let parameters: Value = serde_json::from_str(&tool.parameters).map_err(|error| {
            GeminiFailure::ToolSchemaUnreadable {
                tool: tool.name.clone(),
                parser: error.to_string(),
            }
        })?;
        declarations.push(wire::FunctionDeclaration {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters: within_geminis_subset(parameters),
        });
    }
    if declarations.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![wire::Tool {
        function_declarations: declarations,
    }])
}

/// Read a response body as one of the port's three arms.
///
/// `bytes` is how large the body was, so that a response this function cannot
/// make sense of is reported by its size and never by its content.
///
/// # Errors
///
/// [`GeminiFailure::Unreadable`] when the response carries no candidate at
/// all. A successful HTTP status with no candidate is a shape the API does
/// not document, and guessing at it — an empty `Text`, a `Stopped` with an
/// invented reason — would put words in the provider's mouth.
pub fn response_from(
    answer: &wire::Response,
    bytes: usize,
) -> Result<ModelResponse, GeminiFailure> {
    let tokens = usage_from(answer.usage_metadata);

    let Some(candidate) = answer.candidates.first() else {
        return Err(GeminiFailure::Unreadable {
            bytes,
            parser: "the response carried no candidate, which the API does not document as a \
                     successful shape"
                .to_owned(),
        });
    };

    let parts = candidate
        .content
        .as_ref()
        .map_or::<&[wire::Part], _>(&[], |content| &content.parts);

    let calls: Vec<ToolRequest> = parts
        .iter()
        .filter_map(|part| match part {
            wire::Part::FunctionCall { function_call, .. } => Some(ToolRequest {
                // Carried, never generated. An id the provider did not send
                // becomes an empty one rather than a number this client made
                // up: renumbering is the failure `ToolRequest::id` is
                // documented against.
                id: function_call.id.clone().unwrap_or_default(),
                name: function_call.name.clone(),
                // The arguments are opaque to `zaru-core`, so they travel as
                // the JSON text they arrived as.
                arguments: function_call.args.to_string(),
            }),
            wire::Part::Text { .. }
            | wire::Part::FunctionResponse { .. }
            | wire::Part::Other(_) => None,
        })
        .collect();

    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            wire::Part::Text { text, .. } => Some(text.as_str()),
            wire::Part::FunctionCall { .. }
            | wire::Part::FunctionResponse { .. }
            | wire::Part::Other(_) => None,
        })
        .collect();

    let echo = echo_of(parts);

    if !calls.is_empty() {
        return Ok(ModelResponse::Calls {
            calls,
            text,
            echo,
            tokens,
        });
    }

    let finish = candidate.finish_reason.as_deref().unwrap_or_default();
    if finish == wire::FINISH_STOP && !text.is_empty() {
        return Ok(ModelResponse::Text { text, echo, tokens });
    }

    Ok(ModelResponse::Stopped {
        // The provider's own word, whatever it is. `finishReason` is treated
        // as an open enum -- see `wire::Candidate` -- so a value Google adds
        // tomorrow reaches the user as itself rather than as this client's
        // guess about it. An empty reason is reported as such rather than
        // dressed up.
        reason: if finish.is_empty() {
            "the provider reported no finish reason".to_owned()
        } else {
            finish.to_owned()
        },
        tokens,
    })
}

/// What one streamed frame reports, or `None` when it reports nothing.
///
/// # Why this is `Option` where [`usage_from`] is not
///
/// `usage_from` answers about a **folded** response, where a missing
/// `usageMetadata` means the provider reported none for the whole exchange
/// and zero is the honest reading. This answers about **one frame**, where a
/// missing `usageMetadata` means only that this frame did not repeat what an
/// earlier one already said — so a zero here would overwrite a real count
/// with an invented one. The caller is `GeminiClient::record_usage`, which
/// leaves its slot alone on `None`.
///
/// Measured 2026-09-15 against the live API: every frame of both recorded
/// streams carries `usageMetadata`, so this is a guard against a shape the
/// API does not document rather than one observed. `turn-liveness`'
/// accepting sibling feeds a frame without one and asserts the slot is
/// unchanged.
pub(super) fn usage_of(frame: &wire::Response) -> Option<TokenUsage> {
    frame
        .usage_metadata
        .map(|metadata| usage_from(Some(metadata)))
}

/// ADR-0012 D7's two quantities, or zero when the provider reported none.
///
/// A response with no `usageMetadata` reports zeroes rather than refusing.
/// That is the one place this module lets a missing datum become a number,
/// and it is bounded: [`TokenUsage`] has no way to express "unreported", and
/// inventing one would be widening `zaru-core`'s port from inside a provider
/// client -- which is a stop. `Provider::usage` answers `None` before the
/// first exchange, which is where "nothing has been reported" is expressible,
/// and the gap is raised on ADR-0012 rather than closed here.
pub(super) fn usage_from(metadata: Option<wire::UsageMetadata>) -> TokenUsage {
    let metadata = metadata.unwrap_or_default();
    TokenUsage {
        prompt: metadata.prompt_token_count,
        // Candidates **plus** thoughts, because thinking tokens are billed as
        // output and reporting the candidates alone under-reports what the
        // user pays -- by more than four times on the first real response
        // this client received. See `wire::UsageMetadata::thoughts_token_count`
        // for the measurement, the reasoning, and the fact that it is a
        // reading raised on ADR-0012 rather than a settled one.
        //
        // Saturating, because two counts a provider reported cannot be
        // trusted not to overflow a sum and a panic here would lose an answer
        // that already arrived.
        completion: metadata
            .candidates_token_count
            .saturating_add(metadata.thoughts_token_count),
    }
}

/// The keywords Gemini's `FunctionDeclaration.parameters` accepts.
///
/// **Not a JSON Schema.** Google documents that field as a subset of OpenAPI
/// 3.0's Schema object, and it refuses an unknown key outright rather than
/// ignoring it. The eight below are what that subset admits for the shapes
/// [ADR-0011] D1's seven tools use; anything else is dropped here rather than
/// sent.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
const GEMINI_SCHEMA_KEYWORDS: [&str; 8] = [
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "items",
    "properties",
    "required",
];

/// Narrow a JSON Schema to the subset Gemini's tool declarations accept.
///
/// # This was measured against the live API rather than read
///
/// [ADR-0011] D1's argument contract emits `"additionalProperties": false`,
/// which is correct JSON Schema and is the half of that contract that says a
/// request carrying an unexpected field is not a call. Gemini's schema is an
/// **OpenAPI 3.0 subset** and rejects the keyword by name:
///
/// ```text
/// Invalid JSON payload received. Unknown name "additionalProperties" at
/// 'tools[0].function_declarations[0].parameters': Cannot find field.
/// ```
///
/// — HTTP 400 `INVALID_ARGUMENT`, once per tool, so **all seven declarations
/// were refused and every turn failed**. Measured 2026-09-05 by the first
/// composition that handed these descriptors to a provider; nothing before it
/// ever had, which is why a contract landed on 2026-09-05 and a client landed
/// on 2026-09-05 could both be right and still not work together.
///
/// It is the same shape [ADR-0011]'s own Update already recorded once: "an
/// empty string is not JSON … the seven empty schemas this surface offered
/// would have been refused, all seven, by the first provider client handed
/// them." That was fixed by giving them real schemas. This is the second
/// reason the same seven were refused, and it could only be found by sending
/// them.
///
/// # Why the mapping narrows rather than the contract
///
/// ADR-0011 D1's schema is **the harness's** wire contract, offered to every
/// provider kind; `additionalProperties: false` is part of what that contract
/// says and a provider that accepts it should be told it. Narrowing here is
/// what a provider mapping is for — this module already maps every other part
/// of the request to Google's shape — and it keeps the contract intact for the
/// four kinds that have no client yet.
///
/// **It drops rather than translates.** There is no equivalent keyword in
/// Gemini's subset, so the closed-object constraint is simply not expressible
/// to this provider; the parser on the way back in is what actually enforces
/// it, and that is unchanged. Recursive, because `properties` holds schemas
/// and a nested object would carry the same keyword.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
fn within_geminis_subset(schema: Value) -> Value {
    let Value::Object(fields) = schema else {
        // A schema is an object. Anything else is a leaf this function has no
        // business rewriting -- `required`'s array of names, `enum`'s array of
        // values, a `description` string.
        return schema;
    };
    let mut kept = serde_json::Map::new();
    for (name, value) in fields {
        if !GEMINI_SCHEMA_KEYWORDS.contains(&name.as_str()) {
            continue;
        }
        // **`properties` is a map of field NAMES to schemas, not a schema.**
        // Recursing into it as one would filter out every field whose name is
        // not a keyword -- which is every field -- and send Gemini a tool
        // whose parameters have no properties at all. Its values are schemas
        // and are narrowed; its keys are the contract's own field names and
        // are untouched. Found by writing the recursion the obvious way first.
        let narrowed = if name == "properties" {
            match value {
                Value::Object(properties) => Value::Object(
                    properties
                        .into_iter()
                        .map(|(field, schema)| (field, within_geminis_subset(schema)))
                        .collect(),
                ),
                other => other,
            }
        } else {
            within_geminis_subset(value)
        };
        kept.insert(name, narrowed);
    }
    Value::Object(kept)
}

/// Fold every frame of a streamed exchange into the one response it is.
///
/// # One exchange is one response, and that is the whole reason this exists
///
/// A streamed `functionCall` and the `finishReason` that ends the same
/// exchange **arrive in different frames**. Measured 2026-09-05 against
/// `gemini-3.6-flash`: the call came back complete in frame 1 with no finish
/// reason, and frame 2 carried `finishReason: "STOP"` beside an empty text
/// part. A reader that mapped each frame as it arrived would map frame 1 as
/// [`ModelResponse::Calls`] and frame 2 as [`ModelResponse::Stopped`] and
/// report two answers to one question — so the frames are folded first and
/// [`response_from`] is called **once**, on the fold.
///
/// That also preserves the ordering `provider-client` recorded as
/// load-bearing. Its finding was that tool calls must be read before the
/// finish reason, because a non-streamed tool-call response carries
/// `finishReason: "STOP"` alongside the call; the stream widens the same trap
/// across two frames rather than removing it, and folding closes both at
/// once because [`response_from`] still reads the calls first.
///
/// # A `functionCall` is taken whole because the contract sends it whole
///
/// **Measured, not assumed.** The recorded tool-call stream delivered
/// `{"functionCall": {"name": "fs.read", "args": {"path": "notes.txt"},
/// "id": "call_1605341"}, "thoughtSignature": "…"}` as one complete part in
/// one frame — the arguments a finished JSON object, the id beside them, the
/// signature on the same part. **No `functionCall` was ever split across
/// frames**, so nothing here accumulates partial arguments and no second JSON
/// parser exists to go wrong. If that ever stops being true it stops being
/// true loudly, at the `serde` boundary, rather than quietly producing a call
/// with truncated arguments.
///
/// # Why concatenating the parts is the entire fold
///
/// [`response_from`] already collects every `functionCall` part into
/// [`ModelResponse::Calls`] and already joins every text part into one
/// string. So a fold that concatenates each frame's parts, in arrival order,
/// into a single candidate gives that function exactly the input it would
/// have had from a non-streamed response of the same content — which is why
/// there is one mapping here rather than two, and why the three recorded
/// fixtures keep their meaning: each is a stream of one frame.
///
/// A `thoughtSignature` needs no special handling for the same reason. It
/// rides on the part that carries it — on the `functionCall` part in the
/// tool-call stream, and on an empty-text final part in the text stream, both
/// measured — and moving the parts moves the signatures with them.
///
/// # The usage is the last frame's, because the counts are cumulative
///
/// Every frame carries a full `usageMetadata` and the counts **grow**:
/// measured across a three-frame text stream, `candidatesTokenCount` ran 13,
/// 15, 15 while `promptTokenCount` held at 13 and `thoughtsTokenCount` at
/// 183. So the last frame's metadata is the exchange's total. **Summing them
/// would multiply the prompt count by the number of frames** — the shape of
/// error ADR-0012 D7 exists to prevent, in the direction that over-reports
/// rather than under-reports.
///
/// The `finishReason` is the last one any frame carried, and the
/// `modelVersion` the last one, for the same reason: a later frame is a later
/// statement about the same exchange.
#[must_use]
pub fn fold(frames: &[wire::Response]) -> wire::Response {
    let mut parts: Vec<wire::Part> = Vec::new();
    let mut finish_reason: Option<String> = None;
    let mut usage_metadata: Option<wire::UsageMetadata> = None;
    let mut model_version: Option<String> = None;

    for frame in frames {
        if let Some(usage) = frame.usage_metadata {
            usage_metadata = Some(usage);
        }
        if let Some(version) = frame.model_version.clone() {
            model_version = Some(version);
        }
        // The first candidate, which is the one this client reads -- the same
        // choice `response_from` documents.
        let Some(candidate) = frame.candidates.first() else {
            continue;
        };
        if let Some(reason) = candidate.finish_reason.clone() {
            finish_reason = Some(reason);
        }
        if let Some(content) = candidate.content.as_ref() {
            parts.extend(content.parts.iter().cloned());
        }
    }

    wire::Response {
        candidates: vec![wire::Candidate {
            content: Some(wire::Content {
                role: wire::ROLE_MODEL.to_owned(),
                parts,
            }),
            finish_reason,
        }],
        usage_metadata,
        model_version,
    }
}
