// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The bearer value, and the one thing about it the store may say out loud.
//!
//! ADR-0007 D2 gives `secret` the source "server" and the purpose "The bearer
//! value. **Never leaves the store.**" D3 says why in the form that matters:
//! "a model that can read its own bearer token can exfiltrate it through any
//! tool that takes a string."
//!
//! # The redaction is structural, not a habit
//!
//! [`Secret`] has no [`Display`](core::fmt::Display), so it cannot be
//! interpolated into a message by accident, and its
//! [`Debug`](core::fmt::Debug) is written by hand to print a fixed marker
//! rather than derived. There is exactly one accessor that yields the value,
//! [`Secret::expose_for_dispatch`], named so that every call site says what
//! it is doing and so that one search finds all of them.
//!
//! [`Secret`] is deliberately **not** serialisable. See
//! [`crate::credentials`] for why the on-disk type has no field for one.
//!
//! # Two families, and only one of them has a prefix rule
//!
//! This store held Nuclear Notes tokens alone until 2026-09-05, when the open
//! question "whether the credential store holds provider credentials, or only
//! Nuclear Notes tokens" was closed in favour of **one store**, under
//! directive 20 and recorded as an accepted Update on [ADR-0007]. The reason
//! is [`crate::redaction::HeldSecrets`]: it is built from what the store
//! holds, so [ADR-0008] trigger clause 6 covers a provider key **by
//! construction** only in the one-store shape. A second store would be a
//! second seam, and a seam the redactor did not know about is a secret it
//! cannot remove.
//!
//! **The prefix rule is the Nuclear Notes kinds', and it does not generalise.**
//! ADR-0007 D2 derives `kind` from `nn_mcp_` and `nn_app_` because
//! [ADR-0161] discriminates the two that way and they "never blur" — that is a
//! fact about Nuclear Notes' own issuance, which this harness is downstream
//! of. A provider key's shape belongs to the provider. Google's keys begin
//! `AIza` today; that is a convention nobody promised us, and a harness that
//! refused a key for failing somebody else's undocumented pattern would be
//! rejecting a valid credential with a message it could not justify. So a
//! provider secret's kind is **declared by the caller** — it is the
//! [`ProviderKind`] the user named on the command line — and the value is
//! validated for nothing but the shapes a listing and a header cannot carry.
//!
//! That is the same division [`crate::providers::ProviderEndpoint`] already
//! makes: refuse what a surface cannot render, and invent no other rule.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0161]: https://cortex.page/adrs/p/0161-nuclear-notes-apps

use crate::providers::ProviderKind;
use core::fmt;

/// What the marker a redacted secret renders as.
///
/// Named rather than repeated, because the checks assert its presence as well
/// as the absence of the value — a redaction that erased the whole string
/// would satisfy an absence assertion on its own.
pub const REDACTED: &str = "<redacted>";

/// The prefix ADR-0161 gives a personal token.
const PERSONAL_PREFIX: &str = "nn_mcp_";

/// The prefix ADR-0161 gives an app token.
const APP_PREFIX: &str = "nn_app_";

/// Which family a bearer value belongs to.
///
/// # Two of these are derived and one is declared, and the difference is the
/// whole of the amended D2
///
/// ADR-0007 D2 makes `kind` *derived* for a Nuclear Notes token and cites
/// ADR-0161: the two "never blur" and are discriminated by prefix. Both Notes
/// variants are therefore still read off the value, never stored beside it —
/// a stored copy is a second source of truth that can disagree with the
/// first.
///
/// [`Kind::Provider`] is **declared**, because there is no prefix rule to
/// derive it from that this harness is entitled to assert. See the module
/// documentation. It is set once, at the moment the user names a kind on the
/// command line, and it travels with the value from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An `nn_mcp_*` token, issued to a person.
    Personal,
    /// An `nn_app_*` token, issued to an app per ADR-0161.
    App,
    /// A model provider's API key, for one of [ADR-0012] D3's five kinds.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Provider(ProviderKind),
}

impl Kind {
    /// The kind's name as ADR-0007 D2's table spells it, or the provider
    /// kind's own name as ADR-0012 D3 spells it.
    ///
    /// One string for both families, because both are rendered into the same
    /// column of the same two listings.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::App => "app",
            Self::Provider(kind) => kind.as_str(),
        }
    }

    /// Whether this is a Nuclear Notes token rather than a provider key.
    ///
    /// The predicate the two listings are filtered by, so that ADR-0007 D7's
    /// `notes tokens` lists Notes tokens and nothing else and does not lie
    /// about what it lists.
    #[must_use]
    pub const fn is_notes(self) -> bool {
        match self {
            Self::Personal | Self::App => true,
            Self::Provider(_) => false,
        }
    }
}

/// The store would not take a bearer value.
///
/// This type deliberately carries **nothing at all**. A refusal is exactly
/// the text that gets pasted into a report, and the library's [Credentials]
/// page is blunt about it: "A failing test that quotes the value it was
/// handed has published it." Not even a length or a prefix is kept, because a
/// field that exists is a field something will one day render.
///
/// [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretRefused {
    /// A Nuclear Notes bearer value whose prefix names no kind.
    NoNotesPrefix,
    /// A provider key that was empty.
    Empty,
    /// A provider key carrying a control character.
    ///
    /// Refused for the reason [`ProviderEndpoint`](crate::providers::ProviderEndpoint)
    /// refuses one: it is attached to an HTTP header, where a control
    /// character is a header-injection primitive, and a stored credential is
    /// rendered into a terminal listing by alias beside a kind.
    Control,
    /// A provider key that began or ended with whitespace.
    ///
    /// Almost always a paste artefact, and a key that differs from the issued
    /// one by an invisible character fails with an authentication error that
    /// says nothing about whitespace.
    SurroundingWhitespace,
}

