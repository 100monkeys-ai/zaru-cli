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
use crate::tools::WorkingDirectory;
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
        11,
        "ADR-0015 D2's table has eight rows plus `/models`, `/init` and `/providers`, all \
         three added 2026-09-05"
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
        let mut words: Vec<&str> = spelling
            .iter()
            .map(String::as_str)
            // A `<placeholder>` in the help text stands for an argument, and
            // the arm below supplies a real one; carrying the placeholder
            // through would have the parser refuse a command the help lists
            // for the wrong reason.
            .filter(|word| !word.starts_with('<'))
            .collect();
        // A slice rather than one value, because `notes tokens add` takes two
        // words and a command that took two would otherwise be exercised with
        // one and refused for the wrong reason.
        let arguments: Vec<&str> = match words.as_slice() {
            ["config", "explain"] => vec!["runtime.tier"],
            ["sessions", "rm"] => vec!["01HM2E5Y001440E1G50G1G4080"],
            // An alias and an instance host. Neither is reached: the parser is
            // all this exercises, and nothing here opens a session.
            ["notes", "tokens", "add"] => vec!["work", "cortex.page"],
            // `<kind>` is one of ADR-0012 D3's five, and the help text spells
            // the placeholder rather than the value. Taken from
            // `ProviderKind::ALL` rather than written here, so a sixth kind
            // does not leave this arm exercising a name that is no longer the
            // first one.
            ["providers", "keys", "add"] => vec![
                crate::providers::ProviderKind::ALL
                    .first()
                    .expect("ADR-0012 D3 names at least one kind")
                    .as_str(),
            ],
            _ => Vec::new(),
        };
        words.extend(arguments);
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
        Request::Session,
        "`zaru` with no arguments is the request to be in a session, and which answer a reader \
         gets is `terminal::open`'s rather than the parser's"
    );
    // The two are not the same request, which is the whole of the 2026-09-06
    // Update: a bare `zaru` at a terminal opens a session and `--help` never
    // does, however it is spelled.
    assert_eq!(accepted(&["--help"]).request, Request::Help);
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

// ---------------------------------------------------------------------------
// ADR-0016 — the class each failure this surface can raise belongs to
// ---------------------------------------------------------------------------

/// Every refusal the grammar can produce is user-correctable and carries a
/// remedy that reaches the reader.
///
/// **Enumerated rather than sampled**, which is what [ADR-0016] trigger clause
/// 3 asks for in as many words and what [Verification Lessons] §4 calls the
/// disguise a spot check wears: a remedy that exists for most refusals and not
/// the one a user hits is indistinguishable from having no policy.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [Verification Lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons
#[test]
fn every_refusal_the_grammar_produces_is_the_users_and_says_what_to_change() {
    let surface = classify::Surface::new("0.0.0", "https://example.invalid/report");
    let mut wrong = Vec::new();

    for (label, words) in refusable_lines() {
        let refusal = refuse(words);
        let classified = surface.command(&refusal);
        if classified.class() != crate::failure::Class::UserCorrectable {
            wrong.push((label, format!("{:?}", classified.class())));
            continue;
        }
        let presented = crate::failure::Presentation::of(&classified).to_string();
        if presented.lines().count() < 2 {
            wrong.push((label, "the remedy reached no line".to_owned()));
        }
    }

    assert!(
        wrong.is_empty(),
        "a command line is by construction something the user typed, so every refusal of one is \
         ADR-0016 D1 row 2 and every one carries a remedy the reader can see: {wrong:?}"
    );
}

/// Every variant of the grammar's refusal is exercised by the list above.
///
/// **Asserts the staging.** Without this the check above is satisfied by a
/// list that reaches three variants, which is [Verification Lessons] §14 —
/// a check that can decline passes vacuously, and the decline here is a
/// variant nobody wrote a line for.
///
/// [Verification Lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons
#[test]
fn the_refusal_list_reaches_every_variant_the_grammar_can_produce() {
    let mut seen: Vec<&'static str> = refusable_lines()
        .into_iter()
        .map(|(_, words)| variant_of(&refuse(words)))
        .collect();
    seen.sort_unstable();
    seen.dedup();

    let mut expected = vec![
        "NotText",
        "UnknownCommand",
        "NamespaceNotBuilt",
        "VerbMissing",
        "UnknownVerb",
        "UnexpectedWord",
        "ArgumentMissing",
        "UnknownFlag",
        "FlagNeedsValue",
        "FlagTakesNoValue",
        "FlagRepeated",
        "ResumeAndContinue",
        "RequestFlagWithCommand",
        "UnusableKey",
        "UnusableSessionId",
    ];
    expected.sort_unstable();

    assert_eq!(
        seen, expected,
        "the enumeration above must reach every variant `CommandRefused` has, or a variant with \
         no remedy passes by never being built"
    );
}

/// One line per refusal variant the grammar can produce, with a label.
///
/// The last is real bytes rather than a stand-in: `NotText` cannot be reached
/// from text at all, and a list that omitted it would leave one variant with
/// no remedy and nothing saying so.
fn refusable_lines() -> Vec<(String, Vec<OsString>)> {
    use std::os::unix::ffi::OsStringExt;

    let mut lines: Vec<(String, Vec<OsString>)> = [
        vec!["runtim"],
        vec!["stack"],
        vec!["sessions"],
        vec!["sessions", "lst"],
        vec!["runtime", "extra"],
        vec!["config", "explain"],
        vec!["--runtimee", "bare"],
        vec!["--runtime"],
        vec!["--continue=yes"],
        vec!["--runtime", "bare", "--runtime", "linked"],
        vec!["--resume", "01HM2E5Y001440E1G50G1G4080", "--continue"],
        vec!["--continue", "models"],
        vec!["config", "explain", "runtime..tier"],
        vec!["sessions", "rm", "not-a-ulid"],
    ]
    .into_iter()
    .map(|words| (words.join(" "), typed(&words)))
    .collect();

    lines.push((
        "an argument carrying an invalid byte".to_owned(),
        vec![OsString::from_vec(vec![b'r', 0xFF, b'm'])],
    ));
    lines
}

