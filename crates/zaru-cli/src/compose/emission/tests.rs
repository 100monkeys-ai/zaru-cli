// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The instrument [ADR-0002] trigger clauses 1 and 8 ask for.
//!
//! Four checks. Two walk this crate's own product source off disk, in the
//! shapes `only_one_place_in_the_product_records_a_tip_showing` and
//! `corpus_one_place_in_the_terminal_renders_a_classified_failure` already
//! use; two walk the registry itself.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output

use super::{Cause, Door, EXEMPT, Subject, UNPROMPTED_HOMES, Unprompted, Wording};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// This crate's `src`, which is the only tree these walks read.
fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// One product source file: its path relative to the crate root, and its text.
struct Product {
    relative: String,
    text: String,
}

/// Every product source file under `src`, with the test trees left out.
///
/// # What is skipped, and the one exemption that is not obvious
///
/// A `tests.rs` and a `fixtures.rs` are not the product, which is the rule
/// [`only_one_place_in_the_product_records_a_tip_showing`] already applies.
/// **`src/compose/emission.rs` is skipped too**, and that is the registry's
/// own file: [`Door::needle`] spells every door there, so a walk that read it
/// would find every door opened in the one module that opens none. A rule and
/// an instance of it are different things, which is the discrimination
/// `corpus_one_place_in_the_terminal_renders_a_classified_failure` makes for
/// comments and this makes for one file.
///
/// [`only_one_place_in_the_product_records_a_tip_showing`]: crate::compose::tips
fn product_files() -> Vec<Product> {
    let root = source_root();
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    let mut walk = vec![root];
    while let Some(at) = walk.pop() {
        for entry in std::fs::read_dir(&at).expect("the source directory reads") {
            let path = entry.expect("the directory entry reads").path();
            if path.is_dir() {
                walk.push(path);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests.rs" || name == "fixtures.rs" {
                continue;
            }
            let relative = path
                .strip_prefix(crate_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if relative == "src/compose/emission.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("the source file reads");
            found.push(Product { relative, text });
        }
    }
    found
}

/// Whether this line is code rather than prose about code.
///
/// A doc comment or an ordinary comment naming a door is a description of the
/// rule; only an uncommented line is an instance of it.
fn is_code(line: &str) -> bool {
    !line.trim_start().starts_with("//")
}

/// Every door is opened only in the files the registry names.
///
/// # What this asserts, and why it is what makes the registry exhaustive
///
/// [`Unprompted`] on its own is a list, and [ADR-0002] clause 1 rejects a list
/// — it asks for an enumeration of "what the harness can emit". This is the
/// other half: [`Door::ALL`] is the closed set of ways an unprompted line
/// reaches a person, each naming the product files that may open it, and a
/// call from anywhere else fails here. So the pair is an enumeration and its
/// proof rather than a list somebody has to remember to extend.
///
/// # The liveness arm
///
/// A walk that read the wrong directory, or whose skip list grew until it
/// excused everything, passes vacuously — the shape [Verification lessons] §8
/// names. `zaru-cli`'s `src` is some hundreds of files; a floor well under
/// that still catches a scan that found nothing.
///
/// # The mutant and the accepting sibling
///
/// The mutant is a second `.set_standing(` anywhere under `src/`, which is the
/// exact shape of a line added to an unprompted surface without a member. The
/// accepting sibling is `terminal/driver.rs`'s
/// `Line::new(Register::Plain, KEY_IS_STORED)` — an authored constant painted
/// into the pane, which stays green because it is the answer to a command the
/// user typed and opens no door. This check bans an unregistered *door*, never
/// a literal.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_unprompted_door_is_opened_only_where_the_registry_says() {
    let files = product_files();

    assert!(
        files.len() > 100,
        "this walk read {} product file(s), which is too few to have asserted anything about \
         where an unprompted line reaches a person",
        files.len()
    );

    let mut wrong: Vec<String> = Vec::new();
    for door in Door::ALL {
        let needle = door.needle();
        let mut opened: BTreeSet<&str> = BTreeSet::new();
        for file in &files {
            for line in file.text.lines() {
                if is_code(line) && line.contains(needle) {
                    opened.insert(file.relative.as_str());
                }
            }
        }
        let declared: BTreeSet<&str> = door.opened_in().iter().copied().collect();
        if opened != declared {
            wrong.push(format!(
                "{door:?} (needle {needle:?}) is opened in {opened:?} and the registry says \
                 {declared:?}"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "every unprompted line reaches a person through a door the registry names, so that a \
         line added to one of these surfaces cannot escape ADR-0002's emission set. {} door(s) \
         disagree with the tree:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}

/// No member of the emission set reports on the user's own behaviour.
///
/// # This is [ADR-0002] clause 8 held as a shape rather than as a grep
///
/// Clause 8 asks for the assertion to be made "by enumerating the emission set
/// exhaustively rather than by grepping for words". Every member declares a
/// [`Subject`], [`Subject::TheUser`] is the variant D7's prohibition names,
/// and nothing may declare it: a streak or a characterisation cannot be added
/// to an unprompted surface without writing that variant down, and writing it
/// down is this check's red.
///
/// # The mutant and the accepting sibling
///
/// The mutant is one member's [`Unprompted::subject`] arm changed to
/// [`Subject::TheUser`]. The accepting sibling is every other member, which
/// declares a subject from the same enum and stays green — the check bans one
/// subject, not the declaring of subjects.
///
/// # What this does **not** claim
///
/// Clause 8 does not move on this check. [`Unprompted::TurnCounter`] is
/// declared [`Subject::TheSession`] on a reading that is stated rather than
/// settled, and while that line's subject is a question the clause is
/// withheld. See that variant's own documentation.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn no_member_of_the_emission_set_reports_on_the_user() {
    let reporting: Vec<Unprompted> = Unprompted::ALL
        .into_iter()
        .filter(|line| line.subject() == Subject::TheUser)
        .collect();

    assert!(
        reporting.is_empty(),
        "ADR-0002 D7: \"Zaru does not report on the user's own behaviour\". {} member(s) of the \
         emission set declare the user as their subject: {reporting:?}",
        reporting.len()
    );

    // The other arm, without which the check above is satisfied by a set with
    // no members at all.
    let subjects: BTreeSet<&str> = Unprompted::ALL
        .into_iter()
        .map(|line| match line.subject() {
            Subject::TheHarness => "the harness",
            Subject::ATier => "a tier",
            Subject::TheProject => "the project",
            Subject::TheModel => "the model",
            Subject::TheSession => "the session",
            Subject::TheUser => "the user",
        })
        .collect();
    assert!(
        subjects.len() >= 5,
        "the emission set declares {} distinct subject(s), which is too few for the absence of \
         one to mean anything: {subjects:?}",
        subjects.len()
    );
}

/// Every authored sentence in an unprompted home is a member or is exempt.
///
/// # This is the closure arm, and it is what stops the registry going stale
///
/// A registry nobody has to extend is a registry that goes stale the first
/// time a sentence is added. The three modules in [`UNPROMPTED_HOMES`] are
/// where an unprompted line's wording is declared; this reads each off disk,
/// takes every `pub const … : &str` it declares, and fails when one is neither
/// referred to by a member's [`Wording::Authored`] nor named in [`EXEMPT`]
/// with its reason.
///
/// It runs in both directions: an exemption whose constant has stopped being
/// declared fails as loudly as a constant with no member, so the list cannot
/// quietly outlive what it excuses.
///
/// # The mutant and the accepting sibling
///
/// The mutant is a new `pub const … : &str` in `compose/prose.rs` — the exact
/// shape of an authored sentence arriving with no member. The accepting
/// sibling is `prose::SUMMARISE_SPAN`, exempt with its reason because it is
/// read by a model, which stays green.
#[test]
fn every_authored_sentence_in_an_unprompted_home_is_a_member_or_exempt() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut declared: BTreeSet<String> = BTreeSet::new();
    for home in UNPROMPTED_HOMES {
        let text = std::fs::read_to_string(crate_root.join(home))
            .unwrap_or_else(|_| panic!("{home} is named as an unprompted home and does not read"));
        let mut found = 0_usize;
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("pub const ") else {
                continue;
            };
            let Some((name, tail)) = rest.split_once(':') else {
                continue;
            };
            // `&str` and `&'static str` alike, and nothing else — a `usize`
            // or a `u32` is not a sentence.
            if !tail.contains("str") {
                continue;
            }
            declared.insert(name.trim().to_owned());
            found += 1;
        }
        assert!(
            found > 0,
            "{home} is named as an unprompted home and declares no string constant, so this walk \
             read the wrong file or the home has moved"
        );
    }

    let members: BTreeSet<&str> = Unprompted::ALL
        .into_iter()
        .filter_map(|line| match line.wording() {
            Wording::Authored { name, .. } => Some(name),
            Wording::Composed { .. } => None,
        })
        .collect();
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(name, _)| *name).collect();

    let unaccounted: Vec<&String> = declared
        .iter()
        .filter(|name| !members.contains(name.as_str()) && !exempt.contains(name.as_str()))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "an authored sentence is declared in an unprompted home and no registry member refers to \
         it, so it would reach a person outside ADR-0002's emission set: {unaccounted:?}"
    );

    let stale: Vec<&str> = exempt
        .iter()
        .chain(members.iter())
        .filter(|name| !declared.contains(**name))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "the registry names constant(s) that no unprompted home declares any more, so the walk \
         is asserting against a tree that has moved: {stale:?}"
    );
}

