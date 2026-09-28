// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every fact the README states about this build is checked against the code.
//!
//! # Why this file exists
//!
//! No decision record governs the README, so nothing else stops it drifting
//! from the program. An audit (`operations/harness-look-and-feel-audit-2`,
//! row 12) found four false sentences and eleven false counts in an earlier
//! version, each one true on the day it was written.
//!
//! # What is checked, and what is not
//!
//! Each arm reads one list or table the README carries and compares it, in
//! both directions, with the constant that decides it: the commands and flags
//! `--help` prints, the exit codes, the provider kinds that have a client, the
//! built-in tools, the permission modes, the configuration layers, the
//! validator kinds, the model aliases and the pinned toolchain. The last arm
//! checks that every relative link and every `#anchor` resolves.
//!
//! The arms find their list by the table it sits in or by a short phrase on
//! the same line, and compare the **items**, never the wording around them.
//! Rewording a sentence is free. Changing what it lists, or a constant
//! changing under it, fails here with both lists printed.
//!
//! A fact the README states that no arm here reads is not checked. When you
//! add one that a later commit could make false, add an arm for it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The repository root, from this crate's manifest rather than from the cwd.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root resolves from this crate's manifest")
}

fn readme() -> String {
    std::fs::read_to_string(repo_root().join("README.md")).expect("the repository has a README")
}

/// The lines under `heading`, down to the next heading of the same or a
/// higher level. `heading` is written with its hashes, as in the file.
fn section(text: &str, heading: &str) -> String {
    let level = heading.chars().take_while(|c| *c == '#').count();
    let mut lines = text.lines().skip_while(|line| line.trim() != heading);
    assert!(
        lines.next().is_some(),
        "the README has no heading {heading:?}"
    );
    lines
        .take_while(|line| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            !(hashes > 0 && hashes <= level && line[hashes..].starts_with(' '))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first cell of every body row of every table in `text`.
fn first_cells(text: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut in_table = false;
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            in_table = false;
            continue;
        }
        if !in_table {
            // The header row.
            in_table = true;
            continue;
        }
        if line.starts_with("| ---") {
            continue;
        }
        let cell = line.trim_start_matches('|').split('|').next().unwrap_or("");
        cells.push(cell.trim().to_owned());
    }
    cells
}

/// Every span between backticks in `text`.
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The one line of the README containing `phrase`.
fn line_with(text: &str, phrase: &str) -> String {
    let found: Vec<&str> = text.lines().filter(|line| line.contains(phrase)).collect();
    assert_eq!(
        found.len(),
        1,
        "the README should have exactly one line containing {phrase:?}; it has {found:#?}"
    );
    found[0].to_owned()
}

/// The one sentence of the README containing `phrase`.
///
/// A sentence rather than a line, because a paragraph is one line and a list
/// in the next sentence of it is not the list this phrase introduces.
fn sentence_with(text: &str, phrase: &str) -> String {
    line_with(text, phrase)
        .split(". ")
        .find(|sentence| sentence.contains(phrase))
        .expect("the phrase is in one of the line's sentences")
        .to_owned()
}

/// A count as the README spells it.
fn spelled(count: usize) -> &'static str {
    [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ]
    .get(count)
    .copied()
    .unwrap_or_else(|| panic!("the README spells no count as large as {count}"))
}

/// The word immediately before `phrase` on `line`, lower-cased.
fn word_before(line: &str, phrase: &str) -> String {
    let at = line.find(phrase).expect("the phrase is on the line");
    line[..at]
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_lowercase()
}

/// Assert two sets are equal, printing what each side has that the other
/// lacks.
fn same(what: &str, readme: &BTreeSet<String>, build: &BTreeSet<String>) {
    assert!(!build.is_empty(), "{what}: the build side is empty");
    let missing: Vec<_> = build.difference(readme).collect();
    let extra: Vec<_> = readme.difference(build).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{what}: the README is missing {missing:?} and names {extra:?}, which this build does not \
         have. The README lists {readme:?}; this build has {build:?}"
    );
}

fn set<I: IntoIterator<Item = S>, S: Into<String>>(items: I) -> BTreeSet<String> {
    items.into_iter().map(Into::into).collect()
}

/// The command table lists exactly the commands `--help` prints.
///
/// The mutant: a command renamed, added or removed in `cli::help`, or a row
/// added to the table for a command that does not exist.
#[test]
fn the_command_table_is_what_help_prints() {
    let text = section(&readme(), "## Command reference");
    let listed = set(first_cells(&text).iter().filter_map(|cell| {
        cell.strip_prefix("`zaru ")
            .and_then(|rest| rest.strip_suffix('`'))
            .map(str::to_owned)
    }));
    let printed = set(zaru_cli::cli::Namespace::ALL
        .into_iter()
        .flat_map(zaru_cli::cli::help::summaries_of)
        .map(|(spelling, _)| *spelling));
    same("commands", &listed, &printed);
}

