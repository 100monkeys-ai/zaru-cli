// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every number on the repository's front page is a constant this build holds.
//!
//! # The rule, and why it has no record behind it
//!
//! `operations/harness-look-and-feel-audit-2` row 12 measured four sentences in
//! `README.md` that were false: "The binary starts no session", "Nothing
//! populates it on this machine" of the hint strip, "Seven commands run" and
//! "the eleven namespaces". A bare `zaru` opens a session, the strip is served
//! from `~/.zaru/corpus.jsonl`, `--help` walks seventeen command spellings and
//! the picker counts twelve namespaces. Eleven further count-or-capability
//! claims were measured false beside them.
//!
//! That row's own diagnosis is the reason this file exists: **no record
//! governs the README**, which is why it had drifted further than `--help`,
//! which at least has a clause. A file with no clause behind it should carry
//! no number a commit elsewhere can falsify — so every number left in it is
//! asserted here against the constant that decides it, and the ones no
//! constant decides were deleted rather than corrected.
//!
//! # Three arms, and the limit of the third stated plainly
//!
//! [`every_number_the_readme_carries_is_the_constant_it_names`] pins each claim
//! to its constant. [`the_command_block_names_only_commands_that_run`] pins the
//! fenced block to the same table `--help` is walked from.
//! [`a_counted_noun_is_counted_once`] pins the other direction: a second
//! sentence counting a thing this file already counts cannot disagree with the
//! first, which is the shape "Seven of the eleven namespaces answer" took.
//!
//! **What none of them catches is a number attached to a noun no row here
//! names.** That is a real limit rather than an oversight: the alternative is a
//! rule over every English "one" and "two" in the prose, which needs an
//! exemption list, and an exemption list goes stale in exactly the way this
//! file exists to stop. An arc that teaches the README to count a new thing
//! owes this file a row, and the doc comment on [`claims`] says so where that
//! arc will read it.

use std::path::{Path, PathBuf};

/// The repository root, from this crate's manifest rather than from the cwd.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root resolves from this crate's manifest")
}

/// `README.md` with its fenced blocks removed and its whitespace flattened.
///
/// Fenced blocks are out of scope for the same reason a laid-out row is out of
/// `sentence_spacing_from_outside`'s: they are examples and values rather than
/// prose, `max_iterations = 3` is not a claim about this build, and the one
/// block that *is* a claim is read by its own arm below. Flattening is what
/// lets an anchor span the line breaks a paragraph happens to have today.
fn prose() -> String {
    let text = std::fs::read_to_string(repo_root().join("README.md"))
        .expect("the repository has a README");
    let mut kept = String::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            kept.push_str(line);
            kept.push(' ');
        }
    }
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A count as the README spells it.
fn spelled(count: usize) -> &'static str {
    match count {
        1 => "one",
        2 => "two",
        3 => "three",
        4 => "four",
        5 => "five",
        6 => "six",
        7 => "seven",
        8 => "eight",
        9 => "nine",
        10 => "ten",
        11 => "eleven",
        12 => "twelve",
        13 => "thirteen",
        14 => "fourteen",
        15 => "fifteen",
        16 => "sixteen",
        17 => "seventeen",
        18 => "eighteen",
        19 => "nineteen",
        20 => "twenty",
        21 => "twenty-one",
        22 => "twenty-two",
        23 => "twenty-three",
        24 => "twenty-four",
        other => panic!("the README spells no number as large as {other}"),
    }
}

/// How many crates this workspace ships, from the list `--version` prints.
///
/// `composition()` names the five libraries the binary is composed *of*, so
/// the workspace is those plus the binary crate itself. Read from the
/// composition rather than from `Cargo.toml`'s members because that is the
/// list `--version` already puts in front of a reader.
fn workspace_crates() -> usize {
    zaru_cli::composition().len() + 1
}

/// How many packages `Cargo.lock` resolves, counted from the file itself.
fn lock_packages() -> usize {
    std::fs::read_to_string(repo_root().join("Cargo.lock"))
        .expect("the repository has a lock file")
        .lines()
        .filter(|line| *line == "[[package]]")
        .count()
}