/// Every member names a cause, a clause, a door and a wording, and the set
/// prints.
///
/// # The instrument's own output
///
/// Run with `--nocapture` this prints the emission set as a table: the line,
/// the record and clause that puts it in front of a person, its cause in
/// [ADR-0002] clause 1's three terms, its subject in D7's vocabulary, and the
/// door it arrives through. That listing is the artefact clauses 1 and 8 ask
/// for, and it is produced by walking [`Unprompted::ALL`] rather than by being
/// typed.
///
/// # What it asserts about clause 1, exactly
///
/// Every member's cause is a user message or a turn in progress, and
/// **nothing declares [`Cause::ArmedTrigger`]** — asserted here rather than
/// assumed, because the fact that no line traces to a trigger is the fact that
/// makes clause 1 two of three rather than whole. No trigger type exists in
/// this workspace, so the third term is unused rather than satisfied.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn every_member_names_a_cause_a_clause_a_door_and_a_wording() {
    assert_eq!(
        Unprompted::ALL.len(),
        18,
        "the emission set changed size without this check being read"
    );

    let mut texts: Vec<&str> = Vec::new();
    let mut rows: Vec<String> = Vec::new();
    for line in Unprompted::ALL {
        let (record, clause) = line.clause();
        assert!(
            record.starts_with("ADR-0") && record.len() == 8,
            "{line:?} names {record:?}, which is not a four-digit record in this workspace"
        );
        assert!(
            clause.starts_with('D') && clause.len() >= 2,
            "{line:?} names {clause:?}, which is not a decision clause"
        );

        let shown = match line.wording() {
            Wording::Authored { name, text } => {
                assert!(!text.is_empty(), "{line:?} refers to an empty constant");
                texts.push(text);
                format!("{name} = {text:?}")
            }
            Wording::Composed { by } => {
                assert!(
                    by.starts_with("crate::"),
                    "{line:?} names {by:?}, which is not a path a reader can open"
                );
                format!("composed by {by}")
            }
        };

        // Clause 1's third term, asserted rather than assumed.
        assert_ne!(
            line.cause(),
            Cause::ArmedTrigger,
            "{line:?} claims to trace to an armed trigger, and no type in this workspace \
             represents one"
        );

        rows.push(format!(
            "{record} {clause:<3} {:<18} {:<15} {:<20} {shown}",
            format!("{:?}", line.cause()),
            format!("{:?}", line.subject()),
            format!("{:?}", line.door()),
        ));
    }

    let before = texts.len();
    texts.sort_unstable();
    texts.dedup();
    assert_eq!(
        texts.len(),
        before,
        "two members of the emission set refer to the same sentence, so one of them is a second \
         spelling of the other"
    );

    println!("ADR-0002's emission set — {} lines", Unprompted::ALL.len());
    for row in &rows {
        println!("  {row}");
    }
    for (name, reason) in EXEMPT {
        println!("  exempt: {name} — {reason}");
    }
}
