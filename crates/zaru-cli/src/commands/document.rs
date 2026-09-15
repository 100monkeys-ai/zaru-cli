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
use zaru_core::iteration::validator::Declared;

/// The directory a command file sits in, under both of D3's built locations.
pub const COMMANDS_DIRECTORY: &str = "commands";

/// The extension D3 spells: `<name>.md`.
pub const COMMAND_EXTENSION: &str = "md";

/// What [ADR-0015] D4's gate says when a project offers something new.
///
/// D4 names no sentence, so this is one, **drafted under a delegated
/// coordinator ruling of 2026-09-15 and open to Jeshua's veto**, in the shape
/// `compose::prose`'s constants and `tools::prompt::SUFFIX` already have: no
/// record supplies a line and one is needed, so it is named once here with
/// its reasoning rather than typed at a call site.
///
/// It says three things and nothing else, which is what D4's own Negative
/// section asks for — "the report must be short and the admission one
/// keystroke". That this directory offers commands; that nothing has loaded
/// them; and what the question is. The commands themselves are the question's
/// `detail` rather than words in this sentence, so a project with nine of
/// them makes the list longer and this line does not move.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub const ADMISSION_STATEMENT: &str =
    "this project offers commands, and none of them is loaded until you admit it. Admit them here?";

/// What [ADR-0015] D4's gate says once it has been answered yes.
///
/// **Authored under a delegated coordinator ruling of 2026-09-15 11:01:57Z
/// and open to Jeshua's veto**, in the shape [`ADMISSION_STATEMENT`] above
/// takes. It reads, for a project offering two commands and one skill:
///
/// > `admitted 2 commands and 1 skill from this project`
///
/// **The count is composed and the words are not.** A tally is a fact about
/// the project, and the kinds are counted separately because a skill's
/// validators run a command line where a command's body is text — a person
/// who admitted one has admitted a different thing. A half that is zero is
/// omitted rather than written as `0 skills`, which would say the project
/// offers a kind it does not.
///
/// # Why an admission says anything at all
///
/// It said nothing until 2026-09-15, measured on the release binary at
/// `c49e669` and again at `15d31f1`: answering the door with `y` left the
/// pane empty, and the only later evidence was that the picker had gained a
/// row. An act that outlives the session and governs what a cloned repository
/// may put into a model's prompt was invisible at the moment it happened.
/// That is the reasoning [ADR-0011](https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface-updates-3)'s
/// amendments volume 3 already wrote for the prompt beside it: without a
/// line, "the only difference … would be the **absence** of a later prompt,
/// which is a thing a reader cannot see".
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[must_use]
pub fn admitted_statement(commands: usize, skills: usize) -> String {
    format!("admitted {} from this project", tally(commands, skills))
}

/// What the same gate says when it is declined.
///
/// **Authored under the same ruling and open to the same veto.** It reads,
/// verbatim:
///
/// > `nothing was admitted; this project's commands stay unloaded`
///
/// A decline writes nothing to [`Admissions`](super::Admissions) by design —
/// D4's gate is a standing question rather than a standing verdict — so this
/// line is the only trace it leaves, and it says what the state now is rather
/// than what the person pressed.
pub const NOTHING_WAS_ADMITTED: &str =
    "nothing was admitted; this project's commands stay unloaded";

/// `2 commands and 1 skill`, with a zero half left out.
///
/// Both halves zero cannot be reached from the door — it is put only where
/// something is offered — and is written as `nothing` rather than as an empty
/// string, because a sentence with a hole in it is worse than a sentence that
/// is merely never said.
fn tally(commands: usize, skills: usize) -> String {
    let counted = |how_many: usize, one: &str, many: &str| {
        format!("{how_many} {}", if how_many == 1 { one } else { many })
    };
    match (commands, skills) {
        (0, 0) => "nothing".to_owned(),
        (0, skills) => counted(skills, "skill", "skills"),
        (commands, 0) => counted(commands, "command", "commands"),
        (commands, skills) => format!(
            "{} and {}",
            counted(commands, "command", "commands"),
            counted(skills, "skill", "skills")
        ),
    }
}

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

/// Which of [ADR-0015] D1's two file-borne extension kinds a file is.
///
/// D1's table gives three kinds and two of them are files: a **command**, "a
/// named prompt template with arguments", and a **skill**, "a named
/// procedure: instructions plus optional validators". The third is an MCP
/// server, which is a process rather than a file and has no variant here for
/// the reason [`Source`] has no `Served` variant.
///
/// **The word is D1's own**, so a rendered `(skill)` authors nothing: it is
/// the name the record gives the kind.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// `<name>.md`. Expands to text and declares nothing.
    Command,
    /// `<name>.skill.md`. May carry [ADR-0009] D3's `expect` clauses in
    /// `[[validator]]` blocks, and runs inside the iteration loop when it
    /// does.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    Skill,
}

