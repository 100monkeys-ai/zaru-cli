// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The OpenAI chat-completions shapes this client sends and reads.
//!
//! # Every `Option` and every `default` here was earned by a frame
//!
//! Two real servers were measured on 2026-09-14 — Ollama's OpenAI-compatible
//! surface at `/v1/chat/completions`, and `llama-server` from the same
//! install under `--jinja` — and they disagree about more of this shape than
//! a reader of the published documentation would expect. Each leniency below
//! names the frame that forced it, because a `#[serde(default)]` nobody can
//! point at a body for is a field this client guessed about.
//!
//! # The difference from [`crate::providers::ollama::wire`], in one field
//!
//! [`CalledFunction::arguments`] is a **`String`**. Ollama's own `/api/chat`
//! sends an **object** there, which is why that module's field is a
//! [`serde_json::Value`] and why its map re-serialises. Here the arguments
//! arrive as text and leave as text: `zaru-core`'s
//! [`ToolRequest::arguments`](zaru_core::tool_call::ToolRequest::arguments) is
//! documented opaque and `crate::tools::execute` is what parses it, so this
//! client does no round trip and cannot lose a key ordering or a number's
//! spelling in one.
//!
//! **And the string may arrive in pieces.** Measured the same day: Ollama's
//! `/v1` sends `"{\"city\":\"Paris\",\"unit\":\"c\"}"` complete in one frame,
//! and `llama-server` sends the identical call as thirteen fragments starting
//! `"{"`, `"\""`, `"city"`. Both are this kind. That is what
//! [`super::map::fold`] exists for and it is why [`FunctionDelta::arguments`]
//! is a separate type from [`CalledFunction::arguments`] — one is a fragment
//! and one is whole, and giving them one name is how a client comes to assign
//! where it should append.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What this client sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    /// The resolved model identifier.
    pub model: String,
    /// The conversation, oldest first.
    pub messages: Vec<Message>,
    /// The tool descriptors this turn offers.
    pub tools: Vec<Tool>,
    /// Always `true`. This client has no non-streamed path at all.
    pub stream: bool,
    /// Always `Some`, and see [`StreamOptions`] for what happens when a server
    /// ignores it.
    pub stream_options: StreamOptions,
}

/// The opt-in that makes a server report usage on a streamed response.
///
/// **Usage is not sent unless it is asked for.** Measured 2026-09-14 against
/// Ollama's `/v1`: with `stream_options` omitted the stream runs content
/// frames, then `finish_reason: "stop"`, then `data: [DONE]`, and **no usage
/// frame is sent at all**. With it present the same request gains a frame
/// carrying `prompt_tokens` and `completion_tokens`. So this is always asked
/// for, because [ADR-0012] D7 wants the numbers and a server that is never
/// asked will never volunteer them.
///
/// A server that ignores the field sends no usage frame and this client
/// reports `0 + 0` rather than inventing one. See
/// [`Provider::capabilities`] on [`super::OpenAiCompatibleClient`] for why the
/// descriptor still declares token accounting.
///
/// [`Provider::capabilities`]: crate::providers::Provider::capabilities
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StreamOptions {
    /// Always `true`.
    pub include_usage: bool,
}

/// One turn of the conversation, in either direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: String,
    /// The text.
    ///
    /// **`Option` because `null` is what a real server sends.** Measured
    /// 2026-09-14: `llama-server`'s opening frame is
    /// `{"role":"assistant","content":null}` where Ollama's `/v1` sends
    /// `{"role":"assistant","content":""}`. A `String` with `#[serde(default)]`
    /// deserialises the second and **fails** on the first, which is a whole
    /// provider this client would not have been able to read.
    ///
    /// It is also what an assistant turn carrying only tool calls must send
    /// back, which is the same `null` arriving in the other direction.
    #[serde(default)]
    pub content: Option<String>,
    /// The calls an assistant turn asked for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Which call a `role: "tool"` message answers.
    ///
    /// **The call's id, not the tool's name** — the opposite of
    /// [`crate::providers::ollama::wire::Message::tool_name`], because this
    /// API correlates by id where that one correlates by name. Carried from
    /// the remembered call and never generated; see [`super::map::request_from`]
    /// for why the *pairing* is still by position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A complete tool call, as it is sent back in an assistant turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The provider's own id. Carried, never generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Always `"function"`. `type` is a Rust keyword, hence the rename.
    #[serde(rename = "type")]
    pub kind: String,
    /// What was called and with what.
    pub function: CalledFunction,
}

/// The called function, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalledFunction {
    /// The tool's name.
    pub name: String,
    /// The arguments as JSON **text**. See the module documentation.
    pub arguments: String,
}

/// One tool descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tool {
    /// Always `"function"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The declaration.
    pub function: DeclaredFunction,
}

/// A tool as this harness declares it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredFunction {
    /// The tool's name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// The schema, offered whole. This client narrows nothing, for the reason
    /// [`crate::providers::ollama::map`] gives: the server hands the schema to
    /// the model's own template and imposes no subset, so there is no keyword
    /// to drop and no narrowing to justify.
    pub parameters: Value,
}