/// Parse an argument list, expecting a refusal.
fn refuse(words: Vec<OsString>) -> CommandRefused {
    match parse(words) {
        Err(refusal) => refusal,
        Ok(line) => panic!(
            "a line meant to be refused was accepted as {:?}",
            line.request
        ),
    }
}

/// Which variant a refusal is, as a name a check can compare.
fn variant_of(refusal: &CommandRefused) -> &'static str {
    match refusal {
        CommandRefused::NotText { .. } => "NotText",
        CommandRefused::UnknownCommand { .. } => "UnknownCommand",
        CommandRefused::NamespaceNotBuilt { .. } => "NamespaceNotBuilt",
        CommandRefused::VerbMissing { .. } => "VerbMissing",
        CommandRefused::UnknownVerb { .. } => "UnknownVerb",
        CommandRefused::UnexpectedWord { .. } => "UnexpectedWord",
        CommandRefused::ArgumentMissing { .. } => "ArgumentMissing",
        CommandRefused::UnknownFlag { .. } => "UnknownFlag",
        CommandRefused::FlagNeedsValue { .. } => "FlagNeedsValue",
        CommandRefused::FlagTakesNoValue { .. } => "FlagTakesNoValue",
        CommandRefused::FlagRepeated { .. } => "FlagRepeated",
        CommandRefused::ResumeAndContinue => "ResumeAndContinue",
        CommandRefused::RequestFlagWithCommand { .. } => "RequestFlagWithCommand",
        CommandRefused::UnusableKey(_) => "UnusableKey",
        CommandRefused::UnusableSessionId(_) => "UnusableSessionId",
        CommandRefused::UnusableAlias(_) => "UnusableAlias",
    }
}

/// A remedy that names a command names one `--help` lists.
///
/// The command surface is what made `Action::runnable` usable at all — before
/// it, `failure::classify`'s every remedy was a described action, "because the
/// command surface is ADR-0015's and does not exist". It does now, and the
/// discipline that replaces the old refusal is that a suggested command has to
/// be one this binary runs.
#[test]
fn every_command_a_remedy_suggests_is_one_the_parser_accepts() {
    let surface = classify::Surface::new("0.0.0", "https://example.invalid/report");
    let mut suggested = Vec::new();
    let mut unrunnable = Vec::new();

    for (_, words) in refusable_lines() {
        let classified = surface.command(&refuse(words));
        let crate::failure::Classified::UserCorrectable { remedy, .. } = &classified else {
            continue;
        };
        for action in remedy.actions() {
            let Some(command) = action.command() else {
                continue;
            };
            suggested.push(command.to_owned());
            let rest: Vec<&str> = command.split_whitespace().skip(1).collect();
            if parse(typed(&rest)).is_err() {
                unrunnable.push(command.to_owned());
            }
        }
    }

    assert!(
        !suggested.is_empty(),
        "no remedy suggested a command at all, so this check asserted nothing"
    );
    assert!(
        unrunnable.is_empty(),
        "a remedy suggested a command this binary refuses, which is ADR-0016 D2's stack trace \
         with better grammar: {unrunnable:?}"
    );
}

/// A file this harness wrote and cannot read back is reported as ours.
///
/// The provenance reading, at the one seam where it goes the other way: every
/// other failure the surface meets is about the user's machine, and a
/// malformed transcript is about ours. Both arms, because a classifier that
/// called everything a defect would satisfy the defect arm perfectly.
#[test]
fn a_file_this_harness_wrote_is_a_defect_and_a_missing_one_is_the_users() {
    use crate::failure::{Class, SessionEvidence};
    use crate::session::{ResumeFailure, TranscriptError};

    let surface = classify::Surface::new("0.0.0", "https://example.invalid/report");

    let absent = ResumeFailure::NoSuchDirectory {
        path: std::path::PathBuf::from("/nowhere/01HM2E5Y001440E1G50G1G4080"),
    };
    assert_eq!(
        surface
            .resume(&absent, SessionEvidence::NoSessionExists)
            .class(),
        Class::UserCorrectable,
        "a session the user named and that is not there is the user's, and the remedy is the \
         listing"
    );

    let torn = ResumeFailure::Transcript(TranscriptError::Malformed {
        path: std::path::PathBuf::from("/nowhere/transcript.jsonl"),
        line: 4,
        detail: "expected value".to_owned(),
    });
    assert_eq!(
        surface
            .resume(&torn, SessionEvidence::NoSessionExists)
            .class(),
        Class::Defect,
        "this harness is the only writer of a transcript, so a complete line it cannot read back \
         is a file it wrote wrongly -- telling the reader to check their configuration would be \
         ADR-0016 D3's own worked mistake"
    );
}

// ---------------------------------------------------------------------------
// ADR-0014 D1 — layers 1 and 5
// ---------------------------------------------------------------------------

/// The flags are a layer rather than a value the program reads separately.
///
/// The mutant this catches is the obvious shortcut — reading
/// `overrides.tier` where the tier is wanted — which produces a tier the
/// explain block cannot account for, because it never entered the fold.
#[test]
fn a_flag_reaches_the_tier_through_adr_0014s_layer_five_and_not_around_it() {
    let overrides = Overrides {
        tier: Some("linked".to_owned()),
        model: None,
        mode: None,
    };
    let resolution =
        layers::resolve(&overrides, [], &Files::none()).expect("three readable layers fold");
    let explanation = resolution.explain(&crate::runtime::key());

    assert_eq!(
        explanation.effective_layer(),
        Some(crate::config::Layer::Flag),
        "a tier given as a flag must be supplied by layer 5, so `config explain` and the resolved \
         tier are one reading rather than two"
    );

    let resolved = crate::runtime::ResolvedTier::from_configuration(&resolution)
        .expect("`linked` is one of ADR-0001 D1's three tiers");
    assert_eq!(resolved.tier(), crate::runtime::Tier::Linked);
    assert_eq!(resolved.supplied_by(), crate::config::Layer::Flag);
}

