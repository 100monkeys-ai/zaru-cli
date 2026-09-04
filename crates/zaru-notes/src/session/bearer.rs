// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The bearer value a session authenticates with, and the one thing this crate
//! may say about it out loud.
//!
//! [ADR-0007] D2 gives `secret` the source "server" and the purpose "The bearer
//! value. **Never leaves the store.**" D3 says why in the form that matters:
//! "a model that can read its own bearer token can exfiltrate it through any
//! tool that takes a string."
//!
//! # Why this is not `zaru-cli`'s `Secret`
//!
//! [ADR-0003] D8 permits `zaru-notes` no sibling dependency, and
//! `scripts/check-crate-boundaries.py` reads a crate's whole `dependencies`
//! list without filtering on kind — measured, not assumed: adding `zaru-cli`
//! as a **dev**-dependency fails the gate with "crate 'zaru-notes' depends on
//! sibling 'zaru-cli' ... Permitted for 'zaru-notes': nothing." So the type is
//! this crate's own. `zaru-cli` is the composition root and is where the two
//! meet.
//!
//! # What this type deliberately does not do
//!
//! It does **not** validate the prefix. `zaru-cli`'s `Secret` already refuses a
//! value whose prefix names no [ADR-0161] kind, and a second copy of the
//! `nn_mcp_`/`nn_app_` vocabulary here would be a second source of truth for a
//! security-relevant discrimination — the thing that record's own store warns
//! about. This type takes whatever the composition root hands it and promises
//! only that it will not show it.
//!
//! # The redaction is structural, not a habit
//!
//! [`Bearer`] has no [`Display`](core::fmt::Display), so it cannot be
//! interpolated into a message by accident, and its
//! [`Debug`](core::fmt::Debug) is written by hand to print a fixed marker
//! rather than derived. There is exactly one accessor that yields the value,
//! [`Bearer::expose_for_dispatch`], named so that every call site says what it
//! is doing and so that one search finds all of them. In this crate that search
//! returns exactly one product call site: [`Endpoint::open`](super::Endpoint),
//! which is a port with no implementation here.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0161]: https://cortex.page/adrs/p/0161-nuclear-notes-apps

use core::fmt;

/// What a redacted bearer renders as.
///
/// Named rather than repeated, because the checks assert its presence as well
/// as the absence of the value — a redaction that erased the whole string
/// would satisfy an absence assertion on its own.
pub const REDACTED: &str = "<redacted>";

/// A bearer value the harness holds and never shows.
///
/// Not `Display`, not `Serialize`, and `Debug` only as [`REDACTED`].
#[derive(Clone)]
pub struct Bearer(String);

impl Bearer {
    /// Take a bearer value from the composition root.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The bearer value, for attaching to a request and for nothing else.
    ///
    /// **This is the only door out of this type.** [ADR-0007] D3 puts the
    /// bearer on the harness's own dispatch path and nowhere else: not in a
    /// prompt, not in a transcript, not in a log, not in a tool result. A call
    /// to this method that is not a dispatch is the defect that record exists
    /// to prevent, which is why the name says what it is for rather than what
    /// it returns.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    #[must_use]
    pub fn expose_for_dispatch(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Bearer {
    /// Written by hand rather than derived.
    ///
    /// The mutant this defends against is a single word: replacing this impl
    /// with `#[derive(Debug)]` puts the bearer into every `{:?}`, every
    /// `assert_eq!` failure, and every panic message in the program — and a
    /// [`Session`](super::Session) holds one, so that includes every session
    /// this crate ever prints.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bearer({REDACTED})")
    }
}
