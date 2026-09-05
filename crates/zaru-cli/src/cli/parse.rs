// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The hand-written parser [ADR-0003] D2 decided on 2026-09-05.
//!
//! D2, as amended under directive 20: "**Layer 5 takes no crate.** ADR-0014
//! D1's command-line layer is read with `std::env::args` and a hand-written
//! parser, and no argument-parsing crate is taken... because the flag surface
//! is small, this table is closed on purpose, and a parser crate would be a
//! dependency arriving without a record of its own."
//!
//! # The arguments are a parameter, and that is the same seam layer 4 uses
//!
//! [`parse`] takes an iterator of [`OsString`] rather than reading the
//! process, exactly as [`crate::config::environment::read`] takes pairs rather
//! than reading the environment. The reason there was that `set_var` is
//! `unsafe` in this edition and the workspace denies `unsafe_code`; the reason
//! here is the same shape from the other side — a check that had to spawn a
//! process to exercise a grammar could not exercise a hostile one at all. The
//! product path is [`parse_process`], which passes `std::env::args_os`.
//!
//! # The grammar, whole
//!
//! ```text
//! zaru [flags] [<namespace> [<verb> [<argument>]]] [-- <task words>]
//! zaru [flags] <task words>
//! ```
//!
//! **Whether a line is a command or a task is decided once, by the parser, and
//! never decided again.** The rule, ruled 2026-09-05 as a delegated
//! coordinator decision open to Jeshua's veto: *a command is a word and a task
//! is a sentence.* One positional that is not a namespace and carries no
//! whitespace is a mistyped command, refused naming the nearest — [ADR-0014]
//! D5's precedent, through the same metric. Anything else that does not begin
//! with a namespace is task words. So `zaru runtim` is a typo and `zaru fix
//! the failing test` is a task, with no edit-distance threshold anywhere: D5
//! names none, and a threshold would be a number nobody decided.
//!
//! # What this module deliberately does not do
//!
//! It validates a [`Key`] and a [`SessionId`] because those types validate
//! themselves and the rest of the program should never see a string. It does
//! **not** validate a tier or a model identifier, because those are layer 5's
//! values and [`crate::runtime`] and [`crate::providers`] refuse them from
//! inside the fold, where the refusal can name the layer they arrived in. A
//! second refusal here would be a second answer to one question.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::cli::flag::Flag;
use crate::cli::invocation::{CommandLine, Overrides, Request};
use crate::cli::namespace::Namespace;
use crate::cli::refusal::CommandRefused;
use crate::config::Key;
use crate::providers::ProviderKind;
use crate::session::SessionId;
use std::ffi::OsString;

/// What the flag walk produced, before the positionals are read.
#[derive(Debug, Default)]
struct Flags {
    overrides: Overrides,
    resume: Option<SessionId>,
    r#continue: bool,
    help: bool,
    version: bool,
    seen: Vec<(Flag, Option<String>)>,
}

/// Parse this process's own arguments.
///
/// # Errors
///
/// [`CommandRefused`], for every shape the grammar does not admit.
pub fn parse_process() -> Result<CommandLine, CommandRefused> {
    parse(std::env::args_os().skip(1))
}

/// Parse a command line.
///
/// The iterator is the arguments **after** the program name, so a caller's
/// list is exactly the words a user typed.
///
/// # Errors
///
/// [`CommandRefused`], for every shape the grammar does not admit.
pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<CommandLine, CommandRefused> {
    let words = to_text(arguments)?;
    let (flags, positionals) = walk(words)?;

    // `--help` first, and before anything else can refuse. A user who cannot
    // remember the grammar is exactly the user most likely to type something
    // else wrong in the same line, and answering the refusal instead of the
    // question would be the least useful moment to be strict.
    if flags.help {
        return Ok(finish(flags, Request::Help));
    }
    if flags.version {
        return Ok(finish(flags, Request::Version));
    }

    if flags.resume.is_some() && flags.r#continue {
        return Err(CommandRefused::ResumeAndContinue);
    }
    if let Some(first) = positionals.first()
        && let Some(flag) = request_flag(&flags)
    {
        return Err(CommandRefused::RequestFlagWithCommand {
            flag: flag.spelling(),
            command: first.escape_debug().to_string(),
        });
    }
    if let Some(id) = flags.resume.clone() {
        return Ok(finish(flags, Request::Resume { id }));
    }
    if flags.r#continue {
        return Ok(finish(flags, Request::Continue));
    }

    let request = read_positionals(&positionals)?;
    Ok(finish(flags, request))
}

/// The request flag that was given, if one was.
fn request_flag(flags: &Flags) -> Option<Flag> {
    if flags.resume.is_some() {
        Some(Flag::Resume)
    } else if flags.r#continue {
        Some(Flag::Continue)
    } else {
        None
    }
}

/// Assemble the parsed line.
fn finish(flags: Flags, request: Request) -> CommandLine {
    CommandLine {
        overrides: flags.overrides,
        request,
    }
}

