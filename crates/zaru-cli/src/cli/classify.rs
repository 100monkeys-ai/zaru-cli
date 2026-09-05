// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What class each failure the command surface can raise belongs to.
//!
//! # Why this is here rather than in `failure::classify`
//!
//! [ADR-0016]'s own Update of 2026-09-04 named the shape this module is the
//! first instance of: "**A refusal of a caller-passed number takes its class
//! from the number's provenance**... A classification written as a pure
//! function of the error value would be wrong for a reason no check could
//! catch." The same is true one level up. `SessionError::Io` raised while a
//! *user* named a session is the user's; the same value raised while the
//! harness wrote a file of its own is not, and nothing on the value says
//! which.
//!
//! [`crate::failure::classify`] maps the enums whose class a record states
//! outright, and deliberately maps none of the four whose class cannot be read
//! off the value. **It is not edited here.** What is added is a mapping the
//! *command surface* can make honestly, because at this seam the provenance is
//! known: every path below begins with something the person in front of the
//! terminal typed, so a failure reaching it is about their machine, their
//! files or their words.
//!
//! Written as a **proposed Update** on ADR-0016 rather than as a change to its
//! module, under a delegated coordinator ruling of 2026-09-05 open to Jeshua's
//! veto. It is the first instance of that record's own "class by provenance"
//! reading, and a person decides whether the reading stands.
//!
//! # What stays unmapped, and it is named rather than defaulted
//!
//! Four things reaching this surface still have no readable class and each is
//! carried as a defect **with its own sentence saying why**, rather than being
//! forced into a row:
//!
//! - `StoreError::Malformed` — the user's if a person hand-wrote
//!   `credentials.json`, ours if our own writer did, and the store is the only
//!   writer. ADR-0016's Update lists it and asks for an accepted reading.
//! - `StoreError::Seal` and anything carrying a `SealFailure` — a port
//!   failure, whose class belongs to the port's implementation, of which there
//!   is none.
//! - `TranscriptError::Malformed` and `CheckpointError::Malformed` — the
//!   harness is the only writer of both files, so a file that does not parse
//!   is either a defect in this harness or a file somebody edited by hand, and
//!   no record says which.
//!
//! The residue is a defect **on purpose**: [ADR-0016] D1's Negative section
//! says "a misclassified error is worse than an unclassified one because the
//! presentation actively misleads", and a defect report says plainly that this
//! is not something the reader can configure, which is the honest answer when
//! the harness genuinely does not know.
//!
//! # `io::ErrorKind` is not read
//!
//! ADR-0016's Update says a mapping of I/O kinds to D1's rows is a clause that
//! record would need, and that `io::ErrorKind` is `#[non_exhaustive]` so no
//! wildcard-free match over it exists. **No such mapping is invented here.**
//! Every I/O failure at this surface is the user's for a reason that does not
//! need the kind: the path was named by the user or derived from their `$HOME`,
//! and the remedy is the path and the operation, which the error already
//! carries.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::layers::LoadFailure;
use crate::cli::refusal::CommandRefused;
use crate::config::{Key, Schema};
use crate::credentials::{
    CREDENTIAL_KEY_VARIABLE, SealingError, SealingKey, SecretRefused, StoreError,
};
use crate::failure::{
    Action, Classified, DefectReport, Location, Remedy, SessionEvidence, Statement, Wait,
};
use crate::providers::{ModelAlias, ModelId, ProviderKind};
use crate::runtime::TierRefused;
use crate::session::{PruneFailure, ResumeFailure, SessionError, SessionIdRefused};

/// A remedy of one described action.
fn act(sentence: String) -> Remedy {
    Remedy::one(Action::described(Statement::sanitised(sentence)))
}

