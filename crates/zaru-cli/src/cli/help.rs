// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What `--help` prints, derived from the grammar rather than typed beside it.
//!
//! # It lists only what runs
//!
//! [ADR-0003] D2's acceptance made "what `--help` prints" [ADR-0015]'s to
//! decide. The decision, ruled 2026-09-05 and open to Jeshua's veto: **help
//! lists exactly the commands and flags this binary implements, and nothing
//! it does not.** A help text naming `zaru init` or `/stack install` would be
//! a promise the binary refuses to keep at the moment the user acts on it,
//! which is a worse failure than not knowing the command exists — and it is
//! the specific failure [ADR-0016] D2 calls "an error message whose reader
//! cannot act".
//!
//! So the command list is **walked** from [`Namespace::ALL`] filtered by
//! [`Namespace::is_built`], and the flag list from [`Flag::ALL`]. Nothing here
//! is a literal list of commands, so a namespace that becomes built appears
//! in the help without anybody remembering to add it, and one that is not
//! cannot appear at all.
//!
//! # What it says about what is missing
//!
//! It says it, in one paragraph, rather than staying silent. A user who runs
//! `zaru` and finds five inspection commands is owed the sentence explaining
//! that the harness cannot yet run a task, because otherwise they will
//! conclude the binary is broken. That is [ADR-0016] D2's "where there
//! genuinely is no action, say that", applied to a help text.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::flag::Flag;
use crate::cli::namespace::Namespace;

/// What each built command does, in the words its own record uses.
///
/// One arm per built namespace, wildcard-free, so a namespace that becomes
/// built has to be given a line here.
///
/// Public so a check can read one namespace's summary without re-deriving it
/// from the whole printed help, which is what
/// `the_help_row_says_what_the_flag_beside_it_says` needs.
#[must_use]
pub fn summaries_of(namespace: Namespace) -> &'static [(&'static str, &'static str)] {
    match namespace {
        Namespace::Runtime => &[("runtime", "print the tier and what changing it would alter")],
        Namespace::Models => &[(
            "models",
            "print each model alias, what it resolved to, and where",
        )],
        Namespace::Config => &[(
            "config explain <key>",
            "print every layer's value for one key, with the effective one marked",
        )],
        Namespace::Session => &[
            ("sessions list", "print every session on this machine"),
            ("sessions rm <id>", "delete a session's directory"),
        ],
        Namespace::Notes => &[
            (
                "notes tokens",
                "print the stored Nuclear Notes tokens and which is the composer's",
            ),
            (
                "notes tokens add <alias> <host>",
                "store a Nuclear Notes token, read from standard input; add the word `apex` \
                 after the host for a credential with no instance boundary",
            ),
            (
                "notes tokens describe <alias> <text>",
                "set what that token is for; the words after the alias are the description",
            ),
            (
                "notes tokens rm <alias>",
                "remove that token and its stored value",
            ),
            (
                "notes use <alias>",
                "move the composer role to that token, so the hint strip searches with it",
            ),
        ],
        Namespace::Init => &[(
            "init",
            // The manifest is ADR-0009 D1's.
            "write a project manifest into this directory, once, if there is none",
        )],
        Namespace::Providers => &[
            (
                "providers keys",
                "print which providers this machine holds a key for",
            ),
            (
                "providers keys add <kind>",
                "store a provider's API key, read from standard input",
            ),
            (
                "providers keys rm <kind>",
                "remove that provider's key and its stored value",
            ),
        ],
        // **The summary is `Flag::Help`'s own words rather than a second
        // description of one thing.** The row and the flag print the same
        // lines, so a summary written afresh here would be two sentences about
        // one command, which is the drift D2's one-table rule exists to
        // prevent; `the_help_row_says_what_the_flag_beside_it_says` holds them
        // equal rather than leaving it to whoever edits one of them next.
        Namespace::Help => &[("help", "print this")],
        Namespace::Stack | Namespace::Memory | Namespace::Learned | Namespace::Inbox => &[],
    }
}

