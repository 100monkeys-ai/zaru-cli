// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What went wrong reaching a provider, classified by provenance rather than
//! by HTTP status.
//!
//! # [ADR-0016] D1's classes, and which of them a provider can produce
//!
//! | What happened | Class | Why |
//! | --- | --- | --- |
//! | the API rejected the key | user-correctable | they can replace it, and the remedy names the alias and the kind |
//! | the API refused the request's shape | defect | this client built it |
//! | 5xx, or the socket never opened | environmental | nothing the user typed caused it and nothing they type fixes it |
//! | a body this client cannot read | defect | the mapping is ours |
//!
//! # Two rules the whole module exists to hold
//!
//! **The key is never in a failure.** Not the value, not its ASCII core, not
//! a prefix, not a length. A failure is exactly the text that gets pasted
//! into a bug report, and [Credentials] is blunt about it: "A failing test
//! that quotes the value it was handed has published it." So every message
//! this module composes is built from the alias, the kind, the status and the
//! provider's own words — and the provider's own words are **checked** before
//! they are carried, because a request that put the key somewhere it should
//! not be can come back with the key quoted in the error.
//!
//! **A body this client cannot read is reported by its length.** Never by its
//! content. A malformed response is the one place a provider's bytes would
//! otherwise be pasted verbatim into a report, and a body that failed to
//! parse is exactly the body nobody can promise is free of a credential.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials

use crate::credentials::Alias;
use crate::providers::ProviderKind;
use core::fmt;

/// What the client could not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiFailure {
    /// The provider rejected the credential.
    ///
    /// Carries the alias the key is stored under and the kind it was offered
    /// to, which is what a user needs in order to act, plus the canonical
    /// status name so that a report says which of the three key-shaped
    /// refusals it was. **Never the key.**
    CredentialRejected {
        /// Where the key is stored.
        alias: Alias,
        /// Which provider rejected it.
        kind: ProviderKind,
        /// The HTTP status.
        code: u16,
        /// AIP-193's canonical status name, where the body carried one.
        status: String,
    },
    /// The provider refused the shape of the request.
    ///
    /// The user cannot act on this: they did not build the request. It is
    /// carried with the provider's own sentence because that sentence is what
    /// a maintainer needs.
    RequestRefused {
        /// The HTTP status.
        code: u16,
        /// AIP-193's canonical status name.
        status: String,
        /// What the provider said, **checked free of the key** before it got
        /// here. See [`GeminiFailure::redacted_detail`].
        detail: String,
    },
    /// The provider failed on its own side, or was unreachable.
    Unavailable {
        /// The HTTP status, or `None` when the exchange never got one.
        code: Option<u16>,
        /// What the transport or the provider said.
        detail: String,
    },
    /// A response body this client could not read.
    ///
    /// **The length and never the content.** See the module documentation.
    Unreadable {
        /// How many bytes came back.
        bytes: usize,
        /// What the parser said. Positional and structural — `serde_json`'s
        /// message names a line, a column and an expected type, and does not
        /// quote the input.
        parser: String,
    },
    /// A tool descriptor whose parameters are not JSON.
    ///
    /// This client's caller supplied it, so it is a defect of the harness
    /// rather than of the provider or the user.
    ToolSchemaUnreadable {
        /// Which tool.
        tool: String,
        /// What the parser said.
        parser: String,
    },
}

impl fmt::Display for GeminiFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CredentialRejected {
                alias,
                kind,
                code,
                status,
            } => write!(
                f,
                "the `{kind}` provider rejected the key stored under `{alias}` (HTTP {code}\
                 {status}). The key itself is deliberately not quoted here, nor is any part of \
                 it. Replace it with `zaru providers keys add {kind}`, which reads the new key \
                 from standard input",
                status = if status.is_empty() {
                    String::new()
                } else {
                    format!(", {status}")
                },
            ),
            Self::RequestRefused {
                code,
                status,
                detail,
            } => write!(
                f,
                "the provider refused this request as malformed (HTTP {code}, {status}): \
                 {detail}. Nothing the reader typed produced that shape -- this harness built \
                 the request",
            ),
            Self::Unavailable { code, detail } => match code {
                Some(code) => write!(f, "the provider answered HTTP {code}: {detail}"),
                None => write!(f, "the provider could not be reached: {detail}"),
            },
            Self::Unreadable { bytes, parser } => write!(
                f,
                "the provider's response could not be read: {parser}. The body was {bytes} \
                 byte(s) and its content is deliberately not quoted -- a body that failed to \
                 parse is exactly the body nobody can promise is free of a credential",
            ),
            Self::ToolSchemaUnreadable { tool, parser } => write!(
                f,
                "the tool `{tool}` was offered to a model with a parameter schema that is not \
                 JSON: {parser}. ADR-0011 D1 declares no argument shapes, so the schema is \
                 whichever surface owns the tool -- and this harness supplied it",
            ),
        }
    }
}