/// Layer 1 supplies `bare`, so a machine with no configuration has a tier.
///
/// Both arms. The refusal alone is satisfied by a binary that always answers
/// `bare`; the default alone by one that ignores what the user set. What
/// separates them is that the *supplying layer* differs, which is the thing
/// D3's block exists to print.
#[test]
fn adr_0014_layer_one_supplies_bare_and_every_higher_layer_still_wins() {
    let bare =
        layers::resolve(&Overrides::default(), [], &Files::none()).expect("layer 1 alone folds");
    let resolved = crate::runtime::ResolvedTier::from_configuration(&bare)
        .expect("layer 1 supplies ADR-0001 D1's on-ramp tier");
    assert_eq!(
        (resolved.tier(), resolved.supplied_by()),
        (crate::runtime::BUILT_IN_TIER, crate::config::Layer::BuiltIn),
        "with nothing configured the tier is the built-in one and the trace says so; a default \
         hidden inside the resolution would be a value D3's block could not show"
    );

    let overridden = layers::resolve(
        &Overrides {
            tier: Some("contained".to_owned()),
            model: None,
            mode: None,
        },
        [],
        &Files::none(),
    )
    .expect("layers 1 and 5 fold");
    let resolved = crate::runtime::ResolvedTier::from_configuration(&overridden)
        .expect("`contained` is one of ADR-0001 D1's three tiers");
    assert_eq!(
        (resolved.tier(), resolved.supplied_by()),
        (crate::runtime::Tier::Contained, crate::config::Layer::Flag),
        "layer 5 beats layer 1, or the built-in is a floor rather than a default"
    );
}

/// A flag can reach three keys and no others.
///
/// D1 illustrates layer 5 as "`--tier`, `--model`, …" and names no closed
/// list, so what belongs here is the set of keys whose **own records** give
/// them a flag: `runtime.tier` from ADR-0001 D2 (which spells it `--runtime`),
/// `model.default` from ADR-0012 D4, and `tools.mode` from ADR-0011 D3, which
/// arrived on 2026-09-05. What the check holds is the *count and the
/// spellings*: a flag that reached a fourth key would let the command line set
/// something no record put on layer 5, and it would do it silently.
///
/// **This read "the two keys" until 2026-09-05.** The ellipsis in D1's own
/// illustration is why the number is not itself a decision — the list grows
/// when a record gives its key a flag, and this assertion is where that has to
/// be said out loud.
///
/// The mutant is dropping `tools.mode` from `Flags::of`.
#[test]
fn layer_five_carries_exactly_the_three_keys_whose_records_name_a_flag() {
    use crate::config::LayerSource;

    let everything = Overrides {
        tier: Some("bare".to_owned()),
        model: Some("a-model".to_owned()),
        mode: Some("allow".to_owned()),
    };
    let document = layers::Flags::of(&everything)
        .read()
        .expect("a flag layer was read from the process before it was built");

    let mut reached: Vec<String> = Vec::new();
    fn walk(prefix: &str, table: &crate::config::Table, into: &mut Vec<String>) {
        for (name, value) in table.iter() {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            match value {
                crate::config::Value::Table(inner) => walk(&path, inner, into),
                _ => into.push(path),
            }
        }
    }
    walk("", &document, &mut reached);
    reached.sort();

    assert_eq!(
        reached,
        vec![
            "model.default".to_owned(),
            "runtime.tier".to_owned(),
            "tools.mode".to_owned(),
        ],
        "layer 5 reaches exactly the three keys whose records name a flag for them"
    );

    let nothing = layers::Flags::of(&Overrides::default())
        .read()
        .expect("an empty flag layer is still a layer");
    assert!(
        nothing.is_empty(),
        "a command line with no settings must contribute an empty layer 5, which D3 renders as \
         `(not set)`, rather than a layer carrying defaults"
    );
}

/// A layer this binary did not open names its own label; one it opened names
/// its file.
///
/// **Both arms, and the second is new on 2026-09-05.** Until then layers 2 and
/// 3 had no reader at all and this check asserted only the first half —
/// "naming `~/.zaru/config.toml` in the source column would claim a reading
/// that did not happen". They have readers now, so the rule is unchanged and
/// its consequence inverts: a layer that *was* opened names the file, because
/// the column is what makes ADR-0014 D3's block evidence about where a value
/// came from.
///
/// A check that only kept the first arm would now be asserting that the binary
/// never reads a file, which is the assertion that would go quiet as this arc
/// landed.
///
/// The mutant is a source that names the layer's label even when the file was
/// read.
#[test]
fn a_layer_this_binary_did_not_open_names_its_own_label_and_one_it_opened_names_its_file() {
    // Nothing opened.
    let unread = layers::resolve(&Overrides::default(), [], &Files::none())
        .expect("three readable layers fold");
    let rendered = unread.explain(&crate::runtime::key()).to_string();
    assert!(
        rendered.contains("user config") && rendered.contains("project config"),
        "a layer with no file appears under its own label: {rendered}"
    );
    assert!(
        !rendered.contains(".toml"),
        "no layer this binary did not open may name a file in D3's source column, or the block \
         claims a reading that did not happen: {rendered}"
    );

    // Both opened.
    let tree = crate::tools::fixtures::ScratchTree::new();
    let home = tree.base().join("home");
    std::fs::create_dir_all(&home).expect("staging: the scratch home");
    std::fs::write(
        home.join(crate::config::CONFIG_FILE),
        b"[model]\ndefault = \"from-the-user-file\"\n",
    )
    .expect("staging: layer 2");
    std::fs::write(
        tree.project().join(crate::manifest::MANIFEST_FILE),
        b"[project]\nname = \"from-the-project-file\"\n",
    )
    .expect("staging: layer 3");

    let files = Files::at(
        Some(&home),
        Some(WorkingDirectory::at(tree.project()).expect("the project directory exists")),
    );
    let read = layers::resolve(&Overrides::default(), [], &files).expect("five layers fold");
    let block = read
        .explain(&crate::providers::ModelAlias::Default.key())
        .to_string();
    assert!(
        block.contains(crate::config::CONFIG_FILE),
        "a layer this binary opened names the file it opened: {block}"
    );
    let project_block = read
        .explain(&crate::config::Key::new(crate::manifest::NAME_KEY).expect("a key"))
        .to_string();
    let marked: Vec<&str> = project_block
        .lines()
        .filter(|line| line.contains(crate::config::explain::EFFECTIVE_MARKER))
        .collect();
    assert_eq!(
        marked.len(),
        1,
        "exactly one row is marked:\n{project_block}"
    );
    assert!(
        marked[0].trim_start().starts_with("3 ")
            && marked[0].contains(crate::manifest::MANIFEST_FILE)
            && marked[0].contains("from-the-project-file"),
        "layer 3 names the manifest it read AND carries what that file set; a source column that \
         named the file over an empty document would satisfy a check that only read the name:\n\
         {project_block}"
    );
    println!("{block}{project_block}");
}

