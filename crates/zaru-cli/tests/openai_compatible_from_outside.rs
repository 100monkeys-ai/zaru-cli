// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One real exchange against a real OpenAI-compatible endpoint, from outside
//! the crate.
//!
//! # This check does not run on a runner, and it says so rather than passing
//!
//! [Testing] forbids a check calling a provider, and the CI runner has no model
//! server. So this file's network checks are **skipped unless an environment
//! variable says an endpoint is listening on this machine**, and when they skip
//! they print why. A gated check that passed silently would be
//! indistinguishable from one that ran, which is [Verification lessons]' "a
//! check that did not run cannot fail" in the form that actually bites: a green
//! suite that proves nothing about the thing it is named for.
//!
//! The variables are [`RUN_VARIABLE`] and [`ENDPOINT_VARIABLE`]. Setting the
//! first is a statement by whoever set it that this machine has a server to
//! call; the second says where it is, because **this kind publishes no default
//! endpoint** and there is nothing to fall back to.
//!
//! # The one thing this file can prove that no fixture can
//!
//! That the request this client *builds* is one a real server *accepts*. The
//! recorded fixtures are responses, and a response cannot tell you your request
//! was well-formed — only that this particular one was. A tool declaration with
//! a field the server rejects, a `stream_options` it refuses, a `role` it does
//! not know: every one of those is invisible to a corpus of recorded answers
//! and fatal at the first real call.
//!
//! # No credential is involved unless one is held
//!
//! `ZARU_OPENAI_COMPATIBLE_KEY_ALIAS` is deliberately **not** a variable here.
//! A key, where there is one, is read from [ADR-0007]'s store through the
//! ordinary door under the ordinary alias — never from the environment and
//! never from a file of its own. The ordinary case for this check is a local
//! server that wants none, and the client is asserted to work with `None`.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons

use zaru_cli::config::{Contribution, Layer, Resolution, Schema, Source, Table, Value};
use zaru_cli::providers::openai_compatible::OpenAiCompatibleClient;
use zaru_cli::providers::{Provider, ProviderEndpoint, ProviderKind};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{Model, ModelRequest, ModelResponse, ToolDescriptor};

/// The variable that says a server is listening on this machine.
///
/// Named rather than written inline, because the skip message quotes it and a
/// skip message naming the wrong variable is worse than none.
const RUN_VARIABLE: &str = "ZARU_OPENAI_COMPATIBLE_EXCHANGE";

/// The variable that says where it is listening.
///
/// **There is no default to fall back to**, which is this kind's own decision —
/// see `providers::openai_compatible::endpoint`. So this is required rather
/// than optional, and the skip message says so.
const ENDPOINT_VARIABLE: &str = "ZARU_OPENAI_COMPATIBLE_ENDPOINT";

/// The variable naming the model the endpoint serves.
const MODEL_VARIABLE: &str = "ZARU_OPENAI_COMPATIBLE_MODEL";

/// A redactor holding nothing, so `Prompt` can be built.
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

/// A [`ModelId`] for `name`, built the way the binary builds one.
///
/// `ModelId` is constructible only inside the resolution table, which is
/// ADR-0012 D1 as a compile error. An outside caller gets one the same way
/// `zaru --model <identifier>` does: stage a layer-5 contribution and read the
/// alias back out of the fold. No test-only door is opened in the product for
/// this — a door that exists only for a check is a door.
///
/// [`ModelId`]: zaru_cli::providers::ModelId
fn model_id(name: &str) -> zaru_cli::providers::ModelId {
    use zaru_cli::providers::{ModelAlias, ModelTable, ResolvedModel, declare};

    let mut document = Table::new();
    document.insert_path(&ModelAlias::Default.key(), Value::Text(name.to_owned()));
    let resolution = Resolution::resolve(
        &declare(Schema::new()),
        vec![Contribution::new(
            Layer::Flag,
            Source::named("openai_compatible_from_outside"),
            document,
        )],
    )
    .expect("the staged contribution resolves");

    match ModelTable::from_configuration(&resolution)
        .expect("the value is text")
        .row(ModelAlias::Default)
    {
        ResolvedModel::Resolved { model, .. } => model.clone(),
        ResolvedModel::Unresolved => panic!("the alias was set above"),
    }
}

/// What this machine was told, or `None` with a printed reason.
fn configured() -> Option<(ProviderEndpoint, String)> {
    if std::env::var(RUN_VARIABLE).is_err() {
        println!(
            "SKIPPED: {RUN_VARIABLE} is not set, so no provider is called. This check reaches a \
             server and must never run on a CI runner. To run it: start an OpenAI-compatible \
             server, then set {RUN_VARIABLE}=1, {ENDPOINT_VARIABLE} to its origin (including any \
             `/v1`, which belongs to the endpoint rather than to this client) and \
             {MODEL_VARIABLE} to a model it serves."
        );
        return None;
    }
    let Ok(origin) = std::env::var(ENDPOINT_VARIABLE) else {
        println!(
            "SKIPPED: {RUN_VARIABLE} is set but {ENDPOINT_VARIABLE} is not. This kind publishes \
             no default endpoint -- it covers vLLM, LM Studio, llama.cpp, Ollama's own `/v1` and \
             every hosted gateway -- so there is nothing to fall back to and nothing to call."
        );
        return None;
    };
    let model = std::env::var(MODEL_VARIABLE).unwrap_or_else(|_| "llama3.2:3b".to_owned());
    let endpoint = ProviderEndpoint::new(&origin)
        .unwrap_or_else(|refusal| panic!("{ENDPOINT_VARIABLE} is not an origin: {refusal}"));
    Some((endpoint, model))
}

