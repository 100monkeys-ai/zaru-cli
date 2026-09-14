// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What sealing can refuse, as a closed set with a class for every member.
//!
//! # This enum exists so that [ADR-0016] has something to read
//!
//! [`crate::failure::classify`] records that a port failure's class belongs to
//! the port's *implementation* and not to the value it hands back, and that no
//! port had a product implementation anywhere in the workspace — so no such
//! statement existed to read, and the credential store's sealing failure was
//! left deliberately unmapped. **This arc writes the first one.**
//!
//! The classification is possible now, and it is possible only because this
//! type is closed. The failure it replaces carried a `String`, which is what an
//! implementation's own wording has to be when the port cannot know its
//! implementations; a string has no discriminant and cannot be classified. A
//! closed enum raised by the one implementation that exists can be.
//!
//! # The version byte is the discriminant, and that is the whole trick
//!
//! A blob that will not open has two possible causes and they belong to
//! different people. If its version byte is one this harness writes, the
//! harness wrote the blob and the cipher rejected it, which means **the key is
//! not the key it was sealed under** — the user changed it, or restored a file
//! without restoring the keyring. That is user-correctable and the remedy says
//! so. If the version byte is one this harness has never written, the bytes did
//! not come from here, and a file this harness alone writes carrying a byte it
//! never writes is a defect.
//!
//! Nothing else can tell those apart. Reading the same failure two ways is
//! exactly [ADR-0016] D1's Negative consequence — "a misclassified error is
//! worse than an unclassified one because the presentation actively misleads"
//! — so the byte is read before the class is chosen.
//!
//! # No variant carries a key, a bearer value, or a length of either
//!
//! The same rule [`SecretRefused`](crate::credentials::SecretRefused) already
//! holds, for the same reason: a refusal is exactly the text that gets pasted
//! into a bug report. [`SealingError::KeyNotHex`] carries **nothing at all** —
//! not the offered value, not its length, not how far the parse got — because
//! the length of a rejected key is a fact about the key.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::credentials::sealing::key::{CREDENTIAL_KEY_VARIABLE, SealingKey};
use core::fmt;

/// Sealing or unsealing would not proceed.
///
/// Closed, and matched exhaustively in [`crate::failure::classify`] with no
/// wildcard arm, so a new variant fails to compile there rather than taking a
/// neighbouring class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealingError {
    /// There is no key in the OS keyring and none in the environment.
    ///
    /// The ordinary state of a headless machine that has never been given one.
    NoKey,
    /// The OS keyring is present and would not answer.
    ///
    /// Distinct from its absence: absence falls through to the environment
    /// variable, and a keyring that is *there* and failing must not, because
    /// falling through would seal the next credential under a different key
    /// and leave every existing blob unopenable.
    KeyringFailed {
        /// What the keyring binding said, in its own words. Never a key.
        detail: String,
    },
    /// A key was offered that is not [`SealingKey::HEX_CHARACTERS`]
    /// hexadecimal characters.
    ///
    /// Carries nothing. See the module documentation.
    KeyNotHex,
    /// The OS keyring held something this harness did not put there.
    ///
    /// Only the harness writes to its own keyring entry, and it writes exactly
    /// a key. Anything else is a defect rather than a thing to correct.
    KeyringHeldNonsense,
    /// A blob's version byte names a format this harness has never written.
    UnknownVersion {
        /// The byte that was found. A format tag, not a secret.
        found: u8,
    },
    /// A blob is shorter than a version byte, a nonce and a tag.
    TooShort {
        /// How many bytes were there.
        found: usize,
        /// How many the shortest possible blob has.
        minimum: usize,
    },
    /// A blob is not the hexadecimal this harness writes.
    NotHex,
    /// A blob this harness wrote will not open under this key.
    ///
    /// The version byte was one we write, so the bytes are ours and the key is
    /// the thing that changed.
    WillNotOpen,
    /// The cipher refused to seal.
    ///
    /// Unreachable for the shapes this module admits — a 256-bit key, a 96-bit
    /// nonce and a bearer value bounded by what [`Secret`](crate::credentials::Secret)
    /// takes — and reported rather than unwrapped so that a future change
    /// cannot make it a panic.
    WillNotSeal,
}

impl fmt::Display for SealingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKey => write!(
                f,
                "there is no sealing key: the OS keyring is not reachable on this machine and \
                 {CREDENTIAL_KEY_VARIABLE} is not set. The key is kept in the OS \
                 keyring where there is one and in that variable where there is not"
            ),
            Self::KeyringFailed { detail } => write!(
                f,
                "the OS keyring is present and would not answer: {detail}. The sealing key was \
                 not read from {CREDENTIAL_KEY_VARIABLE} instead, deliberately -- sealing the \
                 next credential under a different key would leave every credential already \
                 stored unopenable"
            ),
            Self::KeyNotHex => write!(
                f,
                "a sealing key must be exactly {} lower-case hexadecimal characters, which is \
                 256 bits; the one offered is not, and its value is deliberately not quoted \
                 here, nor is its length",
                SealingKey::HEX_CHARACTERS
            ),
            Self::KeyringHeldNonsense => write!(
                f,
                "the OS keyring holds something under this store's entry that is not a sealing \
                 key. Only this harness writes there, and it writes exactly {} hexadecimal \
                 characters, so this is a bug in Zaru rather than something to correct",
                SealingKey::HEX_CHARACTERS
            ),
            Self::UnknownVersion { found } => write!(
                f,
                "a sealed value carries the format version {found} and this harness only ever \
                 writes version {}. The bytes did not come from here",
                super::blob::VERSION
            ),
            Self::TooShort { found, minimum } => write!(
                f,
                "a sealed value is {found} bytes and the shortest one this harness can write is \
                 {minimum} -- a version byte, a 96-bit nonce and a 128-bit tag, before any \
                 ciphertext at all. The file has been truncated or edited"
            ),
            Self::NotHex => f.write_str(
                "a sealed value is not the lower-case hexadecimal this harness writes. The file \
                 has been edited",
            ),
            Self::WillNotOpen => f.write_str(
                "a sealed value this harness wrote will not open. Its format version is one we \
                 write, so the bytes are ours and the key is what changed: the OS keyring entry \
                 was removed or replaced, or a different key reached the harness",
            ),
            Self::WillNotSeal => f.write_str(
                "the cipher refused to seal a bearer value. Nothing this store admits can cause \
                 that, so this is a bug in Zaru",
            ),
        }
    }
}

impl std::error::Error for SealingError {}
