// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Everything that can go wrong reaching an OpenAI-compatible endpoint.
//!
//! # No [ADR-0016] D1 class is added, and one shape is new
//!
//! Eight of these nine map onto classes the other two clients already use.
//! The ninth, [`OpenAiCompatibleFailure::StreamFailed`], is a shape neither of
//! them has: **an error that arrives as a frame, inside a stream, after the
//! server already answered `HTTP 200`**. It still takes an existing class —
//! see [`crate::cli::classify`] — so D1's five rows are untouched.
//!
//! # Two classifications read differently from the `gemini` client's
//!
//! The same two the `ollama` client changed, for the same [ADR-0012] D5
//! reason and with the same authority. D1 row 2 — the user's — names "Missing
//! key, **unreachable endpoint**, bad config"; row 3 — neither's — names
//! "provider outage". A local server that is not running is **row 2**,
//! because starting it is exactly what the reader does.
//!
//! **This kind is the one where that argument is genuinely uncertain**, and it
//! is resolved rather than hedged. `openai-compatible` covers a vLLM on the
//! reader's laptop and a hosted gateway they do not own, and the kind alone
//! does not say which — which is the same fact [`super::endpoint`] gives for
//! refusing a default. So the remedy names **both** routes out rather than
//! assuming one: start the server, or point the endpoint somewhere that is
//! listening. A reader whose gateway is down reads a sentence that tells them
//! nothing they can do and loses a little time; a reader whose local server is
//! merely not started reads "wait and try later" and loses the afternoon.
//! The asymmetry decides it, exactly as it decided the 400 tie-break on the
//! `gemini` client.
//!
//! # The key never reaches a rendering
//!
//! [ADR-0016] D2's two implied rules, which the `gemini` client's amendment
//! named: **a failure never carries the key**, and **a response body that will
//! not parse is reported by its length and never its content**. Both hold
//! here. [`OpenAiCompatibleFailure::redacted_detail`] is the first, applied to
//! every sentence taken from a server; [`OpenAiCompatibleFailure::Unreadable`]
//! carrying `bytes` rather than the body is the second.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::credentials::Alias;
use crate::providers::capacity::{Exceeded, Refused};
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::kind::ProviderKind;
use core::fmt;

/// What stands in for a detail that could have quoted the key.
///
/// The same constant the `gemini` client publishes, spelled here rather than
/// imported: a client that stopped withholding should have to change its own
/// module, and a shared constant would let one client's edit silently change
/// what another prints.
pub const DETAIL_WITHHELD: &str = "the server's message is withheld, because it quoted the key";

