// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D2's slash commands: a second grammar over one vocabulary.
//!
//! # Two entry points, two grammars, one table
//!
//! D2, settled 2026-09-05 under directive 20: "**A namespace has two entry
//! points, and they are one operation.** `/session <verb>` inside a session
//! and `zaru sessions <verb>` outside one are the same commands reached from
//! the two places a user can be, so this table governs both spellings rather
//! than only the slash one."
//!
//! Two places means two grammars. Outside a session the words arrive as
//! `OsString`s from the process and the first of them is a bare subcommand;
//! inside one they arrive as a line the user typed and a command is
//! distinguished from a task by a leading `/`. Those are different parsing
//! problems with different refusals, and `zaru-cli`'s parser answers a
//! question this one never asks — what to do with `--`, with `=`, with a flag
//! given twice.
//!
//! What is **not** duplicated is the table. Every namespace, every spelling,
//! whether this build implements it, and [ADR-0014] D5's nearest match arrive
//! through [`CommandVocabulary`], which `zaru-cli` implements over its own
//! closed `Namespace` enum. This module holds no list of commands at all.
//!
//! # `/exit` is the shell's own word and belongs to no namespace
//!
//! No record names a way to leave. D2's table has ten rows and none of them is
//! leaving, so `/exit` is **drafted under a delegated coordinator ruling of
//! 2026-09-05, open to Jeshua's veto**, together with `Ctrl-C`. Both are
//! recorded rather than one chosen, because a terminal user reaches for
//! `Ctrl-C` before reading anything and a user who has read the hint strip
//! reaches for the word.
//!
//! D2's shadowing rule applies to it exactly as it applies to a user command:
//! "A user command may not shadow a built-in namespace. Shadowing produces
//! behaviour that depends on load order, which is unexplainable at the moment
//! it matters." `/exit` names no namespace, and
//! `the_shells_own_leave_word_shadows_no_namespace` asserts it against
//! whatever the vocabulary carries rather than against a list written here —
//! so an eleventh namespace called `exit` is caught by a check rather than by
//! somebody remembering.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::shell::port::CommandVocabulary;
use core::fmt;

/// The word that leaves a session, including its slash.
///
/// Drafted; see the module documentation.
pub const LEAVE: &str = "/exit";

/// What a typed line turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    /// Nothing: the line was empty or only whitespace.
    Nothing,
    /// A task for the harness to do. Anything with no leading `/`.
    Task(String),
    /// The user is leaving.
    Leave,
    /// One of [ADR-0015] D2's namespaces, with what followed it.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Command(Command),
    /// A line beginning with `/` that named no command.
    Refused(Refused),
}

/// A slash command this harness can act on.
///
/// The words are carried rather than interpreted. What `/config explain <key>`
/// does with its third word is the same question `zaru config explain <key>`
/// answers, and that answer lives in `zaru-cli` in one place; a grammar that
/// validated a key here would be a second answer to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The namespace's slash spelling, as the vocabulary gave it.
    pub slash: &'static str,
    /// The verb, for a namespace that takes one.
    pub verb: Option<&'static str>,
    /// Everything after the verb, split on whitespace.
    pub words: Vec<String>,
}

