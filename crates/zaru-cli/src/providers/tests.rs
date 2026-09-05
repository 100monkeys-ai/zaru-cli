// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks on [ADR-0012]'s vocabulary.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use crate::config::environment::variable_name;
use crate::providers::{EndpointRefused, ModelAlias, ProviderEndpoint, ProviderKind};
use std::collections::BTreeSet;

/// ADR-0012 D2's four, and the shape that makes a fifth a compile error.
///
/// The spellings are literals this check owns rather than values read back out
/// of the type, so a renamed variant reddens here instead of agreeing with
/// itself.
#[test]
fn the_alias_set_is_adr_0012_d2s_four_and_no_more() {
    assert_eq!(
        ModelAlias::ALL.len(),
        4,
        "ADR-0012 D2 names four aliases and says adding one is an ADR-level change"
    );

    // A wildcard-free match, so a fifth variant stops this file compiling
    // rather than travelling -- the same signal `Class` and `Layer` use.
    for alias in ModelAlias::ALL {
        let expected = match alias {
            ModelAlias::Default => "default",
            ModelAlias::Fast => "fast",
            ModelAlias::Reasoning => "reasoning",
            ModelAlias::Local => "local",
        };
        assert_eq!(
            alias.as_str(),
            expected,
            "ADR-0012 D2's table spells this alias {expected:?}"
        );
    }
}

/// Four names and four intents, all distinct.
///
/// Without this, `the_alias_set_is_adr_0012_d2s_four_and_no_more` passes
/// against an implementation whose arms were copied and one left unedited:
/// the length is still four and every assertion is still about the arm it
/// names. Distinctness is what separates the two.
#[test]
fn every_alias_has_its_own_name_and_its_own_intent() {
    let names: BTreeSet<&str> = ModelAlias::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(
        names.len(),
        ModelAlias::ALL.len(),
        "two aliases share a name, so a user cannot address them separately: {names:?}"
    );

    let intents: BTreeSet<&str> = ModelAlias::ALL.iter().map(|a| a.intent()).collect();
    assert_eq!(
        intents.len(),
        ModelAlias::ALL.len(),
        "two aliases share ADR-0012 D2's intent text, so a listing cannot tell them apart"
    );
}

/// ADR-0014's key for each alias, spelled out.
#[test]
fn an_aliass_configuration_key_is_model_dot_the_alias() {
    let spelled: Vec<String> = ModelAlias::ALL
        .iter()
        .map(|alias| alias.key().as_str().to_owned())
        .collect();
    assert_eq!(
        spelled,
        vec![
            "model.default".to_owned(),
            "model.fast".to_owned(),
            "model.reasoning".to_owned(),
            "model.local".to_owned(),
        ],
        "ADR-0012 owns these four keys and ADR-0014 says each record owns its own"
    );
}

/// **ADR-0012 D4's own worked environment variable is what ADR-0014's
/// transform produces, and this check is why that is a measurement rather
/// than a story.**
///
/// D4 prints `ZARU_MODEL_DEFAULT=...`. [`operations/adr-status`] records, as
/// an open question, that D4's spelling is "a third spelling again" alongside
/// ADR-0014 D3's unproducible `ZARU_MAX_ITER`. Run rather than read, the
/// transform gives exactly `ZARU_MODEL_DEFAULT` from `model.default` — so half
/// of that question is answered by the key spelling, and this check pins it so
/// that changing either record's spelling reddens rather than drifting.
///
/// The mutant: spell the table `models` instead of `model`. The transform then
/// gives `ZARU_MODELS_DEFAULT` and D4's example is no longer producible.
///
/// [`operations/adr-status`]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status
#[test]
fn adr_0012_d4s_environment_variable_is_what_adr_0014s_transform_produces() {
    let produced: Vec<String> = ModelAlias::ALL
        .iter()
        .map(|alias| variable_name(&alias.key()))
        .collect();
    assert_eq!(
        produced,
        vec![
            "ZARU_MODEL_DEFAULT".to_owned(),
            "ZARU_MODEL_FAST".to_owned(),
            "ZARU_MODEL_REASONING".to_owned(),
            "ZARU_MODEL_LOCAL".to_owned(),
        ],
        "ADR-0012 D4 prints ZARU_MODEL_DEFAULT, and ADR-0014's transform must produce it from \
         this record's own key rather than from a name somebody chose"
    );
}

/// ADR-0012 D3's four kinds, with the name a user reads.
#[test]
fn the_provider_kinds_are_adr_0012_d3s_four_and_no_more() {
    assert_eq!(
        ProviderKind::ALL.len(),
        4,
        "ADR-0012 D3 names four provider kinds"
    );

    for kind in ProviderKind::ALL {
        let expected = match kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAiCompatible => "openai-compatible",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Aegis => "aegis",
        };
        assert_eq!(
            kind.as_str(),
            expected,
            "ADR-0012 D3 spells this kind {expected:?}"
        );
    }

    let names: BTreeSet<&str> = ProviderKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        names.len(),
        ProviderKind::ALL.len(),
        "two kinds share a name: {names:?}"
    );
}

