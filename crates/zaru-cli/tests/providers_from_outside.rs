// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives [ADR-0012]'s whole invariant half.
//!
//! This file is an **integration test**: it can see only what `zaru-cli`
//! exports, exactly as a future `zaru models` command or a composition root
//! would. That is the reachability evidence [Verification lessons] §25 asks
//! for, and it is the reason this exists beside the unit checks rather than
//! instead of them — a per-property check cannot see a defect that lives in a
//! seam, and every seam here is one this file has to cross.
//!
//! **It is evidence about the mechanism and must not be quoted as evidence
//! about the `zaru` binary.** `zaru` takes no arguments, reaches none of this,
//! and still prints its composition and exits 0.
//!
//! Nothing here reaches a provider, opens a socket, or holds a credential.
//! `LayerSource` has no product implementation, so this file implements one —
//! which is what a TOML reader and an argument parser will be when [ADR-0003]
//! D2's table admits them.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use zaru_cli::config::{
    Contribution, Key, Layer, LayerSource, Resolution, Schema, Source, SourceFailure, Table, Value,
    gather,
};
use zaru_cli::failure::{Class, Classified};
use zaru_cli::providers::{
    AliasNegotiation, CapabilityRefused, Inference, ModelAlias, ModelTable, NegotiationFailure,
    Placement, Provider, ProviderCapabilities, ProviderEndpoint, ProviderKind, RemoteModelId,
    ResolvedModel, TokenUsage, declare, disagreements, endpoint_of, inference_of,
};

/// One of ADR-0014 D1's layers, read through the crate's own port.
///
/// The product implements `LayerSource` nowhere — a TOML parser and an argument
/// parser are two dependencies ADR-0003 D2's table does not name — so this is
/// the caller those readers will be.
struct StagedLayer {
    layer: Layer,
    source: String,
    document: Table,
}

impl LayerSource for StagedLayer {
    fn layer(&self) -> Layer {
        self.layer
    }

    fn source(&self) -> Source {
        Source::named(self.source.clone())
    }

    fn read(&self) -> Result<Table, SourceFailure> {
        Ok(self.document.clone())
    }
}

/// An orchestrator's answer, staged. `zaru-aegis` is a skeleton and nothing
/// reaches a real one.
struct StagedOrchestrator {
    table: Vec<(ModelAlias, RemoteModelId)>,
}

impl AliasNegotiation for StagedOrchestrator {
    fn remote_table(&self) -> Result<Vec<(ModelAlias, RemoteModelId)>, NegotiationFailure> {
        Ok(self.table.clone())
    }
}

/// A configured provider, staged. Nothing in any product tree implements this.
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

fn key(text: &str) -> Key {
    Key::new(text).expect("a spelling this file owns")
}

fn document(entries: &[(Key, &str)]) -> Table {
    let mut table = Table::new();
    for (path, value) in entries {
        table.insert_path(path, Value::Text((*value).to_owned()));
    }
    table
}

