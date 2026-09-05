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
///
/// # Two variants carry a `thoughtSignature`, and it is not decoration
///
/// A model turn is **resent** to the API on every later round of a turn, and
/// the thinking guide's rule for it is imperative: "You MUST always resend
/// all thought blocks exactly as they were received from the model. You
/// should NOT remove or modify thought blocks from the history, as they
/// contain the signatures required for the model to continue its reasoning."
/// Its note says where they live on this API — "In the `generateContent`
/// API, there are no dedicated thought blocks. Because of this, signatures
/// are metadata that can be attached to any part, such as living inside
/// `functionCall` parts or the final part of a response" — which is why the
/// field is on [`Part::FunctionCall`] and [`Part::Text`] and not on
/// [`FunctionCall`] itself.
///
/// **Measured, not inferred.** A model turn resent without it is refused,
/// HTTP 400 `INVALID_ARGUMENT`, in the API's own words: *"Function call is
/// missing a thought_signature in functionCall parts. This is required for
/// tools to work correctly, and missing thought_signature may lead to
/// degraded model performance."* Eight of eight, 2026-09-05, against
/// `gemini-3.6-flash`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    /// A tool call the model is asking for.
    #[serde(rename_all = "camelCase")]
    FunctionCall {
        /// The call.
        function_call: FunctionCall,
        /// Google's opaque record of the reasoning behind this call.
        ///
        /// Read on the way in and written back unchanged on the way out. See
        /// the enum's own documentation for the rule and the measurement.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    /// A tool result the caller is supplying.
    #[serde(rename_all = "camelCase")]
    FunctionResponse {
        /// The result.
        function_response: FunctionResponse,
    },
    /// Text, in either direction.
    #[serde(rename_all = "camelCase")]
    Text {
        /// The text.
        text: String,
        /// Google's opaque record of the reasoning behind this text.
        ///
        /// The documentation names "the final part of a response" as the
        /// other place a signature rides, so it is carried here too. Nothing
        /// in this client resends a `Text` part today — a turn that answers
        /// is a turn that ends — and the field is here so that a model turn
        /// which mixes prose with a call round-trips whole rather than
        /// losing half of what it must be given back.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought_signature: Option<String>,
    },
    /// A part this client does not understand.
    ///
    /// # Measured, not defensive
    ///
    /// The first real `generateContent` response this client ever received —
    /// recorded 2026-09-05 at `recorded/calls.json` — carried a
    /// `thoughtSignature` beside the `functionCall`, a field the reference
    /// documents nowhere. On that response it rode on the same part object as
    /// the call, so `FunctionCall` matched it.
    ///
    /// **What that sentence used to claim, and why it was wrong.** It said
    /// "and nothing broke". Nothing broke *at parse time*; the field was
    /// silently dropped, because the variant that matched had nowhere to put
    /// it, and dropping it is one third of the defect this arm's neighbour
    /// now carries a field for. A part that matches a variant which cannot
    /// hold all of it is not a part that round-trips, and "the response
    /// parsed" is a weaker statement than it reads as. Corrected 2026-09-05
    /// by the `gemini-read-loop` arc.
    ///
    /// **A part carrying only such a field would have failed every variant,
    /// and an untagged enum with no fallback fails the whole response.** So
    /// one unreadable part would have turned a perfectly good answer into
    /// `Unreadable`, reported as this harness's defect. This arm makes an
    /// unknown part something the mapping skips rather than something that
    /// loses the answer — the same asymmetry `deny_unknown_fields` gets right
    /// on our own file and wrong on somebody else's.
    ///
    /// It is last, so it is tried only after the three that mean something.
    Other(Value),
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
    /// Which tool answered, by the name it was declared under.
    ///
    /// **The reference calls this "Required. The name of the function to
    /// call", and it means the declaration's name rather than any identifier
    /// the caller has to hand.** A client that sent the *call id* here
    /// produced a request the API accepted with HTTP 200 and the model read
    /// as a result from something it had never called: measured 2026-09-05,
    /// the model asked for the same tool again in six runs of eight. That is
    /// the whole of the `gemini-read-loop` defect, and it is why
    /// [`super::map::Answered`] exists — the loop's [`ToolResult`] carries no
    /// name, so the name has to come from the call this client itself
    /// received.
    ///
    /// [`ToolResult`]: zaru_core::tool_call::ToolResult
    pub name: String,
    /// What it produced.
    ///
    /// The reference leaves the shape to the caller — "Callers can use any
    /// keys of their choice that fit the function's syntax to return the
    /// function output, e.g. `output`, `result`, etc." — and that freedom was
    /// measured rather than assumed: the same exchange answers identically
    /// with this client's `content` key and with `output`, eight of eight
    /// each, 2026-09-05. So the key is not what a model reads a result by,
    /// and it is not the thing that was wrong.
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
    /// Tokens the model spent thinking, which are **billed as output**.
    ///
    /// # This field is why the reported completion count is a sum
    ///
    /// Undocumented on the reference page and present on the first real
    /// response this client received: `recorded/calls.json` reports
    /// `promptTokenCount` 54, `candidatesTokenCount` 17,
    /// `thoughtsTokenCount` 62 and `totalTokenCount` **133** — and 54 + 17 is
    /// 71, not 133. The three that add up are prompt, candidates and
    /// thoughts.
    ///
    /// Reporting `candidatesTokenCount` alone as the completion would
    /// under-report what the user is billed by more than four times on that
    /// exchange, which is precisely what [ADR-0012] D7 exists to prevent:
    /// "nobody discovers their spend at the end of a month". So
    /// [`super::map`] reports candidates **plus** thoughts as the completion,
    /// and the arithmetic is checkable rather than asserted —
    /// `TokenUsage::total()` then equals Google's own `totalTokenCount`
    /// exactly, over a recorded response, which a check pins.
    ///
    /// **It is a reading and it is raised rather than settled.** D7 names
    /// "prompt tokens, completion tokens" and knows nothing of a third
    /// quantity; whether a thinking token is a completion token is that
    /// record's to say. Recorded on ADR-0012 under directive 20, open to
    /// Jeshua's veto, and pinned by a check so that deciding it the other way
    /// reddens rather than passing quietly.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[serde(default)]
    pub thoughts_token_count: u64,
    /// Both together and then some, as Google totals them.
    ///
    /// Read and **checked against** rather than reported: it is what makes
    /// the completion sum above a measurement instead of a guess.
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