/// The `openai-compatible` client's taxonomy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAiCompatibleFailure {
    /// The socket never opened, or the stream stopped mid-body.
    Unreachable {
        /// What the user configured.
        endpoint: ProviderEndpoint,
        /// What the transport said.
        detail: String,
    },
    /// The endpoint rejected the key, or wanted one and was given none.
    CredentialRejected {
        /// The alias the key is stored under.
        alias: Alias,
        /// Always [`ProviderKind::OpenAiCompatible`]; carried so the remedy can
        /// spell it without this module knowing how a remedy is worded.
        kind: ProviderKind,
        /// 401 or 403.
        code: u16,
    },
    /// The server has no such model.
    ModelNotFound {
        /// What was asked for.
        model: String,
        /// The server's own sentence.
        detail: String,
    },
    /// The server refused the request's shape.
    RequestRefused {
        /// The 4xx.
        code: u16,
        /// The server's own sentence.
        detail: String,
    },
    /// The server refused the request for exceeding the model's context or
    /// token capacity, in a field or in words that say so.
    ///
    /// **The reader's**, per [ADR-0036] D2: the window is
    /// `provider.openai_compatible.context_tokens`, which this kind has no
    /// default for. The type and its sentence are
    /// [`crate::providers::capacity`]'s, shared with the other two clients.
    ///
    /// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
    CapacityRefused(Refused),
    /// The complete request for the next exchange would exceed the window this
    /// client was configured with, and was not sent.
    ///
    /// See [`crate::providers::capacity::preflight`].
    ContextWindowExceeded(Exceeded),
    /// The server failed on its own side.
    Unavailable {
        /// The 5xx, where there was one.
        code: Option<u16>,
        /// What the server or the transport said.
        detail: String,
        /// How long the provider asked to be left, from its `Retry-After`.
        retry_after: Option<core::time::Duration>,
    },
    /// **A response that had begun and then sent nothing** for the stall
    /// bound, abandoned rather than waited on to the exchange's ceiling.
    ///
    /// **Neither's**, as a provider outage is, and transient: the retry loop
    /// in `providers::resilience` asks again.
    Stalled {
        /// How long nothing arrived for.
        silent_for: core::time::Duration,
    },
    /// **An error frame inside a stream the server already answered 200 for.**
    ///
    /// Measured 2026-09-14 against `llama-server`, twice: eight good frames
    /// carrying a tool call's opening fragments, then
    /// `data: {"error":{"code":500,"message":"The model produced output that
    /// does not match the expected peg-native format","type":"server_error"}}`
    /// and end of body, with **no `data: [DONE]`**.
    ///
    /// The published shape does not describe this and neither the `gemini`
    /// client nor the `ollama` client has it: both measured that "a failure
    /// arrives as an ordinary response, not as frames", and **that is true of
    /// this API's status path and false of its stream**. A client without this
    /// variant would fold the eight good frames and report a successful tool
    /// call with truncated arguments — a wrong answer rather than a failure,
    /// which is the worst outcome available here.
    StreamFailed {
        /// The code the frame carried, where it carried one.
        code: Option<u16>,
        /// The server's own sentence.
        detail: String,
    },
    /// A frame that is not JSON, or a stream carrying no frames at all.
    Unreadable {
        /// How many bytes the stream had delivered. **The length, never the
        /// content** — see the module documentation.
        bytes: usize,
        /// What the parser said.
        parser: String,
    },
    /// A tool descriptor whose parameters are not JSON.
    ToolSchemaUnreadable {
        /// The tool.
        tool: String,
        /// What the parser said.
        parser: String,
    },
}

impl fmt::Display for OpenAiCompatibleFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { endpoint, detail } => write!(
                f,
                "nothing answered at {endpoint}: {detail}. `{key}` is where this harness was told \
                 to look, so either start the server listening there or set it to where one is",
                key = ProviderKind::OpenAiCompatible.endpoint_key(),
            ),
            Self::CredentialRejected { alias, kind, code } => write!(
                f,
                "the endpoint rejected the key stored as {alias} with HTTP {code}. An \
                 OpenAI-compatible endpoint may want a key or want none, and this one wants a \
                 different one than it was given -- or was given none and wants one; {kind} \
                 sends a key only when this machine holds one",
            ),
            Self::ModelNotFound { model, detail } => write!(
                f,
                "the endpoint does not have the model {model:?}: {detail}. Set `model.default` to \
                 a model it serves, or point `{key}` at an endpoint that serves this one",
                key = ProviderKind::OpenAiCompatible.endpoint_key(),
            ),
            Self::RequestRefused { code, detail } => write!(
                f,
                "the endpoint refused this request with HTTP {code}: {detail}",
            ),
            Self::CapacityRefused(refused) => fmt::Display::fmt(refused, f),
            Self::ContextWindowExceeded(exceeded) => fmt::Display::fmt(exceeded, f),
            Self::Unavailable { code, detail, .. } => match code {
                Some(code) => write!(f, "the endpoint answered HTTP {code}: {detail}"),
                None => write!(f, "the endpoint could not be reached: {detail}"),
            },
            Self::StreamFailed { code, detail } => match code {
                Some(code) => write!(
                    f,
                    "the endpoint accepted this request and then failed part-way through its own \
                     answer, reporting {code}: {detail}. What had arrived before that is \
                     discarded, because a partial answer presented as a whole one is worse than \
                     none",
                ),
                None => write!(
                    f,
                    "the endpoint accepted this request and then failed part-way through its own \
                     answer: {detail}. What had arrived before that is discarded, because a \
                     partial answer presented as a whole one is worse than none",
                ),
            },
            Self::Stalled { silent_for } => write!(
                f,
                "the endpoint began answering and then sent nothing for {}, so the exchange was \
                 abandoned rather than waited on",
                crate::providers::resilience::spoken(*silent_for)
            ),
            Self::Unreadable { bytes, parser } => write!(
                f,
                "the endpoint's {bytes}-byte response could not be read: {parser}",
            ),
            Self::ToolSchemaUnreadable { tool, parser } => write!(
                f,
                "the tool {tool:?} has parameters this harness could not render as JSON: {parser}",
            ),
        }
    }
}

