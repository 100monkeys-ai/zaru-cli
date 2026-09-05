// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What sealing must hold, checked against the cipher rather than against
//! itself.
//!
//! Every bearer value here is a generated nonce and no real credential is
//! held. Nothing in this file reaches an operating-system keyring — the
//! precedence is driven through [`StagedKeyring`], and the real backend gets
//! its caller in the environment-gated arm of
//! `tests/sealing_from_outside.rs`.

use crate::credentials::alias::Alias;
use crate::credentials::fixtures::{ascii_core, personal_secret_nonce};
use crate::credentials::sealing::blob::{Sealed, VERSION};
use crate::credentials::sealing::failure::SealingError;
use crate::credentials::sealing::fixtures::{StagedKeyring, staged_key_hex};
use crate::credentials::sealing::hex;
use crate::credentials::sealing::key::{
    CREDENTIAL_KEY_VARIABLE, FromKeyring, HarnessKeys, KeyStore, Keyring, OsKeyring, SealingKey,
};
use crate::credentials::secret::Secret;

fn alias(name: &str) -> Alias {
    Alias::new(name).expect("the fixture alias is well formed")
}

fn secret() -> (Secret, String) {
    let value = personal_secret_nonce();
    (
        Secret::new(value.clone()).expect("the fixture value has a kind"),
        value,
    )
}

// --- the blob ------------------------------------------------------------

/// The round trip, and the two ways it could pass while doing nothing.
///
/// A sealer that returned its input would satisfy "the value comes back", and
/// a sealer that returned a constant would satisfy "the bytes are not the
/// value". Both are asserted, in the same check, so neither mutant survives.
#[test]
fn a_sealed_value_comes_back_and_the_bytes_are_not_the_value() {
    let key = SealingKey::mint();
    let work = alias("work");
    let (value, raw) = secret();

    let sealed = Sealed::seal(&key, &work, &value).expect("a bearer value seals");
    let rendered = sealed.as_hex();

    assert!(
        !rendered.contains(&raw),
        "the sealed rendering carries the bearer value verbatim: {rendered}"
    );
    assert!(
        !rendered.contains(ascii_core(&raw)),
        "the sealed rendering carries the bearer value's ASCII core: {rendered}"
    );

    let opened = sealed
        .open(&key, &work)
        .expect("it opens under its own key");
    assert_eq!(
        opened.expose_for_dispatch(),
        raw,
        "the value did not survive the round trip"
    );
}

/// The nonce moves on every seal, and the check reads the bytes rather than
/// trusting the cipher.
///
/// The mutant is one line — a constant nonce — and it is the mutant that
/// destroys AES-GCM outright, so the assertion is on the nonce field itself
/// rather than on the ciphertext, which would also differ for two different
/// plaintexts.
#[test]
fn every_seal_takes_a_fresh_nonce() {
    let key = SealingKey::mint();
    let work = alias("work");
    let (value, _) = secret();

    let first = Sealed::seal(&key, &work, &value).expect("the first seals");
    let second = Sealed::seal(&key, &work, &value).expect("the second seals");

    let first_nonce = &hex::decode(&first.as_hex()).expect("our own hex decodes")[1..13];
    let second_nonce = &hex::decode(&second.as_hex()).expect("our own hex decodes")[1..13];

    assert_ne!(
        first_nonce, second_nonce,
        "two seals of the same value under the same key used the same nonce, which destroys \
         AES-GCM's confidentiality and its authenticity for both of them"
    );
    assert_eq!(first_nonce.len(), 12, "a nonce is 96 bits");
}

