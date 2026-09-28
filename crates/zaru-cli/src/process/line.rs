// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A command line as data: a program and its arguments, and the one door a
//! string comes through.
//!
//! # There is no shell, and that is the absence of a constructor
//!
//! [`Spawn`](super::Spawn) accepts a [`CommandLine`] and nothing else, and the
//! only way to build one from text is [`CommandLine::split`]. So "no shell is
//! invoked" is not a rule anybody keeps — there is no path from a string to
//! `std::process::Command` that does not pass through this module, and this
//! module hands over a program and a vector.
//!
//! # Why a splitter at all, and why this one
//!
//! Both ports hand a command over as text. [ADR-0009] D1 writes a validator's
//! `run` as `"cargo build --locked"`, which is what a user types into a file
//! they can read; [ADR-0011] D1's row for `cmd.run` is a single string the
//! model chose. Neither is an argument vector, and neither is a shell script.
//!
//! Under a **delegated coordinator ruling of 2026-09-05** made under Jeshua's
//! directive of that day and open to his veto, the rule is: split on
//! whitespace, honour single quotes, double quotes and backslash escapes,
//! perform **no** expansion of any kind, and **refuse** a shell construct
//! rather than passing it through as a literal argument. Both records carry
//! the ruling as an accepted Update.
//!
//! The refusal is the load-bearing half. `cargo test | tee log` split as words
//! runs `cargo` with the arguments `test`, `|`, `tee`, `log` — a *different
//! command* that fails in a way naming neither the pipe nor the harness. A
//! difference between what somebody wrote and what actually ran, with nothing
//! saying so, is the worst outcome available here, and it is the same shape
//! ADR-0011 D5 refuses for truncation: "a truncation the user cannot notice is
//! how a diagnosis gets built on a fragment."
//!
//! Running a shell instead was rejected for a reason of the record's own:
//! [ADR-0011] D2 says the harness "is not a sandbox and says so", and a shell
//! at a tier with no membrane is an unbounded capability handed to a model
//! from a single string. [ADR-0004] D6's worked example decides a `cmd.run` by
//! its program — `SUBCOMMAND_DENIED — curl not in allowed_subcommands` — which
//! is a decision a shell line cannot be given.
//!
//! # What is still passed through literally, and it is a finding
//!
//! `$NAME` and a leading `~` are **not** refused, because the ruled set names
//! `$(` and no other dollar form and names no tilde. So `echo $HOME` runs
//! `echo` with the literal argument `$HOME`, which is the same silent
//! difference the operator refusal exists to prevent, arriving through a door
//! the ruling did not close. **Raised on ADR-0011 as a finding for that
//! record's author and deliberately not widened here** — adding a name to the
//! set of things a model-driven action may not write is authoring a security
//! vocabulary, which [Autonomous Development] puts on the human side.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use core::fmt;

/// The shell constructs [`CommandLine::split`] refuses, longest first.
///
/// **Longest first is load-bearing**, because the scan takes the first entry
/// that matches and `&&` must be reported as `&&` rather than as `&`. A check
/// enumerates this array, so an entry added or removed is a visible change to
/// what the harness refuses rather than an edit inside a match.
///
/// The set is the coordinator's ruling of 2026-09-05 transcribed, and nothing
/// here was chosen by this module.
pub const REFUSED_CONSTRUCTS: [&str; 9] = ["&&", "||", "$(", "|", "&", ">", "<", ";", "`"];

/// Why a string is not a command line this harness will run.
///
/// Every variant quotes the offered text back, escaped, because a `run` came
/// out of a file the user wrote and a `cmd.run` argument is what the model
/// asked for — and a refusal neither of them can locate is [ADR-0016] D2's
/// "stack trace with better grammar".
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotACommandLine {
    /// The string was empty or only whitespace.
    Empty,
    /// The string carried one of [`REFUSED_CONSTRUCTS`] outside quotes.
    ShellConstruct {
        /// Which construct, as [`REFUSED_CONSTRUCTS`] spells it.
        construct: &'static str,
        /// The whole string as it was offered, escaped.
        offered: String,
    },
    /// A quote was opened and never closed.
    UnterminatedQuote {
        /// Which quote character opened it.
        quote: char,
        /// The whole string as it was offered, escaped.
        offered: String,
    },
    /// The string ended on a backslash, which escapes nothing.
    TrailingEscape {
        /// The whole string as it was offered, escaped.
        offered: String,
    },
}

