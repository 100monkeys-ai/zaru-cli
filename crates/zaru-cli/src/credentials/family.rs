// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which of the two families a stored credential belongs to.
//!
//! # Why this is a third type and not [`Kind`](crate::credentials::Kind)
//!
//! `Kind` is what a *value* is: `personal`, `app`, or a provider's. `Family`
//! is what a *record* declares before anything has been decrypted. They carry
//! almost the same information and they are read at opposite ends of the seal:
//! the record's family is what the store knows with the ciphertext still
//! closed, and the value's kind is what exists once it is open.
//!
//! Collapsing them would mean the store holding a `Kind::Personal` it had not
//! verified — a claim about a value it cannot see. [ADR-0007] D2 makes a Notes
//! token's kind **derived**, so the store must not be able to assert one; what
//! it may assert is the family, because the user declared that at `add` time
//! and it was sealed beside the value.
//!
//! So [`Family::Notes`] carries no Notes kind. `personal` versus `app` is the
//! value's to say, through
//! [`Secret::notes`](crate::credentials::Secret::notes), every time it is
//! opened. [`Family::Provider`] does carry its [`ProviderKind`], because
//! nothing about a provider key's bytes names its provider and there is
//! nothing to derive it from — see [`crate::credentials::secret`].
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::providers::ProviderKind;
use core::fmt;

/// Which family a stored record declares, read before the seal is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// A Nuclear Notes token. Which of the two Notes kinds is the value's to
    /// say; see the module documentation.
    Notes,
    /// A model provider's API key, under the kind the user declared.
    Provider(ProviderKind),
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Notes => f.write_str("Nuclear Notes"),
            Self::Provider(kind) => write!(f, "provider `{kind}`"),
        }
    }
}
