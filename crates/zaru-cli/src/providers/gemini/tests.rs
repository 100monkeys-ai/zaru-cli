// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The mapping, both ways, with no socket anywhere.
//!
//! Every check here is a pure function over a value. [Testing] forbids a
//! check calling a provider, and the CI runner has no key — so the mapping is
//! exercised offline or it is not exercised at all.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use super::endpoint::{ALT_SSE, API_VERSION, DEFAULT_ENDPOINT, Endpoint, METHOD};
use super::failure::{DETAIL_WITHHELD, GeminiFailure};
use super::wire;
use super::{API_KEY_HEADER, map, stream};
use crate::credentials::fixtures::{ascii_core, provider_secret_nonce};
use crate::credentials::{Alias, Secret};
use crate::providers::ProviderKind;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::resolution::{ModelId, ModelTable};
use zaru_core::conversation::Message;
use zaru_core::iteration::Prompt;
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{ModelRequest, ModelResponse, ToolDescriptor, ToolRequest};

/// A redactor that holds nothing, for building a `Prompt` in a check.
///
/// The redaction seam's own checks are `crate::redaction`'s; what these need
/// is a `Redacted` to exist at all, since `Prompt` has no other constructor.
struct NothingHeld;

impl zaru_core::redaction::Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

fn prompt(text: &str) -> Prompt {
    Prompt::new(Redacted::by(&NothingHeld, text))
}

fn model(name: &str) -> ModelId {
    // `ModelId` is constructible only inside the resolution table, which is
    // ADR-0012 D1 as a compile error. A check needs one, and the honest way
    // to get one is the door the product uses: stage a configuration and read
    // the alias back out of it, exactly as the binary does.
    use crate::config::{Contribution, Layer, Resolution};
    use crate::providers::{ModelAlias, ResolvedModel, declare};

    let key = ModelAlias::Default.key();
    let schema = declare(crate::config::Schema::new());
    let document = crate::config::fixtures::document([(
        Box::leak(key.as_str().to_owned().into_boxed_str()) as &'static str,
        crate::config::fixtures::text(name),
    )]);
    let resolution = Resolution::resolve(
        &schema,
        vec![Contribution::new(
            Layer::Flag,
            crate::config::Source::named("a check"),
            document,
        )],
    )
    .expect("the staged fixture resolves");

    match ModelTable::from_configuration(&resolution)
        .expect("every value is text")
        .row(ModelAlias::Default)
    {
        ResolvedModel::Resolved { model, .. } => model.clone(),
        ResolvedModel::Unresolved => panic!("the alias was set above"),
    }
}

// The whole of "a key is never in a URL", as a check over the one function
// that builds one. Its arguments are an endpoint and a model identifier;
// there is no parameter a secret could travel through, so this asserts the
// signature's consequence rather than a habit.
#[test]
fn the_url_carries_no_key_and_there_is_no_parameter_one_could_arrive_through() {
    let key = provider_secret_nonce();
    let endpoint = Endpoint::new(&Endpoint::default_endpoint());
    let url = endpoint.url_for(&model("gemini-3.6-flash"));

    assert_eq!(
        url,
        format!("{DEFAULT_ENDPOINT}/{API_VERSION}/models/gemini-3.6-flash:{METHOD}?{ALT_SSE}")
    );
    assert!(!url.contains(&key));
    assert!(!url.contains(ascii_core(&key)));
    assert!(
        !url.contains("key="),
        "the URL carries a query parameter named `key`: {url}"
    );

    // **This check said the URL carries no query string at all until
    // 2026-09-05**, and that was the strongest available statement while the
    // method was `generateContent` and nothing needed one. `alt=sse` needs
    // one, so the blunt assertion is replaced by the one it was standing in
    // for rather than dropped: the query string holds exactly one parameter,
    // and it is the wire format.
    //
    // The property is **more** load-bearing now, not less. A URL with no
    // query string cannot carry a key by construction; a URL with one can, so
    // what was a free consequence of the shape is now a thing to assert.
    let (_, query) = url.split_once('?').expect("the URL carries its transport");
    assert_eq!(
        query, ALT_SSE,
        "the query string carries something other than the wire format, which is where a key \
         would end up: {url}"
    );
    assert!(
        !query.contains(&key),
        "the query string carries the key: {url}"
    );
    assert!(!query.contains(ascii_core(&key)));

    // The header is where it goes, and the constant is what the client uses.
    assert_eq!(API_KEY_HEADER, "x-goog-api-key");

    // **The method and the transport are pinned by literal, for the same
    // reason the header above is.** The assertion on the whole URL builds its
    // expectation *from* these constants, so it agrees with them whatever
    // they say -- it pins the shape and cannot pin the contract. Reverting
    // `METHOD` to `generateContent` left that assertion green when it was
    // tried as a mutation on 2026-09-05, which is what these two lines exist
    // to catch: a client that quietly stopped streaming while its descriptor
    // still said it did.
    assert_eq!(METHOD, "streamGenerateContent");
    assert_eq!(ALT_SSE, "alt=sse");

    // A configured endpoint with a trailing slash does not produce a double
    // slash, which some gateways route differently.
    let configured =
        ProviderEndpoint::new("https://example.invalid/proxy/").expect("a well-formed endpoint");
    assert_eq!(
        Endpoint::new(&configured).url_for(&model("m")),
        format!("https://example.invalid/proxy/{API_VERSION}/models/m:{METHOD}?{ALT_SSE}")
    );
}

/// One model message as the loop keeps it, with the parts the API returned
/// in its echo.
fn model_message(id: &str, name: &str, signature: Option<&str>) -> Message {
    let parts = model_turn(id, name, signature);
    Message::Assistant {
        text: String::new(),
        calls: vec![ToolRequest {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: r#"{"path":"notes.txt"}"#.to_owned(),
        }],
        echo: Some(serde_json::to_string(&parts).expect("parts serialise")),
    }
}

/// One model turn as the API returns it.
fn model_turn(id: &str, name: &str, signature: Option<&str>) -> Vec<wire::Part> {
    vec![wire::Part::FunctionCall {
        function_call: wire::FunctionCall {
            id: Some(id.to_owned()),
            name: name.to_owned(),
            args: serde_json::json!({"path": "notes.txt"}),
        },
        thought_signature: signature.map(str::to_owned),
    }]
}

// The request the loop's values become, and all three turns of it: the prompt
// as a `user` turn, the model's own turn resent exactly as it arrived, and
// this turn's tool results as a further `user` turn -- not `tool` and not
// `function`, which the current API does not accept.
//
// **It asserted only the first and the last until 2026-09-05**, and never the
// `functionResponse.name` the reference calls Required, which is how a golden
// body written over the defective function pinned a shape rather than a
// contract.
#[test]
fn a_model_request_becomes_the_documented_request_body() {
    let text = "summarise the repository";
    let prompt = prompt(text);
    let tools = [ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "read a file".to_owned(),
        parameters: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.to_owned(),
    }];
    // A result exists only because the model asked for something, so the
    // model's own message is part of the request's input. This check used to
    // build a result turn out of nothing, which is how it asserted a shape
    // the API accepts and the model cannot read.
    let turn = [
        model_message("call_a1", "fs.read", Some("sig-a1")),
        Message::Tool {
            id: "call_a1".to_owned(),
            name: "fs.read".to_owned(),
            content: "the file's bytes".to_owned(),
            failed: false,
        },
    ];

    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        turn: &turn,
    };
    let body = map::request_from(&request).expect("the schema is JSON");
    let json = serde_json::to_value(&body).expect("the body serialises");

    assert_eq!(json["contents"][0]["role"], "user");
    assert_eq!(json["contents"][0]["parts"][0]["text"], text);

    // The model's own turn, which the documented history must include:
    // "All model-generated steps returned in Turn 1 (including thought and
    // function_call steps) exactly as received."
    assert_eq!(
        json["contents"][1]["role"], "model",
        "the model's own turn is missing from the history, so the result below answers a call \
         the model has no record of making: {json}"
    );
    let echoed = &json["contents"][1]["parts"][0];
    assert_eq!(echoed["functionCall"]["name"], "fs.read");
    assert_eq!(
        echoed["thoughtSignature"], "sig-a1",
        "the model turn was resent without its thought signature, which the API refuses by name: \
         {json}"
    );

    // The result turn. `user`, the id echoed exactly, and the **name of the
    // tool that answered** -- which is what the reference calls "Required.
    // The name of the function to call" and what this check did not assert
    // until 2026-09-05.
    assert_eq!(json["contents"][2]["role"], "user");
    let response = &json["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(response["id"], "call_a1");
    assert_eq!(
        response["name"], "fs.read",
        "the result was returned under a name that is not the tool's, so the model reads it as \
         output from something it never called: {json}"
    );
    assert_eq!(response["response"]["content"], "the file's bytes");
    assert_eq!(response["response"]["failed"], false);

    // The declaration, with the opaque schema sent as an object rather than
    // as the string `zaru-core` carries it in.
    let declaration = &json["tools"][0]["functionDeclarations"][0];
    assert_eq!(declaration["name"], "fs.read");
    assert_eq!(declaration["description"], "read a file");
    assert_eq!(declaration["parameters"]["type"], "object");
    assert_eq!(
        declaration["parameters"]["properties"]["path"]["type"],
        "string"
    );

    // A turn offering no tools sends no `tools` key at all -- which is a
    // different request from one sending an empty array.
    let bare = ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &[],
    };
    let json = serde_json::to_value(map::request_from(&bare).expect("no schema to read"))
        .expect("the body serialises");
    assert!(
        json.get("tools").is_none(),
        "an empty tool list was sent as an empty array: {json}"
    );
    assert_eq!(json["contents"].as_array().expect("one turn").len(), 1);
}

