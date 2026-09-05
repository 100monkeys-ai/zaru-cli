// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a request goes, and the one function that builds a URL.
//!
//! # A query string is not refused here — it is unreachable
//!
//! Google's API accepts a key either as the `x-goog-api-key` header or, on
//! some surfaces, as a `?key=` query parameter. The second is refused
//! absolutely: a URL is written into proxy logs, into `Display` on every
//! transport error, into terminal scrollback and into every bug report that
//! quotes a request, and a credential in a URL has therefore been published
//! by the time anybody notices.
//!
//! **The mechanism is that [`Endpoint::url_for`] takes no secret.** It is the
//! only function in this client that produces a URL and its arguments are an
//! endpoint and a [`ModelId`] — neither of which can be built from a bearer
//! value, both of which refuse control characters and surrounding whitespace
//! for the listing reasons their own modules give. There is no parameter to
//! pass a key through, so putting one in a URL is not a rule somebody has to
//! remember: it is a signature change.
//!
//! That is the same argument [`ProviderEndpoint`] itself makes about
//! [ADR-0012] D5's missing local-versus-hosted variant, and the one
//! [`Sealed`](crate::credentials::Sealed) makes about plaintexts.
//!
//! # The default, and where it came from
//!
//! [ADR-0012] D5 makes every provider configure the same way and names no
//! default for any kind. [`DEFAULT_ENDPOINT`] is proposed by this arc under
//! directive 20 and recorded on that record rather than asserted: it is the
//! **origin** and nothing else, because the path carries the model
//! identifier, and a model identifier is [ADR-0012] D1's — resolved through
//! ADR-0014's layers, never baked into an endpoint.
//!
//! The endpoint remains refused to project files. That is
//! `providers::resolution::fields`' doing, already built, and ADR-0014 D6's
//! proposed fifth escalation: "where a user's prompts are sent is the user's
//! choice, and a repository they cloned must not be able to redirect them".
//! What this arc adds is a check that the refusal holds *through this path* —
//! it is the path down which a redirected endpoint would carry both the
//! user's prompts and their key.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::resolution::ModelId;

/// Where the Gemini API lives when nobody configures otherwise.
///
/// The origin alone. See the module documentation.
pub const DEFAULT_ENDPOINT: &str = "https://generativelanguage.googleapis.com";

/// The API version this client speaks.
///
/// `v1beta` because that is the version `generateContent` is documented under
/// at <https://ai.google.dev/api/generate-content>, read 2026-09-05, and
/// because it is the version the function-calling surface this client uses is
/// documented under. A constant rather than a literal in a `format!`, so the
/// version this client speaks is a thing a check can read.
pub const API_VERSION: &str = "v1beta";

/// The method this client calls.
///
/// Not `streamGenerateContent`. See [`super`].
pub const METHOD: &str = "generateContent";

/// The origin every request to this provider goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    origin: String,
}

impl Endpoint {
    /// Take the configured endpoint, or the default when none was configured.
    #[must_use]
    pub fn new(configured: &ProviderEndpoint) -> Self {
        Self {
            // A trailing slash on a configured origin would produce a double
            // slash in the path, which some gateways treat as a different
            // route. `ProviderEndpoint` refuses surrounding whitespace and
            // control characters and deliberately parses no URL, so this is
            // the one normalisation and it is the one that changes routing.
            origin: configured.as_str().trim_end_matches('/').to_owned(),
        }
    }

    /// The default endpoint, for a kind nobody configured.
    ///
    /// # Panics
    ///
    /// Never. [`DEFAULT_ENDPOINT`] is this module's own constant and carries
    /// none of the four shapes [`ProviderEndpoint::new`] refuses.
    #[must_use]
    pub fn default_endpoint() -> ProviderEndpoint {
        ProviderEndpoint::new(DEFAULT_ENDPOINT)
            .expect("this module's own default endpoint is well-formed")
    }

    /// Where a request for `model` goes.
    ///
    /// **Takes no secret, which is the whole design.** See the module
    /// documentation.
    #[must_use]
    pub fn url_for(&self, model: &ModelId) -> String {
        format!(
            "{origin}/{API_VERSION}/models/{model}:{METHOD}",
            origin = self.origin,
        )
    }

    /// The origin, as configured.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
