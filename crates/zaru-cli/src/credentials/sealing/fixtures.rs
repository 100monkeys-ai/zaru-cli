// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Keyrings and keys the checks are built from. Compiled only under
//! `cfg(test)`.
//!
//! **No real keyring is reached from here and no real credential is held.** A
//! key here is 32 bytes of a repeated pattern or a minted one; a keyring here
//! is a `RefCell`. What a check may conclude from these is how the precedence
//! in [`HarnessKeys`](super::HarnessKeys) behaves, and nothing whatever about
//! the operating system's own store — that is what the environment-gated arm of
//! `tests/sealing_from_outside.rs` is for.

use crate::credentials::sealing::failure::SealingError;
use crate::credentials::sealing::key::{FromKeyring, KeyStore, Keyring, SealingKey};
use std::cell::RefCell;

/// A keyring staged into one of [`FromKeyring`]'s four states.
///
/// Writes are recorded so that a check can assert **that** a key was stored as
/// well as that one came back — a minting path that returned a key and stored
/// nothing would satisfy an assertion about the return value perfectly, and the
/// next run would find the keyring still empty.
pub(crate) struct StagedKeyring {
    answer: RefCell<FromKeyring>,
    written: RefCell<Vec<String>>,
    refuses_writes: bool,
}

impl StagedKeyring {
    /// A keyring holding this key.
    pub(crate) fn holding(key: &SealingKey) -> Self {
        Self::answering(FromKeyring::Held(key.expose_for_the_keyring()))
    }

    /// A keyring that exists and holds nothing.
    pub(crate) fn empty() -> Self {
        Self::answering(FromKeyring::Empty)
    }

    /// No keyring on this machine.
    pub(crate) fn absent() -> Self {
        Self::answering(FromKeyring::NoKeyring)
    }

    /// A keyring that is there and will not answer.
    pub(crate) fn failing(detail: &str) -> Self {
        Self::answering(FromKeyring::Failed(detail.to_owned()))
    }

    /// A keyring holding something that is not a key.
    pub(crate) fn holding_nonsense(held: &str) -> Self {
        Self::answering(FromKeyring::Held(held.to_owned()))
    }

    /// A keyring that exists, holds nothing, and refuses to be written to.
    pub(crate) fn empty_and_unwritable() -> Self {
        Self {
            answer: RefCell::new(FromKeyring::Empty),
            written: RefCell::new(Vec::new()),
            refuses_writes: true,
        }
    }

    fn answering(answer: FromKeyring) -> Self {
        Self {
            answer: RefCell::new(answer),
            written: RefCell::new(Vec::new()),
            refuses_writes: false,
        }
    }

    /// Every key this keyring was asked to store, in order.
    pub(crate) fn written(&self) -> Vec<String> {
        self.written.borrow().clone()
    }
}

impl Keyring for StagedKeyring {
    fn read(&self) -> FromKeyring {
        self.answer.borrow().clone()
    }

    fn write(&self, key: &SealingKey) -> Result<(), SealingError> {
        if self.refuses_writes {
            return Err(SealingError::KeyringFailed {
                detail: "this staged keyring refuses writes".to_owned(),
            });
        }
        let held = key.expose_for_the_keyring();
        self.written.borrow_mut().push(held.clone());
        // A real keyring answers with what was stored on the next read, and a
        // double that did not would let a check pass over a write that went
        // nowhere.
        *self.answer.borrow_mut() = FromKeyring::Held(held);
        Ok(())
    }
}

/// A key store that hands back one key and asks nothing of any machine.
///
/// What most checks want: [`StagedKeyring`] exercises the precedence, and
/// everything else only needs *a* key. It exposes that key so a check can be
/// the reader that is not the store — opening a blob itself rather than asking
/// the code under test to open it, which is [Verification lessons] §11.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub(crate) struct StagedKey(SealingKey);

impl StagedKey {
    /// A store over a freshly minted key.
    pub(crate) fn minted() -> Self {
        Self(SealingKey::mint())
    }

    /// The key, so a check can open what the store sealed without going back
    /// through the store.
    pub(crate) const fn key(&self) -> &SealingKey {
        &self.0
    }
}

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A key store with nothing in it, so a caller's refusal path can be driven.
pub(crate) struct NoKeyAnywhere;

impl KeyStore for NoKeyAnywhere {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Err(SealingError::NoKey)
    }
}

/// 64 hexadecimal characters that are a key, for a check that needs a literal.
pub(crate) fn staged_key_hex(fill: u8) -> String {
    super::hex::encode(&[fill; 32])
}
