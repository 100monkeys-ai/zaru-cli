// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What this client could not do, and which of [ADR-0016] D1's five classes
//! each shape is.
//!
//! # There is no credential, so three of the `gemini` client's concerns do not
//! arise
//!
//! That client has a `CredentialRejected` shape, a `redacted_detail` function
//! that checks a provider's own sentence for the key before it is carried, and
//! a documented rule that a URL never receives a secret. **None of the three
//! has a counterpart here, and the absence is the design rather than an
//! oversight**: this kind sends no credential, so there is none to be
//! rejected, none to appear in a message, and none to keep out of a URL.
//! Measured on 2026-09-14 — a request carrying no authorization header of any
//! kind answers HTTP 200.
//!
//! That is why a refusal here carries the server's sentence **verbatim** where
//! the other client carries it only after a check. A sentence that cannot
//! contain a secret needs no scrubbing, and scrubbing it anyway would imply a
//! secret exists somewhere on this path.
//!
//! # Two classes read differently from the `gemini` client's, and D1 is why
//!
//! [ADR-0016] D1 row 2 — the user's — is "Missing key, **unreachable
//! endpoint**, bad config, no manifest. Says exactly what to change." Row 3 —
//! neither's — is "Rate limit, network, **provider outage**."
//!
//! A hosted provider that cannot be reached is row 3: the user cannot fix
//! Google. **A local server that is not running is row 2**: the user starts it,
//! or points the endpoint elsewhere, and the remedy is a sentence they can
//! act on. The same is true of a model this Ollama does not have — row 2's
//! "bad config", remedied by pulling it or resolving the alias to one the
//! server holds.
//!
//! **This is [ADR-0012] D5 reaching the error taxonomy.** D5 says the local
//! path must not be "a second-class code path that breaks quietly"; giving a
//! local failure the class its hosted sibling happens to use, when a different
//! person can act on it, is exactly the quiet break. Recorded as an accepted
//! amendment on that record; **no class was added** and D1's five are
//! untouched.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::providers::capacity::{Exceeded, Refused};
use crate::providers::endpoint::ProviderEndpoint;
use core::fmt;

/// What this client could not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OllamaFailure {
    /// Nothing answered at the endpoint.
    ///
    /// **The user's, per [ADR-0016] D1 row 2's "unreachable endpoint".** It
    /// carries the endpoint because the remedy is about that endpoint and a
    /// reader has to see which one was tried — and an endpoint is not a
    /// credential, so quoting it back costs nothing.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    Unreachable {
        /// Where this client tried to reach.
        endpoint: ProviderEndpoint,
        /// What the transport said.
        detail: String,
    },
    /// The server does not have the model that was asked for.
    ///
    /// **The user's**, and the one failure of this kind that has no `gemini`
    /// counterpart: there a key is rejected before a model name is ever
    /// considered, and here there is no key to reject, so this is the first
    /// thing a misconfigured machine meets.
    ModelNotFound {
        /// The identifier that was asked for, as configured.
        model: String,
        /// What the server said.
        detail: String,
    },
    /// The server refused the shape of the request.
    ///
    /// **Ours.** The user did not build the request, so there is nothing they
    /// can change; the server's own sentence is carried because that is what a
    /// maintainer needs.
    RequestRefused {
        /// The HTTP status.
        code: u16,
        /// What the server said.
        detail: String,
    },
    /// The server refused the request for exceeding the model's context or
    /// token capacity, in words that say so.
    ///
    /// **The reader's**, per [ADR-0036] D2: the window is
    /// `provider.ollama.context_tokens`, the number this client also sends as
    /// `num_ctx`. The type and its sentence are
    /// [`crate::providers::capacity`]'s, shared with the other two clients.
    ///
    /// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
    CapacityRefused(Refused),
    /// The complete request for the next exchange would exceed the window this
    /// client was configured with, and was not sent.
    ///
    /// See [`crate::providers::capacity::preflight`]. For this kind it is also
    /// what stops a request reaching the server's own silent truncation: at
    /// `16b4376`, Ollama's `/api/chat` drops the oldest messages until the
    /// prompt fits `num_ctx` unless the request says `"truncate": false`.
    ContextWindowExceeded(Exceeded),
    /// The server failed on its own side.
    ///
    /// **Neither's**, which is the one class this kind shares with a hosted
    /// provider for the same reason: a server that has broken internally is
    /// not something the reader fixes by changing a setting.
    Unavailable {
        /// The HTTP status.
        code: u16,
        /// What the server said.
        detail: String,
    },
    /// A response body this client could not read.
    ///
    /// **The length and never the content**, the same rule the `gemini`
    /// client's equivalent follows — not because a body here could hold a
    /// credential, since none is ever sent, but because a body is the model's
    /// output and a failure sentence is something a reader pastes into a bug
    /// report.
    Unreadable {
        /// How many bytes came back.
        bytes: usize,
        /// What the parser said. Positional and structural — `serde_json`
        /// names a line, a column and an expected type, and does not quote the
        /// input.
        parser: String,
    },
    /// A tool descriptor whose parameters are not JSON.
    ///
    /// This client's caller supplied it, so it is a defect of the harness
    /// rather than of the server or the user.
    ToolSchemaUnreadable {
        /// Which tool.
        tool: String,
        /// What the parser said.
        parser: String,
    },
    /// The turn's accumulated results and its remembered calls are not the
    /// same number.
    ///
    /// A defect for the same reason [`Self::ToolSchemaUnreadable`] is: both
    /// numbers come from this process. **Two numbers and no content** — a
    /// result's bytes are a tool's output and a call's name is a tool's name,
    /// and neither belongs in a sentence a reader will paste somewhere.
    ResultsDoNotMatchCalls {
        /// How many results the loop accumulated.
        results: usize,
        /// How many calls this client remembers asking for.
        calls: usize,
    },
}

