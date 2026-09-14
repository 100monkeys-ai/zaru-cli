// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Why a command line was not accepted.
//!
//! # Every variant is the user's, and every one names what to change
//!
//! [ADR-0016] D1 row 2 is "the user's... Says exactly what to change", and a
//! command line is the one input in this harness that is unambiguously
//! something the person in front of it typed. So the whole of this enum maps
//! to `UserCorrectable`, and every variant's
//! `Display` is written to be the *statement*, with the remedy built beside it
//! from the same fields rather than from a second reading of the same failure.
//!
//! # Nothing here quotes a value it was not given
//!
//! A refusal quotes the word the user typed, escaped, and the vocabulary it
//! was placed against. It never quotes a flag's *value*: `--model` and
//! `--runtime` carry text destined for a configuration layer, and
//! [ADR-0014] D4's whole argument is that a configuration value can be a
//! bearer token somebody pasted. The one exception is a value the parser
//! itself refused for its shape — a key or a session id — which is quoted by
//! the refusal that owns it ([`KeyRefused`], [`SessionIdRefused`]) and which
//! carries no value at all in the session id's case.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::namespace::Namespace;
use crate::config::KeyRefused;
use crate::credentials::AliasRefused;
use crate::session::SessionIdRefused;
use core::fmt;

/// A command line this harness could not accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRefused {
    /// An argument that is not valid text.
    ///
    /// `args_os` yields whatever the operating system handed the process, and
    /// on Unix that is bytes rather than text. Refusing beats an unwrap: a
    /// panic here would be reported as a defect in Zaru by [ADR-0016] D3's
    /// boundary, when what happened is that somebody's shell expanded a
    /// filename with an invalid byte in it.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    NotText {
        /// The argument as far as it can be rendered, with the invalid bytes
        /// replaced.
        lossy: String,
    },
    /// A single word that names no subcommand.
    UnknownCommand {
        /// What was typed, escaped.
        offered: String,
        /// The nearest subcommand this harness implements.
        nearest: &'static str,
    },
    /// A subcommand [ADR-0015] D2 names and this harness does not implement.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    NamespaceNotBuilt {
        /// Which namespace.
        namespace: Namespace,
    },
    /// A subcommand that needs a verb and was given none.
    VerbMissing {
        /// Which namespace.
        namespace: Namespace,
    },
    /// A verb the command it was offered under does not take.
    ///
    /// # It carries the command rather than the namespace, and that is a fix
    ///
    /// Until 2026-09-14 this carried a [`Namespace`], and both the statement
    /// and the remedy were composed from it alone. That is right for a
    /// grammar one word deep and **false for the two that are deeper**:
    /// `notes tokens <extra>` and `providers keys <extra>` raise this refusal
    /// with a word that was offered under `notes tokens` and `providers
    /// keys`, and the namespace is the wrong half of the spelling to name.
    ///
    /// Measured from the release binary at `a8539ac`, both halves wrong at
    /// once: `zaru notes tokens rm x` answered *"`zaru notes` has no `rm`
    /// verb"* with the remedy *"run `zaru notes add`"* — and **`zaru notes
    /// add` is not a command**, the real one being `zaru notes tokens add`.
    /// `zaru providers keys rm gemini` answered *"`zaru providers` has no
    /// `keys` verb"*, naming as unknown the one word in the line that **is** a
    /// verb. [ADR-0016] D2's own bar is that a remedy names something the
    /// binary runs; a remedy naming a command that does not exist is the
    /// failure that record calls "an error message whose reader cannot act".
    ///
    /// So this carries `command`, spelled as `--help` spells it, exactly as
    /// [`CommandRefused::UnexpectedWord`] and
    /// [`CommandRefused::ArgumentMissing`] beside it already do. One field
    /// rather than a namespace plus a depth means the statement and the
    /// remedy cannot come to disagree about which command was being typed.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    UnknownVerb {
        /// The command the word was offered under, as `--help` spells it.
        command: String,
        /// What was typed, escaped.
        offered: String,
        /// The nearest verb that command takes, absent when it takes none.
        nearest: Option<&'static str>,
    },
    /// A command that takes no further word was given one.
    UnexpectedWord {
        /// The command, as `--help` spells it.
        command: String,
        /// What was typed, escaped.
        offered: String,
    },
    /// A command whose argument is required was given none.
    ArgumentMissing {
        /// The command, as `--help` spells it.
        command: String,
        /// What the argument is called.
        argument: &'static str,
    },
    /// A flag this harness does not take.
    UnknownFlag {
        /// What was typed, escaped.
        offered: String,
        /// The nearest flag this harness takes.
        nearest: &'static str,
    },
    /// A flag that takes a value and was given none.
    FlagNeedsValue {
        /// The flag.
        flag: &'static str,
        /// What the value is called.
        value: &'static str,
    },
    /// A flag that takes no value was given one with `=`.
    FlagTakesNoValue {
        /// The flag.
        flag: &'static str,
    },
    /// The same flag twice.
    ///
    /// **Refused rather than resolved by taking the last.** [ADR-0014] D1
    /// gives one precedence order across five layers and says nothing about a
    /// precedence order *within* layer 5; last-wins would be a sixth rule
    /// nobody wrote, and it is the rule under which a user who typed a flag
    /// twice by accident silently gets one of them. Refusing costs one
    /// retype. Ruled 2026-09-05, delegated, open to Jeshua's veto.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    FlagRepeated {
        /// The flag.
        flag: &'static str,
        /// The first value, or `None` for a flag that takes none.
        first: Option<String>,
        /// The second.
        second: Option<String>,
    },
    /// `--resume` and `--continue` together.
    ResumeAndContinue,
    /// A flag that is itself a request, beside a subcommand.
    RequestFlagWithCommand {
        /// The flag.
        flag: &'static str,
        /// The subcommand.
        command: String,
    },
    /// A key `config explain` was given that is not a configuration key.
    UnusableKey(KeyRefused),
    /// A session id that is not a ULID.
    UnusableSessionId(SessionIdRefused),
    /// An alias that is not a name this store will hold.
    UnusableAlias(AliasRefused),
}

