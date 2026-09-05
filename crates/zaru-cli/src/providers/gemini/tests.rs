// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The mapping, both ways, with no socket anywhere.
//!
//! Every check here is a pure function over a value. [Testing] forbids a
//! check calling a provider, and the CI runner has no key — so the mapping is
//! exercised offline or it is not exercised at all.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use super::endpoint::{API_VERSION, DEFAULT_ENDPOINT, Endpoint, METHOD};
use super::failure::{DETAIL_WITHHELD, GeminiFailure};
use super::wire;
use super::{API_KEY_HEADER, map};
use crate::credentials::Alias;
use crate::credentials::fixtures::{ascii_core, provider_secret_nonce};
use crate::providers::ProviderKind;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::resolution::{ModelId, ModelTable};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{ModelRequest, ModelResponse, ToolDescriptor, ToolResult};

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
        format!("{DEFAULT_ENDPOINT}/{API_VERSION}/models/gemini-3.6-flash:{METHOD}")
    );
    assert!(!url.contains(&key));
    assert!(!url.contains(ascii_core(&key)));
    assert!(
        !url.contains("key="),
        "the URL carries a query parameter named `key`: {url}"
    );
    assert!(
        !url.contains('?'),
        "the URL carries a query string at all, which is where a key would end up: {url}"
    );

    // The header is where it goes, and the constant is what the client uses.
    assert_eq!(API_KEY_HEADER, "x-goog-api-key");

    // A configured endpoint with a trailing slash does not produce a double
    // slash, which some gateways route differently.
    let configured =
        ProviderEndpoint::new("https://example.invalid/proxy/").expect("a well-formed endpoint");
    assert_eq!(
        Endpoint::new(&configured).url_for(&model("m")),
        format!("https://example.invalid/proxy/{API_VERSION}/models/m:{METHOD}")
    );
}

// The request the loop's values become. Both halves: the prompt as a `user`
// turn, and this turn's tool results as a second `user` turn -- not `tool`
// and not `function`, which the current API does not accept.
#[test]
fn a_model_request_becomes_the_documented_request_body() {
    let text = "summarise the repository";
    let prompt = prompt(text);
    let tools = [ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "read a file".to_owned(),
        parameters: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.to_owned(),
    }];
    let results = [ToolResult {
        id: "call_a1".to_owned(),
        content: Redacted::by(&NothingHeld, "the file's bytes"),
        failed: false,
    }];

    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    let body = map::request_from(&request).expect("the schema is JSON");
    let json = serde_json::to_value(&body).expect("the body serialises");

    assert_eq!(json["contents"][0]["role"], "user");
    assert_eq!(json["contents"][0]["parts"][0]["text"], text);

    // The result turn. `user`, and the id echoed exactly.
    assert_eq!(json["contents"][1]["role"], "user");
    let response = &json["contents"][1]["parts"][0]["functionResponse"];
    assert_eq!(response["id"], "call_a1");
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
        results: &[],
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
        results: &[],
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
                }],
            }),
            finish_reason: Some(wire::FINISH_STOP.to_owned()),
        }],
        usage_metadata: Some(usage),
        model_version: Some("gemini-3.6-flash".to_owned()),
    };
    match map::response_from(&answer, 64).expect("it maps") {
        ModelResponse::Text { text, tokens } => {
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
                }],
            }),
            finish_reason: Some("SOMETHING_GOOGLE_ADDED_LATER".to_owned()),
        }],
        usage_metadata: Some(usage),
        model_version: None,
    };
    match map::response_from(&answer, 64).expect("it maps") {
        ModelResponse::Calls { calls, tokens } => {
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
        ModelResponse::Calls { calls, tokens } => {
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

// The text arm, against what the API actually sent.
#[test]
fn the_recorded_text_exchange_maps_to_text() {
    let answer: wire::Response =
        serde_json::from_str(RECORDED_TEXT).expect("the recorded body parses");
    match map::response_from(&answer, RECORDED_TEXT.len()).expect("it maps") {
        ModelResponse::Text { text, tokens } => {
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
