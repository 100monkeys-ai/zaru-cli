// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Values the checks on this module are built from. Compiled only under
//! `cfg(test)`.
//!
//! The nonces come from [`credentials::fixtures`](crate::credentials::fixtures)
//! rather than being generated again here, because the reasoning behind their
//! shape — and behind [`ascii_core`](crate::credentials::fixtures::ascii_core)
//! in particular — was paid for by a mutation that survived on 2026-09-04, and
//! a second copy of it beside these checks would be a rule living in two
//! places.

use crate::credentials::fixtures::nonce;
use crate::failure::class::Class;
use crate::failure::classified::{Classified, Expected};
use crate::failure::defect::{DefectReport, Location, SessionEvidence, SessionId};
use crate::failure::remedy::{Action, Remedy, Statement};
use crate::failure::wait::{Backoff, RetryCeiling, RetryPolicy, RetryRecord, Wait};
use crate::tools::Tier;
use core::time::Duration;
use std::path::PathBuf;

/// A statement carrying a nonce, so a check cannot pass by matching a
/// constant somebody hard-coded.
pub(super) fn statement(label: &str) -> Statement {
    Statement::new(nonce(label)).expect("a nonce carries no control character")
}

/// A retry policy with two distinct numbers, so a rendering that printed the
/// ceiling where the count belongs is visible.
pub(super) fn policy() -> RetryPolicy {
    RetryPolicy {
        ceiling: RetryCeiling::new(5).expect("5 is not zero"),
        backoff: Backoff::new(Duration::from_millis(250)).expect("250ms is not zero"),
    }
}

/// One failure of each of D1's five classes, in the record's own order.
///
/// **Built from `Class::ALL` rather than from a list retyped beside it**, so a
/// sixth class fails to compile here as well as in the taxonomy — the same
/// signal `zaru-core`'s state-set check and `config`'s layer table already
/// give.
pub(super) fn one_of_each_class() -> Vec<(Class, Classified)> {
    Class::ALL
        .into_iter()
        .map(|class| (class, of_class(class)))
        .collect()
}

/// A failure of exactly this class.
///
/// Exhaustive with no wildcard arm: a sixth class cannot arrive without a
/// fixture being written for it, so no check can silently stop covering one.
pub(super) fn of_class(class: Class) -> Classified {
    match class {
        Class::Expected => Classified::Expected(Expected::new(statement("expected"))),
        Class::UserCorrectable => Classified::UserCorrectable {
            statement: statement("user-correctable"),
            remedy: two_action_remedy(),
        },
        Class::Environmental => Classified::Environmental {
            statement: statement("environmental"),
            wait: Wait::Retrying(
                RetryRecord::new(policy(), 2).expect("2 is within a ceiling of 5"),
            ),
        },
        Class::Capability => Classified::Capability {
            statement: statement("capability"),
            offered_by: Tier::Contained,
        },
        Class::Defect => Classified::Defect(defect_report(SessionEvidence::NoSessionExists)),
    }
}

/// D2's worked example has two actions — a `set one:` and an `or:` — so the
/// fixture has two. A one-action remedy cannot tell a renderer that shows
/// every action from one that shows only the first.
pub(super) fn two_action_remedy() -> Remedy {
    Remedy::one(
        Action::runnable(statement("set one"), nonce("first-command"))
            .expect("a nonce carries no control character"),
    )
    .also(
        Action::runnable(statement("or"), nonce("second-command"))
            .expect("a nonce carries no control character"),
    )
}

/// A defect report against whichever session evidence a check wants to stage.
pub(super) fn defect_report(session: SessionEvidence) -> DefectReport {
    DefectReport::new(
        nonce("version"),
        nonce("report-at"),
        Location {
            file: nonce("file"),
            line: 41,
            column: 7,
        },
        session,
    )
}

/// ADR-0010 D1's session as it will be, staged by a check because nothing in
/// any product tree supplies one.
pub(super) fn a_session() -> SessionEvidence {
    SessionEvidence::Session {
        id: SessionId::new(nonce("session")).expect("a nonce carries no control character"),
        transcript: PathBuf::from(format!("/nowhere/{}/transcript.jsonl", nonce("dir"))),
    }
}

