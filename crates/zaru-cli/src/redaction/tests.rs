// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the one product redactor does, and what it refuses to say.
//!
//! Every credential here is a generated nonce from `credentials::fixtures`,
//! carrying a decomposed grapheme cluster, a precomposed one and an
//! astral-plane character. A nonce is a uniqueness device rather than a
//! secret, and the awkwardness is what makes an absence assertion mean
//! something: see that module for the mutation it answers.

use crate::credentials::alias::Alias;
use crate::credentials::entry::{Description, Entry, Instance, Reach, ToolScope};
use crate::credentials::fixtures::{
    InMemorySecrets, ScratchRoot, app_secret_nonce, ascii_core as fixture_ascii_core, nonce,
    personal_secret_nonce,
};
use crate::credentials::secret::Secret;
use crate::credentials::store::CredentialStore;
use crate::redaction::{HeldSecrets, ascii_core, held_secrets_for_redaction, marker};
use std::borrow::Cow;
use zaru_core::redaction::{Redacted, Redactor};

/// A store on its own scratch root holding one entry per supplied value.
///
/// Returns the store, the sealer it was sealed through, and the aliases in
/// the order the values were given.
fn store_holding(
    scratch: &ScratchRoot,
    values: &[String],
) -> (CredentialStore, InMemorySecrets, Vec<Alias>) {
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let mut aliases = Vec::new();
    for (index, value) in values.iter().enumerate() {
        // Deliberately **not** a nonce. An alias from the same generator as
        // the secret shares its pid and its timestamp, so a marker naming it
        // legitimately carries bytes that also appear in the value -- which
        // made the marker check below fail on its own staging rather than on
        // the product. Each check owns its scratch root, so a short fixed
        // name is unique where it has to be.
        let alias = Alias::new(&format!("held{index}")).expect("a plain name is a legal alias");
        let entry = Entry::new(
            alias.clone(),
            Description::new(format!("held {index}, {}", nonce("purpose"))).expect("one line"),
            Secret::new(value.clone()).expect("the fixture prefixes name a kind"),
            Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
        )
        .with_tools(ToolScope::new(["pages.read"]));
        store
            .add(entry, &mut sealer, None)
            .expect("an entry is added");
        aliases.push(alias);
    }
    (store, sealer, aliases)
}

#[test]
fn a_held_value_and_its_ascii_core_are_replaced_by_a_marker_naming_the_alias() {
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let core = ascii_core(&value);
    assert!(
        !core.is_empty() && core != value,
        "the fixture must produce an ASCII core distinct from the value, or \
         the escaped-form arm of this check asserts nothing: {value:?}"
    );

    let (store, sealer, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &sealer).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "one entry is one held value");

    // The expected marker is composed here from the alias, not read back out
    // of the redactor. An assertion phrased in the quantity under test cannot
    // survive its own mutation -- Verification lessons §11, and the shape a
    // mutation actually exploited in `zaru-core`'s own redaction checks.
    let expected = format!("<redacted: {}>", aliases[0]);
    assert_eq!(marker(&aliases[0]), expected);

    // The second text is the **real** escaped rendering rather than one
    // typed by hand: `{:?}` is what a Debug in a refusal, a panic or an
    // assertion failure would produce, and it is the exact form the mutation
    // ADR-0007's Status tracking records survived through.
    for text in [
        format!("cmd.run failed: Authorization: Bearer {value}"),
        format!("cmd.run failed: Authorization: Bearer {value:?}"),
    ] {
        let redacted = Redacted::by(&held, &text);
        assert!(
            !redacted.as_str().contains(&value),
            "the bearer value reached the model: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(core),
            "the bearer value's ASCII core reached the model, so an escaping \
             renderer would publish it: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(fixture_ascii_core(&value)),
            "the credential fixtures' own notion of an ASCII core -- the \
             value with its awkward tail stripped -- reached the model. The \
             two definitions differ and both must be absent: {:?}",
            redacted.as_str()
        );
        assert!(
            redacted.as_str().contains(&expected),
            "nothing marks where the value was, and a redactor that erased \
             its whole input would satisfy both assertions above on its own: \
             {:?}",
            redacted.as_str()
        );
    }
}