/// A remedy whose action is a command the reader can paste.
///
/// **The command surface is what makes this constructor usable at all.**
/// `failure::classify`'s own documentation says every remedy there is a
/// described action because "the command surface is ADR-0015's and does not
/// exist". It does now, so a remedy may name a command — and it names only a
/// command `--help` lists, which is what keeps
/// `every_command_the_help_text_lists_is_one_the_parser_accepts` load-bearing
/// for the remedies as well as for the help.
fn run(sentence: &str, command: &str) -> Remedy {
    Remedy::one(
        Action::runnable(Statement::sanitised(sentence.to_owned()), command)
            .expect("the commands this crate suggests carry no control character"),
    )
}

/// A user-correctable failure: the refusal's own words, and what to change.
fn correctable(refusal: &impl core::fmt::Display, remedy: Remedy) -> Classified {
    Classified::UserCorrectable {
        statement: Statement::sanitised(refusal.to_string()),
        remedy,
    }
}

/// A failure whose class no record states, carried as what it is.
///
/// The version and where to report come from the caller rather than from this
/// module's own `env!`, so a report carries the version the *binary* was built
/// with. The location is this file and the line of the arm that raised it, so
/// a maintainer following the report lands on the comment explaining why that
/// arm cannot be read — which is where the reason lives, because
/// [`DefectReport`] has no field for one and adding one would be editing
/// [ADR-0016]'s module from outside it.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn undecided(version: &str, report_at: &str, session: SessionEvidence, line: u32) -> Classified {
    Classified::Defect(DefectReport::new(
        version,
        report_at,
        Location {
            file: file!().to_owned(),
            line,
            column: 0,
        },
        session,
    ))
}

/// Everything the command surface can be handed, classified.
///
/// Every method is a wildcard-free match, so a variant added to any of these
/// enums fails to compile here rather than taking a neighbouring class.
pub struct Surface<'a> {
    version: &'a str,
    report_at: &'a str,
}

impl<'a> Surface<'a> {
    /// The classifier for one run of the binary.
    #[must_use]
    pub const fn new(version: &'a str, report_at: &'a str) -> Self {
        Self { version, report_at }
    }

    /// A command line the grammar did not admit.
    ///
    /// **Every variant is the user's**, which is what makes this the one
    /// mapping in the crate that needs no provenance argument: a command line
    /// is by construction something the person in front of the terminal typed.
    #[must_use]
    pub fn command(&self, refusal: &CommandRefused) -> Classified {
        let remedy = match refusal {
            CommandRefused::NotText { .. } => run(
                "check how your shell expanded that word; every word `zaru` takes is text",
                "zaru --help",
            ),
            CommandRefused::UnknownCommand { nearest, .. } => act(format!(
                "run `zaru {nearest}`, or `zaru --help` for everything this harness runs"
            )),
            CommandRefused::NamespaceNotBuilt { namespace } => act(format!(
                "nothing can be done about `{namespace}` from here; `zaru --help` lists what this \
                 harness does run"
            )),
            CommandRefused::VerbMissing { namespace } => {
                act(format!("give it one of: {}", namespace.verbs().join(", ")))
            }
            CommandRefused::UnknownVerb {
                namespace, nearest, ..
            } => match nearest {
                Some(nearest) => act(format!("run `zaru {namespace} {nearest}`")),
                None => act(format!("`zaru {namespace}` takes no verb")),
            },
            CommandRefused::UnexpectedWord { command, .. } => {
                act(format!("run `zaru {command}` with nothing after it"))
            }
            CommandRefused::ArgumentMissing { command, argument } => {
                act(format!("run `zaru {command}` with {argument} after it"))
            }
            CommandRefused::UnknownFlag { nearest, .. } => act(format!(
                "the nearest flag this harness takes is `{nearest}`"
            )),
            CommandRefused::FlagNeedsValue { flag, value } => {
                act(format!("write `{flag} {value}`, or `{flag}={value}`"))
            }
            CommandRefused::FlagTakesNoValue { flag } => act(format!("write `{flag}` on its own")),
            CommandRefused::FlagRepeated { flag, .. } => act(format!(
                "give `{flag}` once, with the value you meant; nothing here decides which of two \
                 flags wins"
            )),
            CommandRefused::ResumeAndContinue => act(
                "give `--resume <id>` to name a session, or `--continue` to take the most recent \
                 one"
                .to_owned(),
            ),
            CommandRefused::RequestFlagWithCommand { flag, command } => act(format!(
                "run `{flag}` on its own, or `zaru {command}` on its own"
            )),
            CommandRefused::UnusableKey(_) => {
                act("a configuration key is dotted text, as in `runtime.tier`".to_owned())
            }
            CommandRefused::UnusableSessionId(_) => run(
                "`zaru sessions list` prints every id on this machine",
                "zaru sessions list",
            ),
        };
        correctable(refusal, remedy)
    }

