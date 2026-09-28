// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside this crate drives [ADR-0010]'s session lifecycle through
//! its own public door.
//!
//! # What this establishes, and what it does not
//!
//! [Verification lessons] §25: "For any capability a user interacts with, one
//! check drives the interaction end to end and reads the outcome... Mutation
//! testing cannot find this — it operates on the assertions that exist, and
//! there is no mutant for a call that was never written." The unit checks
//! reach the module's internals; this one reaches only what `zaru-cli`
//! exports, so a type or method that was never made public fails here and
//! nowhere else.
//!
//! **It is not evidence about the `zaru` binary.** No binary reaches the
//! session lifecycle. `zaru` takes no arguments, prints its composition and
//! exits 0, and creating `~/.zaru/sessions/<ulid>/` as a side effect of that
//! would write a directory into the user's home on every invocation for a
//! session that never had a turn — against ADR-0010 D6's own inode
//! consequence and against D5's claim to hold something worth reading. The
//! coherent place for the binary to create one is a command that has a
//! session to start, and ADR-0015's surface does not exist. A **delegated
//! coordinator ruling of 2026-09-04**: the binary creates no session,
//! `SessionEvidence::NoSessionExists` stays, and the day a session exists is
//! the day something reaches the loop. What this file prints is evidence
//! about the mechanism and must not be quoted as evidence about the binary.
//!
//! # The one place a captured value legitimately lands
//!
//! ADR-0010's own Negative section: "Plain-text transcripts on disk contain
//! whatever the session contained, including secrets that appeared in command
//! output. Filesystem permissions are the only protection, and that is worth
//! saying out loud rather than implying encryption that does not exist." So
//! this file asserts a planted value **is** in the transcript, and is **not**
//! in anything that is about a session rather than is its data. **No
//! redaction is designed anywhere**; ADR-0008's trigger clause 6 is open and
//! nothing here answers it.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zaru_cli::failure::{Guarded, SessionEvidence, guard};
use zaru_cli::session::{
    Checkpoint, Millis, Record, RetentionWindow, SessionId, SessionStore, SystemWallClock,
    Transcript, WallClock, prune, resume,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call produces, awkward on purpose.
///
/// Written here rather than imported: the crate's own fixtures are
/// `#[cfg(test)]` and an outside caller cannot see them, which is part of
/// what this file establishes. The tail is the credential store's — a
/// decomposed grapheme cluster, a precomposed one, and an astral-plane
/// character — so a formatter that escaped rather than redacted would produce
/// something the ASCII core still finds.
fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{label}-{}-{nanos}-{seq}-e\u{301}\u{e9}\u{1f701}",
        std::process::id()
    )
}

/// The part of a nonce no formatter can alter.
fn ascii_core(value: &str) -> &str {
    value
        .strip_suffix("-e\u{301}\u{e9}\u{1f701}")
        .unwrap_or(value)
}

