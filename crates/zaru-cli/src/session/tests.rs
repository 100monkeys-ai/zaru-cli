// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The session lifecycle's checks, clause by clause.
//!
//! Every check names the ADR-0010 clause it holds and the mutant that would
//! make it redden. Where a mutant is named it has been run, and its printed
//! sentence is quoted in the commit that carries the check.

use super::fixtures::{InMemoryMeta, ScratchRoot, StagedClock, entropy, id_at};
use crate::session::checkpoint::Checkpoint;
use crate::session::id::{
    ALPHABET, ID_LENGTH, Millis, SessionId, SessionIdRefused, SystemWallClock,
};
use crate::session::meta::{Meta, MetaStore};
use crate::session::record::Record;
use crate::session::retention::RetentionWindow;
use crate::session::store::{DIRECTORY_MODE, SessionStore};
use crate::session::transcript::Transcript;
use crate::tools::Tier;
use core::time::Duration;
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

// ---------------------------------------------------------------------------
// D2 — the transcript is append-only, one event per line, and it is the
// loop's own event stream
// ---------------------------------------------------------------------------

/// D2: "Every event from ADR-0008 D3 is written as it occurs."
///
/// The round trip goes through the file rather than through the serialiser
/// alone, so what is asserted is what a reader of the transcript gets
/// ([Verification lessons] §10). The event carries a nonce with a newline and
/// a non-ASCII character in it, because a line-oriented format that did not
/// escape a newline would silently become two records.
///
/// The mutant: writing a record without its trailing newline, which merges it
/// with the next.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_transcript_is_the_loops_own_event_stream_one_event_per_line() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 1))
        .expect("could not start a session");

    let awkward = format!("{}\nwith a newline in it", super::fixtures::nonce("detail"));
    let events = vec![
        zaru_core::iteration::Event::IterationStarted { n: 1, of: 5 },
        zaru_core::iteration::Event::ValidatorEvaluated {
            name: "cargo test".to_owned(),
            outcome: zaru_core::iteration::ValidatorOutcome::Failed,
            detail: awkward.clone(),
        },
        zaru_core::iteration::Event::LoopExhausted {
            iterations: 5,
            reason: zaru_core::iteration::ExhaustionReason::CeilingReached,
            last_failure: Some(awkward.clone()),
        },
    ];

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    for event in &events {
        transcript
            .record(&Record::Loop(event.clone()))
            .expect("could not append an event");
    }

    let bytes = std::fs::read(session.transcript_path()).expect("the transcript is not there");
    assert_eq!(
        bytes.iter().filter(|byte| **byte == b'\n').count(),
        events.len(),
        "D2 is one event per line, and the file does not carry one newline per event",
    );

    let reading =
        Transcript::read(&session.transcript_path()).expect("the transcript did not read");
    assert_eq!(reading.fragment, None, "a clean write left a partial line");
    assert_eq!(
        reading.records,
        events.into_iter().map(Record::Loop).collect::<Vec<_>>(),
        "the transcript did not read back as the events that were written",
    );
    assert!(
        String::from_utf8_lossy(&bytes).contains(&awkward.replace('\n', "\\n")),
        "the detail's embedded newline was not escaped, so one event became two lines",
    );
}

/// D2's transcript is append-only: a second opening adds to the file rather
/// than replacing it.
///
/// The mutant: opening with `truncate(true)` instead of `append(true)`.
#[test]
fn a_second_opening_appends_rather_than_replacing() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 2))
        .expect("could not start a session");

    for seq in 0..3u64 {
        let mut transcript = Transcript::append_to(session.transcript_path())
            .expect("could not open the transcript");
        transcript
            .record(&Record::Loop(super::fixtures::sequenced_event(seq, 4)))
            .expect("could not append");
    }

    let reading =
        Transcript::read(&session.transcript_path()).expect("the transcript did not read");
    assert_eq!(
        reading
            .records
            .iter()
            .filter_map(super::fixtures::sequence_of)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "three separate openings did not leave three records, so the transcript is not \
         append-only",
    );
}

/// The transcript carries `0600`, read back off the filesystem.
///
/// ADR-0010's own Negative section: "Filesystem permissions are the only
/// protection, and that is worth saying out loud rather than implying
/// encryption that does not exist." So this is the whole of that protection,
/// and it is asserted against a file that already existed with the wrong mode
/// as well as one this call created — `OpenOptions::mode` applies only on
/// creation.
///
/// The mutant: dropping the `set_permissions` after the open.
#[test]
fn the_transcript_carries_0600_even_when_it_already_existed_with_another_mode() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 3))
        .expect("could not start a session");

    std::fs::write(session.transcript_path(), b"").expect("could not stage the file");
    std::fs::set_permissions(
        session.transcript_path(),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("could not stage the wrong mode");
    assert_eq!(
        std::fs::metadata(session.transcript_path())
            .expect("staged")
            .permissions()
            .mode()
            & 0o777,
        0o644,
        "the staging did not take, so this check would assert nothing",
    );

    let _transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");

    assert_eq!(
        std::fs::metadata(session.transcript_path())
            .expect("opened")
            .permissions()
            .mode()
            & 0o777,
        crate::session::store::FILE_MODE,
        "a transcript that already existed kept a mode every process on the machine can read, \
         and ADR-0010 D5 says that mode is the only protection there is",
    );
}