impl std::error::Error for OpenAiCompatibleFailure {}

/// What a transport error said, and how many `source` links it may carry.
///
/// **Both moved to [`crate::providers::transport`] on 2026-09-14** and
/// re-exported here, so this client's three call sites and its two checks are
/// unchanged. The `gemini` and `ollama` clients raise the same useless
/// sentence from the same three points and now call the same function; a walk
/// of a dependency's error chain is not this kind's, and it was here only
/// because this kind was the first measured against a closed port.
pub use crate::providers::transport::{CHAIN_DEPTH, transport_detail, transport_detail_within};

impl OpenAiCompatibleFailure {
    /// `detail`, unless it quotes `key`.
    ///
    /// **Both the value and its ASCII core**, because `{:?}` escapes a
    /// combining mark: a message containing the key's escaped rendering would
    /// pass a value-only check while publishing every byte. The same rule and
    /// the same reasoning as the `gemini` client's.
    ///
    /// # The empty-key guard, which the `gemini` client does not have and does
    /// not need
    ///
    /// `"anything".contains("")` is `true`, so a client holding no key would
    /// withhold **every** sentence a server ever sent it and a reader would
    /// get [`DETAIL_WITHHELD`] where the server had told them exactly what was
    /// wrong. The `gemini` client cannot reach that: its key is mandatory and
    /// `Secret` refuses an empty value, so the argument is never empty.
    ///
    /// **This kind's key is optional** — [`crate::providers::selection::KeyUse`]
    /// — so a client reaching a local server holds none, the argument is `""`,
    /// and the guard is what stands between that reader and an error message
    /// that says nothing. It is the first real consequence of the key being
    /// optional and it is why the two clients' spellings differ by one branch.
    #[must_use]
    pub fn redacted_detail(detail: &str, key: &str) -> String {
        if key.is_empty() {
            return detail.to_owned();
        }
        let core = crate::redaction::ascii_core(key);
        if detail.contains(key) || (!core.is_empty() && detail.contains(core)) {
            return DETAIL_WITHHELD.to_owned();
        }
        detail.to_owned()
    }