/// A blob moved between entries does not open, because the alias is bound in.
///
/// This is the check the associated data exists for. Without it the blob opens
/// and one token's bearer is served under another token's name — which is the
/// namespace confusion ADR-0007 D5 exists to prevent, arriving through the file
/// rather than through a tool call.
#[test]
fn a_blob_moved_to_another_alias_does_not_open() {
    let key = SealingKey::mint();
    let work = alias("work");
    let home = alias("home");
    let (value, _) = secret();

    let sealed = Sealed::seal(&key, &work, &value).expect("it seals under work");

    assert!(
        sealed.open(&key, &work).is_ok(),
        "the blob does not open under its own alias, so this check would pass for the wrong \
         reason"
    );
    assert_eq!(
        sealed.open(&key, &home).expect_err(
            "a blob sealed against \"work\" opened against \"home\"; the alias is not bound into \
             the associated data, so one token's bearer is served under another token's name"
        ),
        SealingError::WillNotOpen
    );
}

/// A different key does not open a blob, and the refusal says the key changed.
#[test]
fn a_blob_does_not_open_under_a_different_key() {
    let sealed_under = SealingKey::mint();
    let offered = SealingKey::mint();
    let work = alias("work");
    let (value, _) = secret();

    let sealed = Sealed::seal(&sealed_under, &work, &value).expect("it seals");

    assert_eq!(
        sealed
            .open(&offered, &work)
            .expect_err("a blob opened under a key it was not sealed with"),
        SealingError::WillNotOpen
    );
}

/// One flipped bit in the ciphertext is refused, which is what the tag is for.
#[test]
fn a_single_flipped_byte_is_refused_by_the_tag() {
    let key = SealingKey::mint();
    let work = alias("work");
    let (value, _) = secret();

    let sealed = Sealed::seal(&key, &work, &value).expect("it seals");
    let mut bytes = hex::decode(&sealed.as_hex()).expect("our own hex decodes");
    let last = bytes.len() - 1;
    bytes[last] ^= 1;

    let tampered = Sealed::from_hex(&hex::encode(&bytes)).expect("the shape is still a blob");
    assert_eq!(
        tampered.open(&key, &work).expect_err(
            "a blob with one flipped byte opened; the authentication tag is not being checked"
        ),
        SealingError::WillNotOpen
    );
}

/// The version byte is what tells "we wrote this" from "we did not", which is
/// what decides the failure's class.
#[test]
fn the_version_byte_discriminates_a_defect_from_a_key_that_changed() {
    let key = SealingKey::mint();
    let work = alias("work");
    let (value, _) = secret();

    let sealed = Sealed::seal(&key, &work, &value).expect("it seals");
    let mut bytes = hex::decode(&sealed.as_hex()).expect("our own hex decodes");
    assert_eq!(bytes[0], VERSION, "this harness writes exactly one version");

    // Ours, and the key is the thing that changed: user-correctable.
    assert_eq!(
        sealed
            .open(&SealingKey::mint(), &work)
            .expect_err("a blob opened under a key it was not sealed with"),
        SealingError::WillNotOpen
    );

    // Not ours: a defect.
    bytes[0] = 0x7f;
    assert_eq!(
        Sealed::from_hex(&hex::encode(&bytes)).expect_err(
            "a blob carrying a format version this harness never writes was accepted, so a \
             failure to open it would be classified as a changed key rather than as a defect"
        ),
        SealingError::UnknownVersion { found: 0x7f }
    );
}

/// A truncated or non-hexadecimal blob is refused at the point it is read.
#[test]
fn a_blob_that_is_not_one_is_refused_when_it_is_read() {
    assert_eq!(
        Sealed::from_hex("00").unwrap_err(),
        SealingError::TooShort {
            found: 1,
            minimum: 29
        }
    );
    assert_eq!(Sealed::from_hex("zz").unwrap_err(), SealingError::NotHex);
    assert_eq!(Sealed::from_hex("0").unwrap_err(), SealingError::NotHex);
    // Upper case is not what this harness writes, so it is not what it reads.
    assert_eq!(Sealed::from_hex("AB").unwrap_err(), SealingError::NotHex);
}

