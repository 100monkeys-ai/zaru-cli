// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The session lifecycle's checks, clause by clause.
//!
//! Every check names the ADR-0010 clause it holds and the mutant that would
//! make it redden. Where a mutant is named it has been run, and its printed
//! sentence is quoted in the commit that carries the check.

use super::fixtures::{InMemoryMeta, ScratchRoot, StagedClock, entropy, id_at};
use crate::session::id::{
    ALPHABET, ID_LENGTH, Millis, SessionId, SessionIdRefused, SystemWallClock,
};
use crate::session::meta::{Meta, MetaStore};
use crate::session::store::{DIRECTORY_MODE, SessionStore};
use crate::tools::Tier;
use std::os::unix::fs::PermissionsExt;

// ---------------------------------------------------------------------------
// D1 — a ULID, because lexical order is creation order
// ---------------------------------------------------------------------------

/// D1: "**it sorts lexically by creation time, so listing sessions in order
/// costs a directory read.**"
///
/// The mutant: putting the entropy before the timestamp in
/// `SessionId::from_parts`, which makes lexical order random.
#[test]
fn a_session_id_sorts_lexically_in_the_order_it_was_minted() {
    // Deliberately descending seeds against ascending milliseconds: if the
    // entropy led the encoding, the sort would follow the seeds instead.
    let minted: Vec<SessionId> = (0..6u64)
        .map(|step| {
            id_at(
                1_700_000_000_000 + step,
                200 - u8::try_from(step).expect("small"),
            )
        })
        .collect();

    let mut sorted = minted.clone();
    sorted.sort();

    assert_eq!(
        minted, sorted,
        "ADR-0010 D1 chose a ULID so that lexical order is creation order, and these six do not \
         sort into the order they were minted in",
    );
}

/// The arm that stops a constant satisfying the arm above.
///
/// Two ids at the *same* millisecond must differ, and must agree on their
/// first ten characters — which is what makes those ten the timestamp.
///
/// The mutants: reusing one entropy draw (the first assertion reddens), and
/// dropping the timestamp from the encoding (the second reddens).
#[test]
fn two_ids_minted_in_one_millisecond_differ_and_share_their_timestamp() {
    let first = id_at(1_700_000_000_000, 1);
    let second = id_at(1_700_000_000_000, 2);

    assert_ne!(
        first, second,
        "two sessions started in the same millisecond would name one directory",
    );
    assert_eq!(
        &first.as_str()[..10],
        &second.as_str()[..10],
        "the first ten characters are the timestamp, and two ids minted at one millisecond do \
         not agree on them",
    );
}

/// The independent reader for the encoder.
///
/// ADR-0010 D6's pruning reads a session's age out of its own name, so the
/// decode has to be exact rather than approximate. The table covers both ends
/// of what 48 bits hold, because a shift that was one bit wrong would still
/// round-trip a middling value.
///
/// The mutant: taking eleven characters instead of ten, which shifts every
/// answer by five bits.
#[test]
fn an_id_carries_the_millisecond_it_was_minted_at() {
    for millis in [0u64, 1, 1_700_000_000_000, (1 << 48) - 1] {
        let id = id_at(millis, 7);
        assert_eq!(
            id.minted_at(),
            Millis::new(millis),
            "the id {id} does not carry back the millisecond it was minted at",
        );
    }

    assert!(
        SessionId::from_parts(Millis::new(1 << 48), entropy(0)).is_err(),
        "a millisecond past 48 bits must be refused rather than wrapped, because a wrapped \
         timestamp sorts before every session that came earlier",
    );
}

/// D1's alphabet, and the four characters it omits.
///
/// The mutant: putting `I`, `L`, `O` or `U` back into `ALPHABET`, which makes
/// an id read out of a terminal ambiguous when it is typed back in.
#[test]
fn an_id_is_twenty_six_crockford_characters_and_omits_the_four_ambiguous_ones() {
    let id = id_at(1_700_000_000_000, 42);
    assert_eq!(id.as_str().len(), ID_LENGTH);
    assert!(
        id.as_str().bytes().all(|byte| ALPHABET.contains(&byte)),
        "the id {id} carries a character Crockford base32 does not use",
    );
    for ambiguous in *b"ILOU" {
        assert!(
            !ALPHABET.contains(&ambiguous),
            "{} is in the alphabet, and it is one of the four Crockford omits",
            char::from(ambiguous),
        );
    }
    assert_eq!(ALPHABET.len(), 32, "base32 has thirty-two symbols");
}

