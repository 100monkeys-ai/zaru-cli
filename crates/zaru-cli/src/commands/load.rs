// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D3's two built locations, D4's gate, and clause 5's collision.
//!
//! # The order, and what each step decides
//!
//! 1. Both directories are listed and every `<name>.md` in them is read,
//!    parsed and checked — including a project's, because D4's "the harness
//!    **reports what the project offers**" cannot be done without reading it.
//!    A file that is refused here is refused for good: it never reaches the
//!    offer, so nothing can admit it.
//! 2. **Clause 5 is applied at that read.** "A user command attempting to
//!    shadow a built-in namespace is rejected at load, naming the collision",
//!    and D2's own sentence binds "a subcommand exactly as it binds a slash
//!    command", so a name is refused against **both** spellings of every
//!    [`Namespace`] — `session.md` and `sessions.md` alike.
//! 3. The project's commands are handed to [`Admissions`] and load only if
//!    every one of them is already admitted with that body. Otherwise nothing
//!    of the project's loads and the offer stands, which is D4's gate.
//! 4. **Project over user for a name both define**, and D6's attribution line
//!    says which won. A team's committed command is the workflow the record
//!    exists to share; the shadowed user command is not an error and is not
//!    reported — it is simply not the one that ran, which the attribution
//!    makes visible at the moment it matters.
//!
//! # `exit`, which is a reading rather than the letter
//!
//! `/exit` is the shell's leave word and is not a namespace, so clause 5's
//! letter does not reach it. Its **reason** does: D2 refuses shadowing because
//! it "produces behaviour that depends on load order, which is unexplainable
//! at the moment it matters", and a command named `exit` that never ran
//! because the shell left first is precisely that. It is refused, under a
//! delegated coordinator ruling of 2026-09-15 open to Jeshua's veto and
//! recorded on [ADR-0015's amendments volume 2].
//!
//! # One refused file does not disable its neighbours
//!
//! Every refusal is collected and every other file still loads. A project
//! whose one bad file disabled the rest would punish the reader for somebody
//! else's typo, and the refusals are shown once at load in [ADR-0016] D1's
//! error register rather than swallowed.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0015's amendments volume 2]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates-2
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::namespace::Namespace;
use crate::commands::admission::Admissions;
use crate::commands::document::{
    COMMAND_EXTENSION, COMMANDS_DIRECTORY, Command, CommandRefused, Expanded, Kind, Source,
};
use crate::commands::{front_matter, placeholder, skill};
use crate::config::file::{SizeCeiling, TomlFile, text};
use crate::config::{Table, Value};
use crate::tools::WorkingDirectory;
use std::path::{Path, PathBuf};

/// The keys a command file's front matter may carry.
///
/// Walked rather than matched against literals at each site, so a third key
/// arrives here or nowhere. **A skill has one more**, and the set is
/// [`Kind::keys`]'s rather than this constant's wherever the kind is known;
/// this is the command's, kept as the name the refusal's nearest-match is
/// computed over when there is no kind to ask.
pub const KEYS: &[&str] = Kind::Command.keys();

/// The word the shell leaves on, without its slash.
///
/// Named from [`zaru_tui::shell::LEAVE`] rather than typed, so the two cannot
/// drift apart.
fn leave_word() -> &'static str {
    zaru_tui::shell::LEAVE.trim_start_matches('/')
}

/// What a project offers and what this user has said about it.
#[derive(Debug)]
pub enum Offer {
    /// The project offers nothing, or everything it offers is already
    /// admitted with that body.
    Settled,
    /// The project offers these, and the user has not been asked about all of
    /// them.
    ///
    /// Nothing in here has loaded. The names are in load order.
    Pending(Vec<Command>),
}

impl Offer {
    /// The commands still owed a question, in load order.
    #[must_use]
    pub fn pending(&self) -> &[Command] {
        match self {
            Self::Settled => &[],
            Self::Pending(commands) => commands,
        }
    }
}

/// What a session's two locations came to.
#[derive(Debug)]
pub struct Loaded {
    /// Every command that loaded, project over user, sorted by name.
    pub commands: Vec<Command>,
    /// The date each loaded command was admitted, where it needed admitting.
    pub admitted: Vec<(String, String)>,
    /// What the project offers that the user has not answered for.
    pub offer: Offer,
    /// Every file that was refused, in the order they were read.
    pub refusals: Vec<CommandRefused>,
}

impl Loaded {
    /// The command this name spells, if one loaded.
    #[must_use]
    pub fn named(&self, name: &str) -> Option<&Command> {
        self.commands.iter().find(|command| command.name() == name)
    }

