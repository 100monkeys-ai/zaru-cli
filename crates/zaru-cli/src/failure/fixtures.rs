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