/// D2's three producers, each a variant, and the five that do not exist
/// getting none.
///
/// The mutant: a `kind: String` field instead of an enum, which lets a fourth
/// producer arrive as a typo rather than as a compile error.
#[test]
fn every_producer_that_exists_is_a_variant_and_the_five_that_do_not_are_absent() {
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working =
        crate::tools::WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let entry = super::fixtures::entry_for(&working, "src/main.rs", true);

    let records = vec![
        Record::Loop(zaru_core::iteration::Event::IterationStarted { n: 1, of: 3 }),
        Record::ToolCall(crate::session::ToolCall::started(&entry)),
        Record::Failure(crate::session::FailureLine::of(
            &crate::failure::Classified::Expected(crate::failure::Expected::new(
                crate::failure::Statement::new("the loop was exhausted")
                    .expect("a statement is not empty"),
            )),
        )),
    ];

    assert_eq!(
        records
            .iter()
            .map(crate::session::Record::producer)
            .collect::<Vec<_>>(),
        vec!["loop", "tool_call", "failure"],
        "the three producers that exist do not each have their own variant",
    );

    // Every line is one JSON object whose single key names its producer, so a
    // reader tells them apart without a convention.
    for record in &records {
        let rendered = serde_json::to_string(record).expect("a record must serialise");
        assert!(
            rendered.starts_with(&format!("{{\"{}\"", record.producer())),
            "the line does not name its producer: {rendered}",
        );
        assert_eq!(
            serde_json::from_str::<Record>(&rendered).expect("a record must parse"),
            *record,
        );
    }

    // The five producers D2 names that no record in this workspace builds are
    // absent rather than stubbed: a variant nothing can construct would be a
    // permanent exemption dressed as a promise.
    let names = serde_json::to_string(&records).expect("must serialise");
    for absent in [
        "user_message",
        "seal_verdict",
        "attachment",
        "learning_announcement",
    ] {
        assert!(
            !names.contains(absent),
            "the transcript has a variant for a producer nothing in this workspace builds: \
             {absent}",
        );
    }
}

/// ADR-0011 D4's record reaches the transcript through that type's own public
/// door, and **nothing in `tools/` changed**.
///
/// D4 requires an out-of-tree call to render differently and D6 requires a
/// destructive one to be annotated; both come from `TranscriptEntry::render`,
/// so the stored line is the line the prompt showed rather than a second
/// spelling of it.
///
/// The mutant: composing the stored line here instead of calling `render`.
#[test]
fn a_tool_call_reaches_the_transcript_as_the_line_adr_0011_d4_renders() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 4))
        .expect("could not start a session");
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working =
        crate::tools::WorkingDirectory::at(tree.project()).expect("the project directory resolves");

    let in_tree = super::fixtures::entry_for(&working, "src/main.rs", false);
    let out_of_tree = super::fixtures::entry_for(&working, "../elsewhere.rs", true);

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    transcript
        .record(&Record::ToolCall(crate::session::ToolCall::started(
            &out_of_tree,
        )))
        .expect("could not append");
    transcript
        .record(&Record::ToolCall(crate::session::ToolCall::completed(
            &out_of_tree,
        )))
        .expect("could not append");
    transcript
        .record(&Record::ToolCall(crate::session::ToolCall::started(
            &in_tree,
        )))
        .expect("could not append");

    let reading =
        Transcript::read(&session.transcript_path()).expect("the transcript did not read");
    let calls: Vec<&crate::session::ToolCall> = reading
        .records
        .iter()
        .filter_map(|record| match record {
            Record::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();

    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0].line,
        out_of_tree.render(),
        "the stored line is not the line ADR-0011 D4 renders",
    );
    assert!(
        calls[0].out_of_tree && calls[0].destructive,
        "D4's out-of-tree class and D6's annotation did not reach the transcript",
    );
    assert!(
        !calls[2].out_of_tree && !calls[2].destructive,
        "an ordinary in-tree call was marked, so the markings distinguish nothing",
    );
    assert_eq!(
        (calls[0].phase, calls[1].phase, calls[2].phase),
        (
            crate::session::Phase::Started,
            crate::session::Phase::Completed,
            crate::session::Phase::Started
        ),
        "a call is recorded as a started/completed pair, which is what makes an interruption \
         derivable at all",
    );
}

// ---------------------------------------------------------------------------
// D2 / trigger clause 2 — a killed process loses at most the event in flight
// ---------------------------------------------------------------------------

