// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The vocabulary's checks, and the first two cases of the hostile corpus.
//!
//! [Testing]'s rule for this surface: "Every escape found at a security
//! boundary — a sandbox, a permission model, a credential store, anything
//! deciding what a model-driven action may reach — joins a permanent
//! hostile-input corpus as its reproduction... **the corpus never shrinks**."
//! The cases in [`ALIAS_CORPUS`] are that corpus's first entries and are
//! added to, never removed from.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/project-management/p/process/testing

use crate::credentials::alias::{Alias, AliasRefused};
use crate::credentials::entry::{Description, Ttl};
use crate::credentials::fixtures::{app_secret_nonce, ascii_core, nonce, personal_secret_nonce};
use crate::credentials::secret::{Kind, REDACTED, Secret};
use core::time::Duration;

/// Aliases that must never reach the store, with what each one is trying.
///
/// A table rather than a run of `assert!`s, so that the count is visible and
/// a case removed is a case a reader can see was removed.
const ALIAS_CORPUS: &[(&str, &str)] = &[
    ("", "the empty name"),
    (".", "the current directory"),
    ("..", "the parent directory"),
    ("../../etc/passwd", "a relative traversal out of the store"),
    ("/etc/passwd", "an absolute path"),
    ("a/b", "a nested path"),
    ("a\\b", "a windows-style path"),
    ("..\\..\\windows", "a windows-style traversal"),
    ("work\0hidden", "a NUL, which truncates a C string"),
    (
        "work\nagent",
        "a newline, which forges a second listing row",
    ),
    (
        "work\u{1b}[2K",
        "an ANSI erase-line sequence, which hides a neighbouring row",
    ),
    (
        "notes:work",
        "a colon, which forges an ADR-0007 D5 namespace",
    ),
    (" work", "leading whitespace, invisible in a listing"),
    ("work ", "trailing whitespace, invisible in a listing"),
];

// The corpus case: a path that escapes the store's directory.
//
// The primary defence is that there is nothing to escape with — the store is
// one file and an alias is a key inside it, never a path segment. This is the
// second line, and it is what stops an alias that *reads* as a traversal from
// reaching a listing, a namespace, or a future caller who does build a path.
//
// The mutant: return `Ok(Self(offered.to_owned()))` from `Alias::new` before
// any check. Every row below reddens, and the failure names the row.
#[test]
fn an_alias_that_would_escape_the_store_is_refused_at_construction() {
    for (offered, what_it_tries) in ALIAS_CORPUS {
        let outcome = Alias::new(offered);
        assert!(
            outcome.is_err(),
            "the alias {offered:?} was taken, and it is {what_it_tries}"
        );
    }
    assert_eq!(
        ALIAS_CORPUS.len(),
        14,
        "the hostile-input corpus only grows; a case was removed rather than added"
    );
}

// The counterpart, and it is not decoration. Verification lessons §8: an
// instrument that has only ever printed one answer has not been shown able to
// print another. A refusal that refuses everything is not a boundary.
#[test]
fn an_alias_carrying_a_multibyte_grapheme_is_taken() {
    let offered = nonce("alias");
    let alias = Alias::new(&offered).unwrap_or_else(|refusal| {
        panic!(
            "the alias {offered:?} was refused, and it carries nothing the corpus names: {refusal}"
        )
    });
    assert_eq!(alias.as_str(), offered);
}

#[test]
fn each_refusal_names_the_shape_it_refused_rather_than_a_generic_reason() {
    assert_eq!(Alias::new(""), Err(AliasRefused::Empty));
    assert_eq!(Alias::new(".."), Err(AliasRefused::DotOrDotDot));
    assert!(matches!(
        Alias::new("a/b"),
        Err(AliasRefused::Separator { found: '/', .. })
    ));
    assert!(matches!(
        Alias::new("notes:work"),
        Err(AliasRefused::NamespaceSeparator { .. })
    ));
    assert!(matches!(
        Alias::new("work\u{1b}[2K"),
        Err(AliasRefused::Control { .. })
    ));
    assert!(matches!(
        Alias::new(" work"),
        Err(AliasRefused::SurroundingWhitespace { .. })
    ));
}

