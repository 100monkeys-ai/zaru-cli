// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where this client sends, and the one thing it appends.
//!
//! # This kind has no built-in default endpoint, and that is the decision
//!
//! The `gemini` client has `https://generativelanguage.googleapis.com` and
//! the `ollama` client has `http://localhost:11434`, each proposed on
//! [ADR-0012] D5 as that kind's own default. **This kind proposes none**, and
//! the absence is the answer rather than an omission.
//!
//! D3 gives this kind "everything OpenAI-shaped — vLLM, LM Studio, most
//! gateways", and [`crate::providers::kind`] already says why that matters:
//! "`openai-compatible` covers both vLLM on a laptop and a hosted gateway, so
//! the kind alone does not say where its endpoint is". Measured on 2026-09-14,
//! the five origins in reach of this machine are `http://127.0.0.1:11434/v1`
//! (Ollama's compatible surface), `http://127.0.0.1:18080` (`llama-server`,
//! with no prefix at all), `http://localhost:8000/v1` (vLLM's documented
//! default), `http://localhost:1234/v1` (LM Studio's) and
//! `https://api.openai.com/v1`. There is no majority and there is no
//! convention: a default here would be one vendor's port painted on a kind
//! that covers five, and the first user it was wrong for would be sent to a
//! machine they never configured.
//!
//! So [`crate::compose::turn`]'s endpoint-default match keeps refusing for
//! this kind, and the refusal names [`ProviderKind::endpoint_key`] — which is
//! a user-correctable failure with an exact remedy, and is strictly better
//! than a default that silently reaches the wrong port.
//!
//! # The client appends `/chat/completions` and nothing else
//!
//! **The `/v1` belongs to the user's endpoint.** Of the three real endpoints
//! measured on 2026-09-14, two carry a `/v1` prefix and one does not:
//! Ollama's surface is at `/v1/chat/completions`, `llama-server`'s is at
//! `/v1/chat/completions` on an origin with no prefix, and OpenAI's own
//! published base URL is `https://api.openai.com/v1`. A client that assumed
//! `/v1` would be wrong for `llama-server` and a client that assumed none
//! would be wrong for OpenAI, so the client assumes neither: it appends
//! [`CHAT_PATH`] to whatever the user configured and the prefix, where there
//! is one, is theirs.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`ProviderKind::endpoint_key`]: crate::providers::ProviderKind::endpoint_key

use crate::providers::endpoint::ProviderEndpoint;

/// The one path this client appends. See the module documentation for why the
/// `/v1` is not here.
pub const CHAT_PATH: &str = "/chat/completions";

/// A configured origin, normalised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    origin: String,
}

impl Endpoint {
    /// Normalise `configured` for use as an origin.
    #[must_use]
    pub fn new(configured: &ProviderEndpoint) -> Self {
        Self {
            // A trailing slash on a configured origin would produce a double
            // slash in the path, which some gateways treat as a different
            // route. `ProviderEndpoint` refuses surrounding whitespace and
            // control characters and deliberately parses no URL, so this is
            // the one normalisation and it is the one that changes routing.
            // The same normalisation the `gemini` and `ollama` clients do, for
            // the same reason -- and the one most likely to matter here, since
            // this kind's endpoint is the only one a user types in full.
            origin: configured.as_str().trim_end_matches('/').to_owned(),
        }
    }

    /// Where a chat completion is asked for.
    #[must_use]
    pub fn chat_url(&self) -> String {
        format!("{origin}{CHAT_PATH}", origin = self.origin)
    }

    /// The normalised origin.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
