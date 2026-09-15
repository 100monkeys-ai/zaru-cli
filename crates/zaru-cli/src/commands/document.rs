// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a command file becomes, and every way one is refused.
//!
//! # The stem is the name, and the key may only agree
//!
//! [ADR-0015] D3 spells the file `<name>.md`, so **the file names the
//! command**. A `name` key in the front matter is permitted and is checked
//! against the stem rather than believed: a disagreement is refused naming
//! both. That is a redundant assertion rather than a second source of truth,
//! and it is what removes the whole class of "two files inside one directory
//! define one name" — inside a directory the stem is unique by the
//! filesystem, so the only collision left is across the two locations, which
//! is precedence rather than an error.
//!
//! **`arguments` is not a key**, and that is stated rather than omitted. The
//! arguments a command takes are the placeholders in its body; a declared
//! list would be a second statement of the same fact, which is the drift
//! [`crate::manifest::file`] already refuses for a manifest's tables.
//!
//! # No refusal carries a line of the body
//!
//! A refusal is the text that gets pasted into a report. Names and key
//! spellings are quoted, because a reader has to be able to find the thing in
//! their own file; a line of the body never is, which is
//! [`crate::config::file`]'s own rule and the reason it builds a TOML refusal
//! from the parser's `message` rather than its `Display`.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::file::FileRefused;
use core::fmt;
use std::path::{Path, PathBuf};

/// The directory a command file sits in, under both of D3's built locations.
pub const COMMANDS_DIRECTORY: &str = "commands";

/// The extension D3 spells: `<name>.md`.
pub const COMMAND_EXTENSION: &str = "md";

/// Which of [ADR-0015] D3's locations a command came from.
///
/// **Two variants, not three.** The served location loads nothing, so a
/// variant for it would be [Verification lessons] §7's "check whose trigger
/// can never fire" wearing a type: a value nothing constructs, matched
/// everywhere, asserting coverage it does not have. It arrives when the
/// loader does.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// `~/.zaru/commands/<name>.md`. The user's own, and needs no admission.
    User,
    /// `./.zaru/commands/<name>.md`. The project's, and [ADR-0015] D4 gates
    /// it.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Project,
}

impl Source {
    /// The word [ADR-0015] D6's attribution line uses.
    ///
    /// D6's own example is `◈ /deploy-check (project · admitted 2026-08-19)`,
    /// so `project` is the record's word and `user` is its counterpart.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }

    /// The directory this location's commands sit in, under `root`.
    #[must_use]
    pub fn directory(self, root: &Path) -> PathBuf {
        match self {
            Self::User => root.join(COMMANDS_DIRECTORY),
            Self::Project => root.join(".zaru").join(COMMANDS_DIRECTORY),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// One loaded command.
///
/// The body is **text** and nothing else; see this module tree's own
/// documentation for why that is the whole of D1's inertness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    name: String,
    description: Option<String>,
    body: String,
    source: Source,
    path: PathBuf,
}

impl Command {
    /// A command, already checked.
    ///
    /// Built by [`crate::commands::load`] and by fixtures, never from
    /// unvalidated text: the name has been checked against the stem and
    /// against [ADR-0015] D2's namespaces, and the body's placeholders have
    /// been checked against the grammar.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: Option<String>,
        body: impl Into<String>,
        source: Source,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            description,
            body: body.into(),
            source,
            path: path.into(),
        }
    }

    /// The command's name, without a leading slash.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The spelling a user types.
    #[must_use]
    pub fn slash(&self) -> String {
        format!("/{}", self.name)
    }

    /// What the front matter said this command is for, if it said.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// The template, verbatim.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Which of D3's locations it came from.
    #[must_use]
    pub const fn source(&self) -> Source {
        self.source
    }

    /// The file it was read from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The task this command is, given everything typed after its name.
    ///
    /// One pass. See [`crate::commands::placeholder::expand`].
    #[must_use]
    pub fn expand(&self, tail: &str) -> String {
        crate::commands::placeholder::expand(&self.body, tail)
    }
}

/// A command, expanded, together with what D6 attributes it by.
///
/// **There is no field here a tool name could ride.** The task is a `String`
/// and its one consumer is the turn's own `Start::Task`, which is what makes
/// D1's "a command cannot execute anything" a property of the types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expanded {
    /// The command's name, without a leading slash.
    pub name: String,
    /// Which of D3's locations it came from.
    pub source: Source,
    /// The date the user admitted it, or `None` for a user command, which
    /// needs no admission.
    pub admitted: Option<String>,
    /// The line the user actually typed, verbatim.
    pub typed: String,
    /// The text the turn runs.
    pub task: String,
}