/// Neither a key nor a blob renders what it holds under `{:?}`.
///
/// The mutant is one word — `#[derive(Debug)]` — and it is the same mutant
/// `Secret`'s own `Debug` defends against. The marker's **presence** is
/// asserted as well as the value's absence, because a `Debug` that printed
/// nothing would satisfy an absence assertion on its own.
#[test]
fn neither_a_key_nor_a_blob_shows_what_it_holds() {
    let hexadecimal = staged_key_hex(0xa7);
    let key = SealingKey::from_hex(&hexadecimal).expect("64 hex characters are a key");
    let rendered = format!("{key:?}");
    assert!(
        rendered.contains("<redacted>"),
        "a key's Debug lost its marker, so an absence assertion would pass over a Debug that \
         printed nothing: {rendered}"
    );
    assert!(
        !rendered.contains(&hexadecimal),
        "a key's Debug published the key: {rendered}"
    );
    assert!(
        !rendered.contains("a7a7"),
        "a key's Debug published part of the key: {rendered}"
    );

    let work = alias("work");
    let (value, raw) = secret();
    let sealed = Sealed::seal(&key, &work, &value).expect("it seals");
    let rendered = format!("{sealed:?}");
    assert!(
        rendered.contains("Sealed(v1,"),
        "a blob's Debug lost its version marker: {rendered}"
    );
    assert!(
        !rendered.contains(&sealed.as_hex()),
        "a blob's Debug published its whole ciphertext: {rendered}"
    );
    assert!(
        !rendered.contains(&raw) && !rendered.contains(ascii_core(&raw)),
        "a blob's Debug published the bearer value: {rendered}"
    );
}

// --- the key -------------------------------------------------------------

/// A key is 256 bits of hexadecimal and anything else is refused naming the
/// shape, never the value and never its length.
#[test]
fn a_key_that_is_not_256_bits_of_hexadecimal_is_refused_without_quoting_it() {
    let short = "ab".repeat(31);
    let long = "ab".repeat(33);
    let upper = staged_key_hex(0xab).to_uppercase();
    let nonsense = "z".repeat(64);

    for offered in [&short, &long, &upper, &nonsense] {
        let refusal = SealingKey::from_hex(offered)
            .expect_err("a value that is not 256 bits of lower-case hexadecimal became a key");
        assert_eq!(refusal, SealingError::KeyNotHex);
        let said = refusal.to_string();
        assert!(
            !said.contains(offered.as_str()),
            "the refusal quoted the key it rejected: {said}"
        );
        assert!(
            said.contains("64"),
            "the refusal does not say what shape a key has, so it names no remedy: {said}"
        );
    }

    let good = staged_key_hex(0x5c);
    assert!(
        SealingKey::from_hex(&good).is_ok(),
        "64 lower-case hexadecimal characters were refused, so the check above passes for \
         everything and discriminates nothing"
    );
}

/// A minted key is 256 bits and no two are the same.
#[test]
fn a_minted_key_is_256_bits_and_moves_every_time() {
    let first = SealingKey::mint();
    let second = SealingKey::mint();
    assert_eq!(
        first.expose_for_the_keyring().len(),
        SealingKey::HEX_CHARACTERS
    );
    assert_ne!(
        first.expose_for_the_keyring(),
        second.expose_for_the_keyring(),
        "two minted keys are identical, so the randomness is not reaching the key"
    );
}

// --- the precedence ------------------------------------------------------

/// The keyring wins, and a variable set beside it is ignored rather than
/// merged.
///
/// The variable holds a *different* key, so a precedence that read it would
/// produce a key that does not open anything already stored — which is why the
/// assertion is on which key came back and not merely on success.
#[test]
fn the_keyring_wins_and_the_variable_beside_it_is_ignored() {
    let in_keyring = SealingKey::from_hex(&staged_key_hex(0x11)).expect("a key");
    let keyring = StagedKeyring::holding(&in_keyring);
    let keys = HarnessKeys::new(&keyring, Some(staged_key_hex(0x22)));

    let resolved = keys.key().expect("a key is available");
    assert_eq!(
        resolved.expose_for_the_keyring(),
        in_keyring.expose_for_the_keyring(),
        "the environment variable overrode the OS keyring; ADR-0007 D3 makes the keyring the \
         primary and the variable the fallback"
    );
}