/// A scratch tree this check owns, with a sibling control that must survive.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let base = std::env::temp_dir().join(nonce("sl-outside"));
        std::fs::create_dir_all(base.join("control")).expect("could not stage the scratch tree");
        Self { base }
    }

    fn home(&self) -> std::path::PathBuf {
        self.base.join("zaru")
    }

    fn control(&self) -> std::path::PathBuf {
        self.base.join("control")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The whole lifecycle, driven from outside: start, record, checkpoint,
/// resume, prune — and the seam ADR-0016 D3 left open.
#[test]
fn an_outside_caller_starts_a_session_records_resumes_and_prunes_it() {
    let scratch = Scratch::new();
    let store = SessionStore::open(scratch.home()).expect("the store did not open");

    let day = 24 * 60 * 60 * 1000u64;
    let now = Millis::new(1_700_000_000_000 + 60 * day);
    let live = SessionId::from_parts(now, [9, 8, 7, 6, 5, 4, 3, 2, 1, 0])
        .expect("a millisecond in 2023 fits in 48 bits");
    let ancient = SessionId::from_parts(
        Millis::new(now.get() - 45 * day),
        [1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    )
    .expect("so does one forty-five days earlier");

    let session = store
        .start(live.clone())
        .expect("could not start a session");
    store
        .start(ancient.clone())
        .expect("could not start a session");

    // A captured value, of the kind ADR-0010's Negative section is about.
    let captured = nonce("command-output");

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    transcript
        .record(&Record::Loop(
            zaru_core::iteration::Event::IterationStarted { n: 1, of: 3 },
        ))
        .expect("could not append");
    transcript
        .record(&Record::Loop(
            zaru_core::iteration::Event::ValidatorEvaluated {
                name: "cargo test".to_owned(),
                outcome: zaru_core::iteration::ValidatorOutcome::Failed,
                detail: captured.clone(),
            },
        ))
        .expect("could not append");

    Checkpoint::at(session.checkpoint_path())
        .write(&serde_json::json!({ "messages": ["hello"] }))
        .expect("could not write the checkpoint");

    // ---- what the directory actually holds, printed rather than described --
    println!("session directory: {}", session.directory().display());
    for entry in std::fs::read_dir(session.directory()).expect("the directory is there") {
        let path = entry.expect("an entry").path();
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        println!(
            "  {:o} {}",
            mode,
            path.file_name().expect("a name").to_string_lossy()
        );
    }
    println!(
        "transcript bytes:\n{}",
        std::fs::read_to_string(session.transcript_path()).expect("the transcript is there")
    );

    // ---- D1's three files: two written, one a port with no implementation --
    assert!(session.transcript_path().is_file());
    assert!(session.checkpoint_path().is_file());
    assert!(
        !session.meta_path().is_file(),
        "meta.toml was written, and ADR-0003 D2's table names no TOML crate",
    );

    // ---- D4: resume restores, and executes nothing --------------------------
    let resumed = resume(session.directory(), 1).expect("the session did not resume");
    assert_eq!(
        resumed.checkpoint,
        Some(serde_json::json!({ "messages": ["hello"] })),
    );
    assert_eq!(resumed.tail.len(), 1, "the tail is the last stretch");
    assert_eq!(resumed.interrupted, None);

    // ---- ADR-0016 D3's seam, produced and consumed --------------------------
    let caught = match guard("0.0.0", "https://example.invalid", || {
        // What opens a session tells the boundary as it does; this check
        // opened its session above, outside the boundary, so it tells here.
        session.entered();
        panic!("a deliberate panic, to reach D3's boundary");
    }) {
        Guarded::Defected(caught) => caught,
        Guarded::Ran(()) => panic!("the boundary did not catch the panic"),
    };
    let report = caught.report();
    assert!(
        matches!(report.session(), SessionEvidence::Session { .. }),
        "the defect boundary was not told which session it was inside",
    );
    assert_eq!(
        report.session().transcript(),
        Some(session.transcript_path().as_path()),
        "D3 says the message says the transcript is on disk, and it named a different file",
    );
    println!("defect report:\n{caught}");

    // ---- the value is in the transcript, and nowhere else -------------------
    let on_disk = std::fs::read_to_string(session.transcript_path()).expect("the transcript");
    assert!(
        on_disk.contains(&captured),
        "the captured value is not in the transcript; ADR-0010 D2 makes the transcript the \
         record of what happened and its Negative section says outright that it carries \
         whatever the session carried",
    );
    // A resumed session carries the tail of the transcript, so it carries
    // transcript content by construction — found by running this check rather
    // than by reasoning about it, on the first attempt to assert the opposite.
    // That is a **finding rather than a defect**: whatever renders a resumed
    // session is rendering the transcript, so ADR-0008's open trigger clause 6
    // reaches this value too, and it is recorded on ADR-0010 rather than
    // filtered here.
    // Asserted on the **ASCII core** rather than the raw value, and that is
    // the credential store's surviving mutation of 2026-09-04 arriving from
    // the other direction: `Debug` on a `String` escapes the nonce's
    // combining mark, so the raw value is genuinely absent from a rendering
    // that publishes every byte of it. An absence assertion there reads a
    // leak as absence; a presence assertion here reads a leak as a gap. Both
    // need the core (Verification lessons §50).
    assert!(
        format!("{resumed:?}").contains(ascii_core(&captured)),
        "the resumed session does not carry the transcript's own content, so D4's re-render \
         would show something other than what happened",
    );

    // What must not carry it is everything that is *about* a session rather
    // than *is* its data.
    for (what, rendered) in [
        ("the session's Debug", format!("{session:?}")),
        ("the store's Debug", format!("{store:?}")),
        ("the id's Debug", format!("{:?}", session.id())),
        ("the defect report", caught.to_string()),
        (
            "the defect report's Debug",
            format!("{:?}", caught.report()),
        ),
    ] {
        assert!(
            !rendered.contains(&captured) && !rendered.contains(ascii_core(&captured)),
            "{what} carries a value that belongs to the transcript alone: {rendered}",
        );
    }

    // ---- D6: pruning, and the sibling that must survive ---------------------
    let window =
        RetentionWindow::new(std::time::Duration::from_millis(30 * day)).expect("not zero");
    let pruned = prune(&store, window, now, Some(&live)).expect("pruning failed");
    println!("pruned: {pruned:?}");
    assert_eq!(pruned.removed, vec![ancient.clone()]);
    assert_eq!(pruned.kept, vec![live.clone()]);
    assert!(
        !store.sessions_directory().join(ancient.as_str()).exists(),
        "the pruned session's directory is still there",
    );
    assert!(
        store.sessions_directory().join(live.as_str()).is_dir(),
        "the sibling that must survive did not, so the assertion above would pass against a \
         checker that reports absence for everything",
    );
    assert!(scratch.control().is_dir(), "the control did not survive");
}

/// The one impure path, reached from outside: the machine's own clock and
/// `/dev/urandom` produce an id the module's own parser accepts.
#[test]
fn an_outside_caller_can_mint_an_id_from_the_machine() {
    let clock = SystemWallClock;
    assert!(
        clock.now().get() > 0,
        "the wall clock reads zero, so either this machine's clock is before 1970 or the port \
         is not reading it",
    );
    let minted = SessionId::mint(&clock).expect("the machine has a clock and /dev/urandom");
    assert!(SessionId::parse(minted.as_str()).is_ok());
    assert_ne!(
        minted,
        SessionId::mint(&clock).expect("the machine has a clock and /dev/urandom"),
    );
}

/// **The security corpus: what a session has already said, over a real
/// session directory, read from the transcript and never from the
/// checkpoint.**
///
/// ADR-0011 D2's notice and ADR-0002 D8's recommendation are each said once
/// and each rebuilt when a process opens, so what makes "once" span processes
/// is what a later process reads. This drives the whole chain an outside
/// caller can reach — a real directory, real records through `Transcript`,
/// `session::resume`, and then the two rules that decide the two lines — and
/// holds the three mutants that matter.
///
/// **The counter is ignored at open**: a session whose transcript says it said
/// both owes neither, with the accepting sibling below it.
///
/// **The two lines decided by one rule**: a session that said the notice and
/// not the recommendation, and its mirror.
///
/// **The decision read from the checkpoint instead of the transcript**, which
/// is the arm worth its cost because it would pass on today's data: both
/// sentences genuinely reach `context.json`, as part of a turn's answer. So
/// one session here has the sentences in its checkpoint and **no** `said`
/// record, and must be owed both.
#[test]
fn corpus_what_a_session_has_said_is_read_from_its_transcript_and_not_its_checkpoint() {
    use zaru_cli::manifest::MissingManifest;
    use zaru_cli::session::{Said, SaidOnce};
    use zaru_cli::tools::SessionNotice;

    let scratch = Scratch::new();
    let store = SessionStore::open(scratch.home()).expect("the store opened");

    // A statement nothing else in this process produces, so an assertion about
    // it is about this staging.
    let notice_text = nonce("notice");
    let recommendation_text = nonce("recommendation");

    /// The two decisions, taken exactly as the composition takes them.
    fn owed(said: &zaru_cli::session::AlreadySaid) -> (bool, bool) {
        (
            SessionNotice::in_session("the sentence", said).is_some(),
            MissingManifest::for_manifest_in_session(
                None,
                zaru_cli::failure::Statement::new("unavailable").expect("a statement"),
                zaru_cli::failure::Statement::new("how to get it").expect("a statement"),
                said,
            )
            .is_some(),
        )
    }

    // 1 — a session that said both. Neither is owed again.
    let both = store
        .start(SessionId::mint(&SystemWallClock).expect("an id"))
        .expect("a session started");
    let mut transcript = Transcript::append_to(both.transcript_path()).expect("the transcript");
    for (line, text) in [
        (SaidOnce::Notice, notice_text.clone()),
        (SaidOnce::Recommendation, recommendation_text.clone()),
    ] {
        transcript
            .record(&Record::Said(Said { line, text }))
            .expect("could not append");
    }
    let resumed = resume(both.directory(), usize::MAX).expect("it resumed");
    assert_eq!(
        owed(&resumed.said),
        (false, false),
        "this session's transcript says it said both lines, and both records say once",
    );

    // 2 — the accepting sibling: a session that has said nothing owes both, so
    // a rule that refused everything could not pass both halves.
    let neither = store
        .start(SessionId::mint(&SystemWallClock).expect("an id"))
        .expect("a session started");
    let resumed = resume(neither.directory(), usize::MAX).expect("it resumed");
    assert_eq!(
        owed(&resumed.said),
        (true, true),
        "a session that has said nothing is owed both lines",
    );

    // 3 — the arm that tells the two rules apart: one line said, not the other.
    let only_notice = store
        .start(SessionId::mint(&SystemWallClock).expect("an id"))
        .expect("a session started");
    Transcript::append_to(only_notice.transcript_path())
        .expect("the transcript")
        .record(&Record::Said(Said {
            line: SaidOnce::Notice,
            text: notice_text.clone(),
        }))
        .expect("could not append");
    let resumed = resume(only_notice.directory(), usize::MAX).expect("it resumed");
    assert_eq!(
        owed(&resumed.said),
        (false, true),
        "a session told it is not in a sandbox has not been told its project declares no \
         validators; one rule for two lines cannot answer this and its mirror below",
    );

    let only_recommendation = store
        .start(SessionId::mint(&SystemWallClock).expect("an id"))
        .expect("a session started");
    Transcript::append_to(only_recommendation.transcript_path())
        .expect("the transcript")
        .record(&Record::Said(Said {
            line: SaidOnce::Recommendation,
            text: recommendation_text.clone(),
        }))
        .expect("could not append");
    let resumed = resume(only_recommendation.directory(), usize::MAX).expect("it resumed");
    assert_eq!(owed(&resumed.said), (true, false), "and the mirror of it",);

    // 4 — the checkpoint arm. Both sentences are in `context.json`, as they
    // genuinely are on a real turn, and NO `said` record is on the transcript.
    // Both lines must still be owed.
    let checkpointed = store
        .start(SessionId::mint(&SystemWallClock).expect("an id"))
        .expect("a session started");
    Checkpoint::at(checkpointed.checkpoint_path())
        .write(&serde_json::json!({
            "exchanges": [{
                "task": "say hello",
                "tool_results": [],
                "answer": format!("{notice_text}\n\nhello\n\n{recommendation_text}"),
            }],
        }))
        .expect("the checkpoint was written");
    let resumed = resume(checkpointed.directory(), usize::MAX).expect("it resumed");
    // The staging is asserted rather than assumed: without the sentences in
    // the checkpoint this arm holds nothing.
    let stored = std::fs::read_to_string(checkpointed.checkpoint_path()).expect("it is readable");
    assert!(
        stored.contains(&notice_text) && stored.contains(&recommendation_text),
        "the checkpoint must carry both sentences for this arm to discriminate: {stored}",
    );
    assert_eq!(
        owed(&resumed.said),
        (true, true),
        "the counter is the transcript's; a decision read from the checkpoint would find both \
         sentences in layer 6 and silence two lines this session never showed",
    );
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
