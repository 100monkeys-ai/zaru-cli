// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The key: where it comes from, what it is, and the two ports around it.
//!
//! [ADR-0007] D3: "encrypted at rest with AES-256-GCM, key from the OS keyring
//! where available, environment variable as the CI fallback."
//!
//! # Two ports, because two different things vary
//!
//! [`KeyStore`] is what the credential store calls: *give me the key these
//! blobs are sealed under*. [`Keyring`] is the narrower seam over the operating
//! system's own store, and it exists so that the precedence below can be driven
//! by a check on a machine with no keyring — which is this development machine,
//! and every CI runner. A single port would have made the precedence reachable
//! only through D-Bus, and a rule that can only be exercised where the substrate
//! happens to exist is [Verification lessons] §26's rule-holding-by-circumstance.
//!
//! # The precedence, and why each arm is what it is
//!
//! 1. **The keyring holds a key** — use it. If the variable is also set it is
//!    **ignored**, and that is deliberate: the keyring is D3's primary and a
//!    variable that silently overrode it would make which key sealed a blob a
//!    property of the environment rather than of the machine.
//! 2. **The keyring is present and empty** — mint 256 bits and store them
//!    there, once. This is the design D3 describes rather than a convenience:
//!    the key lives in the keyring, so the first seal on a machine that has one
//!    is what puts it there. **Nothing is written to disk**, which is the whole
//!    of the "never a generated key silently persisted" rule — the mutant that
//!    breaks it writes the key to a file.
//! 3. **There is no keyring** — read [`CREDENTIAL_KEY_VARIABLE`]. This is D3's
//!    "CI fallback", and it is more than that: measured 2026-09-05, neither this
//!    development machine nor a GitHub runner has a session bus, so it is the
//!    ordinary path for anyone running the harness over SSH.
//! 4. **The keyring is present and failing** — refuse, naming it. Deliberately
//!    *not* a fall-through to the variable: a machine whose keyring is
//!    temporarily unreachable would otherwise seal its next credential under a
//!    different key and leave every existing one unopenable.
//! 5. **Neither** — refuse, naming both.
//!
//! # `ZARU_CREDENTIAL_KEY` is reserved and is not configuration
//!
//! It holds a key and never a setting, so [ADR-0014] D4 — "configuration holds
//! a reference to a credential, never a credential" — is not bent by it. It is
//! declared **here**, once, and [`crate::config::environment`] skips exactly
//! this constant by reference rather than by a second spelling. No configuration
//! key `credential.key` may ever be declared, because the `ZARU_` transform
//! would produce this same name from it.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::credentials::sealing::failure::SealingError;
use crate::credentials::sealing::hex;
use crate::credentials::store::STORE_FILE;
use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{Generate, Key};
use core::fmt;
use std::path::Path;

/// The environment variable [ADR-0007] D3 calls the fallback.
///
/// **The one reserved `ZARU_*` name that is not a configuration key**, declared
/// here and read from here by [`crate::config::environment`]. See the module
/// documentation.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
pub const CREDENTIAL_KEY_VARIABLE: &str = "ZARU_CREDENTIAL_KEY";

/// How many bytes AES-256 takes.
const KEY_BYTES: usize = 32;

/// The key a credential store's blobs are sealed under.
///
/// Not `Display`, not `Serialize`, and `Debug` only as a marker. There is one
/// accessor that yields the characters and it is named for the single place
/// they are allowed to go.
#[derive(Clone)]
pub struct SealingKey([u8; KEY_BYTES]);

impl SealingKey {
    /// How many hexadecimal characters a key is written as: 256 bits.
    pub const HEX_CHARACTERS: usize = KEY_BYTES * 2;

    /// Mint 256 bits from the operating system.
    ///
    /// Through the AEAD crate's own `getrandom` feature rather than through
    /// [`std::fs`] and `/dev/urandom`, which is what
    /// [`SessionId::mint`](crate::session::SessionId::mint) does. **The
    /// precedent deliberately does not transfer**: a repeated ULID collides two
    /// session directories, while a repeated key or nonce under AES-GCM
    /// destroys confidentiality *and* authenticity. `getrandom(2)` blocks until
    /// the kernel pool is initialised and a bare open of `/dev/urandom` does
    /// not, which is a difference that only matters for exactly this use.
    #[must_use]
    pub fn mint() -> Self {
        let key: Key<Aes256Gcm> = Key::<Aes256Gcm>::generate();
        let mut bytes = [0u8; KEY_BYTES];
        bytes.copy_from_slice(&key);
        Self(bytes)
    }

