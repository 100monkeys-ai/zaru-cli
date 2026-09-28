// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One real exchange against a real provider, from outside the crate.
//!
//! # This check does not run on a runner, and it says so rather than passing
//!
//! [Testing] forbids a check calling a provider, and the CI runner has no key
//! and no business having one. So this file's one network check is **skipped
//! unless an environment variable says a key is in a store at a scratch
//! home**, and when it skips it prints why. A gated check that passed
//! silently would be indistinguishable from one that ran, which is
//! [Verification lessons]' "a check that did not run cannot fail" in the form
//! that actually bites: a green suite that proves nothing about the thing it
//! is named for.
//!
//! The variable is [`RUN_VARIABLE`]. Setting it is a statement by whoever set
//! it that this machine has a key that may be spent, and the check reads the
//! key from [ADR-0007]'s store under the ordinary alias, through the ordinary
//! door — never from the environment, and never from a file of its own.
//!
//! # What it asserts, and the two arms of the absence
//!
//! That a response comes back with text in it; that usage is reported and is
//! not zero; and that the key appears in **nothing** the run produced — not
//! the response, not the rendered failure of a deliberate second exchange
//! against a wrong key, not the client's `Debug`, not the transcript this
//! check writes. Absence is asserted by the value **and** by its ASCII core,
//! because `{:?}` escapes a combining mark and a value-only assertion is
//! blind to a rendering that published every byte.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons

use zaru_cli::config::{Contribution, Layer, Resolution, Schema, Source, Table, Value};
use zaru_cli::credentials::{CredentialStore, HarnessKeys, OsKeyring};
use zaru_cli::providers::gemini::{Endpoint, GeminiClient};
use zaru_cli::providers::{Provider, ProviderKind};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{Model, ModelRequest, ModelResponse};

/// The variable that says a key is in a store on this machine.
///
/// Named rather than written inline, because the skip message quotes it and a
/// skip message naming the wrong variable is worse than none.
const RUN_VARIABLE: &str = "ZARU_GEMINI_EXCHANGE";

/// The model the issued test credential serves.
///
/// `operations/repositories` records it, measured 2026-08-25: the key serves
/// `gemini-3.6-flash`. Overridable so that whoever runs this against another
/// key is not forced to edit a check.
fn model_name() -> String {
    std::env::var("ZARU_GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.6-flash".to_owned())
}

