// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one product implementation of [ADR-0008] trigger clause 6's port:
//! the harness removing its own held secrets from what it sends a model.
//!
//! # What was decided, and what was deliberately not
//!
//! Clause 6 was open from the day the loop landed. Jeshua decided it on
//! 2026-09-05: one [`Redactor`] port in `zaru-core`, applied on every path
//! from captured bytes into a model prompt, with **exactly one product
//! implementation that redacts values the harness itself holds** and nothing
//! pattern-based. This module is that implementation, over [ADR-0007]'s
//! credential store.
//!
//! **Unknown secrets in command output are out of scope, and that is the
//! decision rather than a gap in it.** [`HeldSecrets`] removes values the
//! store put there — a bearer this harness holds under an alias — and looks
//! for nothing else. No regular expression, no entropy heuristic, no
//! `nn_mcp_`-prefix matcher.
//!
//! The reason is [ADR-0011] D6's, arriving somewhere worse. That record
//! refuses to author a destructive-command pattern list because "a prompt
//! that cries wolf gets dismissed reflexively"; a redaction matcher fails the
//! same way in both directions at once. A false positive silently corrupts
//! the model's input — the harness rewriting a compiler error it decided
//! looked secret-shaped — and a false negative is invisible, because nothing
//! about a prompt says a secret went through it. Worse, a matcher cannot be
//! wrong *safely*: it would have the harness claiming a protection it cannot
//! deliver, which is the dishonesty [ADR-0011] D2 exists to refuse. What the
//! harness holds it can redact exactly. What it does not hold, it says it
//! does not cover.
//!
//! # Two arms: the exact value, and its ASCII core
//!
//! [ADR-0007]'s Status tracking records a mutation that **survived** the
//! store's first clause-3 check: putting the bearer into a refusal's
//! `Display` through `{:?}` left every assertion green, because `{:?}` on a
//! `String` escapes a combining mark to `\u{301}` — so the value as typed was
//! genuinely absent from a rendering that had published every byte of it.
//!
//! An assertion written against the value alone is blind to that, and so is a
//! *redaction* written against the value alone. So the second arm is the
//! value's **ASCII core**: everything before its first non-ASCII character,
//! which no escaping scheme alters. For a wholly-ASCII bearer — the ordinary
//! case, since [`Secret`] admits only `nn_mcp_` and `nn_app_` prefixes — the
//! core *is* the value and the second arm does nothing.
//!
//! **This errs towards over-redaction and that is deliberate.** A core is
//! never shorter than the seven-character prefix `Secret` requires, but it
//! can still be a substring of ordinary text, in which case a marker appears
//! where no secret was. On a boundary [ADR-0007] D3 states absolutely — "a
//! model that can read its own bearer token can exfiltrate it through any
//! tool that takes a string" — that is the direction to be wrong in, and it
//! is recorded on ADR-0008 rather than left for a reader to infer from
//! behaviour.
//!
//! Values are replaced **longest first**, so a secret that is a prefix of
//! another cannot leave its tail behind.
//!
//! # The marker names the alias and never the value
//!
//! `<redacted: work>`, where `work` is [ADR-0007] D2's alias — "a local
//! unique name. The handle everywhere", already shown to the human and to the
//! agent. Naming it is what makes the marker actionable: a reader learns
//! *which* credential was in the text rather than only that something was.
//!
//! **No record in the Zaru workspace names a redaction marker's wording.** A
//! literal search for `<redacted>` and for "redaction marker" on 2026-09-05
//! returned nothing, so this spelling is proposed on ADR-0008's Status
//! tracking rather than lifted from a decision. The existing
//! [`credentials::REDACTED`] and `zaru_notes`' are untouched: those are
//! `Debug` markers on the *types*, a different job, and ADR-0007 clause 3 is
//! already checked against them.
//!
//! # What holds a value, and for how long
//!
//! [`HeldSecrets`] holds plaintext bearer values in memory, which is what any
//! redactor must. It carries the [`Secret`] discipline in full: no
//! `Display`, no `Serialize`, and a `Debug` written by hand so that a `{:?}`
//! in a panic message cannot publish what it holds. It is built by exactly
//! one function, [`held_secrets_for_redaction`], named for the purpose in the
//! shape [`bearer_for_dispatch`] already set — so one search finds every
//! place a stored secret is exposed, and each of them says what it is for.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [`Secret`]: crate::credentials::Secret
//! [`bearer_for_dispatch`]: crate::credentials::bearer_for_dispatch
//! [`credentials::REDACTED`]: crate::credentials::REDACTED

use crate::credentials::alias::Alias;
use crate::credentials::sealing::key::KeyStore;
use crate::credentials::store::{CredentialStore, StoreError};
use core::fmt;
use std::borrow::Cow;
use zaru_core::redaction::Redactor;

