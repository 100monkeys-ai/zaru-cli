// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `generateContent` request and response shapes, as Google documents
//! them.
//!
//! # Transcribed from one place, on one date, with the URL in the code
//!
//! Every type here is the wire shape at
//! <https://ai.google.dev/api/generate-content>, read 2026-09-05. Nothing is
//! inferred from an SDK and nothing is copied from another client. A field
//! this module does not carry is a field this client does not send or read,
//! and `serde`'s defaults mean an unknown field in a response is ignored
//! rather than fatal — which is deliberate, because a provider adding a field
//! must not break a harness that does not use it.
//!
//! **`deny_unknown_fields` is exactly wrong here**, and the contrast with
//! [`crate::credentials::store::Record`] is worth stating: that file is ours,
//! and a key nothing reads in it might have been a restriction. This one is
//! Google's, and a key nothing reads in it is Google shipping.
//!
//! # The API this is written against is the one Google marks Legacy
//!
//! `generateContent` is documented under the heading "Gemini Generate Content
//! API (Legacy)". The current surface is `POST /v1beta/interactions`, with
//! `input`, `steps` and `previous_interaction_id` in place of `contents`,
//! `candidates` and a caller-managed history. Building against the legacy one
//! is a deliberate choice recorded on [ADR-0012]'s Status tracking under
//! directive 20: `zaru-core`'s [`Model`](zaru_core::tool_call::Model) port
//! matches `generateContent` field for field — including the carried
//! `functionCall.id`, which is what [`ToolRequest::id`](zaru_core::tool_call::ToolRequest)
//! exists for — and migrating is a later arc's when the record decides.
//!
//! # No streaming
//!
//! `streamGenerateContent` is not here. `Model::respond` is one exchange, the
//! capability descriptor says `streaming: false`, and ADR-0012 D3's streaming
//! concern and its trigger clause 2 stay open. A stream contract with one
//! provider behind it would be a shape chosen by an implementation rather
//! than by a record.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `generateContent` request body.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The conversation so far, oldest first.
    pub contents: Vec<Content>,
    /// The tools the model may ask for. Omitted entirely when there are none,
    /// because an empty `tools` array is not the same request as no `tools`
    /// key and the second is what "this turn offers no tools" means.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
}

/// One turn of the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    /// `"user"` or `"model"`.
    ///
    /// **A function result goes back under `"user"`**, not under `"tool"` or
    /// `"function"`. Gemini 2.0 and 2.5 accepted those two; the current API
    /// does not, and the documented shape puts a `functionResponse` part in a
    /// `"user"` turn. [`ROLE_USER`] is the constant, so the two call sites
    /// that need it cannot drift.
    #[serde(default)]
    pub role: String,
    /// The turn's parts, in order.
    #[serde(default)]
    pub parts: Vec<Part>,
}

/// The role a request's own turns carry.
pub const ROLE_USER: &str = "user";

/// The role the model's turns come back under.
pub const ROLE_MODEL: &str = "model";

/// One piece of a turn.
///
/// Untagged rather than enumerated, because Google's `Part` is a union with
/// one field set and the JSON carries no discriminator. The order of the
/// variants is the order `serde` tries them, and it matters: `Text` last, so
/// a part carrying both a `text` and a `functionCall` — which the API does
/// not document but does not forbid — is read as the call rather than as the
/// prose beside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    /// A tool call the model is asking for.
    #[serde(rename_all = "camelCase")]
    FunctionCall {
        /// The call.
        function_call: FunctionCall,
    },
    /// A tool result the caller is supplying.
    #[serde(rename_all = "camelCase")]
    FunctionResponse {
        /// The result.
        function_response: FunctionResponse,
    },
    /// Text, in either direction.
    Text {
        /// The text.
        text: String,
    },
}

/// A tool call, as the model asks for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    /// The provider's own identifier for this call.
    ///
    /// **`Option` because the field is Google's to send.** The documentation
    /// says the API "now always returns a unique `id` with every
    /// `functionCall`" for Gemini 3 models, and this client carries whatever
    /// arrives rather than asserting the promise: a model that does not send
    /// one produces an empty id, which the mapping surfaces rather than
    /// inventing a number for. Renumbering is the failure
    /// [`ToolRequest::id`](zaru_core::tool_call::ToolRequest) is documented
    /// against — "a provider that correlates results to calls by its own id
    /// and a harness that renumbers them disagree about which result answered
    /// which call".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Which tool.
    pub name: String,
    /// The arguments, as an object.
    #[serde(default)]
    pub args: Value,
}

