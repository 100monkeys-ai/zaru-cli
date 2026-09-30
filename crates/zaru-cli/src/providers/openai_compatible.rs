// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D3's `openai-compatible` kind, over the chat-completions API.
//!
//! # What this kind is, and why it is not the `ollama` client with a flag
//!
//! D3 gives it "everything OpenAI-shaped — vLLM, LM Studio, most gateways",
//! which is the widest of the five kinds and the only one that is a *shape*
//! rather than a vendor. The `ollama-client` arc measured on 2026-09-14 that
//! the two differ at the wire — "a tool call's arguments arriving as an object
//! where the compatible kind's arrive as a string" — and this arc measured
//! that the difference is larger still: on this API the arguments are a string
//! that **may arrive in pieces**, and no amount of configuration on the
//! `ollama` client would make it read one.
//!
//! # Two real servers, and they disagree with each other
//!
//! Both measured on 2026-09-14 on the development machine, both a real
//! provider's own compatible surface and neither a fake of one:
//!
//! - **Ollama's `/v1/chat/completions`** sends a tool call **whole in one
//!   frame** — `id`, `type`, `name` and the complete `arguments` string
//!   together — then `finish_reason` on the next frame, then usage on a frame
//!   whose `choices` is empty, then `data: [DONE]`. Its head frame carries
//!   `"content":""`.
//! - **`llama-server` from the same install, under `--jinja`**, sends the
//!   identical call as **thirteen frames**: the first carries the identity and
//!   `"arguments":"{"`, and the next twelve carry `{"index":0,"function":
//!   {"arguments":"city"}}` and nothing else. Its head frame carries
//!   `"content":null`.
//!
//! The two are the same binary on the same weights: Ollama runs it with
//! `--no-jinja --chat-template chatml` and parses the call on its Go side.
//! **A client written against either alone would be wrong about the other**,
//! and both are servers a user of this kind will point it at.
//! [`map::fold`] is where that is resolved, in one line.
//!
//! # The key is optional, which is new in this workspace
//!
//! `gemini` needs a key and `ollama` takes none; this kind is the first where
//! the answer is "it depends on the endpoint", because D3's own list spans a
//! vLLM on a laptop and a hosted gateway. So
//! [`crate::providers::selection::KeyUse::Optional`] is what this kind
//! declares: the kind is **selected** by a configured endpoint, and a key is
//! **sent** when this machine holds one, as `Authorization: Bearer`.
//!
//! Measured, with a live control: Ollama's `/v1` answers 200 to a request
//! carrying `Authorization: Bearer sk-not-a-real-key`, so a key sent to a
//! server that wants none costs nothing; and `llama-server --api-key` answers
//! `401 {"error":{"message":"Invalid API Key",…}}` to a request with none and
//! serves the same request when the right bearer is sent, so a key withheld
//! from a server that wants one is a refusal the reader can act on.
//!
//! # The key, and where it is put
//!
//! In the [`crate::credentials`] store as [`Secret`], attached to exactly one
//! place: the `Authorization` header. **Never a query string** — a URL reaches
//! every proxy log and every message that quotes a request. Because the key is
//! in the store, [`crate::redaction::HeldSecrets`] already carries it, so
//! ADR-0008 clause 6 reaches a provider key here without a second seam.
//!
//! # No stream contract enters `zaru-core`, for the fourth time
//!
//! `gemini-streaming` wrote none, `ollama-client` wrote none, and neither does
//! this. [`Model`] and [`Provider`] are satisfied as they stand and
//! [`zaru_core::iteration::Generator`] is reached through the existing generic
//! adapter. A shape with three providers behind it would still be a shape
//! chosen by an implementation.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

pub mod endpoint;
pub mod failure;
pub mod map;
pub mod wire;

#[cfg(test)]
mod tests;

pub use endpoint::{CHAT_PATH, Endpoint};
pub use failure::{DETAIL_WITHHELD, OpenAiCompatibleFailure};

use crate::credentials::{Alias, Secret};
use crate::providers::capability::ProviderCapabilities;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::kind::ProviderKind;
use crate::providers::port::Provider;
use crate::providers::resolution::ModelId;
use crate::providers::sse;
use crate::providers::usage::TokenUsage;
use std::sync::Mutex;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// The header a key is presented in.
///
/// The API family's own documented form, and the only place this client puts
/// the key.
pub const AUTHORIZATION_HEADER: &str = "authorization";

/// What an `Authorization` value is prefixed with.
pub const BEARER_PREFIX: &str = "Bearer ";