/// How many rows `[workspace.dependencies]` carries.
///
/// A row is a top-level `name = ` at column zero inside that table, which is
/// what `Cargo.toml`'s own comment calls "a dependency arriving here in the arc
/// that has a caller". Continuation lines of a multi-line value are indented
/// and are not rows.
fn workspace_dependencies() -> usize {
    let manifest = std::fs::read_to_string(repo_root().join("Cargo.toml"))
        .expect("the repository has a workspace manifest");
    let table = manifest
        .split_once("[workspace.dependencies]")
        .expect("the workspace declares its dependencies")
        .1;
    let table = match table.find("\n[workspace.lints") {
        Some(at) => &table[..at],
        None => table,
    };
    table
        .lines()
        .filter(|line| {
            !line.starts_with(char::is_whitespace) && !line.starts_with('#') && line.contains(" = ")
        })
        .count()
}

/// Every number the README is allowed to carry, with the constant that decides
/// it and the words it sits between.
///
/// **An arc that teaches the README to count a new thing adds a row here.** A
/// number with no row is not caught by any arm in this file, which is the limit
/// the module documentation states; the cost of the alternative is an exemption
/// list over ordinary English, and that is the thing row 12 is about.
///
/// The anchor is the words on either side of the number, so a paragraph that is
/// rewritten around it reddens here and is corrected by whoever rewrote it —
/// which is the point rather than a cost.
fn claims() -> Vec<(&'static str, &'static str, String)> {
    let kinds = zaru_cli::providers::ProviderKind::ALL.len();
    let with_a_client = zaru_cli::compose::KINDS_WITH_A_CLIENT.len();
    let namespaces = zaru_cli::cli::Namespace::ALL.len();
    let built = zaru_cli::cli::Namespace::ALL
        .into_iter()
        .filter(|namespace| namespace.is_built())
        .count();
    let tools = zaru_cli::tools::ToolName::ALL.len();
    let layers = zaru_cli::config::Layer::ALL.len();
    let classes = zaru_cli::failure::Class::ALL.len();
    let dependencies = workspace_dependencies();

    vec![
        (
            "the crates that compile",
            "installable.** ? crates compile",
            spelled(workspace_crates()).to_owned(),
        ),
        (
            "the kinds with no client",
            "will refuse.** ? of ADR-0012",
            spelled(kinds - with_a_client).to_owned(),
        ),
        (
            "ADR-0012 D3's kinds",
            "ADR-0012 D3's ? provider kinds",
            spelled(kinds).to_owned(),
        ),
        (
            "the kinds a refusal names",
            "refused naming the ? that do",
            spelled(with_a_client).to_owned(),
        ),
        (
            "the namespaces",
            "vocabulary: the ? namespaces",
            spelled(namespaces).to_owned(),
        ),
        (
            "the namespaces that answer",
            "subcommand runs. ? of the",
            spelled(built).to_owned(),
        ),
        (
            "the built-in tools",
            "whatever of the ? built-in tools",
            spelled(tools).to_owned(),
        ),
        (
            "the tools that act",
            "permission model. **All ? act**",
            spelled(tools).to_owned(),
        ),
        (
            "the tool web.fetch was last of",
            "the last of the ? to act",
            spelled(tools).to_owned(),
        ),
        (
            "the configuration layers",
            "zaru-cli`, resolving ? layers",
            spelled(layers).to_owned(),
        ),
        (
            "the layers with readers",
            "own them; **all ? layers have readers",
            spelled(layers).to_owned(),
        ),
        (
            "the failure classes",
            "exits through. ? classes of failure",
            spelled(classes).to_owned(),
        ),
        (
            "the packages Cargo.lock resolves",
            "`Cargo.lock` resolves ? packages",
            lock_packages().to_string(),
        ),
        (
            "the workspace's own packages",
            "packages, ? of which are this workspace's own",
            spelled(workspace_crates()).to_owned(),
        ),
        (
            "the third-party rows",
            "third-party set is ? rows",
            spelled(dependencies).to_owned(),
        ),
        (
            "what those rows pull in",
            "and what those ? pull in",
            spelled(dependencies).to_owned(),
        ),
    ]
}

