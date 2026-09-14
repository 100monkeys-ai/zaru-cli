// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0007] clause 4, read by somebody who is not the store.
//!
//! # What this establishes that the unit checks cannot
//!
//! Clause 4 asks that the store's file "carries no plaintext secret, read by a
//! reader that is not the store, and what it does carry is AES-256-GCM under a
//! key from the OS keyring with an environment variable as the CI fallback".
//! The unit checks reach the store's internals and are therefore the wrong
//! instrument for the first half of that sentence: a check that asks the store
//! what it wrote is asking the thing under test.
//!
//! So this file does the reading itself. It opens the file with
//! [`std::fs::read`], pulls the sealed value out of the JSON with `serde_json`,
//! decodes the hexadecimal with its own decoder, and opens the ciphertext with
//! `aes-gcm` **directly** — never through `CredentialStore::secret`, never
//! through `Sealed::open`. Neither arm of the comparison travels through the
//! code under test, which is [Verification lessons] §11.
//!
//! # The real keyring has a caller, and it is not on a runner
//!
//! `zaru_cli::credentials::OsKeyring` is the product implementation and a check
//! that never ran it would leave clause 4's keyring half asserted by nothing.
//! But no CI runner has a keyring, and a check that reached the developer's own
//! keyring on every `cargo test` would be a check that changes shared state it
//! does not own.
//!
//! `ZARU_SEALING_KEYRING_PRESENT` settles it, and **both of its arms assert
//! something** — this is not a skip. Where a keyring is declared present the
//! real backend is driven end to end and the entry removed afterwards; where
//! one is not, the backend is asserted to report its own *absence* rather than
//! emptiness, which is the failure a mock credential store would produce.
//!
//! Everything here is a generated nonce. No real credential is held, no network
//! is opened, and no Nuclear Notes server is called.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Family, FromKeyring, HarnessKeys, Instance,
    KeyStore, Keyring, OsKeyring, Reach, SealingError, SealingKey, Secret,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The environment variable that says a keyring is reachable here.
///
/// Never set on a runner. Set by hand, beside the `dbus-run-session` recipe in
/// this arc's records, when the real backend is being exercised.
const KEYRING_DECLARED: &str = "ZARU_SEALING_KEYRING_PRESENT";

/// A value no other call produces.
///
/// Written here rather than imported: the crate's fixtures are private, and a
/// check about the public door that borrowed the crate's internals would be
/// reaching around the door. The awkward tail is deliberate — a combining mark,
/// a precomposed character and an astral-plane one — so that a redaction or an
/// encoding that mangled the value produces something a check can see.
fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{label}-{}-{nanos}-{seq}-e\u{301}\u{e9}\u{1f701}",
        std::process::id()
    )
}

/// Everything before the first non-ASCII character, which no encoding alters.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// This check's own hexadecimal decoder.
///
/// Deliberately not the crate's: an arm of a comparison that used the encoder
/// under test would agree with it however wrong both were.
fn decode_hex(text: &str) -> Vec<u8> {
    assert!(
        text.len().is_multiple_of(2),
        "the store wrote an odd number of hexadecimal characters: {} of them",
        text.len()
    );
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                other => panic!(
                    "the store wrote {:?}, which is not lower-case hexadecimal",
                    other as char
                ),
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect()
}

/// A directory this check owns, removed when it ends.
struct ScratchHome {
    base: PathBuf,
}

impl ScratchHome {
    fn new(label: &str) -> Self {
        let base = std::env::temp_dir().join(nonce(label));
        std::fs::create_dir_all(&base).expect("could not stage the scratch tree");
        Self { base }
    }

    fn store_root(&self) -> PathBuf {
        self.base.join("zaru")
    }
}

impl Drop for ScratchHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The key port, implemented outside the crate that declares it.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A token, and the bearer value planted in it.
fn staged_entry(label: &str) -> (Entry, String) {
    let value = format!("nn_mcp_{}", nonce(label));
    (
        Entry::notes(
            Alias::new(label).expect("the fixture alias is well formed"),
            Description::new("a token this check planted").expect("one renderable line"),
            Secret::notes(value.clone()).expect("an nn_mcp_ value names a kind"),
            Reach::InstanceLocked(Instance::new("cortex.page")),
        )
        .expect("an nn_ value builds a Nuclear Notes entry"),
        value,
    )
}