/// The child half of the kill check. **The parent below spawns it.**
///
/// It is this crate's own test binary re-invoked under an environment
/// variable and nothing else — no shell, no helper binary, no script. It
/// appends records for ever, reporting on standard output the sequence of
/// every record whose `record()` call **returned**, so the parent can compare
/// what the writer promised was durable against what is actually on disk.
///
/// Ignored, so the ordinary suite lists it rather than running it, and the
/// parent names it with `--exact ... --ignored`.
#[test]
#[ignore = "the child half of the kill check; a_killed_process_loses_at_most_the_event_in_flight spawns it"]
fn the_kill_checks_child_appends_until_it_is_killed() {
    let Ok(path) = std::env::var(super::fixtures::KILL_CHILD_TRANSCRIPT) else {
        panic!(
            "this check is the child half of the kill check and is spawned with {} set; running \
             it by hand asserts nothing",
            super::fixtures::KILL_CHILD_TRANSCRIPT
        );
    };

    use std::io::Write as _;

    let mut transcript =
        Transcript::append_to(path).expect("the child could not open a transcript");
    let mut out = std::io::stdout();

    // One newline before the first promise, and it is load-bearing. libtest
    // prints `test <name> ... ` with **no** terminating newline and completes
    // that line when the test ends -- which never happens here, because the
    // parent kills this process mid-loop. Without this line the first promise
    // is glued onto libtest's progress line, so the parent's line-prefix
    // filter cannot see it and the writer is held to every promise except the
    // first. Measured on 2026-09-05: 114 promises written, 113 free-standing,
    // and the missing one is always `DURABLE 0` -- which is exactly the record
    // a buffering bug loses most visibly.
    writeln!(out).expect("the child could not report");

    // Bounded rather than unbounded: the parent kills this long before the
    // ceiling, and a loop with an end is one clippy will let past. Reaching it
    // is a failure the child says out loud rather than a silent stop.
    for seq in 0u64..u64::MAX {
        transcript
            .record(&Record::Loop(super::fixtures::sequenced_event(
                seq,
                super::fixtures::KILL_LINE_PAYLOAD,
            )))
            .expect("the child could not append");
        // Only after `record` returned, which is after the write, the flush
        // and the sync. This line is the writer's promise that the record is
        // on disk, and the parent holds it to that promise.
        writeln!(out, "DURABLE {seq}").expect("the child could not report");
    }
}

/// How many records must be on disk before the parent kills the child.
///
/// **Derived from the staging assertion rather than chosen.** That assertion
/// needs two records on disk *and* two promises the child reported durable.
/// The child writes `record(n)` and only then `DURABLE n`, so a transcript
/// holding `k` complete records proves the child returned from `record(k - 1)`
/// and therefore already wrote `DURABLE 0` through `DURABLE k - 2` — `k - 1`
/// promises. Both halves hold from `k = 3`. The kill happens after the
/// observation rather than instead of it, so three is a floor and the rounds
/// in practice clear it.
const RECORDS_BEFORE_THE_KILL: usize = 3;

/// How long the parent waits for that condition before refusing by name.
///
/// Generous on purpose. Nothing here is a latency assertion, and a machine
/// under load is the case this check exists to survive rather than the case it
/// should fail on.
const KILL_CONDITION_DEADLINE: Duration = Duration::from_secs(30);

