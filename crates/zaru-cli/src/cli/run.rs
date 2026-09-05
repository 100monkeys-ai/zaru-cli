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
use crate::credentials::CredentialStore;
use crate::failure::{Classified, Exit, SessionEvidence};
use crate::providers::{ModelAlias, ModelTable, ResolvedModel};
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
            Request::SessionsList => self.sessions_list(),
            Request::SessionsRemove { id } => self.sessions_remove(id),
            Request::Resume { id } => self.resume(id, &line.overrides),
            Request::Continue => self.resume_latest(&line.overrides),
            Request::NotesTokens => self.notes_tokens(),
            Request::Task { .. } => self.no_provider(&line.overrides),
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
    /// against. `/session rm` inside a session is where that guard bites, and
    /// it is `zaru-tui`'s.
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
                Outcome::failed(surface.resume(&failure, SessionEvidence::NoSessionExists))
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
