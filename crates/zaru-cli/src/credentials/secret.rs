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

/// Which token family a bearer value belongs to.
///
/// ADR-0007 D2 makes `kind` *derived* rather than user-supplied, and cites
/// ADR-0161: the two "never blur" and are discriminated by prefix. So this is
/// computed from the value every time it is asked for and is never stored
/// beside it — a stored copy is a second source of truth that can disagree
/// with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An `nn_mcp_*` token, issued to a person.
    Personal,
    /// An `nn_app_*` token, issued to an app per ADR-0161.
    App,
}

impl Kind {
    /// The kind's name as ADR-0007 D2's table spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::App => "app",
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
pub struct SecretRefused;

impl fmt::Display for SecretRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a bearer value must begin with {PERSONAL_PREFIX:?} or {APP_PREFIX:?}; this one \
             begins with neither, and its value is deliberately not quoted here. ADR-0007 D2 \
             admits only those two kinds. Whether this store also holds provider credentials \
             is an open question — ADR-0014 D4 and ADR-0016 D2 both send them here, while every \
             field of D2's entry is Nuclear Notes' — and it is recorded on operations/adr-status \
             rather than answered by this refusal"
        )
    }
}

impl std::error::Error for SecretRefused {}

/// A bearer value the harness holds and never shows.
///
/// Not `Display`, not `Serialize`, and `Debug` only as [`REDACTED`].
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Take a bearer value, refusing one whose prefix names no kind.
    ///
    /// # Errors
    ///
    /// [`SecretRefused`] when the value begins with neither `nn_mcp_` nor
    /// `nn_app_`.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretRefused> {
        let value = value.into();
        if kind_of(&value).is_none() {
            return Err(SecretRefused);
        }
        Ok(Self(value))
    }

    /// Which family this token belongs to, read off the value itself.
    #[must_use]
    pub fn kind(&self) -> Kind {
        // Unwrap is not reachable: `new` is the only constructor and it
        // refuses a value with no kind, so the two cannot disagree.
        kind_of(&self.0).expect("a constructed Secret always has a kind")
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
        &self.0
    }
}

/// Which kind a value's prefix names, if any.
fn kind_of(value: &str) -> Option<Kind> {
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