impl std::error::Error for GeminiFailure {}

/// What a provider's own sentence is replaced by when it carries the key.
///
/// Named rather than written inline, because a check asserts its presence as
/// well as the key's absence: a replacement that erased the sentence entirely
/// would satisfy an absence assertion on its own.
pub const DETAIL_WITHHELD: &str =
    "<the provider's message is withheld: it carried the key this harness sent>";

impl GeminiFailure {
    /// A provider's sentence, or a marker when it carries the key.
    ///
    /// # This is a belt over a brace, and it is here because the brace can be
    /// removed
    ///
    /// The key is sent in a header, so a well-behaved provider has no reason
    /// to echo it. That is a statement about the provider, and the whole
    /// discipline here is not to rely on one — a misconfigured gateway, a
    /// proxy that logs and reflects, or a future call site that puts the key
    /// somewhere else all produce a body with the key in it, and the body is
    /// what gets pasted into a report.
    ///
    /// Both arms of [`crate::redaction`]'s rule apply: the exact value, and
    /// its ASCII core, which is what an escaping formatter leaves intact.
    #[must_use]
    pub fn redacted_detail(detail: &str, key: &str) -> String {
        let core = crate::redaction::ascii_core(key);
        if detail.contains(key) || (!core.is_empty() && detail.contains(core)) {
            return DETAIL_WITHHELD.to_owned();
        }
        detail.to_owned()
    }

    /// Whether this is the user's to fix, per ADR-0016 D1.
    ///
    /// One place, so the two callers — the classifier and the check that
    /// asserts the mapping — cannot disagree about which class a failure is.
    #[must_use]
    pub const fn is_user_correctable(&self) -> bool {
        matches!(self, Self::CredentialRejected { .. })
    }

    /// Whether this is environmental, per ADR-0016 D1.
    #[must_use]
    pub const fn is_environmental(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Whether this is a defect of the harness, per ADR-0016 D1.
    #[must_use]
    pub const fn is_defect(&self) -> bool {
        matches!(
            self,
            Self::RequestRefused { .. }
                | Self::Unreadable { .. }
                | Self::ToolSchemaUnreadable { .. }
        )
    }

    /// Which of AIP-193's statuses mean the credential was rejected.
    ///
    /// # Measured rather than assumed
    ///
    /// AIP-193 documents the `{"error": {code, message, status, details}}`
    /// envelope and names `PERMISSION_DENIED` (HTTP 403) for a caller without
    /// permission. **It does not say what a bad API key returns**, and
    /// neither does the Gemini API-key page, so all three of the shapes an
    /// API can plausibly use are treated as the credential's: 400
    /// `INVALID_ARGUMENT`, which is what Google returns for a malformed key,
    /// 401 `UNAUTHENTICATED`, and 403 `PERMISSION_DENIED`.
    ///
    /// 400 is the awkward one, because `INVALID_ARGUMENT` is also what a
    /// malformed *request* returns, and those are opposite classes: one is
    /// the user's and one is this harness's. They are told apart by the
    /// message naming the key, which is a heuristic over somebody else's
    /// prose — so the tie is broken **towards the user-correctable class**,
    /// and the reason is asymmetric cost. Told "replace your key" when the
    /// request was malformed, a user checks their key, finds it fine, and
    /// reports a bug. Told "this is a harness defect" when their key is
    /// expired, they file a bug that wastes a maintainer's day and their own.
    ///
    /// What was actually measured against the live endpoint on 2026-09-05 is
    /// recorded on ADR-0016 and in this arc's records.
    #[must_use]
    pub fn is_credential_status(code: u16, status: &str, message: &str) -> bool {
        match code {
            401 | 403 => true,
            400 => {
                let message = message.to_ascii_lowercase();
                status == "INVALID_ARGUMENT"
                    && (message.contains("api key") || message.contains("api_key"))
            }
            _ => false,
        }
    }
}
