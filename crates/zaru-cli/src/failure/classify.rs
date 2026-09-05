// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which of [ADR-0016] D1's classes each error this workspace already raises
//! belongs to.
//!
//! # Every mapping is one exhaustive `match` with no wildcard arm
//!
//! A new variant anywhere in the workspace therefore fails to compile *here*
//! rather than quietly taking a neighbouring class — which D1's own Negative
//! consequence calls the worse outcome: "a misclassified error is worse than
//! an unclassified one because the presentation actively misleads". No arm
//! uses `_`, and no arm uses `..` on fields it needs to read.
//!
//! # Four enums are deliberately not mapped, and that is a decision
//!
//! Under a **delegated coordinator ruling of 2026-09-04**, open to Jeshua's
//! veto: this module maps the enums every one of whose variants has a class a
//! record states, and maps **none** of the four that do not — per enum, all or
//! nothing, because a partial mapping looks like coverage and is a hole with a
//! comment on it. The unmapped four are `zaru_core::iteration::IterationError`,
//! [`StoreError`](crate::credentials::StoreError),
//! [`PresentationRefused`](crate::tools::PresentationRefused) and
//! [`RefusedBecause`](crate::tools::RefusedBecause), and every unreadable
//! variant is listed on ADR-0016's own 2026-09-04 Update with the clause it
//! would need.
//!
//! Two shapes account for most of them, and both are properties of the
//! taxonomy rather than gaps in the survey:
//!
//! **A port failure's class belongs to the port's implementation, not to the
//! value it hands back.** `zaru-core`'s `PortFailure`,
//! [`SourceFailure`](crate::config::SourceFailure),
//! [`SealFailure`](crate::credentials::SealFailure) and
//! [`OverflowFailure`](crate::tools::OverflowFailure) each carry an
//! implementation's own wording and nothing else, by design. `zaru-core`'s own
//! port module says why that settles nothing: "a provider outage is
//! environmental, a missing credential is user-correctable, and neither is the
//! loop failing" — both are one `PortFailure`, and D1 puts them in different
//! rows. No port has a product implementation anywhere in this workspace, so
//! no such statement exists to read.
//!
//! **A refusal of a caller-passed number takes its class from where the number
//! came from.** `zaru-core`'s `ConfigurationError`,
//! [`TtlRefused`](crate::credentials::TtlRefused) and
//! [`BudgetIsZero`](crate::tools::BudgetIsZero) are all "you passed zero". A
//! ceiling that arrived from a user's `runtime.max_iterations` is
//! user-correctable; the same ceiling hard-coded by us is a defect. Nothing
//! hands any of the three to a user today — [ADR-0014]'s schema declares no key
//! — so all three are defects today and stop being defects the moment a key is
//! declared. A classification written as a pure function of the error value
//! would be wrong for a reason no check could catch.
//!
//! # No command is invented
//!
//! Every remedy here is an [`Action::described`] — a sentence naming the file,
//! the layer, the key or the alias to change. Not one names a command to run,
//! because the command surface is [ADR-0015]'s and does not exist, and because
//! ADR-0016 D2's own worked example (`zaru config set ...`) is the subject of
//! an open question against [ADR-0014] D4 on [operations/adr-status].
//!
//! # Nothing here quotes a value
//!
//! Every remedy names a key, an alias or a layer, exactly as the refusals it
//! is built from already do — [`ConfigRefused`] carries no value by
//! construction and [`SecretRefused`]
//! carries nothing at all. The checks assert the absence of a planted bearer
//! value **and of its ASCII core** from every statement, remedy and rendering
//! this module can produce.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status

use crate::config::{CoercionFailure, ConfigRefused, KeyRefused};
use crate::credentials::{AliasRefused, DescriptionRefused, SecretRefused};
use crate::failure::classified::Classified;
use crate::failure::defect::DefectReport;
use crate::failure::remedy::{Action, Remedy, Statement};
use crate::tools::{InvocationRefused, ModeRefused, TreeError};

/// A remedy of one described action, built from a sentence.
fn act(sentence: String) -> Remedy {
    Remedy::one(Action::described(Statement::sanitised(sentence)))
}

/// A user-correctable failure: the refusal's own words, and what to change.
fn correctable(refusal: &impl core::fmt::Display, remedy: Remedy) -> Classified {
    Classified::UserCorrectable {
        statement: Statement::sanitised(refusal.to_string()),
        remedy,
    }
}