/// One `data:` frame of a streamed response.
///
/// Named `Chunk` rather than `Response` because the API calls it
/// `chat.completion.chunk` and because it is emphatically not a response: a
/// single exchange is sixteen of these on one measured stream, and
/// [`super::map::fold`] is what makes them one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Chunk {
    /// Usually one choice; **empty on the usage frame**.
    ///
    /// Measured 2026-09-14 on both servers: the frame carrying `usage` sends
    /// `"choices":[]`. Without the default, the last frame of every successful
    /// stream would fail to parse.
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// Present on the terminal frame and on no other, and only when
    /// [`StreamOptions::include_usage`] asked for it.
    #[serde(default)]
    pub usage: Option<Usage>,
    /// **The server's own failure, arriving as a frame inside a 200 stream.**
    ///
    /// # Why this is a field here rather than a second parse
    ///
    /// It was a second parse first, tried after a chunk failed to
    /// deserialise — and a check caught that the chunk **never** fails.
    /// `serde` ignores unknown fields by default, so
    /// `{"error":{"code":500,"message":…}}` deserialises perfectly happily
    /// into `Chunk { choices: [], usage: None, .. }`: an ordinary,
    /// well-formed, entirely empty frame. The error branch was unreachable
    /// and the recorded failing stream folded into a tool call with truncated
    /// arguments — a **wrong answer**, silently, which is the exact outcome
    /// [`super::super::OpenAiCompatibleFailure::StreamFailed`] exists to
    /// prevent.
    ///
    /// Declaring it is what makes one parse answer both questions, and it is
    /// the honest wire shape besides: this producer really does send a frame
    /// carrying an error where another carries choices.
    /// `#[serde(deny_unknown_fields)]` would have been the other fix and is
    /// refused — both servers send fields this type does not read
    /// (`system_fingerprint`, `object`, `created`, and `llama-server`'s
    /// `timings`), and a gateway adding one more must not break a client.
    #[serde(default)]
    pub error: Option<ErrorBody>,
}

/// One choice's delta.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
    /// What this frame adds.
    ///
    /// **Defaults because the finish frame sends `"delta":{}`** — measured on
    /// both servers, where the frame carrying `finish_reason` carries nothing
    /// else.
    #[serde(default)]
    pub delta: Delta,
    /// `"stop"`, `"tool_calls"`, or absent.
    ///
    /// **It arrives on a frame of its own, after the content it describes.**
    /// That is the same shape the `gemini` client records for `finishReason`
    /// and the `ollama` client for `done_reason`, reached here by measurement
    /// rather than inherited — and it is why [`super::map::response_from`]
    /// reads the calls before the reason.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

/// What one frame adds to one choice.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Delta {
    /// A fragment of the answer's text, or `null`.
    #[serde(default)]
    pub content: Option<String>,
    /// Fragments of the calls this turn is asking for.
    #[serde(default)]
    pub tool_calls: Vec<CallDelta>,
}

/// A fragment of one tool call.
///
/// # Everything but `index` is optional, and a measured frame says why
///
/// `llama-server`'s second frame carries the identity —
/// `{"index":0,"id":"hRJp…","type":"function","function":{"name":"get_weather",
/// "arguments":"{"}}` — and its next eleven carry
/// `{"index":0,"function":{"arguments":"city"}}` and nothing else. So `id`,
/// `kind` and [`FunctionDelta::name`] are absent on every frame after the
/// first, and a type that required them could read the first frame of a real
/// stream and none of the rest.
///
/// **`index` is the correlation and it is the only field that is always
/// there.** It is what [`super::map::fold`] keys on, which is what lets two
/// calls interleave their fragments — and is why a fold that pushed in arrival
/// order would report thirteen calls where a real stream sent one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CallDelta {
    /// Which call of the turn this fragment belongs to.
    #[serde(default)]
    pub index: u32,
    /// The provider's id, on the frame that carries one.
    #[serde(default)]
    pub id: Option<String>,
    /// `"function"`, on the frame that carries one.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// The name and the argument fragment, on a frame that carries either.
    #[serde(default)]
    pub function: Option<FunctionDelta>,
}

/// A fragment of one call's function.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FunctionDelta {
    /// The tool's name, on the frame that carries one.
    #[serde(default)]
    pub name: Option<String>,
    /// **A fragment of the arguments, to be appended** — never the whole,
    /// except on a server that happens to send the whole in one frame.
    ///
    /// Defaults to the empty string because a frame may carry a `name` and no
    /// `arguments` at all, and because appending nothing is exactly the right
    /// thing to do with such a frame.
    #[serde(default)]
    pub arguments: String,
}

/// What the exchange cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Usage {
    /// Tokens in the request.
    #[serde(default)]
    pub prompt_tokens: u64,
    /// Tokens in the answer.
    #[serde(default)]
    pub completion_tokens: u64,
}

/// An error body, in the shape this API family publishes.
///
/// **It is read both from a non-200 response and from inside a 200 stream.**
/// See [`super::OpenAiCompatibleClient::exchange`] for the second, which is
/// the shape no published documentation prepares a client for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ErrorEnvelope {
    /// The error.
    pub error: ErrorBody,
}

/// The error itself.
///
/// # `code` and `param` are deliberately not read, and the reason is measured
///
/// The two servers disagree about `code`'s **type**. Ollama's `/v1` sends
/// `{"message":…,"type":"not_found_error","param":null,"code":null}` and
/// `llama-server` sends `{"code":400,"message":…,"type":
/// "invalid_request_error"}` — a JSON `null` in one and a JSON number in the
/// other. A field typed for either fails to deserialise the other's body, and
/// a client that failed to read an error envelope would report a defect where
/// the server had given it a perfectly good sentence.
///
/// Nothing needs it: the HTTP status is what this client classifies on, and it
/// is already in hand. So `code` and `param` are not declared, which is the
/// narrowest thing that reads both.
///
/// **This is also why [`crate::providers::ollama::wire::ErrorEnvelope`] cannot
/// be reused.** Ollama's native `/api/chat` sends a **flat string** —
/// `{"error":"…"}` — where its own OpenAI surface sends this object. The same
/// server, two APIs, two envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ErrorBody {
    /// The server's own sentence.
    pub message: String,
    /// The server's own classification, where it gives one. `type` is a Rust
    /// keyword, hence the rename.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}