/// The flag table lists exactly the flags `--help` prints, with their values.
#[test]
fn the_flag_table_is_what_help_prints() {
    let text = section(&readme(), "## Command reference");
    let listed = set(first_cells(&text).iter().filter_map(|cell| {
        cell.strip_prefix("`--")
            .and_then(|rest| rest.strip_suffix('`'))
            .map(|rest| format!("--{rest}"))
    }));
    let printed = set(zaru_cli::cli::Flag::ALL
        .into_iter()
        .map(|flag| match flag.value_name() {
            Some(value) => format!("{} {value}", flag.spelling()),
            None => flag.spelling().to_owned(),
        }));
    same("flags", &listed, &printed);
}

/// The exit code table is the error taxonomy's codes, success, and the signal
/// row.
#[test]
fn the_exit_code_table_is_the_taxonomy() {
    let cells = first_cells(&section(&readme(), "## Exit codes"));
    let listed = set(cells
        .iter()
        .filter(|cell| cell.parse::<u8>().is_ok())
        .cloned());
    let taxonomy = set(std::iter::once(zaru_cli::failure::SUCCESS)
        .chain(
            zaru_cli::failure::Class::ALL
                .into_iter()
                .map(zaru_cli::failure::Class::exit_code),
        )
        .map(|code| code.to_string()));
    same("exit codes", &listed, &taxonomy);

    let offset = zaru_cli::failure::signalled(0);
    assert!(
        cells.contains(&format!("{offset} + n")),
        "a signal ends the program with {offset} + n, and the exit code table has no such row: \
         {cells:?}"
    );
}

/// Where the README says which provider kinds work and which do not, it names
/// exactly the kinds that have a client and exactly the ones that do not.
#[test]
fn the_warning_the_quick_start_shows_is_the_one_the_program_prints() {
    let text = section(&readme(), "### 2. Run a task");
    assert!(
        text.contains(zaru_cli::compose::prose::NOT_A_SANDBOX),
        "the quick start shows a warning the program does not print; it prints {:?}",
        zaru_cli::compose::prose::NOT_A_SANDBOX
    );
}

/// The keys the README says a permission prompt takes are the ones the
/// prompt's own line names, word for word.
#[test]
fn the_permission_prompt_the_readme_quotes_is_the_one_the_program_shows() {
    let text = section(&readme(), "### Permission prompts and modes");
    let line = zaru_cli::tools::prompt::SUFFIX.trim();
    assert!(
        text.contains(line),
        "the README quotes a permission prompt the program does not show; it shows {line:?}"
    );
}

/// The Safety section says that `yolo` asks nothing, and so lets a model
/// change which validators are approved.
#[test]
fn the_safety_section_says_what_yolo_gives_a_model() {
    let text = section(&readme(), "## Safety");
    assert!(
        text.contains("In `yolo` mode it asks nothing, so a model can also change which validators are approved"),
        "the Safety section does not say what `yolo` lets a model do: {text}"
    );
}

#[test]
fn the_provider_kinds_named_as_working_are_the_ones_with_a_client() {
    use zaru_cli::providers::ProviderKind;
    let text = readme();
    let kinds_on = |line: &str| {
        set(backticked(line)
            .into_iter()
            .filter(|token| ProviderKind::parse(token).is_some()))
    };
    let with = set(zaru_cli::compose::KINDS_WITH_A_CLIENT
        .into_iter()
        .map(ProviderKind::as_str));
    let without = set(ProviderKind::ALL
        .into_iter()
        .filter(|kind| !zaru_cli::compose::KINDS_WITH_A_CLIENT.contains(kind))
        .map(ProviderKind::as_str));

    let working = sentence_with(&text, "provider kinds work");
    same("kinds that work", &kinds_on(&working), &with);
    assert_eq!(
        word_before(&working, "provider kinds work"),
        spelled(with.len()),
        "the README counts the kinds that work wrongly: {working:?}"
    );
    same(
        "kinds with no client",
        &kinds_on(&sentence_with(&text, "have no client")),
        &without,
    );
    same(
        "kinds `provider.default.kind` accepts",
        &kinds_on(&line_with(&text, "| `provider.default.kind` |")),
        &with,
    );
}