// A tool descriptor whose schema is not JSON is this harness's defect, not
// the user's and not the provider's -- and it is reported before a socket is
// opened.
#[test]
fn a_tool_schema_that_is_not_json_is_a_defect_and_is_named() {
    let prompt = prompt("anything");
    let tools = [ToolDescriptor {
        name: "cmd.run".to_owned(),
        description: "run a command".to_owned(),
        parameters: "not json at all".to_owned(),
    }];
    let failure = map::request_from(&ModelRequest {
        prompt: &prompt,
        tools: &tools,
        turn: &[],
    })
    .expect_err("the schema is not JSON");

    assert!(failure.is_defect());
    assert!(!failure.is_user_correctable());
    assert!(!failure.is_environmental());
    assert!(failure.to_string().contains("cmd.run"));
}

// The three arms, and what decides between them. The ordering clause is the
// one that matters: a candidate carrying a call *and* a finish reason this
// client does not know is a `Calls`, because a turn that asks for a tool is a
// turn that continues.
#[test]
fn a_response_becomes_one_of_the_ports_three_arms() {
    let usage = wire::UsageMetadata {
        prompt_token_count: 11,
        candidates_token_count: 7,
        thoughts_token_count: 0,
        total_token_count: 18,
    };

    // Text.
    let answer = wire::Response {
        candidates: vec![wire::Candidate {
            content: Some(wire::Content {
                role: wire::ROLE_MODEL.to_owned(),
                parts: vec![wire::Part::Text {
                    text: "forty-two".to_owned(),
                    thought_signature: None,
                }],
            }),
            finish_reason: Some(wire::FINISH_STOP.to_owned()),
        }],
        usage_metadata: Some(usage),
        model_version: Some("gemini-3.6-flash".to_owned()),
    };
    match map::response_from(&answer, 64).expect("it maps") {
        ModelResponse::Text { text, tokens, .. } => {
            assert_eq!(text, "forty-two");
            assert_eq!(tokens.prompt, 11);
            assert_eq!(tokens.completion, 7);
            assert_eq!(tokens.total(), 18);
        }
        other => panic!("a text answer became {other:?}"),
    }

    // Calls, with the provider's own id carried rather than renumbered, and
    // a finish reason this client has never heard of beside it.
    let answer = wire::Response {
        candidates: vec![wire::Candidate {
            content: Some(wire::Content {
                role: wire::ROLE_MODEL.to_owned(),
                parts: vec![wire::Part::FunctionCall {
                    function_call: wire::FunctionCall {
                        id: Some("call_9f3".to_owned()),
                        name: "fs.list".to_owned(),
                        args: serde_json::json!({"path": "."}),
                    },
                    thought_signature: None,
                }],
            }),
            finish_reason: Some("SOMETHING_GOOGLE_ADDED_LATER".to_owned()),
        }],
        usage_metadata: Some(usage),
        model_version: None,
    };
    match map::response_from(&answer, 64).expect("it maps") {
        ModelResponse::Calls { calls, tokens, .. } => {
            assert_eq!(calls.len(), 1);
            assert_eq!(
                calls[0].id, "call_9f3",
                "the provider's own call id was not carried through"
            );
            assert_eq!(calls[0].name, "fs.list");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&calls[0].arguments)
                    .expect("the arguments are the JSON they arrived as")["path"],
                "."
            );
            assert_eq!(tokens.completion, 7);
        }
        other => panic!("a tool call became {other:?}, so an unknown finish reason won"),
    }

    // Stopped, carrying the provider's own word verbatim.
    let answer = wire::Response {
        candidates: vec![wire::Candidate {
            content: None,
            finish_reason: Some("MAX_TOKENS".to_owned()),
        }],
        usage_metadata: Some(usage),
        model_version: None,
    };
    match map::response_from(&answer, 64).expect("it maps") {
        ModelResponse::Stopped { reason, .. } => assert_eq!(reason, "MAX_TOKENS"),
        other => panic!("a stop became {other:?}"),
    }

    // No candidate at all is not a shape the API documents, so it is
    // reported rather than guessed at -- and reported by the body's length.
    let empty = wire::Response {
        candidates: Vec::new(),
        usage_metadata: None,
        model_version: None,
    };
    let failure = map::response_from(&empty, 4096).expect_err("no candidate is unreadable");
    assert!(failure.is_defect());
    assert!(failure.to_string().contains("4096 byte(s)"));
}

// The security corpus. A provider's own sentence is withheld whole when it
// carries the key, by value **or** by ASCII core -- the second arm being the
// one that catches an escaping formatter, which is the mutation ADR-0007's
// Status tracking records surviving once already.
#[test]
fn a_providers_message_is_withheld_when_it_carries_the_key() {
    let key = provider_secret_nonce();
    let core = ascii_core(&key);

    let verbatim = format!("API key {key} is not valid");
    let withheld = GeminiFailure::redacted_detail(&verbatim, &key);
    assert_eq!(withheld, DETAIL_WITHHELD);
    assert!(!withheld.contains(&key));
    assert!(!withheld.contains(core));

    // The escaped form: the raw value is genuinely absent and the core is
    // not, which is exactly the case a value-only check is blind to.
    let escaped = format!("API key {:?} is not valid", key);
    assert!(
        !escaped.contains(&key),
        "the fixture is not exercising the escaped case"
    );
    assert!(escaped.contains(core));
    assert_eq!(
        GeminiFailure::redacted_detail(&escaped, &key),
        DETAIL_WITHHELD
    );

    // The discriminating arm: a message with no key in it is carried
    // through, so the two assertions above are about the key rather than
    // about the function returning a constant.
    let clean = "the model is overloaded, try again later";
    assert_eq!(GeminiFailure::redacted_detail(clean, &key), clean);
}