/// Wait until the child's transcript holds [`RECORDS_BEFORE_THE_KILL`]
/// records, or refuse naming what was being waited for.
///
/// [Verification lessons] §20 — **wait on the condition, never on a count.**
/// What this replaced was `sleep(20 + round * 5)` milliseconds followed by the
/// kill, which is a different experiment on a busy machine than on an idle
/// one. Measured on 2026-09-05 at `9b70ce5` with no change in the tree: the
/// check refused on one run in three under a parallel `cargo doc` and on none
/// of twelve runs on an idle machine, so what it reported was the load rather
/// than the harness.
///
/// The refusal says whether the count was still **moving** when the deadline
/// expired, because a plateau and a slow arrival are different findings and a
/// timeout that cannot tell them apart should not be quoted.
///
/// A read that fails while the child is mid-append is treated as *not yet*
/// rather than as an error: the authoritative read is the one after the kill,
/// which panics rather than tolerating anything. The child cannot stall on its
/// own pipe before this returns — it blocks on `stdout` only once roughly six
/// thousand promises are undrained, and by then the transcript is thousands of
/// records past the floor.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn wait_until_the_transcript_holds(path: &std::path::Path, wanted: usize, round: u64) {
    let started = std::time::Instant::now();
    let mut a_moment_ago = (0usize, std::time::Instant::now());

    loop {
        let seen = Transcript::read(path).map_or(0, |reading| reading.records.len());
        if seen >= wanted {
            return;
        }

        let waited = started.elapsed();
        assert!(
            waited < KILL_CONDITION_DEADLINE,
            "round {round}: the child never got {wanted} records onto disk. It reached {seen} in \
             {waited:?}, and over the last {:?} of that wait the count {}. This check kills a \
             live writer, so a child that never wrote cannot say anything about what a kill costs",
            a_moment_ago.1.elapsed(),
            if seen > a_moment_ago.0 {
                "was still moving"
            } else {
                "did not move"
            },
        );

        if a_moment_ago.1.elapsed() >= Duration::from_secs(1) {
            a_moment_ago = (seen, std::time::Instant::now());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// ADR-0010 trigger clause 2: "A killed process loses at most one event,
/// asserted by killing mid-session and reading the transcript."
///
/// Two properties, and they catch different mutations.
///
/// **No torn line.** Every byte before the last newline parses as a record,
/// and there is nothing after the last newline. This is what a `BufWriter`
/// breaks: it flushes on its own 8 KiB boundary, which falls inside whichever
/// line crosses it.
///
/// **Nothing the writer promised is missing.** The child reports the sequence
/// of every record whose `record()` call returned — after the write, the
/// flush and the sync — and every one of those must be on disk. This is what
/// buffering breaks far more violently than tearing does: a buffered child
/// reports thousands of durable records with a few dozen on the file.
///
/// The kill is `SIGKILL`, which is what `Child::kill` sends on Unix, so the
/// child gets no chance to flush anything.
///
/// **What this cannot see.** A `SIGKILL` cannot split a single unbuffered
/// `write_all` to a regular file, so this stays green with `sync_data`
/// removed. The sync is what survives the *machine* losing power, and no
/// check on this machine exercises it. Recorded rather than glossed.
#[test]
fn a_killed_process_loses_at_most_the_event_in_flight() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");

    let mut rounds_with_a_fragment = Vec::new();
    let mut rounds_missing_a_durable_record = Vec::new();
    let mut most_records_seen = 0usize;
    let mut most_durable_seen = 0u64;
    const ROUNDS: u64 = 12;

    for round in 0..ROUNDS {
        let session = store
            .start(id_at(1_700_000_000_000 + round, 11))
            .expect("could not start a session");
        let path = session.transcript_path();

        let mut child = std::process::Command::new(
            std::env::current_exe().expect("the test binary knows where it is"),
        )
        .args([
            "--exact",
            "session::tests::the_kill_checks_child_appends_until_it_is_killed",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(super::fixtures::KILL_CHILD_TRANSCRIPT, path.as_os_str())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("could not spawn this crate's own test binary");

        // Wait on the condition, not on a clock. See
        // `wait_until_the_transcript_holds`.
        wait_until_the_transcript_holds(&path, RECORDS_BEFORE_THE_KILL, round);
        child.kill().expect("could not kill the child");
        let mut reported = String::new();
        if let Some(mut out) = child.stdout.take() {
            use std::io::Read as _;
            let _ = out.read_to_string(&mut reported);
        }
        let _ = child.wait();

        let promised: Vec<u64> = reported
            .lines()
            .filter_map(|line| line.strip_prefix("DURABLE "))
            .filter_map(|seq| seq.parse().ok())
            .collect();

        let reading = Transcript::read(&path).unwrap_or_else(|failure| {
            panic!("round {round}: a killed writer left a transcript that will not read: {failure}")
        });
        let on_disk: Vec<u64> = reading
            .records
            .iter()
            .filter_map(super::fixtures::sequence_of)
            .collect();

        most_records_seen = most_records_seen.max(on_disk.len());
        most_durable_seen = most_durable_seen.max(u64::try_from(promised.len()).unwrap_or(0));

        if let Some(bytes) = reading.fragment {
            rounds_with_a_fragment.push((round, bytes));
        }
        for seq in &promised {
            if !on_disk.contains(seq) {
                rounds_missing_a_durable_record.push((round, *seq, on_disk.len(), promised.len()));
                break;
            }
        }
        assert_eq!(
            on_disk,
            (0..u64::try_from(on_disk.len()).expect("a small count")).collect::<Vec<_>>(),
            "round {round}: the transcript has a gap in it, so more than the event in flight was \
             lost",
        );
    }

    // The staging: without this, every assertion above is vacuously true over
    // a child that never wrote anything (Verification lessons §4).
    assert!(
        most_records_seen >= 2 && most_durable_seen >= 2,
        "no round got the child far enough to assert anything: the most records on disk in any \
         round was {most_records_seen} and the most the child reported durable was \
         {most_durable_seen}",
    );

    assert!(
        rounds_with_a_fragment.is_empty(),
        "{} of {ROUNDS} kills left a torn line — (round, trailing bytes): {:?}. ADR-0010 D2 says \
         a crash loses at most the event in flight, and a partial line is an event nobody can \
         read at all",
        rounds_with_a_fragment.len(),
        rounds_with_a_fragment,
    );
    assert!(
        rounds_missing_a_durable_record.is_empty(),
        "{} of {ROUNDS} kills lost a record the writer had already reported durable — (round, \
         sequence, records on disk, records the writer promised): {:?}. `record` returns only \
         after the write, the flush and the sync, so a caller that got past it is entitled to \
         find the line on disk",
        rounds_missing_a_durable_record.len(),
        rounds_missing_a_durable_record,
    );
}

// ---------------------------------------------------------------------------
// D3 — the checkpoint is rewritten, and the rewrite is atomic
// ---------------------------------------------------------------------------

/// D3's checkpoint is overwritten each turn and read back whole.
///
/// The mutant: appending instead of truncating, which leaves two documents
/// in one file.
#[test]
fn the_checkpoint_is_overwritten_each_turn_and_reads_back_whole() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 20))
        .expect("could not start a session");
    let checkpoint = Checkpoint::at(session.checkpoint_path());

    assert_eq!(
        checkpoint
            .read()
            .expect("an absent checkpoint is not an error"),
        None,
        "a session with no turns has no checkpoint, which is not a failure",
    );

    for turn in 1..=3u64 {
        let state = serde_json::json!({ "turn": turn, "messages": ["a", "b"] });
        checkpoint.write(&state).expect("could not rewrite");
        assert_eq!(
            checkpoint.read().expect("could not read back"),
            Some(state),
            "turn {turn} did not read back as what was written",
        );
    }

    assert!(
        !checkpoint.temporary_path().exists(),
        "the rewrite left its sibling behind, so a session directory accumulates a file the \
         user did not ask for and D5's \"read every byte with cat\" gets harder each turn",
    );
    assert_eq!(
        std::fs::metadata(session.checkpoint_path())
            .expect("the checkpoint is there")
            .permissions()
            .mode()
            & 0o777,
        crate::session::store::FILE_MODE,
        "the checkpoint carries the conversation and does not carry 0600",
    );
}