/// **ADR-0012 trigger clause 1's datum, from outside the crate: every alias,
/// through every layer, naming the layer that supplied it.**
///
/// Each of the five layers sets a different subset, so the winner differs per
/// alias and a table that answered "the highest layer" for everything could not
/// produce this. The expected values are literals this file planted, never
/// values read back out of the resolution.
#[test]
fn an_outside_caller_resolves_every_alias_and_is_told_which_layer_supplied_it() {
    let schema = declare(Schema::new());

    let built_in = StagedLayer {
        layer: Layer::BuiltIn,
        source: "built-in".to_owned(),
        document: document(&[
            (ModelAlias::Default.key(), "built-in-default"),
            (ModelAlias::Fast.key(), "built-in-fast"),
            (ModelAlias::Smart.key(), "built-in-smart"),
            (ModelAlias::Cheap.key(), "built-in-cheap"),
        ]),
    };
    let user = StagedLayer {
        layer: Layer::User,
        source: "~/.zaru/config.toml".to_owned(),
        document: document(&[(ModelAlias::Fast.key(), "user-fast")]),
    };
    let project = StagedLayer {
        layer: Layer::Project,
        source: "./zaru.toml".to_owned(),
        document: document(&[(ModelAlias::Smart.key(), "project-smart")]),
    };
    let environment = StagedLayer {
        layer: Layer::Environment,
        source: "environment".to_owned(),
        document: document(&[(ModelAlias::Cheap.key(), "environment-cheap")]),
    };
    let flag = StagedLayer {
        layer: Layer::Flag,
        source: "flag".to_owned(),
        document: document(&[(ModelAlias::Local.key(), "flag-local")]),
    };

    let sources: Vec<&dyn LayerSource> = vec![&built_in, &user, &project, &environment, &flag];
    let contributions: Vec<Contribution> = gather(sources).expect("every staged layer reads");
    let resolution = Resolution::resolve(&schema, contributions).expect("the fixture resolves");
    let table = ModelTable::from_configuration(&resolution).expect("every value is text");

    let expected = [
        (ModelAlias::Default, "built-in-default", Layer::BuiltIn),
        (ModelAlias::Fast, "user-fast", Layer::User),
        (ModelAlias::Smart, "project-smart", Layer::Project),
        (ModelAlias::Cheap, "environment-cheap", Layer::Environment),
        (ModelAlias::Local, "flag-local", Layer::Flag),
    ];
    assert_eq!(
        expected.len(),
        ModelAlias::ALL.len(),
        "every alias must be exercised, and every layer must be the winner for one of them"
    );

    let mut printed = String::new();
    for (alias, model, layer) in expected {
        match table.row(alias) {
            ResolvedModel::Resolved {
                model: got,
                supplied_by,
            } => {
                assert_eq!(
                    got.as_str(),
                    model,
                    "`{alias}` must resolve to what this file planted in layer {}",
                    layer.number()
                );
                assert_eq!(
                    *supplied_by, layer,
                    "and ADR-0012 D4 requires the layer that supplied it to be named"
                );
                printed.push_str(&format!(
                    "{:8} {:22} {}\n",
                    alias.to_string(),
                    got.as_str(),
                    supplied_by.label()
                ));
            }
            ResolvedModel::Unresolved => panic!("`{alias}` was set and resolved to nothing"),
        }
    }

    // What `zaru models` would print, composed by a caller rather than by the
    // crate: ADR-0015 owns the command and it does not exist.
    println!("alias    model                  supplied by\n{printed}");
    assert_eq!(
        printed.lines().count(),
        ModelAlias::ALL.len(),
        "one row per alias, which is what D4 asks the command to print"
    );
}

/// D5, from outside: every kind is configured by the same key shape and read
/// back by the same call, and the inference axis resolves beside the model.
#[test]
fn an_outside_caller_configures_every_kind_the_same_way() {
    let schema = declare(Schema::new());

    let mut entries: Vec<(Key, &str)> = ProviderKind::ALL
        .iter()
        .map(|kind| (kind.endpoint_key(), "https://staged.example"))
        .collect();
    let axis = key(&format!("inference.{}", ModelAlias::Local.as_str()));
    entries.push((axis.clone(), "local"));

    let user = StagedLayer {
        layer: Layer::User,
        source: "~/.zaru/config.toml".to_owned(),
        document: document(&entries),
    };
    let sources: Vec<&dyn LayerSource> = vec![&user];
    let resolution = Resolution::resolve(&schema, gather(sources).expect("it reads"))
        .expect("the user's own layer may set any of these");

    for kind in ProviderKind::ALL {
        let endpoint = endpoint_of(&resolution, kind)
            .expect("the value is text")
            .unwrap_or_else(|| panic!("`{kind}` was configured and read back as nothing"));
        assert_eq!(
            endpoint.as_str(),
            "https://staged.example",
            "every kind is reached through one endpoint shape"
        );
    }

    assert_eq!(
        inference_of(&resolution, ModelAlias::Local, ProviderKind::Anthropic)
            .expect("the value names an axis"),
        Inference::Local,
        "a configured axis wins over the default the kind implies"
    );
    assert_eq!(
        inference_of(&resolution, ModelAlias::Default, ProviderKind::Ollama)
            .expect("nothing is set for this alias"),
        Inference::Local,
        "and an unset one falls back to the kind's"
    );
    assert_eq!(
        Placement::of(ProviderKind::Aegis),
        Placement::Offloaded,
        "only the aegis kind hands work to something else"
    );
}

