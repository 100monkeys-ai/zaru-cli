// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the command surface.
//!
//! Every check here owns its own argument list, because [`parse`] takes an
//! iterator rather than reading the process — so a hostile command line is an
//! ordinary value rather than something a check would have to spawn a process
//! to say.

use super::*;
use crate::cli::invocation::Request;
use std::ffi::OsString;

/// The words a user typed, as the parser receives them.
fn typed(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

/// Parse, expecting the grammar to admit it.
fn accepted(words: &[&str]) -> CommandLine {
    parse(typed(words)).unwrap_or_else(|refusal| {
        panic!("`zaru {}` was refused: {refusal}", words.join(" "));
    })
}

/// Parse, expecting a refusal.
fn refused(words: &[&str]) -> CommandRefused {
    match parse(typed(words)) {
        Err(refusal) => refusal,
        Ok(line) => panic!(
            "`zaru {}` was accepted as {:?}, and it should not have been",
            words.join(" "),
            line.request
        ),
    }
}

// ---------------------------------------------------------------------------
// ADR-0015 D2 — the namespace table, both spellings, closed
// ---------------------------------------------------------------------------

/// D2's table governs both spellings and neither is derived from the other.
///
/// The mutant this catches is the obvious economy — deriving the subcommand
/// from the slash spelling by dropping the `/` — which produces `session`
/// where [ADR-0010] D4 and D6 say `zaru sessions`, and which D2's own settling
/// sentence forbids by keeping the difference "as it was written in each
/// record rather than normalised".
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn every_namespace_carries_both_of_adr_0015_d2s_spellings_and_one_pair_differs() {
    let mut differing = Vec::new();
    for namespace in Namespace::ALL {
        assert!(
            namespace.slash().starts_with('/'),
            "{namespace:?}'s in-session spelling is not a slash command: {:?}",
            namespace.slash()
        );
        assert!(
            !namespace.subcommand().is_empty(),
            "{namespace:?} has no out-of-session spelling"
        );
        if namespace.slash().trim_start_matches('/') != namespace.subcommand() {
            differing.push((namespace.slash(), namespace.subcommand()));
        }
    }

    assert_eq!(
        differing,
        vec![("/session", "sessions")],
        "ADR-0015 D2 keeps the singular/plural difference as each record wrote it, so exactly one \
         pair differs and it is `/session` against `zaru sessions`; deriving one spelling from the \
         other makes this list empty"
    );
}

/// D2's shadowing rule needs a vocabulary, and the vocabulary is closed.
///
/// Nothing loads a user command yet, so no collision can occur. What is
/// asserted is that the set the rule would be checked against is complete and
/// has no duplicate spelling in either column — a duplicate would make
/// "shadowing" ambiguous before any user command existed.
#[test]
fn the_namespace_set_is_closed_and_no_two_namespaces_share_a_spelling() {
    assert_eq!(
        Namespace::ALL.len(),
        9,
        "ADR-0015 D2's table has eight rows plus `/models`, added 2026-09-05"
    );

    let mut slashes: Vec<&str> = Namespace::ALL.iter().map(|n| n.slash()).collect();
    let mut subcommands: Vec<&str> = Namespace::ALL.iter().map(|n| n.subcommand()).collect();
    slashes.sort_unstable();
    subcommands.sort_unstable();
    let before = (slashes.len(), subcommands.len());
    slashes.dedup();
    subcommands.dedup();

    assert_eq!(
        (slashes.len(), subcommands.len()),
        before,
        "two namespaces share a spelling, so D2's shadowing rule cannot say which one a user \
         command would shadow"
    );
}

/// `models` is a namespace, so D2's shadowing rule reaches it.
///
/// [ADR-0012] D4 names `zaru models` in as many words and D2's table carried
/// no row for it until 2026-09-05. Without the row, a user command called
/// `models` would have been a collision the rule could not see.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[test]
fn adr_0012_d4s_zaru_models_is_a_namespace_rather_than_a_loose_subcommand() {
    let found = Namespace::from_subcommand("models")
        .expect("ADR-0012 D4 spells the command `zaru models`, so it is a namespace");
    assert_eq!(found, Namespace::Models);
    assert_eq!(found.slash(), "/models");
    assert_eq!(
        accepted(&["models"]).request,
        Request::Models,
        "the subcommand does not reach the request it names"
    );
}

// ---------------------------------------------------------------------------
// The grammar
// ---------------------------------------------------------------------------

/// Every command `--help` lists is a command the parser accepts.
///
/// **This is the clause that keeps help honest**, and it runs in the direction
/// that matters: the help text is walked and each spelling is fed back to the
/// parser. A help line naming something the grammar refuses fails here, which
/// is the failure the ruling "help lists only what runs" exists to prevent.
#[test]
fn every_command_the_help_text_lists_is_one_the_parser_accepts() {
    let help = help::lines("0.0.0");
    let listed: Vec<Vec<String>> = help
        .iter()
        .skip_while(|line| line.trim() != "commands:")
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .map(|line| {
            // The command column is padded and separated from its summary by
            // two spaces, so the split is on that rather than on whitespace:
            // `sessions rm <id>` carries a space of its own.
            let spelling = line
                .trim_start()
                .split("  ")
                .next()
                .unwrap_or_default()
                .trim();
            spelling
                .split_whitespace()
                .take_while(|word| !word.starts_with('<'))
                .map(str::to_owned)
                .collect()
        })
        .collect();

    assert!(
        listed.len() >= 5,
        "the help text lists {} command(s), which is too few to have asserted anything about it",
        listed.len()
    );

    let mut unreachable = Vec::new();
    for spelling in &listed {
        // Stand in a value for whatever argument the command takes, so that a
        // command with one is exercised rather than skipped.
        let mut words: Vec<&str> = spelling.iter().map(String::as_str).collect();
        let argument = match words.as_slice() {
            ["config", "explain"] => Some("runtime.tier"),
            ["sessions", "rm"] => Some("01HM2E5Y001440E1G50G1G4080"),
            _ => None,
        };
        if let Some(argument) = argument {
            words.push(argument);
        }
        if parse(typed(&words)).is_err() {
            unreachable.push(spelling.join(" "));
        }
    }

    assert!(
        unreachable.is_empty(),
        "`--help` lists {} command(s) the parser refuses, and a help text that names a command \
         the binary will not run is worse than one that omits it: {unreachable:?}",
        unreachable.len()
    );
}

/// No command the parser accepts is missing from `--help`.
///
/// The other direction, which catches a namespace becoming built without a
/// help line.
#[test]
fn every_built_namespace_appears_in_the_help_text() {
    let help = help::lines("0.0.0").join("\n");
    let mut missing = Vec::new();
    for namespace in Namespace::ALL {
        let listed = help.contains(&format!("  {}", namespace.subcommand()));
        if namespace.is_built() != listed {
            missing.push((namespace.subcommand(), namespace.is_built(), listed));
        }
    }
    assert!(
        missing.is_empty(),
        "the help text and Namespace::is_built disagree about (subcommand, built, listed): \
         {missing:?}"
    );
}

/// A flag's value may be attached with `=` or given as the next word.
#[test]
fn a_flag_takes_its_value_attached_or_separate_and_the_two_are_one_line() {
    let separate = accepted(&["--runtime", "contained"]);
    let attached = accepted(&["--runtime=contained"]);
    assert_eq!(
        separate, attached,
        "`--runtime contained` and `--runtime=contained` are the same line, and ADR-0003 D2's \
         acceptance made that ADR-0015's decision to write down"
    );
    assert_eq!(separate.overrides.tier.as_deref(), Some("contained"));
}

/// `--` ends flag parsing, which is how a task beginning with a dash is said.
#[test]
fn a_double_dash_ends_flag_parsing_and_what_follows_is_never_a_flag() {
    let line = accepted(&["--", "--runtime", "please fix this"]);
    assert_eq!(
        line.overrides.tier, None,
        "`--runtime` after `--` set a configuration layer, so `--` did not end flag parsing"
    );
    assert_eq!(
        line.request,
        Request::Task {
            words: vec!["--runtime".to_owned(), "please fix this".to_owned()],
        }
    );
}

/// A command is a word and a task is a sentence.
///
/// Both arms, because the refusal alone is satisfied by a parser that refuses
/// every line it does not recognise, and the acceptance alone by one that
/// treats every unknown word as a task and never suggests anything.
#[test]
fn one_unknown_word_is_a_mistyped_command_and_anything_longer_is_a_task() {
    match refused(&["runtim"]) {
        CommandRefused::UnknownCommand { offered, nearest } => {
            assert_eq!(offered, "runtim");
            assert_eq!(
                nearest, "runtime",
                "ADR-0014 D5's nearest match, over the subcommands this harness implements"
            );
        }
        other => panic!("one mistyped word was not read as a mistyped command: {other}"),
    }

    assert_eq!(
        accepted(&["fix", "the", "failing", "test"]).request,
        Request::Task {
            words: vec![
                "fix".to_owned(),
                "the".to_owned(),
                "failing".to_owned(),
                "test".to_owned(),
            ],
        },
        "four words are a task"
    );
    assert_eq!(
        accepted(&["fix the failing test"]).request,
        Request::Task {
            words: vec!["fix the failing test".to_owned()],
        },
        "one quoted sentence is a task, because it carries whitespace"
    );
}

/// The nearest match is placed against what runs, not against the whole table.
#[test]
fn an_unbuilt_namespace_is_named_rather_than_placed_against_the_nearest_command() {
    match refused(&["stack"]) {
        CommandRefused::NamespaceNotBuilt { namespace } => {
            assert_eq!(namespace, Namespace::Stack);
        }
        other => panic!(
            "`zaru stack` should say the namespace is not built rather than suggest a neighbour: \
             {other}"
        ),
    }

    // The consequence, rather than the candidate list the check would
    // otherwise be reading back out of the thing under test: a word one edit
    // from an unbuilt namespace must not be answered with that namespace,
    // because acting on the suggestion produces a refusal.
    let mut answered_with_something_that_refuses = Vec::new();
    for namespace in Namespace::ALL.into_iter().filter(|n| !n.is_built()) {
        let subcommand = namespace.subcommand();
        let typo: String = subcommand[..subcommand.len() - 1].to_owned();
        match refused(&[&typo]) {
            CommandRefused::UnknownCommand { nearest, .. } => {
                if !Namespace::from_subcommand(nearest).is_some_and(Namespace::is_built) {
                    answered_with_something_that_refuses.push((typo, nearest));
                }
            }
            CommandRefused::NamespaceNotBuilt { .. } => {}
            other => panic!("`zaru {typo}` was refused unexpectedly: {other}"),
        }
    }
    assert!(
        answered_with_something_that_refuses.is_empty(),
        "a typo was answered with a command that would itself refuse, which is the remedy \
         ADR-0016 D2 calls a stack trace with better grammar: {answered_with_something_that_refuses:?}"
    );
}

/// A flag given twice is refused naming both values.
#[test]
fn a_flag_given_twice_is_refused_naming_both_values() {
    match refused(&["--runtime", "bare", "--runtime", "linked"]) {
        CommandRefused::FlagRepeated {
            flag,
            first,
            second,
        } => {
            assert_eq!(flag, "--runtime");
            assert_eq!(
                (first.as_deref(), second.as_deref()),
                (Some("bare"), Some("linked")),
                "a repeated flag names both values, so the reader can see which two they gave"
            );
        }
        other => panic!("a repeated flag was not refused: {other}"),
    }
}

/// A request flag beside a subcommand is refused rather than one winning.
#[test]
fn a_request_flag_beside_a_subcommand_is_refused_rather_than_silently_preferred() {
    match refused(&["--continue", "models"]) {
        CommandRefused::RequestFlagWithCommand { flag, command } => {
            assert_eq!(flag, "--continue");
            assert_eq!(command, "models");
        }
        other => panic!("`--continue models` was not refused: {other}"),
    }
    assert!(matches!(
        refused(&["--resume", "01HM2E5Y001440E1G50G1G4080", "--continue"]),
        CommandRefused::ResumeAndContinue
    ));
}

/// `--help` answers before anything else refuses.
#[test]
fn help_is_answered_even_beside_a_line_that_would_otherwise_be_refused() {
    assert_eq!(
        accepted(&["nonsense", "--help"]).request,
        Request::Help,
        "a user who cannot remember the grammar is the user most likely to type something else \
         wrong in the same line"
    );
    assert_eq!(
        accepted(&[]).request,
        Request::Help,
        "`zaru` with no arguments answers the question the user asked by typing the name"
    );
}

/// Nothing reaches the rest of the program as a string it has to re-read.
///
/// The two arguments the grammar takes are validated by the types that own
/// them, so a refusal for a bad key or a bad id happens once, here, in that
/// type's own words.
#[test]
fn an_argument_is_validated_by_the_type_that_owns_it_and_never_travels_as_text() {
    let line = accepted(&["config", "explain", "runtime.tier"]);
    match line.request {
        Request::ConfigExplain { key } => assert_eq!(key.as_str(), "runtime.tier"),
        other => panic!("`config explain` did not produce a key: {other:?}"),
    }

    assert!(matches!(
        refused(&["config", "explain", "runtime..tier"]),
        CommandRefused::UnusableKey(_)
    ));
    assert!(matches!(
        refused(&["sessions", "rm", "not-a-ulid"]),
        CommandRefused::UnusableSessionId(_)
    ));
    assert!(matches!(
        refused(&["--resume", "not-a-ulid"]),
        CommandRefused::UnusableSessionId(_)
    ));
}

/// A tier is not validated here, so there is one refusal for a bad one.
///
/// The mutant this catches is a parser that calls `Tier::named` and refuses,
/// which would produce a second refusal beside `TierRefused::NoSuchTier` —
/// and the second one could not name the layer the value arrived in, which is
/// the whole point of resolving it through ADR-0014's fold.
#[test]
fn a_tier_the_parser_does_not_recognise_is_carried_rather_than_refused() {
    let line = accepted(&["--runtime", "nonsense", "runtime"]);
    assert_eq!(
        line.overrides.tier.as_deref(),
        Some("nonsense"),
        "the parser refused a tier, so a bad `--runtime` now has two refusals and only one of \
         them can say which layer it came from"
    );
}

/// An argument that is not text is refused rather than unwrapped.
#[test]
fn an_argument_that_is_not_text_is_refused_rather_than_becoming_a_defect() {
    use std::os::unix::ffi::OsStringExt;

    let hostile = OsString::from_vec(vec![b'r', 0xFF, b'm']);
    match parse(vec![hostile]) {
        Err(CommandRefused::NotText { lossy }) => assert!(
            lossy.contains('r') && lossy.contains('m'),
            "the lossy rendering must show the reader where the problem is: {lossy:?}"
        ),
        other => panic!(
            "an argument carrying an invalid byte must be refused as the user's, not unwrapped \
             into ADR-0016 D3's defect boundary: {other:?}"
        ),
    }
}

/// Every verb `--help` implies is one the namespace declares.
#[test]
fn a_namespaces_verbs_are_one_list_read_by_both_the_parser_and_the_help_text() {
    assert!(matches!(
        refused(&["sessions", "lst"]),
        CommandRefused::UnknownVerb {
            namespace: Namespace::Session,
            ..
        }
    ));
    assert!(matches!(
        refused(&["sessions"]),
        CommandRefused::VerbMissing {
            namespace: Namespace::Session
        }
    ));
    assert!(matches!(
        refused(&["runtime", "extra"]),
        CommandRefused::UnexpectedWord { .. }
    ));
    assert!(matches!(
        refused(&["config", "explain"]),
        CommandRefused::ArgumentMissing { .. }
    ));
    assert!(matches!(
        refused(&["--runtime"]),
        CommandRefused::FlagNeedsValue { .. }
    ));
    assert!(matches!(
        refused(&["--continue=yes"]),
        CommandRefused::FlagTakesNoValue { .. }
    ));
    assert!(matches!(
        refused(&["--runtimee", "bare"]),
        CommandRefused::UnknownFlag {
            nearest: "--runtime",
            ..
        }
    ));
}