/// [ADR-0012] D3's `openai-compatible` provider, and `zaru-core`'s model
/// behind it.
///
/// # `Debug` is derived, and that is safe because of what the fields are
///
/// Every field either redacts itself or carries no secret: [`Secret`]'s
/// `Debug` is hand-written to print a marker, and an endpoint, a model
/// identifier and an alias are all types that refuse a credential-shaped value
/// at construction. An outside check asserts the whole rendering is free of
/// the key by value and by ASCII core.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub struct OpenAiCompatibleClient {
    endpoint: Endpoint,
    configured: ProviderEndpoint,
    model: ModelId,
    alias: Alias,
    /// **`None` is an ordinary, supported state**, unlike the `gemini` client
    /// where a key is the reason the client exists. A local server needs none.
    key: Option<Secret>,
    /// How large this endpoint's window is, and **`None` is an ordinary state
    /// here too**: this kind has no default window for the same reason it has
    /// no default endpoint. `require_context_size` refuses on it.
    context_tokens: Option<u64>,
    http: reqwest::Client,
    /// Where the answer's text goes as it arrives, when anything is watching.
    /// See `providers::gemini` for why a channel rather than a borrowed sink.
    deltas: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// What the last exchange cost, for [`Provider::usage`].
    last: Mutex<Option<(u64, u64)>>,
    /// The bytes-per-token ratio this session has learned from the
    /// provider's counts, shared with the session's context. See
    /// [`crate::providers::capacity::Calibration`].
    calibration: crate::providers::capacity::Calibration,
}

impl OpenAiCompatibleClient {
    /// A client for `endpoint`, serving `model`.
    ///
    /// `key` is `None` for an endpoint that wants none, which is the ordinary
    /// case for a local server.
    ///
    /// # Errors
    ///
    /// [`OpenAiCompatibleFailure::Unavailable`] when the HTTP client cannot be
    /// built at all — a machine with no usable TLS backend, which is
    /// environmental and not the user's.
    pub fn new(
        endpoint: ProviderEndpoint,
        model: ModelId,
        alias: Alias,
        key: Option<Secret>,
        context_tokens: Option<u64>,
    ) -> Result<Self, OpenAiCompatibleFailure> {
        // Built through `crate::web::client::build`, which is the one place
        // this workspace builds an HTTP client -- so this client, the `gemini`
        // one, the `ollama` one and `web.fetch` cannot drift about cookies,
        // TLS and redirects. What this caller differs on is passed as an
        // argument: its own timeout, and `reqwest`'s default redirect policy.
        let http = crate::web::client::build(
            crate::providers::transport::EXCHANGE_TIMEOUT,
            reqwest::redirect::Policy::default(),
        )
        .map_err(|error| OpenAiCompatibleFailure::Unavailable {
            code: None,
            detail: error.detail().to_owned(),
            retry_after: None,
        })?;
        Ok(Self {
            endpoint: Endpoint::new(&endpoint),
            configured: endpoint,
            model,
            alias,
            key,
            context_tokens,
            http,
            deltas: Mutex::new(None),
            last: Mutex::new(None),
            calibration: crate::providers::capacity::Calibration::starting(),
        })
    }

    /// The ratio this client estimates requests at and learns into, shared.
    #[must_use]
    pub fn calibration(&self) -> crate::providers::capacity::Calibration {
        self.calibration.clone()
    }

    /// What this client's tool surface costs, in bytes as it is sent.
    ///
    /// # ADR-0013's window is read against a request, and this is the rest of
    /// one
    ///
    /// The context the harness measures is the prompt. What reaches the
    /// provider is the prompt **and** every tool declaration, on every
    /// exchange -- and a window is what the provider measures the whole of
    /// that against. Measured 2026-09-14 from the release binary against a
    /// local Ollama through a logging proxy: the first exchange of a session
    /// put **1,967 bytes** on the wire, of which **231** were message content
    /// and the rest the seven tool declarations, and the provider reported
    /// **465** prompt tokens. A count over the message content alone is
    /// therefore *below* the provider's own, which is the direction that
    /// overflows a window in silence.
    ///
    /// So this number reaches
    /// [`Context::reserved`](zaru_core::context::Context::reserved), where it
    /// is on every whole-context measurement and on no single exchange's.
    ///
    /// **Per kind, because the wire shape is per kind.** It is measured
    /// through this client's own `tools_of`, so it is the bytes this client
    /// sends rather than a guess made from the descriptors.
    ///
    /// # Errors
    ///
    /// The mapping failure a request carrying these tools would raise, so a
    /// schema this harness cannot map is refused at configuration time rather
    /// than on the first exchange.
    pub fn tool_surface_bytes(
        &self,
        descriptors: &[zaru_core::tool_call::ToolDescriptor],
    ) -> Result<u64, OpenAiCompatibleFailure> {
        let tools = map::tools_of(descriptors)?;
        let rendered = serde_json::to_string(&tools).unwrap_or_default();
        Ok(rendered.len() as u64)
    }