/// ADR-0014 clause 1, whole, from the fold this binary runs.
///
/// "A value set in all five layers resolves to the flag, and `config explain`
/// prints every layer with the effective one marked." **The five-layer half was
/// short by two layers until 2026-09-05**, because from the binary a value could
/// only be set at layers 1, 4 and 5; layers 2 and 3 have readers now and this is
/// the whole clause.
///
/// The key is `model.default`, which is free at every layer. `runtime.tier`
/// cannot be the demonstration: ADR-0014 D6 refuses it to layer 3, so a
/// five-layer case over it is a case the record forbids.
///
/// Every expected value is a literal this check owns, and the effective one is
/// read out of the **rendered block** rather than asked of the resolution, so
/// the two sides do not travel through one path
/// ([Verification lessons] §11).
///
/// The mutant is any precedence order wrong in the middle; the adjacent-pair
/// check in `config::tests` is what sees that, and this one sees the top.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn adr_0014_clause_1_a_value_set_in_all_five_layers_resolves_to_the_flag() {
    let tree = crate::tools::fixtures::ScratchTree::new();
    let home = tree.base().join("home");
    std::fs::create_dir_all(&home).expect("staging: the scratch home");
    std::fs::write(
        home.join(crate::config::CONFIG_FILE),
        b"[model]\ndefault = \"layer-two\"\n",
    )
    .expect("staging: layer 2");
    std::fs::write(
        tree.project().join(crate::manifest::MANIFEST_FILE),
        b"[project]\nname = \"p\"\n",
    )
    .expect("staging: layer 3");

    // Layer 3 sets it through the manifest's `[project]`? It cannot -- ADR-0009
    // D1's manifest carries no `model` table -- so layer 3's contribution is
    // `[project]` and `[runtime]` and the alias is set at the other four. The
    // five-layer case for a key layer 3 CAN set is `project.name`, below.
    let files = Files::at(
        Some(&home),
        Some(WorkingDirectory::at(tree.project()).expect("the project directory exists")),
    );
    let resolution = layers::resolve(
        &Overrides {
            tier: None,
            model: Some("layer-five".to_owned()),
            mode: None,
        },
        [("ZARU_MODEL_DEFAULT".to_owned(), "layer-four".to_owned())],
        &files,
    )
    .expect("five layers fold");

    let block = resolution
        .explain(&crate::providers::ModelAlias::Default.key())
        .to_string();
    let marked: Vec<&str> = block
        .lines()
        .filter(|line| line.contains(crate::config::explain::EFFECTIVE_MARKER))
        .collect();
    assert_eq!(marked.len(), 1, "exactly one row is marked:\n{block}");
    assert!(
        marked[0].trim_start().starts_with("5 ") && marked[0].contains("layer-five"),
        "ADR-0014 D1: the flag wins over every layer below it:\n{block}"
    );
    assert_eq!(
        block.lines().count(),
        6,
        "D3's block is the key's line and one row per layer, always five:\n{block}"
    );
    // The three layers below the flag are each present with their own value, so
    // the marked row is "the highest that SET it" rather than "the highest".
    for planted in ["layer-two", "layer-four"] {
        assert!(
            block.contains(planted),
            "every layer that set the key appears in the block:\n{block}"
        );
    }
    println!("{block}");
}

/// A malformed configuration file is the user's, and the message names it.
///
/// [ADR-0016] D3: "Never present a defect as a user error", and its converse is
/// what this holds — until 2026-09-05 `LoadFailure::Source` was carried as a
/// defect, which was honest while nothing could produce one and would have told
/// a user with a typo in their own file to report a bug in the harness.
///
/// The mutant is classifying the source arm as a defect.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn a_malformed_configuration_file_is_the_users_and_the_message_names_it() {
    use crate::failure::{Class, Classified};

    let tree = crate::tools::fixtures::ScratchTree::new();
    let home = tree.base().join("home");
    std::fs::create_dir_all(&home).expect("staging: the scratch home");
    // The mistake is on the second line on purpose, so "line 2" in the message
    // is the parser's reading of this file rather than a constant that would be
    // right for any file whose first line is wrong.
    std::fs::write(
        home.join(crate::config::CONFIG_FILE),
        b"[model]\ndefault = \n",
    )
    .expect("staging: a file a person mistyped");

    let failure = layers::resolve(&Overrides::default(), [], &Files::at(Some(&home), None))
        .expect_err("a file that is not TOML does not fold");
    let classified = Surface::load(&failure);

    assert_eq!(
        classified.class(),
        Class::UserCorrectable,
        "a file this harness never writes is the user's: {classified:?}"
    );
    let Classified::UserCorrectable { statement, .. } = &classified else {
        panic!("expected a user-correctable classification, got {classified:?}");
    };
    let rendered = statement.to_string();
    assert!(
        rendered.contains(crate::config::CONFIG_FILE) && rendered.contains("line 2"),
        "the message names the file and where: {rendered}"
    );
    println!("{rendered}");
}

