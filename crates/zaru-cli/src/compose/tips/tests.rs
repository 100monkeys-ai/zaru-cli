// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0002] D8's standing tip: its budget, its suppression and its file.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output

use super::{Conditions, SUPPRESS_AFTER, Tip, Tips, eligible};
use std::path::PathBuf;

/// A `~/.zaru`-equivalent directory a check owns.
struct Root {
    path: PathBuf,
}

impl Root {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tips-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("staging: the root");
        Self { path }
    }

    fn tips(&self) -> Tips {
        Tips::under(&self.path)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The condition under which the one tip is still undiscovered.
///
/// The mouse is not captured here, so the selection tip's condition does not
/// hold and every check written for the one tip before 2026-09-27 still reads
/// that one tip.
const UNDISCOVERED: Conditions = Conditions {
    composer_token: false,
    mouse_captured: false,
};

/// [ADR-0002] D8: "a standing tip is suppressed after three displays without
/// action", and this record's trigger clause 10 requires the counter to
/// survive "a session restart".
///
/// # What discriminates
///
/// The boundary is asserted on **both** sides — two showings still offer it,
/// three do not — so a `>` written for a `>=` reddens, and so does a counter
/// that never rises. The count is read back through a **second** [`Tips`]
/// built from the path, which is what "survives a restart" means here: no
/// value is carried in memory between the recording and the reading.
///
/// The mutant: `SUPPRESS_AFTER` compared with `>` instead of `>=`, which
/// prints the third arm's sentence.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn a_standing_tip_is_suppressed_after_three_showings_and_the_count_survives_a_restart() {
    let root = Root::new("suppression");

    for shown in 0..SUPPRESS_AFTER {
        // Every read is through a freshly built `Tips`, so the count comes
        // off the file rather than out of the value that wrote it.
        let reading = root.tips();
        assert_eq!(
            reading.displays_of(Tip::NotesToken).expect("the tips read"),
            shown,
            "the file should hold {shown} showings before the next one"
        );
        assert_eq!(
            eligible(true, UNDISCOVERED, &reading).expect("the tips read"),
            Some(Tip::NotesToken),
            "after {shown} of {SUPPRESS_AFTER} showings the tip is still owed"
        );
        reading
            .record_a_showing(Tip::NotesToken, "2026-09-15")
            .expect("the showing records");
    }

    let after = root.tips();
    assert_eq!(
        after.displays_of(Tip::NotesToken).expect("the tips read"),
        SUPPRESS_AFTER
    );
    assert_eq!(
        eligible(true, UNDISCOVERED, &after).expect("the tips read"),
        None,
        "after {SUPPRESS_AFTER} showings the tip is suppressed, and the count came off the file"
    );
}

/// [ADR-0002] D8's budget: "At most **one recommendation of either kind per
/// session**."
///
/// # What discriminates, and it is the arm a first-come budget fails
///
/// A session that owes [ADR-0009] D4's missing-manifest line is offered no
/// tip. The accepting sibling is the same session with the line already
/// spent, which **is** offered one — without it a budget implemented as
/// "never offer a tip" would pass.
///
/// The mutant: dropping the recommendation term from
/// [`Owed::has_room_for_a_tip`](crate::compose::Owed::has_room_for_a_tip).
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[test]
fn a_session_that_owes_an_event_anchored_recommendation_is_offered_no_tip() {
    let root = Root::new("budget");

    assert_eq!(
        eligible(false, UNDISCOVERED, &root.tips()).expect("the tips read"),
        None,
        "the session's one recommendation is owed to ADR-0009 D4's line, so no tip is offered"
    );
    assert_eq!(
        root.tips()
            .displays_of(Tip::NotesToken)
            .expect("the tips read"),
        0,
        "a tip that was not offered was not counted either"
    );

    assert_eq!(
        eligible(true, UNDISCOVERED, &root.tips()).expect("the tips read"),
        Some(Tip::NotesToken),
        "with the budget free the same session is offered the tip"
    );
}

/// [ADR-0002] D8's "three displays **without action**", which needs no
/// observer.
///
/// # What discriminates
///
/// The count is zero, so nothing but the condition can be refusing. The
/// accepting sibling is the same file with the capability still undiscovered.
///
/// The mutant: [`Conditions::holds`] returning `true` unconditionally.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn a_tip_whose_capability_the_user_has_found_is_not_offered_however_low_its_count_is() {
    let root = Root::new("without-action");
    let tips = root.tips();
    assert_eq!(
        tips.displays_of(Tip::NotesToken).expect("the tips read"),
        0,
        "the staging is a tip that has never been shown"
    );

    let found = Conditions {
        composer_token: true,
        mouse_captured: false,
    };
    assert_eq!(
        eligible(true, found, &tips).expect("the tips read"),
        None,
        "a token holds the composer role, so the capability is discovered and the tip is done"
    );
    assert_eq!(
        eligible(true, UNDISCOVERED, &tips).expect("the tips read"),
        Some(Tip::NotesToken),
        "the sibling: with no composer token the same file offers the tip"
    );
}

