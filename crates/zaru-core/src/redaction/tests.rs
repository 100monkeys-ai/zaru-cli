// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the port promises, and what the type promises.

use crate::redaction::fixtures::{HoldingOne, NothingHeld, ascii_core, staged_secret};
use crate::redaction::{Redacted, Redactor};
use std::borrow::Cow;

#[test]
fn text_with_nothing_to_redact_is_borrowed_rather_than_rebuilt() {
    // The trait's contract, and it is load-bearing rather than cosmetic: a
    // caller distinguishes "nothing was redacted" from "something was"
    // without comparing strings, and the ordinary case allocates nothing.
    let holding = HoldingOne::new(staged_secret(), "work");
    assert!(
        matches!(holding.redact("nothing secret here"), Cow::Borrowed(_)),
        "a redactor that matched nothing must borrow, so that a caller can \
         tell an untouched prompt from a rebuilt one"
    );
    assert!(
        matches!(NothingHeld.redact("anything at all"), Cow::Borrowed(_)),
        "a redactor holding nothing must borrow everything"
    );
}

#[test]
fn a_held_value_and_its_ascii_core_are_both_replaced_by_the_marker() {
    let secret = staged_secret();
    let core = ascii_core(&secret);
    assert!(
        !core.is_empty() && core != secret,
        "the staged secret must have an ASCII core distinct from itself, or \
         the second arm of this check asserts nothing: {secret:?}"
    );
    let holding = HoldingOne::new(secret.clone(), "work");

    // The expected marker is a **literal owned by this check**, not
    // `holding.marker()`. A first draft asserted the latter and a mutation
    // that emptied the fixture's marker survived, because `contains("")` is
    // true of everything -- the assertion was phrased in the quantity under
    // test, which is the shape Verification lessons §11 names. The literal
    // cannot be moved by any mutation of the redactor.
    let expected_marker = "<redacted: work>";
    assert_eq!(
        holding.marker(),
        expected_marker,
        "the fixture's marker moved away from the text this check asserts, so \
         every assertion below would be about a different string"
    );

    // Two texts, because a value published verbatim and a value published in
    // an escaped form are different failures and only one of them is caught
    // by an assertion written against the value as typed. ADR-0007's Status
    // tracking records the mutation that survived exactly this gap.
    for text in [
        format!("stderr: authentication failed for {secret}"),
        format!("stderr: authentication failed for {core}\\u{{301}}"),
    ] {
        let redacted = Redacted::by(&holding, &text);
        assert!(
            !redacted.as_str().contains(&secret),
            "the held value survived redaction: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(core),
            "the held value's ASCII core survived redaction, so an escaping \
             renderer would publish it: {:?}",
            redacted.as_str()
        );
        assert!(
            redacted.as_str().contains(expected_marker),
            "nothing marks where the value was; a redaction that erased the \
             whole string would satisfy the two assertions above on its own: \
             {:?}",
            redacted.as_str()
        );
    }
}

#[test]
fn redaction_is_idempotent_because_a_marker_carries_no_value() {
    // Two paths rely on this: they redact their parts before truncating, so
    // a secret cannot survive as a fragment across an elision, and then
    // redact the assembled whole so that the value the type carries was
    // produced by the port.
    let secret = staged_secret();
    let holding = HoldingOne::new(secret.clone(), "work");
    let once = Redacted::by(&holding, &format!("the token is {secret}, use it"));
    let twice = Redacted::by(&holding, once.as_str());
    assert_eq!(
        once, twice,
        "a second pass changed the text, so a marker carries something the \
         redactor still recognises"
    );
}

#[test]
fn a_redactor_holding_nothing_carries_the_bytes_through_unaltered() {
    // The discriminating arm for every absence assertion in the workspace.
    // Without it, a redactor that erased its whole input passes all of them.
    let secret = staged_secret();
    let text = format!("stdout: {secret}\nstderr:\n");
    let redacted = Redacted::by(&NothingHeld, &text);
    assert_eq!(
        redacted.as_str(),
        text,
        "a redactor holding nothing must be the identity, byte for byte"
    );
    assert_eq!(redacted.len(), text.len());
    assert!(!redacted.is_empty());
}