    /// [ADR-0009] D1 validators the command this name spells declares.
    ///
    /// Empty for a command, for a skill that declares none, and for a name
    /// nothing loaded — which are three different facts with one consequence,
    /// because [ADR-0015] D5's "one without runs as instructions" is exactly
    /// the turn a project with no manifest already runs.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[must_use]
    pub fn validators_of(&self, name: &str) -> &[zaru_core::iteration::validator::Declared] {
        self.named(name).map_or(&[], Command::validators)
    }

    /// The date `name` was admitted, where it needed admitting.
    #[must_use]
    pub fn admitted_on(&self, name: &str) -> Option<&str> {
        self.admitted
            .iter()
            .find(|(admitted_name, _)| admitted_name == name)
            .map(|(_, date)| date.as_str())
    }

    /// The task `typed` is, if `name` loaded.
    ///
    /// `typed` is the whole line, `/name` and all; the tail handed to the
    /// grammar is everything after the first word, trimmed of the whitespace
    /// that separated the two and of nothing else — an argument's own
    /// interior spacing is the user's.
    #[must_use]
    pub fn expand(&self, name: &str, typed: &str) -> Option<Expanded> {
        let command = self.named(name)?;
        let tail = typed
            .trim_start()
            .strip_prefix('/')
            .and_then(|rest| rest.strip_prefix(name))
            .map(str::trim_start)
            .unwrap_or_default();
        Some(Expanded {
            name: command.name().to_owned(),
            source: command.source(),
            kind: command.kind(),
            admitted: self.admitted_on(name).map(ToOwned::to_owned),
            typed: typed.to_owned(),
            task: command.expand(tail),
        })
    }
}

/// Load both of D3's built locations under `home` and `here`.
///
/// `home` is the `~/.zaru`-equivalent root and `here` is [ADR-0011] D4's
/// canonical working directory; either may be absent on a machine that has
/// neither, which is no commands rather than a failure.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn load_from(
    home: Option<&Path>,
    here: Option<&Path>,
    admissions: &Admissions,
    ceiling: SizeCeiling,
) -> Loaded {
    let mut refusals = Vec::new();

    let user = home.map_or_else(Vec::new, |home| {
        read_directory(
            &home.join(COMMANDS_DIRECTORY),
            Source::User,
            ceiling,
            None,
            None,
            &mut refusals,
        )
    });
    // **The project's location is measured against [ADR-0011] D4's boundary
    // and the user's is not.** A command file under `~/.zaru/commands/` is
    // the user's own and there is no tree for it to leave; one under
    // `./.zaru/commands/` came with a repository, and a symlink there is how
    // a cloned project reads a file the person never offered it — into the
    // picker, into the admissions record, and into a model prompt. The
    // containment is `WorkingDirectory`'s and is not restated here, which is
    // the rule `manifest::file` already follows for `zaru.toml`.
    let project = here.map_or_else(Vec::new, |here| {
        let boundary = WorkingDirectory::at(here).ok();
        read_directory(
            &here.join(".zaru").join(COMMANDS_DIRECTORY),
            Source::Project,
            ceiling,
            boundary.as_ref(),
            // **A project's refusal names its file from here.** The path is
            // under the directory the person is standing in, so its absolute
            // spelling repeats what they know and pushes the sentence off a
            // narrow frame: measured at 40 columns from the release binary at
            // `15d31f1`, a `---`-fenced `greet.md` took seven rows of the
            // opening pane and four of them were the path. The user's own
            // location above passes `None`, because there is no tree for
            // `~/.zaru/commands/` to be inside.
            Some(here),
            &mut refusals,
        )
    });

    // D4's gate. A project whose commands cannot be checked against the file
    // is treated as unadmitted -- the safe direction is the one that asks,
    // and a read failure is shown as a refusal rather than silently trusted.
    let (project, offer) = match here {
        Some(here) => match admissions.covers(here, &project) {
            Ok(true) => (project, Offer::Settled),
            Ok(false) => (Vec::new(), Offer::Pending(project)),
            Err(_) => (Vec::new(), Offer::Pending(project)),
        },
        None => (Vec::new(), Offer::Settled),
    };

    let mut admitted = Vec::new();
    if let Some(here) = here {
        for command in &project {
            if let Ok(Some(date)) = admissions.admitted_on(here, command.name(), command.file()) {
                admitted.push((command.name().to_owned(), date));
            }
        }
    }

    // Project over user for a name both define. The user's is dropped rather
    // than reported: it is not an error, and D6's attribution is what makes
    // the winner visible.
    let mut commands = project;
    for command in user {
        if !commands.iter().any(|kept| kept.name() == command.name()) {
            commands.push(command);
        }
    }
    commands.sort_by(|left, right| left.name().cmp(right.name()));

    Loaded {
        commands,
        admitted,
        offer,
        refusals,
    }
}