/// Layer 4 reaches the same keys as layer 5, and layer 5 beats it.
///
/// The pair matters more than either: an adjacent-pair check is the only one
/// that sees a precedence order wrong in the middle, which the configuration
/// arc measured on this very fold.
#[test]
fn the_environment_sets_the_same_keys_and_a_flag_beats_it() {
    let from_environment = layers::resolve(
        &Overrides::default(),
        [("ZARU_RUNTIME_TIER".to_owned(), "contained".to_owned())],
        &Files::none(),
    )
    .expect("layers 1 and 4 fold");
    let resolved = crate::runtime::ResolvedTier::from_configuration(&from_environment)
        .expect("`contained` is a tier");
    assert_eq!(
        (resolved.tier(), resolved.supplied_by()),
        (
            crate::runtime::Tier::Contained,
            crate::config::Layer::Environment
        )
    );

    let flag_wins = layers::resolve(
        &Overrides {
            tier: Some("linked".to_owned()),
            model: None,
            mode: None,
        },
        [("ZARU_RUNTIME_TIER".to_owned(), "contained".to_owned())],
        &Files::none(),
    )
    .expect("layers 1, 4 and 5 fold");
    let resolved =
        crate::runtime::ResolvedTier::from_configuration(&flag_wins).expect("`linked` is a tier");
    assert_eq!(
        (resolved.tier(), resolved.supplied_by()),
        (crate::runtime::Tier::Linked, crate::config::Layer::Flag),
        "ADR-0014 D1's layer 5 beats layer 4, and the trace says which supplied it"
    );
}