impl fmt::Display for CommandRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotText { lossy } => write!(
                f,
                "the argument {lossy:?} is not valid text, and every word `zaru` takes is text"
            ),
            Self::UnknownCommand { offered, .. } => {
                write!(f, "there is no `zaru {offered}` command")
            }
            Self::NamespaceNotBuilt { namespace } => write!(
                f,
                "`{namespace}` is one of this harness's namespaces, governing {}, and it \
                 does not implement it yet",
                namespace.governs()
            ),
            Self::VerbMissing { namespace } => write!(
                f,
                "`zaru {namespace}` is a namespace rather than a command, and it was given no verb"
            ),
            Self::UnknownVerb {
                command, offered, ..
            } => write!(f, "`zaru {command}` has no `{offered}` verb"),
            Self::UnexpectedWord { command, offered } => write!(
                f,
                "`zaru {command}` takes no further word, and it was given {offered:?}"
            ),
            Self::ArgumentMissing { command, argument } => {
                write!(f, "`zaru {command}` needs {argument} and was given none")
            }
            Self::UnknownFlag { offered, .. } => {
                write!(f, "there is no `{offered}` flag")
            }
            Self::FlagNeedsValue { flag, value } => {
                write!(f, "`{flag}` needs {value} and was given none")
            }
            Self::FlagTakesNoValue { flag } => {
                write!(f, "`{flag}` takes no value and was given one")
            }
            Self::FlagRepeated { flag, .. } => write!(
                f,
                "`{flag}` was given twice, and nothing says which of two flags wins"
            ),
            Self::ResumeAndContinue => f.write_str(
                "`--resume` names a session and `--continue` takes the most recent one, and both \
                 were given",
            ),
            Self::RequestFlagWithCommand { flag, command } => write!(
                f,
                "`{flag}` restores a session and `zaru {command}` does something else, and both \
                 were given"
            ),
            Self::UnusableKey(refusal) => write!(f, "{refusal}"),
            Self::UnusableSessionId(refusal) => write!(
                f,
                "a session is named by a ULID, and this one is not: {refusal}"
            ),
            Self::UnusableAlias(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for CommandRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnusableKey(refusal) => Some(refusal),
            Self::UnusableSessionId(refusal) => Some(refusal),
            Self::UnusableAlias(refusal) => Some(refusal),
            Self::NotText { .. }
            | Self::UnknownCommand { .. }
            | Self::NamespaceNotBuilt { .. }
            | Self::VerbMissing { .. }
            | Self::UnknownVerb { .. }
            | Self::UnexpectedWord { .. }
            | Self::ArgumentMissing { .. }
            | Self::UnknownFlag { .. }
            | Self::FlagNeedsValue { .. }
            | Self::FlagTakesNoValue { .. }
            | Self::FlagRepeated { .. }
            | Self::ResumeAndContinue
            | Self::RequestFlagWithCommand { .. } => None,
        }
    }
}