/// Every `<name>.md` in one directory, in name order, refused by a path a
/// reader standing in `from_here` can act on.
///
/// `from_here` is the working directory for [ADR-0015] D3's project location
/// and `None` for the user's own — see the call sites for which and why.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
fn read_directory(
    directory: &Path,
    source: Source,
    ceiling: SizeCeiling,
    boundary: Option<&WorkingDirectory>,
    from_here: Option<&Path>,
    refusals: &mut Vec<CommandRefused>,
) -> Vec<Command> {
    let mut mine = Vec::new();
    let commands = read_location(directory, source, ceiling, boundary, &mut mine);
    // **One seam rather than one per push.** Every refusal this location
    // produced passes through here, so a shape added to the reader below
    // cannot arrive with a path nobody shortened, and the rule is stated once
    // rather than at each of the five places a refusal is made.
    if let Some(root) = from_here {
        for refusal in &mut mine {
            refusal.shorten_against(root);
        }
    }
    refusals.append(&mut mine);
    commands
}

/// Read one of [ADR-0015] D3's locations, as it is on disk.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
fn read_location(
    directory: &Path,
    source: Source,
    ceiling: SizeCeiling,
    boundary: Option<&WorkingDirectory>,
    refusals: &mut Vec<CommandRefused>,
) -> Vec<Command> {
    let listing = match std::fs::read_dir(directory) {
        Ok(listing) => listing,
        // A location that does not exist offers nothing. A machine that has
        // never written a command file is not a machine with a fault.
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(source) => {
            refusals.push(CommandRefused::NotListed {
                path: directory.to_path_buf(),
                source,
            });
            return Vec::new();
        }
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in listing {
        match entry {
            Ok(entry) => {
                let path = entry.path();
                if path.extension().is_some_and(|it| it == COMMAND_EXTENSION) {
                    paths.push(path);
                }
            }
            Err(failure) => refusals.push(CommandRefused::NotListed {
                path: directory.to_path_buf(),
                source: failure,
            }),
        }
    }
    // A directory's order is the filesystem's; a command corpus that changed
    // shape between two runs of one machine would make every frame check a
    // lottery.
    paths.sort();

    let mut commands = Vec::new();
    for path in paths {
        match read_file(&path, source, ceiling, boundary) {
            Ok(command) => commands.push(command),
            Err(refused) => refusals.push(refused),
        }
    }
    refuse_a_collision(&mut commands, refusals);
    commands
}

/// Drop every name two files in one directory both claim, naming both.
///
/// **Both go, rather than one winning.** `<name>.md` and `<name>.skill.md`
/// are two stems spelling one name, so which of them ran would depend on the
/// order a directory was read in — "behaviour that depends on load order,
/// which is unexplainable at the moment it matters", which is D2's own reason
/// for the shadowing rule one paragraph up from this one.
///
/// The paths are sorted before this runs, so `<name>.md` is always the
/// command and `<name>.skill.md` always the skill, and the refusal names them
/// in that order however the filesystem listed them.
fn refuse_a_collision(commands: &mut Vec<Command>, refusals: &mut Vec<CommandRefused>) {
    let mut colliding: Vec<String> = Vec::new();
    for (at, command) in commands.iter().enumerate() {
        if let Some(other) = commands
            .iter()
            .skip(at + 1)
            .find(|later| later.name() == command.name())
        {
            let (first, second) = if command.kind() == Kind::Command {
                (command, other)
            } else {
                (other, command)
            };
            refusals.push(CommandRefused::NameCollision {
                command: first.path().to_path_buf(),
                skill: second.path().to_path_buf(),
                name: command.name().to_owned(),
            });
            colliding.push(command.name().to_owned());
        }
    }
    commands.retain(|command| !colliding.iter().any(|name| name == command.name()));
}

/// One file, read, parsed and checked.
fn read_file(
    path: &Path,
    source: Source,
    ceiling: SizeCeiling,
    boundary: Option<&WorkingDirectory>,
) -> Result<Command, CommandRefused> {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        // A name that is not UTF-8 cannot be typed at the composer, which
        // reads text. It is refused as a name disagreement rather than
        // reported as a filesystem fault, because that is what a reader can
        // act on: rename the file.
        return Err(CommandRefused::NameDisagrees {
            path: path.to_path_buf(),
            stem: String::new(),
            declared: String::new(),
        });
    };
    // The stem decides the name **and** the kind: `triage.skill.md` stems to
    // `triage.skill`, which is the command `triage` written as a skill. See
    // `commands::skill` for why that spelling had to be claimed rather than
    // added.
    let (name, kind) = skill::of_stem(stem);
    if name.is_empty() {
        // `.skill.md` names nothing. Refused as a name disagreement for the
        // reason a non-UTF-8 stem is: what a reader can act on is the
        // filename.
        return Err(CommandRefused::NameDisagrees {
            path: path.to_path_buf(),
            stem: stem.to_owned(),
            declared: String::new(),
        });
    }
    let stem = name.to_owned();

    // Clause 5, before the file is even opened: a shadowing name is rejected
    // at load however well-formed the file behind it is. It is checked
    // against the **name**, not the file stem, so `session.skill.md` collides
    // exactly as `session.md` does -- a skill and a command share one
    // namespace and D2's reason does not care which kind shadowed.
    if let Some(spelling) = shadowed(&stem) {
        return Err(CommandRefused::Shadows {
            path: path.to_path_buf(),
            name: stem,
            spelling,
        });
    }

    // Before the file is opened, so a link out of the tree is refused unread
    // rather than read and then judged.
    if let Some(boundary) = boundary
        && boundary.classify(path).placement().is_out_of_tree()
    {
        return Err(CommandRefused::OutsideTheWorkingDirectory {
            path: path.to_path_buf(),
        });
    }

    let Some(raw) = text(path, ceiling).map_err(CommandRefused::File)? else {
        // Listed a moment ago and gone now. Treated as absent rather than as
        // a fault, which is the rule every reader in `config::file` follows.
        return Err(CommandRefused::NoFrontMatter {
            path: path.to_path_buf(),
        });
    };

    let Some(split) = front_matter::split(&raw) else {
        return Err(CommandRefused::NoFrontMatter {
            path: path.to_path_buf(),
        });
    };

    let head = TomlFile::at(path, ceiling)
        .parse_text(split.head, split.above)
        .map_err(CommandRefused::File)?;

    let description = string(&head, "description", path)?;
    if let Some(declared) = string(&head, "name", path)?
        && declared != stem
    {
        return Err(CommandRefused::NameDisagrees {
            path: path.to_path_buf(),
            stem,
            declared,
        });
    }
    // A `[[validator]]` in a `<name>.md` is answered before the key walk, so
    // the reader is told what to rename rather than which key is nearest.
    if kind == Kind::Command {
        skill::refuse_a_validator_in_a_command(&head, &stem, path)?;
    }
    let keys = kind.keys();
    if let Some((offered, _)) = head.iter().find(|(key, _)| !keys.contains(&key.as_str())) {
        return Err(CommandRefused::UnknownKey {
            path: path.to_path_buf(),
            offered: offered.clone(),
            // The same metric ADR-0014 D5's nearest match uses, over this
            // schema's own keys rather than a list typed here.
            nearest: crate::config::nearest::nearest(keys.iter().copied(), offered)
                .unwrap_or(keys[0]),
        });
    }

    // D5's `expect` clauses, through the manifest's own reader.
    let validators = skill::validators_of(&head, path)?;

    if let Some(spelling) = placeholder::unknown(split.body) {
        return Err(CommandRefused::UnknownPlaceholder {
            path: path.to_path_buf(),
            spelling,
        });
    }

    Ok(Command::new(stem, description, split.body, source, path).of(kind, validators, raw))
}

/// One string key, or `None` when it is absent.
fn string(head: &Table, key: &'static str, path: &Path) -> Result<Option<String>, CommandRefused> {
    match head.get(key) {
        None => Ok(None),
        Some(Value::Text(text)) => Ok(Some(text.clone())),
        Some(_) => Err(CommandRefused::NotAString {
            path: path.to_path_buf(),
            key,
        }),
    }
}

/// The built-in spelling `name` collides with, if it collides with one.
///
/// Walked from [`Namespace::ALL`] rather than matched against a list retyped
/// here, so a thirteenth namespace closes this rule the day it is declared —
/// [Verification lessons] §17.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[must_use]
pub fn shadowed(name: &str) -> Option<String> {
    for namespace in Namespace::ALL {
        if namespace.slash().trim_start_matches('/') == name {
            return Some(namespace.slash().to_owned());
        }
        // D2: "**This binds a subcommand exactly as it binds a slash
        // command** -- a namespace with two entry points is one namespace, and
        // shadowing either spelling produces the same ambiguity."
        if namespace.subcommand() == name {
            return Some(format!("zaru {}", namespace.subcommand()));
        }
    }
    if name == leave_word() {
        return Some(zaru_tui::shell::LEAVE.to_owned());
    }
    None
}
