// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks on [ADR-0012]'s vocabulary.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use crate::config::environment::variable_name;
use crate::config::{
    ConfigRefused, Contribution, Field, FieldKind, Key, Layer, Resolution, Schema, Source, Table,
    Value,
};
use crate::failure::{Class, Classified};
use crate::providers::resolution::ModelTable;
use crate::providers::{
    CapabilityRefused, EndpointRefused, Inference, InferenceRefused, ModelAlias, ModelIdRefused,
    Placement, ProviderCapabilities, ProviderEndpoint, ProviderKind, ResolvedModel, TableRefused,
    declare, endpoint_of, fields, inference_of,
};
use crate::providers::{Cost, CostRefused, Provider, RemoteModelId, TokenUsage, disagreements};
use std::collections::BTreeSet;

/// The schema every resolution check below resolves against.
///
/// Built by [`declare`] from the record's own [`fields`], never retyped here:
/// a check that restated the key set would agree with itself about a key that
/// had been renamed.
fn schema() -> Schema {
    declare(Schema::new())
}

/// One layer, setting one key to one value.
fn at(layer: Layer, key: &Key, value: &str) -> Contribution {
    let mut document = Table::new();
    document.insert_path(key, Value::Text(value.to_owned()));
    Contribution::new(layer, Source::named(layer.label()), document)
}

/// A value distinct per layer and per key, so nothing can pass by coincidence.
fn planted(layer: Layer, key: &Key) -> String {
    format!(
        "model-{}-{}",
        layer.number(),
        key.as_str().replace('.', "-")
    )
}