/// [ADR-0010] D5: "The user can read every byte the harness stores about them
/// with `cat`", and this file stores a name, a count and a date.
///
/// # What discriminates
///
/// The tip's **own authored line** is asserted absent from the bytes, and its
/// stable name asserted present — so a `Shown` that carried the sentence
/// instead of the name reddens, and a file that was simply empty could not
/// satisfy the second arm. This is what makes the file's security property
/// structural: there is no field a task, an answer or a credential could
/// reach.
///
/// The mutant: `Shown::tip` built from [`Tip::line`] instead of
/// [`Tip::name`].
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn the_tips_file_carries_a_name_a_count_and_a_date_and_never_a_tips_own_line() {
    let root = Root::new("bytes");
    let tips = root.tips();
    for tip in Tip::ALL {
        tips.record_a_showing(tip, "2026-09-15")
            .expect("the showing records");
    }

    // Read with `std::fs` rather than through `Tips::entries`, so neither arm
    // travels through the writer under test.
    let raw = std::fs::read_to_string(tips.path()).expect("the tips file reads");
    for tip in Tip::ALL {
        assert!(
            raw.contains(tip.name()),
            "the sibling: {} is what the file records, and it is not there: {raw}",
            tip.name()
        );
        assert!(
            !raw.contains(tip.line()),
            "{}'s authored line reached the file, which stores a count and not prose: {raw}",
            tip.name()
        );
    }
    assert!(
        raw.contains("2026-09-15"),
        "the date the showing was recorded is not there: {raw}"
    );
}

/// A trailing fragment is the line that was in flight, exactly as it is on
/// [ADR-0010] D2's transcript, and it is never counted.
///
/// # What discriminates
///
/// A complete line **and** a fragment are planted, and the count is the
/// complete one's. A reader that split on newlines without dropping the tail
/// would either count the fragment or fail to parse it.
///
/// The mutant: `entries` reading `raw.lines()` instead of the span before the
/// last newline.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn a_half_written_tips_line_is_the_showing_in_flight_and_is_never_counted() {
    let root = Root::new("fragment");
    let tips = root.tips();
    tips.record_a_showing(Tip::NotesToken, "2026-09-15")
        .expect("the showing records");
    let mut raw = std::fs::read_to_string(tips.path()).expect("the tips file reads");
    raw.push_str("{\"tip\":\"notes-token\",\"displays\":9");
    std::fs::write(tips.path(), &raw).expect("the fragment plants");

    assert_eq!(
        tips.displays_of(Tip::NotesToken).expect("the tips read"),
        1,
        "the fragment was counted, and a power cut is not a showing: {raw}"
    );
}

/// Every tip has a name, a line and a condition, and no two share a name.
///
/// # What discriminates
///
/// It walks [`Tip::ALL`] rather than listing the tips, so a second tip is
/// covered the moment it is declared. The width arm is D8's "one line either
/// way" against the forty-column frame the strip's other lines are written
/// for.
#[test]
fn every_tip_has_a_name_a_line_that_fits_a_narrow_frame_and_a_condition() {
    let mut names: Vec<&str> = Vec::new();
    for tip in Tip::ALL {
        assert!(!tip.name().is_empty(), "a tip with no name");
        assert!(!tip.line().is_empty(), "{} has no line", tip.name());
        assert!(
            !tip.line().contains('\n'),
            "{}'s line is not one line",
            tip.name()
        );
        let columns = tip.line().chars().count();
        assert!(
            columns <= 40,
            "{}'s line is {columns} columns and the strip clips at forty: {}",
            tip.name(),
            tip.line()
        );
        assert!(
            tip.name()
                .chars()
                .all(|glyph| glyph.is_ascii_lowercase() || glyph == '-'),
            "{} is not a stable identifier",
            tip.name()
        );
        assert!(
            !names.contains(&tip.name()),
            "two tips share the name {}",
            tip.name()
        );
        names.push(tip.name());
        // Every tip's condition is written, which is what stops a second tip
        // arriving with a `holds` arm nobody filled in: the match is
        // exhaustive, so this only has to be reachable.
        let _ = UNDISCOVERED.holds(tip);
    }
    assert_eq!(names.len(), Tip::ALL.len());
}

