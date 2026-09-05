// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The first provider client in this workspace: [ADR-0012] D3's `gemini`
//! kind.
//!
//! # What this is, and why it is one kind rather than five
//!
//! Until 2026-09-05 nothing here could reach a model. [`Provider`] and
//! `zaru-core`'s [`Model`] were both ports implemented only by checks, and
//! `zaru <task>` was refused at exit 4 with "this harness carries no provider
//! client". This module is the first implementation of both, for exactly one
//! of D3's five kinds.
//!
//! `gemini` and not one of the other four, for one reason: it is the only
//! kind for which a key exists that an agent may use. The Anthropic key is
//! Jeshua's and is not issued for this; no `ollama` is installed on the
//! development machine; `aegis` needs a platform. That is a fact about what
//! could be *proved* rather than a judgement about which provider is best,
//! and D3's own Negative consequence — "each addition is a maintenance
//! surface with its own streaming quirks and error taxonomy" — is the reason
//! the other four are not written blind beside it.
//!
//! # One type implements both ports, and they answer different questions
//!
//! [`Provider`] is the **configured** half: which kind, which endpoint, what
//! it says it can do, what the last request cost. [`Model`] is the
//! **exchange** half: prompt in, response out. `providers::port`'s own
//! documentation explains why those are two traits rather than one, and
//! [`GeminiClient`] implementing both is what makes the pairing concrete:
//! [`Provider::capabilities`] and [`Model::capabilities`] are the same three
//! flags read twice, through [`From`], so there is one statement of what this
//! client can do.
//!
//! # `streaming: true` since 2026-09-05, and clause 2 still does not move
//!
//! **This section said `streaming: false`, said honestly until 2026-09-05.**
//! It is corrected rather than left: this client now calls
//! `streamGenerateContent?alt=sse` and nothing else, so the descriptor says
//! `true` and the old sentence would be the drift a capability descriptor
//! exists to prevent.
//!
//! **ADR-0012 trigger clause 2 is unmoved by this module, and it is unmoved
//! twice over.** It asks for a streaming exchange *against a stub* for *each
//! of the five* provider kinds. This is a streaming exchange against a **real
//! provider** for **one**, and four kinds still have no client. What moved is
//! D3's fourth capability becoming real for this kind — recorded as an
//! amendment on that record, not as a clause.
//!
//! **No stream contract entered `zaru-core`, and that is the design rather
//! than an omission.** `Model::respond` is unchanged, `Capabilities` is
//! unchanged, and the event enum is unchanged. ADR-0012's own Status tracking
//! withholds a stream contract because "a shape chosen by an implementation
//! rather than by a record" ossifies early, and `tool-call-loop` withholds it
//! while no provider is behind it. Putting the framing and the fold entirely
//! inside this client satisfies both at once: the user sees text arrive, and
//! no public interface was shaped by the one kind that happens to have a key.
//!
//! `respond` is still one exchange in, one response out. The frames are
//! folded here — see [`map::fold`] — so the loop above this client cannot
//! tell a streamed exchange from a non-streamed one, which is what keeps one
//! exchange one answer.
//!
//! # The key
//!
//! Read from [ADR-0007]'s store by the alias `provider.gemini`, held as a
//! [`Secret`], and attached to exactly one place: the `x-goog-api-key`
//! header. **Never a query string** — a URL reaches every proxy log, every
//! error that quotes a request, and every terminal scrollback — and
//! [`Endpoint::url_for`] is the only thing that builds a URL, takes no
//! secret, and cannot therefore put one in one.
//!
//! Because the key is in the store, [`crate::redaction::HeldSecrets`] already
//! covers it: that type is built from what the store holds, so ADR-0008
//! trigger clause 6 reaches a provider key without a second seam. That is why
//! the store holds it at all — see [`crate::credentials::secret`].
//!
//! # This client holds one piece of conversational state, and it has to
//!
//! Said here because a provider client that remembers anything is a surprise
//! worth announcing. [`map::Answered`] holds the model turns of the turn now
//! in flight, because `generateContent` is stateless and its own guide
//! requires every later round to resend "All model-generated steps returned
//! in Turn 1 (including thought and function_call steps) exactly as
//! received" — and `zaru-core`'s [`ModelRequest`] has no field for them,
//! rightly, since a `thoughtSignature` means nothing to a headless loop.
//!
//! It is scoped to one turn and reset by the act of building a first
//! request, which is [`map::request_from`]'s doing rather than this
//! module's: a rule held at the only place that can express it instead of at
//! a call site that could forget.
//!
//! **This section said "Nothing here is wired to a loop" until 2026-09-05.**
//! That stopped being true when `composer-wiring` landed `zaru "<task>"`, and
//! it stayed on the page for a day; the state above is the thing being wired
//! to a loop made necessary.
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
pub use failure::{DETAIL_WITHHELD, GeminiFailure};

