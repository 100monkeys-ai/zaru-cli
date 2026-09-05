// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What each request does, and what the process exits with.
//!
//! # This module writes nothing
//!
//! It returns lines and an [`Exit`], and the binary writes them. That keeps
//! the whole of [ADR-0015]'s out-of-session surface reachable from a check
//! that is an ordinary caller, and it keeps the one `println!` in this crate
//! in `main.rs` where the composition root already is. The real evidence for
//! this surface is `tests/cli_from_outside.rs`, which runs the built binary;
//! this shape is what makes the *unit* half possible at all.
//!
//! # Every exit code the binary can reach comes from here
//!
//! [ADR-0016] D5's mapping was already built and, until 2026-09-05, only `0`
//! was observable from the real artefact — "`zaru` takes no arguments, so
//! nothing a user can do makes it fail". Something can now. What became
//! observable is `0` and `2`; `1`, `3`, `4` and `70` did not, because there
//! is still no loop to exhaust, no network to be unreachable, no tier that
//! withholds anything, and no honest way to make the binary panic.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::classify::Surface;
use crate::cli::invocation::{CommandLine, Overrides, Request};
use crate::cli::{help, layers, render};
use crate::config::{Key, Resolution};
use crate::credentials::{CredentialStore, Description, Entry, HarnessKeys, OsKeyring, Secret};
use crate::failure::{Classified, Exit, SessionEvidence};
use crate::providers::{ModelAlias, ModelTable, ProviderKind, ResolvedModel};
use crate::runtime::{ResolvedTier, Runtime};
use crate::session::{SessionId, SessionStore};

/// What one run produced.
///
/// `lines` is standard output; `exit` is [ADR-0016] D5's code. A failed run may
/// still have lines — a partial listing is worth more than nothing — though
/// nothing this surface does produces both today.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub struct Outcome {
    /// What to write to standard output.
    pub lines: Vec<String>,
    /// What the process exits with.
    pub exit: Exit,
}

impl Outcome {
    /// A run that did what was asked.
    fn printed(lines: Vec<String>) -> Self {
        Self {
            lines,
            exit: Exit::Succeeded,
        }
    }

    /// A run that did not.
    fn failed(classified: Classified) -> Self {
        Self {
            lines: Vec::new(),
            exit: Exit::Failed(classified),
        }
    }
}

/// Everything one invocation of the binary needs from outside itself.
///
/// The version and the report URL are read out of the binary's own package
/// metadata by its `main` rather than by this module's `env!`, for the reason
/// `composition()` already reads the crate names out of the crates: a value
/// retyped beside the binary is a value that drifts.
pub struct Run<'a> {
    /// The harness version.
    pub version: &'a str,
    /// Where a defect is reported.
    pub report_at: &'a str,
}