/// A directory name read off the filesystem is a boundary, and a refusal of
/// one does not print it back.
///
/// The mutant: putting the offered name into `SessionIdRefused`'s `Display`,
/// which publishes whatever a person put in `~/.zaru/sessions/`.
#[test]
fn a_name_that_is_not_a_ulid_is_refused_and_the_refusal_does_not_quote_it() {
    let planted = super::fixtures::nonce("session-name");

    let refusal = SessionId::parse(&planted).expect_err("a nonce is not a ULID");
    let rendered = refusal.to_string();
    assert!(
        !rendered.contains(&planted),
        "the refusal published the directory name it was handed: {rendered}",
    );
    assert!(
        !rendered.contains(super::fixtures::ascii_core(&planted)),
        "the refusal published an escaped form of the directory name: {rendered}",
    );

    // Each arm of the refusal, so the classification distinguishes something.
    assert!(matches!(
        SessionId::parse("01M1Q966M0ZDQHVZ2KAHDBBZ4"),
        Err(SessionIdRefused::WrongLength { found: 25 })
    ));
    assert!(matches!(
        SessionId::parse("01M1Q966M0ZDQHVZ2KAHDBBZ4I"),
        Err(SessionIdRefused::NotInTheAlphabet { at: 25, .. })
    ));
    assert!(
        matches!(
            SessionId::parse("81M1Q966M0ZDQHVZ2KAHDBBZ48"),
            Err(SessionIdRefused::BeyondTheEncoding)
        ),
        "a leading character past 7 sets a bit the two-bit padding leaves clear",
    );
    assert!(
        SessionId::parse("01M1Q966M0ZDQHVZ2KAHDBBZ48").is_ok(),
        "a well-formed ULID must be accepted, or this table is satisfied by a parser that \
         refuses everything",
    );
}

/// The one impure path: the clock and `/dev/urandom`, both real.
///
/// It asserts nothing about what the clock said — the testing contract
/// forbids asserting on wall-clock time — only that the machine's own
/// readings produce an id this module's own parser accepts, and that two
/// mints differ.
#[test]
fn minting_reads_the_machine_and_produces_an_id_the_parser_accepts() {
    let clock = SystemWallClock;
    let first = SessionId::mint(&clock).expect("the machine has a clock and /dev/urandom");
    let second = SessionId::mint(&clock).expect("the machine has a clock and /dev/urandom");

    assert!(SessionId::parse(first.as_str()).is_ok());
    assert_ne!(
        first, second,
        "two mints produced one id, so 80 bits of entropy reached neither",
    );
}

// ---------------------------------------------------------------------------
// D1 — a session is a directory
// ---------------------------------------------------------------------------

/// D5 says filesystem permissions are the only protection a transcript has,
/// so the modes are read back **off the filesystem** rather than taken from
/// what the code asked for.
///
/// The mutant: creating the directories with `fs::create_dir_all` instead of
/// through the one creator, which leaves the umask's mode.
#[test]
fn a_session_directory_and_its_parents_carry_0700_read_off_the_filesystem() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 3))
        .expect("the session directory was not created");

    for directory in [
        store.root(),
        store.sessions_directory().as_path(),
        session.directory(),
    ] {
        assert_eq!(
            std::fs::metadata(directory)
                .expect("the directory was just created")
                .permissions()
                .mode()
                & 0o777,
            DIRECTORY_MODE,
            "{} does not carry 0700, and ADR-0010 D5 says the mode is the only protection a \
             transcript has",
            directory.display(),
        );
    }

    assert_eq!(
        session.directory(),
        store.sessions_directory().join(session.id().as_str()),
        "ADR-0010 D1 names the directory ~/.zaru/sessions/<ulid>/",
    );
}

/// D1's payoff: "listing sessions in order costs a directory read".
///
/// The staging deliberately creates them out of order, so a listing that
/// returned creation order by accident of the filesystem would not pass.
///
/// The mutant: dropping the `sort`, which returns whatever order the
/// filesystem hands back.
#[test]
fn the_sessions_listing_is_in_creation_order() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");

    let oldest = id_at(1_700_000_000_000, 9);
    let middle = id_at(1_700_000_000_500, 8);
    let newest = id_at(1_700_000_001_000, 7);
    for id in [&middle, &newest, &oldest] {
        store.start(id.clone()).expect("could not start a session");
    }

    assert_eq!(
        store.ids().expect("could not list sessions"),
        vec![oldest, middle, newest],
        "ADR-0010 D1 chose a ULID so that a directory read plus a sort is the creation order",
    );
}

