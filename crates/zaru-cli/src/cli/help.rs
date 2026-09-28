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
//! It says it, in one paragraph, rather than staying silent. A user owed a
//! sentence about what the binary will not do for them is owed it here,
//! because otherwise they conclude the binary is broken. That is [ADR-0016]
//! D2's "where there genuinely is no action, say that", applied to a help
//! text.
//!
//! **That paragraph is walked too, and the comment at its site says why.** It
//! named capabilities by hand and went stale three times in ten days, each
//! time in a commit that never opened this file, so the half of it that is a
//! set is read from [`KINDS_WITH_A_CLIENT`](crate::compose::KINDS_WITH_A_CLIENT)
//! and only the half that is a property of the composition is still typed.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::flag::Flag;
use crate::cli::namespace::Namespace;
use crate::providers::ProviderKind;

/// The width the closing paragraph is wrapped to.
///
/// Sixty-eight columns, which is the width the paragraph was hand-wrapped to
/// for as long as it was hand-wrapped. The number is here rather than at the
/// call so that it is one decision rather than a constant folded into a
/// `format!`, and it is deliberately narrower than eighty: a help text read
/// through a pipe is usually read beside something else.
const PARAGRAPH_COLUMNS: usize = 68;

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
        // [ADR-0002](https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output)
        // D6's two. The summary says what the command is *for* rather than
        // what this build has to show, because the row describes the command
        // and the command's own answer describes the build.
        Namespace::Inbox => &[("inbox", "print the deposits waiting to be read")],
        Namespace::Learned => &[("learned", "print what this session wrote to craft memory")],
        Namespace::Validators => &[
            (
                "validators approve",
                "show the commands this project's zaru.toml declares and ask to approve them",
            ),
            (
                "validators list",
                "print every project whose validators are approved, and their commands",
            ),
        ],
        Namespace::Stack | Namespace::Memory => &[],
    }
}

/// What the closing paragraph says is missing, from the kinds with no client.
///
/// A function rather than two lines inside [`lines`] because the branch that
/// matters most is the one no build reaches today: **every kind having a
/// client is the state this sentence is written towards**, and a paragraph
/// that rendered "the provider kind(s) , which carry no client" on the day it
/// arrived would be the drift this walk exists to end, wearing a different
/// shape. Both branches are asserted rather than one being reasoned about.
///
/// The membrane half is in both branches and is a literal in both: it is true
/// of the composition rather than of a constant, `verdicts` being `NoMembrane`
/// at every tier, so there is no array to walk and nothing to keep in step.
pub(crate) fn cannot_do_yet(without_a_client: &[String]) -> String {
    if without_a_client.is_empty() {
        "a membrane at the contained and linked tiers, which enforce nothing yet".to_owned()
    } else {
        format!(
            "the provider kind(s) {}, which carry no client, and a membrane at the contained \
             and linked tiers, which enforce nothing yet",
            without_a_client.join(", ")
        )
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
        "usage: zaru [flags] [<command>]".to_owned(),
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

    // **The last sentence is walked, and this comment is the third thing to
    // stand here.** Its two predecessors were prose recording that the
    // sentence had drifted -- once when the `gemini` client landed and once
    // when the iteration loop and `--resume` did -- and the second of them
    // ended by asserting that "`KINDS_WITH_A_CLIENT` has one element", which
    // was itself the third drift and reached a reader as "any provider kind
    // but `gemini`" on a binary that had talked to three kinds since
    // 2026-09-14.
    //
    // The mechanism that comment named is right: **this sentence is about
    // capabilities other arcs land, so a hand-written one goes stale in a
    // commit that never opens this file.** What it concluded -- that no check
    // can hold it, because "what this binary cannot do" is a sentence about
    // absent code -- is what a walk makes unnecessary. The kinds with no
    // client are `ProviderKind::ALL` minus the kinds that have one, which is
    // a set the refusal path in `cli::classify` already reads, so the arc
    // that writes the fourth client changes this paragraph by changing that
    // array and nothing else.
    //
    // The membrane half stays a literal because it is true of the
    // composition rather than of a constant: `verdicts` is `NoMembrane` at
    // every tier, and there is no array to walk.
    let without_a_client: Vec<String> = ProviderKind::ALL
        .into_iter()
        .filter(|kind| !crate::compose::KINDS_WITH_A_CLIENT.contains(kind))
        .map(|kind| format!("`{}`", kind.as_str()))
        .collect();
    let cannot = cannot_do_yet(&without_a_client);
    let paragraph = format!(
        "`zaru \"<task>\"` runs a turn: it asks the model, runs the tools it asks for under the \
         permission model, and writes a transcript you can read with `cat`. A project that \
         declares validators in `./zaru.toml` runs the iteration loop instead, and `--resume` \
         and `--continue` reopen a session where each line you type is a turn. Store a key with \
         `providers keys add <kind>` first. What it cannot do yet: {cannot}."
    );
    // **Trimmed, and the trim is not tidiness.** `wrap::rows` breaks a row
    // *after* the spaces that ended its last word rather than consuming them,
    // because the pane it was written for needs concatenating the rows to
    // reproduce the text byte for byte. A pane never shows a trailing space
    // and a pipe does: `--help` is read by `diff` and by a release checklist,
    // and rows that end in whitespace are bytes a reader did not ask for. The
    // hand-wrapped literals this replaced carried none, so trimming is what
    // keeps the change to the sentence rather than to the file's bytes.
    lines.extend(
        zaru_tui::shell::wrap::rows(&paragraph, PARAGRAPH_COLUMNS)
            .into_iter()
            .map(|row| row.trim_end().to_owned()),
    );

    lines
}