impl Expanded {
    /// [ADR-0015] D6's attribution line, in the record's own shape:
    /// `◈ /deploy-check (project · admitted 2026-08-19)`.
    ///
    /// **The glyph is not here.** `◈` is
    /// [`zaru_tui::shell::Register::Announced`]'s own marker and the pane
    /// paints it from the register, so this composes the text after it and
    /// authors no glyph.
    ///
    /// A user command has no admission date, so it reads `(user)`. Saying
    /// `admitted` of something nobody was asked about would be false.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn attribution(&self) -> String {
        match &self.admitted {
            Some(admitted) => format!("/{} ({} · admitted {admitted})", self.name, self.source),
            None => format!("/{} ({})", self.name, self.source),
        }
    }
}

/// Why a file did not become a [`Command`].
#[derive(Debug)]
pub enum CommandRefused {
    /// The file could not be read, or its front matter is not TOML.
    File(FileRefused),
    /// The directory could not be listed.
    NotListed {
        /// The directory.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// There is no `+++` fence, or the opening one is never closed.
    NoFrontMatter {
        /// The file.
        path: PathBuf,
    },
    /// The front matter's `name` is not the file's stem.
    NameDisagrees {
        /// The file.
        path: PathBuf,
        /// What the file is called.
        stem: String,
        /// What the key said.
        declared: String,
    },
    /// The front matter carries a key this schema has no row for.
    UnknownKey {
        /// The file.
        path: PathBuf,
        /// The key, as it was spelled.
        offered: String,
        /// The nearest key this schema does have.
        nearest: &'static str,
    },
    /// A key is present and is not a string.
    NotAString {
        /// The file.
        path: PathBuf,
        /// The key.
        key: &'static str,
    },
    /// The body carries something in placeholder shape that the grammar has
    /// no rule for.
    UnknownPlaceholder {
        /// The file.
        path: PathBuf,
        /// The spelling, including the `$`. Never the line it sat on.
        spelling: String,
    },
    /// The name is one of [ADR-0015] D2's namespaces, or the shell's own
    /// leave word.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Shadows {
        /// The file.
        path: PathBuf,
        /// The name it wanted.
        name: String,
        /// The built-in spelling it collided with, as a user would type it.
        spelling: String,
    },
}

impl CommandRefused {
    /// The file the refusal is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::File(refused) => refused.path(),
            Self::NotListed { path, .. }
            | Self::NoFrontMatter { path }
            | Self::NameDisagrees { path, .. }
            | Self::UnknownKey { path, .. }
            | Self::NotAString { path, .. }
            | Self::UnknownPlaceholder { path, .. }
            | Self::Shadows { path, .. } => path,
        }
    }
}

impl fmt::Display for CommandRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(refused) => refused.fmt(f),
            Self::NotListed { path, source } => {
                write!(f, "could not list {}: {source}", path.display())
            }
            Self::NoFrontMatter { path } => write!(
                f,
                "{} has no `+++` front matter; a command file opens with a `+++` line and closes \
                 the block with another",
                path.display()
            ),
            Self::NameDisagrees {
                path,
                stem,
                declared,
            } => write!(
                f,
                "{} is named `{stem}` and its front matter says `{declared}`; a command's name is \
                 its file's",
                path.display()
            ),
            Self::UnknownKey {
                path,
                offered,
                nearest,
            } => write!(
                f,
                "{} declares `{offered}`, which a command file has no key for; the nearest is \
                 `{nearest}`",
                path.display()
            ),
            Self::NotAString { path, key } => {
                write!(f, "`{key}` in {} is not a string", path.display())
            }
            Self::UnknownPlaceholder { path, spelling } => write!(
                f,
                "{} uses `{spelling}`, which is not an argument placeholder; a command takes \
                 `$ARGUMENTS` or `$1` to `$9`",
                path.display()
            ),
            Self::Shadows {
                path,
                name,
                spelling,
            } => write!(
                f,
                "{} would name the command `{name}`, and `{spelling}` is already this harness's; a \
                 command file may not shadow a built-in",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CommandRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(refused) => Some(refused),
            Self::NotListed { source, .. } => Some(source),
            Self::NoFrontMatter { .. }
            | Self::NameDisagrees { .. }
            | Self::UnknownKey { .. }
            | Self::NotAString { .. }
            | Self::UnknownPlaceholder { .. }
            | Self::Shadows { .. } => None,
        }
    }
}
