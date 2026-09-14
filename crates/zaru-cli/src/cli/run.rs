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
use crate::credentials::{
    Alias, Confirm, CredentialStore, Description, Entry, Family, HarnessKeys, Instance, Listing,
    OsKeyring, Reach, Secret, StoreError, tool_scope_at,
};
use crate::failure::{Classified, Exit, SessionEvidence};
use crate::providers::{ModelTable, ProviderKind};
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
            // A bare `zaru` reaching this surface is one nobody is watching:
            // `terminal::take_over` answers the other reader before `execute`
            // is called. Printing the usage is what it has always printed
            // through a pipe, byte for byte, and it mints nothing -- asking
            // the binary a question still writes nothing to the user's home.
            Request::Session => Outcome::printed(help::lines(self.version)),
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
            Request::Resume { id } => self.resume(id),
            Request::Continue => self.resume_latest(),
            Request::NotesTokens => self.notes_tokens(),
            Request::NotesUse { alias } => self.notes_use(alias),
            Request::NotesTokensAdd { alias, host, apex } => {
                self.notes_tokens_add(alias, host, *apex)
            }
            Request::NotesTokensDescribe { alias, text } => self.notes_tokens_describe(alias, text),
            Request::NotesTokensRemove { alias } => self.notes_tokens_remove(alias),
            Request::ProviderKeysRemove { kind } => self.provider_keys_remove(*kind),
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
        let here = match crate::tools::WorkingDirectory::of_this_process() {
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
    fn resume(&self, id: &SessionId) -> Outcome {
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
                // **A bare resume that printed what it was asked for
                // succeeded.** It exited with the no-provider refusal's `2` or
                // `4` until 2026-09-05, which is the asymmetry ADR-0010 D4's
                // own Update recorded: "the refusal's sentence is about
                // *running a task*, and a bare `--resume` asks for no task, so
                // the code was arguably always slightly wrong for it and is now
                // visibly so". Nothing was asked of a provider, so a refusal
                // about there being none is an answer to a question nobody put
                // — and ADR-0016 D5's reader is a wrapper, for which a non-zero
                // code means the thing it asked for did not happen. It did.
                //
                // The terminal path has exited `0` on `/exit` since the shell
                // landed; this is the same operation reaching the other kind of
                // reader, and the two now agree. Decided under Jeshua's
                // directive of 2026-09-05 and written as an accepted Update on
                // ADR-0010 D4, open to his veto.
                //
                // `--resume` naming a session that does not exist is untouched
                // and still fails: that one *is* a refusal, and it is what
                // `a_resume_of_a_session_that_is_not_there_...` holds.
                Outcome::printed(render::resumed(id, &restored))
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
    fn resume_latest(&self) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let store = match self.store() {
            Ok(store) => store,
            Err(outcome) => return *outcome,
        };
        // ADR-0010 D4 is "the most recent session **in this directory**", and
        // `most_recent_in` is the one place that sentence is implemented --
        // `terminal::open::most_recent` is its other caller. Until 2026-09-06
        // this function and that one each took `ids().last()`, which is a
        // recency test where the clause asks for a locality one; the register
        // recorded the cause as "structural rather than one call site".
        let here = match crate::tools::WorkingDirectory::of_this_process() {
            Ok(here) => here,
            Err(failure) => return Outcome::failed(Surface::working_directory(&failure)),
        };
        match crate::session::most_recent_in(&store, here.root()) {
            Ok(Some(id)) => self.resume(&id),
            Ok(None) => Outcome::failed(surface.no_session_to_continue()),
            Err(failure) => Outcome::failed(surface.continuing(&failure)),
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
        let mut offered = String::new();
        if let Err(failure) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut offered) {
            return Outcome::failed(Surface::key_not_readable(kind, &failure));
        }
        self.store_a_provider_key(kind, trim_one_line_ending(&offered))
    }

    /// Seal an offered provider key into the store, however it was read.
    ///
    /// # Both spellings reach this, and that is what makes them one operation
    ///
    /// [ADR-0015] D2's "**a namespace has two entry points, and they are one
    /// operation**" is a claim about a credential write as much as about a
    /// listing. Out of a session the bytes come from standard input, for the
    /// reason [`Run::provider_keys_add`] above states; inside one they come
    /// from [ADR-0011] D3's masked question, because a terminal in raw mode
    /// has no standard input to hand them. **Everything after the bytes is
    /// this function and is shared**: the refusal, the alias, the description,
    /// the sealing, the store and the two lines a user reads.
    ///
    /// Splitting here rather than lower is deliberate. A shared function that
    /// began after `Secret::provider` would leave each caller free to refuse a
    /// key differently, and what a user is told about a key with a space in it
    /// is not a property of which surface they typed it at.
    ///
    /// **Nothing this function does echoes the key.** It is handed over,
    /// sealed, and what is printed afterwards is the alias and the kind.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub(crate) fn store_a_provider_key(&self, kind: ProviderKind, offered: &str) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);

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

    /// [ADR-0007] D7's `describe`, the fourth of that clause's five surfaces.
    ///
    /// # The refusal a person meets here has never been reachable before
    ///
    /// The text is a user's, so [`Description::new`] can refuse it — and
    /// until now the only description this harness composed was
    /// [`notes_entry`]'s machine-made sentence, which cannot carry a control
    /// character. That is why [`undecided_description`] exists and reports a
    /// **defect**: for a sentence this module composed, a refusal is this
    /// harness's fault.
    ///
    /// **This path must not go near it.** A typed newline is the user's, and
    /// `From<DescriptionRefused> for Classified` already classifies it as the
    /// user's with a remedy naming what to remove. Sending it through the
    /// defect reporter instead would exit 70, "a defect in Zaru", for somebody
    /// who pressed Return in the wrong place — [ADR-0016] D3's "never present
    /// a defect as a user error" inverted, which is a mistake this clause's
    /// own Status tracking already records twice, on the apex path and on
    /// `use`.
    ///
    /// The store is opened for **writing**, unlike `notes tokens`, because
    /// setting a description rewrites the file.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    fn notes_tokens_describe(&self, alias: &Alias, text: &str) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let description = match Description::new(text) {
            Ok(description) => description,
            // The user's, and it says which character to take out.
            Err(refusal) => return Outcome::failed(refusal.into()),
        };
        let mut store = match Self::store_for_writing() {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match store.describe(alias, &description, Family::Notes) {
            Ok(()) => Outcome::printed(vec![format!(
                "\"{alias}\" now reads: {}",
                description.as_str()
            )]),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// [ADR-0007] D7's `rm`, the fifth of that clause's five surfaces.
    ///
    /// # It will remove the composer's token, and says so when it does
    ///
    /// D4 flags exactly one token `composer`, and this removes it if that is
    /// the alias named: revoking a credential is the person's to do, and a
    /// store that refused would leave someone unable to remove a token they
    /// had already revoked on the server.
    ///
    /// What it must not do is let that pass silently. The listing afterwards
    /// cannot show the role is unheld, because the row that carried it is the
    /// row that went, so the outcome says so — and says what the composer
    /// reads with now, which is the three-case reading of
    /// [`composer_token`](crate::credentials::composer_token) rather than a
    /// guess. It is asked **after** the write, so it is a reading of the store
    /// as it now stands.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    fn notes_tokens_remove(&self, alias: &Alias) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let mut store = match Self::store_for_writing() {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match store.remove(alias, Family::Notes) {
            Ok(removed) => {
                let mut lines = vec![format!("removed \"{alias}\"; {SEALED_VALUE_IS_GONE}")];
                if removed.held_composer_role {
                    lines.push(format!("  {}", composer_role_now_unheld(&store)));
                }
                Outcome::printed(lines)
            }
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// The provider half of D7's `rm`, over the same store operation.
    ///
    /// It takes the kind and composes the alias, because a provider key's
    /// alias is `provider.<kind>` and is not the user's to choose.
    fn provider_keys_remove(&self, kind: ProviderKind) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let alias = kind.credential_alias();
        let mut store = match Self::store_for_writing() {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match store.remove(&alias, Family::Provider(kind)) {
            Ok(_) => Outcome::printed(vec![format!(
                "removed the `{kind}` key; {SEALED_VALUE_IS_GONE}"
            )]),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// The store, opened for writing.
    ///
    /// Three commands resolve the root and open it identically; written once
    /// so the sentence a person reads when their home directory cannot be
    /// resolved is one sentence. It hands back the store's own refusal rather
    /// than a rendered outcome, so the caller classifies it the same way it
    /// classifies everything else the store can say.
    fn store_for_writing() -> Result<CredentialStore, StoreError> {
        CredentialStore::open(CredentialStore::default_root()?)
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

    /// [ADR-0007] D7's `use`, the third of that clause's five surfaces.
    ///
    /// # What a person running this sees today, and why that is the point
    ///
    /// D7 is "`/notes use <alias>` — move the composer role to another
    /// token", and this calls `CredentialStore::move_composer_role`, which is
    /// the operation that clause names. It **moves**: the incumbent's role is
    /// revoked and the named token's granted in one store write, so `use` can
    /// be run as often as a person has tokens.
    ///
    /// It deliberately does **not** call `grant_composer_role`, which refuses
    /// whenever any token holds the role. It did until 2026-09-14, and the
    /// consequence was that `use` could succeed at most **once on a machine,
    /// ever** — a dead end whose first victim is the person adding their
    /// second token. The two operations stay two and each says which it is:
    /// one refuses a second holder, the other replaces the holder on purpose.
    ///
    /// **It refuses every token that exists, naming the tool.** The store
    /// refuses the composer role to a credential whose cached `tools/list`
    /// reaches outside [ADR-0006] D4's set, and every Nuclear Notes token
    /// measured on 2026-09-14 grants 94 tools — the whole surface. So the
    /// ordinary outcome of this command is a refusal that names the first
    /// offending tool and says what the composer's credential may carry.
    ///
    /// That is not a broken command. It is the store holding D4 correctly,
    /// made **readable**: before this, the refusal was a code path no surface
    /// reached, so a person could not find out why their token was not the
    /// composer's. The composer meanwhile reads with the single stored token
    /// under the 2026-09-14 reading — see
    /// [`composer_token`](crate::credentials::composer_token) — so refusing
    /// the role does not leave the strip empty.
    ///
    /// The store is opened for **writing**, unlike `notes tokens`, because
    /// granting a role rewrites the file.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    fn notes_use(&self, alias: &Alias) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let root = match CredentialStore::default_root() {
            Ok(root) => root,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        let mut store = match CredentialStore::open(root) {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };
        match store.move_composer_role(alias) {
            Ok(()) => Outcome::printed(vec![format!(
                "\"{alias}\" now carries the composer role; the hint strip searches with it."
            )]),
            Err(failure) => Outcome::failed(
                surface.credential_store(&failure, SessionEvidence::NoSessionExists),
            ),
        }
    }

    /// [ADR-0007] D7's `add`, the second of that clause's five surfaces.
    ///
    /// # The order is the record's and not a convenience
    ///
    /// Read the token, **reach the instance and read `tools/list`**, build the
    /// entry with that scope on it, then store it. D6 wants "one `tools/list`
    /// per token at attach"; D8 wants an apex confirmation that states "what it
    /// grants", and `CredentialStore::add` composes that sentence out of the
    /// entry's own scope. Storing first and reading the scope afterwards would
    /// ask the user to accept a credential the prompt said grants zero tools.
    /// See [`tool_scope_at`](crate::credentials::tool_scope_at).
    ///
    /// # The confirmation is asked at the terminal, and refusing is the answer
    /// when it cannot be
    ///
    /// The token arrives on standard input, so standard input is spent by the
    /// time D8's question needs asking. It is asked on the controlling
    /// terminal instead. **On a machine with no terminal — a pipeline, a
    /// runner, a container — no confirmer is supplied at all**, and the store
    /// refuses: clause 11's own words are that apex "requires a confirmation
    /// that refuses rather than defaults when it cannot be asked". The refusal
    /// is the correct outcome rather than a limitation, and it is the outcome
    /// on every headless machine.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    fn notes_tokens_add(&self, alias: &Alias, host: &str, apex: bool) -> Outcome {
        let mut offered = String::new();
        if let Err(failure) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut offered) {
            return Outcome::failed(Surface::token_not_readable(alias, &failure));
        }
        // Exactly one trailing line ending, and only if it is there -- the same
        // rule `providers keys add` applies, because the same `printf` and the
        // same `echo` reach both. It is the same *function* as of 2026-09-14,
        // rather than the same rule typed twice: inside one crate a rule lives
        // in one place, and two copies of a trimming rule are two places a
        // credential can be mangled differently.
        let offered = trim_one_line_ending(&offered);

        let secret = match a_notes_secret(alias, host, offered) {
            Ok(secret) => secret,
            Err(outcome) => return *outcome,
        };

        // **This runtime is now around the scope read alone**, and that is what
        // the split is for. Until 2026-09-14 it also spanned the store write
        // and D8's confirmation, and the in-session spelling therefore could
        // not reach any of it: a shell is already inside
        // `terminal::open`'s own `block_on`, and a `block_on` inside a
        // `block_on` panics. Everything after this call is
        // [`Run::store_a_notes_token`], which builds no runtime at all, so the
        // pump awaits the scope on the runtime it is already running under and
        // then calls the same storing function this line falls through to.
        //
        // The `expect` is the one `terminal::open` already carries and is
        // deliberately not a classification: a reactor that will not register
        // with the operating system is ADR-0016 D3's defect, caught by the
        // boundary in `main`, and inventing a user-correctable class for it
        // would be that record's "never present a defect as a user error".
        let runtime = crate::compose::turn::runtime()
            .expect("a current-thread runtime with the io and time drivers");
        let scope = runtime.block_on(tool_scope_at(host, &secret));

        let terminal = TerminalConfirm::available();
        let confirmer = terminal.as_ref().map(|tty| tty as &dyn Confirm);
        self.store_a_notes_token(alias, host, apex, secret, scope, confirmer)
    }

    /// Seal an offered Nuclear Notes token into the store, however it was read
    /// and wherever [ADR-0007] D8's confirmation is asked.
    ///
    /// # Both spellings reach this, and that is what makes them one operation
    ///
    /// The twin of [`Run::store_a_provider_key`], for the same [ADR-0015] D2
    /// reason and split at the same seam. Out of a session the bytes come from
    /// standard input and the scope read runs on a runtime this module builds;
    /// inside one they come from [ADR-0011] D3's masked question and the scope
    /// is awaited on the shell's own runtime. **Everything after the bytes is
    /// this function and is shared**: the unreachable-instance refusal, the
    /// entry, the store, D8's gate and the three lines a user reads. It takes
    /// the scope as a `Result` rather than a `ToolScope` for exactly that
    /// reason — a caller free to word "that host did not answer" its own way
    /// is a caller free to word it differently, and which surface a person
    /// typed at is not a property of whether their instance replied.
    ///
    /// # It builds no runtime, and that is the property rather than an
    /// incidental
    ///
    /// The in-session spelling was unreachable for three measured reasons on
    /// 2026-09-14, and the first was a `block_on` inside the shell's own. This
    /// function is synchronous and names no `Runtime`, so there is nothing
    /// here for a second one to be built by;
    /// `the_terminal_module_names_no_runtime_and_no_block_on` pins the other
    /// half, that the module which calls this names none either.
    ///
    /// # The confirmer is the caller's, and the two are different objects
    ///
    /// `None` on a machine with no terminal — a pipeline, a runner, a
    /// container — and the store then refuses, which is clause 11's
    /// "a confirmation that refuses rather than defaults when it cannot be
    /// asked". Out of a session it is `TerminalConfirm`, on `/dev/tty`. Inside
    /// one that descriptor belongs to a terminal in raw mode, so it is the
    /// pane's own confirmer instead — see
    /// `terminal::driver::PaneConfirm`, named in prose because it is
    /// reached through this trait object and not by this module.
    ///
    /// **Nothing this function does echoes the token.** It is handed over,
    /// sealed, and what is printed afterwards is the alias, the host and a
    /// count.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub(crate) fn store_a_notes_token(
        &self,
        alias: &Alias,
        host: &str,
        apex: bool,
        secret: Secret,
        scope: Result<crate::credentials::ToolScope, crate::credentials::ReachFailure>,
        confirmer: Option<&dyn Confirm>,
    ) -> Outcome {
        let surface = Surface::new(self.version, self.report_at);
        let scope = match scope {
            Ok(scope) => scope,
            Err(failure) => {
                return Outcome::failed(Surface::notes_unreachable(alias, host, &failure));
            }
        };
        let granted = scope.count();
        let entry = match notes_entry(alias, host, apex, secret, scope) {
            Ok(entry) => entry,
            Err(EntryUndecided::Description(refusal)) => {
                return Outcome::failed(undecided_description(
                    self.version,
                    self.report_at,
                    &refusal,
                ));
            }
            Err(EntryUndecided::Entry(refusal)) => {
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
        let keyring = OsKeyring::for_store(&root);
        let keys = HarnessKeys::from_process(&keyring);
        let mut store = match CredentialStore::open(root) {
            Ok(store) => store,
            Err(failure) => {
                return Outcome::failed(
                    surface.credential_store(&failure, SessionEvidence::NoSessionExists),
                );
            }
        };

        match store.add(entry, &keys, confirmer) {
            Ok(()) => Outcome::printed(vec![
                format!("stored a Nuclear Notes token under the alias `{alias}`."),
                format!("  {host} reported {granted} tool(s), and that is what the store cached."),
                "  the value is sealed and is not printed by any command.".to_owned(),
            ]),
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

/// What `rm` says about the value it took with the credential.
///
/// Authored 2026-09-14 and recorded on [ADR-0007]'s amendments page as
/// Jeshua's to veto. It is a named constant rather than a literal at the two
/// call sites so that the Notes half and the provider half cannot come to say
/// different things about one store operation.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
const SEALED_VALUE_IS_GONE: &str = "its sealed value is gone from the store.";

/// What reads your notes now that the composer role is unheld.
///
/// # Three cases, and they are the store's rather than this function's
///
/// [ADR-0007] D4 says exactly one token carries the role and does not say what
/// happens when none does — which is the state of every machine that exists,
/// and which the amendment of 2026-09-14 answers in three cases: a
/// role-carrying token wins; failing that a **lone** stored Nuclear Notes
/// token serves the composer's reads; failing that nothing serves and the
/// strip says so. `composer_token` is the implementation of exactly those
/// three, so this asks it rather than restating them — a sentence composed
/// from a second reading of the store would be a second answer to one
/// question.
///
/// It is called **after** the removal, so what it describes is the store as it
/// now stands rather than as it was.
///
/// The three sentences are authored and are recorded on [ADR-0007]'s
/// amendments page as Jeshua's to veto.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
fn composer_role_now_unheld(store: &CredentialStore) -> String {
    match crate::credentials::composer_token(store) {
        // Case 2: one token left, and it serves without carrying the role.
        Some((alias, _)) => format!(
            "nothing carries the composer role now; the hint strip searches with \"{alias}\", the \
             only token stored."
        ),
        // Cases 3 and the empty store, told apart because the remedies differ:
        // one person names a token, the other has none to name.
        None if store.listed(Listing::Notes).is_empty() => {
            "nothing carries the composer role now, and no token is stored; the hint strip has \
             nothing to search with."
                .to_owned()
        }
        None => "nothing carries the composer role now and several tokens are stored, so the hint \
                 strip has nothing to search with; name one with `zaru notes use <alias>`."
            .to_owned(),
    }
}

/// What building a Nuclear Notes entry can refuse on, so the caller can render
/// each with the sentence it already has.
#[derive(Debug)]
pub(crate) enum EntryUndecided {
    /// The description this function composed was not one the store takes.
    Description(crate::credentials::DescriptionRefused),
    /// The entry was not one the store takes.
    Entry(crate::credentials::EntryRefused),
}

/// The entry `notes tokens add` stores, from the scope the instance reported.
///
/// # Why this is a function and not four statements in its caller
///
/// **It is the only place the measured scope and the entry meet**, and the
/// defect it exists to make checkable is a single dropped call: an entry built
/// without `with_tools` carries `ToolScope::default()`, so ADR-0007 D8's
/// confirmation tells the user the credential grants nothing, and the
/// description the agent reads says the same. Neither is visible without a
/// server unless the joining is a function, so it is one, and a check drives it
/// with a scope of a known size.
///
/// The description names the count for the same reason D8's prompt does: a
/// stored credential whose own description understates it is metadata that will
/// be confidently acted upon, which that record's Negative section names.
pub(crate) fn notes_entry(
    alias: &Alias,
    host: &str,
    apex: bool,
    secret: Secret,
    scope: crate::credentials::ToolScope,
) -> Result<Entry, EntryUndecided> {
    let description = Description::new(format!(
        "the Nuclear Notes token for {host}, granting {} tool(s)",
        scope.count()
    ))
    .map_err(EntryUndecided::Description)?;
    // ADR-0007 D8: instance-locked unless the user explicitly chose otherwise,
    // and the word they typed is the only thing that chooses.
    let reach = if apex {
        Reach::Apex
    } else {
        Reach::InstanceLocked(Instance::new(host))
    };
    Ok(Entry::notes(alias.clone(), description, secret, reach)
        .map_err(EntryUndecided::Entry)?
        .with_tools(scope))
}

/// The bytes of a Nuclear Notes token, refused in one place.
///
/// Split out for [`Run::store_a_provider_key`]'s own reason: a shared function
/// that began after `Secret::notes` would leave each surface free to refuse a
/// token differently, and what a user is told about a value with no notes
/// prefix on it is not a property of where they typed it. This is the one
/// refusal `Secret::provider` has no analogue of, so it is the one most worth
/// having in a single place.
///
/// **The refusal carries no part of the value**: `SecretRefused` is `Copy` and
/// therefore cannot.
///
/// It answers with the [`Outcome`] rather than the refusal because both
/// callers do the same thing with it, and a second `match` at the second call
/// site is a second chance to classify it differently.
///
/// # Errors
///
/// The [`Outcome`] the caller should return, boxed for the reason
/// [`crate::terminal::open::shell_for`]'s and [`crate::compose::turn::prepare`]'s
/// errors are: an `Outcome` carries the lines and the ADR-0016 exit, which is
/// far larger than a [`Secret`], and `clippy::result_large_err` refuses a
/// `Result` shaped that way. One allocation on the refusal path, and none on
/// the path a token is actually stored on.
pub(crate) fn a_notes_secret(
    alias: &Alias,
    host: &str,
    offered: &str,
) -> Result<Secret, Box<Outcome>> {
    Secret::notes(offered).map_err(|refusal| {
        Box::new(Outcome::failed(Surface::token_refused(
            alias, host, &refusal,
        )))
    })
}

/// [ADR-0007] D8's confirmation, asked on the controlling terminal.
///
/// # Why not standard input
///
/// The token is read from standard input, so by the time D8's question needs
/// asking there is nothing left on it: a pipe is at end of file and a here-doc
/// is spent. Reading the answer from the same descriptor would either block
/// forever or read end-of-file and take it for a refusal the user never gave.
/// Exactly one trailing line ending, and only if it is there.
///
/// **Named once because two surfaces read a credential from a stream and a
/// third now reads one from a terminal**, and a rule that decides how many
/// bytes of a secret survive is not a rule to have two copies of. It was
/// written twice, identically, until 2026-09-14.
///
/// The trailing newline is trimmed rather than refused because it is not the
/// user's: `printf '%s\n' "$KEY" | zaru …` and pressing return in a terminal
/// both add one, and refusing it would make every ordinary way of supplying a
/// credential fail. **Exactly one**, so a *second* line ending is the user's
/// and is refused by `Secret::provider`'s `value.trim() != value`, which is
/// the distinction that matters.
///
/// # A dated correction, because the sentence this replaced was false
///
/// It read: "Whitespace the user actually typed is still refused by
/// `Secret::provider` … a key with a space in the middle of it is a paste
/// artefact worth telling them about." **Measured 2026-09-14 by driving the
/// binary: it is not.** `Secret::provider` refuses an empty value, a control
/// character, and *surrounding* whitespace — `value.trim() != value` — and a
/// key with a space in the **middle** of it is stored without comment. The
/// rule is correct as written and only the prose about it was wrong, so
/// nothing is widened here: what a credential may contain is
/// [ADR-0007](https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store)'s
/// to say, and inventing an interior-whitespace rule would be this crate
/// deciding somebody else's key format — the same reason that record gives for
/// declaring a provider kind rather than deriving it from a prefix.
///
/// `\r\n` and `\n` both go; a lone `\r` stays, because nothing this harness
/// reads from produces one and stripping it would be inventing a rule.
fn trim_one_line_ending(offered: &str) -> &str {
    let without_newline = offered.strip_suffix('\n').unwrap_or(offered);
    without_newline
        .strip_suffix('\r')
        .unwrap_or(without_newline)
}

/// `/dev/tty` is the descriptor that is still the person, whatever standard
/// input was redirected to.
///
/// # A machine with no terminal supplies no confirmer at all
///
/// [`Self::available`] answers `None` there, and the caller passes `None` to
/// `CredentialStore::add`, which refuses with its own sentence naming the
/// alias. That is [ADR-0007] clause 11's requirement in its own words — apex
/// "requires a confirmation that refuses rather than defaults when it cannot be
/// asked" — and it is what happens on every runner, pipeline and container.
/// **An implementation that answered `false` here would be a different thing**:
/// a decline the user made rather than a question nobody could ask, and the
/// store has separate refusals for the two.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Debug)]
struct TerminalConfirm {
    tty: std::path::PathBuf,
}

impl TerminalConfirm {
    /// The controlling terminal, if this process has one.
    fn available() -> Option<Self> {
        let tty = std::path::PathBuf::from("/dev/tty");
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tty)
            .ok()
            .map(|_| Self { tty })
    }
}

impl Confirm for TerminalConfirm {
    /// Ask, and take anything but an explicit yes for a no.
    ///
    /// **The default is refusal**, which is D8's "never silent, never a
    /// default" read the only way that is safe: a reader who presses return
    /// without reading has not accepted a credential with no instance
    /// boundary.
    fn confirm_apex(&self, alias: &Alias, grants: &str) -> bool {
        use std::io::{BufRead, Write};
        let Ok(mut terminal) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.tty)
        else {
            return false;
        };
        if write!(
            terminal,
            "`{alias}` has {grants}.\nThis is never stored without being asked. Type `yes` to \
             store it: "
        )
        .and_then(|()| terminal.flush())
        .is_err()
        {
            return false;
        }
        let mut answer = String::new();
        if std::io::BufReader::new(terminal)
            .read_line(&mut answer)
            .is_err()
        {
            return false;
        }
        answer.trim() == "yes"
    }
}
