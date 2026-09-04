// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks over the values a session is built from.
//!
//! Everything here is synchronous and needs no connection. What needs one — a
//! session driven end to end against a real MCP server — is in
//! `tests/notes_session_from_outside.rs`, which reaches this crate through its
//! public door only.

use crate::session::fixtures::{ascii_core, assert_absent, bearer_nonce};
use crate::session::{Attachment, AttachmentRefused, Bearer, REDACTED, WorkspaceId, WorkspaceSlug};

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

// -- ADR-0006 D6, the self-locating attachment ---------------------------

/// A well-formed attachment, in the shape this substrate actually returns.
fn staged_attachment(slug: &str, path: &str) -> Result<Attachment, AttachmentRefused> {
    Attachment::new(
        WorkspaceSlug::new(slug),
        path,
        format!("https://100monkeys-ai.cortex.page/{slug}/p/{path}"),
        format!("nn://workspace/{slug}/p/{path}"),
    )
}

#[test]
fn an_attachment_carries_all_four_of_adr_0006_d6s_parts() {
    let attachment = staged_attachment("zaru", "adrs/0006-nuclear-notes-surfaces")
        .expect("a well-formed attachment is accepted");

    assert_eq!(attachment.workspace().as_str(), "zaru");
    assert_eq!(attachment.path(), "adrs/0006-nuclear-notes-surfaces");
    assert_eq!(
        attachment.permalink(),
        "https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces"
    );
    assert_eq!(
        attachment.uri(),
        "nn://workspace/zaru/p/adrs/0006-nuclear-notes-surfaces"
    );
}

#[test]
fn an_attachment_that_cannot_locate_itself_is_refused_naming_which_part_failed() {
    let cases: Vec<(AttachmentRefused, AttachmentCase)> = vec![
        (
            AttachmentRefused::NoWorkspace,
            AttachmentCase {
                slug: "",
                path: "home",
                permalink: "https://host/x/p/home".to_owned(),
                uri: "nn://workspace/x/p/home".to_owned(),
            },
        ),
        (
            AttachmentRefused::NoPath,
            AttachmentCase {
                slug: "zaru",
                path: "",
                permalink: "https://host/zaru/p/".to_owned(),
                uri: "nn://workspace/zaru/p/".to_owned(),
            },
        ),
        (
            AttachmentRefused::PermalinkOmitsWorkspace,
            AttachmentCase {
                slug: "zaru",
                path: "home",
                // The permalink names a *different* workspace. Every part is
                // present and non-empty, so a check that only counted parts
                // would accept an attachment that resolves somewhere else.
                permalink: "https://host/promptly-game/p/home".to_owned(),
                uri: "nn://workspace/zaru/p/home".to_owned(),
            },
        ),
        (
            AttachmentRefused::PermalinkOmitsPath,
            AttachmentCase {
                slug: "zaru",
                path: "adrs/0006",
                permalink: "https://host/zaru/p/somewhere-else".to_owned(),
                uri: "nn://workspace/zaru/p/adrs/0006".to_owned(),
            },
        ),
        (
            AttachmentRefused::UriOmitsWorkspace,
            AttachmentCase {
                slug: "zaru",
                path: "home",
                permalink: "https://host/zaru/p/home".to_owned(),
                uri: "nn://workspace/promptly-game/p/home".to_owned(),
            },
        ),
        (
            AttachmentRefused::UriOmitsPath,
            AttachmentCase {
                slug: "zaru",
                path: "adrs/0006",
                permalink: "https://host/zaru/p/adrs/0006".to_owned(),
                uri: "nn://workspace/zaru/p/somewhere-else".to_owned(),
            },
        ),
    ];

    for (expected, case) in cases {
        let outcome = Attachment::new(
            WorkspaceSlug::new(case.slug),
            case.path,
            case.permalink.clone(),
            case.uri.clone(),
        );
        assert_eq!(
            outcome.err(),
            Some(expected),
            "an attachment with slug {:?}, path {:?}, permalink {:?} and uri {:?} should have been \
             refused as {expected:?}",
            case.slug,
            case.path,
            case.permalink,
            case.uri
        );
    }
}

struct AttachmentCase {
    slug: &'static str,
    path: &'static str,
    permalink: String,
    uri: String,
}

#[test]
fn an_attachments_refusal_names_adr_0006_d6s_reason_rather_than_only_a_field() {
    let rendered = AttachmentRefused::PermalinkOmitsWorkspace.to_string();
    assert!(
        rendered.contains("missing page"),
        "the refusal must say what goes wrong when an attachment cannot locate itself -- it comes \
         back as a missing page, which is the hardest failure to diagnose. {rendered:?}"
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