/// ADR-0014 D5's own refusals. Every one is a key in a file the user or a
/// repository they cloned wrote, so every one is D1 row 2.
impl From<KeyRefused> for Classified {
    fn from(refusal: KeyRefused) -> Self {
        let remedy = match &refusal {
            KeyRefused::Empty => act("give the key a name, or remove the entry".to_owned()),
            KeyRefused::EmptySegment { offered } => act(format!(
                "remove the leading, trailing or doubled dot from {offered:?}"
            )),
            KeyRefused::Control { offered } => act(format!(
                "remove the control character from {offered:?}; a configuration key is ordinary \
                 text"
            )),
            KeyRefused::SurroundingWhitespace { offered } => act(format!(
                "remove the space before or after a dot in {offered:?}"
            )),
        };
        correctable(&refusal, remedy)
    }
}

/// ADR-0014 D2's coercions. A value in a file, in the shape the schema does
/// not declare.
impl From<CoercionFailure> for Classified {
    fn from(refusal: CoercionFailure) -> Self {
        // `CoercionFailure` carries no `Display`, and `{:?}` is not a
        // substitute for one on this path: it escapes, which is how a
        // published value hides from an absence assertion. Each variant gets
        // its own sentence instead, and none of them can hold a value —
        // `found` is a shape name and the alias arm carries a name rather than
        // a secret.
        let (said, remedy) = match &refusal {
            CoercionFailure::WrongShape { found } => (
                format!("the value is {found}, which is not the shape the key declares"),
                act(format!(
                    "the value is {found}; write it in the shape the key declares"
                )),
            ),
            CoercionFailure::Unparsable => (
                "the value is text and does not read as the shape the key declares".to_owned(),
                act("write the value in the shape the key declares".to_owned()),
            ),
            CoercionFailure::Alias(alias) => (alias.to_string(), Remedy::from(alias.clone())),
        };
        Classified::UserCorrectable {
            statement: Statement::sanitised(said),
            remedy,
        }
    }
}

/// ADR-0007 D2's alias rules. An alias is a name the user chose.
impl From<AliasRefused> for Classified {
    fn from(refusal: AliasRefused) -> Self {
        let remedy = Remedy::from(refusal.clone());
        correctable(&refusal, remedy)
    }
}

/// The remedy for an alias, reachable from both the store and configuration.
impl From<AliasRefused> for Remedy {
    fn from(refusal: AliasRefused) -> Self {
        match refusal {
            AliasRefused::Empty => act("give the token an alias".to_owned()),
            AliasRefused::DotOrDotDot => {
                act("choose an alias that is not \".\" or \"..\"".to_owned())
            }
            AliasRefused::Separator { offered, found } => act(format!(
                "remove the {found:?} from {offered:?}; an alias is a name rather than a path"
            )),
            AliasRefused::NamespaceSeparator { offered } => act(format!(
                "remove the ':' from {offered:?}; ADR-0007 D5 separates the namespace with it"
            )),
            AliasRefused::Control { offered } => {
                act(format!("remove the control character from {offered:?}"))
            }
            AliasRefused::SurroundingWhitespace { offered } => act(format!(
                "remove the space at the start or end of {offered:?}"
            )),
        }
    }
}

/// ADR-0007 D2's two token kinds. **The remedy names neither the value nor its
/// length**, and neither does the refusal, which carries nothing at all.
impl From<SecretRefused> for Classified {
    fn from(refusal: SecretRefused) -> Self {
        correctable(
            &refusal,
            act("supply a Nuclear Notes token, which begins \"nn_mcp_\" or \"nn_app_\"".to_owned()),
        )
    }
}

/// ADR-0007 D2's one-line description.
impl From<DescriptionRefused> for Classified {
    fn from(refusal: DescriptionRefused) -> Self {
        let remedy = act(format!(
            "remove the control character from the description {:?}",
            refusal.offered
        ));
        correctable(&refusal, remedy)
    }
}

/// ADR-0011 D4's working directory. The user named a directory that is not
/// there, or one they cannot reach.
impl From<TreeError> for Classified {
    fn from(refusal: TreeError) -> Self {
        let remedy = match &refusal {
            TreeError::NoSuchWorkingDirectory { path, source: _ } => act(format!(
                "run from a directory that exists, or point the harness at one; it was given {}",
                path.display()
            )),
        };
        correctable(&refusal, remedy)
    }
}

