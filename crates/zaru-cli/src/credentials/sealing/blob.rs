// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The sealed blob: one format, version-tagged, bound to its alias.
//!
//! [ADR-0007] D3: "encrypted at rest with AES-256-GCM", mirroring [ADR-093]'s
//! `~/.aegis/auth.json`.
//!
//! # The shape on disk
//!
//! ```text
//! byte 0        version, currently 0x01
//! bytes 1..13   nonce, 96 bits, fresh from the operating system on every seal
//! bytes 13..    ciphertext with its 128-bit tag appended
//!
//! associated data = the version byte followed by the alias
//! ```
//!
//! Rendered into `credentials.json` as lower-case hexadecimal, by a codec written
//! by hand in this module's `hex` submodule because [ADR-0003] D2's table names
//! no encoding crate.
//!
//! # The associated data binds the alias, so a blob cannot be moved
//!
//! AES-GCM authenticates its associated data without encrypting it. Binding
//! the alias means a blob lifted out of one entry and pasted into another
//! **fails to open** rather than yielding the first entry's bearer under the
//! second entry's name. Measured rather than assumed: a check moves one and
//! asserts the refusal, and the mutation that drops the associated data
//! reddens it.
//!
//! The version byte is inside the associated data as well as in front of it,
//! so a blob cannot be re-labelled as a different format either.
//!
//! # A fresh nonce every time, and what that bounds
//!
//! A nonce repeated under one key destroys both confidentiality and
//! authenticity for the two messages that share it, which is why
//! [`SealingKey::mint`](super::key::SealingKey::mint) and this nonce both come
//! from the operating system rather than from anything this harness derives.
//! Random 96-bit nonces collide with probability about 2⁻³² after 2³² seals
//! under one key; a credential store performs single digits.
//!
//! # This type has no constructor that takes a plaintext
//!
//! [`Sealed::seal`] is the only way to make one from a value, and it needs a
//! key and an alias. [`Deserialize`] is the only other way, and it validates
//! the version and the length before it yields anything. So "the file carries
//! no plaintext secret" is a property of the type rather than a claim about a
//! code path — the same argument [ADR-0014] D4 makes about configuration, and
//! the same one this store made when the field did not exist at all.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-093]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/093-aegis-cli-authentication-flow

