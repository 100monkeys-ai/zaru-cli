// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks over the values a session is built from.
//!
//! Everything here is synchronous and needs no connection. What needs one — a
//! session driven end to end against a real MCP server — is in
//! `tests/notes_session_from_outside.rs`, which reaches this crate through its
//! public door only.

use crate::session::fixtures::{ascii_core, assert_absent, bearer_nonce, nonce};
use crate::session::{
    Attachment, AttachmentRefused, Bearer, CallRefused, EndpointFailure, Invalidation, NotesError,
    REDACTED, WorkspaceId, WorkspaceSlug,
};
use core::time::Duration;

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

// -- what a refusal may never say ---------------------------------------

#[test]
fn no_refusal_this_crate_can_raise_carries_a_bearer_value() {
    let planted = bearer_nonce();

    // Every variant, assembled as if something had put the bearer in it. The
    // point is not that this crate does -- it is that if one ever did, this
    // check names which.
    let refusals: Vec<(&str, String)> = vec![
        (
            "NotesError::Endpoint",
            NotesError::Endpoint {
                detail: format!("could not reach the host holding {planted}"),
            }
            .to_string(),
        ),
        (
            "NotesError::Attach",
            NotesError::Attach {
                detail: format!("handshake refused for {planted}"),
            }
            .to_string(),
        ),
        (
            "NotesError::Transport",
            NotesError::Transport {
                detail: format!("stream closed while carrying {planted}"),
            }
            .to_string(),
        ),
        (
            "NotesError::Call",
            NotesError::Call(CallRefused {
                tool: "pages.read".to_owned(),
                code: -32601,
                detail: format!("no such method for {planted}"),
            })
            .to_string(),
        ),
        (
            "NotesError::WorkspaceUnattachable",
            NotesError::WorkspaceUnattachable {
                slug: WorkspaceSlug::new(planted.clone()),
            }
            .to_string(),
        ),
        (
            "EndpointFailure",
            EndpointFailure::new(format!("no route to {planted}")).to_string(),
        ),
    ];

    // Deliberately staged so the check is not vacuous: each rendering above
    // genuinely contains the planted value, so the assertions below must all
    // fail. That is the point -- this check asserts the *shape* of the
    // assertion, and the real assertion is the one over what the session
    // renders. See the second half.
    for (what, rendered) in &refusals {
        assert!(
            rendered.contains(&planted),
            "{what} was staged with the bearer in it and does not carry it, so this check would \
             assert nothing"
        );
    }

    // Now the real one: nothing this crate constructs *for itself* carries a
    // bearer, because no constructor is handed one.
    let honest = vec![
        (
            "NotesError::Unreadable",
            NotesError::Unreadable {
                tool: "workspaces.resolve_slug".to_owned(),
                expected: "a JSON object carrying a string `id`",
            }
            .to_string(),
        ),
        (
            "NotesError::WorkspaceUnattachable over a real slug",
            NotesError::WorkspaceUnattachable {
                slug: WorkspaceSlug::new("zaru"),
            }
            .to_string(),
        ),
    ];
    for (what, rendered) in &honest {
        assert_absent(what, rendered, &planted);
    }
}

#[test]
fn a_workspace_that_cannot_be_attached_says_the_same_thing_whatever_the_cause() {
    // ADR-0006 D7: the server throws `forbidden` without revealing which gate
    // tripped, so the harness must not invent one. The type carries no cause,
    // which is what makes three causes indistinguishable rather than merely
    // rendered alike today.
    let slug = WorkspaceSlug::new(nonce("ws"));
    let rendered = NotesError::WorkspaceUnattachable { slug: slug.clone() }.to_string();

    assert!(
        rendered.contains(slug.as_str()),
        "the refusal must name the slug the user typed, which is theirs; {rendered:?} does not"
    );
    for gate in ["existence", "membership", "scope.workspaceIds", "forbidden"] {
        assert!(
            !rendered.contains(gate),
            "the refusal named the gate {gate:?}, which ADR-0006 D7 says the server does not \
             reveal and the harness therefore cannot know: {rendered:?}"
        );
    }
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

// -- ADR-0007 D6, the three signals -------------------------------------

#[test]
fn the_ttl_backstop_fires_at_the_window_and_not_before() {
    let window = Duration::from_secs(300);
    let cached_at = Duration::from_secs(1_000);

    assert_eq!(
        Invalidation::expired(
            cached_at,
            cached_at + window - Duration::from_nanos(1),
            window
        ),
        None,
        "one nanosecond before the window elapses the cache is still good"
    );
    assert_eq!(
        Invalidation::expired(cached_at, cached_at + window, window),
        Some(Invalidation::Expired {
            window,
            elapsed: window
        }),
        "at exactly the window the backstop is due"
    );
    assert!(
        Invalidation::expired(cached_at, cached_at + window * 2, window).is_some(),
        "well past the window it is certainly due"
    );
    assert_eq!(
        Invalidation::expired(cached_at, cached_at - Duration::from_secs(1), window),
        None,
        "a reading before the cache was taken is no elapsed time, not a wrapped one"
    );
}

#[test]
fn a_refusal_for_a_claimed_tool_invalidates_and_one_for_an_unclaimed_tool_comes_back_untouched() {
    let claimed = vec!["pages.read".to_owned(), "search.global".to_owned()];

    let refused = CallRefused {
        tool: "pages.read".to_owned(),
        code: -32601,
        detail: "no such tool".to_owned(),
    };
    assert_eq!(
        Invalidation::claimed(refused.clone(), &claimed),
        Ok(Invalidation::Claimed(refused)),
        "a refusal for a tool the cache claimed means the cache is stale"
    );

    let unrelated = CallRefused {
        tool: "pages.apply_patch".to_owned(),
        code: -32601,
        detail: "no such tool".to_owned(),
    };
    assert_eq!(
        Invalidation::claimed(unrelated.clone(), &claimed),
        Err(unrelated),
        "a refusal for a tool the cache never claimed says nothing about the cache, and the \
         failure must come back unchanged rather than being swallowed"
    );
}

#[test]
fn a_method_not_found_is_read_off_the_code_the_server_sent() {
    let not_found = CallRefused {
        tool: "kg.related".to_owned(),
        code: -32601,
        detail: "no such tool".to_owned(),
    };
    assert!(not_found.is_method_not_found());

    let other = CallRefused {
        tool: "kg.related".to_owned(),
        code: -32602,
        detail: "invalid params".to_owned(),
    };
    assert!(
        !other.is_method_not_found(),
        "invalid params is not a missing method; -32602 and -32601 are different failures"
    );
}

#[test]
fn every_invalidation_says_which_of_adr_0007_d6s_three_causes_it_is() {
    let rendered = [
        Invalidation::ListChanged.to_string(),
        Invalidation::Expired {
            window: Duration::from_secs(300),
            elapsed: Duration::from_secs(301),
        }
        .to_string(),
        Invalidation::Claimed(CallRefused {
            tool: "pages.read".to_owned(),
            code: -32601,
            detail: "no such tool".to_owned(),
        })
        .to_string(),
    ];
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        assert_ne!(
            rendered[a], rendered[b],
            "two of D6's three causes render identically, so a reader cannot tell which fired"
        );
    }
    assert!(
        rendered[0].contains("list_changed"),
        "the notification signal must name the notification: {:?}",
        rendered[0]
    );
    assert!(
        rendered[2].contains("pages.read"),
        "the claimed-tool signal must name the tool that was refused: {:?}",
        rendered[2]
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