use crate::credentials::{Alias, Secret};
use crate::providers::capability::ProviderCapabilities;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::kind::ProviderKind;
use crate::providers::port::Provider;
use crate::providers::resolution::ModelId;
use crate::providers::usage::TokenUsage;
use std::sync::Mutex;
use std::time::Duration;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// How long one exchange may take before the client gives up.
///
/// **A ceiling rather than a policy.** No record states a timeout, and one
/// that a caller cannot see is a value chosen for a different caller — so
/// this is a named constant a check can read, not a hidden default, and it is
/// raised on ADR-0012 rather than settled here. Sixty seconds is long enough
/// that a large prompt on a slow link is not cut off, short enough that a
/// hung socket is not a hung terminal.
///
/// **It bounds the whole streamed exchange, first byte to last, and that is
/// stated because it is the reading that changed on 2026-09-05.** This
/// sentence said "the shape a single non-streaming completion needs" while
/// the client made one request and read one body; a stream is still one
/// request and one body, so the ceiling still applies to the same thing — but
/// the body now arrives over the whole time the model is answering, so the
/// budget is spent by generation rather than by latency.
///
/// There is deliberately **no retry and no backoff**. A retry policy decides
/// whether a request that may have had an effect is repeated, and no record
/// makes that decision; a client that retried on its own would be answering
/// it silently.
pub const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(60);

/// The header the API key is presented in.
///
/// Google's own documented form, and the only place this client puts the key.
pub const API_KEY_HEADER: &str = "x-goog-api-key";

/// [ADR-0012] D3's `gemini` provider, and `zaru-core`'s model behind it.
///
/// # `Debug` is derived, and that is safe because of what the fields are
///
/// Every field either redacts itself or carries no secret: [`Secret`]'s
/// `Debug` is hand-written to print a marker, and an endpoint, a model
/// identifier and an alias are all types that refuse a credential-shaped
/// value at construction. An outside check asserts the whole rendering is
/// free of the key by value and by ASCII core.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub struct GeminiClient {
    endpoint: Endpoint,
    configured: ProviderEndpoint,
    model: ModelId,
    alias: Alias,
    key: Secret,
    http: reqwest::Client,
    /// What the last exchange cost, for [`Provider::usage`].
    ///
    /// Interior mutability because [`Provider::usage`] takes `&self` — the
    /// trait's signature, and rightly so: asking what something cost is a
    /// read. The alternative was `&mut self` on the trait, which would make
    /// `Provider` unusable behind a shared reference for every
    /// implementation including the four that do not exist yet.
    ///
    /// **A `Mutex` and not a `Cell`, and the compiler is why.**
    /// [`Model::respond`] returns `impl Future + Send`, so the future
    /// borrowing `&self` requires `Self: Sync`, and `Cell` is not. That is
    /// the port stating a real requirement rather than an inconvenience: the
    /// tool-call loop is asynchronous because ADR-0012 D3 has providers
    /// stream and call tools, so a client is reachable from more than one
    /// task and a datum it mutates has to be safe to read from all of them.
    /// Watched as "error: future cannot be sent between threads safely".
    last: Mutex<Option<(u64, u64)>>,
    /// What the model has already said in the turn now in flight.
    ///
    /// [`map::Answered`] says why a stateless API's client has to keep this
    /// and why the loop cannot. A `Mutex` for the same reason `last` is one:
    /// [`Model::respond`] takes `&self` and returns a `Send` future, so
    /// `Self: Sync` is required and a `RefCell` would not compile.
    answered: Mutex<map::Answered>,
}