use crate::credentials::alias::Alias;
use crate::credentials::family::Family;
use crate::credentials::sealing::failure::SealingError;
use crate::credentials::sealing::hex;
use crate::credentials::sealing::key::SealingKey;
use crate::credentials::secret::Secret;
use aes_gcm::aead::{Aead, AeadCore, Generate, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use core::fmt;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The format this harness writes. One version, and a second is an amendment.
pub const VERSION: u8 = 1;

/// How many bytes AES-GCM's nonce takes: 96 bits, the size the standard names.
const NONCE_BYTES: usize = 12;

/// How many bytes AES-GCM's authentication tag takes: 128 bits.
const TAG_BYTES: usize = 16;

/// The shortest blob that could possibly be one: version, nonce and tag, with
/// no ciphertext at all.
const MINIMUM_BYTES: usize = 1 + NONCE_BYTES + TAG_BYTES;

/// A bearer value at rest.
///
/// The one field [`Record`](crate::credentials::Record) gained when sealing
/// arrived, and the only type in this crate that a secret may be inside.
#[derive(Clone, PartialEq, Eq)]
pub struct Sealed(Vec<u8>);

impl Sealed {
    /// Seal a bearer value under `key`, bound to `alias`.
    ///
    /// # Errors
    ///
    /// [`SealingError::WillNotSeal`], which nothing this store admits can
    /// cause.
    pub fn seal(key: &SealingKey, alias: &Alias, secret: &Secret) -> Result<Self, SealingError> {
        let cipher = Aes256Gcm::new(key.bytes().into());
        let nonce: Nonce<<Aes256Gcm as AeadCore>::NonceSize> = Nonce::generate();
        let associated = associated_data(alias);
        let sealed = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: secret.expose_for_dispatch().as_bytes(),
                    aad: &associated,
                },
            )
            .map_err(|_| SealingError::WillNotSeal)?;

        let mut bytes = Vec::with_capacity(1 + NONCE_BYTES + sealed.len());
        bytes.push(VERSION);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&sealed);
        Ok(Self(bytes))
    }

    /// Open a sealed value under `key`, for the alias it was sealed against.
    ///
    /// # Errors
    ///
    /// [`SealingError::UnknownVersion`] and [`SealingError::TooShort`] for
    /// bytes this harness did not write, and [`SealingError::WillNotOpen`] when
    /// it did write them and the key is not the one they were sealed under. See
    /// [`SealingError`] for why the version byte is what tells those apart.
    pub fn open(
        &self,
        key: &SealingKey,
        alias: &Alias,
        family: Family,
    ) -> Result<Secret, SealingError> {
        let (nonce, body) = self.parts()?;
        // The slice is exactly `NONCE_BYTES` long because `parts` refused
        // anything shorter than a version, a nonce and a tag, so the conversion
        // cannot fail; it is reported rather than unwrapped so that a change to
        // `parts` cannot turn this into a panic.
        let nonce = Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::try_from(nonce)
            .map_err(|_| SealingError::WillNotOpen)?;
        let cipher = Aes256Gcm::new(key.bytes().into());
        let associated = associated_data(alias);
        let opened = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: body,
                    aad: &associated,
                },
            )
            .map_err(|_| SealingError::WillNotOpen)?;
        let text = String::from_utf8(opened).map_err(|_| SealingError::WillNotOpen)?;
        // The family comes from the record and the value comes from the
        // ciphertext, and the two have to agree before a `Secret` exists.
        // For a Notes token the kind is still *derived* -- `Secret::notes`
        // reads it off the prefix, which is ADR-0007 D2 -- so a record
        // claiming `notes` over a value with no Notes prefix does not open.
        // For a provider key there is no prefix to derive from, so the
        // record's declared kind is what the value is opened under; that is
        // the same declaration the user made at `add` time, sealed beside it.
        match family {
            Family::Notes => Secret::notes(text).map_err(|_| SealingError::WillNotOpen),
            Family::Provider(kind) => {
                Secret::provider(kind, text).map_err(|_| SealingError::WillNotOpen)
            }
        }
    }

    /// The nonce and the ciphertext, once the version and the length agree.
    fn parts(&self) -> Result<(&[u8], &[u8]), SealingError> {
        let found = self.0.first().copied().unwrap_or_default();
        if self.0.len() < MINIMUM_BYTES {
            return Err(SealingError::TooShort {
                found: self.0.len(),
                minimum: MINIMUM_BYTES,
            });
        }
        if found != VERSION {
            return Err(SealingError::UnknownVersion { found });
        }
        Ok((&self.0[1..=NONCE_BYTES], &self.0[1 + NONCE_BYTES..]))
    }

    /// Read a blob back from the characters the file carries.
    ///
    /// Crate-private and used by [`Deserialize`]: an outside caller reaches a
    /// blob through the store, and one that could build a `Sealed` from
    /// arbitrary characters would be a caller that could put a plaintext in a
    /// record.
    pub(crate) fn from_hex(text: &str) -> Result<Self, SealingError> {
        let bytes = hex::decode(text).ok_or(SealingError::NotHex)?;
        let sealed = Self(bytes);
        // Validated on the way in rather than only on the way out, so a file
        // this harness did not write is refused at load with the file named
        // rather than at the first dispatch.
        sealed.parts()?;
        Ok(sealed)
    }

    /// The characters the file carries.
    #[must_use]
    pub fn as_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// How many bytes this blob is. A length of ciphertext, not of a secret.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this blob carries nothing, which [`Self::seal`] cannot produce.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// What the tag is computed over besides the ciphertext.
///
/// The version byte and then the alias. See the module documentation.
fn associated_data(alias: &Alias) -> Vec<u8> {
    let mut associated = Vec::with_capacity(1 + alias.as_str().len());
    associated.push(VERSION);
    associated.extend_from_slice(alias.as_str().as_bytes());
    associated
}

impl fmt::Debug for Sealed {
    /// Written by hand, printing the version and a length and never the bytes.
    ///
    /// Ciphertext is not a secret, but a `Debug` that printed 122 characters of
    /// it into every `assert_eq!` failure would make every such failure
    /// unreadable, and the argument for *why* it is safe is one nobody should
    /// have to make twice.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Sealed(v{}, {} bytes)",
            self.0.first().copied().unwrap_or_default(),
            self.0.len()
        )
    }
}

impl Serialize for Sealed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_hex())
    }
}

impl<'de> Deserialize<'de> for Sealed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text).map_err(D::Error::custom)
    }
}