/// The whole help text, as lines.
///
/// The version is the first line so that the artefact's own identity is
/// readable from a bare `zaru` — which is what `tests/version.rs` asserts, and
/// it asserts it for the release checklist's reason rather than for this one.
#[must_use]
pub fn lines(version: &str) -> Vec<String> {
    let mut lines = vec![
        format!("zaru {version}"),
        String::new(),
        "usage:  zaru [flags] [<command>]".to_owned(),
        String::new(),
        "commands:".to_owned(),
    ];

    let commands: Vec<(&str, &str)> = Namespace::ALL
        .into_iter()
        .filter(|namespace| namespace.is_built())
        .flat_map(|namespace| summaries_of(namespace).iter().copied())
        .collect();
    let width = commands
        .iter()
        .map(|(spelling, _)| spelling.chars().count())
        .max()
        .unwrap_or(0);
    for (spelling, summary) in commands {
        lines.push(format!("  {spelling:width$}  {summary}"));
    }

    lines.push(String::new());
    lines.push("flags:".to_owned());
    let spellings: Vec<String> = Flag::ALL
        .into_iter()
        .map(|flag| match flag.value_name() {
            Some(value) => format!("{} {value}", flag.spelling()),
            None => flag.spelling().to_owned(),
        })
        .collect();
    let width = spellings
        .iter()
        .map(|spelling| spelling.chars().count())
        .max()
        .unwrap_or(0);
    for (flag, spelling) in Flag::ALL.into_iter().zip(spellings) {
        lines.push(format!("  {spelling:width$}  {}", flag.summary()));
    }

    lines.push(String::new());
    // Rewritten twice on 2026-09-05, both times because it overstated what was
    // missing. It said no provider client was built until the `gemini` client
    // landed; it then said nothing connected a client to the agent loop, which
    // stopped being true the moment `zaru <task>` ran a turn — and it went on
    // saying it, in the one place a user reads to find out what this binary
    // does, through every arc since. A help text that overstates what is
    // missing is as wrong as one that overstates what works, and it is the
    // more expensive way round: nobody tries the thing it disowns.
    //
    // Corrected on sight by the `mode-key` arc, which needed this paragraph to
    // be true in order to add a line to the list above it.
    //
    // **It drifted a second time and is corrected again on 2026-09-05.** It
    // read "cannot do yet: any provider kind but `gemini`, the iteration loop,
    // or a conversation longer than one turn", and two of those three had
    // stopped being true: `iteration-wiring` landed the iteration loop, so a
    // project declaring validators in `./zaru.toml` runs it, and
    // `shell-task-turns` landed `--resume` and `--continue`, so a session is a
    // conversation whose next turn remembers the last. Twice in two days is
    // the shape rather than the accident: **this list names capabilities other
    // arcs land, so it goes stale in a commit that never touches this file**,
    // and no check can hold it because "what this binary cannot do" is a
    // sentence about absent code. What is left below is verified rather than
    // remembered -- `KINDS_WITH_A_CLIENT` has one element, and the
    // composition's `verdicts` is `NoMembrane` at every tier.
    lines.push("`zaru \"<task>\"` runs a turn: it asks the model, runs the tools it".to_owned());
    lines.push(
        // The permission model is ADR-0011's.
        "asks for under the permission model, and writes a transcript you can".to_owned(),
    );
    lines.push(
        "read with `cat`. A project that declares validators in `./zaru.toml` runs".to_owned(),
    );
    lines.push(
        "the iteration loop instead, and `--resume` and `--continue` reopen a session".to_owned(),
    );
    lines.push(
        "where each line you type is a turn. Store a key with `providers keys add".to_owned(),
    );
    lines.push(
        "<kind>` first. What it cannot do yet: any provider kind but `gemini`, and a".to_owned(),
    );
    lines.push("membrane at the contained and linked tiers, which enforce nothing yet.".to_owned());

    lines
}
