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
use crate::credentials::{Alias, ReachFailure};
use crate::credentials::{
    CREDENTIAL_KEY_VARIABLE, SealingError, SealingKey, SecretRefused, StoreError,
};
use crate::failure::{
    Action, Classified, DefectReport, Location, Remedy, SessionEvidence, Statement, THIS_HARNESS,
    Wait,
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
            CommandRefused::UnusableAlias(_) => act(
                "an alias is the local name you will call this credential by, as in `work` or \
                 `personal`"
                    .to_owned(),
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

    /// A model resolved and this machine holds no provider key at all.
    ///
    /// **User-correctable, and the remedy is a command this binary runs.**
    /// They configured a model, which is half of what a turn needs; the other
    /// half is [ADR-0007]'s store, and `zaru providers keys add <kind>` is the
    /// surface that fills it. It names the kinds this build can actually
    /// reach rather than all five of [ADR-0012] D3's, because a remedy naming
    /// a kind with no client is a remedy whose reader cannot act.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub fn no_key_for(kinds: &[ProviderKind], model: &ModelId) -> Classified {
        let named: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();
        Classified::UserCorrectable {
            statement: Statement::sanitised(format!(
                "the alias `{alias}` resolves to {model:?} and this machine holds no provider \
                 key, so there is nothing to authenticate the request with. The key is never in \
                 configuration and never in an argument, because an argument is \
                 in the shell's history and in `ps`",
                alias = ModelAlias::Default,
                model = model.as_str(),
            )),
            remedy: run(
                &format!(
                    "store one for the kind this build can reach ({})",
                    named.join(", ")
                ),
                &format!(
                    "providers keys add {}",
                    kinds.first().map_or("gemini", |kind| kind.as_str())
                ),
            ),
        }
    }

    /// A model resolved and this build carries no client for its kind.
    ///
    /// # The sentence changed on 2026-09-05 and the reason is the whole point
    ///
    /// It said "nothing wires a provider client to a loop in this build", which
    /// was true from the day the `gemini` client landed until the day this
    /// composition ran a turn. **Something wires one now**, so what is missing
    /// is no longer the wiring: it is a client for four of [ADR-0012] D3's five
    /// kinds. A refusal that overstates what is absent sends a reader looking
    /// for the wrong thing, which is why the sentence is corrected in the
    /// commit that made it false rather than loosened.
    ///
    /// # It is still D1's capability class and the class still does not fit
    ///
    /// [`Classified::Capability`] carries a [`Tier`](crate::runtime::Tier),
    /// D1's row for it is "the tier does not offer this", and no tier is what
    /// is wrong — the client has not been written. **D1 has no row for "not
    /// built yet"**, which [ADR-0016]'s open question already records and
    /// which this arc did not answer. It had a second instance until
    /// 2026-09-05, `Surface::no_inner_loop`, and that one is gone because the
    /// thing it said was not built is built.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn no_provider_client(&self, model: &ModelId) -> Classified {
        let reachable: Vec<&str> = crate::compose::KINDS_WITH_A_CLIENT
            .iter()
            .map(|kind| kind.as_str())
            .collect();
        Classified::Capability {
            statement: Statement::sanitised(format!(
                "the alias `{alias}` resolves to {model:?}, and this build carries a client for \
                 {reachable} of {total} provider kinds. A turn against that kind \
                 runs; nothing you can configure gives the other {remaining} a client, because a \
                 client has to be written",
                alias = ModelAlias::Default,
                model = model.as_str(),
                reachable = reachable.join(", "),
                remaining = ProviderKind::ALL.len() - reachable.len(),
                total = ProviderKind::ALL.len(),
            )),
            offered_by: crate::runtime::Tier::Bare,
        }
    }

    /// This machine holds provider keys, and none is for a kind with a client.
    ///
    /// The sibling of [`Surface::no_key_for`], and the two are different
    /// classes because what the reader can do differs: a machine with no key
    /// has one to add, and a machine holding a key for `anthropic` has
    /// configured something correctly that this build does not carry. Naming
    /// which keys are held is what tells the two apart for the reader as well.
    #[must_use]
    pub fn no_client_for_the_kinds_held(
        &self,
        model: &ModelId,
        held: &[ProviderKind],
    ) -> Classified {
        let named: Vec<&str> = held.iter().map(|kind| kind.as_str()).collect();
        let reachable: Vec<&str> = crate::compose::KINDS_WITH_A_CLIENT
            .iter()
            .map(|kind| kind.as_str())
            .collect();
        Classified::Capability {
            statement: Statement::sanitised(format!(
                "the alias `{alias}` resolves to {model:?}, and the {count} provider key(s) this \
                 machine holds ({names}) are for kinds this build carries no client for. It \
                 carries {reachable}, of {total}. Nothing you can configure changes \
                 that: the client has to be written",
                alias = ModelAlias::Default,
                model = model.as_str(),
                count = held.len(),
                names = named.join(", "),
                reachable = reachable.join(", "),
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
    /// [`SecretRefused`] is `Copy` and
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

    /// Standard input could not be read while adding a Nuclear Notes token.
    ///
    /// The sibling of [`Self::key_not_readable`] and for the same reasons; the
    /// two are separate functions rather than one taking a noun because the
    /// remedy names the command, and naming the wrong one is
    /// [ADR-0016](https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy)
    /// D2's "an error message whose reader cannot act".
    #[must_use]
    pub fn token_not_readable(alias: &Alias, failure: &std::io::Error) -> Classified {
        Classified::Environmental {
            statement: Statement::sanitised(format!(
                "the token for \"{alias}\" could not be read from standard input: {failure}. \
                 Whatever was read before the failure is deliberately not quoted -- it is part of \
                 a credential"
            )),
            wait: Wait::NoWaitWillHelp(Statement::sanitised(
                "standard input has already been consumed; run the command again with the token \
                 on its input"
                    .to_owned(),
            )),
        }
    }

    /// A Nuclear Notes token the store would not take.
    #[must_use]
    pub fn token_refused(alias: &Alias, host: &str, refusal: &SecretRefused) -> Classified {
        correctable(
            refusal,
            act(format!(
                "pipe the token in with no trailing spaces, as in `printf %s \"$TOKEN\" | zaru \
                 notes tokens add {alias} {host}`"
            )),
        )
    }

    /// A Nuclear Notes instance that would not answer while a token was added.
    ///
    /// # This classifies the command, not the enum
    ///
    /// [ADR-0016]'s taxonomy is mapped per enum, all or nothing, and
    /// `NotesError` is one of the enums this crate deliberately does not map —
    /// a `forbidden` from Nuclear Notes could be a revoked token, a scope
    /// change or a workspace the user was removed from, and
    /// [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces)
    /// D7 says the server does not reveal which. **Nothing here reads the
    /// variant**: the class comes from what the command was doing, which is the
    /// "class by provenance" reading [ADR-0016] already carries as proposed.
    /// A person who ran `notes tokens add` and got no answer has exactly two
    /// things to change — the host they typed and the token they piped — so the
    /// class is theirs and the remedy names both. The client's own sentence is
    /// carried verbatim beside it and is not interpreted.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn notes_unreachable(alias: &Alias, host: &str, failure: &ReachFailure) -> Classified {
        Classified::UserCorrectable {
            statement: Statement::sanitised(format!(
                "the token for \"{alias}\" was not stored, because {host} did not complete a \
                 session: {failure}"
            )),
            remedy: act(format!(
                "check the host and the token, then run `printf %s \"$TOKEN\" | zaru notes tokens \
                 add {alias} {host}` again"
            )),
        }
    }

    /// `--continue` on a machine with no sessions at all.
    #[must_use]
    pub fn no_session_to_continue(&self) -> Classified {
        Classified::UserCorrectable {
            statement: Statement::sanitised(
                "there is no session to continue: no session has been started in this directory"
                    .to_owned(),
            ),
            remedy: act(
                "run a task here to start one, or name a session from anywhere with `zaru \
                 --resume <id>`"
                    .to_owned(),
            ),
        }
    }

    /// [`ContinueFailure`](crate::session::ContinueFailure), which is two
    /// classes rather than one.
    ///
    /// The store is the user's and a `meta.toml` this harness wrote is ours;
    /// that type's own documentation carries the argument, and this function
    /// is the whole of what it buys — the two are told apart by **which
    /// variant**, never by anything on the value, which is what
    /// [ADR-0016]'s Update asks for.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn continuing(&self, failure: &crate::session::ContinueFailure) -> Classified {
        match failure {
            crate::session::ContinueFailure::Store(store) => self.session(store),
            crate::session::ContinueFailure::Meta { evidence, failure } => {
                Self::meta(failure, evidence.clone())
            }
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
            TierRefused::WrongShape { key, .. } | TierRefused::NoSuchTier { key, .. } => act(
                format!("set {key} to one of the tiers, or give `--runtime <tier>`"),
            ),
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
                 {THIS_HARNESS} names a session directory by a ULID and nothing else belongs \
                 there"
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

    /// A checkpoint that parses as JSON but is not what this harness writes.
    ///
    /// # The same class as a checkpoint that will not parse, by the same
    /// argument
    ///
    /// [`Self::resume`] above gives `ResumeFailure::Checkpoint` a defect
    /// because "the transcript and the checkpoint have exactly one writer in
    /// this workspace and it is this harness, so a *complete* line that does
    /// not parse is a file we produced and cannot read". A document that
    /// parses and holds something other than [ADR-0013] D1's layer 6 is the
    /// same fact one layer in: `serde_json` accepted it and
    /// [`crate::compose::SessionContext`] did not.
    ///
    /// It is a separate method rather than a `ResumeFailure` variant because
    /// `crate::session::resume` deliberately treats the file as opaque —
    /// `crate::session::checkpoint` "writes it whole, reads it whole, and
    /// interprets no field" — and the one place it is interpreted is the
    /// boundary. Making the reader parse it would put ADR-0013's shape in two
    /// modules.
    ///
    /// **Nothing of the error is rendered.** A `serde_json::Error`'s own
    /// message quotes the value it tripped on, and this file holds a session's
    /// conversation; the defect report carries a location, a version, where
    /// to report and the session evidence, which is what [ADR-0016] D3's Update
    /// already decided for a panic's message on the same grounds.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn checkpoint_contents(
        &self,
        _error: &serde_json::Error,
        session: SessionEvidence,
    ) -> Classified {
        undecided(self.version, self.report_at, session, line!())
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
            // **Reachable since `notes tokens add` landed on 2026-09-05, and
            // found by running rather than by reading**: this arm was
            // `undecided` for every one of the five, on the sentence "nothing
            // this harness runs can add or re-role a stored token". Adding one
            // is what `notes tokens add` does, so an apex token offered on a
            // machine with no terminal reached this arm and was reported as a
            // **defect in Zaru with a report URL** — ADR-0016 D3's own
            // "never present a defect as a user error", inverted.
            //
            // ADR-0007 D8's confirmation could not be asked. The user's: they
            // chose `apex` and they choose where they run it.
            StoreError::ApexNeedsConfirmation { .. } => correctable(
                failure,
                act(
                    "run it again at a terminal, where the question can be asked; or leave the \
                     word `apex` off to store an instance-locked credential, which is what \
                     {THIS_HARNESS} makes the default"
                        .to_owned(),
                ),
            ),
            // **Not an error at all.** The user was asked and said no, and the
            // harness did exactly what they said. ADR-0016 D1: an expected
            // failure "is the mechanism operating, and colouring it like a
            // crash teaches users to fear the thing that makes the product
            // work".
            StoreError::ApexDeclined { alias } => Classified::Expected(
                crate::failure::Expected::new(Statement::sanitised(format!(
                    "\"{alias}\" was not stored, because the confirmation it requires \
                     was declined"
                ))),
            ),
            // **All three have a path as of 2026-09-14, and the comment that
            // stood here said they had none.** It read: "nothing this harness
            // runs grants a composer role or re-roles a stored token, so a
            // store failure of one of these three shapes reaching a user is
            // this harness in a state it has no path to". `zaru notes use
            // <alias>` runs `grant_composer_role`, so all three are now things
            // a **person** did at a command line.
            //
            // **Found by running the binary rather than by reading it**, and
            // it is the second time this exact shape has been found that way:
            // `zaru notes use play2` against a real stored token exited 70,
            // "a defect in Zaru", with a report URL, for a person who had
            // simply offered the role to a token whose scope is too wide.
            // That is D3's "never present a defect as a user error" inverted.
            //
            // Each remedy names something this binary runs, because a remedy
            // that does not is a stack trace with better grammar.
            StoreError::UnknownAlias { .. } => correctable(
                failure,
                act("run `zaru notes tokens` to see the aliases this machine holds".to_owned()),
            ),
            // **The statement is the honest half and it is worth reading.**
            // `notes use` grants the role where none is held; it does not take
            // it from a token that holds one, so this refusal is what a second
            // grant looks like rather than a failed move. Whether that verb
            // should move the role is a question for its own record and is
            // deliberately not answered by a classification.
            StoreError::SecondComposerRole { .. } => correctable(
                failure,
                act(
                    "run `zaru notes tokens` to see which token carries the role; this command \
                     grants it where none is held rather than taking it from one that has it"
                        .to_owned(),
                ),
            ),
            // The composer's credential may carry the read-only set and
            // nothing else, and the refusal above already names the offending
            // tool. What the remedy adds is that this is not the strip going
            // dark: a single stored token still serves the hint strip without
            // carrying the role at all.
            StoreError::ComposerScopeExceeded { .. } => correctable(
                failure,
                act(
                    "offer the role to a token scoped to the read-only set; `zaru notes tokens` \
                     prints each token's tool count, and a single stored token already serves the \
                     hint strip without carrying the role"
                        .to_owned(),
                ),
            ),
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

// --- What a turn is refused with, and what it ends as ----------------------
//
// Everything below is raised by [`crate::compose::turn`] rather than by the
// parser, and it is here for the reason the rest of this module is: the
// provenance is known at the surface, and `failure::classify` maps only the
// enums whose class a record states outright. **`failure::classify` is not
// edited**, and neither are the four enums that record deliberately leaves
// unmapped.

impl Surface<'_> {
    /// A `./zaru.toml` this harness could not read.
    ///
    /// **User-correctable**, by the reading [ADR-0014]'s Update settled for the
    /// two configuration files: this harness is not the writer of a manifest
    /// except through `zaru init`, which writes a constant a check parses, so a
    /// file that does not read back is a file a person edited. The refusal
    /// carries the reader's own words, which name the path and the position
    /// and never the line's contents.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn manifest(refusal: &crate::manifest::ManifestNotRead) -> Classified {
        correctable(
            refusal,
            act(
                "open the file at the position named and fix it, or delete it: a project with no \
                 manifest runs the tool-call loop"
                    .to_owned(),
            ),
        )
    }

    /// A project's declared validators do not form a plan.
    ///
    /// A cycle, an unknown prerequisite, or two validators with one name —
    /// all of them [ADR-0009] D2's `after` being wrong in a file the user can
    /// open and edit, which is [ADR-0016] D1 row 2 exactly.
    ///
    /// **This replaces `Surface::no_inner_loop`**, deleted 2026-09-05 when the
    /// iteration loop was wired. That refusal said "this build has no
    /// iteration loop to run"; it has one, so the sentence went with the
    /// condition rather than being left to become false.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn validator_plan(refusal: &zaru_core::iteration::validator::PlanRefused) -> Classified {
        correctable(
            refusal,
            act("open `./zaru.toml` and fix the `after` list it names".to_owned()),
        )
    }

    /// No iteration ceiling could be resolved.
    ///
    /// Two classes, and which one is the difference between a value the reader
    /// set and a row [ADR-0001] D3 does not have. A number they typed is
    /// theirs to change; a tier that offers no loop for their placement is
    /// [ADR-0016] D1 row 4's capability, and the remedy names the key that
    /// overrides it.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn iteration_ceiling(refusal: &crate::runtime::CeilingRefused) -> Classified {
        match refusal {
            crate::runtime::CeilingRefused::NotACount { key, .. }
            | crate::runtime::CeilingRefused::WrongShape { key, .. } => correctable(
                refusal,
                run(
                    "see every layer's value for it",
                    &format!("config explain {key}"),
                ),
            ),
            crate::runtime::CeilingRefused::NoCell { tier, .. } => Classified::Capability {
                statement: Statement::sanitised(refusal.to_string()),
                offered_by: *tier,
            },
        }
    }

    /// A project set an endpoint this harness cannot use.
    #[must_use]
    pub fn endpoint(kind: ProviderKind, refusal: &crate::providers::EndpointRefused) -> Classified {
        correctable(
            refusal,
            act(format!(
                "set `{}` to an origin, or unset it and the built-in one is used",
                kind.endpoint_key()
            )),
        )
    }

    /// The provider client could not be built at all.
    ///
    /// Environmental and never the user's: the only way
    /// [`GeminiClient::new`](crate::providers::GeminiClient::new) fails is a
    /// machine with no usable TLS backend, which is what that constructor's
    /// own documentation says. Nothing the reader types fixes it, so the wait
    /// says so.
    #[must_use]
    pub fn provider(failure: &crate::providers::GeminiFailure) -> Classified {
        Classified::Environmental {
            statement: Statement::sanitised(failure.to_string()),
            wait: Wait::NoWaitWillHelp(Statement::sanitised(
                "this is about the machine rather than about the provider, so waiting changes \
                 nothing"
                    .to_owned(),
            )),
        }
    }

    /// The model says it cannot call tools.
    ///
    /// [ADR-0012] clause 3's configuration-time half, met on the real path for
    /// the first time: the refusal is raised **before** the loop starts,
    /// because `ToolCalling::required` is what produces the value `run` needs
    /// and there is no way to start a turn without it.
    ///
    /// User-correctable, and deliberately not D1's capability class, for the
    /// reason `providers::capability` already gives: "no tier is what is
    /// wrong, since every tier can reach a provider that calls tools, so
    /// naming one would be a lie the type would force".
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    #[must_use]
    pub fn model_cannot_call_tools(
        refusal: &zaru_core::tool_call::ModelCannotCallTools,
    ) -> Classified {
        correctable(
            refusal,
            act(format!(
                "set `{}` to a model whose provider calls tools",
                ModelAlias::Default.key()
            )),
        )
    }

    /// A session id could not be minted.
    ///
    /// Environmental: [`SessionId::mint`](crate::session::SessionId::mint)
    /// fails when the machine's entropy source cannot be read, which is
    /// neither the reader's doing nor the harness's.
    #[must_use]
    pub fn session_not_started(failure: &crate::session::MintFailure) -> Classified {
        Classified::Environmental {
            statement: Statement::sanitised(format!("a session could not be started: {failure}")),
            wait: Wait::NoWaitWillHelp(Statement::sanitised(
                "an entropy source that cannot be read does not begin answering on its own"
                    .to_owned(),
            )),
        }
    }

    /// `meta.toml` could not be written.
    ///
    /// **A defect, and this is the reading [ADR-0010]'s Status tracking took
    /// for exactly this file**: "its only writer is this harness, so a
    /// malformed one stays a defect — the argument [ADR-0016]'s Update already
    /// applies to `StoreError::Malformed`. The two are told apart by which
    /// port failed, never by anything on the value."
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn meta(failure: &crate::session::MetaFailure, session: SessionEvidence) -> Classified {
        let _ = failure;
        undecided(self_version(), self_report_at(), session, line!())
    }

    /// The checkpoint could not be written.
    ///
    /// A defect for the same reason `meta.toml` is: [ADR-0010] D3's
    /// `context.json` has exactly one writer and it is this harness.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn checkpoint(
        failure: &crate::session::CheckpointError,
        session: SessionEvidence,
    ) -> Classified {
        let _ = failure;
        undecided(self_version(), self_report_at(), session, line!())
    }

    /// The transcript could not be opened or appended to.
    ///
    /// A defect, and the class is load-bearing rather than a fallback: this
    /// harness is the transcript's only writer, [ADR-0010] D2 makes it the
    /// replayable record, and a turn whose record is incomplete has not
    /// recorded what happened however the turn itself ended.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn transcript(
        failure: &crate::session::TranscriptError,
        session: SessionEvidence,
    ) -> Classified {
        let _ = failure;
        undecided(self_version(), self_report_at(), session, line!())
    }

    /// A child's environment could not be built from this process's own.
    ///
    /// A defect: `Environment::inherited_minimum` reads five names this
    /// harness names itself and refuses a value carrying a NUL, and a `PATH`
    /// with a NUL in it is a process this harness was started with rather than
    /// anything a reader typed at it. Carried rather than unwrapped so that the
    /// day it fires, a maintainer gets a report rather than a panic.
    #[must_use]
    pub fn child_environment(refusal: &crate::process::NotForAChild) -> Classified {
        let _ = refusal;
        undecided(
            self_version(),
            self_report_at(),
            SessionEvidence::NoSessionExists,
            line!(),
        )
    }

    /// The HTTP client `web.fetch` retrieves through could not be built.
    ///
    /// **Environmental, and the same class the provider client's own
    /// `Unavailable` gets for the same cause**: `reqwest` fails to build a
    /// client on a machine with no usable TLS backend, which is a property of
    /// the machine and not of anything a reader typed. It is not a defect —
    /// nothing here supplied a bad argument — and it is not correctable,
    /// because there is no key to change.
    ///
    /// No wait is offered. Running again on the same machine gets the same
    /// answer, so saying "try later" would be the retry policy on a
    /// environmental failure that ADR-0016 D4 names and no record gives
    /// numbers for.
    #[must_use]
    pub fn web_client(refusal: &crate::web::ClientUnavailable) -> Classified {
        Classified::Environmental {
            statement: Statement::sanitised(refusal.to_string()),
            wait: Wait::NoWaitWillHelp(Statement::sanitised(
                "web.fetch needs an HTTP client and this machine could not provide one, which \
                 waiting does not change. Every other built-in works without it"
                    .to_owned(),
            )),
        }
    }

    /// The allowlist a user's configuration set could not be read.
    #[must_use]
    pub fn allowlist(refusal: &crate::tools::AllowlistRefused) -> Classified {
        correctable(
            refusal,
            run(
                "read what the harness has for that key",
                &format!("config explain {}", crate::tools::allowlist::KEY),
            ),
        )
    }

    /// A turn that ended in a port failure.
    ///
    /// # This is where ADR-0016's own missing clause is answered
    ///
    /// That record's Update: "**A port failure's class belongs to the port's
    /// implementation, not to the value it hands back.**" `PortFailure` carries
    /// a sentence and no discriminant, so the class cannot be read off the
    /// error. It does not have to be: the port kind on
    /// [`ToolCallError`](zaru_core::tool_call::ToolCallError) says *which*
    /// implementation failed, and for the model the composition kept the typed
    /// [`GeminiFailure`](crate::providers::GeminiFailure) that produced it.
    ///
    /// So the model's failures are classified by provenance, exactly as that
    /// record's own table for this client states — a rejected key is the
    /// user's, a refused request shape and an unreadable body are ours, a 5xx
    /// or an unopened socket is neither's — and every other port's failure is
    /// carried as a **defect**, which is the recorded state rather than a
    /// gap: `PortFailure`, `SourceFailure` and `OverflowFailure` "each carry a
    /// `String` by design, and that, rather than the absence of an
    /// implementation, is what will keep them unmapped".
    #[must_use]
    pub fn turn(
        &self,
        error: &zaru_core::tool_call::ToolCallError,
        provider: Option<&crate::providers::GeminiFailure>,
        session: SessionEvidence,
    ) -> Classified {
        use zaru_core::tool_call::{PortKind, ToolCallError};

        let ToolCallError::Port { port, .. } = error;
        match (port, provider) {
            (PortKind::Model, Some(failure)) => self.provider_failure(failure, session),
            // A model failure with no typed value kept is unreachable: the
            // adapter stores one on every `Err`. Carried as a defect rather
            // than unwrapped, because the day it is reachable the adapter has
            // stopped keeping them and that is a bug in this crate.
            // A model failure with no typed value kept is unreachable, and so
            // is an inner-loop failure reaching here: `compose::turn` reads
            // the typed `IterationError` off `Inner` first and classifies by
            // the port that actually failed. Both are carried as defects
            // because the day either is reachable, this crate has stopped
            // keeping what it said it keeps.
            (PortKind::Model, None)
            | (PortKind::Tools | PortKind::ContextPolicy | PortKind::InnerLoop, _) => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }

    /// A summarisation that did not produce a summary, in its own class.
    ///
    /// # It reuses the provider's classification and adds no taxonomy
    ///
    /// [ADR-0013] D2's compaction is a model call, so a failure here is a
    /// provider failure and [`Self::provider_failure`] already says which of
    /// [ADR-0016] D1's classes each shape belongs to. The typed value is kept
    /// by the same `Classifying` adapter the turn uses, which is why the
    /// summariser borrows that adapter rather than a client.
    ///
    /// The arm with no typed failure is not unreachable here, and that is the
    /// difference from [`Self::turn`]: the summariser can fail for a reason
    /// the provider never saw — a model that stopped without text, or one that
    /// asked for a tool it was not offered — and neither is a `GeminiFailure`.
    /// Both are the mechanism reporting what happened rather than a defect, so
    /// they are user-correctable: the reader can shorten the session or
    /// configure a model that answers, and the sentence carries the
    /// provider's own words.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn summarisation(
        &self,
        failure: &zaru_core::iteration::PortFailure,
        provider: Option<&crate::providers::GeminiFailure>,
        session: SessionEvidence,
    ) -> Classified {
        match provider {
            Some(typed) => self.provider_failure(typed, session),
            None => Classified::UserCorrectable {
                statement: Statement::sanitised(format!(
                    "the context could not be compacted, so this turn did not run: {failure}. \
                     {THIS_HARNESS} replaces the oldest conversation with a generated summary \
                     when \
                     the window fills, and the summary is what did not arrive; nothing was \
                     discarded, and the transcript still holds every turn"
                )),
                remedy: Remedy::one(Action::described(Statement::sanitised(
                    "start a new session, or configure a model that answers a summarisation"
                        .to_owned(),
                ))),
            },
        }
    }

    /// One provider failure, in the class [ADR-0016]'s own table gives it.
    ///
    /// The table is that record's Status tracking of 2026-09-05, written when
    /// the client landed and measured against the live endpoint. This is a
    /// wildcard-free match over the six, so a seventh shape fails to compile
    /// here rather than taking a neighbouring class — which is how the sixth
    /// arrived: `ResultsDoNotMatchCalls` was added to `GeminiFailure` and
    /// this function stopped compiling until it was placed.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn provider_failure(
        &self,
        failure: &crate::providers::GeminiFailure,
        session: SessionEvidence,
    ) -> Classified {
        use crate::providers::GeminiFailure as F;
        match failure {
            // "the API rejected the key -- user-correctable: they can replace
            // it, and the remedy names the alias and the kind".
            F::CredentialRejected { kind, .. } => correctable(
                failure,
                run("replace the key", &format!("providers keys add {kind}")),
            ),
            // "5xx, or the socket never opened -- environmental: nothing the
            // reader typed caused it and nothing they type fixes it."
            //
            // **No retry policy is stated and none is invented.** ADR-0016 D4
            // has environmental failures retry with backoff; no record gives
            // the numbers, `providers::gemini` takes none, and clause 5's own
            // account says both "arrive from the caller and neither has a
            // default". So the wait says waiting may help and names no policy,
            // rather than this module choosing one.
            F::Unavailable { .. } => Classified::Environmental {
                statement: Statement::sanitised(failure.to_string()),
                wait: Wait::NoWaitWillHelp(Statement::sanitised(
                    "this harness has no retry policy and nothing states one, so it stops here \
                     and says so rather than retrying on a policy nobody chose. Running the same \
                     command again is the retry"
                        .to_owned(),
                )),
            },
            // "the API refused the request's shape -- defect: this harness
            // built the request"; "a response body the client cannot read --
            // defect: the mapping is ours"; "a tool descriptor whose schema is
            // not JSON -- defect: the harness supplied the descriptor". And
            // the sixth, added 2026-09-05: a turn whose accumulated results
            // and remembered calls disagree in number -- defect, because both
            // numbers are this process's and neither came from the provider.
            F::RequestRefused { .. }
            | F::Unreadable { .. }
            | F::ToolSchemaUnreadable { .. }
            | F::ResultsDoNotMatchCalls { .. } => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }

    /// A turn the model stopped without answering.
    ///
    /// [ADR-0016] D5's `1`, "the work failed", and D1 row 1's "not an error --
    /// this is the loop working". Both are true and the mapping says so where
    /// it is written: the mechanism operated, and the outcome is what a
    /// wrapping CI job needs.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn turn_stopped(reason: &str, rounds: u32) -> Classified {
        Classified::Expected(crate::failure::Expected::new(Statement::sanitised(
            format!(
                "the model stopped after {rounds} exchange(s) without answering: {reason}. That \
                 is the provider's own word for why, carried through rather than interpreted"
            ),
        )))
    }

    /// A turn that reached its ceiling with the model still asking for tools.
    ///
    /// [ADR-0008] D5: exhaustion "is not an error and is not a success … the
    /// harness presents what was tried and where it stopped rather than either
    /// claiming completion or reporting a generic failure". So it names both
    /// numbers and the ceiling it hit.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    #[must_use]
    pub fn turn_exhausted(rounds: u32, calls: u32) -> Classified {
        Classified::Expected(crate::failure::Expected::new(Statement::sanitised(
            format!(
                "the turn reached its ceiling of {rounds} exchange(s) with the model still asking \
                 for tools, having run {calls} tool call(s). Nothing failed and nothing \
                 completed: this is where it stopped"
            ),
        )))
    }

    /// A turn whose body was an iteration that did not succeed.
    ///
    /// [ADR-0008] D5: exhaustion "is not an error and is not a success … the
    /// harness presents what was tried and where it stopped rather than either
    /// claiming completion or reporting a generic failure". So it is
    /// [ADR-0016] D1 row 1's expected register at D5's `1`, never the error
    /// register, and it names the iterations, the reason in
    /// `ExhaustionReason`'s own words, and the last failure the validators
    /// actually printed.
    ///
    /// **This replaces the defect arm**, deleted 2026-09-05: that arm said
    /// this outcome was "unreachable: this composition supplies no inner
    /// loop", and it does now.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn loop_exhausted(
        iterations: u32,
        reason: zaru_core::iteration::ExhaustionReason,
        last_failure: Option<&str>,
    ) -> Classified {
        // The wording is `crate::cli::render`'s, which is where it moved on
        // 2026-09-05 so that the pane paints the same sentence this exit code
        // carries -- see that function for why one run may not have two
        // explanations depending on where it is read.
        let why = crate::cli::render::exhaustion(iterations, reason);
        // The validators' own output, carried rather than summarised: ADR-0008
        // D4 forbids paraphrase on the path into a prompt and D5 asks the
        // harness to present "what was tried", which is the same bytes.
        let tried = last_failure.map_or_else(
            || " No iteration reached an evaluation.".to_owned(),
            |failure| format!(" What the validators last said:\n{failure}"),
        );
        Classified::Expected(crate::failure::Expected::new(Statement::sanitised(
            format!(
                "the iteration loop ran {iterations} iteration(s) and the declared validators \
                 were never all satisfied: {why}.{tried}"
            ),
        )))
    }

    /// The inner loop's own port failed.
    ///
    /// The class is the **inner** port's rather than the branch's, read off
    /// the typed [`IterationError`](zaru_core::iteration::IterationError) the
    /// composition kept — see [`crate::compose::Inner`] for why it is kept
    /// rather than carried through the port.
    ///
    /// A generator failure is a provider failure and is classified exactly as
    /// a turn's is, off the same recorded `GeminiFailure`. A validators
    /// failure is the project's file: an unusable `matches` pattern, a schema
    /// that cannot be read, a `run` that is not a command line. The other four
    /// are the harness's.
    #[must_use]
    pub fn inner_loop(
        &self,
        error: &zaru_core::iteration::IterationError,
        provider: Option<&crate::providers::GeminiFailure>,
        session: SessionEvidence,
    ) -> Classified {
        use zaru_core::iteration::{IterationError, PortKind};

        let IterationError::Port { port, failure, .. } = error;
        match (port, provider) {
            (PortKind::Generator, Some(failure)) => self.provider_failure(failure, session),
            (PortKind::Validators, _) => correctable(
                failure,
                act("open `./zaru.toml` and fix the validator it names".to_owned()),
            ),
            (PortKind::Generator, None) | (PortKind::Executor | PortKind::ContextPolicy, _) => {
                undecided(self.version, self.report_at, session, line!())
            }
        }
    }
}

/// This binary's version, for the associated functions that have no `self`.
///
/// The methods above take the version off `Surface`, which is where a binary's
/// own package metadata arrives. Four of the functions here are associated
/// rather than methods because nothing about the failure they report is the
/// surface's, and they read the same metadata this crate is built from.
const fn self_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Where a defect is reported. See [`self_version`].
const fn self_report_at() -> &'static str {
    env!("CARGO_PKG_REPOSITORY")
}
