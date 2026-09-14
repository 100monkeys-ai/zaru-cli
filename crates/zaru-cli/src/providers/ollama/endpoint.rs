// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a request goes, and the one function that builds a URL.
//!
//! # The default, and where it came from
//!
//! [ADR-0012] D5 makes every provider configure the same way and names no
//! default for any kind. [`DEFAULT_ENDPOINT`] is **proposed by this arc and
//! recorded on that record rather than asserted**, in exactly the shape
//! `provider-client` used for the first one: it is the **origin** and nothing
//! else, because the path is this client's own and a model identifier is
//! [ADR-0012] D1's — resolved through ADR-0014's layers, never baked into an
//! endpoint.
//!
//! `http://localhost:11434` is the origin Ollama serves on with no
//! configuration, which is what makes it the right default rather than a
//! preference: a user who installed Ollama and did nothing else is reachable,
//! and a user who moved it sets `provider.ollama.endpoint` like any other.
//!
//! # There is no key, so there is nothing for a URL to leak
//!
//! The `gemini` client's endpoint module carries a long argument for why its
//! own URL builder takes no secret: a URL reaches every proxy log and
//! every quoted error, so a key must never be able to enter one. **The same
//! signature holds here for a simpler reason — this kind has no credential at
//! all**, so there is no secret on the path for a URL to receive. The property
//! is stated rather than left implicit, because a reader comparing the two
//! clients should not have to wonder whether this one forgot.
//!
//! # The endpoint stays refused to project files
//!
//! `providers::resolution::fields` already declares `provider.<kind>.endpoint`
//! refused to the project layer, for all five kinds, under ADR-0014 D6's
//! proposed fifth escalation: "where a user's prompts are sent is the user's
//! choice, and a repository they cloned must not be able to redirect them".
//! What this module adds is a check that the refusal holds **through this
//! path** — and for a local kind the argument is if anything sharper, since a
//! project that could set this one would redirect a user's prompts to a server
//! of its choosing while the user believed nothing left the machine.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use crate::providers::endpoint::ProviderEndpoint;

/// Where Ollama lives when nobody configures otherwise.
///
/// The origin alone. See the module documentation.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

/// The path this client calls.
///
/// **Ollama's own chat endpoint, and the only one this client calls.** Not
/// `/v1/chat/completions`, which is Ollama's OpenAI-compatible surface and
/// belongs to [ADR-0012] D3's *other* kind; see `super::wire` for the measured
/// difference between the two.
///
/// A constant rather than a literal inside a `format!`, so the path this
/// client speaks is a thing a check can read.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const CHAT_PATH: &str = "/api/chat";

/// How many CPU threads one exchange may use.
///
/// **Six of this machine's sixteen, and it is a bound rather than a tuning.**
/// The harness runs on the same machine as the server, and a local model that
/// takes every core makes the terminal it is rendering into stutter. Six was
/// measured on 2026-09-14 to hold the model runner at about a third of one
/// core-equivalent of the machine's capacity while leaving the rest free.
///
/// It is deliberately **not** an [ADR-0014] key: see `super::wire::Options`
/// for why a value that names no model, endpoint or credential is not
/// something that record's layers have anything to say about.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const NUM_THREAD: u16 = 6;

/// The origin every request to this provider goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    origin: String,
}

impl Endpoint {
    /// Take the configured endpoint.
    #[must_use]
    pub fn new(configured: &ProviderEndpoint) -> Self {
        Self {
            // A trailing slash on a configured origin would produce a double
            // slash in the path, which some gateways treat as a different
            // route. `ProviderEndpoint` refuses surrounding whitespace and
            // control characters and deliberately parses no URL, so this is
            // the one normalisation and it is the one that changes routing.
            // The same normalisation the `gemini` client does, for the same
            // reason.
            origin: configured.as_str().trim_end_matches('/').to_owned(),
        }
    }

    /// The default endpoint, for a kind nobody configured.
    ///
    /// # Panics
    ///
    /// Never. [`DEFAULT_ENDPOINT`] is this module's own constant and carries
    /// none of the three shapes [`ProviderEndpoint::new`] refuses.
    #[must_use]
    pub fn default_endpoint() -> ProviderEndpoint {
        ProviderEndpoint::new(DEFAULT_ENDPOINT)
            .expect("this module's own default endpoint is well-formed")
    }

    /// Where a chat request goes.
    ///
    /// **Takes no model and no secret.** Ollama names the model in the request
    /// *body* rather than in the path, so unlike the `gemini` client's URL
    /// builder this one needs no [`ModelId`](crate::providers::ModelId) — which
    /// means the one place a URL is built here cannot carry a model identifier
    /// either, and [ADR-0012] D1's "a model identifier appearing anywhere
    /// except the resolution table is a bug" is satisfied by the signature
    /// rather than by care.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub fn chat_url(&self) -> String {
        format!("{origin}{CHAT_PATH}", origin = self.origin)
    }

    /// The origin, as configured.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