// The corpus case: a value that appears in an error.
//
// Two arms, deliberately. Asserting only that the value is absent would be
// satisfied by a Debug that printed nothing at all, which is a different
// defect wearing the same green. So the marker must be present *and* the
// value absent.
//
// The two arms are also two readers: the left-hand side is the nonce this
// check generated, and the right-hand side is the string the formatter
// produced. Neither travels through the other.
#[test]
fn a_secrets_debug_carries_the_redaction_and_not_the_value() {
    let value = personal_secret_nonce();
    let secret = Secret::new(value.clone()).expect("an nn_mcp_ value names a kind");

    let rendered = format!("{secret:?}");
    assert!(
        rendered.contains(REDACTED),
        "a secret's Debug printed {rendered:?}, which carries no redaction marker at all"
    );
    assert_absent(&rendered, &value, "a secret's Debug");
}

/// Asserts that a rendering published neither a value nor anything that
/// identifies it, and says which arm caught it.
///
/// Two arms, because one of them is blind. `{:?}` on a `String` escapes a
/// combining mark to `\u{301}`, so a rendering that published every byte of a
/// nonce does not `contain` that nonce — the mutation that put a bearer value
/// into a refusal through `{:?}` survived a check with only the first arm.
/// The ASCII core survives every escaping scheme and identifies the value on
/// its own.
fn assert_absent(rendered: &str, value: &str, what: &str) {
    assert!(
        !rendered.contains(value),
        "{what} published the bearer value verbatim: {rendered}"
    );
    let core = ascii_core(value);
    assert!(
        !core.is_empty(),
        "the fixture produced no ASCII core, so this check asserted nothing"
    );
    assert!(
        !rendered.contains(core),
        "{what} published the bearer value in an escaped form; its ASCII core {core:?} is in \
         {rendered}"
    );
}

#[test]
fn a_kind_is_read_off_the_value_and_never_stored_beside_it() {
    let personal = Secret::new(personal_secret_nonce()).expect("nn_mcp_ names a kind");
    let app = Secret::new(app_secret_nonce()).expect("nn_app_ names a kind");

    assert_eq!(personal.kind(), Kind::Personal);
    assert_eq!(app.kind(), Kind::App);
    assert_eq!(Kind::Personal.as_str(), "personal");
    assert_eq!(Kind::App.as_str(), "app");
}

#[test]
fn a_secret_whose_prefix_names_no_kind_is_refused() {
    for prefix in ["", "nn_", "nn_mcp", "sk-", "aegis_", "NN_MCP_"] {
        let value = format!("{prefix}{}", nonce("wrong"));
        assert!(
            Secret::new(&value).is_err(),
            "a value beginning {prefix:?} was taken, and ADR-0007 D2 admits only nn_mcp_ and nn_app_"
        );
    }
}

// A refusal is exactly the text that gets pasted into a report. This is the
// corpus case applied to the one refusal that is handed a bearer value.
#[test]
fn the_refusal_for_an_unknown_prefix_does_not_quote_the_value() {
    let value = format!("sk-{}", nonce("provider"));
    let refusal = Secret::new(&value).expect_err("sk- names no kind");

    let displayed = refusal.to_string();
    let debugged = format!("{refusal:?}");
    assert_absent(&displayed, &value, "a refusal's Display");
    assert_absent(&debugged, &value, "a refusal's Debug");
    // And it still has to be useful to the person reading it.
    assert!(
        displayed.contains("nn_mcp_") && displayed.contains("nn_app_"),
        "the refusal does not say what a bearer value must begin with: {displayed}"
    );
}

#[test]
fn a_ttl_of_zero_is_refused_and_any_other_is_taken() {
    assert!(Ttl::new(Duration::ZERO).is_err());
    let window = Duration::from_secs(300);
    assert_eq!(Ttl::new(window).expect("a non-zero ttl").get(), window);
}

#[test]
fn a_description_that_is_not_one_renderable_line_is_refused() {
    assert!(Description::new("a token for work\nand a forged row").is_err());
    assert!(Description::new("a token\u{1b}[2Kfor work").is_err());

    let text = format!("read-only, {}", nonce("description"));
    let description = Description::new(&text).expect("one line of ordinary text");
    assert_eq!(description.as_str(), text);
}