/// **Arm 1.** Every number the README carries is the constant it names.
///
/// The mutant: any constant changed — a namespace added, a dependency taken,
/// a tool named — reddens here with the sentence and both numbers.
#[test]
fn every_number_the_readme_carries_is_the_constant_it_names() {
    // Lower-cased on both sides, because a claim that opens a sentence is
    // capitalised and a claim inside one is not -- "Six crates compile" and
    // "resolving five layers" are the same kind of statement, and which of
    // them a paragraph happens to start with is not this arm's business.
    let prose = prose().to_lowercase();
    let mut wrong = Vec::new();

    for (what, anchor, expected) in claims() {
        let (before, after) = anchor
            .split_once('?')
            .expect("every anchor marks where its number sits");
        let sought = format!("{before}{expected}{after}").to_lowercase();
        let found = prose.matches(sought.as_str()).count();
        println!("{what}: {sought:?} -> {found}");
        if found != 1 {
            wrong.push(format!(
                "{what}: the README should say {sought:?} exactly once and says it {found} \
                 time(s); the constant is {expected}"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "no record governs the README, so every number in it is held to the constant that \
         decides it. {} claim(s) disagree: {wrong:#?}",
        wrong.len()
    );
}

/// **Arm 2.** The fenced block names only commands that run.
///
/// The block is the one fenced thing in the README that is a claim rather than
/// an example, and it is walked against the same `summaries_of` table `--help`
/// is walked from — so a renamed command reddens here without the README being
/// opened, which is what row 12's "walked from the same constants" asks for.
///
/// The mutant: a spelling changed in `cli::help`, or a line added to the block.
#[test]
fn the_command_block_names_only_commands_that_run() {
    let text = std::fs::read_to_string(repo_root().join("README.md"))
        .expect("the repository has a README");
    let after = text
        .split_once("Some of them:")
        .expect("the README introduces its command block")
        .1;
    let block = after
        .split("```")
        .nth(1)
        .expect("the block is fenced")
        .trim_start_matches("sh")
        .trim();

    let printed: Vec<&str> = zaru_cli::cli::Namespace::ALL
        .into_iter()
        .flat_map(zaru_cli::cli::help::summaries_of)
        .map(|(spelling, _)| *spelling)
        .collect();

    let mut listed = 0_usize;
    for line in block.lines() {
        let command = line.split('#').next().unwrap_or("").trim();
        if command.is_empty() {
            continue;
        }
        listed += 1;
        let spelling = command
            .strip_prefix("zaru ")
            .expect("every line in the block is an invocation of this binary");
        println!("block: {spelling}");
        assert!(
            printed.contains(&spelling),
            "the README's block offers `zaru {spelling}`, and `--help` lists no such command; \
             it lists {printed:?}"
        );
    }
    assert!(
        listed >= 5,
        "the block lists {listed} command(s), which is too few to be the block"
    );
}

/// **Arm 3.** A counted noun is counted once.
///
/// "Seven of the eleven namespaces answer" was false twice over and in two
/// different ways, and the second number was a *second* count of a thing the
/// paragraph above had already counted. This arm holds every mention of a
/// counted noun to the same number, so a paragraph added later cannot count it
/// differently.
///
/// A mention with no number before it is not a count and is left alone — "a
/// directory named by a ULID holding plain files" is prose, not arithmetic.
///
/// The mutant: any second number attached to one of these nouns.
#[test]
fn a_counted_noun_is_counted_once() {
    let prose = prose();
    let namespaces = zaru_cli::cli::Namespace::ALL.len();
    let nouns = [
        ("crates", spelled(workspace_crates()).to_owned()),
        (
            "provider kinds",
            spelled(zaru_cli::providers::ProviderKind::ALL.len()).to_owned(),
        ),
        ("namespaces", spelled(namespaces).to_owned()),
        (
            "built-in tools",
            spelled(zaru_cli::tools::ToolName::ALL.len()).to_owned(),
        ),
        (
            "layers",
            spelled(zaru_cli::config::Layer::ALL.len()).to_owned(),
        ),
        (
            "classes of failure",
            spelled(zaru_cli::failure::Class::ALL.len()).to_owned(),
        ),
        ("packages", lock_packages().to_string()),
    ];

    let mut wrong = Vec::new();
    let mut counted = 0_usize;
    for (noun, expected) in nouns {
        for (at, _) in prose.match_indices(noun) {
            let before = prose[..at].trim_end();
            let Some(word) = before.rsplit(' ').next() else {
                continue;
            };
            let word = word.trim_matches(|c: char| !c.is_alphanumeric());
            let is_a_count = word.chars().next().is_some_and(char::is_numeric)
                || (1..=17).any(|n| spelled(n).eq_ignore_ascii_case(word));
            if !is_a_count {
                continue;
            }
            counted += 1;
            if !word.eq_ignore_ascii_case(&expected) {
                wrong.push(format!(
                    "the README says {word:?} {noun} where this build has {expected}"
                ));
            }
        }
    }

    assert!(
        counted >= 8,
        "only {counted} counted noun(s) were found, so this arm is reading something other than \
         the README"
    );
    assert!(
        wrong.is_empty(),
        "a thing this file counts is counted twice and the two disagree: {wrong:#?}"
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