// ADR-0036 trigger clause 4: "A remote provider refusal preserves only the
// typed failure's redacted statement in its defect evidence; a test proves a
// credential-shaped detail is not rendered."
//
// The check above holds the redaction function; this one holds the path a
// reader meets. A provider's refusal body enters through the client's own
// `classify` -- the one place `redacted_detail` is applied to a remote body
// -- and leaves as `Presentation::of`, the rendering the binary writes to
// standard error and the pane paints. No socket and no wire fake: the body is
// bytes handed to the function the transport hands them to.
//
// **The credential-shaped detail is the key this client sent**, a synthetic
// nonce, spoken back inside a provider's sentence verbatim and through `{:?}`.
// `crate::redaction` decides that held values are what the harness redacts
// and nothing pattern-based, so the key is the credential this path can know.
//
// **The sentence is capacity prose on purpose.** ADR-0036 D2 renders a
// capacity refusal's detail to the reader with the context-token remedy, so
// that is the arm where a provider's words are carried into the rendering and
// where a detail that escaped redaction would be shown. The accepting sibling
// is the same sentence without the key: its words must reach the rendering,
// or every absence asserted here is satisfied by a path that renders nothing.
#[test]
fn adr_0036_clause_4_a_remote_refusal_renders_no_credential_it_carried() {
    use crate::cli::classify::Surface;
    use crate::failure::{Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let key = provider_secret_nonce();
    let core = ascii_core(&key).to_owned();
    let client = super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("gemini-3.6-flash"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        Secret::provider(ProviderKind::Gemini, key.clone()).expect("a nonce is a provider secret"),
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");
    let surface = Surface::new("0.0.0", "https://example.invalid/report");

    let prose = "request exceeds the maximum context token limit";
    let rendered = |code: u16, status: &str, message: &str| -> String {
        let body = serde_json::json!({
            "error": { "code": code, "message": message, "status": status }
        })
        .to_string();
        let failure = client.classify(code, body.as_bytes());
        let classified = surface.provider_failure(
            &ProviderFailure::Gemini(failure),
            SessionEvidence::NoSessionExists,
        );
        Presentation::of(&classified).to_string()
    };

    // --- the accepting sibling: the provider's words reach the reader -------
    let clean = rendered(400, "INVALID_ARGUMENT", prose);
    assert!(
        clean.contains(prose) && clean.contains("provider.gemini.context_tokens"),
        "a capacity refusal with no credential in it must render the provider's sentence and \
         the context-token key, or the absences below prove nothing: {clean}"
    );

    // --- the credential, spoken back verbatim and escaped -------------------
    for (form, message) in [
        ("verbatim", format!("{prose}; credential {key} was sent")),
        ("escaped", format!("{prose}; credential {key:?} was sent")),
    ] {
        assert!(
            message.contains(&core),
            "the {form} fixture does not carry the key's ASCII core"
        );
        for (code, status) in [(400, "INVALID_ARGUMENT"), (413, "FAILED_PRECONDITION")] {
            let shown = rendered(code, status, &message);
            assert!(
                !shown.contains(&key) && !shown.contains(&core),
                "a remote refusal (HTTP {code}) carrying the key {form} rendered it to the \
                 reader: {shown}"
            );
            assert!(
                !shown.trim().is_empty(),
                "a remote refusal (HTTP {code}) rendered nothing at all, which hides the key by \
                 hiding everything"
            );
        }
    }
}

// ADR-0036 D2: a remote refusal that names a context or token capacity is the
// reader's to fix, rendered with the context-token remedy, and "does not claim
// the harness malfunctioned".
//
// Held where a person reads it: an AIP-193 body through the client's own
// `classify`, `Surface::provider_failure` and `Presentation::of`, the text the
// binary writes to standard error and the pane paints. Until 2026-09-27 the
// capacity refusal was a `RequestRefused`, whose sentence says the request was
// malformed and that this harness built it, so one rendering told the reader
// both that the fix was theirs and that the harness had built a bad request.
//
// The accepting sibling is a 400 that names no capacity: that one is still the
// harness's defect, so the capacity reading cannot pass by reclassifying every
// refused request.
#[test]
fn adr_0036_d2_a_remote_capacity_refusal_names_its_cause_and_never_a_malformed_request() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let client = super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("gemini-3.6-flash"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
            .expect("a nonce is a provider secret"),
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");
    let surface = Surface::new("0.0.0", "https://example.invalid/report");
    let shown = |code: u16, status: &str, message: &str| -> Presentation {
        let body = serde_json::json!({
            "error": { "code": code, "message": message, "status": status }
        })
        .to_string();
        Presentation::of(&surface.provider_failure(
            &ProviderFailure::Gemini(client.classify(code, body.as_bytes())),
            SessionEvidence::NoSessionExists,
        ))
    };

    // Every clause is checked and every miss reported, so one red names all
    // of what is wrong with the rendering rather than the first.
    let prose = "request exceeds the maximum context token limit";
    let mut misses = Vec::new();
    for (code, status) in [(400, "INVALID_ARGUMENT"), (413, "FAILED_PRECONDITION")] {
        let before = misses.len();
        let capacity = shown(code, status, prose);
        let said = capacity.to_string();
        println!("HTTP {code}: {said}");
        if capacity.class != Class::UserCorrectable {
            misses.push(format!(
                "HTTP {code}: a capacity refusal is the reader's to fix, and this one is {:?}",
                capacity.class
            ));
        }
        if said.contains("malformed") || said.contains("this harness built") {
            misses.push(format!(
                "HTTP {code}: a capacity refusal the reader can fix claims the harness built a \
                 malformed request"
            ));
        }
        if !(capacity.headline.contains("capacity") && capacity.headline.contains(prose)) {
            misses.push(format!(
                "HTTP {code}: the statement does not name the capacity cause in its own words \
                 beside the provider's"
            ));
        }
        if !said.contains(ProviderKind::Gemini.context_tokens_key().as_str()) {
            misses.push(format!(
                "HTTP {code}: the remedy does not name the context-token key"
            ));
        }
        if misses.len() > before {
            misses.push(format!("HTTP {code} rendered: {said}"));
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));

    let malformed = shown(
        400,
        "INVALID_ARGUMENT",
        "function declaration has an invalid schema",
    );
    assert_eq!(
        malformed.class,
        Class::Defect,
        "a refused request that names no capacity is still the harness's: {malformed}"
    );
}

// **A model Gemini does not have is the person's to fix, and they are told
// which model and what Gemini said.**
//
// The body is the one the live endpoint returned on 2026-09-28 for `--model
// cheap`, copied from the failure the binary hid: 404 `NOT_FOUND`, "models/
// cheap is not found for API version v1beta, or is not supported for
// generateContent. …". Until then it was a `RequestRefused`, a defect shown
// as "a defect in Zaru" with no words, exit 70.
//
// Watched red on `f98b894`: "a 404 for a model Gemini does not have is the
// person's to fix, and it was Defect".
#[test]
fn a_model_gemini_does_not_have_is_the_persons_to_fix_and_says_which() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let client = super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("cheap"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
            .expect("a nonce is a provider secret"),
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");
    let message = "models/cheap is not found for API version v1beta, or is not supported for \
                   generateContent. Call ModelService.ListModels to see the list of available \
                   models and their supported methods.";
    let body = serde_json::json!({
        "error": { "code": 404, "message": message, "status": "NOT_FOUND" }
    })
    .to_string();
    let shown = Presentation::of(
        &Surface::new("0.0.0", "https://example.invalid/report").provider_failure(
            &ProviderFailure::Gemini(client.classify(404, body.as_bytes())),
            SessionEvidence::NoSessionExists,
        ),
    );
    println!("{shown}");
    assert_eq!(
        shown.class,
        Class::UserCorrectable,
        "a 404 for a model Gemini does not have is the person's to fix, and it was {:?}",
        shown.class
    );
    let said = shown.to_string();
    for needed in [
        "\"cheap\"",
        "HTTP 404",
        message,
        "model.default",
        "zaru models",
    ] {
        assert!(
            said.contains(needed),
            "the refusal for a missing model does not say {needed:?}: {said}"
        );
    }
}

// ADR-0016 D1's classes, by provenance. The credential arm names the alias
// and the kind and never the key; the malformed-body arm reports a length and
// never a content.
#[test]
fn a_failure_names_the_alias_and_the_kind_and_never_the_key() {
    let key = provider_secret_nonce();
    let alias = Alias::new("provider.gemini").expect("a well-formed alias");

    let rejected = GeminiFailure::CredentialRejected {
        alias: alias.clone(),
        kind: ProviderKind::Gemini,
        code: 400,
        status: "INVALID_ARGUMENT".to_owned(),
    };
    assert!(rejected.is_user_correctable());
    assert!(!rejected.is_defect());
    assert!(!rejected.is_environmental());
    let rendered = format!("{rejected} {rejected:?}");
    assert!(rendered.contains("provider.gemini"), "{rendered}");
    assert!(rendered.contains("gemini"));
    assert!(rendered.contains("zaru providers keys add gemini"));
    assert!(!rendered.contains(&key));
    assert!(!rendered.contains(ascii_core(&key)));

    let unreadable = GeminiFailure::Unreadable {
        bytes: 913,
        parser: "expected value at line 1 column 1".to_owned(),
    };
    assert!(unreadable.is_defect());
    let rendered = unreadable.to_string();
    assert!(rendered.contains("913 byte(s)"));
    assert!(rendered.contains("deliberately not quoted"));

    let unavailable = GeminiFailure::Unavailable {
        code: Some(503),
        detail: "the model is overloaded".to_owned(),
        retry_after: None,
    };
    assert!(unavailable.is_environmental());
    assert!(!unavailable.is_defect());
    assert!(!unavailable.is_user_correctable());

    // Every variant is in exactly one class -- no variant is in two and none
    // is in none, which a wildcard-free `match` cannot promise on its own
    // because three separate predicates can disagree.
    for failure in [
        rejected,
        unreadable,
        unavailable,
        GeminiFailure::RequestRefused {
            code: 400,
            status: "INVALID_ARGUMENT".to_owned(),
            detail: "bad request".to_owned(),
        },
        GeminiFailure::CapacityRefused(crate::providers::capacity::Refused {
            code: 400,
            status: Some("INVALID_ARGUMENT".to_owned()),
            detail: "request exceeds the maximum context token limit".to_owned(),
        }),
        GeminiFailure::ContextWindowExceeded(crate::providers::capacity::Exceeded {
            needed: 2_413,
            bytes: 7_239,
            window: 2_000,
            room: 250,
            largest: None,
        }),
        GeminiFailure::ToolSchemaUnreadable {
            tool: "fs.read".to_owned(),
            parser: "expected value".to_owned(),
        },
    ] {
        let classes = u8::from(failure.is_user_correctable())
            + u8::from(failure.is_environmental())
            + u8::from(failure.is_defect());
        assert_eq!(classes, 1, "{failure:?} is in {classes} classes, not one");
    }
}

#[test]
fn only_an_explicit_remote_capacity_refusal_is_read_as_context() {
    use crate::providers::capacity::names_a_capacity;
    assert!(names_a_capacity(
        "request exceeds the maximum context token limit"
    ));
    assert!(!names_a_capacity(
        "function declaration has an invalid schema"
    ));
    // A detail withheld because it carried the key names nothing.
    assert!(!names_a_capacity(DETAIL_WITHHELD));
}

/// The whole native request, including the tool protocol history, is checked
/// before a socket can be opened. A deliberately unusable endpoint makes a
/// network attempt distinguishable from the local refusal this asserts.
#[tokio::test]
async fn an_oversized_request_is_refused_before_it_reaches_the_network() {
    let secret = Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
        .expect("a provider secret is built from a nonce");
    let client = super::GeminiClient::new(
        ProviderEndpoint::new("http://127.0.0.1:1").expect("a well-formed endpoint"),
        model("gemini-3.6-flash"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        secret,
        1,
    )
    .expect("constructing a client does not contact the endpoint");
    let prompt = prompt("a request that cannot fit one byte");

    let failure = client
        .exchange(
            &ModelRequest {
                prompt: &prompt,
                tools: &[],
                turn: &[],
            },
            crate::providers::resilience::Attempt::built_in(),
        )
        .await
        .expect_err("the locally measured request exceeds one byte");

    let GeminiFailure::ContextWindowExceeded(crate::providers::capacity::Exceeded {
        needed,
        window,
        ..
    }) = failure
    else {
        panic!("the oversized request reached the endpoint instead of being refused locally")
    };
    assert!(needed > window);
    assert_eq!(window, 1);
}

// Which statuses mean "your key", measured against the documentation rather
// than assumed. The 400 case is the awkward one and the tie is broken towards
// the user, for the asymmetric-cost reason `failure.rs` records.
#[test]
fn a_rejected_key_is_told_apart_from_a_rejected_request() {
    assert!(GeminiFailure::is_credential_status(
        401,
        "UNAUTHENTICATED",
        "anything"
    ));
    assert!(GeminiFailure::is_credential_status(
        403,
        "PERMISSION_DENIED",
        "anything"
    ));
    assert!(GeminiFailure::is_credential_status(
        400,
        "INVALID_ARGUMENT",
        "API key not valid. Please pass a valid API key."
    ));

    // A malformed request is the same code and status with a different
    // sentence, and it is not the user's.
    assert!(!GeminiFailure::is_credential_status(
        400,
        "INVALID_ARGUMENT",
        "Invalid JSON payload received. Unknown name \"contentz\"."
    ));
    assert!(!GeminiFailure::is_credential_status(
        429,
        "RESOURCE_EXHAUSTED",
        "quota"
    ));
    assert!(!GeminiFailure::is_credential_status(
        503,
        "UNAVAILABLE",
        "overloaded"
    ));
}

// ---------------------------------------------------------------------------
// The recorded exchanges. Three real bodies, captured once on 2026-09-05 from
// `generativelanguage.googleapis.com` with the issued test key, scrubbed of
// the `responseId` and of an opaque `thoughtSignature`, and checked for the
// key's absence by value and by ASCII core before they were committed. The
// key is wholly ASCII, so for these fixtures the core *is* the value and the
// two arms coincide -- which is stated rather than left to be inferred, since
// a reader could otherwise think the second arm had been exercised here.
//
// They are what makes the mapping's evidence about the real API rather than
// about a body this arc invented. Every hand-built value above is a shape
// somebody chose; these are shapes Google sent.
// ---------------------------------------------------------------------------

/// The recorded tool-call exchange.
const RECORDED_CALLS: &str = include_str!("recorded/calls.json");

/// The recorded text exchange.
const RECORDED_TEXT: &str = include_str!("recorded/text.json");

/// The recorded refusal of a key the provider does not know.
const RECORDED_REJECTED: &str = include_str!("recorded/rejected-key.json");

// No fixture in this repository carries the key, and this asserts it rather
// than trusting the scrub. It is the cheapest check here and it is the one
// whose failure would be worst.
#[test]
fn no_recorded_fixture_carries_a_credential() {
    for (name, body) in [
        ("calls.json", RECORDED_CALLS),
        ("text.json", RECORDED_TEXT),
        ("rejected-key.json", RECORDED_REJECTED),
    ] {
        assert!(
            !body.contains("AIza"),
            "{name} carries something shaped like a Google API key"
        );
        assert!(
            !body.contains("x-goog-api-key"),
            "{name} carries the header the key travels in"
        );
    }

    // The success bodies carry a `responseId`, which identifies one request
    // made by one account, and both must show it replaced. The error envelope
    // carries none, so asserting a scrub marker across all three would be a
    // check that passed for the wrong reason on one of them -- the shape a
    // loop over unlike cases produces every time.
    for (name, body) in [("calls.json", RECORDED_CALLS), ("text.json", RECORDED_TEXT)] {
        assert!(
            body.contains(r#""responseId": "<scrubbed>""#),
            "{name} carries an unscrubbed responseId, so either the scrub did not run or the \
             fixture was replaced with a raw capture"
        );
    }
    assert!(
        !RECORDED_REJECTED.contains("responseId"),
        "the recorded error envelope grew a responseId, which is an identifier and must be \
         scrubbed with the others"
    );
}

// The tool-call arm, against what the API actually sent. Three things this
// fixture settles that no hand-built body could:
//
//   1. `functionCall` really does carry an `id` for `gemini-3.6-flash`;
//   2. `finishReason` is `STOP` on a turn that asks for a tool, so reading it
//      before the calls would lose the call -- the ordering is load-bearing
//      against the real API and not only against an invented one;
//   3. a `thoughtSignature` rides beside the call, a field the reference
//      documents nowhere.
#[test]
fn the_recorded_tool_call_maps_to_calls_with_the_providers_own_id() {
    let answer: wire::Response =
        serde_json::from_str(RECORDED_CALLS).expect("the recorded body parses");

    assert_eq!(
        answer.candidates[0].finish_reason.as_deref(),
        Some(wire::FINISH_STOP),
        "the recorded tool call came back with STOP, which is why calls are read first"
    );

    match map::response_from(&answer, RECORDED_CALLS.len()).expect("it maps") {
        ModelResponse::Calls { calls, tokens, .. } => {
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].name, "get_weather");
            assert_eq!(
                calls[0].id, "call_810804",
                "the id Google sent was not carried through"
            );
            assert!(!calls[0].id.is_empty());
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&calls[0].arguments)
                    .expect("the arguments are JSON")["city"],
                "Zurich"
            );
            // The token arithmetic, checked against Google's own total rather
            // than asserted. 54 + (17 + 62) = 133, which is `totalTokenCount`
            // exactly; 54 + 17 is 71, which is not.
            let reported = answer
                .usage_metadata
                .expect("the recorded body reports usage");
            assert_eq!(tokens.prompt, reported.prompt_token_count);
            assert_eq!(
                tokens.completion,
                reported.candidates_token_count + reported.thoughts_token_count
            );
            assert_eq!(
                tokens.total(),
                reported.total_token_count,
                "the reported total does not match Google's own, so a billed quantity is being \
                 dropped: candidates {} + thoughts {} + prompt {} against total {}",
                reported.candidates_token_count,
                reported.thoughts_token_count,
                reported.prompt_token_count,
                reported.total_token_count
            );
            assert!(
                reported.thoughts_token_count > 0,
                "the fixture no longer exercises the thinking-token case it was recorded for"
            );
        }
        other => panic!("the recorded tool call became {other:?}"),
    }
}