/// D3's rewrite is atomic: **no reader ever sees a partial document.**
///
/// A writer thread rewrites the checkpoint many times while a reader thread
/// reads it as fast as it can. Every read must be a whole document carrying
/// one of the versions the writer wrote. This is a stronger statement than a
/// kill test, because the window an in-place write opens is exactly the
/// window a kill would land in, and a thread can hit it thousands of times.
///
/// The mutant: `fs::write` in place, which truncates and then fills.
#[test]
fn no_reader_ever_sees_a_partly_rewritten_checkpoint() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 21))
        .expect("could not start a session");
    let path = session.checkpoint_path();
    let checkpoint = Checkpoint::at(&path);

    const REWRITES: u64 = 400;
    // Long enough that a truncate-then-fill has a window a reader can land in.
    let payload = "y".repeat(60_000);
    checkpoint
        .write(&serde_json::json!({ "turn": 0u64, "payload": payload }))
        .expect("could not stage the first checkpoint");

    let reading_path = path.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readers_flag = std::sync::Arc::clone(&done);
    let reader = std::thread::spawn(move || {
        let mut reads = 0u64;
        let mut torn = Vec::new();
        while !readers_flag.load(std::sync::atomic::Ordering::Relaxed) {
            match std::fs::read(&reading_path) {
                Ok(bytes) => {
                    reads += 1;
                    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
                        torn.push(bytes.len());
                        if torn.len() > 8 {
                            break;
                        }
                    }
                }
                Err(error) => torn.push(usize::MAX - error.raw_os_error().unwrap_or(0) as usize),
            }
        }
        (reads, torn)
    });

    for turn in 1..=REWRITES {
        checkpoint
            .write(&serde_json::json!({ "turn": turn, "payload": payload }))
            .expect("could not rewrite");
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let (reads, torn) = reader.join().expect("the reader thread panicked");

    // The staging: without reads, every assertion below is vacuous
    // (Verification lessons §4).
    assert!(
        reads > 10,
        "the reader only completed {reads} reads, so this check asserted nothing about the \
         {REWRITES} rewrites beside it",
    );
    assert!(
        torn.is_empty(),
        "{} of {reads} reads saw a checkpoint that was not a whole document (byte lengths, or \
         a raw OS error subtracted from usize::MAX): {torn:?}. ADR-0010 D3 overwrites the \
         checkpoint every turn, and a reader that can see between the truncate and the write is \
         a resume that can restore half a conversation",
        torn.len(),
    );
}

// ---------------------------------------------------------------------------
// D4 — resume restores, and never re-executes
// ---------------------------------------------------------------------------

/// D4: resume restores `context.json` and hands back the tail of the
/// transcript.
///
/// The mutant: returning the head of the transcript instead of the tail.
#[test]
fn resume_restores_the_checkpoint_and_hands_back_the_tail() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 22))
        .expect("could not start a session");

    let state = serde_json::json!({ "messages": ["the user said something", "and Zaru replied"] });
    Checkpoint::at(session.checkpoint_path())
        .write(&state)
        .expect("could not write the checkpoint");

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    for seq in 0..6u64 {
        transcript
            .record(&Record::Loop(super::fixtures::sequenced_event(seq, 4)))
            .expect("could not append");
    }

    let resumed =
        crate::session::resume(session.directory(), 2).expect("the session did not resume");

    assert_eq!(
        resumed.checkpoint,
        Some(state),
        "D3's checkpoint is what resume restores, and it did not come back",
    );
    assert_eq!(
        resumed
            .tail
            .iter()
            .filter_map(super::fixtures::sequence_of)
            .collect::<Vec<_>>(),
        vec![4, 5],
        "D4 re-renders the last stretch of the transcript, and this is not the last stretch",
    );
    assert_eq!(resumed.interrupted, None, "nothing was in flight");
    assert_eq!(resumed.fragment, None, "a clean transcript has no fragment");
}

