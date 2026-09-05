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
//! # The nonces come from the credential store's fixtures, not from a copy
//!
//! [`crate::credentials::fixtures`] already carries the awkward nonce and the
//! ASCII core, and the reason the core exists is a mutation that survived
//! there. Retyping either here would put one rule in two places.

use crate::config::key::Key;
use crate::config::layer::{Contribution, Layer, Source};
use crate::config::port::{LayerSource, SourceFailure};
use crate::config::schema::{Field, FieldKind, Schema};
use crate::config::value::{Table, Value};

pub(crate) use crate::credentials::fixtures::{ascii_core, nonce, personal_secret_nonce};

/// A key, for a fixture that knows its own spellings are well formed.
pub(crate) fn key(text: &str) -> Key {
    Key::new(text).expect("a fixture key is well formed")
}

/// A document, from dotted keys and values.
pub(crate) fn document(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Table {
    let mut table = Table::new();
    for (path, value) in entries {
        table.insert_path(&key(path), value);
    }
    table
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
        // ADR-0001's key, asked for rather than transcribed. It was spelled
        // out here and in two outside-caller checks until 2026-09-05, with two
        // different reasons between them -- one rule in three places, which is
        // the divergence [Verification lessons] §27 names.
        .with(crate::runtime::key(), crate::runtime::field())
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

/// Every key the fixture schema refuses to the project layer, with the words
/// its refusal should carry.
///
/// Walked by the check rather than retyped in it: the population comes from
/// the schema that declares it, so a fifth refused key is covered the moment
/// it is declared ([Verification lessons] §17).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub(crate) fn keys_refused_to_projects() -> Vec<(Key, Value)> {
    vec![
        (key("runtime.tier"), text("linked")),
        (key("permission.mode"), text("yolo")),
        (key("seal.enabled"), Value::Bool(false)),
        (key("notes.token_scope"), text("full")),
    ]
}

/// One layer, staged with a document and a name.
pub(crate) fn at(layer: Layer, source: &str, document: Table) -> Contribution {
    Contribution::new(layer, Source::named(source), document)
}

/// A layer whose document a check owns, read through the product's own port.
///
/// [`LayerSource`] has no implementation in the product tree — a TOML parser
/// and an argument parser are two dependencies ADR-0003 D2's table does not
/// name. This is the check standing in as the caller those parsers will be.
pub(crate) struct StagedSource {
    layer: Layer,
    source: Source,
    document: Table,
}

impl StagedSource {
    /// Stage one layer.
    pub(crate) fn new(layer: Layer, source: &str, document: Table) -> Self {
        Self {
            layer,
            source: Source::named(source),
            document,
        }
    }
}

impl LayerSource for StagedSource {
    fn layer(&self) -> Layer {
        self.layer
    }

    fn source(&self) -> Source {
        self.source.clone()
    }

    fn read(&self) -> Result<Table, SourceFailure> {
        Ok(self.document.clone())
    }
}