/// ADR-0012 D2's four, and the shape that makes a fifth a compile error.
///
/// The spellings are literals this check owns rather than values read back out
/// of the type, so a renamed variant reddens here instead of agreeing with
/// itself.
#[test]
fn the_alias_set_is_adr_0012_d2s_four_and_no_more() {
    assert_eq!(
        ModelAlias::ALL.len(),
        5,
        "ADR-0012 D2, as amended under directive 20, names the platform's five aliases and says \
         adding one is an ADR-level change"
    );

    // A wildcard-free match, so a fifth variant stops this file compiling
    // rather than travelling -- the same signal `Class` and `Layer` use.
    for alias in ModelAlias::ALL {
        let expected = match alias {
            ModelAlias::Default => "default",
            ModelAlias::Fast => "fast",
            ModelAlias::Smart => "smart",
            ModelAlias::Cheap => "cheap",
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
            "model.smart".to_owned(),
            "model.cheap".to_owned(),
            "model.local".to_owned(),
        ],
        "ADR-0012 owns these five keys and ADR-0014 says each record owns its own"
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
            "ZARU_MODEL_SMART".to_owned(),
            "ZARU_MODEL_CHEAP".to_owned(),
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
        5,
        "ADR-0012 D3, as amended under directive 20, names five provider kinds"
    );

    for kind in ProviderKind::ALL {
        let expected = match kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAiCompatible => "openai-compatible",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Gemini => "gemini",
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
            "provider.gemini.endpoint".to_owned(),
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

/// **ADR-0012 trigger clause 1, first half: all four aliases set in all five
/// layers resolve to the flag's.**
///
/// This half is deliberately *not* enough on its own, and the check below is
/// why: an implementation that reported `Flag` whatever set the key passes
/// every assertion here perfectly. Kept as two checks rather than one so that
/// the difference is a measurement rather than a claim.
#[test]
fn every_alias_set_in_all_five_layers_resolves_to_the_flags_value() {
    let schema = schema();

    for alias in ModelAlias::ALL {
        let key = alias.key();
        let contributions: Vec<Contribution> = Layer::ALL
            .into_iter()
            .map(|layer| at(layer, &key, &planted(layer, &key)))
            .collect();
        let resolution = Resolution::resolve(&schema, contributions).expect("the fixture resolves");
        let table = ModelTable::from_configuration(&resolution).expect("every value is text");

        match table.row(alias) {
            ResolvedModel::Resolved { model, supplied_by } => {
                assert_eq!(
                    model.as_str(),
                    planted(Layer::Flag, &key),
                    "ADR-0014 D1 has the highest layer win, so `{alias}` must be the flag's value"
                );
                assert_eq!(
                    *supplied_by,
                    Layer::Flag,
                    "and the table must name the layer that supplied it"
                );
            }
            ResolvedModel::Unresolved => {
                panic!("`{alias}` was set in all five layers and resolved to nothing")
            }
        }
    }
}

/// **ADR-0012 trigger clause 1, second half: every one of the five layers can
/// be the one that supplied an alias, and the table says which.**
///
/// One layer at a time, all four aliases, all five layers — twenty cases. This
/// is the half that discriminates: a table answering `Flag` regardless leaves
/// the check above green and reddens here naming every case it got wrong.
#[test]
fn every_layer_can_be_the_one_that_supplied_an_alias() {
    let schema = schema();
    let mut wrong: Vec<(ModelAlias, Layer, Layer)> = Vec::new();
    for alias in ModelAlias::ALL {
        let key = alias.key();
        for layer in Layer::ALL {
            let resolution = Resolution::resolve(&schema, [at(layer, &key, &planted(layer, &key))])
                .expect("the fixture resolves");
            let table = ModelTable::from_configuration(&resolution).expect("every value is text");
            if let ResolvedModel::Resolved { supplied_by, .. } = table.row(alias)
                && *supplied_by != layer
            {
                wrong.push((alias, layer, *supplied_by));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "ADR-0012 D4 requires the layer that supplied each alias, and these are named wrong \
         (alias, layer that set it, layer reported): {wrong:?}"
    );
}

/// **The layer this table names is the layer `config explain` marks, read out
/// of the rendered block rather than asked for a second time.**
///
/// One arm of the comparison must not travel through the thing being checked.
/// The table's side comes from [`ModelTable`]; the other side is parsed from
/// ADR-0014 D3's own rendered text — the row carrying the effective marker —
/// which shares no code with `effective_layer`'s search. A table that invented
/// its own precedence would agree with itself and disagree here.
#[test]
fn the_layer_the_table_names_is_the_layer_adr_0014_d3s_block_marks() {
    let schema = schema();
    let key = ModelAlias::Default.key();

    for highest in [Layer::User, Layer::Project, Layer::Environment, Layer::Flag] {
        let contributions: Vec<Contribution> = Layer::ALL
            .into_iter()
            .filter(|layer| *layer <= highest)
            .map(|layer| at(layer, &key, &planted(layer, &key)))
            .collect();
        let resolution = Resolution::resolve(&schema, contributions).expect("the fixture resolves");

        let rendered = resolution.explain(&key).to_string();
        let marked = rendered
            .lines()
            .find(|line| line.contains("← effective"))
            .unwrap_or_else(|| panic!("D3's block must mark a row:\n{rendered}"));
        let number: u8 = marked
            .split_whitespace()
            .next()
            .and_then(|first| first.parse().ok())
            .unwrap_or_else(|| {
                panic!("the marked row must start with D1's layer number: {marked}")
            });

        let table = ModelTable::from_configuration(&resolution).expect("every value is text");
        let ResolvedModel::Resolved { supplied_by, .. } = table.row(ModelAlias::Default) else {
            panic!("`default` was set and resolved to nothing")
        };

        assert_eq!(
            supplied_by.number(),
            number,
            "the table says layer {} supplied `model.default`; the block it is a projection of \
             marks layer {number}:\n{rendered}",
            supplied_by.number(),
        );
    }
}

/// An alias nobody configured resolves to nothing, and nothing here invents a
/// model to put there.
///
/// ADR-0012's Neutral consequence is one sentence — "Nothing here selects a
/// default model" — so the second half reads this module's own sources for
/// anything shaped like a model name. A default added as a literal reddens
/// here before it can acquire a caller; the needles are the vendor prefixes a
/// default would have to be spelled with.
#[test]
fn an_alias_no_layer_set_is_unresolved_and_no_default_model_is_invented() {
    let resolution = Resolution::resolve(&schema(), []).expect("an empty configuration resolves");
    let table = ModelTable::from_configuration(&resolution).expect("nothing to read");

    for alias in ModelAlias::ALL {
        assert_eq!(
            *table.row(alias),
            ResolvedModel::Unresolved,
            "no layer set `{alias}`, and ADR-0012 selects no default model"
        );
    }

    let sources = [
        ("alias.rs", include_str!("alias.rs")),
        ("kind.rs", include_str!("kind.rs")),
        ("resolution.rs", include_str!("resolution.rs")),
    ];
    let found: Vec<(&str, &str)> = sources
        .iter()
        .flat_map(|(name, body)| {
            ["\"claude-", "\"gpt-", "\"llama", "\"o1-", "\"gemini-"]
                .iter()
                .filter(move |needle| body.contains(**needle))
                .map(move |needle| (*name, *needle))
        })
        .collect();
    assert!(
        found.is_empty(),
        "ADR-0012's Neutral section selects no default model, and these look like one: {found:?}"
    );
}

/// **A project may choose a model and may not choose where the prompts go.**
///
/// Both arms, because the refusal alone is satisfied by a schema that refuses
/// the project layer everything — which would be wrong, and wrong in a way
/// every refusal check passes perfectly.
#[test]
fn a_project_may_set_a_model_alias_and_may_not_set_an_endpoint() {
    let schema = schema();
    let alias_key = ModelAlias::Default.key();

    let resolution = Resolution::resolve(
        &schema,
        [at(
            Layer::Project,
            &alias_key,
            "a-model-a-project-asked-for",
        )],
    )
    .expect("ADR-0012 D4 lists project configuration among the five layers that resolve an alias");
    let table = ModelTable::from_configuration(&resolution).expect("the value is text");
    match table.row(ModelAlias::Default) {
        ResolvedModel::Resolved { model, supplied_by } => {
            assert_eq!(
                model.as_str(),
                "a-model-a-project-asked-for",
                "a project asking for a different model is ADR-0012 D4 working"
            );
            assert_eq!(
                *supplied_by,
                Layer::Project,
                "and the layer that supplied it is the project's"
            );
        }
        ResolvedModel::Unresolved => {
            panic!("the project layer set `model.default` and it resolved to nothing")
        }
    }

    // And every one of the four kinds, separately: a ceiling that refused only
    // the first would pass a check that tried only the first.
    let mut permitted: Vec<ProviderKind> = Vec::new();
    for kind in ProviderKind::ALL {
        let key = kind.endpoint_key();
        match Resolution::resolve(&schema, [at(Layer::Project, &key, "http://elsewhere")]) {
            Err(ConfigRefused::ProjectMayNotSet { key: named, reason }) => {
                assert_eq!(named, key, "the refusal names the key the project set");
                assert!(
                    reason.contains("prompts"),
                    "ADR-0014 D6 requires the reason as well as the key, and this one does not say \
                     what is at stake: {reason:?}"
                );
            }
            _ => permitted.push(kind),
        }
    }
    assert!(
        permitted.is_empty(),
        "a repository the user cloned must not be able to redirect where their prompts go, and \
         these kinds let it: {permitted:?}"
    );
}

/// **ADR-0012 D5, as one declaration shared by all four kinds.**
///
/// D5 says local servers "configure exactly like hosted ones". The strongest
/// reading is that the *declaration* is the same object for every kind, not
/// merely that four similar ones exist, so this compares them against each
/// other rather than against a literal.
#[test]
fn every_provider_kind_is_configured_by_the_very_same_declaration() {
    let declared = fields();
    let endpoints: Vec<&Field> = ProviderKind::ALL
        .iter()
        .map(|kind| {
            let key = kind.endpoint_key();
            &declared
                .iter()
                .find(|(candidate, _)| *candidate == key)
                .unwrap_or_else(|| panic!("`{key}` must be declared"))
                .1
        })
        .collect();

    let differing: Vec<ProviderKind> = ProviderKind::ALL
        .into_iter()
        .zip(endpoints.iter())
        .filter(|(_, field)| **field != endpoints[0])
        .map(|(kind, _)| kind)
        .collect();
    assert!(
        differing.is_empty(),
        "ADR-0012 D5 has every kind configured the same way, and these are declared differently \
         from `anthropic`: {differing:?}"
    );

    // And the same path reads an endpoint back for every kind.
    let schema = schema();
    for kind in ProviderKind::ALL {
        let key = kind.endpoint_key();
        let resolution = Resolution::resolve(&schema, [at(Layer::User, &key, "http://somewhere")])
            .expect("the user's own layer may set an endpoint for any kind");
        let endpoint = endpoint_of(&resolution, kind)
            .expect("the value is text")
            .unwrap_or_else(|| panic!("`{key}` was set and read back as nothing"));
        assert_eq!(endpoint.as_str(), "http://somewhere");
    }
}

/// A configured value a listing could not render is refused, naming the alias.
#[test]
fn an_unusable_model_identifier_is_refused_naming_the_alias() {
    let schema = schema();
    let key = ModelAlias::Fast.key();

    for (offered, expected) in [
        ("", "is empty"),
        ("a\u{7}b", "control character"),
        (" spaced", "whitespace"),
    ] {
        let resolution =
            Resolution::resolve(&schema, [at(Layer::User, &key, offered)]).expect("text resolves");
        match ModelTable::from_configuration(&resolution) {
            Err(TableRefused::UnusableModelId { alias, refusal }) => {
                assert_eq!(alias, ModelAlias::Fast, "the refusal names the alias");
                let said = refusal.to_string();
                assert!(
                    said.contains(expected),
                    "the refusal must say why; it said {said:?} and should mention {expected:?}"
                );
            }
            other => panic!("{offered:?} should be refused, and was {other:?}"),
        }
    }
}

/// **A model identifier cannot be built anywhere but the resolution table.**
///
/// ADR-0012 D1: "A model identifier appearing anywhere except the resolution
/// table is a bug." That is held by Rust's own module privacy — `ModelId`'s
/// field and its constructor are private to `providers::resolution`, and this
/// module is a *sibling*, so `ModelId::new("anything")` written here does not
/// compile. The red-watch for it is therefore a compile error rather than an
/// assertion, quoted in this commit's message.
///
/// What runs is the other half: the source must not grow a public door. The
/// needles are declaration-shaped, so the prose above does not match.
#[test]
fn nothing_outside_the_resolution_table_can_build_a_model_identifier() {
    let body = include_str!("resolution.rs");
    let doors = [
        "pub fn new(",
        "pub const fn new(",
        "impl From<String> for ModelId",
        "impl From<&str> for ModelId",
        "pub struct ModelId(pub",
    ];
    let found: Vec<&str> = doors
        .iter()
        .filter(|needle| body.contains(**needle))
        .copied()
        .collect();
    assert!(
        found.is_empty(),
        "ADR-0012 D1 puts a model identifier in one place, and these would let it be built \
         anywhere: {found:?}"
    );
}

/// **ADR-0012 trigger clause 3, configuration-time half: a provider that
/// cannot call tools is refused before anything runs, and the refusal says
/// what to change.**
///
/// Both arms. A descriptor that *can* call tools is accepted, which is what
/// separates this from a check that refuses everything, and the refusal names
/// the alias, the kind and a remedy the reader can act on.
#[test]
fn a_provider_that_cannot_call_tools_is_refused_at_configuration_time() {
    let able = ProviderCapabilities::declared(true, true, true, Some(1_024));
    for alias in ModelAlias::ALL {
        for kind in ProviderKind::ALL {
            assert_eq!(
                able.require_tool_calling(alias, kind),
                Ok(()),
                "a provider that declares it calls tools must be taken"
            );
        }
    }

    let unable = ProviderCapabilities::declared(true, false, true, Some(1_024));
    for alias in ModelAlias::ALL {
        for kind in ProviderKind::ALL {
            assert_eq!(
                unable.require_tool_calling(alias, kind),
                Err(CapabilityRefused::ToolCallingUnavailable { alias, kind }),
                "ADR-0012 D3 has a provider that cannot call tools say so at configuration time"
            );
        }
    }
}

/// The refusal reaches the reader as ADR-0016's user-correctable class, with a
/// remedy naming the alias, the provider and what to change.
///
/// Not the capability class: ADR-0016 D1's capability row is "the tier does not
/// offer this. Says which tier does" and carries a `Tier`, and no tier is what
/// is wrong here — every tier can reach a provider that calls tools.
#[test]
fn the_tool_calling_refusal_is_user_correctable_and_names_the_alias_the_kind_and_a_remedy() {
    let refusal = CapabilityRefused::ToolCallingUnavailable {
        alias: ModelAlias::Smart,
        kind: ProviderKind::Ollama,
    };
    let classified = Classified::from(refusal);

    assert_eq!(
        classified.class(),
        Class::UserCorrectable,
        "the user can act: they can point the alias somewhere else"
    );

    let said = classified
        .statement()
        .expect("a user-correctable failure carries a statement")
        .to_string();
    for needed in ["smart", "ollama", "mid-loop"] {
        assert!(
            said.contains(needed),
            "the statement must say {needed:?} so the reader knows what happened; it said {said:?}"
        );
    }

    let remedy = classified
        .remedy()
        .expect("a user-correctable failure carries a remedy");
    let lead = remedy
        .actions()
        .next()
        .expect("a remedy always has a first action")
        .lead()
        .to_string();
    for needed in ["model.smart", "ollama"] {
        assert!(
            lead.contains(needed),
            "ADR-0016 D2 says exactly what to change, and this remedy does not name {needed:?}: \
             {lead:?}"
        );
    }
}

/// Every refusal this module can raise reaches the reader classified, and the
/// two that are ours are reported as defects rather than as the user's fault.
///
/// ADR-0016 D3: "Never present a defect as a user error." `NotText` fires only
/// when a caller declared this record's own key as something other than text,
/// and `NoSupplyingLayer` cannot be reached through ADR-0014 D3's explanation
/// at all; both are this harness's.
#[test]
fn the_resolution_tables_own_refusals_are_classified_and_ours_are_defects() {
    let ours = [
        TableRefused::NotText {
            alias: ModelAlias::Default,
            found: "a whole number",
        },
        TableRefused::NoSupplyingLayer {
            key: ModelAlias::Local.key(),
        },
    ];
    for refusal in ours {
        assert_eq!(
            Classified::from(refusal.clone()).class(),
            Class::Defect,
            "{refusal:?} is this harness's and must not be presented as the user's"
        );
    }

    let theirs = [
        TableRefused::UnusableModelId {
            alias: ModelAlias::Fast,
            refusal: ModelIdRefused::Empty,
        },
        TableRefused::UnusableEndpoint {
            kind: ProviderKind::Aegis,
            refusal: EndpointRefused::Empty,
        },
    ];
    for refusal in theirs {
        let classified = Classified::from(refusal.clone());
        assert_eq!(
            classified.class(),
            Class::UserCorrectable,
            "{refusal:?} is a value the user configured"
        );
        let lead = classified
            .remedy()
            .expect("a user-correctable failure carries a remedy")
            .actions()
            .next()
            .expect("a remedy always has a first action")
            .lead()
            .to_string();
        assert!(
            lead.contains("model.fast") || lead.contains("provider.aegis.endpoint"),
            "the remedy must name the key the reader has to edit: {lead:?}"
        );
    }
}

/// **The measurement that decided where the inference key lives.**
///
/// Directive 20 spelled it `model.<alias>.inference`. `model.<alias>` holds the
/// model identifier, so a document carrying both needs one key to be text and a
/// table at once. Both orders are staged here because they fail differently and
/// the silent one is the dangerous one: written second, the nested key is
/// refused loudly; written *first*, the later write replaces the table
/// wholesale and the setting is gone with nothing reported — which is
/// ADR-0014 D5's worst outcome arriving without even a typo.
///
/// The sibling spelling `inference.<alias>` keeps every property the directive
/// wanted, and this check is what stops the nested one being re-proposed.
#[test]
fn an_inference_key_and_a_model_key_cannot_be_nested_inside_one_another() {
    let nested = Key::new("model.default.inference").expect("well formed");
    let model = ModelAlias::Default.key();
    let hypothetical = Schema::new()
        .with(model.clone(), Field::free(FieldKind::Text))
        .with(nested.clone(), Field::free(FieldKind::Text));

    let mut model_last = Table::new();
    model_last.insert_path(&nested, Value::Text("local".to_owned()));
    model_last.insert_path(&model, Value::Text("a-model".to_owned()));
    let resolution = Resolution::resolve(
        &hypothetical,
        [Contribution::new(
            Layer::User,
            Source::named("a document carrying both"),
            model_last,
        )],
    )
    .expect("writing the model key last leaves a document that resolves");
    assert_eq!(
        resolution.get(&nested),
        None,
        "the nested key must be gone without a word — that is why the spelling is a sibling"
    );

    let mut nested_last = Table::new();
    nested_last.insert_path(&model, Value::Text("a-model".to_owned()));
    nested_last.insert_path(&nested, Value::Text("local".to_owned()));
    match Resolution::resolve(
        &hypothetical,
        [Contribution::new(
            Layer::User,
            Source::named("a document carrying both"),
            nested_last,
        )],
    ) {
        Err(ConfigRefused::WrongShape { key, expected, .. }) => {
            assert_eq!(key, model, "the refusal names the key that cannot be both");
            assert_eq!(expected, "text");
        }
        other => panic!("the other order must be refused outright, and was {other:?}"),
    }

    // And the spelling actually used is a sibling, so both resolve together.
    let schema = schema();
    let axis = crate::providers::Inference::key(ModelAlias::Default);
    let mut both = Table::new();
    both.insert_path(&model, Value::Text("a-model".to_owned()));
    both.insert_path(&axis, Value::Text("local".to_owned()));
    let resolution = Resolution::resolve(
        &schema,
        [Contribution::new(Layer::User, Source::named("both"), both)],
    )
    .expect("a model and its axis are two keys and resolve together");
    assert!(
        resolution.get(&model).is_some() && resolution.get(&axis).is_some(),
        "both must survive, which is the whole reason the key is spelled this way"
    );
}

/// Every alias's inference axis resolves through the layers, and where no layer
/// sets it the provider kind decides.
///
/// Both arms: the configured value must win, and the default must be the kind's
/// rather than a constant. A defaults-only implementation passes the second arm
/// and fails the first; one that ignored the kind passes the first and fails
/// the second.
#[test]
fn an_aliass_inference_axis_is_configured_or_defaults_to_the_provider_kinds() {
    let schema = schema();

    // Configured wins, for every alias and against every kind's default.
    for alias in ModelAlias::ALL {
        let key = crate::providers::Inference::key(alias);
        for axis in Inference::ALL {
            let resolution =
                Resolution::resolve(&schema, [at(Layer::Project, &key, axis.as_str())])
                    .expect("an axis is text");
            for kind in ProviderKind::ALL {
                assert_eq!(
                    inference_of(&resolution, alias, kind).expect("the value names an axis"),
                    axis,
                    "a configured `{key}` must win over the default `{kind}` implies"
                );
            }
        }
    }

    // Unset falls back to the kind, and ollama is the only local one.
    let empty = Resolution::resolve(&schema, []).expect("an empty configuration resolves");
    let mut wrong: Vec<(ProviderKind, Inference)> = Vec::new();
    for kind in ProviderKind::ALL {
        let expected = match kind {
            ProviderKind::Ollama => Inference::Local,
            ProviderKind::Anthropic
            | ProviderKind::OpenAiCompatible
            | ProviderKind::Gemini
            | ProviderKind::Aegis => Inference::Frontier,
        };
        let got = inference_of(&empty, ModelAlias::Default, kind).expect("nothing is set");
        if got != expected {
            wrong.push((kind, got));
        }
    }
    assert!(
        wrong.is_empty(),
        "directive 20 makes ollama local and every other kind frontier, and these disagree: \
         {wrong:?}"
    );
}

/// Work is placed locally unless the `aegis` kind is what resolved.
///
/// ADR-0001 D3's `linked` row splits each cell into local and offloaded, and
/// this is the predicate that decides which. It is not configurable: ADR-0012
/// D6 has the harness negotiate before offloading, so a key declaring "this is
/// offloaded" would answer a question that record settles by asking.
#[test]
fn work_is_placed_locally_unless_the_aegis_kind_resolved() {
    let offloading: Vec<ProviderKind> = ProviderKind::ALL
        .into_iter()
        .filter(|kind| Placement::of(*kind) == Placement::Offloaded)
        .collect();
    assert_eq!(
        offloading,
        vec![ProviderKind::Aegis],
        "only `aegis` hands work to something else; every other kind is this machine calling an \
         API, which is a network request rather than an offload"
    );

    let sources = [("inference.rs", include_str!("inference.rs"))];
    let found: Vec<&str> = sources
        .iter()
        .flat_map(|(_, body)| {
            ["Url::", "parse_url", ".host(", "starts_with(\"http"]
                .iter()
                .filter(move |needle| body.contains(**needle))
                .copied()
        })
        .collect();
    assert!(
        found.is_empty(),
        "neither axis is guessed from an endpoint — a localhost address is not a promise that \
         inference is local — and these would guess: {found:?}"
    );
}

/// A value naming neither axis is refused, saying what the two are.
#[test]
fn an_inference_axis_naming_neither_column_is_refused() {
    let schema = schema();
    let key = crate::providers::Inference::key(ModelAlias::Cheap);
    let resolution =
        Resolution::resolve(&schema, [at(Layer::User, &key, "cloudy")]).expect("text resolves");

    match inference_of(&resolution, ModelAlias::Cheap, ProviderKind::Ollama) {
        Err(TableRefused::UnusableInference { alias, refusal }) => {
            assert_eq!(alias, ModelAlias::Cheap, "the refusal names the alias");
            let said = refusal.to_string();
            for needed in ["local", "frontier", "inference.cheap"] {
                assert!(
                    said.contains(needed),
                    "the refusal must name {needed:?} so the reader can act; it said {said:?}"
                );
            }
        }
        other => panic!("a value naming neither axis must be refused, and was {other:?}"),
    }

    let classified = Classified::from(InferenceRefused::NoSuchAxis {
        key: key.clone(),
        offered: "cloudy".to_owned(),
    });
    assert_eq!(
        classified.class(),
        Class::UserCorrectable,
        "the user wrote it and the user can fix it"
    );
}

/// A provider a check implements, because nothing in the product tree does.
///
/// The one place in this file that stands in for what an adapter would be, and
/// it is what the accounting invariant below is checked over.
struct StagedProvider {
    kind: ProviderKind,
    endpoint: ProviderEndpoint,
    capabilities: ProviderCapabilities,
    usage: Option<TokenUsage>,
}

impl Provider for StagedProvider {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        &self.endpoint
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    fn usage(&self) -> Option<TokenUsage> {
        self.usage.clone()
    }
}

/// ADR-0012 D7's three quantities, with a cost that is reported and never
/// computed.
///
/// The second half is the one that matters: the module must contain no rate, no
/// multiplication and no currency, because D7 makes cost conditional on "the
/// provider publishing pricing" and no provider exists to publish any. A number
/// invented here would reach a user's status line looking like one somebody
/// chose.
#[test]
fn usage_carries_what_a_provider_reported_and_computes_no_cost() {
    let counted = TokenUsage::counted(1_200, 340);
    assert_eq!(counted.prompt_tokens(), 1_200);
    assert_eq!(counted.completion_tokens(), 340);
    assert_eq!(
        counted.cost(),
        None,
        "D7 makes cost conditional, so the ordinary case reports none"
    );

    let cost = Cost::reported(4_500, "USD micros").expect("a unit the caller named");
    let priced = TokenUsage::counted(1_200, 340).priced(cost.clone());
    assert_eq!(priced.cost(), Some(&cost));
    assert_eq!(
        priced.cost().map(Cost::amount),
        Some(4_500),
        "the amount is the caller's and nothing here scales it"
    );
    assert_eq!(
        priced.cost().map(Cost::unit),
        Some("USD micros"),
        "the unit travels with the amount, because a bare number is a currency the reader guesses"
    );

    // Case-folded and declaration-shaped, and both halves were paid for. The
    // first version used lower-case needles and a mutation adding `PER_1K_USD`
    // walked straight past it; the second matched the bare word "rate" and
    // reddened on this module's own prose explaining that no rate lives there.
    // What an instrument can see is part of the rule it enforces, so the
    // needles are spellings prose does not produce.
    let body = include_str!("usage.rs").to_lowercase();
    let arithmetic = [
        "per_1k",
        "per_1m",
        "per_token",
        "_rate",
        "rate_",
        "fn rate",
        "usd",
        "eur",
        "gbp",
        "dollar",
        "fn total",
        "0.000",
    ];
    let found: Vec<&str> = arithmetic
        .iter()
        .filter(|needle| body.contains(**needle))
        .copied()
        .collect();
    assert!(
        found.is_empty(),
        "ADR-0012 D7 prices where the provider publishes pricing, and nothing publishes any; these \
         would invent a number: {found:?}"
    );
}

/// A cost a status line could not render is refused, saying why.
#[test]
fn a_cost_with_no_usable_unit_is_refused() {
    assert_eq!(
        Cost::reported(1, "  "),
        Err(CostRefused::UnitMissing),
        "a bare number is one the reader has to guess the currency of"
    );
    match Cost::reported(1, "US\u{7}D") {
        Err(CostRefused::UnitControl { offered }) => assert!(
            offered.contains("\\u{7}"),
            "the refusal escapes the control character: {offered:?}"
        ),
        other => panic!("a control character in a unit must be refused, and was {other:?}"),
    }
    assert!(
        Cost::reported(0, "credits").is_ok(),
        "a zero cost in a unit the caller named is a real answer, not a refusal"
    );
}

/// **The descriptor's accounting flag and the port's usage are two halves of
/// one statement, and an implementation owes both.**
///
/// The trait cannot enforce it — there is nothing in the product tree to wrap —
/// so the obligation is stated on the port and held here over the
/// implementations that exist, which are this check's own. A provider that says
/// it does not account must report nothing, and one that says it does must
/// report something.
#[test]
fn a_providers_accounting_flag_and_its_usage_agree() {
    let accounting = StagedProvider {
        kind: ProviderKind::Anthropic,
        endpoint: ProviderEndpoint::new("https://api.example").expect("well formed"),
        capabilities: ProviderCapabilities::declared(true, true, true, Some(1_024)),
        usage: Some(TokenUsage::counted(10, 20)),
    };
    let silent = StagedProvider {
        kind: ProviderKind::Ollama,
        endpoint: ProviderEndpoint::new("http://localhost:11434").expect("well formed"),
        capabilities: ProviderCapabilities::declared(false, true, false, Some(1_024)),
        usage: None,
    };

    for provider in [&accounting, &silent] {
        assert_eq!(
            provider.capabilities().token_accounting(),
            provider.usage().is_some(),
            "`{}` says it accounts {} and reports usage {}",
            provider.kind(),
            provider.capabilities().token_accounting(),
            provider.usage().is_some(),
        );
    }

    // And the port carries what a provider is configured to be, rather than a
    // second way to ask a model something: a request method here would be two
    // declarations of one exchange.
    let body = include_str!("port.rs");
    let requests = ["fn generate", "fn complete", "fn send", "fn stream("];
    let found: Vec<&str> = requests
        .iter()
        .filter(|needle| body.contains(**needle))
        .copied()
        .collect();
    assert!(
        found.is_empty(),
        "a prompt-in, response-out shape is the tool-call loop's port in `zaru-core`, and a second \
         one here would be the rule in two places: {found:?}"
    );
}

/// **ADR-0012 D6: a disagreement names both sides, and nothing can turn one
/// into the other.**
///
/// The structural half is a compile error rather than an assertion — the two
/// sides are different types, `ModelId` can be built only inside the resolution
/// table, and there is no conversion in either direction — so what runs is the
/// behaviour: the two sides are compared, only genuine differences are
/// reported, and both are carried into the value a prompt would show.
#[test]
fn a_disagreement_names_both_sides_and_nothing_reconciles_it() {
    let schema = schema();
    let mut document = Table::new();
    for alias in ModelAlias::ALL {
        document.insert_path(&alias.key(), Value::Text(format!("ours-{alias}")));
    }
    let resolution = Resolution::resolve(
        &schema,
        [Contribution::new(
            Layer::User,
            Source::named("this harness"),
            document,
        )],
    )
    .expect("the fixture resolves");
    let table = ModelTable::from_configuration(&resolution).expect("every value is text");

    // The orchestrator agrees about two and disagrees about two; the fifth it
    // does not carry at all, which D6 as written does not describe.
    let remote = vec![
        (
            ModelAlias::Default,
            RemoteModelId::reported("ours-default").expect("well formed"),
        ),
        (
            ModelAlias::Fast,
            RemoteModelId::reported("theirs-fast").expect("well formed"),
        ),
        (
            ModelAlias::Smart,
            RemoteModelId::reported("ours-smart").expect("well formed"),
        ),
        (
            ModelAlias::Cheap,
            RemoteModelId::reported("theirs-cheap").expect("well formed"),
        ),
    ];

    let found = disagreements(&table, &remote);
    assert_eq!(
        found.iter().map(|d| d.alias).collect::<Vec<_>>(),
        vec![ModelAlias::Fast, ModelAlias::Cheap],
        "only the aliases the two sides resolve differently are disagreements; agreement is not \
         one, and an alias the orchestrator does not carry is not one either"
    );

    for disagreement in &found {
        assert_eq!(
            disagreement.local.as_str(),
            format!("ours-{}", disagreement.alias),
            "the local side must be what this harness resolved, unaltered"
        );
        assert_eq!(
            disagreement.remote.as_str(),
            format!("theirs-{}", disagreement.alias),
            "and the remote side must be what the orchestrator said, unaltered"
        );
        let shown = disagreement.to_string();
        assert!(
            shown.contains(disagreement.local.as_str())
                && shown.contains(disagreement.remote.as_str()),
            "D6 says a disagreement is shown naming both sides, and this one shows {shown:?}"
        );
    }

    // Nothing in the module reconciles: no conversion between the two types,
    // and no preference for either side. The needles are declaration-shaped.
    let body = include_str!("negotiation.rs");
    let reconcilers = [
        "impl From<RemoteModelId> for ModelId",
        "impl From<ModelId> for RemoteModelId",
        "fn prefer",
        "fn reconcile",
        "fn resolve_conflict",
    ];
    let found: Vec<&str> = reconcilers
        .iter()
        .filter(|needle| body.contains(**needle))
        .copied()
        .collect();
    assert!(
        found.is_empty(),
        "ADR-0012 D6: a disagreement is never silently reconciled, and these would reconcile one: \
         {found:?}"
    );
}

// --- The selection rule -----------------------------------------------------
//
// A PROPOSED reading of 2026-09-14, written on ADR-0012's amendments page
// before the code. These checks pin it so that deciding it the other way
// reddens rather than passing unnoticed, which is the shape this workspace
// already uses for the thinking-token question.

use crate::providers::selection::{
    KEYLESS_ENDING, KeyUse, NoKindSelected, Requirement, kind_key, select,
};

/// The two kinds this build carries a client for, in declaration order.
const WITH_A_CLIENT: [ProviderKind; 2] = [ProviderKind::Gemini, ProviderKind::Ollama];

#[test]
fn an_explicit_kind_decides_it_over_every_requirement() {
    // Part 1 of the rule. The user saying "use this one" is exactly the case
    // where a requirement was a guess standing in for an answer, so it wins
    // even though nothing is held and nothing is configured.
    let chosen = select(
        ModelAlias::Default,
        Some(ProviderKind::Ollama),
        &WITH_A_CLIENT,
        |_| false,
        |_| false,
    )
    .expect("an explicit kind is an answer on its own");
    assert_eq!(
        chosen,
        ProviderKind::Ollama,
        "an explicit `provider.<alias>.kind` did not decide the kind, so the key a user sets is \
         overridden by whatever they happen to hold"
    );
}

#[test]
fn a_kind_this_build_cannot_reach_is_not_chosen_even_when_named() {
    // Naming `anthropic` is naming a real kind of D3's five, and there is
    // still nothing to build. It falls through to the requirement rule rather
    // than being honoured into a panic.
    let chosen = select(
        ModelAlias::Default,
        Some(ProviderKind::Anthropic),
        &WITH_A_CLIENT,
        |kind| kind == ProviderKind::Gemini,
        |_| false,
    )
    .expect("the requirement rule still answers");
    assert_eq!(chosen, ProviderKind::Gemini);
}

#[test]
fn a_machine_with_only_a_gemini_key_resolves_exactly_as_it_did_before() {
    // Part 2 makes the OLD behaviour the special case of a general rule rather
    // than replacing it. This is the case that existed before 2026-09-14, and
    // it must not have moved.
    let chosen = select(
        ModelAlias::Default,
        None,
        &WITH_A_CLIENT,
        |kind| kind == ProviderKind::Gemini,
        |_| false,
    )
    .expect("a held key is a requirement met");
    assert_eq!(
        chosen,
        ProviderKind::Gemini,
        "the rule that replaced credential presence changed what a machine holding one key does"
    );
}

#[test]
fn a_keyless_kind_is_reachable_by_a_configured_endpoint_alone() {
    // The whole reason the rule has two parts. Before this, a kind with a
    // client and no credential could not be selected at all, because the
    // selector asked the credential store and `ollama` is never in it.
    let chosen = select(
        ModelAlias::Default,
        None,
        &WITH_A_CLIENT,
        |_| false,
        |kind| kind == ProviderKind::Ollama,
    )
    .expect("a configured endpoint is a keyless kind's requirement met");
    assert_eq!(
        chosen,
        ProviderKind::Ollama,
        "a keyless kind with its endpoint configured was not selected, which makes it unreachable \
         by construction however the alias-to-kind key is spelled"
    );
}

#[test]
fn declaration_order_decides_when_both_requirements_hold() {
    let chosen = select(
        ModelAlias::Default,
        None,
        &WITH_A_CLIENT,
        |_| true,
        |_| true,
    )
    .expect("both requirements hold");
    assert_eq!(
        chosen,
        ProviderKind::Gemini,
        "the tie was not broken by KINDS_WITH_A_CLIENT's declaration order, so which provider \
         answers depends on something a reader cannot see"
    );
}

#[test]
fn a_machine_with_nothing_configured_is_still_refused() {
    // The rule must not make a keyless kind the answer on every machine. That
    // is what "an endpoint set at any layer OTHER THAN the built-in default"
    // buys: a default every machine carries would mean nobody ever chose.
    let refusal = select(
        ModelAlias::Default,
        None,
        &WITH_A_CLIENT,
        |_| false,
        |_| false,
    )
    .expect_err("nothing is held and nothing is configured");
    assert_eq!(refusal.alias, ModelAlias::Default);

    let said = refusal.to_string();
    for kind in WITH_A_CLIENT {
        assert!(
            said.contains(kind.as_str()),
            "the refusal does not name `{kind}`, so a reader is not told one of the providers \
             they could reach: {said}"
        );
    }
    assert!(
        said.contains("providers keys add")
            && said.contains(kind_key(ModelAlias::Default).as_str()),
        "the refusal names only one of the two routes out, and a reader whose provider needs no \
         key would go looking for a credential that does not exist: {said}"
    );
}

#[test]
fn the_kinds_selected_by_an_endpoint_rather_than_a_key_are_the_two_that_can_work_without_one() {
    // `KeyUse::of` is a wildcard-free match, so a sixth kind fails to compile
    // there. This asserts the mapping itself.
    //
    // **It said `vec![Ollama]` and named one kind until 2026-09-14**, and the
    // sentence under it was "exactly one of D3's five is reached with no
    // secret, and it is the one with no secret". That stopped being true when
    // `openai-compatible` gained a client: a local server of that kind is
    // reached with no secret too. The check is widened rather than deleted,
    // because what it guards is unchanged -- a kind wrongly in this set is
    // selectable on a machine that never configured it.
    let by_endpoint: Vec<ProviderKind> = ProviderKind::ALL
        .into_iter()
        .filter(|kind| Requirement::of(*kind) == Requirement::ConfiguredEndpoint)
        .collect();
    assert_eq!(
        by_endpoint,
        vec![ProviderKind::OpenAiCompatible, ProviderKind::Ollama],
        "the set of kinds selected by a configured endpoint is {by_endpoint:?}; a kind wrongly in \
         it is selectable on a machine that never configured it, and a kind wrongly out of it \
         cannot be selected at all"
    );
}

#[test]
fn what_selects_a_kind_and_whether_it_sends_a_key_are_two_questions_with_one_answer_each() {
    // The distinction that arrived on 2026-09-14. `Requirement` is derived from
    // `KeyUse`, so the interesting content is that the derivation is not the
    // identity: there is exactly one kind where the two answers differ, and a
    // future edit that collapsed them again would redden here.
    let differ: Vec<ProviderKind> = ProviderKind::ALL
        .into_iter()
        .filter(|kind| {
            let selected_by_key = Requirement::of(*kind) == Requirement::HeldKey;
            let sends_a_key = KeyUse::of(*kind) != KeyUse::Never;
            selected_by_key != sends_a_key
        })
        .collect();
    assert_eq!(
        differ,
        vec![ProviderKind::OpenAiCompatible],
        "exactly one kind is selected by its endpoint and still sends a key when one is held; \
         collapsing the two questions back into one would either stop sending a gateway's key or \
         make a local server unreachable without one",
    );

    // And the three spellings, so a kind moved between them reddens.
    assert_eq!(KeyUse::of(ProviderKind::Gemini), KeyUse::Required);
    assert_eq!(KeyUse::of(ProviderKind::Anthropic), KeyUse::Required);
    assert_eq!(KeyUse::of(ProviderKind::Aegis), KeyUse::Required);
    assert_eq!(KeyUse::of(ProviderKind::OpenAiCompatible), KeyUse::Optional);
    assert_eq!(KeyUse::of(ProviderKind::Ollama), KeyUse::Never);
}

#[test]
fn the_keyless_half_of_the_refusal_does_not_tell_a_gateway_user_to_start_a_server() {
    // `openai-compatible` joins the keyless list, and that list's sentence used
    // to end "and start its server" -- true while `ollama` was alone in it and
    // false for a reader whose endpoint is a hosted gateway they do not run.
    let refusal = NoKindSelected {
        alias: ModelAlias::Default,
        with_a_client: crate::compose::KINDS_WITH_A_CLIENT.to_vec(),
    };
    let said = refusal.to_string();

    assert!(
        said.contains(KEYLESS_ENDING),
        "the keyless half names what the reader must supply: {said}",
    );
    assert!(
        !said.contains("start its server"),
        "and not an act half of them cannot perform: {said}",
    );
    // The accepting sibling: the keyed half is unchanged and still names the
    // command, so this is a widening rather than a sentence that lost a route.
    assert!(
        said.contains("providers keys add"),
        "the keyed half still names its command: {said}",
    );
    assert!(
        said.contains(ProviderKind::OpenAiCompatible.as_str())
            && said.contains(ProviderKind::Ollama.as_str()),
        "and both keyless kinds are named: {said}",
    );
}

#[test]
fn the_kind_key_is_a_sibling_and_collides_with_no_endpoint_key() {
    // `provider.<alias>.kind` and `provider.<kind>.endpoint` share a table.
    // They cannot collide while no alias is spelled like a kind, and that is
    // asserted rather than assumed -- a sixth alias named `ollama` would make
    // one key two things.
    for alias in ModelAlias::ALL {
        assert!(
            ProviderKind::parse(alias.as_str()).is_none(),
            "the alias `{alias}` is spelled like a provider kind, so `provider.{alias}.kind` and \
             that kind's own subtable are the same path"
        );
        for kind in ProviderKind::ALL {
            assert_ne!(
                kind_key(alias).as_str(),
                kind.endpoint_key().as_str(),
                "the kind key for `{alias}` and the endpoint key for `{kind}` are one key"
            );
        }
    }
}

#[test]
fn a_project_may_choose_a_model_and_may_not_choose_the_provider_kind() {
    // **Both arms**, because the refusal alone is satisfied by a schema that
    // refuses the project layer everything -- which would be wrong, and wrong
    // in a way every refusal check passes perfectly. The accepting arm is the
    // check above's: `model.<alias>` stays free at every layer.
    //
    // The refusing arm is a stronger form of the argument that refuses the
    // endpoint key. An endpoint redirects a user's prompts to another address;
    // a kind redirects them to another PROVIDER, which for a user running
    // locally means off the machine entirely.
    let schema = schema();
    let mut permitted: Vec<ModelAlias> = Vec::new();
    for alias in ModelAlias::ALL {
        let key = kind_key(alias);
        match Resolution::resolve(&schema, [at(Layer::Project, &key, "gemini")]) {
            Err(ConfigRefused::ProjectMayNotSet { key: named, reason }) => {
                assert_eq!(named, key, "the refusal names the key the project set");
                assert!(
                    reason.contains("prompts"),
                    "the refusal does not say what is at stake: {reason}"
                );
            }
            _ => permitted.push(alias),
        }
    }
    assert!(
        permitted.is_empty(),
        "a project may set the provider kind for {permitted:?}, so a cloned repository can choose \
         which provider a user's prompts are sent to"
    );

    // The accepting arm, stated here too so this check is not satisfied by a
    // schema that refuses the project layer everything.
    let alias_key = ModelAlias::Default.key();
    let resolution = Resolution::resolve(&schema, [at(Layer::Project, &alias_key, "a-model")])
        .expect("ADR-0012 D4 lists project configuration among the five layers");
    let table = ModelTable::from_configuration(&resolution).expect("the value is text");
    assert!(
        matches!(
            table.row(ModelAlias::Default),
            ResolvedModel::Resolved { .. }
        ),
        "a project asking for a different model is ADR-0012 D4 working, and it stopped working"
    );
}

/// A refusal carries what it needs to name both routes.
#[test]
fn the_refusal_carries_the_kinds_this_build_reaches() {
    let refusal = NoKindSelected {
        alias: ModelAlias::Default,
        with_a_client: WITH_A_CLIENT.to_vec(),
    };
    assert_eq!(refusal.with_a_client.len(), 2);
}

/// [ADR-0012] D3's fourth concern: each kind states its window from its own
/// source, and a kind with no source says so rather than guessing.
///
/// # Why the three sources are asserted together
///
/// They are one decision read three ways, and the failure this guards against
/// is a later kind quietly borrowing another's number — which is exactly what
/// the composition did until 2026-09-14, when one constant cited from Google's
/// page for `gemini-3.6-flash` was the window every provider was measured
/// against. Asserting them apart would let two of them agree by accident.
///
/// Watched red twice, each mutation confirmed applied on disk and the file
/// restored byte-identical:
///
/// - `DEFAULT_CONTEXT_TOKENS` set to `/api/show`'s 131,072 — *"Ollama serves
///   `num_ctx` and its default is 4,096 whatever the model was trained on;
///   taking `/api/show`'s 131,072 would have the harness believe thirty-two
///   times the room it has and let the server truncate in silence"*, left
///   131072, right 4096;
/// - `require_context_size` answering `Ok(0)` where the descriptor states no
///   window — *"a window nothing states is refused before a loop starts"*, so
///   the loop would have run against a zero window with nothing said.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[test]
fn each_kind_states_its_window_from_its_own_source_and_a_kind_without_one_refuses() {
    use crate::providers::{ProviderCapabilities, ProviderKind};

    // `gemini`: its own published number, and the citation is on the constant.
    assert_eq!(
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
        1_048_576,
        "Google's model page for `gemini-3.6-flash` states an input token limit of 1,048,576"
    );

    // `ollama`: the server's own default, not the model's trained length.
    // 131,072 is what `/api/show` reports for `llama3.2:3b` and is what this
    // number is deliberately not.
    assert_eq!(
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
        4_096,
        "Ollama serves `num_ctx` and its default is 4,096 whatever the model was trained on; \
         taking `/api/show`'s 131,072 would have the harness believe thirty-two times the room \
         it has and let the server truncate in silence"
    );
    assert_ne!(
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
        131_072
    );

    // `openai-compatible`: the key and no default, so the descriptor answers
    // `None` and the refusal names the key to set.
    let unknown = ProviderCapabilities::declared(true, true, true, None);
    assert_eq!(
        unknown.context_tokens(),
        None,
        "a kind with no default window says so"
    );
    let refusal = unknown
        .require_context_size(ModelAlias::Default, ProviderKind::OpenAiCompatible)
        .expect_err("a window nothing states is refused before a loop starts");
    let said = refusal.to_string();
    assert!(
        said.contains("openai-compatible") && said.contains("context window"),
        "the refusal names the kind and what is missing: {said}"
    );
    let remedy = crate::failure::Presentation::of(&crate::failure::Classified::from(refusal));
    let printed = format!("{remedy:?}");
    assert!(
        printed.contains(ProviderKind::OpenAiCompatible.context_tokens_key().as_str()),
        "the remedy names the key to set, because a refusal a reader cannot act on is a stop \
         rather than a remedy: {printed}"
    );

    // And a kind that does state one hands it back rather than refusing.
    let known = ProviderCapabilities::declared(true, true, true, Some(4_096));
    assert_eq!(
        known
            .require_context_size(ModelAlias::Default, ProviderKind::Ollama)
            .expect("a stated window is not refused"),
        4_096
    );
}

// ---------------------------------------------------------------------------
// The exchange ceiling — ADR-0012 D3, one figure for every kind
// ---------------------------------------------------------------------------

/// The ceiling is ten minutes, and the figure is a measurement.
///
/// **This check is `openai_compatible`'s own, moved rather than written.** It
/// stood beside that kind's constant as
/// `the_exchange_ceiling_is_the_local_one_because_a_cold_load_took_minutes`
/// and carried the cold-load measurement that is the reason for 600; the
/// constant moved to the seam on 2026-09-15 and its reason moved with it,
/// because a figure whose measurement is deleted is a figure nobody can
/// defend. The second measurement is the one that made the move necessary.
#[test]
fn the_exchange_ceiling_is_ten_minutes_for_every_kind() {
    assert_eq!(
        crate::providers::transport::EXCHANGE_TIMEOUT.as_secs(),
        600,
        "measured at both ends: a cold load of llama3.2:3b through llama-server took over four \
         minutes before a token on 2026-09-14, so a 60-second ceiling calls a working server \
         unreachable; and a reasoning turn against gemini-3.6-flash had its first SSE byte at \
         92.7s on 2026-09-15, so a 60-second ceiling kills a turn inside the model's ordinary \
         range",
    );
}

/// A real timeout says what it was refused at, and it runs in milliseconds.
///
/// **The ceiling is passed as an argument, which is the whole reason this can
/// be checked at all.** Driving the real figure would mean a check that waits
/// ten minutes; eighty milliseconds against a listener that accepts the
/// connection and then answers nothing produces the same `reqwest` failure by
/// the same route, and `transport_detail_within` composes the figure it was
/// given rather than one it reads.
///
/// The client is built through `crate::web::client::build`, the one builder
/// in this workspace and the one the three provider clients call, so what is
/// exercised is the path a turn takes rather than a `reqwest` builder written
/// for the check.
#[tokio::test]
async fn a_timed_out_exchange_names_the_ceiling_it_was_refused_at() {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let keeper = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the client connects");
        // Held open and answered never. The connection is not refused and the
        // name resolves, so the only thing that can end the request is the
        // ceiling.
        let _ = finished.recv_timeout(core::time::Duration::from_secs(30));
        drop(stream);
    });

    let ceiling = core::time::Duration::from_millis(80);
    let http = crate::web::client::build(ceiling, reqwest::redirect::Policy::default())
        .expect("an HTTP client builds");
    let error = http
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect_err("a server that answers nothing cannot produce a response");

    assert!(
        error.is_timeout(),
        "a listener that accepts and never answers must fail as a timeout, or this check is \
         measuring something else: {error}"
    );
    let said = crate::providers::transport::transport_detail_within(&error, ceiling);
    assert!(
        said.ends_with("after 80ms"),
        "a timed-out exchange must name the bound it was refused at, because a turn lost to a \
         ceiling and a turn lost to a dead socket otherwise read identically: {said}"
    );

    let _ = done.send(());
    keeper.join().expect("the listener thread ends");
}

/// A failure that is not a timeout says exactly what it said.
///
/// **The accepting sibling of the check above, and it is what says the figure
/// joins a timeout rather than every transport failure.** A refused connection
/// is the case that already read well -- `providers::transport` exists because
/// it did not, and the sentence it now composes is not this arc's to change.
/// Without this half, a `transport_detail_within` that appended the ceiling
/// unconditionally would pass.
#[tokio::test]
async fn a_refused_connection_gains_no_ceiling() {
    // Bound only to learn a free port from the operating system, and dropped
    // before any client is built, so the kernel refuses.
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();
    drop(listener);

    let ceiling = core::time::Duration::from_millis(80);
    let http = crate::web::client::build(ceiling, reqwest::redirect::Policy::default())
        .expect("an HTTP client builds");
    let error = http
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect_err("nothing is listening on that port");

    assert!(
        !error.is_timeout(),
        "a refused connection is not a timeout, or this check cannot see the difference: {error}"
    );
    let said = crate::providers::transport::transport_detail_within(&error, ceiling);
    assert_eq!(
        said,
        crate::providers::transport::transport_detail(&error),
        "a transport failure that is not a timeout must say exactly what the walk says, with no \
         ceiling appended: {said}"
    );
}