/// D4: "An interrupted tool call is recorded as `Interrupted`."
///
/// A killed process writes nothing, so the marker is derived from a `Started`
/// with no `Completed`. The staging deliberately puts a *finished* call
/// before the unfinished one, so a derivation that returned the first started
/// call, or any started call, would be wrong.
///
/// The mutant: treating any `Started` as interrupted rather than only an
/// unmatched one.
#[test]
fn a_tool_call_with_no_result_is_the_interruption_and_nothing_is_re_executed() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let session = store
        .start(id_at(1_700_000_000_000, 23))
        .expect("could not start a session");
    let tree = crate::tools::fixtures::ScratchTree::new();
    let working =
        crate::tools::WorkingDirectory::at(tree.project()).expect("the project directory resolves");

    let finished = super::fixtures::entry_for(&working, "src/finished.rs", false);
    let in_flight = super::fixtures::entry_for(&working, "src/in-flight.rs", true);

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    for record in [
        Record::ToolCall(crate::session::ToolCall::started(&finished)),
        Record::ToolCall(crate::session::ToolCall::completed(&finished)),
        Record::ToolCall(crate::session::ToolCall::started(&in_flight)),
    ] {
        transcript.record(&record).expect("could not append");
    }

    // What the directory holds before the resume, so "nothing was executed"
    // is a comparison rather than an assumption.
    let before = std::fs::read(session.transcript_path()).expect("the transcript is there");
    let listing_before = super::fixtures::listing(session.directory());

    let resumed =
        crate::session::resume(session.directory(), 8).expect("the session did not resume");

    let interrupted = resumed
        .interrupted
        .as_ref()
        .expect("a call that started and never completed is the interruption");
    assert_eq!(
        interrupted.call.line,
        in_flight.render(),
        "the interruption named the wrong call, so a resume would report a completed action as \
         unfinished",
    );
    assert_eq!(interrupted.call.phase, crate::session::Phase::Started);

    // D4: "Resume never re-runs a tool call." `resume` takes a path and a
    // number and holds no port, so there is nothing it could invoke; what is
    // observable is that it changed nothing.
    assert_eq!(
        std::fs::read(session.transcript_path()).expect("the transcript is there"),
        before,
        "resuming appended to the transcript, so something acted",
    );
    assert_eq!(
        super::fixtures::listing(session.directory()),
        listing_before,
        "resuming changed what is in the session directory, so something acted",
    );

    // The arm that discriminates. Without it, a derivation that treated
    // **any** `Started` as the interruption gives the same answer above,
    // because the last started call happens to be the unfinished one --
    // measured, not predicted: that mutation stayed green until this arm
    // existed ([Verification lessons] §9 and §15). A session whose last call
    // completed has nothing in flight, and only the matching rule says so.
    transcript
        .record(&Record::ToolCall(crate::session::ToolCall::completed(
            &in_flight,
        )))
        .expect("could not append");
    let finished_run =
        crate::session::resume(session.directory(), 8).expect("the session did not resume");
    assert_eq!(
        finished_run.interrupted, None,
        "a session whose every call completed was reported as having one in flight, so a resume          would tell the model an action it finished did not complete",
    );
}

/// D2's "at most the event in flight" is surfaced by a resume rather than
/// silently dropped, and a **complete** line that does not parse is a
/// different thing that is reported as one.
///
/// The mutant: treating the trailing fragment as a record, which turns a
/// crash into a parse error the user cannot act on.
#[test]
fn a_resume_reports_a_trailing_fragment_and_refuses_a_malformed_complete_line() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");

    let torn = store
        .start(id_at(1_700_000_000_000, 24))
        .expect("could not start a session");
    let mut transcript =
        Transcript::append_to(torn.transcript_path()).expect("could not open the transcript");
    transcript
        .record(&Record::Loop(super::fixtures::sequenced_event(0, 4)))
        .expect("could not append");
    // The event that was in flight: a line with no newline after it.
    std::fs::OpenOptions::new()
        .append(true)
        .open(torn.transcript_path())
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"{\"loop\":{\"iter"))
        .expect("could not stage the fragment");

    let resumed = crate::session::resume(torn.directory(), 8).expect("the session did not resume");
    assert_eq!(
        resumed.fragment,
        Some(14),
        "the event that was in flight was not reported, so a crash looks like a clean stop",
    );
    assert_eq!(
        resumed
            .tail
            .iter()
            .filter_map(super::fixtures::sequence_of)
            .collect::<Vec<_>>(),
        vec![0],
        "the complete record before the fragment did not survive",
    );

    let broken = store
        .start(id_at(1_700_000_000_001, 25))
        .expect("could not start a session");
    std::fs::write(broken.transcript_path(), b"{\"loop\":{\"iter\n")
        .expect("could not stage the malformed line");
    let failure = crate::session::resume(broken.directory(), 8)
        .expect_err("a complete line that does not parse is not a fragment");
    assert!(
        failure.to_string().contains("line 1"),
        "the refusal does not say which line: {failure}",
    );
}

// ---------------------------------------------------------------------------
// D6 — retention is bounded, deletion is real
// ---------------------------------------------------------------------------

/// D6's window is the caller's and zero is refused, in the shape ADR-0007's
/// `Ttl` and ADR-0016's `RetryCeiling` already use.
///
/// The mutant: accepting zero, which deletes the session that is starting.
#[test]
fn a_retention_window_of_zero_is_refused_and_the_module_carries_no_default() {
    assert_eq!(
        RetentionWindow::new(Duration::ZERO),
        Err(crate::session::WindowRefused),
    );
    assert!(
        RetentionWindow::new(Duration::from_millis(1)).is_ok(),
        "one millisecond is a usable window, or this refusal is satisfied by refusing everything",
    );
    assert!(
        crate::session::WindowRefused
            .to_string()
            .contains("thirty days"),
        "the refusal does not say where D6's number is, so a reader cannot find out why the \
         module has no default",
    );
}

