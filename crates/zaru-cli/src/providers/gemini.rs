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
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// How large this kind's context window is, in tokens.
///
/// **1,048,576, and it is a citation rather than a choice.** Google's model
/// page for `gemini-3.6-flash`, read 2026-09-05, states "Input token limit
/// 1,048,576": <https://ai.google.dev/gemini-api/docs/models/gemini-3.6-flash>.
///
/// **It lived in `crate::cli::layers` until 2026-09-14**, where it was the
/// composition's one number for every provider — true of this kind and wrong
/// for the two that gained clients after it. It is here now because
/// [ADR-0012] D3's capability descriptor is where a property of a provider
/// belongs, and this client is the only thing that knows which provider it
/// is.
///
/// **One number for a kind that serves many models is a real limit and is
/// stated rather than hidden.** This kind's other models have other windows;
/// nothing here reads the resolved model identifier, because Google publishes
/// no endpoint that reports one and a table of model names inside this binary
/// would go stale silently. `provider.gemini.context_tokens` overrides it at
/// any layer for a reader who knows better.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const CONTEXT_WINDOW_TOKENS: u64 = 1_048_576;

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
    /// How large this provider's window is, for the descriptor.
    ///
    /// [`CONTEXT_WINDOW_TOKENS`] unless a configuration layer said otherwise.
    context_tokens: u64,
    http: reqwest::Client,
    /// Where the answer's text goes as it arrives, when anything is watching.
    ///
    /// # Why a channel rather than a borrowed sink
    ///
    /// The pane is created inside the terminal driver and the client is built
    /// before it, in `compose::turn::prepare`, so the client **outlives**
    /// the thing that wants to paint — a borrowed `&dyn` sink could not be
    /// held here without infecting `Prepared` with the pane's lifetime, and
    /// an `Arc<dyn …>` cannot own something that borrows the pane either.
    /// A sender owns nothing of the pane's and is `Send + 'static`, so the
    /// two lifetimes never meet.
    ///
    /// It also lands where the driver can already receive it: that loop is a
    /// `select!` over the turn, the terminal and a beat, and a channel is one
    /// more branch rather than a new mechanism.
    ///
    /// **`None` is the ordinary case and costs nothing.** `zaru "<task>"` has
    /// no pane, so nothing is set, nothing is sent, and no delta is built —
    /// which is also why this is not an unbounded queue nobody drains.
    ///
    /// **Unbounded, deliberately.** The alternative is a bounded channel that
    /// drops deltas when full, and a dropped delta is text the user never
    /// sees in a pane whose whole purpose is showing the answer arrive. The
    /// queue is bounded in practice by one model's output for one turn, and
    /// the receiver drains it on every beat.
    deltas: Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>,

    /// What the most recently learned usage was, for [`Provider::usage`].
    ///
    /// **The exchange in flight while one is, and the last completed one
    /// otherwise.** [`Self::record_usage`] writes it from every streamed
    /// frame that reports anything, and [`Self::exchange`] writes the folded
    /// value again when the stream ends. See `record_usage` for the ruling
    /// that made this the in-flight exchange's rather than the previous
    /// one's, and for what the change does not buy.
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
        context_tokens: u64,
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
            crate::web::client::build(
                crate::providers::transport::EXCHANGE_TIMEOUT,
                reqwest::redirect::Policy::default(),
            )
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
            context_tokens,
            http,
            last: Mutex::new(None),
            answered: Mutex::new(map::Answered::default()),
            deltas: Mutex::new(None),
        })
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
    ) -> Result<u64, GeminiFailure> {
        let tools = map::tools_of(descriptors)?;
        let rendered = serde_json::to_string(&tools).unwrap_or_default();
        Ok(rendered.len() as u64)
    }

    /// Send the answer's text to `sender` as each frame of it arrives.
    ///
    /// Called by a surface that has somewhere to paint. Until it is, and in
    /// every surface that never calls it, the client builds no delta and
    /// sends nothing: `zaru "<task>"` builds the same client, never calls
    /// this, and streams nothing.
    ///
    /// `&self` rather than `&mut self` because the caller holds the client
    /// through a shared reference by the time it has a pane: the composition
    /// builds the client, then the driver builds the pane around it.
    pub fn stream_deltas_to(&self, sender: tokio::sync::mpsc::UnboundedSender<String>) {
        // A poisoned lock means a previous holder panicked while replacing an
        // `Option`, which cannot happen; the value is set either way rather
        // than propagating a panic into a surface that is starting up.
        match self.deltas.lock() {
            Ok(mut slot) => *slot = Some(sender),
            Err(poisoned) => *poisoned.into_inner() = Some(sender),
        }
    }

    /// Take one read from the socket into the frames received so far.
    ///
    /// # This is a seam so that the read path can be checked without a socket
    ///
    /// [Testing] forbids a check calling a provider and the CI runner has no
    /// key, so if the only way to reach this loop were [`Self::exchange`],
    /// the one property that distinguishes a streaming client from the
    /// non-streaming one it replaced — that text is handed on **as each frame
    /// arrives**, not once at the end — could not be checked at all.
    ///
    /// **That is not hypothetical.** An earlier version of this client's
    /// checks drove [`Self::hand_on`] directly, and deleting the call from
    /// the read loop left every one of them green: they proved the delta
    /// builder worked and proved nothing about whether the exchange used it.
    /// Driving *this* function over recorded bytes is what closes that,
    /// because it is the same code the socket reaches.
    ///
    /// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
    fn absorb(
        &self,
        frames: &mut stream::Frames,
        chunk: &[u8],
        bytes: usize,
        received: &mut Vec<wire::Response>,
    ) -> Result<(), GeminiFailure> {
        for payload in frames.feed(chunk) {
            let frame = parse_frame(&payload, bytes)?;
            // Handed on **here**, as the frame is read, which is the whole
            // difference a stream makes to a person waiting. Everything after
            // the read loop happens once the model has finished.
            self.hand_on(&frame);
            // The frame's other half. Two per-frame acts in one loop, so a
            // frame cannot reach the reader with its text and without its
            // cost -- see `Self::record_usage`.
            self.record_usage(&frame);
            received.push(frame);
        }
        Ok(())
    }

    /// Take the frame the body ended without terminating, if there was one.
    ///
    /// Separate from [`Self::absorb`] because it is reached once, after the
    /// last read, and folding it into the loop would mean calling
    /// [`stream::Frames::finish`] on every chunk — which would end the stream
    /// at the first read that did not fill a frame.
    fn absorb_last(
        &self,
        frames: &mut stream::Frames,
        bytes: usize,
        received: &mut Vec<wire::Response>,
    ) -> Result<(), GeminiFailure> {
        if let Some(payload) = frames.finish() {
            let frame = parse_frame(&payload, bytes)?;
            self.hand_on(&frame);
            self.record_usage(&frame);
            received.push(frame);
        }
        Ok(())
    }

    /// Hand one frame's text on, if anything is watching.
    ///
    /// **A frame with no text sends nothing rather than an empty string.**
    /// The last frame of a streamed answer carries a `{"text": ""}` part
    /// beside the finish reason — measured on both recorded streams — and a
    /// consumer that received an empty delta would repaint for no reason at
    /// the one moment the turn is about to end and repaint anyway.
    ///
    /// A send that fails means the receiver is gone, which is an ordinary end
    /// of a surface rather than a failure of an exchange: the answer is still
    /// returned whole. So the result is deliberately discarded.
    fn hand_on(&self, frame: &wire::Response) {
        let text: String = frame
            .candidates
            .first()
            .and_then(|candidate| candidate.content.as_ref())
            .map(|content| {
                content
                    .parts
                    .iter()
                    .filter_map(|part| match part {
                        wire::Part::Text { text, .. } => Some(text.as_str()),
                        wire::Part::FunctionCall { .. }
                        | wire::Part::FunctionResponse { .. }
                        | wire::Part::Other(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if text.is_empty() {
            return;
        }
        let slot = match self.deltas.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(sender) = slot.as_ref() {
            drop(sender.send(text));
        }
    }

    /// Publish one frame's token counts, so the row can name the exchange in
    /// flight rather than the one before it.
    ///
    /// # Why this exists, and what reversing a refusal bought
    ///
    /// Until 2026-09-15 [`Self::last`] was written **once**, after the read
    /// loop and after [`map::fold`], so [`Provider::usage`] answered about
    /// the last *completed* exchange for the whole of the next one. The
    /// status row polls that function on the shell's beat, so a person
    /// watching a turn read the previous exchange's count throughout —
    /// [operations/harness-look-and-feel-audit-2] row 2, which measured the
    /// row holding `813 tokens` for 34 of a 40.8-second turn.
    ///
    /// **This is the per-frame variant [ADR-0012]'s `live-status` amendment
    /// of 2026-09-06 named and refused**, on the ground that choosing it
    /// "would decide by implementation the question this section reserves
    /// for a person". It is taken now because a person's delegate decided it
    /// on the record first: the coordinator's ruling of 2026-09-15 under
    /// directive 35, open to Jeshua's veto, on [ADR-0012's amendments page].
    /// `Provider::usage` now means **the usage most recently learned**,
    /// which is the in-flight exchange's from its first frame.
    ///
    /// # What it does not buy, measured rather than assumed
    ///
    /// **Nothing during the thinking span.** Two probes of
    /// `streamGenerateContent?alt=sse` on 2026-09-15 timestamped every line:
    /// the first SSE byte arrived at 46.0 s of a 55.8-second request and at
    /// 92.7 s of a 106.4-second one — 82% and 87% of the work with **no
    /// frame of any kind** on the wire, not a usage frame and not a
    /// keep-alive. So this makes the number honest from the moment the
    /// provider says anything, and the span before that is
    /// [`crate::terminal::driver::Pane`]'s to narrate.
    ///
    /// # A frame reporting nothing leaves the slot alone
    ///
    /// [`map::usage_of`] answers `None` rather than zero, and this returns
    /// without writing. Overwriting a real count with an invented zero is
    /// exactly what `usage.rs` refuses to do for cost.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0012's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction-updates
    /// [operations/harness-look-and-feel-audit-2]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel-audit-2
    fn record_usage(&self, frame: &wire::Response) {
        let Some(reported) = map::usage_of(frame) else {
            return;
        };
        let counted = (reported.prompt, reported.completion);
        // A poisoned lock means a previous holder panicked while writing two
        // integers, which cannot happen; the value is replaced either way
        // rather than propagating a panic out of a stream that is arriving.
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(counted),
            Err(poisoned) => *poisoned.into_inner() = Some(counted),
        }
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
                // **The whole chain, not `reqwest`'s top-level sentence.** See
                // `providers::transport`: `to_string()` gives "error sending
                // request for url (...)" and drops "Connection refused" three
                // links below it. Measured on the release binary before this
                // call changed: a closed port and a hostname that does not
                // resolve printed the same sentence but for the URL, and this
                // kind's class is environmental, so the remedy beside it told
                // both readers to run the command again.
                detail: crate::providers::transport::transport_detail(&error),
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
                    detail: crate::providers::transport::transport_detail(&error),
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
                    detail: crate::providers::transport::transport_detail(&error),
                })?;
            let Some(chunk) = chunk else { break };
            bytes += chunk.len();
            self.absorb(&mut frames, &chunk, bytes, &mut received)?;
        }
        self.absorb_last(&mut frames, bytes, &mut received)?;

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
        // Written again at the end, **kept** beside the per-frame writes
        // rather than replaced by them: `map::fold` keeps the last frame's
        // `usageMetadata`, so this value and the last thing
        // `Self::record_usage` wrote are the same two integers -- which
        // `the_folded_usage_and_the_last_frames_usage_agree` pins over both
        // recorded streams. It stays because it is the value the *mapped*
        // response reports, so a future fold that stopped agreeing would be
        // caught by that check rather than by a user.
        //
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
        //
        // Context window: this kind's own constant unless a layer said
        // otherwise -- see `CONTEXT_WINDOW_TOKENS` and `Self::new`.
        ProviderCapabilities::declared(true, true, true, Some(self.context_tokens))
    }

    fn usage(&self) -> Option<TokenUsage> {
        // **The usage most recently learned**, which is the exchange in
        // flight from its first frame and the last completed one between
        // exchanges -- the coordinator's ruling of 2026-09-15 under
        // directive 35, on ADR-0012's amendments page, reversing that
        // record's own refusal of 2026-09-06 by name. `Self::record_usage`
        // carries the reasoning and the measurement.
        //
        // `None` before the first frame of the first exchange, `Some` after
        // one. The pairing
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