impl fmt::Display for OllamaFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { endpoint, detail } => write!(
                f,
                "nothing answered at {endpoint}: {detail}. A local model server is started by \
                 whoever runs it, so either start it or set `provider.ollama.endpoint` to where \
                 it is listening",
            ),
            Self::ModelNotFound { model, detail } => write!(
                f,
                "the server does not have the model {model:?}: {detail}. Pull it with `ollama \
                 pull {model}`, or set `model.default` to a model the server already holds",
            ),
            Self::RequestRefused { code, detail } => write!(
                f,
                "the server refused this request with HTTP {code}: {detail}",
            ),
            Self::CapacityRefused(refused) => fmt::Display::fmt(refused, f),
            Self::ContextWindowExceeded(exceeded) => fmt::Display::fmt(exceeded, f),
            Self::Unavailable { code, detail } => {
                write!(f, "the server answered HTTP {code}: {detail}")
            }
            Self::Unreadable { bytes, parser } => write!(
                f,
                "the server's {bytes}-byte response could not be read: {parser}",
            ),
            Self::ToolSchemaUnreadable { tool, parser } => write!(
                f,
                "the tool {tool:?} has parameters this harness could not render as JSON: {parser}",
            ),
            Self::ResultsDoNotMatchCalls { results, calls } => write!(
                f,
                "this turn accumulated {results} tool results for {calls} calls; they are paired \
                 by position, so an unequal count is this harness having lost track rather than \
                 something to send",
            ),
        }
    }
}

impl std::error::Error for OllamaFailure {}

impl OllamaFailure {
    /// Which failure an HTTP status and body describe.
    ///
    /// **A model that is not there is told apart from a request that is
    /// malformed by the status alone**, which is a genuine simplification over
    /// the `gemini` client: there a rejected key and a malformed request share
    /// HTTP 400 `INVALID_ARGUMENT` and are told apart by reading the message,
    /// with the tie broken towards the user. Here 404 means exactly one thing
    /// and 400 means exactly one thing, so nothing is inferred from prose.
    ///
    /// `model` is what the request asked for, carried in rather than read out
    /// of the server's sentence — the server quotes the name back, but reading
    /// it out of a message would be this client parsing somebody else's prose
    /// for a value it already holds.
    #[must_use]
    pub fn from_status(code: u16, body: &[u8], model: &str) -> Self {
        let detail = match serde_json::from_slice::<super::wire::ErrorEnvelope>(body) {
            Ok(envelope) => envelope.error,
            // A body that is not the documented envelope is still evidence, so
            // its length is reported rather than its bytes -- the same rule
            // `Unreadable` follows and for the same reason.
            Err(_) => format!("{} bytes that are not an error envelope", body.len()),
        };
        match code {
            404 => Self::ModelNotFound {
                model: model.to_owned(),
                detail,
            },
            // ADR-0036 D2: a 4xx whose sentence names a context or token
            // capacity is the reader's. **This is the one reading here made
            // from prose**, and it is narrow for the reason `capacity` gives.
            // The form this server sends, read in its source at `16b4376`:
            // `llama-server`'s own envelope carried verbatim as the string of
            // this one, "request (N tokens) exceeds the available context size
            // (M tokens), try increasing it". A sentence it does not read
            // keeps the arm below it.
            400..=499 if crate::providers::capacity::names_a_capacity(&detail) => {
                Self::CapacityRefused(Refused {
                    code,
                    status: None,
                    detail,
                })
            }
            400 => Self::RequestRefused { code, detail },
            _ => Self::Unavailable { code, detail },
        }
    }
}