/// How a marker opens, and the reason it is a named constant.
///
/// A check asserts the marker's **presence** as well as the held value's
/// absence, because a redaction that erased its whole input would satisfy an
/// absence assertion on its own. Naming the prefix once means the assertion
/// and the writer cannot drift apart.
pub const MARKER_PREFIX: &str = "<redacted: ";

/// What replaces a held value wherever one is found.
///
/// Names the alias. Never the value, never a length, never a prefix — the
/// same rule [`SecretRefused`](crate::credentials::SecretRefused) already
/// holds, for the same reason: a marker is exactly the text that gets pasted
/// into a bug report.
#[must_use]
pub fn marker(alias: &Alias) -> String {
    format!("{MARKER_PREFIX}{alias}>")
}

/// Everything before `value`'s first non-ASCII character.
///
/// See the module documentation: `{:?}` escapes a combining mark, so a
/// rendering can publish every byte of a value while the value as typed is
/// absent from it. The core is what survives any escaping scheme unchanged.
pub(crate) fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// One held value and what replaces it.
struct Held {
    value: String,
    core: String,
    marker: String,
}

/// Every secret the harness itself holds, ready to be removed from text.
///
/// Not `Display`, not `Serialize`, and `Debug` only as a count.
pub struct HeldSecrets {
    held: Vec<Held>,
}

impl HeldSecrets {
    /// Hold nothing, which is the honest state of a harness with an empty
    /// store.
    ///
    /// Redacting with this is the identity, and a check that asserts a value
    /// is absent from a prompt is worth nothing unless the same run through
    /// this carries the value through byte for byte.
    #[must_use]
    pub const fn none() -> Self {
        Self { held: Vec::new() }
    }

    /// How many values are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

impl fmt::Debug for HeldSecrets {
    /// Written by hand rather than derived.
    ///
    /// The mutant this defends against is a single word: replacing this impl
    /// with `#[derive(Debug)]` puts every held bearer value into every
    /// `{:?}`, every `assert_eq!` failure and every panic message in the
    /// program. `Secret`'s own `Debug` carries the same sentence.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HeldSecrets({} held)", self.held.len())
    }
}

impl Redactor for HeldSecrets {
    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut out = Cow::Borrowed(text);
        for held in &self.held {
            if out.contains(&held.value) {
                out = Cow::Owned(out.replace(&held.value, &held.marker));
            }
            // The core arm runs only where it says something the value arm
            // did not: for a wholly-ASCII bearer the two are the same string,
            // and replacing it twice would be a second pass over a marker.
            if held.core != held.value && !held.core.is_empty() && out.contains(&held.core) {
                out = Cow::Owned(out.replace(&held.core, &held.marker));
            }
        }
        out
    }
}

/// Take every stored bearer value out of the store, to redact it from what
/// reaches a model.
///
/// **This is one of the two places a stored secret becomes a value something
/// else holds**, and it is named for the purpose rather than for the types so
/// that one search finds both and each says what it is for — the shape
/// [`bearer_for_dispatch`] set. [ADR-0007] D3 puts the bearer on the
/// harness's own dispatch path and nowhere else: "the token string appears in
/// no prompt, no transcript, no log, and no tool result". Removing it from a
/// prompt is how the first of those four becomes true, so this exposure
/// serves that clause rather than working around it.
///
/// Each secret is exposed exactly once, here, and the value is kept nowhere
/// but inside the returned [`HeldSecrets`], which will not render it.
///
/// Values are sorted longest first, so that a secret which is a prefix of
/// another is replaced after the longer one and cannot leave a tail behind.
///
/// # Errors
///
/// [`StoreError`] when a secret cannot be taken out of the store — an unknown
/// alias, or a sealed value that will not open. It is carried out rather than
/// skipped: a redactor built from *some* of the harness's secrets would be a
/// redactor that silently does not cover the rest, and on this boundary a
/// partial redactor is worse than none, because it reads as complete.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [`bearer_for_dispatch`]: crate::credentials::bearer_for_dispatch
pub fn held_secrets_for_redaction(
    store: &CredentialStore,
    keys: &dyn KeyStore,
) -> Result<HeldSecrets, StoreError> {
    let aliases: Vec<Alias> = store.records().map(|(alias, _)| alias.clone()).collect();
    let mut held = Vec::with_capacity(aliases.len());
    for alias in aliases {
        let secret = store.secret(&alias, keys)?;
        let value = secret.expose_for_dispatch().to_owned();
        held.push(Held {
            core: ascii_core(&value).to_owned(),
            marker: marker(&alias),
            value,
        });
    }
    held.sort_by_key(|entry| core::cmp::Reverse(entry.value.len()));
    Ok(HeldSecrets { held })
}

#[cfg(test)]
mod tests;