/// The round-two request, built from the round-one response the API really
/// sent. This is the `gemini-read-loop` defect's regression check.
///
/// # What was wrong, measured rather than reasoned about
///
/// Until 2026-09-05 this client built a round-two body of two `user` turns:
/// the prompt, then the results. The model's own turn was never resent, and
/// each `functionResponse` was named for the **call id** rather than for the
/// tool. Against `gemini-3.6-flash`, replaying one recorded round two:
///
/// | Shape | Result over eight trials |
/// | --- | --- |
/// | as shipped | answered 2, asked for the same tool again **6** |
/// | only the name corrected | **answered 8** |
/// | model turn resent, no signature | **HTTP 400 × 8** |
/// | model turn resent with its signature, name corrected | **answered 8** |
/// | model turn resent with its signature, name left as the id | **HTTP 400 × 8** |
///
/// The last two rows are why both halves land together: once a model turn is
/// present the API validates the name against it, and today's request only
/// gets a 200 because the missing turn leaves it nothing to validate against.
#[test]
fn a_second_round_carries_the_model_turn_and_names_the_tool_that_answered() {
    let answer: wire::Response =
        serde_json::from_str(RECORDED_CALLS).expect("the recorded body parses");
    let ModelResponse::Calls {
        calls, text, echo, ..
    } = map::response_from(&answer, RECORDED_CALLS.len()).expect("the recorded round maps")
    else {
        panic!("the recorded round is a tool call");
    };
    assert!(
        echo.is_some(),
        "the recorded round carries a thought signature, so its parts must be kept to be given \
         back exactly"
    );

    let prompt = prompt("what is the weather in Zurich");
    let result = "exit code: 0\nstdout:\n7C\nstderr:\n";
    let turn = [
        Message::assistant(&NothingHeld, &text, &calls, echo),
        Message::Tool {
            id: "call_810804".to_owned(),
            name: "get_weather".to_owned(),
            content: result.to_owned(),
            failed: false,
        },
    ];
    let body = map::request_from(&ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &turn,
    })
    .expect("a recorded round maps");
    let json = serde_json::to_value(&body).expect("the body serialises");
    let contents = json["contents"].as_array().expect("contents is an array");

    assert_eq!(
        contents.len(),
        3,
        "a round-two request is the prompt, the model's turn and the results; the model's turn \
         is what a re-reading model was never given back: {json}"
    );
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[2]["role"], "user");

    // The model turn, resent as it arrived. Both halves: the call, and the
    // signature the API refuses a call without.
    let echoed = &contents[1]["parts"][0];
    assert_eq!(echoed["functionCall"]["name"], "get_weather");
    assert_eq!(echoed["functionCall"]["id"], "call_810804");
    assert_eq!(
        echoed["thoughtSignature"],
        serde_json::Value::String(
            "<scrubbed: an opaque signature, 344 bytes as recorded>".to_owned()
        ),
        "the recorded signature was not resent, and the API's own refusal is \"Function call is \
         missing a thought_signature in functionCall parts\": {json}"
    );

    // The result, named for the tool that produced it rather than for the
    // call that asked. The reference: "Required. The name of the function to
    // call."
    let response = &contents[2]["parts"][0]["functionResponse"];
    assert_eq!(
        response["name"], "get_weather",
        "the result was returned under a name that is not the declaration's, which is what made \
         a model read its own tool output as a stranger's and ask again: {json}"
    );
    assert_eq!(response["id"], "call_810804");
    assert_eq!(response["response"]["content"], result);
    assert_eq!(
        contents[2]["parts"]
            .as_array()
            .expect("the result turn has parts")
            .len(),
        1,
        "the round asked for one call and the result turn must answer exactly it: {json}"
    );
}