/// One place in the product records a showing, and that is what makes "a
/// display is one session's showing" true rather than nearly true.
///
/// # What discriminates, and the liveness arm
///
/// The walk reads the crate's own product source off disk — never a test
/// tree — and counts the call. A second call site anywhere, in a beat or a
/// repaint, would spend the budget at a rate D8's own reasoning argues
/// against. The liveness arm fails when the walk read too few files to have
/// asserted anything, which is the shape
/// `no_network_call_can_originate_from_session_storage` already uses.
///
/// The mutant: a second `record_a_showing` call anywhere under `src/`,
/// whatever its argument is spelled as. The first form of this check counted
/// `.record(tip` and a mutant that wrote the variant's full path passed it,
/// which is why the method carries a name of its own.
#[test]
fn only_one_place_in_the_product_records_a_tip_showing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut read = 0_usize;
    let mut sites: Vec<String> = Vec::new();
    let mut walk = vec![root.clone()];
    while let Some(at) = walk.pop() {
        let entries = std::fs::read_dir(&at).expect("the source directory reads");
        for entry in entries {
            let path = entry.expect("the directory entry reads").path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            // A test module is not the product, and this file is one of them.
            if path.file_name().is_some_and(|name| name == "tests.rs")
                || path.file_name().is_some_and(|name| name == "fixtures.rs")
            {
                continue;
            }
            read += 1;
            let source = std::fs::read_to_string(&path).expect("the source file reads");
            for (at, line) in source.lines().enumerate() {
                // The call, not a mention of it: a documentation line is
                // prefixed by `///` or `//!` and a comment by `//`.
                if line.contains(".record_a_showing(") && !line.trim_start().starts_with("//") {
                    sites.push(format!("{}:{}", path.display(), at + 1));
                }
            }
        }
    }

    assert!(
        read > 100,
        "this walk read {read} product file(s), which is too few to have asserted anything"
    );
    assert_eq!(
        sites.len(),
        1,
        "a showing is recorded in {} place(s) rather than one: {sites:?}",
        sites.len()
    );
    assert!(
        sites[0].contains("terminal/open.rs"),
        "the showing is recorded somewhere other than where a session opens: {}",
        sites[0]
    );
}

/// [ADR-0002] D8's budget is a session's, and a session that can run no turn
/// still has one.
///
/// # The mutant and the accepting sibling
///
/// `owed.map_or(false, …)`, which is the guard `terminal::open` carried until
/// 2026-09-15 written as a function: the tip is withheld from exactly the
/// session whose person has discovered nothing. Watched red.
///
/// The sibling is the third arm: a session that **has** an `Owed` takes its
/// answer from that value and not from the switch beside it, so the first arm
/// is a decision about the absence of an `Owed` rather than a blanket yes.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn a_session_that_resolved_no_provider_still_has_room_for_a_tip() {
    assert!(
        super::room_for_a_tip(None, true),
        "a session with no provider is offered no tip, which is the session the tip is for"
    );
    assert!(
        !super::room_for_a_tip(None, false),
        "`tips = false` did not reach a session with no provider, and D8 says it disables both"
    );
    // A default `Owed` owes nothing and carries `tips: false`, so it answers
    // `false` whatever the switch beside it says.
    assert!(
        !super::room_for_a_tip(Some(&crate::compose::Owed::default()), true),
        "the switch overrode an `Owed` that said no, so the first arm would pass on a harness \
         that always answered yes"
    );
}

/// While the harness holds the mouse, the tip that says how to select text is
/// offered, and it is offered **before** the notes tip.
///
/// # Why first
///
/// D8's budget is one tip per session, so two eligible tips need an order,
/// and this function's own documentation declined to invent one while there
/// was one tip. The rule taken on 2026-09-27, open to Jeshua's veto: **a tip
/// that says how to get back something the harness took outranks a tip about
/// a capability it adds.** Capturing the mouse took the terminal's plain
/// click-and-drag selection, and a person who reaches for it and finds it gone
/// is met with a regression. A person who has not found note search has not
/// lost anything.
///
/// # What discriminates
///
/// Four stagings. Capture on and no composer token offers the selection tip;
/// capture off offers the notes tip (the sibling, so a selection tip offered
/// unconditionally reddens); capture on with a token offers the selection tip
/// alone; and the selection tip shown three times hands the budget back to
/// the notes tip, so the order is a priority and not an exclusion.
///
/// The mutant: `Tip::ALL` in the other order, which prints the first arm.
#[test]
fn the_selection_tip_is_offered_while_the_mouse_is_held_and_ahead_of_the_notes_tip() {
    let root = Root::new("selection");
    let tips = root.tips();
    let held = Conditions {
        composer_token: false,
        mouse_captured: true,
    };
    assert_eq!(
        eligible(true, held, &tips).expect("the tips read"),
        Some(Tip::Selection),
        "with the mouse held and no composer token, the session was offered something other \
         than how to select text"
    );
    assert_eq!(
        eligible(true, UNDISCOVERED, &tips).expect("the tips read"),
        Some(Tip::NotesToken),
        "the sibling: with the mouse not held there is nothing to say about selecting"
    );
    let held_and_found = Conditions {
        composer_token: true,
        mouse_captured: true,
    };
    assert_eq!(
        eligible(true, held_and_found, &tips).expect("the tips read"),
        Some(Tip::Selection),
        "a composer token discovers note search, not selection"
    );
    for _ in 0..SUPPRESS_AFTER {
        tips.record_a_showing(Tip::Selection, "2026-09-27")
            .expect("the showing records");
    }
    assert_eq!(
        eligible(true, held, &tips).expect("the tips read"),
        Some(Tip::NotesToken),
        "three showings of the selection tip should hand the session's one tip to the next"
    );
}