/// Every variant of every enum [`classify`](crate::failure::classify) maps,
/// with the class a record states for it.
///
/// **This list is the denominator for trigger clause 3's enumeration**, and
/// the compiler is what keeps it honest in the other direction: each mapping
/// is a wildcard-free match, so a new variant fails to compile there and the
/// row for it has to be written here before it can be classified at all.
pub(super) fn every_mapped_refusal() -> Vec<(&'static str, Classified, Class)> {
    use crate::config::{CoercionFailure, ConfigRefused, KeyRefused, Layer};
    use crate::credentials::{AliasRefused, DescriptionRefused, SecretRefused};
    use crate::tools::{InvocationRefused, ModeRefused, ToolName, TreeError};

    let key = |name: &str| crate::config::Key::new(name).expect("a declared key is a usable key");
    let offered = || nonce("offered");

    let mut rows: Vec<(&'static str, Classified, Class)> = Vec::new();
    let mut user = |name: &'static str, classified: Classified| {
        rows.push((name, classified, Class::UserCorrectable));
    };

    // ADR-0014 D5's key rules -- four variants.
    user("KeyRefused::Empty", KeyRefused::Empty.into());
    user(
        "KeyRefused::EmptySegment",
        KeyRefused::EmptySegment { offered: offered() }.into(),
    );
    user(
        "KeyRefused::Control",
        KeyRefused::Control { offered: offered() }.into(),
    );
    user(
        "KeyRefused::SurroundingWhitespace",
        KeyRefused::SurroundingWhitespace { offered: offered() }.into(),
    );

    // ADR-0014 D2's coercions -- three variants.
    user(
        "CoercionFailure::WrongShape",
        CoercionFailure::WrongShape { found: "a list" }.into(),
    );
    user(
        "CoercionFailure::Unparsable",
        CoercionFailure::Unparsable.into(),
    );
    user(
        "CoercionFailure::Alias",
        CoercionFailure::Alias(AliasRefused::Empty).into(),
    );

    // ADR-0007 D2's alias rules -- six variants.
    user("AliasRefused::Empty", AliasRefused::Empty.into());
    user(
        "AliasRefused::DotOrDotDot",
        AliasRefused::DotOrDotDot.into(),
    );
    user(
        "AliasRefused::Separator",
        AliasRefused::Separator {
            offered: offered(),
            found: '/',
        }
        .into(),
    );
    user(
        "AliasRefused::NamespaceSeparator",
        AliasRefused::NamespaceSeparator { offered: offered() }.into(),
    );
    user(
        "AliasRefused::Control",
        AliasRefused::Control { offered: offered() }.into(),
    );
    user(
        "AliasRefused::SurroundingWhitespace",
        AliasRefused::SurroundingWhitespace { offered: offered() }.into(),
    );

    // ADR-0007 D2's two token kinds, and its one-line description.
    user(
        "SecretRefused::NoNotesPrefix",
        SecretRefused::NoNotesPrefix.into(),
    );
    user("SecretRefused::Empty", SecretRefused::Empty.into());
    user("SecretRefused::Control", SecretRefused::Control.into());
    user(
        "SecretRefused::SurroundingWhitespace",
        SecretRefused::SurroundingWhitespace.into(),
    );
    user(
        "DescriptionRefused",
        DescriptionRefused { offered: offered() }.into(),
    );

    // ADR-0011 D4's working directory, and D3's permission mode.
    user(
        "TreeError::NoSuchWorkingDirectory",
        TreeError::NoSuchWorkingDirectory {
            path: PathBuf::from(format!("/nowhere/{}", nonce("wd"))),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        }
        .into(),
    );
    user(
        "ModeRefused::FromAClonedRepository",
        ModeRefused::FromAClonedRepository {
            key: nonce("mode-key"),
            offered: offered(),
            layer: Layer::Project,
        }
        .into(),
    );
    user(
        "ModeRefused::NoSuchMode",
        ModeRefused::NoSuchMode {
            key: nonce("mode-key"),
            offered: offered(),
        }
        .into(),
    );

    // ADR-0014's load-time refusals -- eight of the user's.
    user(
        "ConfigRefused::UnknownKey (with a suggestion)",
        ConfigRefused::UnknownKey {
            layer: Layer::User,
            offered: offered(),
            suggestion: Some(nonce("nearest")),
        }
        .into(),
    );
    user(
        "ConfigRefused::UnknownKey (with none)",
        ConfigRefused::UnknownKey {
            layer: Layer::Project,
            offered: offered(),
            suggestion: None,
        }
        .into(),
    );
    user(
        "ConfigRefused::UnusableKey",
        ConfigRefused::UnusableKey {
            layer: Layer::User,
            offered: offered(),
            refusal: KeyRefused::Empty,
        }
        .into(),
    );
    user(
        "ConfigRefused::CredentialShaped (declared as a reference)",
        ConfigRefused::CredentialShaped {
            layer: Layer::User,
            key: key("notes.token"),
            declared_as_a_reference: true,
        }
        .into(),
    );
    user(
        "ConfigRefused::CredentialShaped (in an ordinary key)",
        ConfigRefused::CredentialShaped {
            layer: Layer::Project,
            key: key("project.name"),
            declared_as_a_reference: false,
        }
        .into(),
    );
    user(
        "ConfigRefused::WrongShape",
        ConfigRefused::WrongShape {
            layer: Layer::User,
            key: key("runtime.max_iterations"),
            expected: "a whole number",
            found: "text",
        }
        .into(),
    );
    user(
        "ConfigRefused::UnparsableText",
        ConfigRefused::UnparsableText {
            layer: Layer::Environment,
            key: key("runtime.max_iterations"),
            expected: "a whole number",
        }
        .into(),
    );
    user(
        "ConfigRefused::UnusableAlias",
        ConfigRefused::UnusableAlias {
            layer: Layer::User,
            key: key("notes.token"),
            refusal: AliasRefused::DotOrDotDot,
        }
        .into(),
    );
    user(
        "ConfigRefused::ProjectMayNotSet",
        ConfigRefused::ProjectMayNotSet {
            key: key("runtime.tier"),
            reason: nonce("reason"),
        }
        .into(),
    );
    user(
        "ConfigRefused::ProjectMayNotRaise",
        ConfigRefused::ProjectMayNotRaise {
            key: key("runtime.max_iterations"),
            granted: 5,
            asked: 8,
        }
        .into(),
    );

    // Ours, all three.
    rows.push((
        "ConfigRefused::DuplicateLayer",
        ConfigRefused::DuplicateLayer {
            layer: Layer::Project,
        }
        .into(),
        Class::Defect,
    ));
    rows.push((
        "ConfigRefused::AmbiguousEnvironmentName",
        ConfigRefused::AmbiguousEnvironmentName {
            variable: nonce("ZARU_VAR"),
            first: key("runtime.max_iterations"),
            second: key("runtime.max.iterations"),
        }
        .into(),
        Class::Defect,
    ));
    rows.push((
        "InvocationRefused",
        InvocationRefused {
            tool: ToolName::WebFetch,
        }
        .into(),
        Class::Defect,
    ));

    rows
}