    /// Which failure an HTTP status and a body are.
    ///
    /// `key` is redacted out of every sentence this lifts from the server; see
    /// [`Self::redacted_detail`].
    ///
    /// # The order of the arms is the classification
    ///
    /// 401 and 403 first, because a rejected key is the one 4xx the reader can
    /// act on and folding it into `RequestRefused` would report a defect for a
    /// typo. Then 404 **only when the body names the model**: an
    /// OpenAI-compatible endpoint behind a gateway answers 404 for a route
    /// that does not exist as readily as for a model it does not have, and
    /// telling someone to change `model.default` when they have the wrong path
    /// is D2's "a stack trace with better grammar" with a wrong suggestion
    /// attached. Then the rest of 4xx as a defect, because this harness built
    /// the request — **except** a 4xx that names a context or token capacity,
    /// which is the reader's ([ADR-0036] D2). Then everything else as the
    /// server's own.
    ///
    /// # A capacity is recognised in a field or in prose, and never guessed
    ///
    /// [`super::wire::ErrorBody::names_a_capacity`] reads the two structured
    /// markers found, and [`crate::providers::capacity::names_a_capacity`] the
    /// sentence; the forms each server sends are listed on
    /// [`super::wire::ErrorEnvelope`]. A refusal matching neither keeps the
    /// arm it had, so an unrecognised form degrades to a defect rather than
    /// to a remedy that would not work. **A sentence withheld for carrying
    /// the key is not read as a capacity by either route**, which is what the
    /// `gemini` client does with the same refusal: the reader is not told the
    /// cause from a sentence they are not shown.
    ///
    /// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
    #[must_use]
    pub fn from_status(code: u16, body: &[u8], model: &str, alias: &Alias, key: &str) -> Self {
        let (detail, status, marked) =
            match serde_json::from_slice::<super::wire::ErrorEnvelope>(body) {
                Ok(envelope) => {
                    let error = envelope.into_body();
                    let marked = error.names_a_capacity();
                    (
                        Self::redacted_detail(&error.message, key),
                        error.kind,
                        marked,
                    )
                }
                // A body that is not the documented envelope is still
                // evidence, so its length is reported rather than its bytes
                // -- the same rule `Unreadable` follows and for the same
                // reason.
                Err(_) => (
                    format!("{} bytes that are not an error envelope", body.len()),
                    None,
                    false,
                ),
            };
        let capacity = detail != DETAIL_WITHHELD
            && (marked || crate::providers::capacity::names_a_capacity(&detail));
        match code {
            401 | 403 => Self::CredentialRejected {
                alias: alias.clone(),
                kind: ProviderKind::OpenAiCompatible,
                code,
            },
            404 if detail.contains(model) => Self::ModelNotFound {
                model: model.to_owned(),
                detail,
            },
            // A rate limit or a request timeout is the endpoint's condition
            // and nobody's request (ADR-0016 D1 row 3), read before the
            // capacity arm for the reason `GeminiClient::classify` gives.
            408 | 429 => Self::Unavailable {
                code: Some(code),
                detail,
                retry_after: None,
            },
            400..=499 if capacity => Self::CapacityRefused(Refused {
                code,
                status,
                detail,
            }),
            400..=499 => Self::RequestRefused { code, detail },
            _ => Self::Unavailable {
                code: Some(code),
                detail,
                retry_after: None,
            },
        }
    }
}

impl OpenAiCompatibleFailure {
    /// The same failure, carrying the `Retry-After` its response sent.
    ///
    /// Only an [`Self::Unavailable`] can carry one; any other failure is an
    /// answer about the request, and a wait changes nothing about it.
    #[must_use]
    pub fn with_retry_after(self, wait: Option<core::time::Duration>) -> Self {
        match self {
            Self::Unavailable { code, detail, .. } => Self::Unavailable {
                code,
                detail,
                retry_after: wait,
            },
            other => other,
        }
    }
}

impl crate::providers::resilience::Transience for OpenAiCompatibleFailure {
    /// An endpoint that could not be reached or broke off, one that went
    /// silent, 408, 429 and 5xx -- before the stream, or as the code of an
    /// error frame inside it. An error frame with no code says nothing a
    /// retry could change and is not retried.
    fn transient(&self) -> Option<crate::providers::resilience::Transient> {
        use crate::providers::resilience::{Cause, Transient, is_transient_status};
        match self {
            Self::Unreachable { .. } | Self::Unavailable { code: None, .. } => Some(Transient {
                cause: Cause::Connection,
                retry_after: None,
            }),
            Self::Unavailable {
                code: Some(code),
                retry_after,
                ..
            } if is_transient_status(*code) => Some(Transient {
                cause: Cause::Status(*code),
                retry_after: *retry_after,
            }),
            Self::StreamFailed {
                code: Some(code), ..
            } if is_transient_status(*code) => Some(Transient {
                cause: Cause::Status(*code),
                retry_after: None,
            }),
            Self::Stalled { silent_for } => Some(Transient {
                cause: Cause::Stalled(*silent_for),
                retry_after: None,
            }),
            Self::CredentialRejected { .. }
            | Self::ModelNotFound { .. }
            | Self::RequestRefused { .. }
            | Self::CapacityRefused(_)
            | Self::ContextWindowExceeded(_)
            | Self::Unavailable { .. }
            | Self::StreamFailed { .. }
            | Self::Unreadable { .. }
            | Self::ToolSchemaUnreadable { .. } => None,
        }
    }
}