/// A [`ModelId`] for `name`, built the way the binary builds one.
///
/// `ModelId` is constructible only inside the resolution table, which is
/// ADR-0012 D1 as a compile error. An outside caller gets one the same way
/// `zaru --model <identifier>` does: stage a layer-5 contribution and read
/// the alias back out of the fold. No test-only door is opened in the product
/// for this — a door that exists only for a check is a door.
fn model_id(name: &str) -> zaru_cli::providers::ModelId {
    use zaru_cli::providers::{ModelAlias, ModelTable, ResolvedModel, declare};

    let mut document = Table::new();
    document.insert_path(&ModelAlias::Default.key(), Value::Text(name.to_owned()));
    let resolution = Resolution::resolve(
        &declare(Schema::new()),
        vec![Contribution::new(
            Layer::Flag,
            Source::named("provider_from_outside"),
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

/// A redactor holding nothing, so `Prompt` can be built.
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Everything before the first non-ASCII character.
///
/// The second arm of every absence assertion here. A copy rather than a
/// re-export, because `crate::redaction`'s is `pub(crate)` and an outside
/// caller is exactly what this file is.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// Assert that `haystack` carries no part of `key`, and say where it was.
fn free_of_key(haystack: &str, key: &str, what: &str) {
    assert!(
        !haystack.contains(key),
        "the key appears verbatim in {what}"
    );
    let core = ascii_core(key);
    assert!(
        !core.is_empty() && !haystack.contains(core),
        "the key's ASCII core appears in {what}, which is what an escaping formatter leaves \
         intact"
    );
}

/// One real exchange, when this machine has a key to spend.
///
/// Skipped loudly otherwise. See the module documentation.
#[tokio::test]
async fn one_real_exchange_against_the_provider_and_the_key_is_in_none_of_it() {
    let Ok(_) = std::env::var(RUN_VARIABLE) else {
        // Printed, not silent. `cargo test -- --nocapture` shows it, and the
        // sentence says exactly what would make it run.
        println!(
            "SKIPPED: {RUN_VARIABLE} is not set, so no key is available and no provider is \
             called. This check reaches the network and must never run on a CI runner. To run \
             it: put a key in a store under a scratch HOME with `zaru providers keys add \
             gemini`, then set {RUN_VARIABLE}=1."
        );
        return;
    };

    // Reads the process's own home **because its operator set it**: the skip
    // line above says to put a key under a scratch `HOME` and run it there.
    // It never runs on a runner, which is what
    // `corpus_one_thing_decides_where_the_harness_lives` exempts it for.
    let root =
        CredentialStore::root_in(&zaru_cli::config::Home::of_this_user()).expect("a HOME is set");
    let store =
        CredentialStore::reading(root.clone()).expect("the store at the scratch HOME opens");
    let keyring = OsKeyring::for_store(&root);
    // The sealing key, out of the environment its operator set, for the
    // reason the home above is: `corpus_one_thing_reads_the_environment`
    // exempts this check and names why.
    let keys = HarnessKeys::within(&keyring, &zaru_cli::config::Variables::of_this_process());

    let alias = ProviderKind::credential_alias(ProviderKind::Gemini);
    let secret = store
        .secret(&alias, &keys)
        .expect("the gemini key is in the store under its ordinary alias");
    let key = secret.expose_for_dispatch().to_owned();

    // `ModelId` is the resolution table's to build, so it is read out of a
    // staged configuration exactly as the binary reads it.
    let model = model_id(&model_name());

    let client = GeminiClient::new(
        Endpoint::default_endpoint(),
        model,
        alias.clone(),
        secret,
        zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds");

    // The descriptor, before anything is sent. **Streaming is true since
    // 2026-09-05** -- this client calls `streamGenerateContent?alt=sse` and
    // nothing else, so the assertion moved with the behaviour rather than
    // being relaxed. Tool calling and token accounting are what the exchange
    // proves.
    let declared = Provider::capabilities(&client);
    assert!(
        declared.streaming(),
        "this client streams and its descriptor must say so"
    );
    assert!(declared.tool_calling());
    assert!(declared.token_accounting());
    assert!(
        Provider::usage(&client).is_none(),
        "usage was reported before any request was made"
    );

    let prompt = Prompt::new(Redacted::by(
        &NothingHeld,
        "Reply with exactly the word: acknowledged",
    ));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &[],
    };

    let response = Model::respond(&client, &request)
        .await
        .expect("the exchange completes");

    let text = match &response {
        ModelResponse::Text { text, .. } => text.clone(),
        other => panic!("the provider answered {other:?} rather than text"),
    };
    assert!(!text.trim().is_empty(), "the provider returned empty text");

    let tokens = response.tokens();
    assert!(tokens.prompt > 0, "no prompt tokens were reported");
    assert!(tokens.completion > 0, "no completion tokens were reported");

    // The pairing `providers::port` states: a descriptor saying it accounts
    // must answer `Some` once it has answered at all.
    let usage = Provider::usage(&client).expect("the descriptor said this provider accounts");
    assert_eq!(usage.prompt_tokens(), tokens.prompt);
    assert_eq!(usage.completion_tokens(), tokens.completion);
    assert!(
        usage.cost().is_none(),
        "a cost was reported, and nothing publishes pricing on this response"
    );

    // The key is in none of it. Both arms, over everything this run produced.
    free_of_key(&text, &key, "the model's answer");
    free_of_key(&format!("{response:?}"), &key, "the response's Debug");
    free_of_key(&format!("{client:?}"), &key, "the client's Debug");
    free_of_key(
        &format!("{:?}", Provider::endpoint(&client)),
        &key,
        "the endpoint's Debug",
    );

    // A deliberate second exchange under a key the provider will reject, so
    // that the *failure* path is exercised with a real API rather than
    // assumed. What is asserted is the class, the alias, and the absence.
    let wrong = zaru_cli::credentials::Secret::provider(
        ProviderKind::Gemini,
        format!("{key}-definitely-not-a-key"),
    )
    .expect("a well-formed value");
    let rejected = GeminiClient::new(
        Endpoint::default_endpoint(),
        model_id(&model_name()),
        alias.clone(),
        wrong,
        zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds")
    .exchange(&request)
    .await
    .expect_err("a key the provider does not know is refused");

    assert!(
        rejected.is_user_correctable(),
        "a rejected key was classified as {rejected:?} rather than as the user's"
    );
    let rendered = format!("{rejected} {rejected:?}");
    assert!(
        rendered.contains(alias.as_str()),
        "the refusal does not name the alias the key is stored under: {rendered}"
    );
    assert!(rendered.contains("gemini"));
    free_of_key(&rendered, &key, "the rejected-key refusal");

    println!(
        "the provider answered: {}",
        text.trim().lines().next().unwrap_or_default()
    );
    println!(
        "usage: prompt {} + completion {} = {} tokens",
        tokens.prompt,
        tokens.completion,
        tokens.total()
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
