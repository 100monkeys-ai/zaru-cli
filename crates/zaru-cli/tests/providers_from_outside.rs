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
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
};
use zaru_cli::failure::{Class, Classified};
use zaru_cli::providers::gemini::map::request_from;
use zaru_cli::providers::gemini::wire::{FunctionCall, Part};
use zaru_cli::providers::{
    AliasNegotiation, CapabilityRefused, Inference, ModelAlias, ModelTable, NegotiationFailure,
    Placement, Provider, ProviderCapabilities, ProviderEndpoint, ProviderKind, RemoteModelId,
    ResolvedModel, TokenUsage, declare, disagreements, endpoint_of, inference_of,
};
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction, marker};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{ModelRequest, ToolResult};

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
    let cannot = ProviderCapabilities::declared(true, false, false, Some(1_024));
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
        capabilities: ProviderCapabilities::declared(true, true, true, Some(1_024)),
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

// --- the security corpus, over the Gemini request body ---------------------
//
// Both cases are about the round-two request `gemini-read-loop` reshaped on
// 2026-09-05. That change gave a request body a second source of text -- the
// model's own turn, echoed back -- and moved where a tool's output sits, so
// the two properties that held before it are asserted again on the new shape
// rather than assumed to have survived.

/// The awkward tail every planted value carries, so that an absence assertion
/// is not satisfied by a formatter that escapes.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