/// An earlier turn is sent in its own roles, its model message with the
/// signature it arrived with, and the system text as `systemInstruction`
/// rather than as something the person said.
///
/// Watched red on the tree before 2026-09-28, where the prompt was one `user`
/// turn holding the absence line, a rendering of every earlier turn and the
/// task, and no `systemInstruction` was sent at all.
#[test]
fn an_earlier_turn_is_resent_in_its_own_roles_and_the_system_text_as_the_system_instruction() {
    let system = "SYSTEM-TEXT-5e8: working directory /w";
    let history = [
        Message::User {
            text: "read notes.txt".to_owned(),
        },
        model_message("call_1", "fs.read", Some("sig-1")),
        Message::Tool {
            id: "call_1".to_owned(),
            name: "fs.read".to_owned(),
            content: "RESULT-BODY-5e8".to_owned(),
            failed: false,
        },
        Message::Assistant {
            text: "It says hello.".to_owned(),
            calls: Vec::new(),
            echo: None,
        },
    ];
    let prompt = Prompt::assembled(&NothingHeld, system, &history, "what did it say?");
    let body = map::request_from(&ModelRequest {
        prompt: &prompt,
        tools: &[],
        turn: &[],
    })
    .expect("a first exchange maps");
    let json = serde_json::to_value(&body).expect("the body serialises");

    assert_eq!(
        json["systemInstruction"]["parts"][0]["text"], system,
        "the system text was not sent as the system instruction: {json}"
    );
    let contents = json["contents"].as_array().expect("contents");
    let roles: Vec<&str> = contents
        .iter()
        .map(|content| content["role"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        roles,
        ["user", "model", "user", "model", "user"],
        "the earlier turn was not sent in its own roles: {json}"
    );
    assert_eq!(contents[1]["parts"][0]["functionCall"]["name"], "fs.read");
    assert_eq!(
        contents[1]["parts"][0]["thoughtSignature"], "sig-1",
        "an earlier turn's model message lost the signature it arrived with: {json}"
    );
    assert_eq!(
        contents[2]["parts"][0]["functionResponse"]["response"]["content"],
        "RESULT-BODY-5e8"
    );
    assert_eq!(contents[4]["parts"][0]["text"], "what did it say?");
    let wire = serde_json::to_string(&json["contents"]).expect("contents serialise");
    assert!(
        !wire.contains("SYSTEM-TEXT-5e8"),
        "the system text reached a user turn: {wire}"
    );
}

/// A model message with nothing to give back exactly is rebuilt from its
/// text and its calls, and one from another provider is too.
#[test]
fn a_model_message_with_no_echo_is_rebuilt_from_its_text_and_calls() {
    let prompt = prompt("go on");
    let turn = [
        Message::Assistant {
            text: "Reading it.".to_owned(),
            calls: vec![ToolRequest {
                id: String::new(),
                name: "fs.read".to_owned(),
                arguments: r#"{"path":"a.txt"}"#.to_owned(),
            }],
            echo: None,
        },
        Message::Tool {
            id: String::new(),
            name: "fs.read".to_owned(),
            content: "bytes".to_owned(),
            failed: false,
        },
    ];
    let json = serde_json::to_value(
        map::request_from(&ModelRequest {
            prompt: &prompt,
            tools: &[],
            turn: &turn,
        })
        .expect("maps"),
    )
    .expect("serialises");
    let model = &json["contents"][1];
    assert_eq!(model["parts"][0]["text"], "Reading it.");
    assert_eq!(model["parts"][1]["functionCall"]["name"], "fs.read");
    assert_eq!(model["parts"][1]["functionCall"]["args"]["path"], "a.txt");
    assert!(
        model["parts"][1]["functionCall"].get("id").is_none(),
        "an id nobody issued was sent: {json}"
    );
    assert_eq!(
        json["contents"][2]["parts"][0]["functionResponse"]["name"],
        "fs.read"
    );
}

// The text arm, against what the API actually sent.
#[test]
fn the_recorded_text_exchange_maps_to_text() {
    let answer: wire::Response =
        serde_json::from_str(RECORDED_TEXT).expect("the recorded body parses");
    match map::response_from(&answer, RECORDED_TEXT.len()).expect("it maps") {
        ModelResponse::Text { text, tokens, .. } => {
            assert!(!text.trim().is_empty());
            assert!(tokens.prompt > 0 && tokens.completion > 0);
        }
        other => panic!("the recorded text exchange became {other:?}"),
    }
}

// A part this client does not understand does not lose the answer.
//
// The fallback arm exists because of what `recorded/calls.json` carries; this
// asserts the consequence directly, over a part that is *only* the
// undocumented field. Without `Part::Other` this body fails to deserialize
// and a perfectly good answer is reported as this harness's defect.
#[test]
fn a_part_this_client_does_not_understand_does_not_lose_the_answer() {
    let body = r#"{
      "candidates": [{
        "content": {"role": "model", "parts": [
          {"thoughtSignature": "an opaque blob with no documented meaning"},
          {"text": "the answer"}
        ]},
        "finishReason": "STOP"
      }],
      "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 2, "totalTokenCount": 5}
    }"#;
    let answer: wire::Response = serde_json::from_str(body).expect("an unknown part still parses");
    match map::response_from(&answer, body.len()).expect("it maps") {
        ModelResponse::Text { text, .. } => assert_eq!(text, "the answer"),
        other => panic!("an unknown part beside text produced {other:?}"),
    }
}

// The refusal the real API gives for a key it does not know, measured on
// 2026-09-05 rather than assumed: **HTTP 400, `INVALID_ARGUMENT`, "API key
// not valid. Please pass a valid API key."** -- not 401 and not 403, which is
// exactly the awkward case `is_credential_status` was written for and the
// reason the 400 arm reads the message at all.
#[test]
fn the_recorded_refusal_of_a_bad_key_is_classified_as_the_users() {
    let envelope: wire::ErrorEnvelope =
        serde_json::from_str(RECORDED_REJECTED).expect("the recorded error parses");

    assert_eq!(envelope.error.code, 400);
    assert_eq!(envelope.error.status, "INVALID_ARGUMENT");
    assert!(
        GeminiFailure::is_credential_status(
            envelope.error.code,
            &envelope.error.status,
            &envelope.error.message,
        ),
        "the real refusal of a real bad key is not recognised as the user's: {:?}",
        envelope.error.message
    );
}

