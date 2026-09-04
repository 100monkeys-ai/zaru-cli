// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks on ADR-0016's taxonomy. Compiled only under `cfg(test)`.
//!
//! Every check here names the mutation it exists to catch, because a check
//! whose mutant nobody can construct is a check nobody has shown to
//! discriminate — [Verification lessons] §12.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use super::fixtures::{a_session, defect_report, of_class, one_of_each_class, policy, statement};
use crate::credentials::fixtures::{ascii_core, nonce};
use crate::failure::class::{Class, Exit, SUCCESS};
use crate::failure::classified::{Classified, Expected};
use crate::failure::defect::{SessionEvidence, SessionId, SessionIdRefused};
use crate::failure::guard::{Guarded, guard};
use crate::failure::partial::{Partial, PartialRefused, StepName};
use crate::failure::present::Presentation;
use crate::failure::remedy::{Action, Remedy, Statement, StatementRefused};
use crate::failure::wait::{Backoff, RETRY_LABEL, RetryCeiling, RetryRecord, Wait, WaitRefused};
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;
use std::sync::{Mutex, PoisonError};

/// ADR-0016 D1 names five classes. There is no sixth and there are not four.
///
/// The names come from the enum rather than from a list retyped beside the
/// check, so a sixth variant makes `Class::ALL`'s annotated length fail to
/// compile; what this check adds is that the five are D1's five and are
/// spelled as D1 spells them.
///
/// The mutant: renaming a class, or dropping one from `ALL`.
#[test]
fn the_five_classes_are_the_ones_adr_0016_d1_names() {
    let named: Vec<&str> = Class::ALL.into_iter().map(Class::as_str).collect();
    assert_eq!(
        named,
        vec![
            "expected",
            "user-correctable",
            "environmental",
            "capability",
            "defect",
        ],
        "ADR-0016 D1's table names five classes in this order; this crate believes they are \
         {named:?}"
    );
}

