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
use crate::credentials::StoreError;
use crate::failure::{
    Action, Classified, DefectReport, Location, Remedy, SessionEvidence, Statement,
};
use crate::providers::{ModelAlias, ModelId};
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
    /// re-decided. The source arm is a port failure with no product
    /// implementation that can produce one — see [`LoadFailure::Source`] — so
    /// reaching it means this binary is in a state it has no path to.
    #[must_use]
    pub fn load(&self, failure: &LoadFailure, session: SessionEvidence) -> Classified {
        match failure {
            LoadFailure::Refused(refusal) => Classified::from(refusal.clone()),
            LoadFailure::Source(_) => undecided(self.version, self.report_at, session, line!()),
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
                "the alias `{}` resolves to {:?}, and this harness carries no provider client \
                 that can reach it: ADR-0012 D3's provider trait has no implementation in any \
                 product tree",
                ModelAlias::Default,
                model.as_str()
            )),
            offered_by: crate::runtime::Tier::Bare,
        }
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
            StoreError::DuplicateAlias { .. }
            | StoreError::UnknownAlias { .. }
            | StoreError::ApexNeedsConfirmation { .. }
            | StoreError::ApexDeclined { .. }
            | StoreError::SecondComposerRole { .. }
            | StoreError::ComposerScopeExceeded { .. }
            // Nothing this harness runs can add, seal or re-role a stored
            // token, so a store failure of one of these shapes reaching a user
            // is this harness in a state it has no path to. `Seal` is a port
            // failure besides, whose class ADR-0016's Update gives to the
            // port's implementation, of which there is none.
            | StoreError::Seal(_) => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }
}