impl GeminiClient {
    /// Build a client for one model, over one HTTP client.
    ///
    /// The `reqwest::Client` is built **once, here**, and reused for every
    /// exchange. That is not a micro-optimisation: a fresh client per request
    /// means a fresh connection pool and a fresh TLS handshake per request,
    /// which is a different observable behaviour against a rate-limited API.
    ///
    /// # Errors
    ///
    /// [`GeminiFailure::Unavailable`] when the HTTP client cannot be built at
    /// all — a machine with no usable TLS backend, which is environmental and
    /// not the user's.
    pub fn new(
        endpoint: ProviderEndpoint,
        model: ModelId,
        alias: Alias,
        key: Secret,
    ) -> Result<Self, GeminiFailure> {
        // Built through [`crate::web::client::build`], which is the one
        // place this workspace builds an HTTP client. What this caller
        // differs on is passed as an argument -- its own timeout, and
        // `reqwest`'s default redirect policy, where `web.fetch` passes one
        // that never leaves a host. Two builders would be two answers to what
        // a client here does about cookies and TLS, which is the
        // rule-in-two-places that made `Layer` drift while it was declared
        // twice.
        let http =
            crate::web::client::build(EXCHANGE_TIMEOUT, reqwest::redirect::Policy::default())
                .map_err(|error| GeminiFailure::Unavailable {
                    code: None,
                    detail: error.detail().to_owned(),
                })?;
        Ok(Self {
            endpoint: Endpoint::new(&endpoint),
            configured: endpoint,
            model,
            alias,
            key,
            http,
            last: Mutex::new(None),
            answered: Mutex::new(map::Answered::default()),
        })
    }

    /// The alias the key is stored under, which every refusal names.
    #[must_use]
    pub const fn alias(&self) -> &Alias {
        &self.alias
    }

    /// The model this client asks for.
    #[must_use]
    pub const fn model(&self) -> &ModelId {
        &self.model
    }

