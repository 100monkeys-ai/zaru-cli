// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The second provider client in this workspace: [ADR-0012] D3's `ollama`
//! kind.
//!
//! # Why this kind, and why now
//!
//! `provider-client` recorded on 2026-09-05 why `gemini` was first and this
//! was not: "it is the only kind for which a key exists that an agent may use
//! … **no `ollama` is installed on the development machine**". That was a fact
//! about the machine rather than about the record, and ADR-0012 trigger clause
//! 4 — "A local Ollama endpoint completes an iteration loop end to end" — was
//! the record's only clause with nothing at all against it.
//!
//! The `ollama-client` arc installed Ollama v0.34.0 in user space on
//! 2026-09-14, ran it as its own process, and built this client against it. So
//! the blocker was a missing dependency rather than a missing credential, and
//! it was removable by the arc that needed it removed.
//!
//! # This kind needs no credential, and that is the difference that matters
//!
//! Every other provider in this workspace is reached with a secret. This one
//! is not: measured on 2026-09-14, a request to `/api/chat` carrying **no**
//! authorization header of any kind answers HTTP 200. Three things follow, and
//! each is built rather than left implicit.
//!
//! - [`OllamaClient::new`] takes no [`Secret`](crate::credentials::Secret) and
//!   this module imports none. There is no key to attach to a header, no key
//!   to keep out of a URL, and no key for a failure's detail to leak — so
//!   [`failure`] carries the server's own sentence verbatim where the `gemini`
//!   client's must check it first.
//! - [ADR-0007]'s store is untouched by this kind. It is the first provider
//!   here that asks nothing of it.
//! - **The composition cannot select this kind by credential presence**, which
//!   is how it selected the only kind that existed before. That is
//!   [`crate::compose::turn`]'s problem rather than this module's, and the
//!   reading that resolves it is a **proposed** amendment on ADR-0012 rather
//!   than a decision taken here.
//!
//! # One type implements both ports, and `Generator` came free
//!
//! [`Provider`] is the **configured** half — which kind, which endpoint, what
//! it says it can do, what the last request cost. [`Model`] is the **exchange**
//! half. [`OllamaClient`] implements both, and
//! [`ProviderCapabilities`] converts to [`Capabilities`] through the same
//! `From` the `gemini` client uses, so the two `capabilities` methods are one
//! statement read twice.
//!
//! **`iteration::Generator` needed nothing at all.** `zaru-cli`'s
//! [`compose::iterate::Generating`](crate::compose::iterate::Generating) is
//! generic over any [`Model`], so the ruling recorded on ADR-0012 in 2026-09-05
//! — "one implementation satisfies both, and neither trait is widened" — held
//! for the second client without a line being written to make it hold. That is
//! worth saying because it is the cheapest possible outcome of a design
//! decision made a week earlier, and it is evidence the decision was right.
//!
//! # No stream contract entered `zaru-core`, for the third time
//!
//! `Model::respond` is unchanged, `Capabilities` is unchanged, the event enum
//! is unchanged. The NDJSON framing and the fold live entirely in this module,
//! exactly as the SSE framing lives entirely in the other one — so a second
//! streaming client did **not** turn into the shared abstraction that
//! `gemini-streaming` deliberately declined to write. Two clients now stream
//! and `zaru-core` still knows nothing about streaming, which is the strongest
//! evidence available that withholding the contract was correct.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`Model`]: zaru_core::tool_call::Model

pub mod endpoint;
pub mod failure;
pub mod map;
pub mod stream;
pub mod wire;

#[cfg(test)]
mod tests;

pub use endpoint::{DEFAULT_ENDPOINT, Endpoint};
pub use failure::OllamaFailure;