impl fmt::Display for NotACommandLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                // ADR-0009 D3's expectations and ADR-0011 D1's `cmd.run`.
                "the command is empty, so there is no program to run. A validator's expectations \
                 are all statements about what running a command produced, and `cmd.run` needs \
                 something to execute",
            ),
            Self::ShellConstruct { construct, offered } => write!(
                f,
                "the command {offered} carries the shell construct {construct:?}, and this \
                 harness runs no shell — it executes a program with arguments. Passing \
                 {construct:?} through as an ordinary argument would run a different command \
                 from the one that was written, and nothing would say so. Put the construct in a \
                 script and run the script",
            ),
            Self::UnterminatedQuote { quote, offered } => write!(
                f,
                "the command {offered} opens a {quote:?} quote and never closes it, so where one \
                 argument ends and the next begins cannot be decided",
            ),
            Self::TrailingEscape { offered } => write!(
                f,
                "the command {offered} ends on a backslash, which has nothing left to escape",
            ),
        }
    }
}

impl std::error::Error for NotACommandLine {}

/// A program and the arguments it is given, with no shell between them.
///
/// Constructed only through [`CommandLine::split`] or
/// [`CommandLine::of`], so a value that exists has already been refused every
/// shape [`NotACommandLine`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    program: String,
    arguments: Vec<String>,
}

impl CommandLine {
    /// Take a command line already split by the caller.
    ///
    /// The door for a caller that has a program and a vector already and has
    /// no string to parse. It refuses an empty program for the same reason
    /// [`CommandLine::split`] refuses an empty string.
    ///
    /// # Errors
    ///
    /// [`NotACommandLine::Empty`] when `program` is empty or only whitespace.
    pub fn of(
        program: impl Into<String>,
        arguments: impl IntoIterator<Item = String>,
    ) -> Result<Self, NotACommandLine> {
        let program = program.into();
        if program.trim().is_empty() {
            return Err(NotACommandLine::Empty);
        }
        Ok(Self {
            program,
            arguments: arguments.into_iter().collect(),
        })
    }

    /// Split a command line out of the text a record or a model supplied.
    ///
    /// Whitespace separates words. A single quote takes everything up to the
    /// next single quote literally, including backslashes. A double quote
    /// takes everything up to the next double quote, with a backslash
    /// escaping only `"` and `\` — every other backslash inside double quotes
    /// is itself. Outside quotes a backslash escapes the next character
    /// whatever it is. **No expansion of any kind happens**, and a shell
    /// construct is refused rather than passed on.
    ///
    /// # Errors
    ///
    /// [`NotACommandLine`], naming what was found and quoting the whole
    /// offered string back.
    pub fn split(offered: &str) -> Result<Self, NotACommandLine> {
        let escaped = || offered.escape_debug().to_string();
        let mut words: Vec<String> = Vec::new();
        let mut word = String::new();
        let mut started = false;
        let mut characters = offered.chars().peekable();

        while let Some(character) = characters.next() {
            match character {
                c if c.is_whitespace() => {
                    if started {
                        words.push(core::mem::take(&mut word));
                        started = false;
                    }
                }
                '\\' => {
                    let Some(next) = characters.next() else {
                        return Err(NotACommandLine::TrailingEscape { offered: escaped() });
                    };
                    word.push(next);
                    started = true;
                }
                '\'' => {
                    started = true;
                    loop {
                        let Some(inner) = characters.next() else {
                            return Err(NotACommandLine::UnterminatedQuote {
                                quote: '\'',
                                offered: escaped(),
                            });
                        };
                        if inner == '\'' {
                            break;
                        }
                        word.push(inner);
                    }
                }
                '"' => {
                    started = true;
                    loop {
                        let Some(inner) = characters.next() else {
                            return Err(NotACommandLine::UnterminatedQuote {
                                quote: '"',
                                offered: escaped(),
                            });
                        };
                        match inner {
                            '"' => break,
                            // Only `"` and `\` are escapes inside double
                            // quotes; every other backslash is itself, which
                            // is what a regular expression or a Windows path
                            // in a quoted argument needs.
                            '\\' => match characters.peek().copied() {
                                Some(escape) if escape == '"' || escape == '\\' => {
                                    characters.next();
                                    word.push(escape);
                                }
                                Some(_) => word.push('\\'),
                                None => {
                                    return Err(NotACommandLine::UnterminatedQuote {
                                        quote: '"',
                                        offered: escaped(),
                                    });
                                }
                            },
                            other => word.push(other),
                        }
                    }
                }
                _ => {
                    if let Some(construct) = construct_at(character, characters.peek().copied()) {
                        return Err(NotACommandLine::ShellConstruct {
                            construct,
                            offered: escaped(),
                        });
                    }
                    word.push(character);
                    started = true;
                }
            }
        }
        if started {
            words.push(word);
        }

        let mut words = words.into_iter();
        let Some(program) = words.next() else {
            return Err(NotACommandLine::Empty);
        };
        Self::of(program, words)
    }

