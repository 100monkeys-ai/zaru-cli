// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A request that outgrows the model's context window, for every provider
//! client: refused before it is sent where the harness can know, and read as
//! the reader's to fix where the provider refused it.
//!
//! # [ADR-0036] is written about providers, not about one of them
//!
//! D1 is "**Each provider** preflights the request it will send", and D2's
//! remedy is "the configured `provider.<kind>.context_tokens` key". Until
//! 2026-09-27 only the `gemini` client did either: the `ollama` and
//! `openai-compatible` clients sent a request of any size, and a capacity
//! refusal from either was classified as a defect of this harness — exit 70,
//! "this is a bug in Zaru, not something you can configure" — where the
//! window was a number the reader sets. Each client had carried its own copy
//! of the recognition, or none, so **the root cause was the missing seam
//! rather than two missing arms**, and this module is the seam.
//!
//! # What is shared and what each kind supplies
//!
//! Shared, here: the two failures ([`Refused`] and [`Exceeded`]) with one
//! sentence each, the prose marker [`names_a_capacity`], and the preflight
//! [`preflight`] over the conservative byte accounting [ADR-0013]'s window is
//! measured in. Each client supplies what is its own: how its error body is
//! read, which of its structured fields name a capacity, and which kind's key
//! the remedy names — the classifier takes the kind, never a spelling.
//!
//! # An unrecognised refusal stays what it was
//!
//! Recognition is deliberately narrow. A refusal whose body does not say, in
//! a structured field or in prose, that a context or token capacity was
//! exceeded keeps the class it had — for a 4xx, the harness's defect — rather
//! than being guessed into this one. D2 says so in as many words: "Other
//! remote request refusals remain defects rather than being guessed into a
//! capacity category". A wrong remedy is worse than none: a reader told to
//! raise a window when the request was malformed raises it and is refused
//! again.
//!
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets

use core::fmt;

/// A provider refused a request for exceeding the model's context or token
/// capacity, in words or in a field that say so.
///
/// **The reader's to fix** — [ADR-0036] D2 — and its sentence says what
/// happened without claiming the harness built a malformed request, which is
/// what a `RequestRefused` says and why this is not one read differently.
///
/// Built only by a client's own reading of a 4xx whose detail was already
/// checked free of any key the client holds. A detail withheld for carrying
/// the key names nothing, so that refusal is never built as this one.
///
/// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The HTTP status.
    pub code: u16,
    /// The provider's own classification of the error, where its body
    /// carried one: AIP-193's status name for `gemini`, the `type` of an
    /// OpenAI-shaped body. `None` for a body that carries none, which is
    /// every `ollama` body.
    pub status: Option<String>,
    /// What the provider said, **checked free of the key** before it got
    /// here where the client holds one.
    pub detail: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            code,
            status,
            detail,
        } = self;
        let status = match status.as_deref() {
            Some(status) if !status.is_empty() => format!(", {status}"),
            _ => String::new(),
        };
        write!(
            f,
            "the provider refused this request for exceeding the model's context or token \
             capacity (HTTP {code}{status}): {detail}. The turn's conversation and tool results \
             have outgrown what the provider accepts in one request",
        )
    }
}

impl std::error::Error for Refused {}

/// The complete request for the next exchange would exceed the provider's
/// configured context window, so it was not sent.
///
/// Checked before I/O by [`preflight`]. `needed` is the complete native
/// request measured in the conservative byte accounting used for context
/// windows, including the provider-required prior model turns and every tool
/// result accumulated in this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exceeded {
    /// Bytes the next request needs.
    pub needed: u64,
    /// Configured capacity for this provider.
    pub window: u64,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { needed, window } = self;
        write!(
            f,
            "the next provider request needs {needed} byte(s), exceeding this provider's \
             configured context window of {window} token(s); it was not sent",
        )
    }
}

impl std::error::Error for Exceeded {}

/// Whether a refusal's already redacted detail explicitly says the request
/// exceeded a context or token capacity.
///
/// **A prose marker, and deliberately narrow.** A capacity word — "context"
/// or "token" — beside an exceeding word — "exceed", "maximum", "limit",
/// "too large" or "overflow", the last for LM Studio's "Trying to keep the
/// first N tokens when context the overflows". It reads every capacity sentence found in the providers'
/// own sources and published errors on 2026-09-27 (listed where each client
/// calls it) and reads none of the malformed-request sentences those clients
/// have recorded. A refusal it does not read keeps its old class.
#[must_use]
pub fn names_a_capacity(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    (detail.contains("context") || detail.contains("token"))
        && (detail.contains("exceed")
            || detail.contains("maximum")
            || detail.contains("limit")
            || detail.contains("too large")
            || detail.contains("overflow"))
}

/// A native request's size in the byte accounting a window is compared in.
///
/// The request as serialised, which is the request as sent: `reqwest`'s
/// `.json` serialises the same value with the same serializer. Bytes are
/// never fewer than tokens, so a request this admits fits whatever tokenizer
/// the provider runs — [ADR-0036]'s Negative consequence is that it can stop
/// earlier than the provider would.
///
/// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
#[must_use]
pub fn request_bytes(request: &impl serde::Serialize) -> u64 {
    serde_json::to_string(request).map_or(0, |rendered| rendered.len() as u64)
}

/// [ADR-0036] D1: refuse, before any network I/O, a request that cannot fit.
///
/// Every client calls this on the body it is about to send and nowhere else,
/// so the measurement is of the provider-native request — assembled prompt,
/// the model's own prior turns, the turn's tool results and the declared tool
/// surface — and never of a request nobody sends.
///
/// # Errors
///
/// [`Exceeded`], carrying the measured need and the configured window.
///
/// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
pub fn preflight(request: &impl serde::Serialize, window: u64) -> Result<(), Exceeded> {
    let needed = request_bytes(request);
    if needed > window {
        return Err(Exceeded { needed, window });
    }
    Ok(())
}

/// What a check of a capacity refusal reads, shared by the three clients'
/// checks so that the three kinds are held to one list of clauses.
#[cfg(test)]
pub mod fixtures {
    use crate::providers::ProviderKind;

    /// Every clause of a capacity refusal as a person reads it, each miss
    /// reported, for one rendering.
    pub fn misses(
        kind: ProviderKind,
        presented: &crate::failure::Presentation,
        theirs: &str,
    ) -> Vec<String> {
        use crate::failure::Class;
        let said = presented.to_string();
        let mut misses = Vec::new();
        if presented.class != Class::UserCorrectable {
            misses.push(format!(
                "a capacity refusal is the reader's to fix, and this one is {:?} at exit {}",
                presented.class,
                presented.class.exit_code()
            ));
        }
        if said.contains("malformed")
            || said.contains("this harness built")
            || said.contains("bug in Zaru")
            || said.contains("defect in Zaru")
        {
            misses.push("it claims the harness malfunctioned".to_owned());
        }
        if !(presented.headline.contains("capacity") && presented.headline.contains(theirs)) {
            misses.push(
                "the statement does not name the capacity beside the provider's words".to_owned(),
            );
        }
        if !said.contains(kind.context_tokens_key().as_str()) {
            misses.push(format!(
                "the remedy does not name `{}`",
                kind.context_tokens_key().as_str()
            ));
        }
        if !misses.is_empty() {
            misses.push(format!("rendered: {said}"));
        }
        misses
    }
}
