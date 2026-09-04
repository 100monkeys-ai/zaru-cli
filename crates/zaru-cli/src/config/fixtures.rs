// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the configuration checks are built from. Compiled only under
//! `cfg(test)`.
//!
//! # The schema here is a fixture and not the product's
//!
//! ADR-0014's Neutral section leaves the key set to the records that own the
//! keys, so the product declares none and neither does this. What is here is
//! a *shape*: keys of every kind, and one key per [`ProjectPolicy`] variant
//! including four that D6 refuses to projects — because the record's own
//! Status tracking says each of D6's escalations "wants its own assertion
//! rather than one test covering 'escalation is rejected'", and a fixture
//! with one refused key cannot tell those two implementations apart.
//!

use crate::config::key::Key;
use crate::config::schema::{Field, FieldKind, Schema};
use crate::config::value::Value;

/// A key, for a fixture that knows its own spellings are well formed.
pub(crate) fn key(text: &str) -> Key {
    Key::new(text).expect("a fixture key is well formed")
}

/// Text, spelled once.
pub(crate) fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}

/// The keys the checks resolve against.
///
/// One key per [`FieldKind`], and one per
/// [`ProjectPolicy`](crate::config::schema::ProjectPolicy) variant — with
/// **four** refused keys rather than one, so that a ceiling which refused
/// only the first would redden.
pub(crate) fn schema() -> Schema {
    Schema::new()
        // Free, of every shape a value can take.
        .with(key("project.name"), Field::free(FieldKind::Text))
        .with(key("project.workspace"), Field::free(FieldKind::Text))
        .with(key("project.verbose"), Field::free(FieldKind::Bool))
        .with(key("project.validators"), Field::free(FieldKind::Array))
        .with(key("project.labels"), Field::free(FieldKind::Table))
        // A whole number the project layer may set in either direction. D3's
        // block is rendered over this rather than over the ceiling, because
        // D3's own worked example raises a ceiling and D6 refuses that -- see
        // `adr_0014_d3s_worked_example_is_refused_by_adr_0014_d6`.
        .with(key("runtime.log_lines"), Field::free(FieldKind::Integer))
        .with(
            key("provider.credential"),
            Field::free(FieldKind::CredentialAlias),
        )
        // D6's permitted direction.
        .with(key("runtime.max_iterations"), Field::ceiling())
        // D6's four escalations, each its own declaration.
        .with(
            key("runtime.tier"),
            Field::refused_to_projects(
                FieldKind::Text,
                "the runtime tier is the membrane the user chose, and ADR-0001 D2 fixes it at \
                 session start",
            ),
        )
        .with(
            key("permission.mode"),
            Field::refused_to_projects(
                FieldKind::Text,
                "the permission mode is the user's, and ADR-0011 D3 defaults it to the safe one",
            ),
        )
        .with(
            key("seal.enabled"),
            Field::refused_to_projects(
                FieldKind::Bool,
                "SEAL verification is the membrane's own check and a repository cannot switch it \
                 off",
            ),
        )
        .with(
            key("notes.token_scope"),
            Field::refused_to_projects(
                FieldKind::Text,
                "a token's scope is what the user granted it, and ADR-0007 D6 has the server \
                 enforce it",
            ),
        )
}