/// A request this client built, accepted by a real server, answered in words.
#[tokio::test]
async fn one_real_exchange_against_a_real_openai_compatible_endpoint() {
    let Some((endpoint, model)) = configured() else {
        return;
    };

    // **`None`, which is the ordinary case for this kind.** A local server
    // wants no key, and the client must work with none rather than treating
    // absence as a refusal.
    let client = OpenAiCompatibleClient::new(
        endpoint.clone(),
        model_id(&model),
        ProviderKind::OpenAiCompatible.credential_alias(),
        None,
        Some(8_192),
    )
    .expect("an HTTP client builds");

    let declared = Provider::capabilities(&client);
    assert!(declared.streaming(), "this client has no non-streamed path");
    assert!(declared.tool_calling());
    assert!(declared.token_accounting());
    assert!(
        Provider::usage(&client).is_none(),
        "usage was reported before any request was made"
    );
    assert_eq!(Provider::endpoint(&client), &endpoint);

    let prompt = Prompt::new(Redacted::by(
        &NothingHeld,
        "Reply with exactly the word: acknowledged",
    ));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &[],
        results: &[],
    };

    let response = Model::respond(&client, &request)
        .await
        .expect("the exchange completes");

    let text = match &response {
        ModelResponse::Text { text, .. } => text.clone(),
        other => panic!("the endpoint answered {other:?} rather than text"),
    };
    assert!(!text.trim().is_empty(), "the endpoint returned empty text");

    // **Token accounting, which only a real exchange can show.** The recorded
    // fixtures prove this client READS a usage frame; this proves a real server
    // SENDS one in answer to the request this client builds -- which is what
    // `stream_options.include_usage` is for and what a fixture cannot say.
    let tokens = response.tokens();
    assert!(
        tokens.prompt > 0,
        "no prompt tokens were reported, so `stream_options` reached a server that ignored it \
         or was never sent: {response:?}"
    );
    assert!(tokens.completion > 0, "no completion tokens: {response:?}");

    let after = Provider::usage(&client).expect("usage is reported after an exchange");
    assert_eq!(
        after,
        zaru_cli::providers::TokenUsage::counted(tokens.prompt, tokens.completion),
        "the client's own slot and the response disagree about what the exchange cost"
    );
}

/// A tool declaration this client built, accepted, and answered with a call.
///
/// **This is the assertion the corpus cannot make.** Every recorded fixture is
/// a response; none of them says the `tools` array this client serialises is
/// one a server will accept. A schema keyword a server rejects, a `type` it
/// does not know, a name it refuses — each is invisible to a corpus of answers
/// and fatal at the first real call.
#[tokio::test]
async fn a_real_endpoint_accepts_this_clients_tool_declaration_and_answers_with_a_call() {
    let Some((endpoint, model)) = configured() else {
        return;
    };

    let client = OpenAiCompatibleClient::new(
        endpoint,
        model_id(&model),
        ProviderKind::OpenAiCompatible.credential_alias(),
        None,
        Some(8_192),
    )
    .expect("an HTTP client builds");

    let tools = [ToolDescriptor {
        name: "get_weather".to_owned(),
        description: "Get the current weather for a city.".to_owned(),
        parameters: r#"{"type":"object","properties":{"city":{"type":"string","description":"City name"}},"required":["city"]}"#
            .to_owned(),
    }];
    let prompt = Prompt::new(Redacted::by(
        &NothingHeld,
        "What is the weather in Paris right now? Use the tool.",
    ));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };

    let response = Model::respond(&client, &request)
        .await
        .expect("the exchange completes");

    let ModelResponse::Calls { calls, tokens } = &response else {
        panic!("the endpoint answered {response:?} rather than a tool call");
    };
    assert_eq!(calls.len(), 1, "one call: {calls:?}");
    assert_eq!(calls[0].name, "get_weather");
    assert!(
        !calls[0].id.is_empty(),
        "the call carries no id, and this API correlates a result to a call by id: {calls:?}"
    );

    // **The arguments arrived as text and are still text.** Parsed here only to
    // assert they are whole -- a fold that lost a fragment would leave
    // truncated JSON, which is exactly the failure a real server's fragmenting
    // stream would produce and a whole-in-one-frame server would hide.
    let arguments: serde_json::Value = serde_json::from_str(&calls[0].arguments)
        .unwrap_or_else(|parser| panic!("the folded arguments are not whole JSON: {parser}"));
    assert_eq!(
        arguments["city"].as_str().map(str::to_lowercase),
        Some("paris".to_owned()),
        "the model was asked about Paris: {arguments}"
    );
    assert!(tokens.prompt > 0 && tokens.completion > 0, "{tokens:?}");
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