/// Why a slash line was not accepted.
///
/// Every variant names something the user can change, which is [ADR-0016] D1
/// row 2 — "the user's... Says exactly what to change" — and the same reading
/// `zaru-cli`'s own `CommandRefused` takes of a typed command line.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// A bare `/` with nothing after it.
    Empty,
    /// A word that names no namespace.
    UnknownCommand {
        /// What was typed.
        offered: String,
        /// The nearest slash spelling this harness knows.
        nearest: Option<&'static str>,
    },
    /// A namespace [ADR-0015] D2 names and this build does not implement.
    ///
    /// **Never placed against a nearest.** See the port's documentation.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    NotBuilt {
        /// The namespace's slash spelling.
        slash: &'static str,
        /// What D2's second column says it governs.
        governs: &'static str,
    },
    /// A namespace that needs a verb and was given none.
    VerbMissing {
        /// The namespace's slash spelling.
        slash: &'static str,
        /// The verbs it takes.
        verbs: &'static [&'static str],
    },
    /// A verb this namespace does not take.
    UnknownVerb {
        /// The namespace's slash spelling.
        slash: &'static str,
        /// What was typed.
        offered: String,
        /// The nearest verb it takes, absent when it takes none.
        nearest: Option<&'static str>,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("`/` on its own names no command"),
            Self::UnknownCommand { offered, nearest } => match nearest {
                Some(nearest) => {
                    write!(
                        f,
                        "there is no `/{offered}` command; the nearest is `{nearest}`"
                    )
                }
                None => write!(f, "there is no `/{offered}` command"),
            },
            Self::NotBuilt { slash, governs } => write!(
                f,
                "`{slash}` is one of ADR-0015 D2's namespaces, governing {governs}, and this \
                 harness does not implement it yet"
            ),
            Self::VerbMissing { slash, verbs } => write!(
                f,
                "`{slash}` is a namespace rather than a command, and it was given no verb; it \
                 takes {}",
                list(verbs)
            ),
            Self::UnknownVerb {
                slash,
                offered,
                nearest,
            } => match nearest {
                Some(nearest) => write!(
                    f,
                    "`{slash}` has no `{offered}` verb; the nearest is `{nearest}`"
                ),
                None => write!(f, "`{slash}` has no `{offered}` verb"),
            },
        }
    }
}

/// The verbs a namespace takes, as a prose list.
fn list(verbs: &[&str]) -> String {
    match verbs {
        [] => "none".to_owned(),
        [only] => format!("`{only}`"),
        [rest @ .., last] => {
            let head: Vec<String> = rest.iter().map(|verb| format!("`{verb}`")).collect();
            format!("{} and `{last}`", head.join(", "))
        }
    }
}

/// Read one typed line against the vocabulary.
///
/// # A command is a word and a task is a sentence, one surface over
///
/// [ADR-0015] D2's flag-surface contract settles the out-of-session half of
/// this: "A command is a word and a task is a sentence." Inside a session the
/// distinction is cheaper and sharper, because the user has a character to
/// spend on it: a leading `/` says command and everything else is the task.
/// **So there is no edit-distance threshold here either**, and for D2's own
/// reason — it names none, and a threshold would be a number nobody decided.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[must_use]
pub fn read(line: &str, vocabulary: &dyn CommandVocabulary) -> Typed {
    let line = line.trim();
    if line.is_empty() {
        return Typed::Nothing;
    }
    let Some(rest) = line.strip_prefix('/') else {
        return Typed::Task(line.to_owned());
    };

    let mut words = rest.split_whitespace();
    let Some(head) = words.next() else {
        return Typed::Refused(Refused::Empty);
    };
    let spelled = format!("/{head}");
    if spelled == LEAVE {
        return Typed::Leave;
    }

    let Some(namespace) = vocabulary
        .namespaces()
        .into_iter()
        .find(|namespace| namespace.slash == spelled)
    else {
        return Typed::Refused(Refused::UnknownCommand {
            offered: head.to_owned(),
            nearest: vocabulary.nearest(head),
        });
    };

    if !namespace.built {
        return Typed::Refused(Refused::NotBuilt {
            slash: namespace.slash,
            governs: namespace.governs,
        });
    }

    // A namespace that takes no verb is a whole command on its own -- `/runtime`
    // and `/models` are, exactly as `zaru runtime` and `zaru models` are. That
    // is a different thing from a namespace whose verbs nothing matched.
    if namespace.verbs.is_empty() {
        return Typed::Command(Command {
            slash: namespace.slash,
            verb: None,
            words: words.map(str::to_owned).collect(),
        });
    }

    let Some(offered) = words.next() else {
        return Typed::Refused(Refused::VerbMissing {
            slash: namespace.slash,
            verbs: namespace.verbs,
        });
    };
    let Some(verb) = namespace
        .verbs
        .iter()
        .copied()
        .find(|verb| *verb == offered)
    else {
        return Typed::Refused(Refused::UnknownVerb {
            slash: namespace.slash,
            offered: offered.to_owned(),
            nearest: vocabulary.nearest_verb(namespace.slash, offered),
        });
    };

    Typed::Command(Command {
        slash: namespace.slash,
        verb: Some(verb),
        words: words.map(str::to_owned).collect(),
    })
}