#[test]
fn the_marker_names_the_alias_and_carries_nothing_of_the_value() {
    // ADR-0007 D2 makes the alias "a local unique name. The handle
    // everywhere", already shown to the human and to the agent, so naming it
    // tells a reader which credential was in the text. The value is what may
    // never travel -- the same rule `SecretRefused` holds, and for the same
    // reason: a marker is exactly the text that gets pasted into a report.
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let (store, sealer, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &sealer).expect("the store yields its secret");

    let redacted = Redacted::by(&held, &format!("here it is: {value}"));
    let written = marker(&aliases[0]);
    assert!(redacted.as_str().contains(&written));
    assert!(
        written.contains(aliases[0].as_str()),
        "the marker does not name the alias, so a reader cannot tell which \
         credential was in the text: {written:?}"
    );
    // Every window of the value that is long enough to identify it. A marker
    // that carried a prefix, a suffix or a middle slice would fail here while
    // passing a whole-value assertion. The alias is a plain name rather than
    // a nonce for exactly this reason -- see `store_holding`.
    for window in 8..=value.len() {
        for start in 0..=value.len().saturating_sub(window) {
            let end = start + window;
            if !value.is_char_boundary(start) || !value.is_char_boundary(end) {
                continue;
            }
            assert!(
                !written.contains(&value[start..end]),
                "the marker carries {} bytes of the value: {written:?}",
                end - start
            );
        }
    }
}

#[test]
fn a_secret_that_is_a_prefix_of_another_cannot_leave_its_tail_behind() {
    // Longest first. Replacing the shorter value first would rewrite the
    // longer one's head and leave its remaining bytes in the text -- a
    // partial bearer, published by a code path that ran the redactor.
    let scratch = ScratchRoot::new();
    let short = personal_secret_nonce();
    let long = format!("{short}-and-more");
    let (store, sealer, _) = store_holding(&scratch, &[short.clone(), long.clone()]);
    let held = held_secrets_for_redaction(&store, &sealer).expect("the store yields its secrets");
    assert_eq!(held.len(), 2);

    let redacted = Redacted::by(&held, &format!("the token is {long} exactly"));
    assert!(
        !redacted.as_str().contains("-and-more"),
        "the longer secret's tail survived, so the shorter one was replaced \
         first and cut it in half: {:?}",
        redacted.as_str()
    );
    assert!(!redacted.as_str().contains(&short));
    assert!(!redacted.as_str().contains(&long));
}

#[test]
fn a_harness_holding_nothing_carries_every_byte_through() {
    // The arm that discriminates every absence assertion in this crate and in
    // `zaru-core`. Without it a redactor that erased its input passes them all.
    let scratch = ScratchRoot::new();
    let (store, sealer, _) = store_holding(&scratch, &[]);
    let held = held_secrets_for_redaction(&store, &sealer).expect("an empty store yields nothing");
    assert!(held.is_empty());

    let text = format!(
        "stdout: {}\nstderr: {}\n",
        app_secret_nonce(),
        nonce("other")
    );
    assert!(matches!(held.redact(&text), Cow::Borrowed(_)));
    assert_eq!(Redacted::by(&held, &text).as_str(), text);
    assert_eq!(Redacted::by(&HeldSecrets::none(), &text).as_str(), text);
}

#[test]
fn the_debug_of_held_secrets_carries_a_count_and_never_a_value() {
    // The mutant is one word: `#[derive(Debug)]` here puts every held bearer
    // into every `{:?}`, every `assert_eq!` failure and every panic message
    // in the program. `Secret`'s own `Debug` carries the same sentence.
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let (store, sealer, _) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &sealer).expect("the store yields its secret");

    let rendered = format!("{held:?}");
    assert!(
        !rendered.contains(&value),
        "a HeldSecrets Debug published a bearer value: {rendered}"
    );
    assert!(
        !rendered.contains(ascii_core(&value)),
        "a HeldSecrets Debug published a bearer value in an escaped form: \
         {rendered}"
    );
    assert!(
        rendered.contains("HeldSecrets(1 held)"),
        "a Debug that rendered nothing at all would satisfy both assertions \
         above on its own: {rendered}"
    );
}

#[test]
fn nothing_here_matches_a_pattern_and_an_unheld_secret_is_carried_through() {
    // The decision's own out-of-scope sentence, as a check rather than as a
    // comment. A value that is shaped exactly like a bearer -- right prefix,
    // right length -- but that the harness does not hold reaches the model
    // unaltered, because the harness redacts what it holds and looks for
    // nothing else. A future pattern matcher reddens here, which is the point.
    let scratch = ScratchRoot::new();
    let held_value = personal_secret_nonce();
    let unheld = app_secret_nonce();
    let (store, sealer, _) = store_holding(&scratch, std::slice::from_ref(&held_value));
    let held = held_secrets_for_redaction(&store, &sealer).expect("the store yields its secret");

    let text = format!("held {held_value} and unheld {unheld}");
    let redacted = Redacted::by(&held, &text);
    assert!(
        !redacted.as_str().contains(&held_value),
        "the held value was not redacted: {:?}",
        redacted.as_str()
    );
    assert!(
        redacted.as_str().contains(&unheld),
        "a value the harness does not hold was redacted, which means \
         something here is matching a pattern. ADR-0008's decision of \
         2026-09-05 names unknown secrets in command output as out of scope: \
         {:?}",
        redacted.as_str()
    );
}