/// A tool result, as the caller returns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    /// The id of the call this answers, where the call carried one.
    ///
    /// Google's guidance is "Include this exact `id` in your
    /// `functionResponse` so the model can accurately map your result back",
    /// so it is echoed rather than regenerated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Which tool answered.
    pub name: String,
    /// What it produced.
    pub response: Value,
}

/// One entry of the request's `tools` array.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// The declarations this entry carries.
    pub function_declarations: Vec<FunctionDeclaration>,
}

/// One tool, as the model is told about it.
#[derive(Debug, Clone, Serialize)]
pub struct FunctionDeclaration {
    /// The tool's name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// The parameter schema.
    ///
    /// [`ToolDescriptor::parameters`](zaru_core::tool_call::ToolDescriptor) is
    /// an opaque string in `zaru-core`, because ADR-0011 D1 declares no
    /// argument shapes for its seven built-ins. It is parsed as JSON here and
    /// sent as an object, because that is what the API takes; a descriptor
    /// whose parameters are not JSON is a defect of whoever supplied it and is
    /// reported as one.
    pub parameters: Value,
}

/// The `generateContent` response body.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    /// The candidates, of which this client reads the first.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// What the exchange cost. Absent on some error-shaped responses, which
    /// is why it is optional rather than defaulted to zero — a reported zero
    /// and an unreported cost are different facts.
    #[serde(default)]
    pub usage_metadata: Option<UsageMetadata>,
    /// Which model actually answered, where the API says.
    #[serde(default)]
    pub model_version: Option<String>,
}

/// One candidate answer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// The content, absent when the model produced none.
    #[serde(default)]
    pub content: Option<Content>,
    /// Why the model stopped.
    ///
    /// **Treated as an open enum.** `STOP`, `MAX_TOKENS` and `SAFETY` are
    /// documented; the reference page truncates before the full list, which
    /// three separate reads on 2026-09-05 confirmed. So this is a `String`
    /// rather than an enum with an `Other` arm, and any value that is not
    /// `STOP` becomes
    /// [`ModelResponse::Stopped`](zaru_core::tool_call::ModelResponse)
    /// carrying the provider's own word. A closed enum here would turn a
    /// value Google adds into a parse failure, which is the harness reporting
    /// its own ignorance as the provider's fault.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// The `finishReason` that means the model said what it had to say.
pub const FINISH_STOP: &str = "STOP";

/// [ADR-0012](https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction)
/// D7's two quantities, as Google reports them.
///
/// There is no cost field, because Google publishes pricing on a web page
/// rather than on the response. [`Cost`](crate::providers::Cost) is reported
/// by a caller and computed by nothing, so this client reports none.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageMetadata {
    /// Tokens the prompt occupied.
    #[serde(default)]
    pub prompt_token_count: u64,
    /// Tokens the answer occupied.
    #[serde(default)]
    pub candidates_token_count: u64,
    /// Both together, as Google totals them. Read but not relied on: this
    /// client reports the two halves, and
    /// [`TokenUsage`](zaru_core::tool_call::TokenUsage) computes its own
    /// total.
    #[serde(default)]
    pub total_token_count: u64,
}

/// The error envelope every Google API returns, per
/// [AIP-193](https://google.aip.dev/193).
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorEnvelope {
    /// The error itself.
    pub error: ApiError,
}

/// The `error` object AIP-193 defines.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    /// The HTTP status, repeated in the body.
    #[serde(default)]
    pub code: u16,
    /// What went wrong, in Google's words.
    ///
    /// **Rendered to the user and never assumed to be free of the key.** The
    /// client checks it before it is carried anywhere; see
    /// [`super::failure`].
    #[serde(default)]
    pub message: String,
    /// The canonical status name, such as `INVALID_ARGUMENT` or
    /// `PERMISSION_DENIED`.
    #[serde(default)]
    pub status: String,
}