/// The sentence naming the built-in tools names all of them, and counts them.
#[test]
fn the_built_in_tools_named_are_this_builds() {
    use zaru_cli::tools::ToolName;
    let line = sentence_with(&readme(), "built-in tools");
    let names = set(ToolName::ALL.into_iter().map(ToolName::as_str));
    let listed = set(backticked(&line)
        .into_iter()
        .filter(|token| token.contains('.') && !token.contains(' ')));
    same("built-in tools", &listed, &names);
    assert_eq!(
        word_before(&line, "built-in tools"),
        spelled(names.len()),
        "the README counts the built-in tools wrongly: {line:?}"
    );
}

/// The mode table lists exactly the permission modes, and marks the default.
#[test]
fn the_mode_table_is_this_builds_modes() {
    use zaru_cli::tools::Mode;
    let cells = first_cells(&section(&readme(), "### Permission prompts and modes"));
    let listed = set(cells
        .iter()
        .filter_map(|cell| backticked(cell).into_iter().next()));
    same(
        "permission modes",
        &listed,
        &set(Mode::ALL.into_iter().map(Mode::as_str)),
    );
    let default = format!("`{}` (default)", Mode::default().as_str());
    assert!(
        cells.contains(&default),
        "the mode table should mark {default:?} as the default; its rows are {cells:?}"
    );
}

/// The numbered list of places settings come from has one entry per layer.
#[test]
fn the_configuration_list_has_every_layer() {
    let text = section(&readme(), "### Configuration");
    let numbered: Vec<&str> = text
        .lines()
        .filter(|line| {
            line.split_once(". ")
                .is_some_and(|(number, _)| number.parse::<usize>().is_ok())
        })
        .collect();
    assert_eq!(
        numbered.len(),
        zaru_cli::config::Layer::ALL.len(),
        "the README lists {} places settings come from, and this build resolves {} layers: \
         {numbered:#?}",
        numbered.len(),
        zaru_cli::config::Layer::ALL.len()
    );
}

/// The sentence naming what `expect` can be names every validator kind.
#[test]
fn the_validator_kinds_named_are_this_builds() {
    let line = sentence_with(&readme(), "`expect` can be");
    let listed = set(backticked(&line).into_iter().skip(1).map(|token| {
        token
            .trim_matches(|c| c == '{' || c == '}' || c == '"' || c == ' ')
            .split(" =")
            .next()
            .unwrap_or("")
            .to_owned()
    }));
    same(
        "validator kinds",
        &listed,
        &set(zaru_core::iteration::validator::Expect::KINDS),
    );
}

/// The `zaru models` row names every model alias.
#[test]
fn the_model_aliases_named_are_this_builds() {
    use zaru_cli::providers::ModelAlias;
    let line = line_with(&readme(), "| `zaru models` |");
    let listed = set(backticked(&line).into_iter().skip(1));
    same(
        "model aliases",
        &listed,
        &set(ModelAlias::ALL.into_iter().map(ModelAlias::as_str)),
    );
}

/// The Rust version the README asks for is the one `rust-toolchain.toml` pins.
#[test]
fn the_rust_version_named_is_the_pinned_one() {
    let pinned = std::fs::read_to_string(repo_root().join("rust-toolchain.toml"))
        .expect("the repository pins its toolchain");
    let channel = pinned
        .lines()
        .find_map(|line| line.strip_prefix("channel = "))
        .expect("the toolchain file names a channel")
        .trim_matches('"');
    let wanted = format!("Rust {channel}");
    assert!(
        readme().contains(&wanted),
        "rust-toolchain.toml pins {channel}, and the README does not say {wanted:?}"
    );
}

/// GitHub's anchor for a heading: lower case, punctuation other than `-` and
/// `_` removed, spaces made hyphens.
fn anchor_of(heading: &str) -> String {
    heading
        .trim_start_matches('#')
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Every relative link points at a file in the tree, and every `#anchor` at a
/// heading in the README.
#[test]
fn every_link_in_the_readme_resolves() {
    let text = readme();
    let mut fenced = false;
    let mut headings = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            headings.insert(anchor_of(line));
        }
    }

    let targets: Vec<&str> = text
        .split("](")
        .skip(1)
        .filter_map(|rest| rest.split(')').next())
        .collect();
    assert!(
        targets.len() >= 5,
        "found only {} link(s), so this is not reading the README's links: {targets:?}",
        targets.len()
    );

    let mut broken = Vec::new();
    for target in targets {
        if target.starts_with("http://") || target.starts_with("https://") {
            continue;
        }
        if let Some(anchor) = target.strip_prefix('#') {
            if !headings.contains(anchor) {
                broken.push(format!("#{anchor}: no heading has that anchor"));
            }
        } else if !repo_root().join(target).exists() {
            broken.push(format!("{target}: no such file in the repository"));
        }
    }
    assert!(
        broken.is_empty(),
        "the README has links that go nowhere: {broken:#?}"
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
