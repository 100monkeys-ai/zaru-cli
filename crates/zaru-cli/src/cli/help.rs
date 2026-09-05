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
fn summaries(namespace: Namespace) -> &'static [(&'static str, &'static str)] {
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
        Namespace::Notes => &[(
            "notes tokens",
            "print the stored Nuclear Notes tokens and which is the composer's",
        )],
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
        .flat_map(|namespace| summaries(namespace).iter().copied())
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
    lines.push(
        "This harness cannot run a task yet: no provider client is built, so there is nothing"
            .to_owned(),
    );
    lines.push(
        "for the agent loop to ask. What runs today is the list above, which reads what is"
            .to_owned(),
    );
    lines.push("already on this machine and changes none of it except `sessions rm`.".to_owned());

    lines
}