/// A keyring that exists and holds nothing gets a key minted into it, once,
/// and nothing is written anywhere else.
#[test]
fn an_empty_keyring_is_given_a_key_and_keeps_it() {
    let keyring = StagedKeyring::empty();
    let keys = HarnessKeys::new(&keyring, None);

    let first = keys.key().expect("a key is minted");
    assert_eq!(
        keyring.written().len(),
        1,
        "a key was handed back without being stored, so the next run would mint a different one \
         and every credential sealed under this one would be unopenable"
    );
    assert_eq!(
        keyring.written()[0],
        first.expose_for_the_keyring(),
        "the key stored is not the key handed back"
    );

    let second = keys.key().expect("the stored key comes back");
    assert_eq!(
        second.expose_for_the_keyring(),
        first.expose_for_the_keyring(),
        "a second call minted a new key over the stored one"
    );
    assert_eq!(
        keyring.written().len(),
        1,
        "a second call wrote to the keyring again"
    );
}

/// With no keyring the variable answers, and that is the ordinary path on a
/// headless machine rather than only CI's.
#[test]
fn with_no_keyring_the_variable_answers() {
    let offered = staged_key_hex(0x33);
    let keyring = StagedKeyring::absent();
    let keys = HarnessKeys::new(&keyring, Some(offered.clone()));

    let resolved = keys.key().expect("the variable answers");
    assert_eq!(resolved.expose_for_the_keyring(), offered);
    assert!(
        keyring.written().is_empty(),
        "a machine with no keyring was written to"
    );
}

/// With neither source the refusal names both, and nothing is minted.
#[test]
fn with_neither_source_the_refusal_names_both_and_mints_nothing() {
    let keyring = StagedKeyring::absent();
    let keys = HarnessKeys::new(&keyring, None);

    let refusal = keys
        .key()
        .expect_err("a key was produced on a machine with no keyring and no variable set");
    assert_eq!(refusal, SealingError::NoKey);
    let said = refusal.to_string();
    assert!(
        said.contains(CREDENTIAL_KEY_VARIABLE),
        "the refusal does not name the environment variable, so it names no remedy: {said}"
    );
    assert!(
        said.contains("keyring"),
        "the refusal does not name the OS keyring: {said}"
    );
    assert!(
        keyring.written().is_empty(),
        "a key was minted and stored on a machine that has nowhere to store one"
    );
}

/// A keyring that is present and failing refuses rather than falling through.
///
/// The fall-through is the tempting mutant and it is the dangerous one: it
/// would seal the next credential under the variable's key while every
/// credential already stored is under the keyring's, and nothing would say so
/// until the first dispatch failed.
#[test]
fn a_failing_keyring_refuses_rather_than_falling_through_to_the_variable() {
    let keyring = StagedKeyring::failing("the session bus went away");
    let keys = HarnessKeys::new(&keyring, Some(staged_key_hex(0x44)));

    let refusal = keys.key().expect_err(
        "a keyring that is present and failing fell through to the environment variable, which \
         would seal the next credential under a key nothing already stored was sealed with",
    );
    assert_eq!(
        refusal,
        SealingError::KeyringFailed {
            detail: "the session bus went away".to_owned()
        }
    );
    let said = refusal.to_string();
    assert!(
        said.contains("the session bus went away"),
        "the refusal drops what the keyring said: {said}"
    );
}

/// A keyring holding something that is not a key is a defect, not a fallback.
#[test]
fn a_keyring_holding_something_that_is_not_a_key_is_a_defect() {
    let keyring = StagedKeyring::holding_nonsense("this is not a key");
    let keys = HarnessKeys::new(&keyring, Some(staged_key_hex(0x55)));

    assert_eq!(
        keys.key().expect_err(
            "a keyring holding rubbish fell through to the variable, which would seal the next \
             credential under a key nothing already stored was sealed with"
        ),
        SealingError::KeyringHeldNonsense
    );
}