/// ADR-0007 clause 4: the file carries ciphertext, and this check opens it.
///
/// Four assertions and each one is load-bearing:
///
/// 1. the alias **is** in the file, so a store that wrote nothing cannot pass
///    the absence assertion below it;
/// 2. the bearer value is absent verbatim, by its ASCII core, and by that core
///    hexadecimal-encoded — three arms, because the store's own encoding hides a
///    published value from the first two, which is this arc's own surviving
///    mutation;
/// 3. the bytes decode to a version, a 96-bit nonce and a tagged ciphertext;
/// 4. `aes-gcm`, driven here rather than through the crate, opens them to
///    exactly the value that was planted.
#[test]
fn the_file_carries_ciphertext_and_a_reader_that_is_not_the_store_opens_it() {
    let home = ScratchHome::new("sealing-outside");
    let key = SealingKey::mint();
    let keys = StagedKey(key.clone());
    let mut store = CredentialStore::open(home.store_root()).expect("a fresh root opens");
    let (entry, planted) = staged_entry("work");
    let alias = entry.alias().clone();
    store.add(entry, &keys, None).expect("the token is stored");

    // (1) Read the file. Not through the store.
    let raw = std::fs::read(store.path()).expect("the store wrote a file");
    let text = String::from_utf8(raw).expect("the store wrote UTF-8");
    assert!(
        text.contains(alias.as_str()),
        "the file does not carry the alias that was just added, so every absence assertion below \
         would pass over a store that had written nothing: {text}"
    );

    // (2) The value is not in it, in any encoding the store can apply.
    let core = ascii_core(&planted);
    assert!(!core.is_empty(), "the fixture produced no ASCII core");
    assert!(
        !text.contains(&planted),
        "the file carries the bearer value verbatim: {text}"
    );
    assert!(
        !text.contains(core),
        "the file carries the bearer value's ASCII core {core:?}: {text}"
    );
    let core_as_hex: String = core.bytes().map(|byte| format!("{byte:02x}")).collect();
    assert!(
        !text.contains(&core_as_hex),
        "the file carries the bearer value hexadecimal-encoded, which is how the sealed blob is \
         written: its core renders as {core_as_hex} and that is in {text}"
    );

    // (3) The bytes are the shape ADR-0007 D3 implies.
    let document: serde_json::Value =
        serde_json::from_str(&text).expect("the store wrote a JSON document");
    let sealed_hex = document["entries"][alias.as_str()]["sealed"]
        .as_str()
        .expect("the record carries a sealed value as a string")
        .to_owned();
    let bytes = decode_hex(&sealed_hex);
    assert_eq!(bytes[0], 1, "the format version this harness writes is 1");
    assert!(
        bytes.len() >= 1 + 12 + 16,
        "a sealed value of {} bytes is shorter than a version, a 96-bit nonce and a 128-bit tag",
        bytes.len()
    );

    // (4) Open it here, with the cipher rather than with the crate.
    let cipher = Aes256Gcm::new_from_slice(&decode_hex(&key.expose_for_the_keyring()))
        .expect("256 bits are a key");
    let nonce = Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::try_from(&bytes[1..13])
        .expect("twelve bytes are a nonce");
    let mut associated = vec![1u8];
    associated.extend_from_slice(alias.as_str().as_bytes());
    let opened = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &bytes[13..],
                aad: &associated,
            },
        )
        .expect("the ciphertext in the file does not open under the key it was sealed with");
    assert_eq!(
        String::from_utf8(opened).expect("the plaintext is UTF-8"),
        planted,
        "the file's ciphertext opens to something other than the value that was stored"
    );

    // And moving that blob to another alias must not open, because the alias is
    // the associated data. Same cipher, same key, same nonce -- only the alias
    // differs, so nothing but the binding can account for the refusal.
    let mut wrong = vec![1u8];
    wrong.extend_from_slice(b"somewhere-else");
    assert!(
        cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &bytes[13..],
                    aad: &wrong,
                },
            )
            .is_err(),
        "the file's ciphertext opens under an alias it was not sealed against, so a blob moved \
         between entries would serve one token's bearer under another token's name"
    );
}