    /// Read a key written as [`Self::HEX_CHARACTERS`] hexadecimal characters.
    ///
    /// # Errors
    ///
    /// [`SealingError::KeyNotHex`], which carries neither the value nor its
    /// length.
    pub fn from_hex(text: &str) -> Result<Self, SealingError> {
        if text.len() != Self::HEX_CHARACTERS {
            return Err(SealingError::KeyNotHex);
        }
        let decoded = hex::decode(text).ok_or(SealingError::KeyNotHex)?;
        let mut bytes = [0u8; KEY_BYTES];
        bytes.copy_from_slice(&decoded);
        Ok(Self(bytes))
    }

    /// The key as characters, **for handing to the OS keyring and for nothing
    /// else**.
    ///
    /// The only door out of this type, named for its purpose in the shape
    /// [`Secret::expose_for_dispatch`](crate::credentials::Secret::expose_for_dispatch)
    /// and [`bearer_for_dispatch`](crate::credentials::bearer_for_dispatch)
    /// already set, so that one search finds every place a key leaves this type
    /// and each says what it is for.
    #[must_use]
    pub fn expose_for_the_keyring(&self) -> String {
        hex::encode(&self.0)
    }

    /// The bytes, for the cipher. Crate-private: outside this module a key is
    /// opaque.
    pub(crate) const fn bytes(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

impl fmt::Debug for SealingKey {
    /// Written by hand rather than derived, for the reason
    /// [`Secret`](crate::credentials::Secret)'s is: `#[derive(Debug)]` here
    /// would put the sealing key into every `{:?}`, every `assert_eq!` failure
    /// and every panic message in the program, and a key is worth every
    /// credential the store holds.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SealingKey(<redacted>)")
    }
}

/// What the operating system's keyring said when asked for this store's key.
///
/// Four outcomes rather than a `Result<Option<String>, _>`, because the
/// precedence in [`HarnessKeys`] treats all four differently and a caller that
/// had to reconstruct "present but empty" from an error string would be reading
/// a sentence as a discriminant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromKeyring {
    /// A key is stored under this store's entry.
    Held(String),
    /// There is a keyring and it has nothing under this store's entry.
    Empty,
    /// There is no keyring on this machine at all.
    NoKeyring,
    /// There is a keyring and it would not answer.
    Failed(String),
}

/// The operating system's own credential store, as this harness needs it.
///
/// Two implementations: [`OsKeyring`] over the `keyring` crate, and an
/// in-memory one under `cfg(test)`. See the module documentation for why this
/// is a port at all.
pub trait Keyring {
    /// Ask for this store's sealing key.
    fn read(&self) -> FromKeyring;

    /// Put this store's sealing key there, once.
    ///
    /// # Errors
    ///
    /// [`SealingError::KeyringFailed`] when the keyring will not take it.
    fn write(&self, key: &SealingKey) -> Result<(), SealingError>;
}

/// Where the sealing key comes from, as the credential store sees it.
///
/// The store calls this and nothing else; which of D3's two sources answered is
/// [`HarnessKeys`]'s business.
pub trait KeyStore {
    /// The key this store's blobs are sealed under.
    ///
    /// # Errors
    ///
    /// [`SealingError`], which names both sources when neither has one.
    fn key(&self) -> Result<SealingKey, SealingError>;
}

/// The real keyring, addressed so that two stores never share a key.
///
/// The service is a constant and the account is the store's own canonical path,
/// so `~/.zaru/credentials.json` and a second store elsewhere get different
/// entries. That matters immediately rather than hypothetically: every check
/// owns its own scratch root, so a shared account would have every check in the
/// suite fighting over one key.
#[derive(Debug, Clone)]
pub struct OsKeyring {
    account: String,
}

impl OsKeyring {
    /// The service name every Zaru credential store's key is filed under.
    pub const SERVICE: &'static str = "zaru-credential-store";

    /// Address the key for the store rooted at `root`.
    ///
    /// The *directory* is canonicalised rather than the file, because the file
    /// does not exist until the store first writes one and a key may be needed
    /// before then. `root` has already been created by
    /// [`crate::config::home::ensure`] whenever a store opened it.
    #[must_use]
    pub fn for_store(root: &Path) -> Self {
        let resolved = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        Self {
            account: resolved.join(STORE_FILE).display().to_string(),
        }
    }