/// A tier no record names is refused once, by the fold, naming its layer.
///
/// This is the consequence of the parser deliberately not validating a tier:
/// there is one refusal for a bad `--runtime`, it comes from the module that
/// owns ADR-0001 D1's three tiers, and it can say which layer offered it.
#[test]
fn a_tier_no_record_names_is_refused_once_and_the_refusal_names_its_layer() {
    let resolution = layers::resolve(
        &Overrides {
            tier: Some("sandboxed".to_owned()),
            model: None,
            mode: None,
        },
        [],
        &Files::none(),
    )
    .expect("an unknown tier is a well-formed text value and folds");

    match crate::runtime::ResolvedTier::from_configuration(&resolution) {
        Err(refusal @ crate::runtime::TierRefused::NoSuchTier { .. }) => {
            let said = refusal.to_string();
            assert!(
                said.contains("flag"),
                "the refusal must name the layer the value arrived in, so the reader knows what \
                 to change: {said}"
            );
            assert!(
                said.contains("bare") && said.contains("contained") && said.contains("linked"),
                "the refusal lists ADR-0001 D1's three tiers, walked from Tier::ALL: {said}"
            );
        }
        other => panic!("an unknown tier was not refused by the fold: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The keys ADR-0009 D1's own manifest sets, declared by the records that own
// them
// ---------------------------------------------------------------------------

/// Every key [ADR-0009] D1's worked manifest sets is one this binary declares.
///
/// **The population is the record's manifest, not a list retyped here**, so a
/// key added to that file is covered the moment it is added
/// ([Verification lessons] §17). It is asserted by *resolving* the manifest
/// through the binary's own schema rather than by asking the schema whether it
/// knows each name: ADR-0014 D5's refusal is what a user would actually meet,
/// and asking `Schema::field` would be a proxy for it.
///
/// Until 2026-09-05 this binary declared sixteen keys and not one of them was
/// `project.name`, `project.workspace` or `runtime.max_iterations` — so the
/// moment layer 3 gained a reader, a file in D1's own shape was refused as an
/// unknown key, and `zaru init` would have written a file the binary refused to
/// fold.
///
/// The mutant is dropping any one of the three declarations from
/// [`layers::schema`].
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_key_adr_0009_d1s_worked_manifest_sets_is_one_this_binary_declares() {
    use crate::config::{Contribution, Layer, Resolution, Source, Table, Value};

    // D1's corrected `[project]` and `[runtime]`, as the record now prints
    // them: no `tier`, and a ceiling that lowers.
    let mut project = Table::new();
    project.insert("name", Value::Text("acme-api".to_owned()));
    project.insert("workspace", Value::Text("acme-engineering".to_owned()));
    let mut runtime = Table::new();
    runtime.insert("max_iterations", Value::Integer(3));
    let mut document = Table::new();
    document.insert(crate::manifest::PROJECT_TABLE, Value::Table(project));
    document.insert(crate::manifest::RUNTIME_TABLE, Value::Table(runtime));

    let resolution = Resolution::resolve(
        &layers::schema(),
        vec![Contribution::new(
            Layer::Project,
            Source::named("./zaru.toml"),
            document,
        )],
    )
    .unwrap_or_else(|refusal| {
        panic!(
            "ADR-0009 D1's own worked manifest must fold through this binary's schema, or `zaru \
             init` writes a file `zaru config explain` refuses: {refusal}"
        )
    });

    for spelling in [
        crate::manifest::NAME_KEY,
        crate::manifest::WORKSPACE_KEY,
        crate::runtime::MAX_ITERATIONS_KEY,
    ] {
        let key = crate::config::Key::new(spelling).expect("a well-formed key");
        assert!(
            resolution.get(&key).is_some(),
            "`{spelling}` is set by ADR-0009 D1's manifest and resolved to nothing"
        );
        println!("{}", resolution.explain(&key));
    }
}

/// ADR-0014 D6's permitted direction, over a real record's key rather than a
/// fixture's.
///
/// **Both arms, and the permitted one is what makes the refusal mean
/// anything**: an implementation that refused everything the project layer
/// offers passes every D6 refusal check and fails this one. That was measured
/// on this fold when the hierarchy landed; what is new is that the key is
/// [ADR-0001]'s rather than a fixture's.
///
/// The mutant is declaring `runtime.max_iterations` `Free` instead of a
/// ceiling, which reddens on the raise.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[test]
fn a_project_may_lower_the_iteration_ceiling_and_may_not_raise_it() {
    use crate::config::{ConfigRefused, Contribution, Layer, Resolution, Source, Table, Value};

    let granted = |number: i64| {
        let mut runtime = Table::new();
        runtime.insert("max_iterations", Value::Integer(number));
        let mut document = Table::new();
        document.insert(crate::manifest::RUNTIME_TABLE, Value::Table(runtime));
        document
    };
    let fold = |asked: i64| {
        Resolution::resolve(
            &layers::schema(),
            vec![
                Contribution::new(
                    Layer::User,
                    Source::named("~/.zaru/config.toml"),
                    granted(5),
                ),
                Contribution::new(Layer::Project, Source::named("./zaru.toml"), granted(asked)),
            ],
        )
    };

    // D6: "A project may lower its own iteration ceiling."
    let lowered = fold(3).expect("a project lowering its own ceiling is what D6 permits");
    assert_eq!(
        lowered.get(&crate::runtime::max_iterations_key()),
        Some(&Value::Integer(3)),
        "the project's lower ceiling is the effective one"
    );

    // And never raise one.
    let refusal = fold(8).expect_err("a project raising a ceiling is what D6 forbids");
    let ConfigRefused::ProjectMayNotRaise {
        key,
        granted: was,
        asked,
    } = &refusal
    else {
        panic!("expected D6's ceiling refusal, got {refusal:?}");
    };
    assert_eq!(
        (key.as_str(), *was, *asked),
        (crate::runtime::MAX_ITERATIONS_KEY, 5, 8)
    );
    println!("{refusal}");
}
/// A store holding a provider key and no Notes token still answers.
///
/// **Found by running rather than by reading**, on 2026-09-05: `notes tokens`
/// asked whether the *store* was empty, and once the store could hold a
/// provider key a machine holding one and no Notes token printed nothing at
/// all and exited 0. A command that answers a question with silence is
/// indistinguishable from one that crashed quietly, and this is the shape a
/// filter added to a listing produces every time -- the emptiness test has to
/// move to the filtered set with the filter.
#[test]
fn notes_tokens_answers_over_a_store_holding_only_a_provider_key() {
    use crate::credentials::fixtures::{ScratchRoot, provider_secret_nonce};
    use crate::credentials::sealing::fixtures::StagedKey;
    use crate::credentials::{CredentialStore, Description, Entry, Listing, Secret};
    use crate::providers::ProviderKind;

    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("the store opens");
    let value = provider_secret_nonce();
    store
        .add(
            Entry::provider(
                ProviderKind::credential_alias(ProviderKind::Gemini),
                Description::new("the gemini API key").expect("a one-line description"),
                Secret::provider(ProviderKind::Gemini, value.clone()).expect("well-formed"),
            )
            .expect("a provider secret builds a provider entry"),
            &keys,
            None,
        )
        .expect("it is added");

    assert!(!store.is_empty(), "the store holds the provider key");
    assert!(
        store.listed(Listing::Notes).is_empty(),
        "the Notes listing is what must be empty"
    );

    let lines = crate::cli::render::tokens(&store);
    assert!(
        !lines.is_empty(),
        "`notes tokens` printed nothing at all over a store that is not empty"
    );
    assert!(
        lines[0].contains("no tokens"),
        "the listing must say there is no Notes token rather than going quiet: {lines:?}"
    );
    let rendered = lines.join("\n");
    assert!(!rendered.contains(&value));
    assert!(!rendered.contains(crate::credentials::fixtures::ascii_core(&value)));

    // The other listing does answer, which is the discriminating arm: the
    // silence above was about the filter, not about the store being unreadable.
    let keys_lines = crate::cli::render::provider_keys(&store);
    assert!(
        keys_lines
            .iter()
            .any(|line| line.contains("provider.gemini"))
    );
    let rendered = keys_lines.join("\n");
    assert!(!rendered.contains(&value), "the listing printed the key");
    assert!(!rendered.contains(crate::credentials::fixtures::ascii_core(&value)));
}

/// `providers keys` over an empty store says how to add one.
#[test]
fn provider_keys_over_an_empty_store_names_the_command_and_the_kinds() {
    use crate::credentials::CredentialStore;
    use crate::credentials::fixtures::ScratchRoot;
    use crate::providers::ProviderKind;

    let scratch = ScratchRoot::new();
    let store = CredentialStore::open(scratch.store_root()).expect("the store opens");
    let lines = crate::cli::render::provider_keys(&store).join("\n");

    assert!(lines.contains("no provider key is stored"));
    assert!(lines.contains("zaru providers keys add"));
    for kind in ProviderKind::ALL {
        assert!(
            lines.contains(kind.as_str()),
            "the kind `{kind}` is missing from the empty listing: {lines}"
        );
    }
}

// --- The four numbers this binary chooses for a turn -----------------------
//
// Each of the four is a value no record carries, so what a check can hold is
// not "the number is right" — nothing is available to compare it against.
// What it can hold is the property each number's own documentation claims,
// and those are genuinely different claims: a floor the mechanism sets, a pair
// the constructor validates, and an ordering between two numbers that answer
// different questions.

/// The tool-call ceiling clears the floor its own mechanism sets.
///
/// `TOOL_CALL_CEILING`'s documentation says the floor is two, because a turn
/// that calls a tool spends one exchange asking and a second answering. That
/// is a property of `zaru_core::tool_call::run` rather than of the number, and
/// it is what makes any ceiling of one unable to complete a tool-using turn.
///
/// The mutant: a ceiling of one, which is a value
/// `ToolCallCeiling::new` accepts — so the constructor cannot hold this and a
/// check has to. It printed *"a ceiling of 1 cannot both call a tool and
/// answer: ADR-0008 D1's cycle needs one exchange to ask and a second to
/// reply"*.
#[test]
fn the_tool_call_ceiling_clears_the_floor_the_mechanism_sets() {
    let ceiling = crate::cli::layers::tool_call_ceiling().get();
    assert!(
        ceiling >= 2,
        "a ceiling of {ceiling} cannot both call a tool and answer: ADR-0008 D1's cycle needs one \
         exchange to ask and a second to reply"
    );
    assert_eq!(ceiling, crate::cli::layers::TOOL_CALL_CEILING);
}

/// The three byte numbers answer two different questions, and the smaller one
/// is the one measured against a context window.
///
/// `OUTPUT_BUDGET_BYTES`' documentation is explicit that it is not the same
/// question as the two mebibyte ceilings: those bound what this harness reads
/// into memory, and this bounds what goes into a window. A budget at or above
/// them would mean one tool result could be the whole of what a model is read.
///
/// Watched red by setting the budget to `FILE_CEILING_BYTES`, which printed
/// *"one tool result may not be as large as the largest file this harness will
/// read whole: 1048576 against 1048576"*.
#[test]
fn what_a_model_is_shown_of_one_tool_result_is_smaller_than_what_the_harness_will_read() {
    let budget = crate::cli::layers::OUTPUT_BUDGET_BYTES as u64;
    for (name, ceiling) in [
        ("FILE_CEILING_BYTES", crate::cli::layers::FILE_CEILING_BYTES),
        (
            "SEARCH_CEILING_BYTES",
            crate::cli::layers::SEARCH_CEILING_BYTES,
        ),
    ] {
        assert!(
            budget < ceiling,
            "one tool result may not be as large as {name}, which is what this harness will read \
             whole: {budget} against {ceiling}"
        );
    }
}

/// The window and the threshold are a pair the constructor accepts, and the
/// threshold is genuinely below the window rather than equal to it.
///
/// `ContextLimits::new` refuses a threshold *above* a window and accepts one
/// equal to it — at which point compaction would fire only once the context
/// already did not fit, which is the warning arriving after the failure it
/// warns about. The constructor cannot hold that; this does.
///
/// Watched red by a threshold equal to the window, which printed *"a threshold
/// of 1048576 is not below the window of 1048576, so compaction would fire
/// only once the context already did not fit"*.
#[test]
fn the_pressure_threshold_is_below_the_window_rather_than_at_it() {
    let limits = crate::cli::layers::context_limits();
    let window = limits.window().get();
    let threshold = limits.threshold().get();
    assert!(
        threshold < window,
        "a threshold of {threshold} is not below the window of {window}, so compaction would fire \
         only once the context already did not fit"
    );
    assert_eq!(window, crate::cli::layers::CONTEXT_WINDOW_TOKENS);
    assert_eq!(threshold, crate::cli::layers::PRESSURE_THRESHOLD_TOKENS);
}

/// The process ceiling bounds a build rather than a request, so it is longer
/// than the provider's own exchange timeout.
///
/// `PROCESS_CEILING`'s documentation says the sixty seconds
/// `providers::gemini::EXCHANGE_TIMEOUT` uses is deliberately not reused,
/// because the two bound different things. Reusing it is the mutant, and it
/// printed *"a command is a build or a test suite and a request is not: 60s
/// against the provider's 60s"*.
#[test]
fn the_process_ceiling_is_longer_than_the_providers_exchange_timeout() {
    let ceiling = crate::cli::layers::process_ceiling().get();
    let exchange = crate::providers::gemini::EXCHANGE_TIMEOUT;
    assert!(
        ceiling > exchange,
        "a command is a build or a test suite and a request is not: {ceiling:?} against the \
         provider's {exchange:?}"
    );
    assert_eq!(ceiling, crate::cli::layers::PROCESS_CEILING);
}

// ---------------------------------------------------------------------------
// ADR-0013 D6 and clause 5 — the context segment's register
// ---------------------------------------------------------------------------

/// The abbreviation is ADR-0013 D3's, and it truncates rather than rounds.
///
/// **Both arms are the record's, not this module's.** D3 renders `18.2k` and
/// `2.1k`; the reading taken off those two examples is that a count at or
/// above a thousand carries one decimal place and truncates, "so the line
/// never reports more than was measured". The values below are chosen so that
/// rounding and truncating give different answers, which is the only way this
/// check can see the difference at all.
///
/// The mutant: rounding instead of truncating in `render::thousands`.
#[test]
fn the_context_segment_abbreviates_as_adr_0013_d3_does_and_never_rounds_up() {
    use zaru_core::context::Usage;

    assert_eq!(
        render::context_usage(Usage::new(12_390, 1_048_576)),
        "context 12.3k/1048.5k tokens",
        "12,390 truncates to 12.3k and 1,048,576 to 1048.5k; rounding would give 12.4k and \
         1048.6k, and D3's reading is that the line never reports more than was measured"
    );

    assert_eq!(
        render::context_usage(Usage::new(999, 1_048_576)),
        "context 999/1048.5k tokens",
        "below a thousand D3's abbreviation is the integer itself, so a session that has barely \
         started shows what it really holds rather than 0.9k"
    );
}

/// D6 needs both numbers, because approaching is a relation.
///
/// D6: "Approaching the threshold is not an event to announce — it is a number
/// that has been visible all along." A bare count of what is used cannot be
/// read as near or far, so the window it is measured against is on the row
/// too. Asserted as a property of the rendering rather than by comparing it
/// with itself: the two numbers are read out of the `Usage` the check built,
/// and both must appear.
///
/// The mutant: rendering `usage.used()` alone and dropping the window.
#[test]
fn the_context_segment_carries_the_window_and_not_only_what_is_used() {
    use zaru_core::context::Usage;

    let usage = Usage::new(300_000, 1_048_576);
    let rendered = render::context_usage(usage);

    assert!(
        rendered.contains(&render::thousands(usage.used())),
        "what is used must be on the row; it was {rendered:?}"
    );
    assert!(
        rendered.contains(&render::thousands(usage.window())),
        "ADR-0013 D6's 'approaching the threshold' is a relation, so the window must be on the \
         row beside what is used; it was {rendered:?}"
    );
}

/// The pressure threshold is deliberately absent, and this pins the decision.
///
/// Ruled 2026-09-05 under directive 20 and open to Jeshua's veto: `Usage` does
/// not carry the threshold, and a third number would be authored onto a row
/// two records already share. Pinned so that adding one is a decision somebody
/// makes rather than a line that drifts onto the row — the same discipline
/// `providers::usage`'s no-arithmetic check uses.
#[test]
fn the_pressure_threshold_is_not_on_the_status_row() {
    use zaru_core::context::Usage;

    let rendered = render::context_usage(Usage::new(
        300_000,
        crate::cli::layers::CONTEXT_WINDOW_TOKENS,
    ));
    let threshold = render::thousands(crate::cli::layers::PRESSURE_THRESHOLD_TOKENS);

    assert!(
        !rendered.contains(&threshold),
        "the threshold {threshold} is not on `Usage` and is not this row's third number; where \
         compaction begins is ADR-0013 D3's announcement, which says so as it happens. The row \
         was {rendered:?}"
    );
}

/// ADR-0007 D7's `add`, and D8's word.
///
/// The two lines differ by one word and produce two different reaches, which is
/// what D8's "instance-locked unless the user explicitly chooses otherwise"
/// means when there is no flag and no default to infer from.
#[test]
fn adr_0007_d8s_reach_is_the_word_the_user_typed_and_locked_when_they_typed_none() {
    let locked = accepted(&["notes", "tokens", "add", "work", "cortex.page"]);
    assert_eq!(
        locked.request,
        Request::NotesTokensAdd {
            alias: crate::credentials::Alias::new("work").expect("a usable alias"),
            host: "cortex.page".to_owned(),
            apex: false,
        }
    );

    let apex = accepted(&["notes", "tokens", "add", "work", "cortex.page", "apex"]);
    assert_eq!(
        apex.request,
        Request::NotesTokensAdd {
            alias: crate::credentials::Alias::new("work").expect("a usable alias"),
            host: "cortex.page".to_owned(),
            apex: true,
        }
    );

    // The listing is still one word and is not swallowed by the verb above.
    assert_eq!(accepted(&["notes", "tokens"]).request, Request::NotesTokens);
}

/// An alias the store would not hold is refused by the parser, not later.
#[test]
fn an_alias_the_store_would_not_hold_is_refused_before_anything_is_read() {
    let refusal = refuse(typed(&["notes", "tokens", "add", "", "cortex.page"]));
    assert_eq!(variant_of(&refusal), "UnusableAlias");
}

/// The measured scope reaches the entry, and reaches the two places that read
/// it back.
///
/// **This is the check the order in `notes_tokens_add` exists for.** A dropped
/// `with_tools` leaves the entry carrying `ToolScope::default()`, and then two
/// separate things lie: ADR-0007 D8's confirmation says the credential grants
/// nothing, and the description the agent reads under D2 says the same. Both
/// are asserted, because either alone would be satisfied by a version that set
/// the count in one place and not the other.
#[test]
fn adr_0007_d6s_measured_scope_reaches_the_entry_and_both_things_that_render_it() {
    let secret = crate::credentials::Secret::notes("nn_mcp_not-a-real-token")
        .expect("an nn_mcp_ value is a Nuclear Notes secret");
    let alias = crate::credentials::Alias::new("work").expect("a usable alias");
    let scope = crate::credentials::ToolScope::new(vec![
        "pages.read".to_owned(),
        "pages.list".to_owned(),
        "search.global".to_owned(),
    ]);

    let entry = crate::cli::run::notes_entry(&alias, "cortex.page", false, secret, scope)
        .expect("a well-formed entry");

    assert_eq!(
        entry.tools().map(crate::credentials::ToolScope::count),
        Some(3),
        "the scope the instance reported did not reach the entry, so D8's confirmation and D2's \
         description will both understate what this credential grants"
    );
    assert!(
        entry.description().as_str().contains("granting 3 tool(s)"),
        "the description the agent reads does not name the scope: {:?}",
        entry.description().as_str()
    );
    assert!(
        entry.description().as_str().contains("cortex.page"),
        "the description does not say which instance this credential is for: {:?}",
        entry.description().as_str()
    );
}

/// The sibling: the word `apex` changes the reach and nothing else.
#[test]
fn adr_0007_d8s_reach_follows_the_word_and_the_scope_is_carried_either_way() {
    let scope = crate::credentials::ToolScope::new(vec!["pages.read".to_owned()]);
    let locked = crate::cli::run::notes_entry(
        &crate::credentials::Alias::new("locked").expect("a usable alias"),
        "cortex.page",
        false,
        crate::credentials::Secret::notes("nn_mcp_one").expect("a Nuclear Notes secret"),
        scope.clone(),
    )
    .expect("a well-formed entry");
    let apex = crate::cli::run::notes_entry(
        &crate::credentials::Alias::new("apex").expect("a usable alias"),
        "cortex.page",
        true,
        crate::credentials::Secret::notes("nn_mcp_two").expect("a Nuclear Notes secret"),
        scope,
    )
    .expect("a well-formed entry");

    assert!(
        !locked
            .reach()
            .expect("a Nuclear Notes entry has a reach")
            .is_apex()
    );
    assert!(
        apex.reach()
            .expect("a Nuclear Notes entry has a reach")
            .is_apex()
    );
    assert_eq!(
        locked.tools().map(crate::credentials::ToolScope::count),
        apex.tools().map(crate::credentials::ToolScope::count),
        "the reach is not the scope and neither may move the other"
    );
}

/// A narrow spelling drops labelling and never a number.
///
/// # Why this is one check over both pairs
///
/// The two spellings of a segment are the one place this arc could have made
/// a row that lies at 40 columns and tells the truth at 200. The property is
/// the same for both pairs — every number in the full form is in the narrow
/// one — so it is asserted once, over both, against literals this check owns
/// rather than against the formatter's own output.
///
/// The token total is the load-bearing half: the full form is the very line
/// the session prints on exit, so a narrow form carrying a different number
/// would put two different totals in front of one user in one session.
///
/// The mutants: the narrow token spelling printing the completion count
/// instead of the total; the narrow context spelling dropping the window.
#[test]
fn a_narrow_spelling_drops_labelling_and_never_a_number() {
    use zaru_core::context::Usage;

    let spent = crate::providers::TokenUsage::counted(390, 79);
    let tokens = render::usage_row(&spent);
    assert_eq!(
        tokens.full, "tokens: 390 prompt + 79 completion = 469",
        "the full spelling is the line the session prints on exit"
    );
    assert_eq!(
        tokens.narrow, "469 tokens",
        "the narrow spelling keeps the total and drops the two parts of it"
    );
    assert!(
        tokens.full.ends_with(&tokens.narrow.replace(" tokens", "")),
        "both spellings must carry the same total; they were {:?} and {:?}",
        tokens.full,
        tokens.narrow
    );

    let context = render::context_row(Usage::new(12_390, 1_048_576));
    assert_eq!(
        context.full, "context 12.3k/1048.5k tokens",
        "the full spelling is ADR-0013 D3's own register"
    );
    assert_eq!(
        context.narrow, "12.3k/1048.5k",
        "the narrow spelling keeps both numbers, because D6's claim is that approaching is a \
         relation and a figure without its window cannot be read as near or far"
    );
    for number in ["12.3k", "1048.5k"] {
        assert!(
            context.narrow.contains(number),
            "the narrow context spelling must keep {number}; it was {:?}",
            context.narrow
        );
    }
}