impl fmt::Display for SecretRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoNotesPrefix => write!(
                f,
                "a Nuclear Notes bearer value must begin with {PERSONAL_PREFIX:?} or \
                 {APP_PREFIX:?}; this one begins with neither, and its value is deliberately not \
                 quoted here. Only those two kinds are admitted for a Notes token. A model \
                 provider's key is stored too, since 2026-09-05, but it is added as a provider \
                 key under a named provider kind rather than by having its prefix guessed"
            ),
            Self::Empty => f.write_str(
                "a provider key is empty; there is nothing to authenticate a request with",
            ),
            Self::Control => f.write_str(
                "a provider key carries a control character, and its value is deliberately not \
                 quoted here. It is attached to an HTTP header, where a control character is how \
                 a header is injected, and the alias it is stored under is rendered into a \
                 terminal listing beside it",
            ),
            Self::SurroundingWhitespace => f.write_str(
                "a provider key begins or ends with whitespace, and its value is deliberately \
                 not quoted here. That is nearly always a paste artefact, and a key differing \
                 from the issued one by an invisible character fails with an authentication \
                 error that says nothing about whitespace",
            ),
        }
    }
}

impl std::error::Error for SecretRefused {}

/// A bearer value the harness holds and never shows.
///
/// Not `Display`, not `Serialize`, and `Debug` only as [`REDACTED`].
///
/// The kind travels with the value rather than being recomputed at every use.
/// For the two Notes families that is a distinction without a difference —
/// [`Secret::notes`] derives it from the prefix and there is no other way in
/// — and for a provider key it is the only possibility, because nothing about
/// the value names its provider. See the module documentation.
#[derive(Clone)]
pub struct Secret {
    value: String,
    kind: Kind,
}

impl Secret {
    /// Take a Nuclear Notes bearer value, refusing one whose prefix names no
    /// kind.
    ///
    /// **Named for the family it admits.** It was `new` until 2026-09-05,
    /// when the store gained a second family; a constructor called `new` on a
    /// type with two families is the one a caller reaches for by default, and
    /// the default here would silently have been "Nuclear Notes".
    ///
    /// # Errors
    ///
    /// [`SecretRefused::NoNotesPrefix`] when the value begins with neither
    /// `nn_mcp_` nor `nn_app_`.
    pub fn notes(value: impl Into<String>) -> Result<Self, SecretRefused> {
        let value = value.into();
        let Some(kind) = notes_kind_of(&value) else {
            return Err(SecretRefused::NoNotesPrefix);
        };
        Ok(Self { value, kind })
    }

    /// Take a model provider's API key under the kind the caller named.
    ///
    /// **The kind is a parameter and never a guess.** See the module
    /// documentation for why a prefix rule is not extended to somebody else's
    /// credential.
    ///
    /// What is refused is what a header or a listing cannot carry, and
    /// nothing else — the same rule
    /// [`ProviderEndpoint::new`](crate::providers::ProviderEndpoint::new)
    /// applies, and for the same reason: a refusal derived from a surface can
    /// be justified to the person whose key was rejected, and one derived
    /// from taste cannot.
    ///
    /// # Errors
    ///
    /// [`SecretRefused::Empty`], [`SecretRefused::Control`] and
    /// [`SecretRefused::SurroundingWhitespace`]. **None of them carries the
    /// value**, escaped or otherwise — see [`SecretRefused`].
    pub fn provider(kind: ProviderKind, value: impl Into<String>) -> Result<Self, SecretRefused> {
        let value = value.into();
        if value.is_empty() {
            return Err(SecretRefused::Empty);
        }
        if value.chars().any(char::is_control) {
            return Err(SecretRefused::Control);
        }
        if value.trim() != value {
            return Err(SecretRefused::SurroundingWhitespace);
        }
        Ok(Self {
            value,
            kind: Kind::Provider(kind),
        })
    }

    /// Which family this credential belongs to.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// The bearer value, for attaching to a request and for nothing else.
    ///
    /// **This is the only door out of this type.** ADR-0007 D3 puts the
    /// bearer on the harness's own dispatch path and nowhere else: not in a
    /// prompt, not in a transcript, not in a log, not in a tool result. A
    /// call to this method that is not a dispatch is the defect that record
    /// exists to prevent, which is why the name says what it is for rather
    /// than what it returns.
    #[must_use]
    pub fn expose_for_dispatch(&self) -> &str {
        &self.value
    }
}

/// Which Nuclear Notes kind a value's prefix names, if any.
///
/// Notes only. There is deliberately no provider arm: a provider kind is
/// declared rather than derived, so there is no function here that could be
/// widened into guessing one.
fn notes_kind_of(value: &str) -> Option<Kind> {
    if value.starts_with(PERSONAL_PREFIX) {
        Some(Kind::Personal)
    } else if value.starts_with(APP_PREFIX) {
        Some(Kind::App)
    } else {
        None
    }
}

impl fmt::Debug for Secret {
    /// Written by hand rather than derived.
    ///
    /// The mutant this defends against is a single word: replacing this impl
    /// with `#[derive(Debug)]` puts the bearer into every `{:?}`, every
    /// `assert_eq!` failure, and every panic message in the program.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}
