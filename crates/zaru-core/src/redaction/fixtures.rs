// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Redactors the checks are built from. Compiled only under `cfg(test)`.
//!
//! **Neither of these is in a product tree**, which is the same discipline
//! every other port in this crate holds: `zaru-core` declares
//! [`Redactor`](crate::redaction::Redactor) and implements it nowhere. The
//! product implementation is `zaru_cli::redaction::HeldSecrets`, over
//! [ADR-0007]'s credential store.
//!
//! # Why the staged secret is deliberately awkward
//!
//! [Verification lessons] §9: a fixture can be too well-behaved. The value
//! here carries a decomposed grapheme cluster — `e` plus a combining acute —
//! so that a check asserting a rendering does not `contain` it is not fooled
//! by a formatter that **escapes**: `{:?}` on a `String` renders the
//! combining mark as `\u{301}`, and the value as typed is then genuinely
//! absent from a rendering that published every byte of it. That exact
//! mutation survived the credential store's first check and is recorded on
//! [ADR-0007]'s Status tracking. So every absence assertion here is made
//! against the value **and** against its ASCII core, which no escaping
//! scheme alters.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::redaction::Redactor;
use std::borrow::Cow;

/// The awkward tail the staged secret carries: a decomposed grapheme cluster,
/// a precomposed one, and an astral-plane character.
pub(crate) const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

/// A value shaped like a bearer token and authenticating nothing.
///
/// A uniqueness device rather than a secret, which is why `std` alone makes
/// one and no random-number crate is needed.
pub(crate) fn staged_secret() -> String {
    format!(
        "nn_mcp_zaru-core-staged-{}{AWKWARD_TAIL}",
        std::process::id()
    )
}

/// The part of a value no formatter can alter.
///
/// Everything before the first non-ASCII character. The product
/// implementation derives the same thing the same way, and the reason is the
/// surviving mutation described in the module documentation.
pub(crate) fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// A redactor holding nothing, which is therefore the identity.
///
/// It exists so that a check can drive a path with the port present and
/// **nothing held**, which is the arm that discriminates: without it, a
/// redactor that erased its whole input would satisfy every absence
/// assertion in the suite.
#[derive(Debug, Default)]
pub(crate) struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(text)
    }
}

/// A redactor holding one staged value, replacing it and its ASCII core.
///
/// The same two arms `zaru-cli`'s product implementation applies, staged here
/// so that this crate's own checks can prove a path calls the port without
/// depending on a sibling crate — which [ADR-0003] D8 forbids and Cargo's
/// refusal of cycles makes impossible.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
#[derive(Debug)]
pub(crate) struct HoldingOne {
    value: String,
    marker: String,
}

impl HoldingOne {
    /// Hold `value`, replacing it with a marker that names `alias`.
    pub(crate) fn new(value: impl Into<String>, alias: &str) -> Self {
        Self {
            value: value.into(),
            marker: format!("<redacted: {alias}>"),
        }
    }

    /// The marker this redactor writes.
    pub(crate) fn marker(&self) -> &str {
        &self.marker
    }
}

impl Redactor for HoldingOne {
    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let core = ascii_core(&self.value);
        // The whole value first, so that the core arm cannot cut a value in
        // half and leave its tail behind.
        let mut out = Cow::Borrowed(text);
        if out.contains(&self.value) {
            out = Cow::Owned(out.replace(&self.value, &self.marker));
        }
        if !core.is_empty() && core != self.value && out.contains(core) {
            out = Cow::Owned(out.replace(core, &self.marker));
        }
        out
    }
}