/// Every argument as text, or the first one that is not.
fn to_text(arguments: impl IntoIterator<Item = OsString>) -> Result<Vec<String>, CommandRefused> {
    arguments
        .into_iter()
        .map(|argument| {
            argument.clone().into_string().map_err(|_| {
                // `to_string_lossy` replaces the invalid bytes rather than
                // dropping them, so the reader sees where the problem is
                // rather than a word that looks fine and was refused.
                CommandRefused::NotText {
                    lossy: argument.to_string_lossy().into_owned(),
                }
            })
        })
        .collect()
}

/// Separate the flags from the positionals, left to right.
fn walk(words: Vec<String>) -> Result<(Flags, Vec<String>), CommandRefused> {
    let mut flags = Flags::default();
    let mut positionals = Vec::new();
    let mut flags_are_over = false;
    let mut words = words.into_iter();

    while let Some(word) = words.next() {
        if flags_are_over || !word.starts_with("--") {
            positionals.push(word);
            continue;
        }
        if word == "--" {
            flags_are_over = true;
            continue;
        }

        let (spelling, attached) = match word.split_once('=') {
            Some((spelling, value)) => (spelling.to_owned(), Some(value.to_owned())),
            None => (word.clone(), None),
        };

        let Some(flag) = Flag::named(&spelling) else {
            return Err(CommandRefused::UnknownFlag {
                offered: word.escape_debug().to_string(),
                nearest: nearest_flag(&spelling),
            });
        };

        let value = match (flag.value_name(), attached) {
            (None, Some(_)) => {
                return Err(CommandRefused::FlagTakesNoValue {
                    flag: flag.spelling(),
                });
            }
            (None, None) => None,
            (Some(_), Some(value)) => Some(value),
            (Some(value_name), None) => {
                Some(words.next().ok_or(CommandRefused::FlagNeedsValue {
                    flag: flag.spelling(),
                    value: value_name,
                })?)
            }
        };

        if let Some((_, first)) = flags.seen.iter().find(|(seen, _)| *seen == flag) {
            return Err(CommandRefused::FlagRepeated {
                flag: flag.spelling(),
                first: first.clone(),
                second: value,
            });
        }
        flags.seen.push((flag, value.clone()));

        match flag {
            Flag::Runtime => flags.overrides.tier = value,
            Flag::Model => flags.overrides.model = value,
            Flag::Mode => flags.overrides.mode = value,
            Flag::Resume => {
                let text = value.unwrap_or_default();
                flags.resume =
                    Some(SessionId::parse(&text).map_err(CommandRefused::UnusableSessionId)?);
            }
            Flag::Continue => flags.r#continue = true,
            Flag::Help => flags.help = true,
            Flag::Version => flags.version = true,
        }
    }

    Ok((flags, positionals))
}

/// The flag nearest to one nothing declares.
fn nearest_flag(offered: &str) -> &'static str {
    crate::config::nearest::nearest(Flag::ALL.into_iter().map(Flag::spelling), offered)
        .expect("Flag::ALL is never empty")
}

/// The subcommand nearest to a word that names none.
///
/// The candidates are the namespaces this harness **implements**, because a
/// suggestion the reader cannot act on is the thing [ADR-0016] D2 calls a
/// stack trace with better grammar.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn nearest_subcommand(offered: &str) -> &'static str {
    crate::config::nearest::nearest(
        Namespace::ALL
            .into_iter()
            .filter(|namespace| namespace.is_built())
            .map(Namespace::subcommand),
        offered,
    )
    .expect("at least one namespace is built")
}