fn nonce(label: &str) -> String {
    format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

/// Everything before the first non-ASCII character: what survives `{:?}`.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// The key port, implemented outside the crate that declares it.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A directory this check owns, removed when it drops.
struct Scratch(std::path::PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A real store on a scratch root holding one bearer, and the redactor built
/// from what it holds.
fn store_holding(label: &str, value: &str) -> (Scratch, HeldSecrets, Alias) {
    let base = std::fs::canonicalize(std::env::temp_dir())
        .expect("the temporary directory resolves")
        .join(format!("pfo-{}", nonce(label)));
    std::fs::create_dir_all(&base).expect("staging: the scratch root");
    let scratch = Scratch(base.clone());

    let keys = StagedKey(SealingKey::mint());
    let mut store = CredentialStore::open(base.join("zaru")).expect("the credential store opens");
    let alias = Alias::new(label).expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias.clone(),
        Description::new("the bearer this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "staging: one secret is held");
    (scratch, held, alias)
}

/// One model turn as the API returns it.
fn recorded_call(id: &str, name: &str, signature: &str) -> Vec<Part> {
    vec![Part::FunctionCall {
        function_call: FunctionCall {
            id: Some(id.to_owned()),
            name: name.to_owned(),
            args: serde_json::json!({"path": "notes.txt"}),
        },
        thought_signature: Some(signature.to_owned()),
    }]
}

/// The model's message for one recorded call, as the loop keeps it: its call,
/// and the parts it arrived with in its echo.
fn recorded_message(id: &str, name: &str, signature: &str) -> zaru_core::conversation::Message {
    zaru_core::conversation::Message::Assistant {
        text: String::new(),
        calls: vec![zaru_core::tool_call::ToolRequest {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: r#"{"path":"notes.txt"}"#.to_owned(),
        }],
        echo: Some(
            serde_json::to_string(&recorded_call(id, name, signature)).expect("parts serialise"),
        ),
    }
}

/// **Security corpus.** A held bearer in a tool's output is redacted on the
/// wire, in the turn that read it and in every later turn that is sent it
/// again.
///
/// ADR-0008 trigger clause 6's guarantee is held by the type for the turn in
/// flight — a result is built from a `ToolResult`, which carries a `Redacted`
/// — and, since 2026-09-28, by `Prompt::assembled` for every earlier turn,
/// whose messages come back off disk and pass the redactor again on their way
/// into a prompt. It is driven through the crate's public door, over a real
/// store and a real sealing key, because a check that built its own redactor
/// would be asserting a property of its own fixture.
#[test]
fn corpus_a_held_secret_in_a_tool_result_is_redacted_in_the_reshaped_request_body() {
    use zaru_core::conversation::Message;

    let planted = format!("nn_mcp_{}{AWKWARD_TAIL}", nonce("gemini-body"));
    let (_scratch, held, alias) = store_holding("gemini-body", &planted);
    let output = format!("exit code: 0\nstdout:\ntoken = {planted}\nstderr:\n");

    // The turn that read it.
    let prompt = Prompt::new(Redacted::by(&held, "read the credentials file"));
    let turn = [
        recorded_message("call_1", "fs.read", "an-opaque-signature"),
        Message::result(
            "fs.read",
            &ToolResult {
                id: "call_1".to_owned(),
                content: Redacted::by(&held, &output),
                failed: false,
            },
        ),
    ];
    let body = request_from(&ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &turn,
    })
    .expect("a round maps");
    let in_flight = serde_json::to_string(&body).expect("the body serialises");

    // A later turn, sent the same result again from a history read off disk,
    // where it is plain text: the redactor has to take it on the way in.
    let history = [
        Message::User {
            text: "read the credentials file".to_owned(),
        },
        recorded_message("call_1", "fs.read", "an-opaque-signature"),
        Message::Tool {
            id: "call_1".to_owned(),
            name: "fs.read".to_owned(),
            content: output.clone(),
            failed: false,
        },
    ];
    let later = Prompt::assembled(&held, "", &history, "what did it say?");
    let body = request_from(&ModelRequest {
        prompt: &later,
        tools: &[],
        turn: &[],
    })
    .expect("a later turn maps");
    let later_wire = serde_json::to_string(&body).expect("the body serialises");

    for (which, wire) in [("in flight", &in_flight), ("a later turn", &later_wire)] {
        assert!(
            !wire.contains(&planted),
            "{which}: the held bearer reached the request body by value"
        );
        assert!(
            !wire.contains(ascii_core(&planted)),
            "{which}: the held bearer reached the request body by its ASCII core, which is what \
             an escaping formatter would have left intact"
        );
        assert!(
            wire.contains(&marker(&alias)),
            "{which}: nothing was replaced, so the absence above could be an empty body: {wire}"
        );
    }

    // **The accepting sibling.** The same later turn with a store that holds
    // nothing carries the text through byte for byte -- so the check above is
    // redaction rather than a mapping that drops tool output.
    let nothing = HeldSecrets::none();
    let later = Prompt::assembled(&nothing, "", &history, "what did it say?");
    let body = request_from(&ModelRequest {
        prompt: &later,
        tools: &[],
        turn: &[],
    })
    .expect("a later turn maps");
    let wire = serde_json::to_string(&body).expect("the body serialises");
    assert!(
        wire.contains(&planted),
        "a store holding nothing still removed the value, so the check above is about the \
         mapping and not about the redactor: {wire}"
    );
}

/// **Security corpus.** A request carries what the context assembled and what
/// the loop hands it for this turn, and nothing a client kept.
///
/// The failure this guards is a prompt carrying text no layer of ADR-0013 D1
/// chose. Until 2026-09-28 a client kept the model's messages of the turn in
/// flight and this case asserted they did not leak into the next turn. The
/// client keeps nothing now, and an earlier turn reaches a model only as the
/// history the context policy assembled — so the property is that a request
/// built with no history and no turn carries nothing from a request built
/// before it, and that the same message given as history is carried.
#[test]
fn corpus_a_request_carries_what_the_context_assembled_and_nothing_a_client_kept() {
    let nothing = HeldSecrets::none();
    let secret_of_the_first_turn = format!("first-turn-only-{}", nonce("history"));
    let first = recorded_message("call_1", "fs.read", &secret_of_the_first_turn);

    // Round two of the first turn: the model's message is in `turn`, and so
    // it is on the wire. The accepting sibling, asserted first so the absence
    // below cannot pass by nothing ever being carried.
    let prompt = Prompt::new(Redacted::by(&nothing, "the first task"));
    let turn = [
        first.clone(),
        zaru_core::conversation::Message::Tool {
            id: "call_1".to_owned(),
            name: "fs.read".to_owned(),
            content: "bytes".to_owned(),
            failed: false,
        },
    ];
    let body = request_from(&ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &turn,
    })
    .expect("a round maps");
    let wire = serde_json::to_string(&body).expect("the body serialises");
    assert!(
        wire.contains(&secret_of_the_first_turn),
        "the turn's own messages are not carried at all: {wire}"
    );

    // A request with no history and no turn, built after it.
    let prompt = Prompt::new(Redacted::by(&nothing, "a second task entirely"));
    let body = request_from(&ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &[],
    })
    .expect("a first exchange maps");
    let wire = serde_json::to_string(&body).expect("the body serialises");
    assert_eq!(
        body.contents.len(),
        1,
        "a request with no history carried a turn it was not handed: {wire}"
    );
    assert!(
        !wire.contains(&secret_of_the_first_turn),
        "a previous request's model output reached a request that was not handed it, so \
         something outside the context policy is keeping a conversation: {wire}"
    );
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