use crate::providers::capability::ProviderCapabilities;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::kind::ProviderKind;
use crate::providers::port::Provider;
use crate::providers::resolution::ModelId;
use crate::providers::usage::TokenUsage;
use std::sync::Mutex;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// [ADR-0012] D3's `ollama` provider, and `zaru-core`'s model behind it.
///
/// # `Debug` is derived, and here that is unremarkable
///
/// The `gemini` client's equivalent carries a paragraph explaining why
/// deriving `Debug` is safe when one field is a key. **This type has no key
/// and therefore no such argument to make**: an endpoint and a model
/// identifier are types that refuse a credential-shaped value at construction,
/// and there is nothing else. The absence is noted so a reader comparing the
/// two does not conclude the check was forgotten.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub struct OllamaClient {
    endpoint: Endpoint,
    configured: ProviderEndpoint,
    model: ModelId,
    /// How many tokens of context this client asks the server for, and the
    /// same number its descriptor declares.
    context_tokens: u64,
    http: reqwest::Client,
    /// Where the answer's text goes as it arrives, when anything is watching.
    ///
    /// Unbounded for the reason the other client's is: the alternative drops
    /// deltas when full, and a dropped delta is text the user never sees in a
    /// pane whose whole purpose is showing the answer arrive.
    deltas: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,
    /// What the last exchange cost, for [`Provider::usage`].
    ///
    /// A `Mutex` rather than a `Cell` because [`Model::respond`] returns
    /// `impl Future + Send`, so the future borrowing `&self` requires
    /// `Self: Sync` and a `Cell` is not.
    last: Mutex<Option<(u64, u64)>>,
    /// The bytes-per-token ratio this session has learned from the server's
    /// counts, shared with the session's context. See
    /// [`crate::providers::capacity::Calibration`].
    calibration: crate::providers::capacity::Calibration,
}