/// ADR-0011 D3's permission mode.
impl From<ModeRefused> for Classified {
    fn from(refusal: ModeRefused) -> Self {
        let remedy = match &refusal {
            // ADR-0014 D6. The *project's* error, and the person who can act
            // is the user: the remedy names the file and the key. A delegated
            // coordinator ruling of 2026-09-04.
            ModeRefused::FromAClonedRepository { key, layer, .. } => act(format!(
                "remove {key:?} from the {layer}, and set the permission mode in your own \
                 configuration, the environment or a flag instead"
            )),
            ModeRefused::NoSuchMode { key, .. } => {
                act(format!("set {key:?} to \"ask\", \"allow\" or \"yolo\""))
            }
        };
        correctable(&refusal, remedy)
    }
}

/// ADR-0014's load-time refusals.
///
/// Eight of the ten are the user's and two are ours, and the two that are ours
/// are ours for reasons the source already states: a layer offered twice is
/// "a caller error rather than a user's", and two declared keys colliding on
/// one `ZARU_*` name is a collision in a schema this crate builds.
impl From<ConfigRefused> for Classified {
    fn from(refusal: ConfigRefused) -> Self {
        let remedy = match &refusal {
            // Ours, both of them. A defect report is built by the boundary
            // rather than here, because D3's report needs the version, where
            // to report and the session, none of which a classification holds.
            ConfigRefused::DuplicateLayer { .. }
            | ConfigRefused::AmbiguousEnvironmentName { .. } => {
                return Classified::Defect(DefectReport::new(
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_REPOSITORY"),
                    crate::failure::defect::Location::unknown(),
                    crate::failure::defect::SessionEvidence::NoSessionExists,
                ));
            }
            ConfigRefused::UnknownKey {
                layer,
                offered,
                suggestion,
            } => match suggestion {
                Some(nearest) => act(format!(
                    "change {offered:?} to {nearest:?} in the {layer}, or remove it"
                )),
                None => act(format!("remove {offered:?} from the {layer}")),
            },
            ConfigRefused::UnusableKey { layer, offered, .. } => act(format!(
                "correct the name {offered:?} in the {layer}, or remove it"
            )),
            ConfigRefused::CredentialShaped {
                layer,
                key,
                declared_as_a_reference,
            } => {
                if *declared_as_a_reference {
                    act(format!(
                        "{key} names a credential the store holds; put the alias there rather \
                         than the value, and add the token to the credential store"
                    ))
                } else {
                    act(format!(
                        "remove the value under {key} from the {layer} and add the token to the \
                         credential store; ADR-0014 D4 keeps credentials out of configuration \
                         because a config file gets committed"
                    ))
                }
            }
            ConfigRefused::WrongShape {
                layer,
                key,
                expected,
                found: _,
            } => act(format!("write {key} in the {layer} as {expected}")),
            ConfigRefused::UnparsableText {
                layer,
                key,
                expected,
            } => act(format!("write {key} in the {layer} as {expected}")),
            ConfigRefused::UnusableAlias {
                layer,
                key,
                refusal,
            } => {
                let named = Remedy::from(refusal.clone());
                act(format!(
                    "{key} in the {layer} names a credential: {}",
                    named
                        .actions()
                        .next()
                        .expect("a remedy always has a first action")
                        .lead()
                ))
            }
            // ADR-0014 D6. The project's error; the user is who can act.
            ConfigRefused::ProjectMayNotSet { key, reason } => act(format!(
                "remove {key} from ./zaru.toml -- {reason} -- and set it in your own \
                 configuration, the environment or a flag if you want it"
            )),
            ConfigRefused::ProjectMayNotRaise {
                key,
                granted,
                asked,
            } => act(format!(
                "lower {key} in ./zaru.toml to {granted} or less; it asked for {asked}, and \
                 raise it in your own configuration if you want {asked}"
            )),
        };
        correctable(&refusal, remedy)
    }
}

/// ADR-0011 D4's subject. The harness offered a URL-addressing tool a
/// filesystem path, which no user can cause.
impl From<InvocationRefused> for Classified {
    fn from(_refusal: InvocationRefused) -> Self {
        Classified::Defect(DefectReport::new(
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_REPOSITORY"),
            crate::failure::defect::Location::unknown(),
            crate::failure::defect::SessionEvidence::NoSessionExists,
        ))
    }
}