/// Read the words that are not flags.
fn read_positionals(positionals: &[String]) -> Result<Request, CommandRefused> {
    let Some(first) = positionals.first() else {
        // `zaru` with no arguments at all. Printing the help is the answer to
        // the question the user asked by typing the name of a program and
        // nothing else.
        return Ok(Request::Help);
    };

    let Some(namespace) = Namespace::from_subcommand(first) else {
        return task_or_typo(positionals);
    };
    if !namespace.is_built() {
        return Err(CommandRefused::NamespaceNotBuilt { namespace });
    }

    let rest = &positionals[1..];
    // Exhaustive over the namespace with no wildcard arm, so a tenth cannot
    // arrive without its out-of-session grammar being written here.
    match namespace {
        Namespace::Runtime => whole(namespace, rest, Request::Runtime),
        Namespace::Models => whole(namespace, rest, Request::Models),
        Namespace::Init => whole(namespace, rest, Request::Init),
        Namespace::Config => match verb(namespace, rest)? {
            ("explain", argument, command) => {
                let key = argument.ok_or(CommandRefused::ArgumentMissing {
                    command,
                    argument: "a configuration key",
                })?;
                Ok(Request::ConfigExplain {
                    key: Key::new(key).map_err(CommandRefused::UnusableKey)?,
                })
            }
            (other, _, _) => unreachable!("`{other}` is not one of Namespace::Config's verbs"),
        },
        Namespace::Session => match verb(namespace, rest)? {
            ("list", None, _) => Ok(Request::SessionsList),
            ("list", Some(extra), command) => Err(CommandRefused::UnexpectedWord {
                command,
                offered: extra.escape_debug().to_string(),
            }),
            ("rm", argument, command) => {
                let id = argument.ok_or(CommandRefused::ArgumentMissing {
                    command,
                    argument: "a session id",
                })?;
                Ok(Request::SessionsRemove {
                    id: SessionId::parse(id).map_err(CommandRefused::UnusableSessionId)?,
                })
            }
            (other, _, _) => unreachable!("`{other}` is not one of Namespace::Session's verbs"),
        },
        Namespace::Notes => match verb(namespace, rest)? {
            ("tokens", None, _) => Ok(Request::NotesTokens),
            ("tokens", Some(extra), command) => Err(CommandRefused::UnexpectedWord {
                command,
                offered: extra.escape_debug().to_string(),
            }),
            (other, _, _) => unreachable!("`{other}` is not one of Namespace::Notes's verbs"),
        },
        // The one namespace whose grammar is two words deep, so it does not
        // go through `verb`. See `Namespace::verbs` for why the nesting is
        // what it is.
        Namespace::Providers => match rest {
            [] => Err(CommandRefused::VerbMissing { namespace }),
            [keys] if keys == KEYS => Ok(Request::ProviderKeys),
            [keys, add] if keys == KEYS && add == ADD => Err(CommandRefused::ArgumentMissing {
                command: format!("{namespace} {KEYS} {ADD}"),
                argument: "a provider kind",
            }),
            [keys, add, kind] if keys == KEYS && add == ADD => Ok(Request::ProviderKeysAdd {
                kind: ProviderKind::parse(kind).ok_or_else(|| CommandRefused::UnknownVerb {
                    namespace,
                    offered: kind.escape_debug().to_string(),
                    nearest: crate::config::nearest::nearest(
                        ProviderKind::ALL.iter().map(|kind| kind.as_str()),
                        kind,
                    ),
                })?,
            }),
            [keys, extra] if keys == KEYS => Err(CommandRefused::UnknownVerb {
                namespace,
                offered: extra.escape_debug().to_string(),
                nearest: crate::config::nearest::nearest([ADD], extra),
            }),
            [keys, add, _, extra, ..] if keys == KEYS && add == ADD => {
                Err(CommandRefused::UnexpectedWord {
                    command: format!("{namespace} {KEYS} {ADD}"),
                    offered: extra.escape_debug().to_string(),
                })
            }
            [other, ..] => Err(CommandRefused::UnknownVerb {
                namespace,
                offered: other.escape_debug().to_string(),
                nearest: crate::config::nearest::nearest(namespace.verbs().iter().copied(), other),
            }),
        },
        Namespace::Stack | Namespace::Memory | Namespace::Learned | Namespace::Inbox => {
            Err(CommandRefused::NamespaceNotBuilt { namespace })
        }
    }
}

/// A namespace whose subcommand is a whole command on its own.
fn whole(
    namespace: Namespace,
    rest: &[String],
    request: Request,
) -> Result<Request, CommandRefused> {
    match rest.first() {
        None => Ok(request),
        Some(extra) => Err(CommandRefused::UnexpectedWord {
            command: namespace.subcommand().to_owned(),
            offered: extra.escape_debug().to_string(),
        }),
    }
}

/// The verb, its one optional argument, and how `--help` spells the command.
///
/// The verb is checked against [`Namespace::verbs`] here and nowhere else, so
/// the list `--help` prints and the list the parser accepts are one list.
fn verb(
    namespace: Namespace,
    rest: &[String],
) -> Result<(&str, Option<&String>, String), CommandRefused> {
    let Some(offered) = rest.first() else {
        return Err(CommandRefused::VerbMissing { namespace });
    };
    if !namespace.verbs().contains(&offered.as_str()) {
        return Err(CommandRefused::UnknownVerb {
            namespace,
            offered: offered.escape_debug().to_string(),
            nearest: crate::config::nearest::nearest(namespace.verbs().iter().copied(), offered),
        });
    }

    let command = format!("{namespace} {offered}");
    if let Some(extra) = rest.get(2) {
        return Err(CommandRefused::UnexpectedWord {
            command,
            offered: extra.escape_debug().to_string(),
        });
    }
    Ok((offered.as_str(), rest.get(1), command))
}

/// A line that begins with no namespace: task words, or one mistyped word.
///
/// **A command is a word and a task is a sentence.** One positional carrying
/// no whitespace is a command somebody meant to type, so it is refused naming
/// the nearest one; anything else is what the user wants done. The rule needs
/// no edit-distance threshold, which matters because [ADR-0014] D5 names none
/// and a threshold here would be a number nobody decided.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn task_or_typo(positionals: &[String]) -> Result<Request, CommandRefused> {
    if let [only] = positionals
        && !only.chars().any(char::is_whitespace)
    {
        return Err(CommandRefused::UnknownCommand {
            offered: only.escape_debug().to_string(),
            nearest: nearest_subcommand(only),
        });
    }
    Ok(Request::Task {
        words: positionals.to_vec(),
    })
}

/// The verb `providers` takes.
const KEYS: &str = "keys";

/// The verb `providers keys` takes.
const ADD: &str = "add";