    /// Send the answer's text to `sender` as each frame of it arrives.
    pub fn stream_deltas_to(&self, sender: tokio::sync::mpsc::UnboundedSender<String>) {
        match self.deltas.lock() {
            Ok(mut slot) => *slot = Some(sender),
            Err(poisoned) => *poisoned.into_inner() = Some(sender),
        }
    }

    /// The model this client serves.
    #[must_use]
    pub const fn model(&self) -> &ModelId {
        &self.model
    }

    /// The alias the key is stored under, which every refusal names.
    #[must_use]
    pub const fn alias(&self) -> &Alias {
        &self.alias
    }

    /// The key as this client would send it, or the empty string when it holds
    /// none.
    ///
    /// Used for redaction, where "withhold nothing" is the right behaviour for
    /// a client with no secret — see
    /// [`OpenAiCompatibleFailure::redacted_detail`].
    fn key_for_redaction(&self) -> &str {
        self.key
            .as_ref()
            .map_or("", crate::credentials::Secret::expose_for_dispatch)
    }

    /// One exchange: send a request, read the stream, fold it into a response.
    ///
    /// # Errors
    ///
    /// Every variant of [`OpenAiCompatibleFailure`]; see that type for what
    /// each means and which [ADR-0016] class it takes.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    pub async fn exchange(
        &self,
        request: &ModelRequest<'_>,
        attempt: crate::providers::resilience::Attempt,
    ) -> Result<ModelResponse, OpenAiCompatibleFailure> {
        // Scoped so the guard is dropped before the first `.await`: a
        // `std::sync::MutexGuard` is `!Send` and `Model::respond` returns a
        // `Send` future, so holding one across an await would not compile.
        let body = map::request_from(request, self.model.as_str())?;
        // ADR-0036 D1, before any network I/O: the whole native request, the
        // model's own prior turns and every tool result included. A window
        // nobody stated never reaches here from the composition --
        // `require_context_size` refuses it by name before a loop starts,
        // which is this kind having no default -- so `None` is the one case
        // with nothing to measure against, and it is refused upstream.
        let sent_bytes = match self.context_tokens {
            Some(window) => crate::providers::capacity::preflight(
                &body,
                window,
                &self.calibration,
                request.turn,
            )
            .map_err(OpenAiCompatibleFailure::ContextWindowExceeded)?,
            None => crate::providers::capacity::request_bytes(&body),
        };

        // What is left of the exchange's ceiling, so a retry never extends it.
        // See `providers::resilience`.
        let mut sending = self
            .http
            .post(self.endpoint.chat_url())
            .timeout(attempt.budget);
        // **The one place the key is attached**, and a header rather than a
        // query string: a URL lands in proxy logs and in every message that
        // quotes a request. A client holding no key sends no header at all
        // rather than an empty one -- measured 2026-09-14, `llama-server`
        // answers 401 to both, but an empty bearer is a value this harness
        // would have invented.
        if let Some(key) = self.key.as_ref() {
            sending = sending.header(
                AUTHORIZATION_HEADER,
                format!("{BEARER_PREFIX}{}", key.expose_for_dispatch()),
            );
        }

        let mut response = sending.json(&body).send().await.map_err(|error| {
            // **User-correctable rather than environmental**, which is the
            // `ollama` client's reading applied here. See
            // `providers::openai_compatible::failure` for why this kind is
            // where that argument is hardest and how it is resolved.
            OpenAiCompatibleFailure::Unreachable {
                endpoint: self.configured.clone(),
                // **The whole chain, not `reqwest`'s top-level sentence.**
                // See `failure::transport_detail`: `to_string()` alone gives
                // "error sending request for url (…)" and drops "Connection
                // refused" three links below it, which is the only part a
                // reader can act on.
                detail: failure::transport_detail_within(
                    &error,
                    crate::providers::transport::EXCHANGE_TIMEOUT,
                ),
            }
        })?;

        let status = response.status();

        // **A failure arrives as an ordinary response, not as frames --
        // sometimes.** Measured 2026-09-14 on both servers: a rejected key
        // answered 401, an unknown model 404 and a malformed body 400, each a
        // plain JSON object with `content-type: application/json` *despite* the
        // request having asked for a stream. So the body is taken whole here
        // and the frame reader never sees it.
        //
        // That is the whole of the story for the `gemini` and `ollama` clients.
        // It is **not** the whole of it here: see the error-frame branch below.
        if !status.is_success() {
            let retry_after = crate::providers::resilience::retry_after_of(response.headers());
            let bytes =
                response
                    .bytes()
                    .await
                    .map_err(|error| OpenAiCompatibleFailure::Unavailable {
                        code: Some(status.as_u16()),
                        detail: failure::transport_detail_within(
                            &error,
                            crate::providers::transport::EXCHANGE_TIMEOUT,
                        ),
                        retry_after: None,
                    })?;
            return Err(OpenAiCompatibleFailure::from_status(
                status.as_u16(),
                &bytes,
                self.model.as_str(),
                &self.alias,
                self.key_for_redaction(),
            )
            .with_retry_after(retry_after));
        }

        // --- The stream, read as it arrives ------------------------------
        //
        // `chunk()` rather than `bytes_stream()`: the first carries no feature
        // gate and the second is behind `stream`, so reading the body
        // incrementally costs this workspace no feature, no manifest row and
        // no lock delta.
        let mut frames = sse::Frames::new();
        let mut received: Vec<wire::Chunk> = Vec::new();
        let mut bytes = 0usize;

        loop {
            // Each piece is read under the stall clock, which restarts at
            // every byte. See `providers::resilience::within_stall`.
            let chunk = crate::providers::resilience::within_stall(attempt.stall, response.chunk())
                .await
                .map_err(|stalled| OpenAiCompatibleFailure::Stalled {
                    silent_for: stalled.silent_for,
                })?
                .map_err(|error| OpenAiCompatibleFailure::Unreachable {
                    endpoint: self.configured.clone(),
                    detail: failure::transport_detail_within(
                        &error,
                        crate::providers::transport::EXCHANGE_TIMEOUT,
                    ),
                })?;
            let Some(chunk) = chunk else { break };
            bytes += chunk.len();
            for payload in frames.feed(&chunk) {
                self.absorb(&payload, bytes, &mut received)?;
            }
        }
        if let Some(payload) = frames.finish() {
            self.absorb(&payload, bytes, &mut received)?;
        }

        let saw_a_choice = received.iter().any(|frame| !frame.choices.is_empty());
        let answer = map::fold(&received);
        let mapped = map::response_from(&answer, bytes, saw_a_choice)?;
        let usage = (mapped.tokens().prompt, mapped.tokens().completion);
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(usage),
            Err(poisoned) => *poisoned.into_inner() = Some(usage),
        }
        // The provider's count for the request just sent is the truth about
        // it, and the next estimate is made from it.
        self.calibration.learn(sent_bytes, usage.0);

        Ok(mapped)
    }

    /// One frame's payload, parsed and handed on.
    ///
    /// # The three things a payload can be, and only one of them is a frame
    ///
    /// `[DONE]` is the sentinel and is discarded — it is not JSON and a client
    /// that handed it to `serde_json` would report a defect at the end of
    /// every successful stream.
    ///
    /// **A frame carrying an error is a failure, and this is the branch no
    /// published documentation prepares a client for.** Measured 2026-09-14
    /// against `llama-server`, reproduced twice: after `HTTP 200` and eight
    /// good frames, `data: {"error":{"code":500,"message":"The model produced
    /// output that does not match the expected peg-native format","type":
    /// "server_error"}}` and end of body, with no sentinel. Raising it here
    /// rather than folding what came before is the whole point: a tool call
    /// whose arguments are `{"city": "Par` is not a smaller answer, it is a
    /// wrong one, and the loop would act on it.
    ///
    /// **It is read off [`wire::Chunk::error`] rather than by a second parse,
    /// and a check is why.** The second-parse version was written first,
    /// tried only after a chunk failed to deserialise — and the chunk never
    /// fails: `serde` ignores unknown fields, so an error frame parses
    /// perfectly as an empty one. That wire type's field documents the whole
    /// measurement.
    fn absorb(
        &self,
        payload: &str,
        bytes: usize,
        received: &mut Vec<wire::Chunk>,
    ) -> Result<(), OpenAiCompatibleFailure> {
        if payload.trim() == map::DONE {
            return Ok(());
        }
        let frame: wire::Chunk = serde_json::from_str(payload).map_err(|parser| {
            OpenAiCompatibleFailure::Unreadable {
                bytes,
                // The length, never the content: an unparsed payload is
                // exactly the one nobody can promise is free of a credential.
                parser: parser.to_string(),
            }
        })?;
        if let Some(error) = frame.error.as_ref() {
            return Err(OpenAiCompatibleFailure::StreamFailed {
                // Not read from the envelope, which types `code` two different
                // ways on the two measured servers -- see `wire::ErrorBody`.
                // The frame arrived inside a 200, so there is no status to
                // carry and the class is the one a server's own mid-answer
                // failure takes.
                code: None,
                detail: OpenAiCompatibleFailure::redacted_detail(
                    &error.message,
                    self.key_for_redaction(),
                ),
            });
        }
        // Handed on **here**, as the frame is read, which is the whole
        // difference a stream makes to a person waiting.
        self.hand_on(&frame);
        received.push(frame);
        Ok(())
    }

    /// Hand a frame's text fragment to whatever is painting.
    fn hand_on(&self, frame: &wire::Chunk) {
        for choice in &frame.choices {
            let Some(text) = choice
                .delta
                .content
                .as_deref()
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            let slot = match self.deltas.lock() {
                Ok(slot) => slot,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(sender) = slot.as_ref() {
                drop(sender.send(text.to_owned()));
            }
        }
    }
}