impl OllamaClient {
    /// Build a client for one model, over one HTTP client.
    ///
    /// The `reqwest::Client` is built **once, here**, and reused for every
    /// exchange, so a connection pool survives between turns rather than being
    /// rebuilt per request.
    ///
    /// **Takes no credential**, which is the signature difference from the
    /// `gemini` client and is the whole of why this kind cannot be selected
    /// the way that one is. See the module documentation.
    ///
    /// # Errors
    ///
    /// [`OllamaFailure::Unreachable`] when the HTTP client cannot be built at
    /// all.
    pub fn new(
        endpoint: ProviderEndpoint,
        model: ModelId,
        context_tokens: u64,
    ) -> Result<Self, OllamaFailure> {
        // Built through `crate::web::client::build`, which is the one place
        // this workspace builds an HTTP client -- so this client, the `gemini`
        // one and `web.fetch` cannot drift about cookies, TLS and redirects.
        // What this caller differs on is passed as an argument: its own
        // timeout, and `reqwest`'s default redirect policy.
        let http = crate::web::client::build(
            crate::providers::transport::EXCHANGE_TIMEOUT,
            reqwest::redirect::Policy::default(),
        )
        .map_err(|error| OllamaFailure::Unreachable {
            endpoint: endpoint.clone(),
            detail: error.detail().to_owned(),
        })?;
        Ok(Self {
            endpoint: Endpoint::new(&endpoint),
            configured: endpoint,
            model,
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
    ) -> Result<u64, OllamaFailure> {
        let tools = map::tools_of(descriptors)?;
        let rendered = serde_json::to_string(&tools).unwrap_or_default();
        Ok(rendered.len() as u64)
    }

    /// Send this client's answer text to `sender` as each frame arrives.
    pub fn stream_deltas_to(&self, sender: tokio::sync::mpsc::UnboundedSender<String>) {
        match self.deltas.lock() {
            Ok(mut slot) => *slot = Some(sender),
            Err(poisoned) => *poisoned.into_inner() = Some(sender),
        }
    }

    /// The model this client asks for, as the resolution table resolved it.
    #[must_use]
    pub const fn model(&self) -> &ModelId {
        &self.model
    }

    /// One exchange, as the failure taxonomy sees it.
    ///
    /// Separate from [`Model::respond`] so that the mapping from
    /// [`OllamaFailure`] to [`PortFailure`] happens in one place and this
    /// function can be read as the request it makes.
    ///
    /// # Errors
    ///
    /// [`OllamaFailure`], classified by provenance. See [`failure`].
    pub async fn exchange(
        &self,
        request: &ModelRequest<'_>,
        attempt: crate::providers::resilience::Attempt,
    ) -> Result<ModelResponse, OllamaFailure> {
        // Scoped so the guard is dropped before the first `.await`: a
        // `std::sync::MutexGuard` is `!Send` and `Model::respond` returns a
        // `Send` future, so holding one across an await would not compile.
        let body = map::request_from(request, self.model.as_str(), self.context_tokens)?;
        // ADR-0036 D1, before any network I/O: the whole native request, the
        // model's own prior turns and every tool result included, estimated in
        // tokens against the window this client also sends as `num_ctx`, less
        // the room kept for the answer.
        let sent_bytes = crate::providers::capacity::preflight(
            &body,
            self.context_tokens,
            &self.calibration,
            request.turn,
        )
        .map_err(OllamaFailure::ContextWindowExceeded)?;

        let mut response = self
            .http
            .post(self.endpoint.chat_url())
            // What is left of the exchange's ceiling, so a retry never
            // extends it. See `providers::resilience`.
            .timeout(attempt.budget)
            .json(&body)
            .send()
            .await
            .map_err(|error| OllamaFailure::Unreachable {
                endpoint: self.configured.clone(),
                // **The whole chain, not `reqwest`'s top-level sentence.** See
                // `providers::transport`: `to_string()` gives "error sending
                // request for url (...)" and drops "Connection refused" three
                // links below it. Measured on the release binary before this
                // call changed: a closed port and a hostname that does not
                // resolve printed the same sentence but for the URL, so a
                // reader whose DNS was wrong was told to start a server.
                detail: crate::providers::transport::transport_detail_within(
                    &error,
                    crate::providers::transport::EXCHANGE_TIMEOUT,
                ),
            })?;

        let status = response.status();

        // **A failure arrives as an ordinary response, not as frames.**
        // Measured 2026-09-14 on this endpoint: an unknown model answered 404
        // with `content-type: application/json` *despite* the request having
        // asked for a stream, and a malformed body answered 400 the same way.
        // So the body is taken whole here and the frame reader never sees it.
        if !status.is_success() {
            let retry_after = crate::providers::resilience::retry_after_of(response.headers());
            let bytes = response
                .bytes()
                .await
                .map_err(|error| OllamaFailure::Unavailable {
                    code: status.as_u16(),
                    detail: crate::providers::transport::transport_detail_within(
                        &error,
                        crate::providers::transport::EXCHANGE_TIMEOUT,
                    ),
                    retry_after: None,
                })?;
            return Err(
                OllamaFailure::from_status(status.as_u16(), &bytes, self.model.as_str())
                    .with_retry_after(retry_after),
            );
        }

        // --- The stream, read as it arrives ------------------------------
        //
        // `chunk()` rather than `bytes_stream()`: the first carries no feature
        // gate and the second is behind `stream`, so reading the body
        // incrementally costs this workspace no feature, no manifest row and
        // no lock delta.
        let mut frames = stream::Frames::new();
        let mut received: Vec<wire::Response> = Vec::new();
        let mut bytes = 0usize;

        loop {
            // A stream that stops mid-way is a socket that stopped. For a
            // local server that is the server having died, which the user can
            // act on -- so it reaches `Unreachable` rather than the
            // environmental class a hosted provider's break reaches.
            //
            // Each piece is read under the stall clock, which restarts at every
            // byte. See `providers::resilience::within_stall`.
            let chunk = crate::providers::resilience::within_stall(attempt.stall, response.chunk())
                .await
                .map_err(|stalled| OllamaFailure::Stalled {
                    silent_for: stalled.silent_for,
                })?
                .map_err(|error| OllamaFailure::Unreachable {
                    endpoint: self.configured.clone(),
                    detail: crate::providers::transport::transport_detail_within(
                        &error,
                        crate::providers::transport::EXCHANGE_TIMEOUT,
                    ),
                })?;
            let Some(chunk) = chunk else { break };
            bytes += chunk.len();
            self.absorb(&mut frames, &chunk, bytes, &mut received)?;
        }
        self.absorb_last(&mut frames, bytes, &mut received)?;

        if received.is_empty() {
            return Err(OllamaFailure::Unreadable {
                bytes,
                parser: "the stream carried no frames, which the API does not document as a \
                         successful shape"
                    .to_owned(),
            });
        }

        self.settled(&received, bytes, sent_bytes)
    }

    /// The end of an exchange: fold the frames into one response, keep what
    /// it cost, and learn the ratio from the server's count.
    ///
    /// `bytes` is what the stream carried and `sent_bytes` what the request
    /// carried. Separate from [`Self::exchange`] so a check can hand it the
    /// frames of a recorded answer and read what was learned, with no socket.
    fn settled(
        &self,
        received: &[wire::Response],
        bytes: usize,
        sent_bytes: u64,
    ) -> Result<ModelResponse, OllamaFailure> {
        // One exchange is one response. See `map::fold` for the measurement
        // that makes folding load-bearing rather than tidy.
        let answer = map::fold(received);
        let mapped = map::response_from(&answer, bytes)?;
        let usage = (mapped.tokens().prompt, mapped.tokens().completion);
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(usage),
            Err(poisoned) => *poisoned.into_inner() = Some(usage),
        }
        // The server's count for the request just sent is the truth about it,
        // and the next estimate is made from it.
        self.calibration.learn(sent_bytes, usage.0);

        Ok(mapped)
    }

    /// Take every frame `chunk` completed.
    fn absorb(
        &self,
        frames: &mut stream::Frames,
        chunk: &[u8],
        bytes: usize,
        received: &mut Vec<wire::Response>,
    ) -> Result<(), OllamaFailure> {
        for payload in frames.feed(chunk) {
            let frame = parse_frame(&payload, bytes)?;
            // Handed on **here**, as the frame is read, which is the whole
            // difference a stream makes to a person waiting.
            self.hand_on(&frame);
            received.push(frame);
        }
        Ok(())
    }

    /// Take the frame the body ended without terminating, if there was one.
    ///
    /// Separate from [`Self::absorb`] because it is reached once, after the
    /// last read; folding it into the loop would mean calling
    /// [`stream::Frames::finish`] on every chunk, which would end the stream
    /// at the first read that did not fill a frame.
    fn absorb_last(
        &self,
        frames: &mut stream::Frames,
        bytes: usize,
        received: &mut Vec<wire::Response>,
    ) -> Result<(), OllamaFailure> {
        if let Some(payload) = frames.finish() {
            let frame = parse_frame(&payload, bytes)?;
            self.hand_on(&frame);
            received.push(frame);
        }
        Ok(())
    }

    /// Hand one frame's text on, if anything is watching.
    ///
    /// **A frame with no text sends nothing rather than an empty string.** The
    /// terminal frame of a streamed answer carries `"content": ""` beside the
    /// reason — measured on every recorded stream — and a consumer that
    /// received an empty delta would repaint for no reason at the one moment
    /// the turn is about to end and repaint anyway.
    ///
    /// A send that fails means the receiver is gone, which is an ordinary end
    /// of a surface rather than a failure of an exchange: the answer is still
    /// returned whole. So the result is deliberately discarded.
    fn hand_on(&self, frame: &wire::Response) {
        let Some(text) = frame
            .message
            .as_ref()
            .map(|message| message.content.as_str())
            .filter(|text| !text.is_empty())
        else {
            return;
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

/// One frame's payload as a response, or the failure that says why not.
///
/// `bytes` is what the stream has delivered so far, because [ADR-0016] D2's
/// rule for an unreadable body is that it is reported by its length and never
/// by its content.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn parse_frame(payload: &str, bytes: usize) -> Result<wire::Response, OllamaFailure> {
    serde_json::from_str(payload).map_err(|error| OllamaFailure::Unreadable {
        bytes,
        parser: error.to_string(),
    })
}

impl Provider for OllamaClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        &self.configured
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // Streaming: true. This client asks for `"stream": true` and reads
        // NDJSON frames; it has no non-streamed path at all.
        //
        // Tool calling: true, and **the model this was measured against says
        // so itself** -- Ollama's own `/api/tags` reports
        // `"capabilities": ["completion", "tools"]` for `llama3.2:3b`. That is
        // the descriptor agreeing with the provider rather than asserting
        // over it.
        //
        // Token accounting: true, because the terminal frame carries
        // `prompt_eval_count` and `eval_count` -- which is the half of the
        // pairing `Provider::usage` owes, and it is answered below.
        //
        // **This moves no clause of ADR-0012.** Clause 2 asks for a streaming
        // tool-calling exchange against a stub for *each of five* kinds; two
        // of five now have a client and three have none.
        //
        // Context window: `provider.ollama.context_tokens` as it resolved,
        // over `endpoint::DEFAULT_CONTEXT_TOKENS`, which is the server's own
        // default and not the model's trained length. The same number is sent
        // as `num_ctx`, so this is a window the server has agreed to rather
        // than one this harness hopes for.
        ProviderCapabilities::declared(true, true, true, Some(self.context_tokens))
    }

    fn usage(&self) -> Option<TokenUsage> {
        // `None` before the first exchange, `Some` after one. A client that
        // had made no request and reported a zero would be inventing a datum.
        let slot = match self.last.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        slot.map(|(prompt, completion)| TokenUsage::counted(prompt, completion))
    }
}

impl Model for OllamaClient {
    fn capabilities(&self) -> Capabilities {
        // One statement, read twice. `From` rather than a second literal, so a
        // client that stops calling tools cannot say so in one place and not
        // the other.
        Provider::capabilities(self).into()
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // The one place `OllamaFailure` becomes `PortFailure`. The port
        // carries a sentence and nothing else, so the class ADR-0016 puts this
        // failure in is lost here -- which is right for `zaru-core`, whose
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