impl Kind {
    /// D1's own word for this kind.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Skill => "skill",
        }
    }

    /// The front-matter keys a file of this kind may carry.
    ///
    /// Walked rather than matched against literals at each site, so a key
    /// arrives here or nowhere — [Verification lessons] §17.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    #[must_use]
    pub const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Command => &["description", "name"],
            Self::Skill => &["description", "name", VALIDATOR_TABLE],
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

/// The array of tables a skill declares its validators in.
///
/// **It is `[[validator]]`, which is the manifest's own spelling**, read from
/// [`crate::manifest::VALIDATOR_TABLE`] rather than typed here so the two
/// cannot drift. [ADR-0015] D5 says a skill "may carry `expect` clauses in
/// the vocabulary of [ADR-0009] D3"; D3's vocabulary is the four `expect`
/// **kinds**, and the table those clauses are written in is [ADR-0009] D1's
/// `[[validator]]`, whose fields are `name`, `run`, `expect` and `after`. An
/// array named for one of its own fields would be two spellings of one thing.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub const VALIDATOR_TABLE: &str = crate::manifest::VALIDATOR_TABLE;

/// One loaded command or skill.
///
/// The body is **text** and nothing else; see this module tree's own
/// documentation for why that is the whole of D1's inertness. A skill's
/// [`Command::validators`] are not part of the body and are never expanded —
/// they are [ADR-0009] D1 declarations the iteration loop runs, which is the
/// only thing D1's table lets a skill do that a command cannot.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    name: String,
    description: Option<String>,
    body: String,
    source: Source,
    kind: Kind,
    validators: Vec<Declared>,
    file: String,
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
        let body = body.into();
        let file = body.clone();
        Self {
            name: name.into(),
            description,
            body,
            source,
            kind: Kind::Command,
            validators: Vec::new(),
            file,
            path: path.into(),
        }
    }

    /// Declare this file's kind, its validators and the bytes it was read
    /// from.
    ///
    /// `file` is the **whole file**, front matter and fences included, which
    /// is what [ADR-0015] D4's record stores: a rewritten `description` or a
    /// rewritten `run` line is a change to what the user admitted exactly as
    /// a rewritten body is, and only the whole file says so.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn of(mut self, kind: Kind, validators: Vec<Declared>, file: impl Into<String>) -> Self {
        self.kind = kind;
        self.validators = validators;
        self.file = file.into();
        self
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

    /// Which of D1's two file-borne kinds this is.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// [ADR-0009] D1 validators this skill declares, in declaration order.
    ///
    /// Always empty for a [`Kind::Command`], which has no key to declare one
    /// in.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    #[must_use]
    pub fn validators(&self) -> &[Declared] {
        &self.validators
    }

    /// The whole file as it was read, front matter and fences included.
    ///
    /// This is what [ADR-0015] D4's admission record stores and compares. See
    /// [`Command::of`].
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The word [ADR-0015] D6's attribution line names this file's origin
    /// with.
    ///
    /// `project` or `user` for a command, and `project skill` or `user skill`
    /// for a skill. **Neither half is authored**: [`Source::word`] is D6's own
    /// example's word and [`Kind::word`] is D1's table's.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn origin(&self) -> String {
        origin_words(self.source, self.kind)
    }

    /// The rows [ADR-0015] D4's question shows this file as, under its
    /// statement.
    ///
    /// The slash spelling, with `(skill)` after it where the kind is one, and
    /// then **one row per declared validator carrying its `run` line
    /// verbatim**, indented. That last part is what makes the gate informed
    /// rather than nominal: a skill's validator command is the one thing in
    /// either file that will actually be executed, and D4's own words are
    /// that "the harness **reports what the project offers**".
    ///
    /// **No sentence is authored here.** `skill` is [`Kind::word`]'s and the
    /// run text is the file's.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn offered_rows(&self) -> Vec<String> {
        let mut rows = match self.kind {
            Kind::Command => vec![self.slash()],
            Kind::Skill => vec![format!("{} ({})", self.slash(), self.kind.word())],
        };
        for validator in &self.validators {
            rows.push(format!("  {}", validator.run.as_str()));
        }
        rows
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
    /// Which of D1's two file-borne kinds it is.
    pub kind: Kind,
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
        attribution_line(
            &self.name,
            &origin_words(self.source, self.kind),
            self.admitted.as_deref(),
        )
    }
}

