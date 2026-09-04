// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks over the values a session is built from.
//!
//! Everything here is synchronous and needs no connection. What needs one — a
//! session driven end to end against a real MCP server — is in
//! `tests/notes_session_from_outside.rs`, which reaches this crate through its
//! public door only.

use crate::session::fixtures::{ascii_core, assert_absent, bearer_nonce};
use crate::session::{Bearer, REDACTED, WorkspaceId, WorkspaceSlug};

// -- the bearer ----------------------------------------------------------

#[test]
fn a_bearers_debug_carries_the_redaction_and_not_the_value() {
    let planted = bearer_nonce();
    let bearer = Bearer::new(planted.clone());

    let rendered = format!("{bearer:?}");
    assert!(
        rendered.contains(REDACTED),
        "a redacted bearer must still say it was redacted; {rendered:?} carries no {REDACTED:?}. \
         An impl that printed nothing at all would satisfy the absence arm below on its own"
    );
    assert_absent("a bearer's Debug", &rendered, &planted);
}

#[test]
fn the_one_door_out_of_a_bearer_yields_the_value_it_was_given() {
    // The discriminating arm. Without it, a `Bearer` that dropped its value on
    // the floor would pass every redaction check above perfectly.
    let planted = bearer_nonce();
    let bearer = Bearer::new(planted.clone());
    assert_eq!(
        bearer.expose_for_dispatch(),
        planted,
        "expose_for_dispatch is the dispatch path; a bearer that does not carry its value \
         authenticates nothing"
    );
}

// -- addressing -----------------------------------------------------------

#[test]
fn a_slug_and_an_identifier_are_not_the_same_type() {
    // ADR-0006 D7 exists because slugs are unique per instance and identifiers
    // are not. The mechanism is the type: this compiles only because the two
    // are constructed separately, and `attach_workspace` accepts one of them.
    let slug = WorkspaceSlug::new("zaru");
    let id = WorkspaceId::new("a96c9dde-becf-4ff0-836e-ad8bef46ff42");
    assert_ne!(slug.as_str(), id.as_str());
    assert_eq!(slug.to_string(), "zaru");
    assert_eq!(id.to_string(), "a96c9dde-becf-4ff0-836e-ad8bef46ff42");
}

#[test]
fn the_ascii_core_of_a_nonce_survives_debug_escaping() {
    // The fixture's own contract, checked -- because every redaction assertion
    // in this crate leans on it. A `{:?}` rendering escapes the combining mark,
    // so the raw value is absent from a string that published all of it.
    let planted = bearer_nonce();
    let escaped = format!("{planted:?}");
    assert!(
        !escaped.contains(&planted),
        "if Debug did not escape the nonce, the second arm of assert_absent would be redundant \
         and this crate's redaction checks would be weaker than they look"
    );
    assert!(
        escaped.contains(ascii_core(&planted)),
        "the ASCII core must survive escaping, or it cannot catch an escaped leak"
    );
}