/// D6: "Sessions older than a configurable window are pruned on startup."
///
/// The age comes from the id rather than the filesystem, which is what makes
/// this staging possible at all: three sessions are created within the same
/// millisecond of real time and are minted at three days apart.
///
/// The boundary is staged three ways — one under the window, one exactly on
/// it, and one past it — because a comparison that is off by one is
/// indistinguishable from a correct one anywhere else in the range.
///
/// The mutant: reading the age off the filesystem's modification time, which
/// makes every session in this check the same age.
#[test]
fn pruning_removes_the_sessions_past_the_window_and_keeps_the_rest() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let day = 24 * 60 * 60 * 1000u64;
    let now = Millis::new(1_700_000_000_000 + 30 * day);
    let window =
        RetentionWindow::new(Duration::from_millis(10 * day)).expect("ten days is not zero");

    // Ages: 20 days, exactly 10 days, 9 days.
    let ancient = id_at(now.get() - 20 * day, 1);
    let exactly_on_the_window = id_at(now.get() - 10 * day, 2);
    let recent = id_at(now.get() - 9 * day, 3);
    for id in [&ancient, &exactly_on_the_window, &recent] {
        store.start(id.clone()).expect("could not start a session");
    }

    let pruned = crate::session::prune(&store, window, now, None).expect("pruning failed");

    assert_eq!(
        pruned.removed,
        vec![ancient.clone()],
        "the sessions removed are not the ones past a ten-day window",
    );
    assert_eq!(
        pruned.kept,
        vec![exactly_on_the_window.clone(), recent.clone()],
        "a session exactly on the window is not past it, and one inside it is not either",
    );
    assert_eq!(pruned.spared_as_current, None);

    assert_eq!(
        store.ids().expect("could not list sessions"),
        vec![exactly_on_the_window, recent],
        "the directory listing disagrees with what pruning said it did",
    );
}

/// D6: "**Deletion removes the directory rather than marking it deleted.**"
///
/// Asserted four ways, and the fourth is the one that discriminates: a
/// sibling control directory must **survive**, because a checker that reports
/// absence for everything passes on the target and fails on the control. That
/// is the credential store's own reading of what makes a deletion check mean
/// something.
///
/// The mutant: writing a `.deleted` marker beside the directory instead of
/// removing it, or emptying the directory rather than removing it.
#[test]
fn a_pruned_session_leaves_no_tombstone_and_a_sibling_survives() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let day = 24 * 60 * 60 * 1000u64;
    let now = Millis::new(1_700_000_000_000 + 30 * day);
    let window = RetentionWindow::new(Duration::from_millis(day)).expect("a day is not zero");

    let doomed = id_at(now.get() - 20 * day, 4);
    let survivor = id_at(now.get(), 5);
    let doomed_session = store.start(doomed.clone()).expect("could not start");
    store.start(survivor.clone()).expect("could not start");

    // A session with contents, so an implementation that removed an empty
    // directory and stopped would be visible.
    let mut transcript = Transcript::append_to(doomed_session.transcript_path())
        .expect("could not open the transcript");
    transcript
        .record(&Record::Loop(super::fixtures::sequenced_event(0, 32)))
        .expect("could not append");
    Checkpoint::at(doomed_session.checkpoint_path())
        .write(&serde_json::json!({ "turn": 1 }))
        .expect("could not write the checkpoint");

    let pruned = crate::session::prune(&store, window, now, None).expect("pruning failed");
    assert_eq!(pruned.removed, vec![doomed.clone()]);

    // Four readings, and the sibling is the one that discriminates.
    assert!(
        !doomed_session.directory().exists(),
        "the pruned session's directory is still there",
    );
    assert!(
        !doomed_session.transcript_path().exists(),
        "the pruned session's transcript is still there",
    );
    assert_eq!(
        super::fixtures::listing(store.sessions_directory().as_path()),
        vec![survivor.to_string()],
        "the sessions directory carries something other than the surviving session — a \
         tombstone is exactly what D6 forbids",
    );
    assert!(
        store.sessions_directory().join(survivor.as_str()).is_dir(),
        "the sibling that must survive did not, so a checker reporting absence for everything \
         would have passed the three assertions above",
    );
}

/// D6's pruning runs at startup, and the session that is starting is never
/// what it removes.
///
/// The staging makes the current session **older than the window**, so the
/// skip is the only thing keeping it: a check that staged it as young would
/// pass against an implementation with no skip at all.
///
/// The mutant: dropping the `current` guard.
#[test]
fn the_current_session_is_never_pruned_even_when_it_is_past_the_window() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let day = 24 * 60 * 60 * 1000u64;
    let now = Millis::new(1_700_000_000_000 + 30 * day);
    let window = RetentionWindow::new(Duration::from_millis(day)).expect("a day is not zero");

    let current = id_at(now.get() - 20 * day, 6);
    let other = id_at(now.get() - 20 * day, 7);
    for id in [&current, &other] {
        store.start(id.clone()).expect("could not start a session");
    }

    let pruned =
        crate::session::prune(&store, window, now, Some(&current)).expect("pruning failed");

    assert_eq!(
        pruned.removed,
        vec![other],
        "the current session was removed, so a startup prune can delete the transcript it is \
         about to append to",
    );
    assert_eq!(
        pruned.spared_as_current,
        Some(current.clone()),
        "the session was kept and nothing said why, so the skip is unobservable",
    );
    assert!(
        store.sessions_directory().join(current.as_str()).is_dir(),
        "the current session's directory is gone",
    );
}

