// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Ollama's `/api/chat` request and response bodies, as types.
//!
//! Every shape here was read off the real endpoint on 2026-09-14 — Ollama
//! v0.34.0 against `llama3.2:3b` — rather than off the documentation, and the
//! places where the two differ are noted on the field that differs. The
//! recorded fixtures beside this module are those exchanges byte for byte.
//!
//! # This is Ollama's own API and not the OpenAI-shaped one
//!
//! [ADR-0012] D3 names `ollama` and `openai-compatible` as two of its five
//! kinds, and Ollama serves both surfaces. **The kind is the one D3 names, so
//! the surface is `/api/chat`.** The difference is not cosmetic: a tool call's
//! `arguments` arrives here as a **JSON object**, where the OpenAI shape
//! delivers a string that the caller has to parse a second time. A client
//! written against one is not a client against the other, which is why
//! `openai-compatible` stays a kind with no client rather than being served by
//! this one.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A `/api/chat` request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    /// Which model answers. [ADR-0012] D1's identifier, resolved by the
    /// resolution table and never built here.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub model: String,
    /// The conversation so far, oldest first.
    pub messages: Vec<Message>,
    /// Every tool the model may ask for.
    ///
    /// Always sent, even when empty, because Ollama treats an absent `tools`
    /// and an empty one identically and sending the field unconditionally
    /// removes a branch that would otherwise have to be justified.
    pub tools: Vec<Tool>,
    /// Always `true`. This client has exactly one request shape.
    pub stream: bool,
    /// Runtime knobs that are not [ADR-0014] configuration.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub options: Options,
    /// Always `false`: the server must not cut the prompt to fit `num_ctx`.
    ///
    /// **Ollama drops the oldest messages in silence by default.** At
    /// `16b4376`, `chatPrompt` (`server/prompt.go`) and
    /// `truncateNativeChatMessages` (`server/routes.go`) drop messages until
    /// the prompt fits `num_ctx` unless the request says `"truncate": false`.
    /// The harness estimates a request in tokens before sending it, and an
    /// estimate can be low; with this field an overflow comes back as the
    /// server's refusal, which names a capacity and is classified as one,
    /// rather than as a model that has forgotten the start of the session.
    pub truncate: bool,
    /// Always `false`: the server must not shift the context while it
    /// generates.
    ///
    /// A context shift, the server's default, discards part of the prompt when
    /// the answer outgrows the window, which is the same silent loss in the
    /// middle of an answer. The harness keeps an eighth of the window for the
    /// answer instead (`providers::capacity::answer_room`).
    pub shift: bool,
}

/// What this client asks the server to do differently from its defaults.
///
/// **Only the thread count, and it is not a configuration key.** It bounds
/// what one exchange may take of the machine the harness is also running on;
/// it names no model, no endpoint and no credential, so it is not a value
/// ADR-0014's layers have anything to say about. A user who wants a different
/// bound has `OLLAMA_NUM_PARALLEL` and the server's own flags, which are the
/// server's business rather than the harness's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Options {
    /// How many CPU threads one exchange may use.
    pub num_thread: u16,
    /// How many tokens of context the server is to serve.
    ///
    /// **Sent because the alternative is two windows that disagree.** Ollama
    /// serves `num_ctx` tokens and silently truncates a longer prompt; its
    /// default is 4,096 whatever the model was trained on, so a harness that
    /// declared a window and did not say it would be compacting against a
    /// number the server had never agreed to. This is
    /// `provider.ollama.context_tokens` as it resolved, which is the same
    /// value [`ProviderCapabilities::context_tokens`] reports — one number,
    /// told to the server and obeyed by the harness, rather than two.
    ///
    /// [`ProviderCapabilities::context_tokens`]: crate::providers::ProviderCapabilities::context_tokens
    pub num_ctx: u64,
}