/// What `rm` leaves behind, read by a reader that is not the store.
///
/// # The arm that must not travel through the code under test
///
/// The sibling above establishes that the file carries ciphertext which opens
/// to the value that was stored. This one removes the credential and asserts
/// the file no longer carries **anything** that opens to it — and it does that
/// with `serde_json` and `aes-gcm` directly, holding the key it minted, rather
/// than by asking the store whether it still has the entry. A store that
/// answered "gone" while the bytes stayed on disk would satisfy every
/// assertion its own accessors can make.
///
/// Two credentials are stored and one is removed, so the absence asserted is
/// the removed one's specifically: a `save` that wrote an empty document would
/// pass a check that stored only one.
#[test]
fn what_rm_removes_is_gone_from_the_file_as_a_reader_that_is_not_the_store_sees_it() {
    let home = ScratchHome::new("rm-outside");
    let key = SealingKey::mint();
    let keys = StagedKey(key.clone());
    let mut store = CredentialStore::open(home.store_root()).expect("a fresh root opens");

    let (going, removed_value) = staged_entry("going");
    let going_alias = going.alias().clone();
    store
        .add(going, &keys, None)
        .expect("the first token is stored");

    let (staying, kept_value) = staged_entry("staying");
    let staying_alias = staying.alias().clone();
    store
        .add(staying, &keys, None)
        .expect("the second token is stored");

    // The control: before the removal the file carries both, so the absence
    // asserted afterwards is the removal's doing and not the fixture's.
    let before = std::fs::read_to_string(store.path()).expect("the store wrote a file");
    let removed_core = ascii_core(&removed_value);
    let kept_core = ascii_core(&kept_value);
    assert!(!removed_core.is_empty() && !kept_core.is_empty());
    assert!(
        before.contains(going_alias.as_str()) && before.contains(staying_alias.as_str()),
        "the file does not carry both aliases before the removal: {before}"
    );
    let opens_to = |text: &str, alias: &Alias| -> Option<String> {
        let document: serde_json::Value = serde_json::from_str(text).ok()?;
        let sealed_hex = document["entries"][alias.as_str()]["sealed"].as_str()?;
        let bytes = decode_hex(sealed_hex);
        let cipher = Aes256Gcm::new_from_slice(&decode_hex(&key.expose_for_the_keyring()))
            .expect("256 bits are a key");
        let nonce = Nonce::<<Aes256Gcm as AeadCore>::NonceSize>::try_from(&bytes[1..13])
            .expect("twelve bytes are a nonce");
        let mut associated = vec![1u8];
        associated.extend_from_slice(alias.as_str().as_bytes());
        let opened = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &bytes[13..],
                    aad: &associated,
                },
            )
            .ok()?;
        String::from_utf8(opened).ok()
    };
    assert_eq!(
        opens_to(&before, &going_alias).as_deref(),
        Some(removed_value.as_str()),
        "the check cannot open what it is about to assert the absence of"
    );

    let removed = store
        .remove(&going_alias, Family::Notes)
        .expect("a stored Notes token is removed");
    assert!(!removed.held_composer_role);

    // Read the file again. Not through the store.
    let after = std::fs::read_to_string(store.path()).expect("the store rewrote the file");

    assert!(
        !after.contains(going_alias.as_str()),
        "the removed alias is still in the file: {after}"
    );
    assert!(
        opens_to(&after, &going_alias).is_none(),
        "a blob under the removed alias still opens to its bearer value"
    );
    assert!(
        !after.contains(&removed_value),
        "the file carries the removed bearer value verbatim: {after}"
    );
    assert!(
        !after.contains(removed_core),
        "the file carries the removed bearer value's ASCII core {removed_core:?}: {after}"
    );
    let core_as_hex: String = removed_core.bytes().map(|b| format!("{b:02x}")).collect();
    assert!(
        !after.contains(&core_as_hex),
        "the file carries the removed value hexadecimal-encoded: {core_as_hex} is in {after}"
    );
    // No ciphertext anywhere in the document opens to it either, whatever
    // alias it might have been filed under -- which is what catches a `remove`
    // that unlinked the key and left the blob.
    assert!(
        !after.contains(&hex_of(&removed_value)),
        "the removed value's plaintext bytes are in the file hexadecimal-encoded"
    );

    // The accepting sibling: the one that stayed is untouched and still opens.
    assert!(after.contains(staying_alias.as_str()));
    assert_eq!(
        opens_to(&after, &staying_alias).as_deref(),
        Some(kept_value.as_str()),
        "removing one credential disturbed the one beside it"
    );
    assert_eq!(
        store
            .secret(&staying_alias, &keys)
            .expect("the survivor still opens through the store")
            .expose_for_dispatch(),
        kept_value
    );
}

/// A value's bytes, hexadecimal, for an absence assertion.
fn hex_of(value: &str) -> String {
    value.bytes().map(|byte| format!("{byte:02x}")).collect()
}