/// Gemini's tool schema is an OpenAPI subset, and `additionalProperties` is
/// not in it.
///
/// # This is a regression check for a defect only a real request could find
///
/// [ADR-0011] D1's argument contract emits `"additionalProperties": false` on
/// every one of the seven built-ins, which is correct JSON Schema. Google's
/// `FunctionDeclaration.parameters` is a subset of OpenAPI 3.0's Schema object
/// and refuses an unknown key by name rather than ignoring it, so **every one
/// of the seven declarations was rejected** and every turn failed with HTTP
/// 400 `INVALID_ARGUMENT`, correctly classified as this harness's defect.
///
/// Measured 2026-09-05 against the live endpoint by the first composition that
/// handed these descriptors to a provider. It is the second time the same
/// seven descriptors have been refused wholesale by this API — that record's
/// own Update carries the first, "an empty string is not JSON" — and both were
/// invisible until something sent them.
///
/// The check drives the **real** descriptors rather than a fixture, because a
/// fixture would be asserting about a schema this harness does not send.
///
/// Watched red by mapping the parameters through unchanged, which printed
/// *"the request carries `additionalProperties`, which Gemini's schema subset
/// refuses by name: every tool declaration would be rejected"*.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn no_tool_declaration_carries_a_keyword_geminis_schema_subset_refuses() {
    use zaru_core::iteration::Prompt;
    use zaru_core::redaction::{Redacted, Redactor};
    use zaru_core::tool_call::ModelRequest;

    struct Nothing;
    impl Redactor for Nothing {
        fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
            std::borrow::Cow::Borrowed(text)
        }
    }

    let descriptors = crate::tools::descriptors();
    assert!(
        !descriptors.is_empty(),
        "no descriptors were offered, so this check asserted nothing"
    );
    // The staging is asserted: the contract really does emit the keyword, so
    // an implementation that stopped emitting it would make this check pass
    // for a reason that has nothing to do with the mapping.
    assert!(
        descriptors
            .iter()
            .all(|tool| tool.parameters.contains("additionalProperties")),
        "ADR-0011 D1's contract no longer emits `additionalProperties`, so this check is about \
         nothing: {descriptors:?}"
    );

    let prompt = Prompt::new(Redacted::by(&Nothing, "read a file"));
    let request = super::map::request_from(&ModelRequest {
        prompt: &prompt,
        tools: &descriptors,
        turn: &[],
    })
    .expect("the seven descriptors map");

    let wire = serde_json::to_string(&request).expect("the request serialises");
    assert!(
        !wire.contains("additionalProperties"),
        "the request carries `additionalProperties`, which Gemini's schema subset refuses by \
         name: every tool declaration would be rejected. {wire}"
    );
    // The discriminating arm: a mapping that dropped everything would satisfy
    // the absence above and send seven tools with no parameters at all.
    for tool in &descriptors {
        assert!(
            wire.contains(&format!("\"{}\"", tool.name)),
            "the tool `{}` is not in the request at all: {wire}",
            tool.name
        );
    }
    // **Read the properties themselves rather than the whole request.** A
    // first version asserted `wire.contains("\"path\"")`, which is satisfied by
    // the field's name appearing in `required` -- so the mutation that recursed
    // into `properties` as though it were a schema, stripping every field name
    // out of it, **survived**. Parsing is what separates "the name is
    // somewhere" from "the model is told the tool takes it".
    let sent: serde_json::Value = serde_json::from_str(&wire).expect("the request is JSON");
    let declared = sent["tools"][0]["functionDeclarations"]
        .as_array()
        .expect("one Tool entry carrying every declaration");
    assert_eq!(
        declared.len(),
        descriptors.len(),
        "the request carries {} of the {} tools this surface offers",
        declared.len(),
        descriptors.len()
    );
    for (sent_tool, offered) in declared.iter().zip(descriptors.iter()) {
        let properties = sent_tool["parameters"]["properties"]
            .as_object()
            .unwrap_or_else(|| {
                panic!(
                    "the tool `{}` was sent with no `properties` object, so the model is told \
                     nothing about what it takes: {sent_tool}",
                    offered.name
                )
            });
        assert!(
            !properties.is_empty(),
            "the tool `{}` was sent with an empty `properties`, which is the narrowing having \
             recursed into a map of field NAMES as though it were a schema: {sent_tool}",
            offered.name
        );
        // Every field the contract declares survives, by name.
        let contract: serde_json::Value =
            serde_json::from_str(&offered.parameters).expect("the contract's schema is JSON");
        for field in contract["properties"]
            .as_object()
            .expect("the contract declares properties")
            .keys()
        {
            assert!(
                properties.contains_key(field),
                "the tool `{}` lost the field `{field}` on the way to the wire: {sent_tool}",
                offered.name
            );
        }
    }
}

// --- The streamed transport, over recorded bytes and no socket -------------

/// The recorded streamed text exchange, three frames.
const RECORDED_STREAM_TEXT: &str = include_str!("recorded/stream-text.sse");

/// The recorded streamed tool call, two frames.
const RECORDED_STREAM_CALLS: &str = include_str!("recorded/stream-calls.sse");