/// One turn of the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// `user`, `assistant` or `tool`.
    pub role: String,
    /// The turn's text. Empty on an assistant turn that only called tools,
    /// which is what the recorded fixture shows.
    #[serde(default)]
    pub content: String,
    /// What the model asked for, on an assistant turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Which tool a `tool` turn is answering for.
    ///
    /// **The tool's name and not the call's id, and that distinction is the
    /// whole of the `gemini-read-loop` defect** recorded on ADR-0012's
    /// amendments page: that client named each result for the call id, and the
    /// model asked for the same tool again. Ollama's field is `tool_name` and
    /// it takes a name, so this client is built to it from the first line
    /// rather than after a defect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// One tool the model asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The provider's own identifier for this call, such as `call_b86hkyop`.
    ///
    /// Optional because the documentation does not promise it, present on
    /// every exchange recorded. Carried through to `ToolRequest::id` where it
    /// exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// What was asked for.
    pub function: CalledFunction,
}

/// The call's name and arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalledFunction {
    /// Which of the offered tools, by the harness's own name — `fs.read` and
    /// the rest. A dot in a tool name is accepted; measured against the real
    /// endpoint with all seven built-ins offered.
    pub name: String,
    /// The arguments, **as a JSON object rather than as a string**.
    ///
    /// This is where Ollama's own API and the OpenAI shape part company, and
    /// it is why this module exists rather than the `openai-compatible` kind
    /// serving both.
    pub arguments: Value,
    /// Which call this is within the frame, where the server sends it.
    ///
    /// Read and not used. It is recorded here because dropping a field that
    /// the wire carries makes the type a lie about what arrived, and because
    /// a client that one day sends several calls in one frame will want it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// One tool the model may ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tool {
    /// Always `function`.
    #[serde(rename = "type")]
    pub kind: String,
    /// What it is called and what it takes.
    pub function: DeclaredFunction,
}

/// A tool's declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclaredFunction {
    /// The harness's own tool name.
    pub name: String,
    /// What it does, for the model to choose by.
    pub description: String,
    /// A JSON Schema object, offered as [ADR-0011] D1 declares it.
    ///
    /// **Offered whole, unlike the `gemini` client's**, which narrows the
    /// schema in its mapping because Google's subset refuses keywords the
    /// harness's descriptors use. Ollama passes the schema to the model's own
    /// template without a subset of its own, so nothing is dropped and no
    /// narrowing has to be justified here.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub parameters: Value,
}

/// One frame of a streamed `/api/chat` response.
///
/// **Every frame deserialises as a whole `Response`**, exactly as each of the
/// `gemini` client's SSE frames deserialises as a whole response of its own.
/// The framing differs — newline-delimited JSON rather than server-sent events
/// — and the per-frame shape does not.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Response {
    /// The model that answered, as the server names it.
    #[serde(default)]
    pub model: String,
    /// This turn's content so far. Text arrives as a **delta** to be
    /// concatenated, measured across six frames spelling "Hello there, friend."
    #[serde(default)]
    pub message: Option<Message>,
    /// Whether this is the terminal frame.
    #[serde(default)]
    pub done: bool,
    /// Why the model stopped, on the terminal frame.
    ///
    /// **Treated as an open set.** Any value that is not `stop` becomes a
    /// stopped response carrying the server's own word, for the reason the
    /// `gemini` client treats `finishReason` the same way: a closed enum
    /// reports this harness's ignorance as the provider's fault.
    #[serde(default)]
    pub done_reason: Option<String>,
    /// Tokens the prompt occupied, on the terminal frame and on no other.
    #[serde(default)]
    pub prompt_eval_count: Option<u64>,
    /// Tokens the completion occupied, on the terminal frame and on no other.
    ///
    /// **Not cumulative across frames**, which is the opposite of what the
    /// `gemini` client sees and is why that client's reason for taking the
    /// last frame's usage is not repeated here. Both take the last frame's;
    /// there it avoids multiplying the prompt count, here it is the only
    /// frame that carries a count at all.
    #[serde(default)]
    pub eval_count: Option<u64>,
}

/// The body Ollama sends when it refuses.
///
/// **Not a frame.** A 404 for an unknown model and a 400 for a malformed body
/// both arrive as a plain JSON object with `content-type: application/json`,
/// *despite* the request having asked for a stream — measured both ways on
/// 2026-09-14. So a refusal is read from the status and this shape, never from
/// the frame reader, which is the same asymmetry the `gemini` client records.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ErrorEnvelope {
    /// What the server said. Carried verbatim into a failure's detail; it
    /// cannot contain a credential, because no credential is ever sent.
    pub error: String,
}