    /// The keyring account this store's key is filed under.
    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }

    /// The entry, or the reason there is no keyring to hold one.
    ///
    /// `keyring` 4 fails here rather than at the first read when no credential
    /// store exists on the platform — measured 2026-09-05 on a machine with no
    /// session bus: `Entry::new` returns `NoDefaultStore`, "No default store has
    /// been set, so cannot search or create entries". That is what makes
    /// [`FromKeyring::NoKeyring`] distinguishable from
    /// [`FromKeyring::Empty`] at all.
    fn entry(&self) -> Option<keyring::Entry> {
        keyring::Entry::new(Self::SERVICE, &self.account).ok()
    }
}

impl Keyring for OsKeyring {
    fn read(&self) -> FromKeyring {
        let Some(entry) = self.entry() else {
            return FromKeyring::NoKeyring;
        };
        match entry.get_password() {
            Ok(held) => FromKeyring::Held(held),
            Err(keyring::Error::NoEntry) => FromKeyring::Empty,
            Err(error) => FromKeyring::Failed(error.to_string()),
        }
    }

    fn write(&self, key: &SealingKey) -> Result<(), SealingError> {
        let entry = self.entry().ok_or_else(|| SealingError::KeyringFailed {
            detail: "there is no credential store on this platform".to_owned(),
        })?;
        entry
            .set_password(&key.expose_for_the_keyring())
            .map_err(|error| SealingError::KeyringFailed {
                detail: error.to_string(),
            })
    }
}

/// [ADR-0007] D3's precedence, over a keyring and a variable.
///
/// The variable arrives as a value rather than being read from the process,
/// for the reason [`crate::config::environment::read`] takes its variables as a
/// parameter: [`std::env::set_var`] is `unsafe` in this edition and the
/// workspace denies `unsafe_code`, so a check that staged one could not exist.
/// [`Self::from_process`] is the product path and passes
/// [`std::env::var`]; a check passes a value it owns. One function, two callers.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
pub struct HarnessKeys<'a> {
    /// **`+ Sync` since 2026-09-15**, because the key store is now reached
    /// from a shared surface: [ADR-0007] D5's projection resolves a bearer
    /// when the model calls a projected tool, and that surface is behind a
    /// `tokio::sync::Mutex` shared by both of ADR-0008 D1's loops. A keyring
    /// that could not be shared between threads would make the whole tool
    /// surface un-shareable. Every implementation in the workspace already
    /// satisfied it; what changed is that the bound is now stated.
    keyring: &'a (dyn Keyring + Sync),
    variable: Option<String>,
}

impl<'a> HarnessKeys<'a> {
    /// Take a keyring and whatever [`CREDENTIAL_KEY_VARIABLE`] holds.
    #[must_use]
    pub fn new(keyring: &'a (dyn Keyring + Sync), variable: Option<String>) -> Self {
        Self { keyring, variable }
    }

    /// Take a keyring and read the variable from this process.
    ///
    /// The product path. **This is the one function here that reads the
    /// environment**, the shape [`SessionId::mint`](crate::session::SessionId::mint)
    /// and [`CredentialStore::default_root`](crate::credentials::CredentialStore::default_root)
    /// already use: one named impure function, findable by one search.
    #[must_use]
    pub fn from_process(keyring: &'a (dyn Keyring + Sync)) -> Self {
        Self::new(keyring, std::env::var(CREDENTIAL_KEY_VARIABLE).ok())
    }
}

impl KeyStore for HarnessKeys<'_> {
    fn key(&self) -> Result<SealingKey, SealingError> {
        match self.keyring.read() {
            // The keyring wins outright. A variable set beside it is ignored.
            FromKeyring::Held(held) => {
                SealingKey::from_hex(&held).map_err(|_| SealingError::KeyringHeldNonsense)
            }
            // The design D3 describes: the key lives in the keyring, so the
            // first seal on a machine that has one is what puts it there.
            // Nothing reaches the filesystem.
            FromKeyring::Empty => {
                let minted = SealingKey::mint();
                self.keyring.write(&minted)?;
                Ok(minted)
            }
            FromKeyring::Failed(detail) => Err(SealingError::KeyringFailed { detail }),
            FromKeyring::NoKeyring => match self.variable.as_deref() {
                Some(text) => SealingKey::from_hex(text),
                None => Err(SealingError::NoKey),
            },
        }
    }
}