/// A keyring that will not accept the minted key refuses rather than handing
/// back a key that lives nowhere.
#[test]
fn a_key_that_cannot_be_stored_is_not_handed_out() {
    let keyring = StagedKeyring::empty_and_unwritable();
    let keys = HarnessKeys::new(&keyring, None);

    let refusal = keys.key().expect_err(
        "a key that could not be stored was handed out anyway, so the credential sealed under it \
         would be unopenable on the next run",
    );
    assert!(
        matches!(refusal, SealingError::KeyringFailed { .. }),
        "a key that could not be stored was handed out anyway, so the credential sealed under it \
         would be unopenable on the next run: {refusal:?}"
    );
}

// --- the real backend, without reaching it -------------------------------

/// A keyring that is absent is never reported as empty, which is the failure a
/// mock backend would produce.
///
/// `keyring` 4 cannot be taken without a real backend — it carries
/// `compile_error!("At least one of the features 'v1' or 'cli' must be
/// enabled")` — so the condition itself cannot compile and a check for it would
/// be [Verification lessons] §36's permanent exemption. What **can** be checked
/// is the consequence a mock would have: it answers `Entry::new` with `Ok` and
/// the first read with "no entry", so this harness would see
/// [`FromKeyring::Empty`], mint a key into a store that forgets it, and leave
/// every credential unopenable on the next run.
///
/// Both arms assert something, so this is not a skip: on a machine with no
/// keyring the answer must be `NoKeyring`, and on one with a keyring it must be
/// `Held` or `Empty`. `ZARU_SEALING_KEYRING_PRESENT` is what says which, and it
/// is never set on a runner.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_keyring_that_is_absent_is_never_reported_as_empty() {
    let root = std::env::temp_dir();
    let keyring = OsKeyring::for_store(&root);
    let answer = keyring.read();
    let declared = std::env::var("ZARU_SEALING_KEYRING_PRESENT").is_ok();

    if declared {
        assert!(
            matches!(answer, FromKeyring::Held(_) | FromKeyring::Empty),
            "ZARU_SEALING_KEYRING_PRESENT is set and the OS keyring did not answer: {answer:?}"
        );
    } else {
        assert_eq!(
            answer,
            FromKeyring::NoKeyring,
            "no keyring is declared present on this machine and the backend reported something \
             other than its absence. A mock credential store answers exactly this way -- with \
             Empty -- and the harness would then mint a key into a store that forgets it"
        );
    }
}

/// Two stores never share a keyring entry, because the account is the store's
/// own path.
#[test]
fn two_stores_do_not_share_a_keyring_entry() {
    let one = OsKeyring::for_store(std::path::Path::new("/tmp"));
    let two = OsKeyring::for_store(std::path::Path::new("/"));

    assert_ne!(
        one.account(),
        two.account(),
        "two stores in different directories resolved to one keyring entry, so they would share \
         a sealing key"
    );
    assert!(
        one.account().ends_with("credentials.json"),
        "the keyring account does not name the store's own file: {}",
        one.account()
    );
}

// --- hexadecimal ---------------------------------------------------------

/// The codec round-trips and refuses everything it does not write.
#[test]
fn hexadecimal_round_trips_and_refuses_what_it_never_writes() {
    let bytes: Vec<u8> = (0..=255u8).collect();
    let rendered = hex::encode(&bytes);
    assert_eq!(rendered.len(), 512);
    assert_eq!(hex::decode(&rendered).as_deref(), Some(bytes.as_slice()));

    assert_eq!(hex::decode("0"), None, "an odd length was accepted");
    assert_eq!(hex::decode("0g"), None, "a non-digit was accepted");
    assert_eq!(
        hex::decode("AB"),
        None,
        "upper case was accepted, and this codec never writes it"
    );
    assert_eq!(hex::encode(&[0x0a, 0xff]), "0aff");
}
