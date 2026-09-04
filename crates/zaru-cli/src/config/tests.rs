// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The configuration hierarchy's checks, clause by clause.
//!
//! Every check here names the ADR-0014 clause it holds and the mutant that
//! would make it redden. Where a mutant is named in a comment it has been
//! run: the failure sentence is quoted in the commit that carries the check.

use super::fixtures::{key, schema, text};
use crate::config::credential::CredentialRef;
use crate::config::environment;
use crate::config::key::Key;
use crate::config::layer::Layer;
use crate::config::refusal::ConfigRefused;
use crate::config::schema::{Field, FieldKind, Schema};
use crate::config::value::{Table, Value};

/// `Ord` and therefore the whole precedence rule.
#[test]
fn the_layers_are_ordered_lowest_to_highest_as_d1_lists_them() {
    assert_eq!(
        Layer::ALL,
        [
            Layer::BuiltIn,
            Layer::User,
            Layer::Project,
            Layer::Environment,
            Layer::Flag,
        ],
    );
    let numbers: Vec<u8> = Layer::ALL.iter().map(|layer| layer.number()).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5]);

    for pair in Layer::ALL.windows(2) {
        assert!(
            pair[0] < pair[1],
            "layer {} ({}) does not sort below layer {} ({}), so `higher wins` is not the enum's \
             own order",
            pair[0].number(),
            pair[0].label(),
            pair[1].number(),
            pair[1].label(),
        );
    }
}

/// and replaces everything below it.
#[test]
fn a_nested_table_merges_at_every_depth() {
    let mut lower = Table::new();
    let mut inner = Table::new();
    inner.insert("kept", text("from-below"));
    inner.insert("replaced", text("from-below"));
    lower.insert("labels", Value::Table(inner));

    let mut higher = Table::new();
    let mut higher_inner = Table::new();
    higher_inner.insert("replaced", text("from-above"));
    higher.insert("labels", Value::Table(higher_inner));

    lower.merge_over(higher);

    let Some(Value::Table(merged)) = lower.get("labels") else {
        panic!("`labels` stopped being a table")
    };
    assert_eq!(
        merged.get("kept"),
        Some(&text("from-below")),
        "a sibling one level down did not survive the merge",
    );
    assert_eq!(
        merged.get("replaced"),
        Some(&text("from-above")),
        "the higher layer's own nested key did not win",
    );
}

/// check compiling.
#[test]
fn a_credential_reference_is_one_alias_and_a_second_field_would_not_compile() {
    let alias = crate::credentials::Alias::new("work").expect("a well-formed alias");
    let reference = CredentialRef::new(alias.clone());
    let CredentialRef { alias: only } = reference;
    assert_eq!(only, alias);
}

/// The mutant is ignoring a `ZARU_*` variable that maps to no declared key.
#[test]
fn an_unknown_environment_variable_is_refused_in_the_environments_own_vocabulary() {
    let refusal = environment::read(
        &schema(),
        vec![("ZARU_RUNTIEM_MAX_ITERATIONS".to_owned(), "8".to_owned())],
    )
    .expect_err("a mistyped ZARU_ variable was read without complaint");

    let ConfigRefused::UnknownKey {
        layer,
        offered,
        suggestion,
    } = &refusal
    else {
        panic!("the refusal was {refusal:?}")
    };
    assert_eq!(*layer, Layer::Environment);
    assert_eq!(offered, "ZARU_RUNTIEM_MAX_ITERATIONS");
    assert_eq!(
        suggestion.as_deref(),
        Some("ZARU_RUNTIME_MAX_ITERATIONS"),
        "the suggestion must be a variable name, not a dotted key",
    );
}

/// transform is what is built; the divergence is recorded on the record.
#[test]
fn the_variable_a_key_maps_to_is_zaru_plus_the_key_upper_cased() {
    assert_eq!(
        environment::variable_name(&key("runtime.max_iterations")),
        "ZARU_RUNTIME_MAX_ITERATIONS",
    );
    assert_eq!(
        environment::variable_name(&key("provider.credential")),
        "ZARU_PROVIDER_CREDENTIAL",
    );
    assert_ne!(
        environment::variable_name(&key("runtime.max_iterations")),
        "ZARU_MAX_ITER",
        "D3's worked example is not producible by this transform, and that is recorded on the \
         record rather than worked around here",
    );
}

/// is refused rather than resolved by picking one.
#[test]
fn a_schema_whose_keys_collide_on_one_variable_is_refused() {
    let colliding = Schema::new()
        .with(key("runtime.max_iterations"), Field::ceiling())
        .with(key("runtime.max.iterations"), Field::ceiling());

    let refusal = environment::read(&colliding, Vec::new())
        .expect_err("two keys produced one variable and the read accepted it");

    assert!(
        matches!(refusal, ConfigRefused::AmbiguousEnvironmentName { .. }),
        "the refusal was {refusal:?}",
    );
}

/// A variable outside the prefix is not this hierarchy's business.
#[test]
fn a_variable_outside_the_prefix_is_ignored() {
    let document = environment::read(
        &schema(),
        vec![
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("HOME".to_owned(), "/home/somebody".to_owned()),
        ],
    )
    .expect("variables outside the prefix are not read");

    assert!(
        document.is_empty(),
        "layer 4 is `ZARU_*` and nothing else; it read {document:?}",
    );
}

/// A key is refused the shapes D3's block cannot render.
#[test]
fn a_key_with_an_empty_segment_is_refused() {
    assert!(Key::new("runtime..max").is_err());
    assert!(Key::new(".runtime").is_err());
    assert!(Key::new("runtime.").is_err());
    assert!(Key::new("").is_err());
    assert!(Key::new("runtime.max_iterations").is_ok());
}

/// deliberately absent.
#[test]
fn no_kind_parses_text_into_a_list() {
    assert!(
        FieldKind::Array
            .coerce(Value::Text("a,b,c".to_owned()))
            .is_err(),
        "no record says how an environment variable expresses a list, and inventing a separator \
         here would settle it",
    );
    assert!(
        FieldKind::Integer
            .coerce(Value::Text("7".to_owned()))
            .is_ok()
    );
    assert!(
        FieldKind::Bool
            .coerce(Value::Text("true".to_owned()))
            .is_ok()
    );
}
