// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a configuration key holds when it names a credential.
//!
//! # This type is ADR-0014 D4 made structural
//!
//! D4: "Configuration holds a *reference* to a credential, never a
//! credential. The values live in the credential store … A config file gets
//! committed to a repository. That is not a hypothetical; it is the single
//! most common way a token leaks, and **the design decision that prevents it
//! is refusing to have a field to put one in**."
//!
//! [`CredentialRef`] has exactly one field and it is an
//! [`Alias`] — the type [ADR-0007] D2 already
//! decided, already validated, already exported by this crate. It has no
//! string form: no `From<String>`, no `new(&str)`, no public field of any
//! other type. **A bearer value cannot be put in it**, and that is a property
//! of the type rather than a rule some loader remembers to apply.
//!
//! The check that holds it is
//! `a_credential_reference_is_one_alias_and_a_second_field_would_not_compile`,
//! which destructures the type exhaustively, so a `String` added here stops
//! that check compiling rather than travelling — the same signal ADR-0007's
//! own agent projection uses.
//!
//! # What this deliberately does not settle
//!
//! Whether the credential store holds *provider* credentials at all, or only
//! Nuclear Notes tokens, is an open question on [operations/adr-status]. A
//! configuration key naming an alias is silent about what kind of credential
//! that alias addresses, so nothing here answers it.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status

use crate::credentials::Alias;
use core::fmt;

/// A configuration value that names a credential in the store.
///
/// One field, one type, and no way to hold a bearer value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CredentialRef {
    /// The [ADR-0007] alias the store keeps the value under.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub alias: Alias,
}

impl CredentialRef {
    /// Name a credential by the alias the store holds it under.
    #[must_use]
    pub fn new(alias: Alias) -> Self {
        Self { alias }
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.alias)
    }
}
