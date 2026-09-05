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
//! # `streaming: false`, said honestly
//!
//! This client does not stream and its descriptor says so. `respond` is one
//! exchange because that is what the port is; `streamGenerateContent` is not
//! called. ADR-0012 D3's streaming concern and its trigger clause 2 are
//! **unmoved by this module**, and clause 2 is unmoved twice over: it asks
//! for a *streaming* exchange against a stub for *each of five* kinds, and
//! this is a non-streaming exchange against a real provider for one.
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
//! # Nothing here is wired to a loop
//!
//! `zaru <task>` still runs no loop. This module makes a `Model` exist; the
//! tool-call loop's wiring is another arc's, and the command surface's
//! refusal now names the four kinds with no client instead of claiming the
//! workspace has none.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`Model`]: zaru_core::tool_call::Model

pub mod endpoint;
pub mod failure;
pub mod map;
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
/// raised on ADR-0012 rather than settled here. Sixty seconds is the shape a
/// single non-streaming completion needs: long enough that a large prompt on
/// a slow link is not cut off, short enough that a hung socket is not a hung
/// terminal.
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
        let http = reqwest::Client::builder()
            .timeout(EXCHANGE_TIMEOUT)
            .build()
            .map_err(|error| GeminiFailure::Unavailable {
                code: None,
                detail: error.to_string(),
            })?;
        Ok(Self {
            endpoint: Endpoint::new(&endpoint),
            configured: endpoint,
            model,
            alias,
            key,
            http,
            last: Mutex::new(None),
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
        let body = map::request_from(request)?;
        let url = self.endpoint.url_for(&self.model);

        let response = self
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
        let bytes = response
            .bytes()
            .await
            .map_err(|error| GeminiFailure::Unavailable {
                code: Some(status.as_u16()),
                detail: error.to_string(),
            })?;

        if !status.is_success() {
            return Err(self.classify(status.as_u16(), &bytes));
        }

        let answer: wire::Response =
            serde_json::from_slice(&bytes).map_err(|error| GeminiFailure::Unreadable {
                bytes: bytes.len(),
                parser: error.to_string(),
            })?;

        let mapped = map::response_from(&answer, bytes.len())?;
        // A poisoned lock means a previous holder panicked while writing two
        // integers, which cannot happen; the value is replaced either way
        // rather than propagating a panic out of an exchange that succeeded.
        let usage = (mapped.tokens().prompt, mapped.tokens().completion);
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(usage),
            Err(poisoned) => *poisoned.into_inner() = Some(usage),
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

impl Provider for GeminiClient {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        &self.configured
    }

    fn capabilities(&self) -> ProviderCapabilities {
        // Streaming: false, and honestly. Tool calling: true, and proved by
        // an exchange rather than asserted. Token accounting: true, because
        // `usageMetadata` is on every successful response -- which is the
        // half of the pairing `Provider::usage` owes, and it is answered
        // below.
        ProviderCapabilities::declared(false, true, true)
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