/// D3's refusal, from outside: it happens at configuration time and reaches the
/// reader as ADR-0016's user-correctable class with a remedy naming the key.
#[test]
fn an_outside_caller_is_refused_before_a_loop_could_start() {
    let cannot = ProviderCapabilities::declared(true, false, false);
    let refusal = cannot
        .require_tool_calling(ModelAlias::Smart, ProviderKind::Ollama)
        .expect_err("a provider that cannot call tools must be refused");
    assert_eq!(
        refusal,
        CapabilityRefused::ToolCallingUnavailable {
            alias: ModelAlias::Smart,
            kind: ProviderKind::Ollama,
        }
    );

    let classified = Classified::from(refusal);
    assert_eq!(classified.class(), Class::UserCorrectable);
    let lead = classified
        .remedy()
        .expect("a user-correctable failure carries a remedy")
        .actions()
        .next()
        .expect("a remedy always has a first action")
        .lead()
        .to_string();
    assert!(
        lead.contains("model.smart"),
        "the remedy must name the key to edit: {lead:?}"
    );
    println!("refused at configuration time: {lead}");
}

/// D6 and D7, from outside: a disagreement carrying both sides, and a usage
/// value carrying what a provider reported.
#[test]
fn an_outside_caller_sees_a_disagreement_and_an_accounting() {
    let schema = declare(Schema::new());
    let user = StagedLayer {
        layer: Layer::User,
        source: "~/.zaru/config.toml".to_owned(),
        document: document(&[
            (ModelAlias::Default.key(), "ours-default"),
            (ModelAlias::Smart.key(), "ours-smart"),
        ]),
    };
    let sources: Vec<&dyn LayerSource> = vec![&user];
    let resolution =
        Resolution::resolve(&schema, gather(sources).expect("it reads")).expect("it resolves");
    let table = ModelTable::from_configuration(&resolution).expect("every value is text");

    let orchestrator = StagedOrchestrator {
        table: vec![
            (
                ModelAlias::Default,
                RemoteModelId::reported("ours-default").expect("well formed"),
            ),
            (
                ModelAlias::Smart,
                RemoteModelId::reported("theirs-smart").expect("well formed"),
            ),
        ],
    };
    let remote = orchestrator.remote_table().expect("the staged answer");
    let found = disagreements(&table, &remote);

    assert_eq!(found.len(), 1, "one alias is resolved differently");
    let shown = found[0].to_string();
    assert!(
        shown.contains("ours-smart") && shown.contains("theirs-smart"),
        "ADR-0012 D6 shows both sides and reconciles neither: {shown}"
    );
    println!("disagreement: {shown}");

    let provider = StagedProvider {
        kind: ProviderKind::Gemini,
        endpoint: ProviderEndpoint::new("https://staged.example").expect("well formed"),
        capabilities: ProviderCapabilities::declared(true, true, true),
        usage: Some(TokenUsage::counted(1_024, 256)),
    };
    let usage = provider.usage().expect("this provider accounts");
    assert_eq!(usage.prompt_tokens(), 1_024);
    assert_eq!(usage.completion_tokens(), 256);
    assert_eq!(
        usage.cost(),
        None,
        "no provider publishes pricing here, so nothing invents a cost"
    );
    assert_eq!(
        provider.capabilities().token_accounting(),
        provider.usage().is_some(),
        "the descriptor and the usage are two halves of one statement"
    );
    println!(
        "usage: {} prompt, {} completion, cost {:?}",
        usage.prompt_tokens(),
        usage.completion_tokens(),
        usage.cost()
    );
}