/// The store's whole round trip through its own public door, under a key that
/// came from `HarnessKeys` rather than from a literal.
///
/// This is the precedence reached from outside: no keyring, a variable set, and
/// the value handed back is the one the variable named.
#[test]
fn the_public_door_seals_and_opens_under_the_key_the_variable_named() {
    let home = ScratchHome::new("sealing-precedence");

    /// A keyring that is not there, implemented from outside the crate.
    struct NoKeyring;
    impl Keyring for NoKeyring {
        fn read(&self) -> FromKeyring {
            FromKeyring::NoKeyring
        }
        fn write(&self, _key: &SealingKey) -> Result<(), SealingError> {
            panic!("a machine with no keyring was written to")
        }
    }

    let offered = "5c".repeat(32);
    let keyring = NoKeyring;
    let keys = HarnessKeys::new(&keyring, Some(offered.clone()));

    let mut store = CredentialStore::open(home.store_root()).expect("a fresh root opens");
    let (entry, planted) = staged_entry("fallback");
    let alias = entry.alias().clone();
    store.add(entry, &keys, None).expect("the token is stored");

    let recovered = store
        .secret(&alias, &keys)
        .expect("the store opens what it sealed");
    assert_eq!(
        recovered.expose_for_dispatch(),
        planted,
        "the value did not survive a seal and an open through the public door"
    );

    // A reopened store reads the same file and opens the same blob, so the key
    // is genuinely at rest rather than held in memory by the first store.
    let reopened = CredentialStore::open(home.store_root()).expect("it reopens");
    assert_eq!(
        reopened
            .secret(&alias, &keys)
            .expect("a reopened store opens what the first one sealed")
            .expose_for_dispatch(),
        planted,
        "a store reopened from disk could not open its own sealed value"
    );

    // And a different key does not open it, so the round trip above is about
    // the key rather than about the store remembering.
    let other = HarnessKeys::new(&keyring, Some("a1".repeat(32)));
    assert!(
        reopened.secret(&alias, &other).is_err(),
        "the store opened its sealed value under a key it was not sealed with"
    );
}

/// The real OS keyring, where there is one, and its absence where there is not.
///
/// **Both arms assert; this is not a skip.** See the module documentation for
/// why the arms are split on an environment variable and why the negative arm
/// is the one that catches a mock backend.
#[test]
fn the_real_keyring_backend_answers_where_one_exists_and_says_so_where_none_does() {
    let home = ScratchHome::new("sealing-real-keyring");
    let root = home.store_root();
    std::fs::create_dir_all(&root).expect("the root is made");
    let keyring = OsKeyring::for_store(&root);
    let keyring_account = keyring.account().to_owned();

    assert!(
        keyring.account().ends_with("credentials.json"),
        "the keyring account does not name this store's own file, so two stores would share a \
         key: {}",
        keyring.account()
    );

    if std::env::var(KEYRING_DECLARED).is_err() {
        assert_eq!(
            keyring.read(),
            FromKeyring::NoKeyring,
            "no keyring is declared present here and the backend reported something else. A mock \
             credential store answers exactly this way -- present and empty -- and the harness \
             would then mint a key into a store that forgets it, leaving every credential \
             unopenable on the next run"
        );
        return;
    }

    // A keyring is declared present, so drive the real backend end to end.
    let before = keyring.read();
    assert_eq!(
        before,
        FromKeyring::Empty,
        "this store's scratch path already has a keyring entry, which it cannot have: {before:?}"
    );

    let keys = HarnessKeys::new(&keyring, None);
    let minted = keys
        .key()
        .expect("an empty keyring did not accept a minted key");

    match keyring.read() {
        FromKeyring::Held(held) => assert_eq!(
            held,
            minted.expose_for_the_keyring(),
            "the key in the keyring is not the key that was handed back"
        ),
        other => panic!("the minted key was not stored in the keyring: {other:?}"),
    }

    // A second call takes the stored key rather than minting over it.
    assert_eq!(
        keys.key()
            .expect("the stored key comes back")
            .expose_for_the_keyring(),
        minted.expose_for_the_keyring(),
        "a second call minted a new key over the one already in the keyring, which would leave \
         every credential sealed under the first one unopenable"
    );

    // And the store round-trips a real credential under a real keyring key.
    let mut store = CredentialStore::open(&root).expect("a fresh root opens");
    let (entry, planted) = staged_entry("keyring");
    let alias = entry.alias().clone();
    store.add(entry, &keys, None).expect("the token is stored");
    assert_eq!(
        store
            .secret(&alias, &keys)
            .expect("the store opens what it sealed under the keyring's key")
            .expose_for_dispatch(),
        planted
    );

    // This check owns the entry it made, so it takes it back -- through the
    // `keyring` crate directly rather than through a port method the product
    // would have no caller for. `OsKeyring::SERVICE` and `account()` are both
    // public, so the address it removes is the address the harness wrote to
    // rather than one this check reconstructed.
    keyring::Entry::new(OsKeyring::SERVICE, &keyring_account)
        .expect("the entry this check just wrote cannot be addressed")
        .delete_credential()
        .expect("the keyring entry this check created could not be removed");
}