// The same guard the non-streamed fixtures carry, over the two streamed ones.
// A fixture recorded from a live exchange is exactly where a credential would
// arrive if a scrub were skipped.
#[test]
fn no_recorded_stream_fixture_carries_a_credential() {
    for (name, body) in [
        ("stream-text.sse", RECORDED_STREAM_TEXT),
        ("stream-calls.sse", RECORDED_STREAM_CALLS),
    ] {
        assert!(
            !body.contains("AIza"),
            "{name} carries something shaped like a Google API key"
        );
        assert!(
            !body.contains("x-goog-api-key"),
            "{name} carries the header the key travels in"
        );
        assert!(
            body.contains(r#""responseId": "<scrubbed>""#),
            "{name} carries an unscrubbed responseId, so either the scrub did not run or the \
             fixture was replaced with a raw capture"
        );
        assert!(
            !body.contains("thoughtSignature\": \"E"),
            "{name} carries an unscrubbed thoughtSignature"
        );
    }
}

/// Every frame of a recorded stream, parsed.
fn frames_of(body: &str) -> Vec<wire::Response> {
    let mut reader = stream::Frames::new();
    let mut payloads = reader.feed(body.as_bytes());
    payloads.extend(reader.finish());
    payloads
        .iter()
        .map(|payload| {
            serde_json::from_str(payload).expect("a recorded frame is a well-formed response")
        })
        .collect()
}

// A read from a socket is not a frame. This feeds the recorded stream one
// byte at a time -- the worst split a network can produce -- and asserts the
// same frames come out as when it arrives whole. The mutant is a reader that
// treats each read as a frame, which works on every fast connection and fails
// on a slow one, where it is hardest to reproduce.
#[test]
fn a_frame_split_across_reads_is_reassembled() {
    let whole = frames_of(RECORDED_STREAM_TEXT);
    assert_eq!(whole.len(), 3, "the recorded text stream is three frames");

    let mut reader = stream::Frames::new();
    let mut payloads = Vec::new();
    for byte in RECORDED_STREAM_TEXT.as_bytes() {
        payloads.extend(reader.feed(&[*byte]));
    }
    payloads.extend(reader.finish());

    assert_eq!(
        payloads.len(),
        whole.len(),
        "feeding the same bytes one at a time produced a different number of frames"
    );
}

// The accepting sibling: two whole frames in one read are two frames, not one
// and not three.
#[test]
fn two_frames_in_one_read_are_two_frames() {
    let mut reader = stream::Frames::new();
    let payloads = reader.feed(b"data: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
    assert_eq!(payloads, vec![r#"{"a":1}"#, r#"{"b":2}"#]);
    assert_eq!(reader.finish(), None, "nothing was left over");
}

// A producer that ends without a trailing blank line has still sent a frame.
// The mutant drops it -- and the frame it drops is the last one, which is the
// only frame carrying the finish reason and the final usage.
#[test]
fn a_final_frame_without_a_terminator_is_not_lost() {
    let mut reader = stream::Frames::new();
    assert!(reader.feed(b"data: {\"a\":1}").is_empty());
    assert_eq!(reader.finish().as_deref(), Some(r#"{"a":1}"#));
}

// A heartbeat comment keeps a connection alive through a proxy. It is an
// event with no data, and a reader that handed it on as a payload would turn
// a healthy connection into a parse failure this harness reported as its own.
#[test]
fn a_comment_frame_carries_no_payload_and_is_not_a_parse_failure() {
    let mut reader = stream::Frames::new();
    let payloads = reader.feed(b": keep-alive\n\ndata: {\"a\":1}\n\n");
    assert_eq!(payloads, vec![r#"{"a":1}"#]);
}

// The decisive property of the fold, and the reason it exists. The recorded
// tool-call stream carries the call in frame 1 and `finishReason: "STOP"` in
// frame 2. Mapping frame by frame would answer one question twice -- `Calls`
// and then `Stopped`. The mutant is exactly that.
#[test]
fn a_streamed_tool_call_and_its_finish_reason_are_one_response() {
    let frames = frames_of(RECORDED_STREAM_CALLS);
    assert_eq!(frames.len(), 2, "the recorded call stream is two frames");
    assert!(
        frames[0].candidates[0].finish_reason.is_none(),
        "the frame carrying the call carries no finish reason, which is what makes this trap real"
    );
    assert_eq!(
        frames[1].candidates[0].finish_reason.as_deref(),
        Some("STOP")
    );

    let folded = map::fold(&frames);
    let mapped = map::response_from(&folded, RECORDED_STREAM_CALLS.len())
        .expect("the recorded call stream maps");

    match mapped {
        ModelResponse::Calls { calls, .. } => {
            assert_eq!(calls.len(), 1, "one call was asked for");
            assert_eq!(calls[0].name, "fs.read");
            assert_eq!(
                calls[0].id, "call_1605341",
                "the id is carried, never generated"
            );
            assert_eq!(calls[0].arguments, r#"{"path":"notes.txt"}"#);
        }
        other => panic!("a streamed tool call became {other:?} rather than a call"),
    }
}

// A `functionCall` arrives complete in one frame, and this asserts the
// contract this client is built on rather than the client's own behaviour. If
// Google ever splits one, this reddens and says so -- which is the point.
#[test]
fn a_streamed_function_call_arrives_whole_in_one_frame() {
    let frames = frames_of(RECORDED_STREAM_CALLS);
    let calls: Vec<_> = frames
        .iter()
        .flat_map(|frame| frame.candidates.first())
        .flat_map(|candidate| candidate.content.as_ref())
        .flat_map(|content| content.parts.iter())
        .filter_map(|part| match part {
            wire::Part::FunctionCall { function_call, .. } => Some(function_call),
            _ => None,
        })
        .collect();

    assert_eq!(calls.len(), 1, "one frame carried one whole call");
    assert!(
        calls[0].args.get("path").is_some(),
        "the arguments arrived as a finished object rather than as a fragment"
    );
    assert!(
        calls[0].id.is_some(),
        "the id arrived with the call rather than in a later frame"
    );
}

// The text arm: three frames whose text concatenates into the answer, painted
// as it arrived. The mutant keeps only the last frame's text, which is what a
// reader that overwrites rather than accumulates produces.
#[test]
fn a_streamed_answer_is_folded_into_the_whole_text() {
    let frames = frames_of(RECORDED_STREAM_TEXT);
    let folded = map::fold(&frames);
    let mapped = map::response_from(&folded, RECORDED_STREAM_TEXT.len())
        .expect("the recorded text stream maps");

    match mapped {
        ModelResponse::Text { text, .. } => {
            assert!(
                text.starts_with("One\nTwo"),
                "the first frame's text is kept"
            );
            assert!(text.ends_with("Eight"), "the last frame's text is kept");
            assert_eq!(
                text.lines().count(),
                8,
                "every frame's text is kept, in order: {text:?}"
            );
        }
        other => panic!("a streamed answer became {other:?} rather than text"),
    }
}

// The usage is the last frame's, because every frame carries a cumulative
// total. The mutant sums them, which multiplies the prompt count by the frame
// count -- over-reporting what the user pays, which is the direction
// ADR-0012 D7 cares about most.
#[test]
fn the_folded_usage_is_the_last_frames_and_is_not_a_sum() {
    let frames = frames_of(RECORDED_STREAM_TEXT);
    // The recorded counts rise across the frames, which is what makes summing
    // and taking-the-last two different answers rather than the same one.
    let counts: Vec<u64> = frames
        .iter()
        .filter_map(|frame| frame.usage_metadata)
        .map(|usage| usage.candidates_token_count)
        .collect();
    assert_eq!(
        counts,
        vec![13, 15, 15],
        "the recorded counts are cumulative"
    );

    let folded = map::fold(&frames);
    let usage = folded
        .usage_metadata
        .expect("the fold carries the last frame's usage");
    assert_eq!(
        usage.prompt_token_count, 13,
        "the prompt count is not summed"
    );
    assert_eq!(usage.candidates_token_count, 15);
}

// A `thoughtSignature` rides on the part that carries it, and the fold moves
// the parts. On the text stream it is on an empty-text final part; on the
// call stream it is on the `functionCall` part. Losing it is one third of the
// defect `gemini-read-loop` fixed, so it is asserted rather than assumed.
#[test]
fn the_fold_carries_a_thought_signature_on_either_part() {
    for (name, body) in [
        ("text", RECORDED_STREAM_TEXT),
        ("calls", RECORDED_STREAM_CALLS),
    ] {
        let folded = map::fold(&frames_of(body));
        let parts = &folded.candidates[0]
            .content
            .as_ref()
            .expect("the fold builds content")
            .parts;
        let signed = parts.iter().any(|part| match part {
            wire::Part::FunctionCall {
                thought_signature, ..
            }
            | wire::Part::Text {
                thought_signature, ..
            } => thought_signature.is_some(),
            _ => false,
        });
        assert!(signed, "the {name} fold lost its thought signature");
    }
}

// The accepting sibling for the whole fold: a stream of one frame folds to
// what that frame already was. It is why the three non-streamed fixtures keep
// their meaning -- each is a stream of length one.
#[test]
fn a_stream_of_one_frame_folds_to_that_frame() {
    let one: wire::Response =
        serde_json::from_str(RECORDED_CALLS).expect("the recorded response parses");
    let folded = map::fold(std::slice::from_ref(&one));

    let direct = map::response_from(&one, RECORDED_CALLS.len()).expect("maps");
    let through_fold = map::response_from(&folded, RECORDED_CALLS.len()).expect("maps");
    assert_eq!(
        direct, through_fold,
        "folding a single frame changed what it means"
    );
}

// The capability descriptor, offline, because it is a statement about this
// client rather than about a request. Streaming is `true` since 2026-09-05
// and the flag is read through `Provider`; `Model::capabilities` is the same
// statement through `From`, which is what `providers::capability` promises.
//
// **The mutant is the descriptor still saying `false`** -- a client that
// streams and denies it, which is the one-field drift a capability descriptor
// exists to prevent, and which nothing else here would catch.
#[test]
fn the_descriptor_says_this_client_streams_and_the_two_readings_agree() {
    use crate::providers::port::Provider;

    let secret = Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
        .expect("a provider secret is built from a nonce");
    let client = super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("gemini-3.6-flash"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        secret,
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");

    let declared = Provider::capabilities(&client);
    assert!(
        declared.streaming(),
        "this client calls streamGenerateContent and its descriptor must say so"
    );
    assert!(declared.tool_calling());
    assert!(declared.token_accounting());

    // ADR-0012 D3's descriptor is one statement read twice, so the two must
    // not be able to disagree about a word.
    let through_model = <super::GeminiClient as zaru_core::tool_call::Model>::capabilities(&client);
    assert_eq!(through_model.tool_calling, declared.tool_calling());

    // Nothing was sent: a descriptor is answered before any request.
    assert!(
        Provider::usage(&client).is_none(),
        "asking what a client can do performed an exchange"
    );
}

/// A client with no network behind it, for checks about what it holds.
fn offline_client() -> super::GeminiClient {
    let secret = Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
        .expect("a provider secret is built from a nonce");
    super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("gemini-3.6-flash"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        secret,
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network")
}

// ADR-0012 D7's number names the exchange **in flight**, from its first
// frame, which is what row 2 of the second look-and-feel audit asked for.
//
// Audit 2 measured the status row holding `813 tokens` -- the *previous*
// exchange's count -- for the whole of the next one. `Provider::usage` used
// to be written once, after the fold; `GeminiClient::record_usage` writes it
// from every frame that reports anything, so the row stops naming the
// exchange before it the moment the current one says a word.
//
// **The mutant is deleting the `record_usage` call from `absorb`**, which
// leaves `usage()` answering `None` until `exchange` ends -- exactly the
// behaviour this reverses -- and which no other check here would notice,
// because every other one reads the folded response.
//
// Driven over the recorded bytes through the client's own read path, because
// [Testing] forbids a check calling a provider.
//
// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
#[test]
fn an_exchange_reports_its_usage_from_its_first_frame() {
    use crate::providers::Provider as _;

    let client = offline_client();
    assert_eq!(
        client.usage(),
        None,
        "a client that has read no frame reported a count it could not have"
    );

    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    let body = RECORDED_STREAM_TEXT.as_bytes();
    let mut after_each: Vec<Option<(u64, u64)>> = Vec::new();
    // One frame at a time, so the reading between two frames is a reading and
    // not an artefact of where a chunk boundary fell.
    for (at, payload) in body.split_inclusive(|byte| *byte == b'\n').enumerate() {
        client
            .absorb(&mut frames, payload, at + 1, &mut received)
            .expect("every recorded frame parses");
        if received.len() > after_each.len() {
            after_each.push(
                client
                    .usage()
                    .map(|spent| (spent.prompt_tokens(), spent.completion_tokens())),
            );
        }
    }
    client
        .absorb_last(&mut frames, body.len(), &mut received)
        .expect("the recorded stream ends cleanly");

    // The recorded stream's own numbers. `thoughtsTokenCount` is 183 on every
    // frame -- the thinking is finished by the time any byte is on the wire,
    // measured against the live API on 2026-09-15 -- and the candidates count
    // is what rises as the text arrives.
    // Three frames, and the third is the one carrying `finishReason` beside
    // an empty text part -- so the count it reports is the one the fold will
    // report too, which is the next check's subject.
    assert_eq!(
        after_each,
        vec![
            Some((13, 13 + 183)),
            Some((13, 15 + 183)),
            Some((13, 15 + 183)),
        ],
        "the count did not follow the frames as they arrived"
    );
}

// A frame that reports nothing leaves the count alone rather than zeroing it.
//
// The accepting sibling of the check above: `map::usage_of` answers `None`
// rather than a zero, so a shape the API does not document cannot overwrite a
// real count with an invented one -- which is what `usage.rs` already refuses
// to do for cost.
//
// **The mutant is `usage_of` returning `Some(TokenUsage::counted(0, 0))`**
// for a frame with no `usageMetadata`.
#[test]
fn a_frame_that_reports_nothing_leaves_the_count_alone() {
    use crate::providers::Provider as _;

    let client = offline_client();
    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    // The first whole **frame**, terminator included: an SSE frame ends at a
    // blank line, so feeding the `data:` line alone would emit nothing and
    // this check would assert about an empty client.
    let end = RECORDED_STREAM_TEXT
        .find("\n\n")
        .expect("the recorded stream has a frame terminator")
        + 2;
    let first = &RECORDED_STREAM_TEXT[..end];
    client
        .absorb(&mut frames, first.as_bytes(), first.len(), &mut received)
        .expect("the recorded frame parses");
    let learned = client.usage().expect("the first frame reports a count");

    // A frame of the documented shape with its `usageMetadata` left out.
    let silent = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" more\"}],\
                  \"role\":\"model\"}}]}\n\n";
    client
        .absorb(
            &mut frames,
            silent.as_bytes(),
            first.len() + silent.len(),
            &mut received,
        )
        .expect("a frame without usageMetadata parses");

    assert_eq!(
        client.usage(),
        Some(learned),
        "a frame reporting nothing overwrote a real count"
    );
}

// The per-frame writes and the post-fold write agree, which is what lets both
// stay.
//
// `map::fold` keeps the **last** frame's `usageMetadata`, so the value
// `exchange` writes when the stream ends is the same two integers
// `record_usage` wrote on that frame. Pinned over both recorded streams, so a
// future fold that stopped agreeing reddens here rather than at a user's row.
#[test]
fn the_folded_usage_and_the_last_frames_usage_agree() {
    for (name, recorded) in [
        ("stream-text.sse", RECORDED_STREAM_TEXT),
        ("stream-calls.sse", RECORDED_STREAM_CALLS),
    ] {
        let mut frames = stream::Frames::new();
        let mut received = Vec::new();
        let client = offline_client();
        let body = recorded.as_bytes();
        client
            .absorb(&mut frames, body, body.len(), &mut received)
            .expect("every recorded frame parses");
        client
            .absorb_last(&mut frames, body.len(), &mut received)
            .expect("the recorded stream ends cleanly");

        let per_frame =
            crate::providers::Provider::usage(&client).expect("the recorded frames report a count");
        let folded = map::response_from(&map::fold(&received), body.len())
            .expect("the recorded stream maps");
        assert_eq!(
            (per_frame.prompt_tokens(), per_frame.completion_tokens()),
            (folded.tokens().prompt, folded.tokens().completion),
            "{name}: the last frame's count and the folded count disagree, so \
             the row and the session-exit line would disagree too"
        );
    }
}

// The answer reaches a watcher frame by frame, which is the whole difference
// a stream makes to a person waiting. Driven over the recorded frames rather
// than a socket: [Testing] forbids a check calling a provider.
//
// **The mutant is handing the answer on once, at the end** -- a client that
// streams from the socket and then delivers the text in a single piece, which
// paints exactly like the non-streamed client it replaced and which no other
// check here would notice.
//
// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
#[test]
fn the_answers_text_reaches_a_watcher_as_each_frame_arrives() {
    let client = offline_client();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    client.stream_deltas_to(sender);

    // Driven through the client's OWN read path -- `absorb` is what the
    // socket reaches -- over the recorded bytes in seventeen-byte pieces, so
    // frames land mid-read exactly as they do on a real connection. Driving
    // `hand_on` directly instead would prove the delta builder works and
    // prove nothing about whether the exchange uses it, which is a gap a
    // mutation found on 2026-09-05.
    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    let body = RECORDED_STREAM_TEXT.as_bytes();
    for (at, piece) in body.chunks(17).enumerate() {
        client
            .absorb(&mut frames, piece, (at + 1) * 17, &mut received)
            .expect("every recorded frame parses");
    }
    client
        .absorb_last(&mut frames, body.len(), &mut received)
        .expect("the recorded stream ends cleanly");
    assert_eq!(
        received.len(),
        3,
        "the recorded text stream is three frames"
    );

    let mut deltas = Vec::new();
    while let Ok(delta) = receiver.try_recv() {
        deltas.push(delta);
    }

    assert!(
        deltas.len() > 1,
        "the answer arrived in one piece, so nothing was streamed: {deltas:?}"
    );
    assert_eq!(
        deltas.concat(),
        "One\nTwo\nThree\nFour\nFive\nSix\nSeven\nEight",
        "the deltas do not reassemble into the answer"
    );

    // Strictly growing, which is what a reader watching the pane sees.
    let mut painted = String::new();
    let mut widths = Vec::new();
    for delta in &deltas {
        painted.push_str(delta);
        widths.push(painted.len());
    }
    assert!(
        widths.windows(2).all(|pair| pair[1] > pair[0]),
        "the painted text did not grow on every delta: {widths:?}"
    );
}

// The final frame of a streamed answer carries an empty text part beside the
// finish reason -- measured on both recorded streams. An empty delta would
// make a consumer repaint for no reason at the one moment the turn is about
// to end and repaint anyway.
#[test]
fn a_frame_with_no_text_hands_nothing_on() {
    let client = offline_client();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    client.stream_deltas_to(sender);

    // The recorded call stream: frame 1 is a `functionCall`, frame 2 is an
    // empty text part. Neither is text a reader could watch arrive. Driven
    // through the same read path the socket reaches.
    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    let body = RECORDED_STREAM_CALLS.as_bytes();
    client
        .absorb(&mut frames, body, body.len(), &mut received)
        .expect("every recorded frame parses");
    client
        .absorb_last(&mut frames, body.len(), &mut received)
        .expect("the recorded stream ends cleanly");

    assert!(
        receiver.try_recv().is_err(),
        "a tool call or an empty text part was handed on as if it were the answer"
    );
}

// A client nobody is watching builds no delta and sends nothing. This is
// `zaru "<task>"`, which has no pane, and it is the ordinary case.
#[test]
fn a_client_with_no_watcher_hands_nothing_on_and_does_not_fail() {
    let client = offline_client();
    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    let body = RECORDED_STREAM_TEXT.as_bytes();
    client
        .absorb(&mut frames, body, body.len(), &mut received)
        .expect("every recorded frame parses");
    // Reaching here is the assertion: no panic, no channel, no delta built.
    assert_eq!(received.len(), 3);
}

// A body that ends without a trailing blank line has still sent its last
// frame, and that frame is the only one carrying the finish reason and the
// final usage. `Frames::finish` is checked on its own above; this checks that
// the client's read path actually calls it, over the recorded stream with its
// terminator trimmed.
//
// **The mutant is `absorb_last` dropping the frame** -- which the recorded
// fixtures cannot catch on their own, because both end with a terminator and
// so never reach it. That is why this check exists rather than being covered
// by the two above.
#[test]
fn a_stream_that_ends_without_a_terminator_still_delivers_its_last_frame() {
    let client = offline_client();
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    client.stream_deltas_to(sender);

    let trimmed = RECORDED_STREAM_TEXT.trim_end();
    let mut frames = stream::Frames::new();
    let mut received = Vec::new();
    client
        .absorb(
            &mut frames,
            trimmed.as_bytes(),
            trimmed.len(),
            &mut received,
        )
        .expect("every recorded frame parses");
    assert_eq!(
        received.len(),
        2,
        "two frames were terminated; the third is waiting on the body ending"
    );

    client
        .absorb_last(&mut frames, trimmed.len(), &mut received)
        .expect("the trailing frame parses");
    assert_eq!(
        received.len(),
        3,
        "the frame the body ended without terminating was dropped, and it is the one carrying \
         the finish reason and the final usage"
    );

    // The whole answer still reassembles, so the trailing frame was delivered
    // to the watcher as well as kept for the fold.
    let mut deltas = Vec::new();
    while let Ok(delta) = receiver.try_recv() {
        deltas.push(delta);
    }
    assert_eq!(
        deltas.concat(),
        "One\nTwo\nThree\nFour\nFive\nSix\nSeven\nEight"
    );

    let folded = map::fold(&received);
    assert_eq!(
        folded.candidates[0].finish_reason.as_deref(),
        Some("STOP"),
        "the finish reason rides on the frame the terminator did not close"
    );
}

// A rate limit and a request timeout are the provider's condition, not a
// request this harness built wrongly. ADR-0016 D1's row 3 names "Rate limit"
// in as many words, and until this check `classify` sent every 4xx it did not
// recognise to `RequestRefused`, a defect at exit 70 telling the person the
// harness had built a malformed request. The 429 body is the shape Google's
// rate-limit page documents -- a sentence that says "exceeded" and names no
// context or token capacity -- written here, not measured.
#[test]
fn a_rate_limit_and_a_request_timeout_are_environmental_and_show_the_providers_words() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let client = super::GeminiClient::new(
        Endpoint::default_endpoint(),
        model("cheap"),
        Alias::new("provider.gemini").expect("a well-formed alias"),
        Secret::provider(ProviderKind::Gemini, provider_secret_nonce())
            .expect("a nonce is a provider secret"),
        crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");
    let mut wrong = Vec::new();
    for (code, status, message) in [
        (
            429,
            "RESOURCE_EXHAUSTED",
            "You exceeded your current quota, please check your plan and billing details.",
        ),
        (
            408,
            "DEADLINE_EXCEEDED",
            "The request timed out before a response was ready.",
        ),
    ] {
        let body = serde_json::json!({
            "error": { "code": code, "message": message, "status": status }
        })
        .to_string();
        let shown = Presentation::of(
            &Surface::new("0.0.0", "https://example.invalid/report").provider_failure(
                &ProviderFailure::Gemini(client.classify(code, body.as_bytes())),
                SessionEvidence::NoSessionExists,
            ),
        );
        if shown.class != Class::Environmental {
            wrong.push(format!(
                "HTTP {code} is the provider's condition and belongs to nobody, and it was {:?}",
                shown.class
            ));
        }
        let said = shown.to_string();
        if !said.contains(message) || !said.contains(&format!("HTTP {code}")) {
            wrong.push(format!(
                "HTTP {code} does not show the provider's own words: {said}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