impl Run<'_> {
    /// Do what the command line asked.
    #[must_use]
    pub fn execute(&self, line: &CommandLine) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        match &line.request {
            Request::Help => Outcome::printed(help::lines(self.version)),
            Request::Version => Outcome::printed(version_lines(self.version)),
            Request::Runtime => {
                self.configured(
                    &line.overrides,
                    |resolution| match ResolvedTier::from_configuration(resolution) {
                        Ok(resolved) => Outcome::printed(render::runtime(&Runtime::of(resolved))),
                        Err(refusal) => Outcome::failed(surface.tier(&refusal)),
                    },
                )
            }
            Request::Models => {
                self.configured(
                    &line.overrides,
                    |resolution| match ModelTable::from_configuration(resolution) {
                        Ok(table) => Outcome::printed(render::models(&table)),
                        Err(refusal) => Outcome::failed(Classified::from(refusal)),
                    },
                )
            }
            Request::ConfigExplain { key } => {
                self.configured(&line.overrides, |resolution| explain(resolution, key))
            }
            Request::Init => self.init(),
            Request::SessionsList => self.sessions_list(),
            Request::SessionsRemove { id } => self.sessions_remove(id),
            Request::Resume { id } => self.resume(id, &line.overrides),
            Request::Continue => self.resume_latest(&line.overrides),
            Request::NotesTokens => self.notes_tokens(),
            Request::ProviderKeys => self.provider_keys(),
            Request::ProviderKeysAdd { kind } => self.provider_keys_add(*kind),
            Request::Task { words } => self.task(words, &line.overrides),
        }
    }

    /// [ADR-0009] D6's writer, and the one thing this surface does that changes
    /// a file the user owns.
    ///
    /// The working directory is this process's own, canonicalised through
    /// [ADR-0011] D4's [`WorkingDirectory`](crate::tools::WorkingDirectory), so
    /// the file goes where `zaru config explain` would read it from and nowhere
    /// else. A directory that cannot be canonicalised is refused rather than
    /// guessed at, for that record's reason: a boundary whose root is a guess is
    /// not a boundary.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    fn init(&self) -> Outcome {
        let here = match std::env::current_dir()
            .map_err(crate::tools::TreeError::from_current_directory)
            .and_then(crate::tools::WorkingDirectory::at)
        {
            Ok(here) => here,
            Err(failure) => return Outcome::failed(Surface::working_directory(&failure)),
        };
        let file = crate::manifest::ManifestFile::in_directory(here, layers::file_ceiling());
        match crate::manifest::init::write(&file) {
            Ok(path) => Outcome::printed(render::initialised(&path)),
            Err(refusal) => Outcome::failed(Surface::init(&refusal)),
        }
    }

    /// Reach the session store without creating anything.
    ///
    /// [`SessionStore::reading`] rather than `open`, so asking what sessions
    /// exist on a machine that has never had one creates neither `~/.zaru` nor
    /// `~/.zaru/sessions`.
    fn store(&self) -> Result<SessionStore, Box<Outcome>> {
        let surface = Surface::new(self.version, self.report_at);
        SessionStore::default_root()
            .map(SessionStore::reading)
            .map_err(|failure| Box::new(Outcome::failed(surface.session(&failure))))
    }

    /// [ADR-0010] D1's directory, listed.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn sessions_list(&self) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let store = match self.store() {
            Ok(store) => store,
            Err(outcome) => return *outcome,
        };
        match store.ids() {
            Ok(ids) => Outcome::printed(render::sessions(&ids)),
            Err(failure) => Outcome::failed(surface.session(&failure)),
        }
    }

    /// [ADR-0010] D6's deletion, from outside a session.
    ///
    /// **Nothing is spared.** D6's guard is for the session a user is inside,
    /// and outside one there is none — this binary starts no session, so the
    /// `current` a `prune` would pass is `None` and there is no id to compare
    /// against. `/session rm` inside a session is where that guard bites. **The
    /// in-session spelling exists as of 2026-09-05** and reaches this same
    /// function through `terminal::driver`'s dispatch, so the guard is unbuilt
    /// rather than unreachable: this binary starts no session, so there is
    /// still no `current` id to spare.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn sessions_remove(&self, id: &SessionId) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let store = match self.store() {
            Ok(store) => store,
            Err(outcome) => return *outcome,
        };
        match crate::session::remove(&store, id) {
            Ok(()) => Outcome::printed(vec![format!("removed {id}")]),
            Err(failure) => Outcome::failed(surface.prune(&failure)),
        }
    }

    /// [ADR-0010] D4's resume, for a named session.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn resume(&self, id: &SessionId, overrides: &Overrides) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let store = match self.store() {
            Ok(store) => store,
            Err(outcome) => return *outcome,
        };
        let directory = store.sessions_directory().join(id.as_str());

        // The whole transcript. See `render::resumed` for why no number is
        // invented here.
        match crate::session::resume(&directory, usize::MAX) {
            Ok(restored) => {
                let mut lines = render::resumed(id, &restored);
                lines.push(String::new());
                let mut outcome = self.no_provider(overrides);
                outcome.lines = lines;
                outcome
            }
            Err(failure) => {
                // **The session exists**, and until 2026-09-05 this passed
                // `NoSessionExists`, so a defect report about a transcript that
                // could not be read said "there is no session and no transcript
                // was written" about a session directory that was right there.
                // ADR-0016 D3 wants the session id and the transcript's path in
                // the report, and `Session::evidence` produces exactly that; the
                // arm that "makes a lie unrepresentable" was being handed the
                // wrong arm on the one path that has a real session.
                let evidence = store
                    .existing(id)
                    .map_or(SessionEvidence::NoSessionExists, |session| {
                        session.evidence()
                    });
                Outcome::failed(surface.resume(&failure, evidence))
            }
        }
    }

    /// D4's `--continue`: the most recent session in this directory.
    fn resume_latest(&self, overrides: &Overrides) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let store = match self.store() {
            Ok(store) => store,
            Err(outcome) => return *outcome,
        };
        let ids = match store.ids() {
            Ok(ids) => ids,
            Err(failure) => return Outcome::failed(surface.session(&failure)),
        };
        // A ULID sorts lexically by creation time and `ids` sorts, so the last
        // is the most recent -- D1's own reason for choosing a ULID over a
        // UUID, rather than a second reading of any clock.
        match ids.last() {
            Some(id) => self.resume(id, overrides),
            None => Outcome::failed(surface.no_session_to_continue()),
        }
    }

    /// [ADR-0007] D7's `tokens`, from outside a session.
    ///
    /// Read-only: `CredentialStore::reading` creates nothing, for the reason
    /// `sessions list` does not either.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// `zaru providers keys` — which providers this machine holds a key for.
    ///
    /// Opens the store for **reading**, exactly as `notes tokens` does, so
    /// asking a question creates nothing.
    fn provider_keys(&self) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let root = match CredentialStore::default_root() {
            Ok(root) => root,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match CredentialStore::reading(root) {
            Ok(store) => Outcome::printed(render::provider_keys(&store)),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// `zaru providers keys add <kind>` — store a provider's key.
    ///
    /// # The key comes from standard input, and that is the whole of the
    /// surface's design
    ///
    /// Not from an argument. An argument is written into the shell's history
    /// file, is readable in `/proc/<pid>/cmdline` by anything that can see the
    /// process, and appears in `ps` output for every user on the machine for
    /// as long as the process runs. [operations/repositories] states the rule
    /// this follows — "a credential enters the product the way a user's would"
    /// — and the way a user's should is the way that does not publish it.
    ///
    /// **Nothing this function does echoes the key.** It is read, trimmed of
    /// the single trailing newline a terminal or a `printf` adds, handed to
    /// [`Secret::provider`], and sealed. What is printed afterwards is the
    /// alias and the kind.
    ///
    /// The trailing newline is trimmed rather than refused because it is not
    /// the user's: `printf '%s\n' "$KEY" | zaru …` and pressing return in a
    /// terminal both add one, and refusing it would make every ordinary way
    /// of supplying a key fail. Whitespace the user actually typed is still
    /// refused by `Secret::provider`, which is the distinction that matters —
    /// a key with a space in the middle of it is a paste artefact worth
    /// telling them about.
    ///
    /// [operations/repositories]: https://100monkeys-ai.cortex.page/zaru/p/operations/repositories
    fn provider_keys_add(&self, kind: ProviderKind) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);

        let mut offered = String::new();
        if let Err(failure) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut offered) {
            return Outcome::failed(Surface::key_not_readable(kind, &failure));
        }
        // Exactly one trailing line ending, and only if it is there.
        let offered = offered
            .strip_suffix('\n')
            .unwrap_or(&offered)
            .strip_suffix('\r')
            .unwrap_or_else(|| offered.strip_suffix('\n').unwrap_or(&offered));

        let secret = match Secret::provider(kind, offered) {
            Ok(secret) => secret,
            // The refusal carries no part of the value -- see `SecretRefused`,
            // which is `Copy` and therefore cannot.
            Err(refusal) => return Outcome::failed(Surface::key_refused(kind, &refusal)),
        };

        let alias = ProviderKind::credential_alias(kind);
        let description = match Description::new(format!("the {kind} API key")) {
            Ok(description) => description,
            Err(refusal) => {
                return Outcome::failed(undecided_description(
                    self.version,
                    self.report_at,
                    &refusal,
                ));
            }
        };
        let entry = match Entry::provider(alias.clone(), description, secret) {
            Ok(entry) => entry,
            Err(refusal) => {
                return Outcome::failed(undecided_entry(self.version, self.report_at, &refusal));
            }
        };

        let root = match CredentialStore::default_root() {
            Ok(root) => root,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        // The key store the product uses: the OS keyring where there is one,
        // and `ZARU_CREDENTIAL_KEY` where there is not -- which is the
        // ordinary case on a headless machine, not just in CI.
        let keyring = OsKeyring::for_store(&root);
        let keys = HarnessKeys::from_process(&keyring);

        let mut store = match CredentialStore::open(root.clone()) {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        // No confirmer: ADR-0007 D8's apex confirmation is a Nuclear Notes
        // token's, and a provider key has no reach to be apex with -- the
        // enum is what says so.
        match store.add(entry, &keys, None) {
            Ok(()) => Outcome::printed(vec![
                format!("stored a `{kind}` key under the alias `{alias}`."),
                "  the value is sealed and is not printed by any command.".to_owned(),
            ]),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    fn notes_tokens(&self) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let root = match CredentialStore::default_root() {
            Ok(root) => root,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match CredentialStore::reading(root) {
            Ok(store) => Outcome::printed(render::tokens(&store)),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// The refusal every path that would need a provider ends in.
    ///
    /// **Two arms, and they are different classes**, because what is missing
    /// differs and [ADR-0016] D2 says an error whose reader cannot act is a
    /// stack trace with better grammar.
    ///
    /// If `model.default` resolves to nothing, the user has not configured a
    /// provider and the remedy is the key and the variable — user-correctable,
    /// exit 2, and this is the arm every machine with no configuration
    /// reaches.
    ///
    /// If it *does* resolve, the user has done their half and this harness
    /// still cannot act, because [ADR-0012] D3's provider trait has no
    /// implementation in any product tree. That is presented as D1's
    /// **capability** class, exit 4, under a delegated coordinator ruling of
    /// 2026-09-05 open to Jeshua's veto. **The class does not fit and the
    /// misfit is the finding**: `Classified::Capability` carries a [`Tier`],
    /// D1's row for it is "the tier does not offer this", and *no* tier in
    /// this build offers a provider. The tier named is `bare`, which is the
    /// tier at which ADR-0001 D1 says a model provider is reached — so the
    /// line is true of the design and not of this binary. **D1 has no row for
    /// "not built yet"**, and that missing row is raised as an open question
    /// on ADR-0016 rather than answered here;
    /// `the_two_halves_of_a_missing_provider_are_different_classes` pins the
    /// reading built so that deciding it the other way reddens a check.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    /// [`Tier`]: crate::runtime::Tier
    /// [ADR-0008] D1's outer loop, over one task.
    ///
    /// The composition is [`crate::compose::turn`] and it is deliberately not
    /// inline here: this module is "what each request does", and what a turn
    /// does is a dependency order twenty steps long that has to be read in one
    /// place. What is here is the call and the fold that feeds it.
    ///
    /// **This is the call site [ADR-0010]'s Status tracking has named since
    /// 2026-09-04**: "the day that call site changes is the day something
    /// reaches the loop".
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn task(&self, words: &[String], overrides: &Overrides) -> Outcome {
        // The words as the user typed them, joined with the single space that
        // separated them on the command line. The parser holds them as a
        // vector so a refusal can quote what it could not run; a turn needs
        // one string, and re-splitting would be a second reading of a line
        // that was already parsed.
        let task = words.join(" ");
        self.configured(overrides, |resolution| {
            let ran = crate::compose::turn::task(self.version, self.report_at, resolution, &task);
            Outcome {
                lines: ran.lines,
                exit: ran.exit,
            }
        })
    }

    /// What a bare `--resume` or `--continue` ends with, having restored.
    ///
    /// **A resume is not a task**, and since 2026-09-05 this is the only
    /// caller: `Request::Task` runs a turn. It restores, prints the
    /// transcript's own bytes, and then stops, because `resume` holds no ports
    /// and there is no task for a turn to be about.
    ///
    /// **The exit code it stops with is an open question and this arc did not
    /// answer it.** [ADR-0010] D4's own Update names it: "the non-terminal
    /// path still exits with the no-provider refusal's `2` or `4` … the
    /// refusal's sentence is about *running a task*, and a bare `--resume`
    /// asks for no task, so the code was arguably always slightly wrong for it
    /// and is now visibly so. It is pinned by an assertion in
    /// `tests/shell_from_outside.rs` so that deciding it the other way reddens
    /// something, and **it wants a person's answer**." It is left exactly as
    /// that arc left it, and the pin is what will redden when somebody decides
    /// it.
    ///
    /// What *has* changed is the sentence: it no longer says nothing wires a
    /// client to a loop, because something does.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn no_provider(&self, overrides: &Overrides) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        self.configured(
            overrides,
            |resolution| match ModelTable::from_configuration(resolution) {
                Err(refusal) => Outcome::failed(Classified::from(refusal)),
                Ok(table) => match table.row(ModelAlias::Default) {
                    ResolvedModel::Unresolved => {
                        Outcome::failed(Surface::no_model_for_the_default_alias())
                    }
                    ResolvedModel::Resolved { model, .. } => {
                        Outcome::failed(surface.no_provider_client(model))
                    }
                },
            },
        )
    }

    /// Fold the configuration, then do something with it.
    ///
    /// One place, so every command that needs configuration reads the same
    /// three layers in the same order and a refusal from the fold has one
    /// classification rather than one per command.
    fn configured(
        &self,
        overrides: &Overrides,
        then: impl FnOnce(&Resolution) -> Outcome,
    ) -> Outcome {
        match layers::resolve_from_process(overrides) {
            Ok(resolution) => then(&resolution),
            Err(failure) => Outcome::failed(Surface::load(&failure)),
        }
    }
}

/// What `--version` prints.
///
/// The composition list, which a bare `zaru` printed until 2026-09-05. It is
/// behind the flag now because "what am I made of" is a question a user asks
/// deliberately, and because a bare `zaru` had to answer the question a user
/// asks by typing the name of a program and nothing else.
fn version_lines(version: &str) -> Vec<String> {
    let mut lines = vec![format!("zaru {version}")];
    for (name, crate_version) in crate::composition() {
        lines.push(format!("  {name} {crate_version}"));
    }
    lines
}

/// [ADR-0014] D3's block for one key, or a refusal for a key nothing declares.
///
/// **A key the schema does not carry is refused rather than explained.**
/// Explaining it would print five `(not set)` rows, which reads as "this key
/// exists and nobody has set it" — D5's silent-typo failure with a block on
/// top, and the failure that clause exists to prevent. Ruled 2026-09-05.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn explain(resolution: &Resolution, key: &Key) -> Outcome {
    let schema = layers::schema();
    if schema.field(key).is_none() {
        return Outcome::failed(Surface::undeclared_key(key, &schema));
    }
    Outcome::printed(render::explanation(&resolution.explain(key)))
}

/// A description this module composed that the store would not take.
///
/// Unreachable: the sentence is `format!("the {kind} API key")` over
/// [`ProviderKind::as_str`]'s five literals, none of which carries a control
/// character. It is reported as a defect rather than unwrapped so that a
/// sixth kind spelled with one cannot turn a command into a panic.
fn undecided_description(
    version: &str,
    report_at: &str,
    refusal: &crate::credentials::DescriptionRefused,
) -> Classified {
    let _ = refusal;
    Classified::Defect(crate::failure::DefectReport::new(
        version,
        report_at,
        crate::failure::Location {
            file: file!().to_owned(),
            line: line!(),
            column: 0,
        },
        SessionEvidence::NoSessionExists,
    ))
}

/// An entry this module built whose secret belongs to the other family.
///
/// Unreachable: the secret two lines above came from [`Secret::provider`],
/// so its kind is `Kind::Provider` by construction and
/// [`Entry::provider`] cannot refuse it. Reported rather than unwrapped for
/// the reason above.
fn undecided_entry(
    version: &str,
    report_at: &str,
    refusal: &crate::credentials::EntryRefused,
) -> Classified {
    let _ = refusal;
    Classified::Defect(crate::failure::DefectReport::new(
        version,
        report_at,
        crate::failure::Location {
            file: file!().to_owned(),
            line: line!(),
            column: 0,
        },
        SessionEvidence::NoSessionExists,
    ))
}