    /// One exchange, as the failure taxonomy sees it.
    ///
    /// Separate from [`Model::respond`] so that the mapping from
    /// [`GeminiFailure`] to [`PortFailure`] happens in one place and this
    /// function can be read as the request it makes.
    ///
    /// # Errors
    ///
    /// [`GeminiFailure`], classified by provenance. See [`failure`].
    pub async fn exchange(
        &self,
        request: &ModelRequest<'_>,
    ) -> Result<ModelResponse, GeminiFailure> {
        // The turn's history, which `request_from` resets when this request
        // begins a turn -- see `map::Answered::at_turn_boundary`, which is
        // where that decision lives so that no call site can forget it.
        //
        // Scoped so the guard is dropped before the first `.await`: a
        // `std::sync::MutexGuard` is `!Send`, and `Model::respond` returns a
        // `Send` future, so holding one across an await would not compile.
        // A poisoned lock means a previous holder panicked while pushing to a
        // `Vec`, which cannot happen; the value is used either way rather
        // than propagating a panic into an exchange.
        let body = {
            let mut answered = match self.answered.lock() {
                Ok(answered) => answered,
                Err(poisoned) => poisoned.into_inner(),
            };
            map::request_from(request, &mut answered)?
        };
        let url = self.endpoint.url_for(&self.model);

        let mut response = self
            .http
            .post(url)
            // The one place the key is attached, and a header rather than a
            // query string: a URL lands in proxy logs and in every message
            // that quotes a request.
            .header(API_KEY_HEADER, self.key.expose_for_dispatch())
            .json(&body)
            .send()
            .await
            .map_err(|error| GeminiFailure::Unavailable {
                code: error.status().map(|status| status.as_u16()),
                detail: error.to_string(),
            })?;

        let status = response.status();

        // **A failure arrives as an ordinary response, not as frames.**
        // Measured 2026-09-05 on this endpoint: a bad model name answered 404
        // `NOT_FOUND`, a rejected key 400 `INVALID_ARGUMENT` word for word as
        // the non-streamed endpoint does, and a malformed body 400 with
        // `fieldViolations` -- each a plain JSON object, *despite* the
        // `content-type: text/event-stream` header the error path also sets.
        // So the body is taken whole here and `classify` is unchanged, which
        // is why `recorded/rejected-key.json` still means what it meant.
        if !status.is_success() {
            let bytes = response
                .bytes()
                .await
                .map_err(|error| GeminiFailure::Unavailable {
                    code: Some(status.as_u16()),
                    detail: error.to_string(),
                })?;
            return Err(self.classify(status.as_u16(), &bytes));
        }

        // --- The stream, read as it arrives ------------------------------
        //
        // `chunk()` rather than `bytes_stream()`: the first carries no
        // feature gate and the second is behind `stream`, so reading the body
        // incrementally costs this workspace no feature, no row and no lock
        // delta. Measured in the vendored source of `reqwest` 0.12.28.
        let mut frames = stream::Frames::new();
        let mut received: Vec<wire::Response> = Vec::new();
        let mut bytes = 0usize;

        loop {
            // A stream that stops mid-way is a socket that stopped, which is
            // neither the user's doing nor ours -- ADR-0016 D1's
            // environmental class, reached through the same variant a refused
            // connection reaches. There is no sentinel frame to miss: this
            // producer sends none, so end-of-body is end-of-stream.
            let chunk = response
                .chunk()
                .await
                .map_err(|error| GeminiFailure::Unavailable {
                    code: Some(status.as_u16()),
                    detail: error.to_string(),
                })?;
            let Some(chunk) = chunk else { break };
            bytes += chunk.len();
            for payload in frames.feed(&chunk) {
                received.push(parse_frame(&payload, bytes)?);
            }
        }
        if let Some(payload) = frames.finish() {
            received.push(parse_frame(&payload, bytes)?);
        }

        if received.is_empty() {
            return Err(GeminiFailure::Unreadable {
                bytes,
                parser: "the stream carried no frames, which the API does not document as a \
                         successful shape"
                    .to_owned(),
            });
        }

        // One exchange is one response. See `map::fold` for why the frames
        // are folded before anything is mapped, and for the two-frame trap
        // that makes folding load-bearing rather than tidy.
        let answer = map::fold(&received);
        let mapped = map::response_from(&answer, bytes)?;
        // A poisoned lock means a previous holder panicked while writing two
        // integers, which cannot happen; the value is replaced either way
        // rather than propagating a panic out of an exchange that succeeded.
        let usage = (mapped.tokens().prompt, mapped.tokens().completion);
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(usage),
            Err(poisoned) => *poisoned.into_inner() = Some(usage),
        }

        // Remember this model turn **only when it asked for tools**, because
        // that is the only case a later round exists to give it back in: a
        // `Text` or a `Stopped` ends the turn and the next exchange arrives
        // with no results and forgets everything anyway. The parts come from
        // the parsed response rather than from `mapped`, which has already
        // narrowed them to `zaru-core`'s three arms and dropped the
        // signature the API requires back.
        if matches!(mapped, ModelResponse::Calls { .. })
            && let Some(parts) = answer
                .candidates
                .first()
                .and_then(|candidate| candidate.content.as_ref())
                .map(|content| content.parts.as_slice())
        {
            match self.answered.lock() {
                Ok(mut answered) => answered.record(parts),
                Err(poisoned) => poisoned.into_inner().record(parts),
            }
        }
        Ok(mapped)
    }

    /// Read a non-success body as one of ADR-0016's classes.
    ///
    /// A body that is not AIP-193's envelope is environmental rather than a
    /// defect: a 502 from a proxy in front of the API is HTML, and reporting
    /// that as "this harness built a bad request" would send a reader looking
    /// for a bug that is not there.
    fn classify(&self, code: u16, body: &[u8]) -> GeminiFailure {
        let Ok(envelope) = serde_json::from_slice::<wire::ErrorEnvelope>(body) else {
            return GeminiFailure::Unavailable {
                code: Some(code),
                // The length, never the content: an unparsed body is exactly
                // the one nobody can promise is free of a credential.
                detail: format!("{} byte(s) that are not an API error envelope", body.len()),
            };
        };
        let error = envelope.error;
        let detail = GeminiFailure::redacted_detail(&error.message, self.key.expose_for_dispatch());

        if GeminiFailure::is_credential_status(code, &error.status, &error.message) {
            return GeminiFailure::CredentialRejected {
                alias: self.alias.clone(),
                kind: ProviderKind::Gemini,
                code,
                status: error.status,
            };
        }
        if (400..500).contains(&code) {
            return GeminiFailure::RequestRefused {
                code,
                status: error.status,
                detail,
            };
        }
        GeminiFailure::Unavailable {
            code: Some(code),
            detail,
        }
    }
}