/// ADR-0016 D5's table, asserted against the record's own numbers.
///
/// The expected values are literals this check owns, taken from D5, and not
/// read back out of the mapping — [Verification lessons] §10: a check that
/// asks the code under test for its expected value is a mirror with a verdict
/// attached.
///
/// The mutant: any code changed, and swapping two of them.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn each_class_exits_with_the_code_adr_0016_d5_gives_it() {
    let mut wrong = Vec::new();
    for (class, expected) in [
        (Class::Expected, 1u8),
        (Class::UserCorrectable, 2),
        (Class::Environmental, 3),
        (Class::Capability, 4),
        (Class::Defect, 70),
    ] {
        let got = class.exit_code();
        if got != expected {
            wrong.push(format!("{class} exits {got}, and D5 says {expected}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "ADR-0016 D5's exit codes are 1, 2, 3, 4 and 70. {} of the five disagree: {wrong:?}",
        wrong.len()
    );

    assert_eq!(
        SUCCESS, 0,
        "D5's table opens with `0   success`, and this crate believes it is {SUCCESS}"
    );
    assert_eq!(
        Exit::Succeeded.code(),
        0,
        "a run that did what was asked exits 0"
    );

    // The arm that discriminates: a failing ending carries its class's code
    // rather than a constant, so a mapping that always answered 70 fails here.
    for (class, classified) in one_of_each_class() {
        assert_eq!(
            Exit::Failed(classified).code(),
            class.exit_code(),
            "a failed ending of class {class} did not exit with that class's code"
        );
    }
}

/// ADR-0016 D1: "**Expected failures never render as errors.**"
///
/// Trigger clause 2's half that this crate holds. The register is *derived*
/// from the class rather than being a field, so the property is structural;
/// what this check adds is that the derivation is the right way round.
///
/// The mutant: inverting `is_the_error_register`, or moving `Expected` to the
/// other arm.
#[test]
fn an_expected_failure_is_not_in_the_error_register_and_the_other_four_are() {
    assert!(
        !Class::Expected.is_the_error_register(),
        "ADR-0016 D1: an iteration failure is the mechanism operating, and \"colouring it like a \
         crash teaches users to fear the thing that makes the product work\""
    );

    // The arm that discriminates: a predicate answering `false` for everything
    // satisfies the assertion above and is wrong.
    let not_errors: Vec<Class> = Class::ALL
        .into_iter()
        .filter(|class| !class.is_the_error_register())
        .collect();
    assert_eq!(
        not_errors,
        vec![Class::Expected],
        "exactly one of D1's five classes is not an error; this crate believes it is \
         {not_errors:?}"
    );

    // And it holds through the projection a renderer is actually handed.
    for (class, classified) in one_of_each_class() {
        assert_eq!(
            Presentation::of(&classified).is_the_error_register(),
            class.is_the_error_register(),
            "the projection of a {class} failure disagrees with its class about the register"
        );
    }
}

/// Trigger clause 3, over every class rather than over the one it names.
///
/// D2: "Every error names the remedy or admits there is not one." The
/// enumeration is over `Class::ALL`, so a sixth class arrives here as a
/// missing fixture rather than as silent coverage — and every arm asserts the
/// obligation D1 and D2 put on *that* class, not a generic one.
///
/// **The clause's own half is not asserted here because it cannot be.** A
/// `Classified::UserCorrectable` cannot be constructed without a `Remedy`, and
/// a `Remedy` cannot be empty, so "every user-correctable error carries a
/// remedy" is a compile error to violate rather than a check to fail. The
/// red-watch for it is the mutation that removes the field from the variant,
/// which stops this crate compiling.
///
/// The mutant: dropping the remedy lines from a user-correctable
/// presentation, the tier line from a capability one, or the
/// nothing-to-configure line from a defect.
#[test]
fn every_class_says_its_half_of_adr_0016_d2() {
    let mut silent = Vec::new();

    for (class, classified) in one_of_each_class() {
        let shown = Presentation::of(&classified);
        let rendered = shown.to_string();
        match class {
            // D1 row 1: nothing is wrong, so there is nothing to remedy — and
            // the type has no field one could go in.
            Class::Expected => {
                if classified.remedy().is_some() || !shown.lines.is_empty() {
                    silent.push("an expected failure offered a remedy".to_owned());
                }
            }
            // D2: "Says exactly what to change."
            // The remedy being non-empty is not asserted, because it is not
            // assertable: `Remedy` has no empty form. What is asserted is
            // that it reached the presentation whole.
            Class::UserCorrectable => match classified.remedy() {
                Some(remedy) if shown.lines.len() == remedy.len() => {}
                other => silent.push(format!(
                    "a user-correctable failure's remedy did not reach its presentation: \
                     {other:?} against {} line(s)",
                    shown.lines.len()
                )),
            },
            // D2: "Says whether to wait and how long."
            Class::Environmental => {
                if !rendered.contains(RETRY_LABEL) {
                    silent.push(format!(
                        "an environmental failure said nothing about waiting: {rendered:?}"
                    ));
                }
            }
            // D1 row 4: "Says which tier does."
            Class::Capability => {
                if !rendered.contains("contained") {
                    silent.push(format!(
                        "a capability failure did not name the tier that offers it: {rendered:?}"
                    ));
                }
            }
            // D2's other half, and D3's "Never present a defect as a user
            // error."
            Class::Defect => {
                if !rendered.contains("not something you can configure") {
                    silent.push(format!(
                        "a defect did not admit there is no remedy: {rendered:?}"
                    ));
                }
            }
        }
    }

    assert!(
        silent.is_empty(),
        "ADR-0016 D2: every error names the remedy or admits there is not one. {} of the five \
         classes said neither: {silent:?}",
        silent.len()
    );
}

/// Trigger clause 1's half: five classes, five distinguishable presentations.
///
/// The mutant: a projection that returns the same lines whatever the class,
/// or that drops the class from what it carries.
#[test]
fn one_failure_of_each_class_projects_to_a_distinguishable_presentation() {
    let shown: Vec<Presentation> = one_of_each_class()
        .into_iter()
        .map(|(_, classified)| Presentation::of(&classified))
        .collect();

    let classes: Vec<Class> = shown
        .iter()
        .map(|presentation| presentation.class)
        .collect();
    assert_eq!(
        classes,
        Class::ALL.to_vec(),
        "each projection must carry the class it was built from"
    );

    for (left, right) in shown.iter().zip(shown.iter().skip(1)) {
        assert_ne!(
            left.to_string(),
            right.to_string(),
            "a {} failure and a {} failure render identically, so the class cannot be \
             determining the presentation",
            left.class,
            right.class
        );
    }
}

/// ADR-0016 D4: "labelled `retry`, never `iteration`."
///
/// Both halves. The label appears where a reader can see it, and the word
/// ADR-0008 D1 and [Ubiquitous Language] reserve for the other thing appears
/// in nothing this module renders — over every class, so a stray use in a
/// neighbouring presentation is caught too.
///
/// The mutant: spelling the label `iteration`, or dropping the count or the
/// ceiling from the line.
///
/// [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language
#[test]
fn a_retry_is_labelled_a_retry_counted_bounded_and_never_called_an_iteration() {
    let record = RetryRecord::new(policy(), 2).expect("2 is within a ceiling of 5");
    let shown = Presentation::of(&Classified::Environmental {
        statement: statement("rate-limited"),
        wait: Wait::Retrying(record),
    });
    let rendered = shown.to_string();

    assert_eq!(RETRY_LABEL, "retry", "ADR-0016 D4 names the label");
    assert!(
        rendered.contains(RETRY_LABEL),
        "D4 requires the repeat be labelled a retry: {rendered:?}"
    );
    // Counted, and bounded, both visible: 2 made against a ceiling of 5, with
    // 3 remaining. Three distinct numbers, so a line printing one where
    // another belongs is visible.
    for expected in ["2 of 5", "3 remaining"] {
        assert!(
            rendered.contains(expected),
            "D4 says the retry is counted and bounded; {expected:?} is not in {rendered:?}"
        );
    }

    for (class, classified) in one_of_each_class() {
        let rendered = Presentation::of(&classified).to_string();
        assert!(
            !rendered.contains("iteration"),
            "ADR-0016 D4 forbids labelling a retry an iteration, and a {class} presentation \
             used the word: {rendered:?}"
        );
    }
}

/// D4's numbers are the caller's, and a useless one is refused at the
/// boundary.
///
/// The mutant: removing a guard, after which a ceiling of zero reports a
/// policy that permits nothing as one that permits retrying.
#[test]
fn a_retry_policy_that_bounds_nothing_is_refused_at_the_boundary() {
    assert_eq!(
        RetryCeiling::new(0),
        Err(WaitRefused::CeilingIsZero),
        "ADR-0016 D4 requires retries be bounded, and a ceiling of zero bounds nothing"
    );
    assert!(RetryCeiling::new(1).is_ok(), "one retry is a usable bound");

    assert_eq!(
        Backoff::new(Duration::ZERO),
        Err(WaitRefused::BackoffIsZero),
        "ADR-0016 D4 has transient failures retry with backoff, and zero is not one"
    );
    assert!(
        Backoff::new(Duration::from_nanos(1)).is_ok(),
        "any wait at all is a usable backoff"
    );

    // Counted and bounded is one clause: a count past the bound is a bound
    // that was not obeyed.
    assert_eq!(
        RetryRecord::new(policy(), 6),
        Err(WaitRefused::MoreRetriesThanTheCeiling {
            made: 6,
            ceiling: 5
        }),
        "ADR-0016 D4 says counted and bounded in one clause, so 6 retries against a ceiling of \
         5 is a bound that was not obeyed"
    );
    let at_the_ceiling = RetryRecord::new(policy(), 5).expect("5 is the ceiling, not past it");
    assert_eq!(
        at_the_ceiling.remaining(),
        0,
        "a run that used its whole budget has none remaining"
    );
}

/// D2's other half for an environmental failure: where waiting is not the
/// answer, say so.
///
/// The mutant: collapsing `Wait` to the retrying arm, which leaves a reader
/// waiting for a recovery that is not coming.
#[test]
fn an_environmental_failure_that_waiting_cannot_fix_says_so() {
    let why = statement("the endpoint is not there");
    let shown = Presentation::of(&Classified::Environmental {
        statement: statement("unreachable"),
        wait: Wait::NoWaitWillHelp(why.clone()),
    });
    let rendered = shown.to_string();

    assert!(
        rendered.contains("waiting will not help"),
        "D2: where there genuinely is no action, say that. {rendered:?}"
    );
    assert!(
        rendered.contains(why.as_str()),
        "the reason the raising site gave did not reach the presentation: {rendered:?}"
    );

    // The arm that discriminates: the other variant does offer waiting, so a
    // presentation that always printed this line would fail here.
    let record = RetryRecord::new(policy(), 1).expect("1 is within a ceiling of 5");
    assert!(Wait::Retrying(record).is_worth_waiting());
    assert!(!Wait::NoWaitWillHelp(why).is_worth_waiting());
}

/// A remedy always has at least one action, and there is no way to build one
/// without.
///
/// The mutant: a constructor taking a collection, which would make an empty
/// remedy expressible and trigger clause 3 a thing to remember.
#[test]
fn a_remedy_cannot_be_empty_and_carries_every_action_in_order() {
    let one = Remedy::one(Action::described(statement("do the thing")));
    assert_eq!(one.len(), 1);
    assert!(!one.is_empty());

    let first = nonce("first");
    let second = nonce("second");
    let two = Remedy::one(
        Action::runnable(statement("set one"), first.clone()).expect("a nonce is renderable"),
    )
    .also(Action::runnable(statement("or"), second.clone()).expect("a nonce is renderable"));

    let commands: Vec<Option<&str>> = two.actions().map(Action::command).collect();
    assert_eq!(
        commands,
        vec![Some(first.as_str()), Some(second.as_str())],
        "D2's worked example has two lines and they are shown in the order they were given"
    );
    assert_eq!(two.len(), 2);
}

/// A statement that a terminal would mangle is refused where it enters.
///
/// The mutant: dropping either guard, after which a defect report can carry a
/// line that erases the row above it.
#[test]
fn a_statement_that_cannot_be_rendered_is_refused_at_the_boundary() {
    assert_eq!(Statement::new(""), Err(StatementRefused::Empty));
    assert_eq!(Statement::new("   "), Err(StatementRefused::Empty));

    for offered in ["a\nb", "a\tb", "a\u{7}b", "\u{1b}[2Kerased"] {
        let refused = match Statement::new(offered) {
            Err(refused) => refused,
            Ok(taken) => panic!(
                "{offered:?} carries a control character and must be refused; it was taken as \
                 {taken:?}"
            ),
        };
        assert!(
            matches!(refused, StatementRefused::Control { .. }),
            "{offered:?} was refused for the wrong reason: {refused:?}"
        );
    }

    // The arm that discriminates: an awkward but renderable statement is
    // taken, so the refusals above are not "refuse everything".
    let awkward = nonce("statement");
    assert_eq!(
        Statement::new(awkward.clone()).map(|s| s.as_str().to_owned()),
        Ok(awkward)
    );
}

/// ADR-0016 D6: three of five, named on both sides.
///
/// The two lists have **different lengths on purpose**: with equal lengths, a
/// report that printed one list where the other belongs would still count
/// correctly.
///
/// The mutant: reporting the count of the wrong list, dropping the names, or
/// carrying a total separately from the lists.
#[test]
fn a_partial_report_names_what_completed_and_what_did_not() {
    let done: Vec<StepName> = ["one", "two", "three"]
        .into_iter()
        .map(|label| StepName::new(nonce(label)).expect("a nonce is renderable"))
        .collect();
    let outstanding: Vec<StepName> = ["four", "five"]
        .into_iter()
        .map(|label| StepName::new(nonce(label)).expect("a nonce is renderable"))
        .collect();

    let partial = Partial::new(done.clone(), outstanding.clone()).expect("three of five");
    assert_eq!(
        partial.of(),
        5,
        "D6's \"of five\" is the sum of the two lists"
    );
    assert_eq!(partial.completed().len(), 3);
    assert_eq!(partial.not_completed().len(), 2);

    let rendered = partial.to_string();
    assert!(
        rendered.contains("3 of 5"),
        "D6: work that completed three of five steps reports three of five. {rendered:?}"
    );
    for step in done.iter().chain(outstanding.iter()) {
        assert!(
            rendered.contains(step.as_str()),
            "D6 names what completed and what did not; {step} is in neither: {rendered:?}"
        );
    }
}

/// Neither end of the range is partial, and both are refused.
///
/// The mutant: dropping either guard, after which a total failure reports an
/// accomplishment there is no record of, or a total success sends a reader
/// looking for work that is not there.
#[test]
fn a_report_that_completed_everything_or_nothing_is_not_a_partial_one() {
    let step = |label: &str| StepName::new(nonce(label)).expect("a nonce is renderable");

    assert_eq!(
        Partial::new(Vec::new(), vec![step("outstanding")]),
        Err(PartialRefused::NothingCompleted),
        "ADR-0016 D6 is about work that completed some of its steps; a run that completed none \
         of them failed, and reporting it as partial claims an accomplishment there is no \
         record of"
    );
    assert_eq!(
        Partial::new(vec![step("completed")], Vec::new()),
        Err(PartialRefused::NothingOutstanding),
        "a run that left nothing outstanding succeeded, and reporting it as partial sends a \
         reader looking for work that is not there"
    );
    assert!(
        Partial::new(vec![step("completed")], vec![step("outstanding")]).is_ok(),
        "one of each is the smallest report that is genuinely partial"
    );
}

/// ADR-0016 D3's report says the version, where to report, and what is known
/// about the session.
///
/// The mutant: dropping any of the three from the presentation.
#[test]
fn a_defect_says_it_is_a_defect_and_names_the_version_and_where_to_report() {
    let session = a_session();
    let report = defect_report(session.clone());
    let rendered = Presentation::of(&Classified::Defect(report.clone())).to_string();

    let mut missing = Vec::new();
    for (what, needle) in [
        ("the version", report.version()),
        ("where to report", report.report_at()),
        ("the session id", session.id().expect("staged").as_str()),
    ] {
        if !rendered.contains(needle) {
            missing.push(what);
        }
    }
    if !rendered.contains(&report.location().to_string()) {
        missing.push("the location");
    }
    assert!(
        missing.is_empty(),
        "ADR-0016 D3 requires a defect report carry the session id, the version and where to \
         report it. {} of them are absent — {missing:?} — from {rendered:?}",
        missing.len()
    );

    assert!(
        rendered.contains("not something you can configure"),
        "D3: never present a defect as a user error. {rendered:?}"
    );
}

/// With no session there is no transcript, and the report does not claim one.
///
/// **This is the seam, and it is the reason `SessionEvidence` is an enum
/// rather than an `Option` of a path.** ADR-0010 is not started, so the binary
/// passes `NoSessionExists`; a report that printed a transcript path anyway
/// would be telling a user to read a file that was never written.
///
/// The mutant: rendering a placeholder path for the absent case, or dropping
/// the sentence that says there is none.
#[test]
fn a_defect_with_no_session_says_so_rather_than_naming_a_transcript() {
    let report = defect_report(SessionEvidence::NoSessionExists);
    let rendered = Presentation::of(&Classified::Defect(report)).to_string();

    assert!(
        rendered.contains("there is no session and no transcript was written"),
        "with ADR-0010 unbuilt the report must say so rather than name a file: {rendered:?}"
    );
    assert!(
        !rendered.contains("transcript.jsonl"),
        "a report with no session named a transcript anyway: {rendered:?}"
    );

    // The arm that discriminates: with a session staged, the path *is* shown,
    // so the assertion above is not satisfied by a report that never shows one.
    let with = defect_report(a_session());
    let rendered = Presentation::of(&Classified::Defect(with)).to_string();
    assert!(
        rendered.contains("transcript.jsonl"),
        "with a session staged the transcript's path must be shown: {rendered:?}"
    );
}

/// A session id a terminal would mangle is refused where it enters.
///
/// The mutant: dropping either guard, after which a bug report can carry a
/// line that erases the row above it.
#[test]
fn a_session_id_that_cannot_be_rendered_is_refused_at_the_boundary() {
    assert_eq!(
        SessionId::new(""),
        Err(SessionIdRefused::Empty),
        "ADR-0016 D3 requires a defect report name the session, and it cannot name nothing"
    );
    let refused = match SessionId::new("01J\u{1b}[2K") {
        Err(refused) => refused,
        Ok(taken) => panic!(
            "a session id carrying an escape sequence must be refused; a defect report is \
             pasted into a bug report, where one can erase the row above it. It was taken as \
             {taken:?}"
        ),
    };
    assert!(
        matches!(refused, SessionIdRefused::Control { .. }),
        "an escape sequence in a session id was refused for the wrong reason: {refused:?}"
    );

    let id = nonce("session");
    assert_eq!(
        SessionId::new(id.clone()).map(|held| held.as_str().to_owned()),
        Ok(id)
    );
}

/// The class a failure reports is the variant it was built as, for all five.
///
/// The mutant: any arm of `Classified::class` returning a neighbour's class,
/// which is exactly the misclassification D1's Negative consequence calls
/// worse than no classification at all.
#[test]
fn a_failures_class_is_the_variant_it_was_built_as() {
    for (class, classified) in one_of_each_class() {
        assert_eq!(
            classified.class(),
            class,
            "a failure built as {class} reports itself as {}",
            classified.class()
        );
    }

    // And the two accessors that are class-specific answer for exactly the
    // classes D1 and D2 put the obligation on.
    let with_a_remedy: Vec<Class> = Class::ALL
        .into_iter()
        .filter(|class| of_class(*class).remedy().is_some())
        .collect();
    assert_eq!(
        with_a_remedy,
        vec![Class::UserCorrectable],
        "D2 puts a `Remedy` on one class and discharges the other four through their own \
         payloads; this crate believes the remedy-carrying classes are {with_a_remedy:?}"
    );

    let with_a_statement: Vec<Class> = Class::ALL
        .into_iter()
        .filter(|class| of_class(*class).statement().is_some())
        .collect();
    assert_eq!(
        with_a_statement,
        vec![
            Class::Expected,
            Class::UserCorrectable,
            Class::Environmental,
            Class::Capability,
        ],
        "D3 decides what a defect says about itself, so a defect carries no caller-written \
         statement; this crate believes the statement-carrying classes are {with_a_statement:?}"
    );

    // An `Expected` is the one class with nothing to add, and that is the
    // absence D1 asks for rather than an oversight.
    let expected = Expected::new(statement("the loop working"));
    assert!(
        Presentation::of(&Classified::Expected(expected))
            .lines
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// ADR-0016 D3 — the defect boundary
// ---------------------------------------------------------------------------

/// Held for the duration of every guarded call below.
///
/// `panic::set_hook` is process-wide and `take_hook`/`set_hook` is not atomic,
/// so two guards running at once could interleave. The product calls
/// [`guard`] exactly once, from `main`; these checks are the only place two
/// calls could overlap, and this is what stops them. Poisoning is recovered
/// from rather than propagated: a check that panicked while holding it has
/// already reported, and turning that into a second failure in a neighbouring
/// check would report the wrong subject.
static ONE_GUARD_AT_A_TIME: Mutex<()> = Mutex::new(());

fn serialised<T>(body: impl FnOnce() -> T) -> T {
    let _held = ONE_GUARD_AT_A_TIME
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    body()
}

/// ADR-0016 D3 and trigger clause 4: a panic is caught, reported as a defect,
/// and exits 70.
///
/// The mutant: the boundary re-raising instead of catching, which kills the
/// test process; and, as a compile error, the boundary building a
/// user-correctable failure instead — D3's "never present a defect as a user
/// error" is unwritable, because a `UserCorrectable` cannot be constructed
/// without a remedy and a defect has none.
#[test]
fn a_panic_under_the_boundary_becomes_a_defect_that_exits_70() {
    let version = nonce("version");
    let where_to_report = nonce("report-at");
    let said = nonce("what-the-panic-said");

    let caught = serialised(|| {
        match guard(
            &version,
            &where_to_report,
            SessionEvidence::NoSessionExists,
            || panic!("{said}"),
        ) {
            Guarded::Defected(caught) => caught,
            Guarded::Ran(()) => panic!("the boundary did not catch a panic under it"),
        }
    });

    let classified = Classified::Defect(caught.report().clone());
    assert_eq!(
        classified.class(),
        Class::Defect,
        "D3: a panic is presented as what it is"
    );
    assert_eq!(
        Exit::Failed(classified).code(),
        70,
        "ADR-0016 D5: an internal defect exits 70"
    );

    let rendered = caught.to_string();
    for needle in [version.as_str(), where_to_report.as_str()] {
        assert!(
            rendered.contains(needle),
            "D3 requires the report carry the version and where to report it; {needle:?} is not \
             in {rendered:?}"
        );
    }
    assert!(
        rendered.contains("tests.rs"),
        "D3's report is only actionable if it says where the defect surfaced: {rendered:?}"
    );
}

/// The boundary is narrow: nothing under it runs after the panic.
///
/// ADR-0016's Negative consequence is the reason — "catching panics at the
/// session boundary risks masking a corrupted state that should have
/// terminated the process. The boundary must be narrow." The widening this
/// exists to catch is guarding each statement rather than the body, after
/// which the flag below would be set and the process would carry on with half
/// its work done.
///
/// The structural half is that [`Guarded`] has no arm carrying both a defect
/// and a value, so there is nothing for a caller to continue with.
#[test]
fn nothing_after_a_caught_panic_runs() {
    static REACHED: AtomicBool = AtomicBool::new(false);

    let caught = serialised(|| {
        let guarded = guard(
            &nonce("version"),
            &nonce("report-at"),
            SessionEvidence::NoSessionExists,
            || {
                panic!("{}", nonce("stop here"));
                #[allow(unreachable_code)]
                REACHED.store(true, Ordering::SeqCst);
            },
        );
        matches!(guarded, Guarded::Defected(_))
    });

    assert!(
        caught,
        "the boundary must catch the panic for this to mean anything"
    );
    assert!(
        !REACHED.load(Ordering::SeqCst),
        "a statement after the panic ran, so the boundary is wrapping statements rather than the \
         body and a corrupted state can be carried past"
    );
}

/// The panic's own words are captured and are **not** in the report.
///
/// Under the coordinator's ruling of 2026-09-04: the message belongs to
/// ADR-0010's transcript, and it is the one field on this path that can carry
/// arbitrary captured text — the surface ADR-0008's open clause 6 blocks on.
/// It travels beside the report rather than inside it, so no presentation can
/// reach it.
///
/// The absence is asserted over the message **and its ASCII core**, because
/// `{:?}` escapes a combining mark and an absence assertion over the raw value
/// alone reads a published leak as absence — the mutation that survived in the
/// credential store on 2026-09-04.
///
/// The mutant: putting `own_words` into the report, or into the presentation.
#[test]
fn the_panics_own_words_are_captured_and_never_presented() {
    let said = nonce("what-the-panic-said");
    let caught = serialised(|| {
        match guard(
            &nonce("version"),
            &nonce("report-at"),
            SessionEvidence::NoSessionExists,
            || panic!("{said}"),
        ) {
            Guarded::Defected(caught) => caught,
            Guarded::Ran(()) => panic!("the boundary did not catch a panic under it"),
        }
    });

    assert_eq!(
        caught.own_words().as_str(),
        said,
        "the boundary must capture what the panic said, or ADR-0010's transcript has nothing to \
         be handed and the default hook was never replaced"
    );

    let rendered = caught.to_string();
    let core = ascii_core(&said);
    for (arm, needle) in [("the message", said.as_str()), ("its ASCII core", core)] {
        assert!(
            !rendered.contains(needle),
            "the defect presentation published {arm} of what the panic said: {needle:?} is in \
             {rendered:?}"
        );
    }
    assert!(
        !format!("{:?}", caught.report()).contains(core),
        "the panic's words reached the report itself, where a future renderer could show them"
    );
}

/// The arm that discriminates: a body that does not panic is handed back
/// untouched.
///
/// Without it, a boundary that reported a defect for every call would satisfy
/// every check above. [Verification lessons] §13.
///
/// The mutant: reporting a defect unconditionally.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_body_that_does_not_panic_is_handed_back_untouched() {
    let carried = nonce("what-the-body-returned");
    let guarded = serialised(|| {
        guard(
            &nonce("version"),
            &nonce("report-at"),
            SessionEvidence::NoSessionExists,
            || carried.clone(),
        )
    });

    match guarded {
        Guarded::Ran(value) => assert_eq!(
            value, carried,
            "the boundary changed what the body returned"
        ),
        Guarded::Defected(caught) => {
            panic!("a body that did not panic was reported as a defect: {caught}")
        }
    }

    // And an ordinary failing run still exits with its own class's code rather
    // than with the defect's, so the boundary is not swallowing classification.
    let exit = Exit::Failed(of_class(Class::UserCorrectable));
    assert_eq!(exit.code(), 2, "ADR-0016 D5: user-correctable exits 2");
}

/// Two guarded panics in a row each report their own defect.
///
/// The boundary restores the hook it replaced, so a second call is not
/// reporting into the first one's sink. The mutant: a shared sink, after which
/// the second report carries the first panic's words.
#[test]
fn two_guarded_panics_each_report_their_own_defect() {
    let first_said = nonce("first-panic");
    let second_said = nonce("second-panic");

    let (first, second) = serialised(|| {
        let take = |said: &str| match guard(
            &nonce("version"),
            &nonce("report-at"),
            SessionEvidence::NoSessionExists,
            || panic!("{said}"),
        ) {
            Guarded::Defected(caught) => caught,
            Guarded::Ran(()) => panic!("the boundary did not catch a panic under it"),
        };
        (take(&first_said), take(&second_said))
    });

    assert_eq!(first.own_words().as_str(), first_said);
    assert_eq!(
        second.own_words().as_str(),
        second_said,
        "the second guarded panic reported the first one's words, so the hook was not restored \
         between them"
    );
}