    /// A key `config explain` was asked about that no record declares.
    ///
    /// Associated rather than a method, because it needs no version and no
    /// report URL: nothing about a key nothing declares is ours.
    #[must_use]
    pub fn undeclared_key(key: &Key, schema: &Schema) -> Classified {
        let statement = format!(
            "no record declares the configuration key {key}, so there is nothing to explain about \
             it; explaining it would print five layers of `(not set)`, which reads as a key that \
             exists and that nobody has set"
        );
        let remedy = match schema.nearest(key.as_str()) {
            Some(nearest) => act(format!("run `zaru config explain {nearest}`")),
            None => act("this binary declares no configuration key at all".to_owned()),
        };
        Classified::UserCorrectable {
            statement: Statement::sanitised(statement),
            remedy,
        }
    }

    /// A configuration fold this binary could not complete.
    ///
    /// The refusal arm is ADR-0014's own, already classified by
    /// [`crate::failure::classify`], and is passed through rather than
    /// re-decided.
    ///
    /// **The source arm is the user's, and the provenance is what says so.**
    /// It was a defect until 2026-09-05, when it was unreachable: the two
    /// sources were a compiled constant and a parsed flag and neither could
    /// fail. Layers 2 and 3 read files now, and the two files a
    /// [`Files`](crate::cli::Files) can be built from are `~/.zaru/config.toml`
    /// under the user's own home and `./zaru.toml` in the directory they ran
    /// the harness in. **Neither is ever written by this harness** — ADR-0014
    /// D6 keeps the loader out of the user's file, and ADR-0009 D6 lets `zaru
    /// init` write the project's exactly once, from a constant a check parses
    /// and folds — so a file that does not parse is a file a person edited, and
    /// [ADR-0016] D1 makes that user-correctable.
    ///
    /// `meta.toml` is deliberately not covered by this reading and stays a
    /// defect when it is malformed, because that file's only writer is this
    /// harness. The two are told apart by *which port* failed, not by the value.
    ///
    /// Associated rather than a method: nothing about a file a person wrote is
    /// ours, so no version and no report URL is needed. Written as an Update on
    /// [ADR-0016] beside the `command-surface` arc's, under a delegated
    /// coordinator ruling of 2026-09-05 open to Jeshua's veto.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn load(failure: &LoadFailure) -> Classified {
        match failure {
            LoadFailure::Refused(refusal) => Classified::from(refusal.clone()),
            LoadFailure::Source(source) => correctable(
                source,
                act(
                    "the message above names the file and, where it parsed far enough to say, the \
                     line and column; edit that file. Nothing in this harness writes either \
                     configuration file except `zaru init`, which writes `./zaru.toml` once when \
                     it is absent"
                        .to_owned(),
                ),
            ),
        }
    }

    /// Nothing configured a model for the alias every task starts from.
    ///
    /// The user's, and the remedy is real: setting `model.default` or
    /// `ZARU_MODEL_DEFAULT` moves the refusal to the next honest one.
    ///
    /// **ADR-0016 D2's own worked remedy is deliberately not reused.** It
    /// offers `zaru config set provider.anthropic.key <key>` -- a command this
    /// harness does not have, and whose existence is an open question against
    /// ADR-0014 D4 -- and `ZARU_ANTHROPIC_KEY`, which that record's transform
    /// cannot produce from `provider.anthropic.key`. Both are raised on
    /// ADR-0016's Status tracking for its author; neither is quoted here.
    #[must_use]
    pub fn no_model_for_the_default_alias() -> Classified {
        Classified::UserCorrectable {
            statement: Statement::sanitised(format!(
                "no model is configured for the alias `{}`, so there is no provider to ask",
                ModelAlias::Default
            )),
            remedy: act(format!(
                "set `{}` in ~/.zaru/config.toml, or `{}` in the environment, or give `--model \
                 <identifier>` for one run",
                ModelAlias::Default.key(),
                crate::config::environment::variable_name(&ModelAlias::Default.key())
            )),
        }
    }

    /// A model resolved and this build carries no client that can reach it.
    ///
    /// See [`crate::cli::run::Run`]'s own documentation for why this is D1's
    /// capability class, why the class does not fit, and where the missing row
    /// is raised.
    #[must_use]
    pub fn no_provider_client(&self, model: &ModelId) -> Classified {
        Classified::Capability {
            statement: Statement::sanitised(format!(
                "the alias `{alias}` resolves to {model:?}, and nothing wires a provider client \
                 to a loop in this build. A `{gemini}` client exists as of 2026-09-05 and the \
                 other {remaining} of ADR-0012 D3's {total} kinds have none; what is missing for \
                 every kind alike is the wiring, which is why this refusal is the same whichever \
                 one the alias resolves to",
                alias = ModelAlias::Default,
                model = model.as_str(),
                gemini = ProviderKind::Gemini,
                remaining = ProviderKind::ALL.len() - 1,
                total = ProviderKind::ALL.len(),
            )),
            offered_by: crate::runtime::Tier::Bare,
        }
    }

    /// Standard input could not be read while adding a provider key.
    ///
    /// Environmental: the reader did not choose their pipe's behaviour, and
    /// nothing they type at the harness fixes a closed descriptor. **The
    /// partial read is discarded and never quoted** — whatever arrived before
    /// the failure is part of a credential.
    #[must_use]
    pub fn key_not_readable(kind: ProviderKind, failure: &std::io::Error) -> Classified {
        Classified::Environmental {
            statement: Statement::sanitised(format!(
                "the `{kind}` key could not be read from standard input: {failure}. Whatever was \
                 read before the failure is deliberately not quoted -- it is part of a credential"
            )),
            // No retry is offered, and the sentence says why rather than
            // leaving a reader to infer it: this command reads a credential
            // from a pipe, and a pipe that closed mid-read does not reopen
            // by being waited on.
            wait: Wait::NoWaitWillHelp(Statement::sanitised(
                "standard input has already been consumed; run the command again with the key                  on its input"
                    .to_owned(),
            )),
        }
    }

    /// A provider key the store would not take.
    ///
    /// The user's: they supplied the value and they can supply another. The
    /// refusal names what was wrong with the shape and **never the value** —
    /// [`SecretRefused`](crate::credentials::SecretRefused) is `Copy` and
    /// therefore cannot carry one.
    #[must_use]
    pub fn key_refused(kind: ProviderKind, refusal: &SecretRefused) -> Classified {
        correctable(
            refusal,
            act(format!(
                "pipe the key in with no trailing spaces, as in `printf %s \"$KEY\" | zaru \
                 providers keys add {kind}`"
            )),
        )
    }

    /// `--continue` on a machine with no sessions at all.
    #[must_use]
    pub fn no_session_to_continue(&self) -> Classified {
        Classified::UserCorrectable {
            statement: Statement::sanitised(
                "there is no session to continue: nothing on this machine has ever started one"
                    .to_owned(),
            ),
            remedy: act(
                "a session is written the first time this harness runs a task, and it cannot run \
                 one yet"
                    .to_owned(),
            ),
        }
    }

    /// A tier that could not be taken from the resolved configuration.
    ///
    /// **The user's, and the provenance is what says so**: every layer that
    /// can set `runtime.tier` at this surface is the user's own — layer 1 is
    /// this binary's built-in, layer 2 their file, layer 4 their environment,
    /// layer 5 their flag — and the refusal already names the key and, where a
    /// layer offered something, the layer.
    #[must_use]
    pub fn tier(&self, refusal: &TierRefused) -> Classified {
        let remedy = match refusal {
            TierRefused::NotSet { key } => act(format!(
                "set {key} in ~/.zaru/config.toml, or give `--runtime <tier>` for one run"
            )),
            TierRefused::WrongShape { key, .. } | TierRefused::NoSuchTier { key, .. } => {
                act(format!(
                    "set {key} to one of the tiers ADR-0001 D1 defines, or give `--runtime <tier>`"
                ))
            }
        };
        correctable(refusal, remedy)
    }

    /// [ADR-0009] D6's `zaru init`, refused.
    ///
    /// **Both arms are the user's.** A manifest that is already there is the
    /// clause working — D6 writes only when absent — and the remedy is to open
    /// the file that is there rather than to ask this command again. A write
    /// that failed is about a directory the user chose by running the command
    /// in it, and the refusal already names the path and the operation, which
    /// is the whole of the remedy; no mapping of I/O kinds is invented, for the
    /// reason this module's own header gives.
    ///
    /// Associated rather than a method: nothing about either arm is ours.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    #[must_use]
    pub fn init(refusal: &crate::manifest::InitRefused) -> Classified {
        let remedy = match refusal {
            crate::manifest::InitRefused::AlreadyThere { path } => act(format!(
                "open {} and edit it; nothing in this harness rewrites a manifest",
                path.display()
            )),
            crate::manifest::InitRefused::NotWritten { path, .. } => act(format!(
                "check that {} is writable by this user",
                path.parent().unwrap_or(path).display()
            )),
        };
        correctable(refusal, remedy)
    }

    /// The working directory could not be established.
    ///
    /// The user's, because it is the directory they ran the harness in.
    /// [ADR-0011] D4 refuses rather than guessing — a boundary whose root is a
    /// guess is not one — and the remedy is to run the command somewhere that
    /// exists.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub fn working_directory(failure: &crate::tools::TreeError) -> Classified {
        correctable(
            failure,
            act("run this command from a directory that exists and this user can read".to_owned()),
        )
    }

    /// A session id that is not a ULID.
    #[must_use]
    pub fn session_id(&self, refusal: &SessionIdRefused) -> Classified {
        correctable(
            refusal,
            run(
                "`zaru sessions list` prints every id on this machine",
                "zaru sessions list",
            ),
        )
    }

    /// A failure reaching a session on disk.
    ///
    /// I/O is the user's here **because of where the path came from**, not
    /// because of its `io::ErrorKind`: it is under their own `$HOME` and the
    /// error already carries the path and the operation, which is the whole of
    /// the remedy. No mapping of I/O kinds is invented — see the module
    /// documentation.
    #[must_use]
    pub fn session(&self, failure: &SessionError) -> Classified {
        let remedy = match failure {
            SessionError::NoHome => act(
                "set HOME to a directory this process can write to; ~/.zaru is where every \
                 session lives"
                    .to_owned(),
            ),
            SessionError::Home(home) => act(format!(
                "make {} writable by this user, or set HOME elsewhere",
                home.path().display()
            )),
            SessionError::Io { path, .. } => act(format!(
                "check that {} is readable by this user",
                path.display()
            )),
            SessionError::NotASessionName { .. } => act(
                "remove or rename whatever is in ~/.zaru/sessions/ that is not a session; \
                 ADR-0010 D1 names a session directory by a ULID and nothing else belongs there"
                    .to_owned(),
            ),
            SessionError::NoSuchSession { .. } => run(
                "`zaru sessions list` prints every id on this machine",
                "zaru sessions list",
            ),
        };
        correctable(failure, remedy)
    }

    /// A failure while removing sessions past the retention window.
    #[must_use]
    pub fn prune(&self, failure: &PruneFailure) -> Classified {
        match failure {
            PruneFailure::Store(store) => self.session(store),
            PruneFailure::NotRemoved { .. } => correctable(
                failure,
                act(
                    "check that ~/.zaru/sessions and the directory named above are writable by \
                     this user"
                        .to_owned(),
                ),
            ),
        }
    }

    /// A session that could not be restored.
    ///
    /// **The one arm that is not the user's is the one the harness wrote.**
    /// The transcript and the checkpoint have exactly one writer in this
    /// workspace and it is this harness, so a *complete* line that does not
    /// parse is a file we produced and cannot read — which is a defect by
    /// D1's own row, and is reported as one rather than as something the
    /// reader can configure. A trailing fragment is not this: ADR-0010 D2
    /// says a crash costs the event in flight, and `resume` reports it as a
    /// fragment rather than failing.
    #[must_use]
    pub fn resume(&self, failure: &ResumeFailure, session: SessionEvidence) -> Classified {
        match failure {
            ResumeFailure::NoSuchDirectory { .. } => correctable(
                failure,
                run(
                    "`zaru sessions list` prints every id on this machine",
                    "zaru sessions list",
                ),
            ),
            // This harness is the only writer of a transcript and a
            // checkpoint, so a *complete* line it cannot read back is a file
            // it wrote wrongly -- and no record states a class for one a
            // person hand-edited instead. Unmapped rather than guessed.
            ResumeFailure::Transcript(_) | ResumeFailure::Checkpoint(_) => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }

    /// A failure reaching the credential store.
    ///
    /// `StoreError` is one of the four enums [ADR-0016]'s Update names as
    /// deliberately unmapped, and it stays unmapped **as a whole**: what is
    /// classified here are only the variants the *listing* can reach, which
    /// are four of eleven. The rest need a store that can be written to, and
    /// nothing in this harness can write one.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn credential_store(&self, failure: &StoreError, session: SessionEvidence) -> Classified {
        match failure {
            StoreError::NoHome => correctable(
                failure,
                act(
                    "set HOME to a directory this process can read; ~/.zaru is where the store \
                     lives"
                        .to_owned(),
                ),
            ),
            StoreError::Io { path, .. } => correctable(
                failure,
                act(format!(
                    "check that {} is readable by this user",
                    path.display()
                )),
            ),
            StoreError::Malformed { path, .. } => correctable(
                failure,
                act(format!(
                    "the only writer of {} is this harness; if it was edited by hand, restore it \
                     or remove it",
                    path.display()
                )),
            ),
            StoreError::Sealing(failure) => self.sealing(failure, session),
            // Reachable from `zaru providers keys add <kind>` since
            // 2026-09-05: one key per kind, so a second `add` for a kind the
            // store already holds lands here. The user's, and the remedy is
            // real -- the listing shows what is already there.
            StoreError::DuplicateAlias { .. } => correctable(
                failure,
                run(
                    "`zaru providers keys` lists every provider key this machine holds",
                    "zaru providers keys",
                ),
            ),
            // Reachable from the listing: a store file naming a provider kind
            // this build does not have. The user's, because the only writer
            // of that file is this harness and a kind it cannot parse means
            // the file came from elsewhere.
            StoreError::UnknownProviderKind { .. } => correctable(
                failure,
                act(
                    "the only writer of ~/.zaru/credentials.json is this harness; if it was \
                     edited by hand, or written by a build that knew a provider kind this one \
                     does not, restore it or remove that entry"
                        .to_owned(),
                ),
            ),
            StoreError::UnknownAlias { .. }
            | StoreError::ApexNeedsConfirmation { .. }
            | StoreError::ApexDeclined { .. }
            | StoreError::SecondComposerRole { .. }
            | StoreError::ComposerScopeExceeded { .. }
            // Nothing this harness runs can add or re-role a stored token, so
            // a store failure of one of these shapes reaching a user is this
            // harness in a state it has no path to.
            => undecided(self.version, self.report_at, session, line!()),
        }
    }

    /// [ADR-0007] D3's sealing, classified by which side of it went wrong.
    ///
    /// # This is the mapping ADR-0016's Update said could not be written
    ///
    /// That record leaves a port failure's class to the port's
    /// *implementation*, and `crate::failure::classify` records the
    /// consequence: no port had one, so no statement existed to read and the
    /// credential store's sealing failure stayed unmapped. **Sealing now has an
    /// implementation**, and because that implementation raises a closed
    /// [`SealingError`] rather than an opaque string, its classes can be
    /// stated. Every arm below is named; there is no wildcard, so a new
    /// variant fails to compile here.
    ///
    /// # The version byte is what tells a defect from a key that changed
    ///
    /// A blob that will not open has two causes belonging to two different
    /// people, and only the format version separates them. A version this
    /// harness writes means the bytes are ours and the **key** is what changed
    /// — user-correctable, and the remedy says how. A version it has never
    /// written means the bytes are not ours, in a file this harness alone
    /// writes, which is a defect.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    #[must_use]
    fn sealing(&self, failure: &SealingError, session: SessionEvidence) -> Classified {
        match failure {
            // The ordinary state of a headless machine, and the remedy is the
            // whole of what D3 offers such a machine.
            SealingError::NoKey => correctable(
                failure,
                act(format!(
                    "set {CREDENTIAL_KEY_VARIABLE} to {} lower-case hexadecimal characters, or                      run where an OS keyring is reachable; that variable holds the key and never                      a credential",
                    SealingKey::HEX_CHARACTERS
                )),
            ),
            // Named the same way and with the same remedy: a value that is not
            // a key is a value the user set.
            SealingError::KeyNotHex => correctable(
                failure,
                act(format!(
                    "set {CREDENTIAL_KEY_VARIABLE} to exactly {} lower-case hexadecimal                      characters; its current value is not, and neither it nor its length is                      quoted anywhere",
                    SealingKey::HEX_CHARACTERS
                )),
            ),
            // The bytes are ours, so the key is what moved.
            SealingError::WillNotOpen => correctable(
                failure,
                act(
                    "restore the OS keyring entry this store's key was in, or set                      ZARU_CREDENTIAL_KEY back to the key these credentials were sealed under; if                      neither is recoverable, remove the store and add the tokens again"
                        .to_owned(),
                ),
            ),
            // The file, which only this harness writes, has been edited.
            SealingError::TooShort { .. } | SealingError::NotHex => correctable(
                failure,
                act(
                    "the only writer of the credential store is this harness; if it was edited                      by hand, restore it from a backup or remove it and add the tokens again"
                        .to_owned(),
                ),
            ),
            // D1's environmental row: the substrate is unwell and the harness
            // is not. Waiting genuinely will not help -- a keyring that is
            // refusing does not start answering on its own -- so the class
            // says so rather than offering a retry that cannot work.
            SealingError::KeyringFailed { .. } => Classified::Environmental {
                statement: Statement::sanitised(failure.to_string()),
                wait: Wait::NoWaitWillHelp(Statement::sanitised(
                    "a keyring that refuses does not begin answering on its own; unlock it, or                      start the session's secret service, and run this again"
                        .to_owned(),
                )),
            },
            // Ours, both of them. Only this harness writes to its own keyring
            // entry, and only this harness writes a version byte.
            SealingError::KeyringHeldNonsense
            | SealingError::UnknownVersion { .. }
            | SealingError::WillNotSeal => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }
}