/// **Every kind's endpoint key produces a variable name a shell can set.**
///
/// ADR-0012 D3 spells one kind `openai-compatible` and ADR-0014's transform
/// upper-cases a key and replaces dots with underscores — it does not touch a
/// hyphen. A key segment carrying D3's own spelling therefore produces
/// `ZARU_PROVIDER_OPENAI-COMPATIBLE_ENDPOINT`, which is not a name a POSIX
/// shell can set at all, so the setting would be unreachable from layer 4 with
/// nothing to report it.
///
/// The mutant: have `key_segment` return `as_str`. This reddens naming the
/// kind and the variable; every other check in this file stays green, because
/// none of them looks at what a shell can express.
#[test]
fn every_kinds_endpoint_key_produces_a_settable_environment_variable_name() {
    let unsettable: Vec<(ProviderKind, String)> = ProviderKind::ALL
        .into_iter()
        .map(|kind| (kind, variable_name(&kind.endpoint_key())))
        .filter(|(_, name)| {
            !name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        })
        .collect();

    assert!(
        unsettable.is_empty(),
        "an environment variable name is letters, digits and underscores, and these cannot be set \
         by any POSIX shell, so ADR-0014 D1's layer 4 could never carry them: {unsettable:?}"
    );
}

/// The four endpoint keys, spelled out, so the segment that differs from D3's
/// name is visible rather than merely legal.
#[test]
fn an_endpoint_key_is_provider_dot_the_kind_dot_endpoint() {
    let spelled: Vec<String> = ProviderKind::ALL
        .iter()
        .map(|kind| kind.endpoint_key().as_str().to_owned())
        .collect();
    assert_eq!(
        spelled,
        vec![
            "provider.anthropic.endpoint".to_owned(),
            "provider.openai_compatible.endpoint".to_owned(),
            "provider.ollama.endpoint".to_owned(),
            "provider.aegis.endpoint".to_owned(),
        ],
        "the key segment is the kind's, with the one hyphen ADR-0014's transform cannot carry \
         written as an underscore"
    );
}

/// **ADR-0012 D5 as an absence: nothing here can tell a local provider from a
/// hosted one.**
///
/// D5 says local servers "configure exactly like hosted ones" and that "the
/// sovereignty promise is not credible if the local path is a second-class
/// code path that breaks quietly". The strongest form of that is not a check
/// on a code path but the absence of anything to branch on, so this reads the
/// two modules' own source and refuses the shapes a branch would be written
/// against.
///
/// **The matching is part of the rule**, and it was got wrong first: the
/// needles were plain words, and the first run reddened on the *prose* in
/// `endpoint.rs` that explains the absence — a correct verdict about a
/// population wider than the rule. Every needle is now spelled the way it
/// would appear in a **declaration**, and the two modules' prose is written
/// not to contain one; both halves are the mechanism, so a comment rewritten
/// to name a case in declaration form reddens here on purpose.
#[test]
fn nothing_distinguishes_a_local_provider_from_a_hosted_one() {
    let sources = [
        ("endpoint.rs", include_str!("endpoint.rs")),
        ("kind.rs", include_str!("kind.rs")),
    ];
    let forbidden = [
        "fn is_local",
        "fn is_hosted",
        "fn local(",
        "fn hosted(",
        "enum ProviderEndpoint",
        "Hosted,",
        "Hosted {",
        "Hosted(",
    ];

    let found: Vec<(&str, &str)> = sources
        .iter()
        .flat_map(|(name, body)| {
            forbidden
                .iter()
                .filter(move |needle| body.contains(**needle))
                .map(move |needle| (*name, *needle))
        })
        .collect();

    assert!(
        found.is_empty(),
        "ADR-0012 D5 has one configuration path for every kind, so there is nothing here to \
         branch on; these would be branched on: {found:?}"
    );
}

/// An endpoint a listing can render is taken, including an awkward one.
///
/// The second half is what stops a refuse-everything implementation passing
/// the refusal checks below: an implementation that took nothing would satisfy
/// every one of them perfectly.
#[test]
fn an_endpoint_that_renders_is_taken_whatever_it_looks_like() {
    for offered in [
        "http://localhost:11434",
        "https://api.anthropic.com",
        "unix:///run/zaru.sock",
        // Not a URL at all, and deliberately taken: nothing here parses a
        // scheme, because that is a dependency or an invented vocabulary.
        "こんにちは",
        "a",
    ] {
        let endpoint = ProviderEndpoint::new(offered).unwrap_or_else(|refusal| {
            panic!("{offered:?} should be taken, and was refused: {refusal}")
        });
        assert_eq!(
            endpoint.as_str(),
            offered,
            "an endpoint must survive being taken byte for byte"
        );
    }
}

/// Each shape a listing cannot render is refused, and refused for its own
/// reason.
///
/// Asserting only that something was refused would count an accidental
/// rejection for the wrong cause as a pass, so each case names the variant it
/// must produce.
#[test]
fn an_endpoint_a_listing_cannot_render_is_refused_for_its_own_reason() {
    assert_eq!(
        ProviderEndpoint::new(""),
        Err(EndpointRefused::Empty),
        "an empty endpoint configures nothing"
    );

    match ProviderEndpoint::new("http://localhost\u{7}:11434") {
        Err(EndpointRefused::Control { offered }) => assert!(
            offered.contains("\\u{7}"),
            "the refusal escapes the control character rather than carrying it into a terminal: \
             {offered:?}"
        ),
        other => panic!("a control character must be refused as Control, and was {other:?}"),
    }

    match ProviderEndpoint::new(" http://localhost:11434") {
        Err(EndpointRefused::SurroundingWhitespace { offered }) => assert_eq!(
            offered, " http://localhost:11434",
            "the refusal quotes the endpoint back so the reader can see which one it was"
        ),
        other => panic!("surrounding whitespace must be refused as such, and was {other:?}"),
    }
}