/// A directory the harness did not write is reported rather than skipped.
///
/// A listing that quietly omitted a name would make pruning look complete
/// over a population it never saw ([Verification lessons] §17).
///
/// The mutant: skipping an unparseable name instead of reporting it.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_directory_that_is_not_a_ulid_is_reported_rather_than_skipped() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    store
        .start(id_at(1_700_000_000_000, 1))
        .expect("could not start a session");
    std::fs::create_dir(store.sessions_directory().join("not-a-ulid"))
        .expect("could not stage the intruder");

    let failure = store
        .ids()
        .expect_err("a directory that is not a session must be reported");
    assert!(
        failure.to_string().contains("not named by a ULID"),
        "the listing did not say what was wrong: {failure}",
    );
    assert!(
        !failure.to_string().contains("not-a-ulid"),
        "the listing published a name a person wrote: {failure}",
    );
}

// ---------------------------------------------------------------------------
// The seam ADR-0016 D3 left open
// ---------------------------------------------------------------------------

/// ADR-0016 D3: a defect report names the session and says the transcript is
/// on disk. `SessionEvidence::NoSessionExists` makes claiming one that was
/// never written unrepresentable; this is the other arm, and it names the
/// path the transcript writer actually appends to.
///
/// The mutant: returning `NoSessionExists` from `Session::evidence`, or
/// naming a path the writer does not use.
#[test]
fn a_session_hands_adr_0016_d3_the_transcript_it_will_actually_write() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 5))
        .expect("could not start a session");

    let evidence = session.evidence();
    assert_eq!(
        evidence.id().map(crate::failure::SessionId::as_str),
        Some(session.id().as_str()),
        "the defect boundary was handed a different session's id",
    );
    assert_eq!(
        evidence.transcript(),
        Some(session.transcript_path().as_path()),
        "the defect boundary was told about a file the transcript writer does not append to",
    );
    assert!(
        session.transcript_path().ends_with("transcript.jsonl"),
        "ADR-0010 D1 names the file transcript.jsonl",
    );
}

// ---------------------------------------------------------------------------
// meta.toml — a port with no product implementation
// ---------------------------------------------------------------------------

/// Nothing in the product tree writes `meta.toml`, so a session directory
/// holds two of D1's three files and the third is a declared seam.
///
/// The check is in two halves because the first alone is satisfied by a store
/// that does nothing at all: the port is driven by a double, so the *value*
/// D1 asks for is asserted to exist and to carry what the record names.
///
/// The mutant: writing a `meta.toml` from the product, which is the
/// dependency stop this arc refused.
#[test]
fn nothing_in_the_product_writes_meta_toml_and_the_port_carries_what_d1_names() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 6))
        .expect("could not start a session");

    assert!(
        !session.meta_path().exists(),
        "something in the product tree wrote meta.toml, and ADR-0003 D2's table names no TOML \
         crate — the amendment that would add one is the same one holding ADR-0007 clause 4",
    );

    let mut held = InMemoryMeta::default();
    let planted = super::fixtures::nonce("workspace");
    let meta = Meta {
        tier: Tier::Bare,
        workspace: Some(planted.clone()),
        provider: Some("a-provider".to_owned()),
        started: Millis::new(1_700_000_000_000),
        ended: None,
    };
    held.write(&meta).expect("the double refused a write");
    assert_eq!(
        held.read().expect("the double lost what it was given"),
        meta,
        "the port does not carry D1's tier, workspace and provider",
    );

    // The planted value went to a port with no product implementation, so it
    // reached no file at all. ADR-0010 D2's transcript is the one place a
    // session's own content legitimately lands, and this is not it.
    assert!(
        !session.meta_path().exists(),
        "a value handed to the meta port reached the filesystem",
    );
    let _ = super::fixtures::ascii_core(&planted);
}

/// A staged clock is what every ordering check reads, so this asserts the
/// fixture is a clock rather than a constant.
#[test]
fn the_staged_clock_moves_when_a_check_moves_it() {
    let clock = StagedClock::at(1_000);
    assert_eq!(
        crate::session::id::WallClock::now(&clock),
        Millis::new(1_000)
    );
    clock.advance(500);
    assert_eq!(
        crate::session::id::WallClock::now(&clock),
        Millis::new(1_500)
    );
}
