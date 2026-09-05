// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The mapping between `zaru-core`'s ports and Gemini's wire shapes, both
//! ways.
//!
//! # Why this is its own module with no I/O in it
//!
//! Everything here is a pure function from one value to another. That is what
//! lets a recorded exchange — a real response body captured once and scrubbed
//! — drive both directions offline, on a CI runner with no key and no
//! network. [Testing] forbids a check calling a provider, and a mapping that
//! could only be exercised through a socket would be a mapping nothing
//! checks.
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
use zaru_core::tool_call::{ModelRequest, ModelResponse, TokenUsage, ToolRequest};

/// Build a request body from what the loop handed the model.
///
/// # Errors
///
/// [`GeminiFailure::ToolSchemaUnreadable`] when a tool descriptor's parameter
/// schema is not JSON. ADR-0011 D1 declares no argument shapes, so the schema
/// is whichever surface owns the tool — and a schema that is not JSON is that
/// surface's defect rather than the user's or the provider's.
pub fn request_from(request: &ModelRequest<'_>) -> Result<wire::Request, GeminiFailure> {
    let mut contents = Vec::with_capacity(2);

    // The prompt. `Prompt` can only be built from `Redacted`, which can only
    // be built by a `Redactor` -- so ADR-0008 clause 6's guarantee reaches
    // this line through the type system rather than through a call somebody
    // remembered to make. Nothing here redacts, and nothing here needs to.
    contents.push(wire::Content {
        role: wire::ROLE_USER.to_owned(),
        parts: vec![wire::Part::Text {
            text: request.prompt.as_str().to_owned(),
        }],
    });

    // This turn's tool results, if any, as one further `user` turn. Not
    // `"tool"` and not `"function"`: Gemini 2.0 and 2.5 accepted those and
    // the current API does not.
    //
    // `ToolResult::content` is `Redacted` too, which is the second half of
    // clause 6's type gate -- a tool's output reaches a model through here
    // and not through a prompt.
    if !request.results.is_empty() {
        contents.push(wire::Content {
            role: wire::ROLE_USER.to_owned(),
            parts: request
                .results
                .iter()
                .map(|result| wire::Part::FunctionResponse {
                    function_response: wire::FunctionResponse {
                        // Echoed, never regenerated. Google's guidance is
                        // "Include this exact `id` in your `functionResponse`
                        // so the model can accurately map your result back".
                        id: (!result.id.is_empty()).then(|| result.id.clone()),
                        // The loop's `ToolResult` carries no name -- it
                        // correlates by id, which is the provider's own
                        // identifier. The API wants a name, so the id is the
                        // best-known name here; a loop that later carries the
                        // tool name through `ToolResult` would improve this,
                        // and that is `zaru-core`'s to decide rather than
                        // this client's to invent.
                        name: result.id.clone(),
                        response: serde_json::json!({
                            "content": result.content.as_str(),
                            "failed": result.failed,
                        }),
                    },
                })
                .collect(),
        });
    }

    let mut declarations = Vec::with_capacity(request.tools.len());
    for tool in request.tools {
        let parameters: Value = serde_json::from_str(&tool.parameters).map_err(|error| {
            GeminiFailure::ToolSchemaUnreadable {
                tool: tool.name.clone(),
                parser: error.to_string(),
            }
        })?;
        declarations.push(wire::FunctionDeclaration {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters,
        });
    }

    Ok(wire::Request {
        contents,
        // One `Tool` entry carrying every declaration, rather than one entry
        // per tool. Both are accepted; one entry is what Google's own
        // examples show, and a client that sent N entries would be making a
        // choice the documentation does not.
        tools: if declarations.is_empty() {
            Vec::new()
        } else {
            vec![wire::Tool {
                function_declarations: declarations,
            }]
        },
    })
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
            wire::Part::FunctionCall { function_call } => Some(ToolRequest {
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
            wire::Part::Text { .. } | wire::Part::FunctionResponse { .. } => None,
        })
        .collect();

    if !calls.is_empty() {
        return Ok(ModelResponse::Calls { calls, tokens });
    }

    let text: String = parts
        .iter()
        .filter_map(|part| match part {
            wire::Part::Text { text } => Some(text.as_str()),
            wire::Part::FunctionCall { .. } | wire::Part::FunctionResponse { .. } => None,
        })
        .collect();

    let finish = candidate.finish_reason.as_deref().unwrap_or_default();
    if finish == wire::FINISH_STOP && !text.is_empty() {
        return Ok(ModelResponse::Text { text, tokens });
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

/// ADR-0012 D7's two quantities, or zero when the provider reported none.
///
/// A response with no `usageMetadata` reports zeroes rather than refusing.
/// That is the one place this module lets a missing datum become a number,
/// and it is bounded: [`TokenUsage`] has no way to express "unreported", and
/// inventing one would be widening `zaru-core`'s port from inside a provider
/// client -- which is a stop. `Provider::usage` answers `None` before the
/// first exchange, which is where "nothing has been reported" is expressible,
/// and the gap is raised on ADR-0012 rather than closed here.
fn usage_from(metadata: Option<wire::UsageMetadata>) -> TokenUsage {
    let metadata = metadata.unwrap_or_default();
    TokenUsage {
        prompt: metadata.prompt_token_count,
        completion: metadata.candidates_token_count,
    }
}