    /// The program, exactly as it was written.
    ///
    /// **Not resolved here.** ADR-0011 D1 names no allowlist of programs, so
    /// what a bare name means is whatever `PATH` finds — see
    /// [`Environment`](super::Environment).
    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments, in order.
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// The command line as one string a person can read.
    ///
    /// This is what [ADR-0011] D4's transcript entry shows and what its prompt
    /// asks about, so it is the one rendering of a command anywhere in the
    /// harness. A word is quoted only when it needs to be, and the quoting is
    /// the quoting [`CommandLine::split`] accepts — so
    /// `split(line.render()) == line` for every line, which is asserted rather
    /// than described.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub fn render(&self) -> String {
        self.words().join(" ")
    }

    /// The program and each argument, each quoted as [`Self::render`] quotes
    /// it, so that the words joined by spaces are the rendered line.
    #[must_use]
    pub fn words(&self) -> Vec<String> {
        core::iter::once(&self.program)
            .chain(&self.arguments)
            .map(|word| quoted(word))
            .collect()
    }
}

impl fmt::Display for CommandLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

/// Whether a refused construct starts at `character`, and which one.
///
/// `following` is the next character without consuming it, which is what
/// tells `&&` from `&`. Nothing is consumed either way, because the refusal
/// quotes the whole offered string rather than a remainder.
///
/// [`REFUSED_CONSTRUCTS`] is walked in its declared order, so a two-character
/// construct is found before the one-character construct it starts with.
fn construct_at(character: char, following: Option<char>) -> Option<&'static str> {
    REFUSED_CONSTRUCTS.into_iter().find(|construct| {
        let mut wanted = construct.chars();
        wanted.next() == Some(character)
            && match wanted.next() {
                None => true,
                Some(second) => Some(second) == following,
            }
    })
}

/// One word, quoted only if it has to be.
///
/// A word needs quoting when it is empty, carries whitespace, carries a quote
/// or a backslash, or carries any character a refused construct begins with —
/// the last so that a rendered line re-read by [`CommandLine::split`] is
/// refused nowhere it should not be.
fn quoted(word: &str) -> String {
    let must = word.is_empty()
        || word.chars().any(|c| {
            c.is_whitespace()
                || matches!(c, '\'' | '"' | '\\')
                || REFUSED_CONSTRUCTS
                    .iter()
                    .filter_map(|construct| construct.chars().next())
                    .any(|first| first == c)
        });
    if !must {
        return word.to_owned();
    }
    let mut out = String::with_capacity(word.len() + 2);
    out.push('"');
    for c in word.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}