/// A directory the harness did not write stops pruning rather than being
/// deleted or silently passed over.
///
/// The mutant: pruning whatever it cannot identify, which deletes a user's
/// own directory under `~/.zaru/sessions/`.
#[test]
fn pruning_refuses_rather_than_deleting_what_it_cannot_identify() {
    let scratch = ScratchRoot::new();
    let store = SessionStore::open(scratch.store_root()).expect("the store did not open");
    let day = 24 * 60 * 60 * 1000u64;
    let now = Millis::new(1_700_000_000_000 + 30 * day);
    let window = RetentionWindow::new(Duration::from_millis(day)).expect("a day is not zero");
    store
        .start(id_at(now.get() - 20 * day, 8))
        .expect("could not start a session");
    let intruder = store.sessions_directory().join("someones-own-notes");
    std::fs::create_dir(&intruder).expect("could not stage the intruder");

    let failure = crate::session::prune(&store, window, now, None)
        .expect_err("a directory that is not a session must stop pruning");
    assert!(
        failure.to_string().contains("not named by a ULID"),
        "the refusal does not say what stopped it: {failure}",
    );
    assert!(
        intruder.is_dir(),
        "pruning removed a directory the harness did not write",
    );
}

// ---------------------------------------------------------------------------
// D5 / trigger clause 6 — nothing leaves the machine
// ---------------------------------------------------------------------------

/// ADR-0010 trigger clause 6: "A test asserts no network call originates from
/// session storage at bare or contained tier."
///
/// **This module takes no tier at all**, so what is held is wider than the
/// clause: no network at any tier, as absence rather than as a branch.
///
/// # Why this is a check inside the crate and not the boundary gate
///
/// The obvious move is to add `zaru-cli` to `check-crate-boundaries.py`'s
/// `NO_NETWORK` set. It would pass today and break within hours: `zaru-cli`
/// depends on `zaru-notes`, and ADR-0006's client is landing `rmcp` there.
/// The gate would then fail on a true statement about `zaru-cli`, and the
/// only ways out are weakening the gate or exempting the crate — either of
/// which costs the `zaru-tui` arm its meaning. **The gate is not touched.**
///
/// # The matching is part of the rule, and so are its limits
///
/// Two arms, because a `use` is not the only way to name a type: a
/// fully-qualified `::std::net::TcpStream::connect` needs no import at all.
/// So the imports are enumerated *and* the source is searched for the names
/// themselves. [Agent lessons] §45: when a rule is enforced by matching
/// source text, the matching is part of the rule, and its limits belong
/// beside it. This check cannot see a socket reached through a re-export from
/// a sibling crate, which is what the import arm is for; `zaru_core` is
/// permitted because its own normal dependency closure is inside
/// `zaru-tui`'s, which `scripts/check-crate-boundaries.py` reports carries
/// none of the network-capable crates it names.
///
/// `tests.rs` and `fixtures.rs` are excluded and the exclusion is checked:
/// they are `#[cfg(test)]`, they are not session storage, and this check
/// names the forbidden strings, so scanning them would match itself.
///
/// The mutant: `use std::net::TcpStream;` in any product file of the module.
///
/// [Agent lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/agent-lessons
#[test]
fn no_network_call_can_originate_from_session_storage() {
    const PERMITTED_ROOTS: [&str; 7] = [
        "std",
        "core",
        "crate",
        "super",
        "serde",
        "serde_json",
        "zaru_core",
    ];
    const FORBIDDEN: [&str; 8] = [
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "ToSocketAddrs",
        "reqwest",
        "rmcp",
        "hyper",
    ];
    // `#[cfg(test)]` and therefore not session storage. The exclusion is
    // asserted below rather than trusted.
    const NOT_THE_PRODUCT: [&str; 2] = ["tests.rs", "fixtures.rs"];

    let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session");
    let declaration = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session.rs"),
    )
    .expect("the module declaration is there");
    for excluded in NOT_THE_PRODUCT {
        let stem = excluded.trim_end_matches(".rs");
        assert!(
            declaration.contains(&format!("#[cfg(test)]\nmod {stem};")),
            "{excluded} is excluded from this scan on the grounds that it is #[cfg(test)], and \
             the module declaration does not say so",
        );
    }

    let mut scanned = Vec::new();
    let mut imports = 0usize;
    let mut offences = Vec::new();

    for entry in std::fs::read_dir(&module).expect("the session module is there") {
        let path = entry.expect("an entry").path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs")
            || NOT_THE_PRODUCT.contains(&name.as_str())
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("a source file reads");
        scanned.push(name.clone());

        for line in source.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("use ") {
                imports += 1;
                let root = rest
                    .trim_start_matches("::")
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .next()
                    .unwrap_or_default();
                if !PERMITTED_ROOTS.contains(&root) {
                    offences.push(format!("{name}: imports `{root}`"));
                }
            }
            // A doc comment naming a type is prose, not a reach.
            if trimmed.starts_with("//") {
                continue;
            }
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    offences.push(format!("{name}: names `{needle}`"));
                }
            }
        }
    }

    // The instrument has to be able to find something before an absence is
    // evidence ([Verification lessons] §8).
    assert!(
        scanned.len() >= 6 && imports >= 10,
        "this scan read {} product file(s) and {imports} import(s), which is too few to have \
         asserted anything about the session module: {scanned:?}",
        scanned.len(),
    );
    assert!(
        offences.is_empty(),
        "session storage can reach something outside std, serde and this workspace's own \
         headless crate — ADR-0010 D5 says sessions are local files and nothing leaves the \
         machine: {offences:?}",
    );
}