impl Provider for OpenAiCompatibleClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiCompatible
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        &self.configured
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // Streaming: true. This client asks for `"stream": true` and reads SSE
        // frames; it has no non-streamed path at all.
        //
        // Tool calling: true, measured against both servers on 2026-09-14 --
        // each returned a `tool_calls` delta for a declared function.
        //
        // Token accounting: true, because a frame carrying `usage` arrives when
        // `stream_options.include_usage` asks for one, which this client always
        // does.
        //
        // **This is a statement about the kind, not about an endpoint, and that
        // is the honest reading of a descriptor for a kind that spans every
        // OpenAI-shaped server there is.** A particular gateway may ignore
        // `stream_options`, in which case this client reports `0 + 0` rather
        // than inventing a number -- measured against Ollama's `/v1` with the
        // field omitted, which sends no usage frame at all. The alternative is
        // a probe at configuration time, which ADR-0012's selection rule
        // deliberately does not do.
        //
        // **This moves no clause of ADR-0012.** Clause 2 asks for a streaming
        // tool-calling exchange against a *stub* for *each of five* kinds;
        // three of five now have a client and two have none.
        //
        // Context window: `provider.openai-compatible.context_tokens` and
        // **no default**, which is this kind's own reading of D5 reached a
        // second time. The kind spans vLLM on a laptop, LM Studio,
        // llama.cpp, Ollama's own `/v1` and every hosted gateway, whose
        // windows differ by three orders of magnitude with no majority and
        // no convention -- so a default would be one vendor's number painted
        // on all of them, which is the argument this client already accepted
        // for its endpoint. Absent the key the descriptor says `None` and
        // `require_context_size` refuses before a loop starts.
        ProviderCapabilities::declared(true, true, true, self.context_tokens)
    }

    fn usage(&self) -> Option<TokenUsage> {
        // `None` before the first exchange, `Some` after one. A client that had
        // made no request and reported a zero would be inventing a datum.
        let slot = match self.last.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        slot.map(|(prompt, completion)| TokenUsage::counted(prompt, completion))
    }
}

impl Model for OpenAiCompatibleClient {
    fn capabilities(&self) -> Capabilities {
        // One statement, read twice. `From` rather than a second literal, so a
        // client that stops calling tools cannot say so in one place and not
        // the other.
        Provider::capabilities(self).into()
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // The one place `OpenAiCompatibleFailure` becomes `PortFailure`. The
        // port carries a sentence and nothing else, so the class ADR-0016 puts
        // this failure in is lost here -- which is right for `zaru-core`, whose
        // loop has no taxonomy, and is why `exchange` is public: the command
        // surface classifies the typed failure, and only the loop sees the
        // flattened one.
        //
        // **One attempt, under the built-in stall and the whole ceiling.** A
        // client driven on its own is not retried: a turn's retries are
        // `providers::resilience::Resilient`'s, which the composition wraps
        // around `ProviderClient` with the configured policy.
        self.exchange(request, crate::providers::resilience::Attempt::built_in())
            .await
            .map_err(|failure| PortFailure::new(failure.to_string()))
    }
}