/// [ADR-0015] D6's origin words for one location and one kind.
///
/// The one place the two are joined, so the pane, the transcript record and
/// the `--resume` replay cannot come to spell them differently. **Neither
/// word is authored**: `project`/`user` is D6's own example's and
/// `skill` is D1's table's.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[must_use]
pub fn origin_words(source: Source, kind: Kind) -> String {
    match kind {
        Kind::Command => source.word().to_owned(),
        Kind::Skill => format!("{} {}", source.word(), kind.word()),
    }
}

/// [ADR-0015] D6's attribution line, less the glyph the register paints.
///
/// **The one place this line is spelled.** Its two callers are
/// [`Expanded::attribution`], which is what the pane paints the moment a
/// command expands, and `terminal::vocabulary`, which is what `--resume`
/// paints from the transcript record. Two spellings of one line are two
/// things that can come to disagree, which is the rule this workspace keeps
/// giving.
///
/// `admitted` is `None` for a user command, which was never admitted; saying
/// it was would be false.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[must_use]
pub fn attribution_line(name: &str, source: &str, admitted: Option<&str>) -> String {
    match admitted {
        Some(admitted) => format!("/{name} ({source} · admitted {admitted})"),
        None => format!("/{name} ({source})"),
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
    /// The file resolves outside the working directory.
    ///
    /// A symlink in a cloned project's `.zaru/commands/` is how that project
    /// reads a file the person never offered it — into the picker, into the
    /// admissions record, and into a model prompt. Refused **unread**, which
    /// is [ADR-0011] D4's boundary applied where `manifest::file` already
    /// applies it.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    OutsideTheWorkingDirectory {
        /// The file, as it was named. Never what it resolved to.
        path: PathBuf,
    },
    /// A `[[validator]]` block did not parse, or one of its fields is
    /// unusable.
    ///
    /// Carried whole from [`crate::manifest::ManifestNotRead`] rather than
    /// re-rendered, because the manifest's own parser is the one that read
    /// it: the position it names is the position inside this file's front
    /// matter, and the path this variant carries is what tells a reader which
    /// file that position is in.
    Validator {
        /// The file.
        path: PathBuf,
        /// The manifest reader's own refusal.
        source: crate::manifest::ManifestNotRead,
    },
    /// A `<name>.md` carries a `[[validator]]`, which is a skill's key.
    ValidatorInACommand {
        /// The file.
        path: PathBuf,
        /// The name it would have to be called to declare one.
        skill: String,
    },
    /// Two files in one directory claim one name.
    ///
    /// The only collision the filesystem cannot prevent: a stem is unique
    /// inside a directory, and `<name>.md` and `<name>.skill.md` are two
    /// stems naming one command. Neither loads, because which of them wins
    /// would be exactly the "behaviour that depends on load order" D2's
    /// shadowing rule exists to prevent.
    NameCollision {
        /// The `<name>.md`.
        command: PathBuf,
        /// The `<name>.skill.md`.
        skill: PathBuf,
        /// The name they both claim.
        name: String,
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
            | Self::OutsideTheWorkingDirectory { path }
            | Self::Validator { path, .. }
            | Self::ValidatorInACommand { path, .. }
            | Self::Shadows { path, .. } => path,
            Self::NameCollision { command, .. } => command,
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
            Self::OutsideTheWorkingDirectory { path } => write!(
                f,
                "{} resolves outside the working directory; a project's command file may not \
                 link out of the tree it came with",
                path.display()
            ),
            Self::Validator { path, source } => {
                write!(f, "{}: {source}", path.display())
            }
            Self::ValidatorInACommand { path, skill } => write!(
                f,
                "{} declares `{VALIDATOR_TABLE}`, which only a skill may declare; rename it \
                 `{skill}` to run its validators in the iteration loop",
                path.display()
            ),
            Self::NameCollision {
                command,
                skill,
                name,
            } => write!(
                f,
                "{} and {} would both name `{name}`, and one name is one file; rename or remove \
                 one of them",
                command.display(),
                skill.display()
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
            Self::Validator { source, .. } => Some(source),
            Self::NoFrontMatter { .. }
            | Self::NameDisagrees { .. }
            | Self::UnknownKey { .. }
            | Self::NotAString { .. }
            | Self::UnknownPlaceholder { .. }
            | Self::OutsideTheWorkingDirectory { .. }
            | Self::ValidatorInACommand { .. }
            | Self::NameCollision { .. }
            | Self::Shadows { .. } => None,
        }
    }
}