/// One frame's payload as a response, or the failure that says why not.
///
/// `bytes` is what the stream has delivered so far, because [ADR-0016] D2's
/// rule for an unreadable body is that it is reported by its length and never
/// by its content -- a body that will not parse is exactly where a key or a
/// user's prompt would be quoted into an error message.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn parse_frame(payload: &str, bytes: usize) -> Result<wire::Response, GeminiFailure> {
    serde_json::from_str(payload).map_err(|error| GeminiFailure::Unreadable {
        bytes,
        parser: error.to_string(),
    })
}

impl Provider for GeminiClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        &self.configured
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // Streaming: **true since 2026-09-05**, and for the same reason it
        // read `false` before -- the descriptor says what this client does.
        // `streamGenerateContent?alt=sse` is now the only method it calls, so
        // a `false` here would be the drift `providers::capability` exists to
        // prevent, one field wide.
        //
        // **This is D3's fourth capability becoming real for ONE kind, and it
        // moves no clause.** ADR-0012 clause 2 asks for a streaming exchange
        // "against a stub" for "each of the five provider kinds"; this is a
        // real provider for one, and four kinds still have no client. What
        // changed is that the flag stopped being a false `false`.
        //
        // Tool calling: true, and proved by an exchange rather than asserted.
        // Token accounting: true, because `usageMetadata` is on every frame
        // of every successful response -- which is the half of the pairing
        // `Provider::usage` owes, and it is answered below.
        ProviderCapabilities::declared(true, true, true)
    }

    fn usage(&self) -> Option<TokenUsage> {
        // `None` before the first exchange, `Some` after one. The pairing
        // `providers::port` states -- "a provider whose descriptor says it
        // does not account must answer `None` here, and one that says it does
        // must answer `Some`" -- is about a provider that *has answered*: a
        // client that had made no request and reported a zero would be
        // inventing a datum, which is exactly what `usage.rs` refuses to do
        // for cost.
        let slot = match self.last.lock() {
            Ok(slot) => *slot,
            Err(poisoned) => *poisoned.into_inner(),
        };
        slot.map(|(prompt, completion)| TokenUsage::counted(prompt, completion))
    }
}

impl Model for GeminiClient {
    fn capabilities(&self) -> Capabilities {
        // One statement, read twice. `From` rather than a second literal, so
        // a client that stops calling tools cannot say so in one place and
        // not the other -- which is the drift `providers::capability`'s
        // documentation promised this conversion would prevent.
        Provider::capabilities(self).into()
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // The one place `GeminiFailure` becomes `PortFailure`. The port
        // carries a sentence and nothing else, so the class ADR-0016 puts
        // this failure in is lost here -- which is right for `zaru-core`,
        // whose loop has no taxonomy, and is why `GeminiClient::exchange` is
        // public: the command surface classifies the typed failure, and only
        // the loop sees the flattened one.
        self.exchange(request)
            .await
            .map_err(|failure| PortFailure::new(failure.to_string()))
    }
}
