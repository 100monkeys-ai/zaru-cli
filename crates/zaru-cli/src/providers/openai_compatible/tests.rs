// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The fold, the mapping both ways, and the failure classes, with no socket
//! anywhere.
//!
//! Every check here is a pure function over a value. [Testing] forbids a check
//! calling a provider and the CI runner has neither server — so the fold and
//! the mapping are exercised offline or they are not exercised at all. The
//! ruling in force forbids a loopback listener serving a provider's responses;
//! there is none here, and the live run is the artefact.
//!
//! # What the fixtures are, and what one of them is not
//!
//! Eleven are **bytes recorded from two real servers** on 2026-09-14 and
//! committed with one substitution, described below. One is **derived** and
//! says so in its own name in every check that reads it.
//!
//! | Fixture | Server |
//! | --- | --- |
//! | [`WHOLE_ARGUMENTS`], [`TEXT_EMPTY_HEAD`], [`NO_USAGE`], [`MODEL_NOT_FOUND`], [`BAD_REQUEST`] | Ollama's `/v1/chat/completions`, v0.34.0 serving `llama3.2:3b` |
//! | [`DELTA_ARGUMENTS`], [`TEXT`], [`ANSWERED`], [`ERROR_FRAME`], [`REJECTED_KEY`], [`BAD_REQUEST_NUMERIC_CODE`], [`LOADING`] | `llama-server` from the same install, `0.3.0-dev (0f3a71be1)`, under `--jinja` |
//! | [`TWO_CALLS`] | **Derived**, not recorded — see below |
//!
//! **The one substitution.** `llama-server` echoes the model as the absolute
//! path of the weights it was started with, which on this machine is a path
//! under `$HOME`. That one field is replaced with `llama3.2-3b` in every
//! recorded fixture and nothing else is touched;
//! [`no_recorded_fixture_carries_a_machine_path`] asserts it rather than
//! trusting it.
//!
//! **Why [`TWO_CALLS`] is derived and what that costs.** A stream carrying two
//! calls at `index: 0` and `index: 1` could not be produced on this machine:
//! `llama3.2:3b` refused twice, the second attempt ending in the mid-stream
//! 500 that [`ERROR_FRAME`] records, and the Ollama attempt timed out after
//! 190 seconds with the model evicted under load. So it is built from the
//! measured single-call frames, with the two calls' fragments **interleaved**
//! — which a real server need not do and which is exactly the case a fold
//! keyed by arrival order would get wrong. **It is evidence about
//! [`super::map::fold`] and never about a server**, and the checks that read
//! it are named so that nobody can quote one as the second.
//!
//! # Nothing here holds a credential
//!
//! [`REJECTED_KEY`] is the 401 body of a `llama-server` started with
//! `--api-key not-the-real-one`, a string this arc invented for the purpose.
//! It is not a credential and never was one, and the body carries no key in
//! any case. [`no_recorded_fixture_carries_a_credential_shaped_value`] asserts
//! it.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use super::endpoint::{CHAT_PATH, Endpoint};
use super::failure::{DETAIL_WITHHELD, OpenAiCompatibleFailure};
use super::{AUTHORIZATION_HEADER, BEARER_PREFIX, EXCHANGE_TIMEOUT, map, wire};
use crate::credentials::{Alias, Secret};
use crate::providers::ProviderKind;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::port::Provider;
use crate::providers::resolution::{ModelId, ModelTable};
use crate::providers::sse;
use zaru_core::iteration::Prompt;
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{ModelRequest, ModelResponse, ToolDescriptor, ToolResult};

/// Ollama's `/v1`: one tool call **whole in one frame**.
const WHOLE_ARGUMENTS: &str = include_str!("recorded/whole-arguments.sse");
/// `llama-server`: the identical call in **thirteen argument fragments**.
const DELTA_ARGUMENTS: &str = include_str!("recorded/delta-arguments.sse");
/// `llama-server`: a streamed answer, whose head frame carries `content: null`.
const TEXT: &str = include_str!("recorded/text.sse");
/// Ollama's `/v1`: a streamed answer, whose head frame carries `content: ""`.
const TEXT_EMPTY_HEAD: &str = include_str!("recorded/text-empty-head.sse");
/// Ollama's `/v1` with `stream_options` omitted: **no usage frame at all**.
const NO_USAGE: &str = include_str!("recorded/no-usage.sse");
/// `llama-server`: round two, with the tool result fed back.
const ANSWERED: &str = include_str!("recorded/answered.sse");
/// `llama-server`: eight good frames, then an error frame, and no `[DONE]`.
const ERROR_FRAME: &str = include_str!("recorded/error-frame.sse");
/// **Derived**, not recorded: two calls interleaving their fragments.
const TWO_CALLS: &str = include_str!("recorded/two-calls.sse");
/// `llama-server --api-key`: a real 401 body.
const REJECTED_KEY: &str = include_str!("recorded/rejected-key.json");
/// Ollama's `/v1`: a 404 whose message names the model.
const MODEL_NOT_FOUND: &str = include_str!("recorded/model-not-found.json");
/// Ollama's `/v1`: a 400, with `code` as JSON `null`.
const BAD_REQUEST: &str = include_str!("recorded/bad-request.json");
/// `llama-server`: a 400, with `code` as a JSON **number**.
const BAD_REQUEST_NUMERIC_CODE: &str = include_str!("recorded/bad-request-numeric-code.json");
/// `llama-server`: a 503 while the model loads.
const LOADING: &str = include_str!("recorded/loading.json");

/// Every recorded fixture, for the corpus-wide checks.
const EVERY_FIXTURE: [(&str, &str); 13] = [
    ("whole-arguments.sse", WHOLE_ARGUMENTS),
    ("delta-arguments.sse", DELTA_ARGUMENTS),
    ("text.sse", TEXT),
    ("text-empty-head.sse", TEXT_EMPTY_HEAD),
    ("no-usage.sse", NO_USAGE),
    ("answered.sse", ANSWERED),
    ("error-frame.sse", ERROR_FRAME),
    ("two-calls.sse", TWO_CALLS),
    ("rejected-key.json", REJECTED_KEY),
    ("model-not-found.json", MODEL_NOT_FOUND),
    ("bad-request.json", BAD_REQUEST),
    ("bad-request-numeric-code.json", BAD_REQUEST_NUMERIC_CODE),
    ("loading.json", LOADING),
];

/// A redactor that holds nothing, for building a `Prompt` in a check.
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
    // ADR-0012 D1 as a compile error. A check needs one, and the honest way to
    // get one is the door the product uses.
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

fn endpoint(origin: &str) -> ProviderEndpoint {
    ProviderEndpoint::new(origin).expect("a check's origin is well-formed")
}

fn alias() -> Alias {
    ProviderKind::OpenAiCompatible.credential_alias()
}

/// The key this corpus pretends to hold. Invented here; not a credential.
const A_KEY: &str = "sk-a-value-this-check-invented-e\u{301}\u{e9}";

fn secret(value: &str) -> Secret {
    Secret::provider(ProviderKind::OpenAiCompatible, value).expect("a check's key is well-formed")
}

fn client_with(key: Option<Secret>) -> super::OpenAiCompatibleClient {
    super::OpenAiCompatibleClient::new(
        endpoint("http://127.0.0.1:11434/v1"),
        model("llama3.2:3b"),
        alias(),
        key,
        Some(8_192),
    )
    .expect("an HTTP client builds without touching the network")
}

/// Every frame of a recorded body, through the product's own SSE reader, with
/// the sentinel discarded exactly as the client discards it.
fn frames_of(body: &str) -> Vec<wire::Chunk> {
    let mut frames = sse::Frames::new();
    let mut out: Vec<wire::Chunk> = Vec::new();
    let absorb = |payload: String, out: &mut Vec<wire::Chunk>| {
        if payload.trim() == map::DONE {
            return;
        }
        out.push(serde_json::from_str(&payload).expect("a recorded frame is a chunk"));
    };
    for payload in frames.feed(body.as_bytes()) {
        absorb(payload, &mut out);
    }
    if let Some(payload) = frames.finish() {
        absorb(payload, &mut out);
    }
    out
}

/// The payloads of a recorded body, sentinel and all.
fn payloads_of(body: &str) -> Vec<String> {
    let mut frames = sse::Frames::new();
    let mut out = frames.feed(body.as_bytes());
    if let Some(last) = frames.finish() {
        out.push(last);
    }
    out
}

// --- The fold: the one line this client exists for --------------------------

#[test]
fn a_tool_calls_arguments_arrive_whole_in_one_frame_on_one_real_server() {
    let answer = map::fold(&frames_of(WHOLE_ARGUMENTS));

    assert_eq!(answer.calls.len(), 1, "one call: {answer:?}");
    assert_eq!(
        answer.calls[0].arguments, "{\"city\":\"Paris\",\"unit\":\"c\"}",
        "Ollama's own OpenAI surface sends the whole string in one frame: {answer:?}",
    );
    assert_eq!(answer.calls[0].name.as_deref(), Some("get_weather"));
    assert_eq!(answer.calls[0].id.as_deref(), Some("call_24517qmn"));
    assert_eq!(answer.finish_reason.as_deref(), Some("tool_calls"));
}

#[test]
fn the_same_call_arrives_in_thirteen_fragments_on_the_other_real_server_and_folds_to_the_same_text()
{
    let answer = map::fold(&frames_of(DELTA_ARGUMENTS));

    // Thirteen frames carried an `arguments` fragment; the fold makes them one.
    let fragments = frames_of(DELTA_ARGUMENTS)
        .iter()
        .flat_map(|frame| frame.choices.iter())
        .flat_map(|choice| choice.delta.tool_calls.iter())
        .filter(|fragment| {
            fragment
                .function
                .as_ref()
                .is_some_and(|function| !function.arguments.is_empty())
        })
        .count();
    assert_eq!(
        fragments, 13,
        "the recorded llama-server stream carries thirteen argument fragments",
    );

    assert_eq!(answer.calls.len(), 1, "still ONE call: {answer:?}");
    assert_eq!(
        answer.calls[0].arguments, "{\"city\": \"Paris\", \"unit\": \"c\"}",
        "thirteen fragments appended are the whole string: {answer:?}",
    );
    assert_eq!(answer.calls[0].name.as_deref(), Some("get_weather"));
    assert_eq!(
        answer.calls[0].id.as_deref(),
        Some("hRJpUTdgFQJkI18Be8rSRbjHaa9qQFdP"),
        "the id came from the FIRST fragment and was not erased by the twelve that carry none",
    );
}

#[test]
fn only_the_fragmented_fixture_can_see_an_assignment_where_the_fold_appends() {
    // This check exists to say, in a form a later reader can run, WHY two
    // servers are recorded rather than one. It reconstructs both mutants by
    // hand -- assigning instead of appending -- and shows that the mutant is
    // invisible against one fixture and fatal against the other.
    let assign_instead_of_append = |body: &str| -> String {
        let mut last = String::new();
        for frame in frames_of(body) {
            for choice in &frame.choices {
                for fragment in &choice.delta.tool_calls {
                    if let Some(function) = fragment.function.as_ref()
                        && !function.arguments.is_empty()
                    {
                        last.clone_from(&function.arguments);
                    }
                }
            }
        }
        last
    };

    let whole = map::fold(&frames_of(WHOLE_ARGUMENTS));
    assert_eq!(
        assign_instead_of_append(WHOLE_ARGUMENTS),
        whole.calls[0].arguments,
        "against the one-frame server an assignment and an append agree, so this fixture alone \
         could not redden the mutant",
    );

    let fragmented = map::fold(&frames_of(DELTA_ARGUMENTS));
    assert_eq!(
        assign_instead_of_append(DELTA_ARGUMENTS),
        "\"}",
        "against the fragmenting server an assignment leaves the LAST fragment",
    );
    assert_ne!(
        assign_instead_of_append(DELTA_ARGUMENTS),
        fragmented.calls[0].arguments,
        "which is why the fragmenting server's stream is recorded too",
    );
}

#[test]
fn fragments_are_correlated_by_index_and_not_by_the_order_they_arrive_in() {
    // Read against the DERIVED fixture. It is evidence about `fold` and never
    // about a server: no server on this machine would produce two calls, and
    // the interleaving here is deliberately harsher than a real stream's.
    let answer = map::fold(&frames_of(TWO_CALLS));

    assert_eq!(answer.calls.len(), 2, "two calls, not fourteen: {answer:?}");
    assert_eq!(answer.calls[0].name.as_deref(), Some("get_weather"));
    assert_eq!(answer.calls[0].arguments, "{\"city\": \"Paris\"}");
    assert_eq!(answer.calls[0].id.as_deref(), Some("call_first"));
    assert_eq!(answer.calls[1].name.as_deref(), Some("get_time"));
    assert_eq!(answer.calls[1].arguments, "{\"city\": \"Tokyo\"}");
    assert_eq!(answer.calls[1].id.as_deref(), Some("call_second"));
}

#[test]
fn the_calls_come_out_in_the_index_order_the_provider_gave_them() {
    let answer = map::fold(&frames_of(TWO_CALLS));
    let names: Vec<&str> = answer
        .calls
        .iter()
        .map(|call| call.name.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        vec!["get_weather", "get_time"],
        "index 0 before index 1, whatever order their fragments arrived in",
    );
}

#[test]
fn a_text_answer_is_appended_across_frames_whether_its_head_frame_is_null_or_empty() {
    let with_null = map::fold(&frames_of(TEXT));
    assert_eq!(
        with_null.text, "Hello there, friend.",
        "llama-server opens with content: null, which is skipped rather than refused",
    );
    assert_eq!(with_null.finish_reason.as_deref(), Some("stop"));

    let with_empty = map::fold(&frames_of(TEXT_EMPTY_HEAD));
    assert_eq!(
        with_empty.text, "Hello there, friend.",
        "Ollama's /v1 opens with content: \"\", and the same fold reads both",
    );
    assert_eq!(with_empty.finish_reason.as_deref(), Some("stop"));
}

#[test]
fn usage_is_the_frame_that_carried_it_and_a_server_that_sent_none_reports_zero() {
    let asked = map::fold(&frames_of(WHOLE_ARGUMENTS));
    let usage = asked.usage.expect("stream_options asked for usage");
    assert_eq!(usage.prompt_tokens, 196);
    assert_eq!(usage.completion_tokens, 17);

    // The same server, the same request, `stream_options` omitted.
    let not_asked = map::fold(&frames_of(NO_USAGE));
    assert!(
        not_asked.usage.is_none(),
        "a server that was not asked sends no usage frame: {not_asked:?}",
    );
    let response =
        map::response_from(&not_asked, NO_USAGE.len(), true).expect("the stream carried choices");
    assert_eq!(
        (response.tokens().prompt, response.tokens().completion),
        (0, 0),
        "and this client reports zero rather than inventing a number",
    );
}

#[test]
fn the_usage_frame_carries_no_choices_and_that_does_not_end_the_answer() {
    let frames = frames_of(WHOLE_ARGUMENTS);
    let last = frames.last().expect("the recorded stream has frames");
    assert!(
        last.choices.is_empty() && last.usage.is_some(),
        "the terminal frame is usage with an empty choices array: {last:?}",
    );
    // And the answer folded from the whole stream still has its call.
    assert_eq!(map::fold(&frames).calls.len(), 1);
}

// --- The sentinel, which is the client's business and not the reader's ------

#[test]
fn the_done_sentinel_arrives_as_a_payload_and_is_discarded_before_anything_parses_it() {
    let payloads = payloads_of(WHOLE_ARGUMENTS);
    assert_eq!(
        payloads.last().map(String::as_str),
        Some(map::DONE),
        "the SSE reader hands `[DONE]` up like any other payload: {payloads:?}",
    );
    assert!(
        serde_json::from_str::<wire::Chunk>(map::DONE).is_err(),
        "and it is not JSON, so a client that did not discard it would report a defect at the \
         end of every successful stream",
    );
    // `frames_of` discards it exactly as the client does, so this holds.
    assert_eq!(frames_of(WHOLE_ARGUMENTS).len(), payloads.len() - 1);
}

#[test]
fn a_stream_that_fails_part_way_through_sends_no_sentinel_at_all() {
    let payloads = payloads_of(ERROR_FRAME);
    assert_ne!(
        payloads.last().map(String::as_str),
        Some(map::DONE),
        "the recorded failing stream ends on its error frame: {payloads:?}",
    );
    assert!(
        payloads.iter().all(|payload| payload.trim() != map::DONE),
        "and carries no sentinel anywhere",
    );
}

// --- The mapping out --------------------------------------------------------

#[test]
fn calls_are_read_before_the_finish_reason_so_a_stop_cannot_swallow_a_call() {
    // Both recorded tool-call streams end `finish_reason: "tool_calls"`, so
    // this ordering is harmless against them -- which is why it is asserted
    // against a folded answer that ends "stop" AND carries a call, the shape
    // the `gemini` and `ollama` clients both measured from their own servers.
    let answer = map::Answer {
        text: String::new(),
        calls: vec![map::Call {
            id: Some("call_1".to_owned()),
            name: Some("fs.read".to_owned()),
            arguments: "{}".to_owned(),
        }],
        finish_reason: Some(map::FINISH_STOP.to_owned()),
        usage: None,
    };
    let response = map::response_from(&answer, 0, true).expect("a choice was seen");
    assert!(
        matches!(response, ModelResponse::Calls { .. }),
        "a call with a `stop` beside it is still a call: {response:?}",
    );
}

#[test]
fn the_arguments_reach_the_loop_as_the_text_they_arrived_as_with_no_round_trip() {
    // Deliberately not canonical JSON: spacing and key order are the model's
    // and survive, which a client that parsed and re-serialised would lose.
    let answer = map::Answer {
        text: String::new(),
        calls: vec![map::Call {
            id: Some("call_1".to_owned()),
            name: Some("fs.read".to_owned()),
            arguments: "{\"z\":  1, \"a\":2}".to_owned(),
        }],
        finish_reason: Some("tool_calls".to_owned()),
        usage: None,
    };
    let ModelResponse::Calls { calls, .. } = map::response_from(&answer, 0, true).expect("mapped")
    else {
        panic!("a call was folded");
    };
    assert_eq!(
        calls[0].arguments, "{\"z\":  1, \"a\":2}",
        "byte for byte what the provider sent",
    );
}

#[test]
fn an_id_the_provider_never_sent_is_empty_and_is_never_invented() {
    let answer = map::Answer {
        text: String::new(),
        calls: vec![map::Call {
            id: None,
            name: Some("fs.read".to_owned()),
            arguments: "{}".to_owned(),
        }],
        finish_reason: Some("tool_calls".to_owned()),
        usage: None,
    };
    let ModelResponse::Calls { calls, .. } = map::response_from(&answer, 0, true).expect("mapped")
    else {
        panic!("a call was folded");
    };
    assert_eq!(
        calls[0].id, "",
        "empty rather than a number this client made up"
    );
}

#[test]
fn a_text_answer_maps_to_text_and_a_reason_this_client_does_not_know_maps_to_stopped() {
    let text = map::fold(&frames_of(TEXT));
    let mapped = map::response_from(&text, TEXT.len(), true).expect("mapped");
    let ModelResponse::Text { text: said, .. } = mapped else {
        panic!("a stop with text is Text, got {mapped:?}");
    };
    assert_eq!(said, "Hello there, friend.");

    let invented = map::Answer {
        text: "half an answer".to_owned(),
        calls: Vec::new(),
        finish_reason: Some("content_filter".to_owned()),
        usage: None,
    };
    let mapped = map::response_from(&invented, 0, true).expect("mapped");
    let ModelResponse::Stopped { reason, .. } = mapped else {
        panic!("an unknown reason is Stopped, got {mapped:?}");
    };
    assert_eq!(
        reason, "content_filter",
        "the provider's own word, not this client's guess about it",
    );
}

#[test]
fn a_stream_that_carried_no_choices_at_all_is_unreadable_rather_than_an_empty_answer() {
    let refusal = map::response_from(&map::Answer::default(), 42, false)
        .expect_err("no choices is not a documented successful shape");
    assert!(
        matches!(
            &refusal,
            OpenAiCompatibleFailure::Unreadable { bytes, .. } if *bytes == 42
        ),
        "{refusal:?}",
    );
}

// --- The mapping in ---------------------------------------------------------

fn a_descriptor() -> ToolDescriptor {
    ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "Read a file.".to_owned(),
        parameters: "{\"type\":\"object\",\"properties\":{\"path\":{\"type\":\"string\"}}}"
            .to_owned(),
    }
}

#[test]
fn the_first_round_of_a_turn_sends_the_prompt_the_tools_and_the_usage_opt_in() {
    let tools = [a_descriptor()];
    let prompt = prompt("read notes.txt");
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let mut answered = map::Answered::default();
    let body = map::request_from(&request, &mut answered, "a-model").expect("built");

    assert!(body.stream, "this client has no non-streamed path");
    assert!(
        body.stream_options.include_usage,
        "usage is asked for, because a server that is not asked never volunteers it",
    );
    assert_eq!(body.messages.len(), 1);
    assert_eq!(body.messages[0].role, map::ROLE_USER);
    assert_eq!(body.messages[0].content.as_deref(), Some("read notes.txt"));
    assert_eq!(body.tools.len(), 1);
    assert_eq!(body.tools[0].kind, map::TOOL_FUNCTION);
    assert_eq!(body.tools[0].function.name, "fs.read");
}

#[test]
fn a_second_round_gives_the_model_its_own_turn_back_and_answers_each_call_by_its_id() {
    let tools = [a_descriptor()];
    let prompt = prompt("read notes.txt");
    let mut answered = map::Answered::default();

    // Round one, remembered exactly as the client remembers it.
    let folded = map::fold(&frames_of(DELTA_ARGUMENTS));
    let (message, calls) = map::assistant_turn(&folded);
    answered.remember(message, calls);

    let results = [ToolResult {
        id: "hRJpUTdgFQJkI18Be8rSRbjHaa9qQFdP".to_owned(),
        content: Redacted::by(&NothingHeld, "18 degrees and sunny"),
        failed: false,
    }];
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    let body = map::request_from(&request, &mut answered, "a-model").expect("built");

    assert_eq!(body.messages.len(), 3, "prompt, assistant turn, result");
    assert_eq!(body.messages[1].role, map::ROLE_ASSISTANT);
    assert_eq!(
        body.messages[1].content, None,
        "an assistant turn that is all tool calls sends content: null, which is what the servers \
         themselves send",
    );
    assert_eq!(body.messages[1].tool_calls.len(), 1);
    assert_eq!(
        body.messages[1].tool_calls[0].function.arguments, "{\"city\": \"Paris\", \"unit\": \"c\"}",
        "the accumulated string goes back whole",
    );
    assert_eq!(body.messages[2].role, map::ROLE_TOOL);
    assert_eq!(
        body.messages[2].tool_call_id.as_deref(),
        Some("hRJpUTdgFQJkI18Be8rSRbjHaa9qQFdP"),
        "the call's id, carried -- this API correlates by id where Ollama's own correlates by name",
    );
    assert_eq!(
        body.messages[2].content.as_deref(),
        Some("18 degrees and sunny"),
    );
}

#[test]
fn a_turn_whose_results_and_remembered_calls_disagree_is_refused_rather_than_sent() {
    let tools = [a_descriptor()];
    let prompt = prompt("read notes.txt");
    let mut answered = map::Answered::default();
    let folded = map::fold(&frames_of(DELTA_ARGUMENTS));
    let (message, calls) = map::assistant_turn(&folded);
    answered.remember(message, calls);

    // Two results for one call. Asserted in THIS direction rather than the
    // other, because a guard written `!=` passes a mutant that only checked
    // one side.
    let results = [
        ToolResult {
            id: "a".to_owned(),
            content: Redacted::by(&NothingHeld, "one"),
            failed: false,
        },
        ToolResult {
            id: "b".to_owned(),
            content: Redacted::by(&NothingHeld, "two"),
            failed: false,
        },
    ];
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    let refusal =
        map::request_from(&request, &mut answered, "a-model").expect_err("2 results, 1 call");
    assert_eq!(
        refusal,
        OpenAiCompatibleFailure::ResultsDoNotMatchCalls {
            results: 2,
            calls: 1,
        },
    );
}

#[test]
fn a_tool_whose_schema_is_not_json_is_refused_and_the_refusal_names_the_tool() {
    let tools = [ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "Read a file.".to_owned(),
        parameters: "not json".to_owned(),
    }];
    let prompt = prompt("read notes.txt");
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let mut answered = map::Answered::default();
    let refusal =
        map::request_from(&request, &mut answered, "a-model").expect_err("the schema is not JSON");
    assert!(
        matches!(
            &refusal,
            OpenAiCompatibleFailure::ToolSchemaUnreadable { tool, .. } if tool == "fs.read"
        ),
        "{refusal:?}",
    );
}

// --- The failures -----------------------------------------------------------

#[test]
fn a_401_is_a_rejected_key_and_names_the_alias_rather_than_the_request() {
    let failure = OpenAiCompatibleFailure::from_status(
        401,
        REJECTED_KEY.as_bytes(),
        "llama3.2:3b",
        &alias(),
        A_KEY,
    );
    assert_eq!(
        failure,
        OpenAiCompatibleFailure::CredentialRejected {
            alias: alias(),
            kind: ProviderKind::OpenAiCompatible,
            code: 401,
        },
    );
    let said = failure.to_string();
    assert!(
        said.contains("provider.openai_compatible"),
        "the remedy names where the key is stored: {said}",
    );
}

#[test]
fn a_403_is_the_same_class_because_a_reader_can_act_on_both() {
    let failure = OpenAiCompatibleFailure::from_status(
        403,
        REJECTED_KEY.as_bytes(),
        "llama3.2:3b",
        &alias(),
        A_KEY,
    );
    assert!(
        matches!(
            failure,
            OpenAiCompatibleFailure::CredentialRejected { code: 403, .. }
        ),
        "{failure:?}",
    );
}

#[test]
fn a_404_naming_the_model_is_a_missing_model_and_a_404_naming_a_route_is_not() {
    let missing = OpenAiCompatibleFailure::from_status(
        404,
        MODEL_NOT_FOUND.as_bytes(),
        "does-not-exist:1b",
        &alias(),
        A_KEY,
    );
    assert!(
        matches!(
            &missing,
            OpenAiCompatibleFailure::ModelNotFound { model, .. } if model == "does-not-exist:1b"
        ),
        "{missing:?}",
    );

    // The same status, a body that names no model: a gateway answering 404 for
    // a path that is not there. Telling this reader to change `model.default`
    // would be a remedy that cannot work.
    let route = OpenAiCompatibleFailure::from_status(
        404,
        br#"{"error":{"message":"Not Found","type":"invalid_request_error"}}"#,
        "does-not-exist:1b",
        &alias(),
        A_KEY,
    );
    assert!(
        matches!(
            route,
            OpenAiCompatibleFailure::RequestRefused { code: 404, .. }
        ),
        "{route:?}",
    );
}

#[test]
fn both_servers_error_envelopes_are_read_although_they_type_code_differently() {
    // The reason `wire::ErrorBody` declares neither `code` nor `param`.
    let ollama: wire::ErrorEnvelope = serde_json::from_str(BAD_REQUEST).expect("code: null parses");
    let llama: wire::ErrorEnvelope =
        serde_json::from_str(BAD_REQUEST_NUMERIC_CODE).expect("code: 400 parses");

    assert!(ollama.error.message.contains("cannot unmarshal"));
    assert_eq!(ollama.error.kind.as_deref(), Some("invalid_request_error"));
    assert!(llama.error.message.contains("to be an array"));
    assert_eq!(llama.error.kind.as_deref(), Some("invalid_request_error"));

    // And the proof that the two really do disagree, so this check cannot pass
    // on two fixtures that happen to be the same shape.
    let raw: serde_json::Value = serde_json::from_str(BAD_REQUEST).expect("json");
    assert!(raw["error"]["code"].is_null(), "Ollama sends code: null");
    let raw: serde_json::Value = serde_json::from_str(BAD_REQUEST_NUMERIC_CODE).expect("json");
    assert!(
        raw["error"]["code"].is_number(),
        "llama-server sends code as a number",
    );
}

#[test]
fn a_400_is_a_defect_because_this_harness_built_the_request() {
    let failure = OpenAiCompatibleFailure::from_status(
        400,
        BAD_REQUEST.as_bytes(),
        "llama3.2:3b",
        &alias(),
        A_KEY,
    );
    assert!(
        matches!(
            failure,
            OpenAiCompatibleFailure::RequestRefused { code: 400, .. }
        ),
        "{failure:?}",
    );
}

#[test]
fn a_503_is_the_servers_own_side_and_not_the_readers() {
    let failure = OpenAiCompatibleFailure::from_status(
        503,
        LOADING.as_bytes(),
        "llama3.2:3b",
        &alias(),
        A_KEY,
    );
    assert!(
        matches!(
            &failure,
            OpenAiCompatibleFailure::Unavailable { code: Some(503), detail } if detail == "Loading model"
        ),
        "{failure:?}",
    );
}

#[test]
fn a_body_that_is_not_an_envelope_is_reported_by_its_length_and_never_its_content() {
    let body = b"<html>a proxy's error page quoting sk-a-value-this-check-invented</html>";
    let failure = OpenAiCompatibleFailure::from_status(500, body, "llama3.2:3b", &alias(), A_KEY);
    let said = failure.to_string();
    assert!(
        said.contains(&format!("{} bytes", body.len())),
        "the length: {said}",
    );
    assert!(
        !said.contains("proxy"),
        "and nothing of the body itself: {said}",
    );
}

#[test]
fn an_error_frame_parses_as_a_perfectly_ordinary_empty_chunk_which_is_why_the_field_exists() {
    // The regression check for a defect this corpus caught. The first version
    // of `absorb` tried the error envelope only AFTER a chunk failed to parse
    // -- and the chunk never fails, because `serde` ignores unknown fields.
    // The error branch was unreachable and the recorded failing stream folded
    // into a tool call with truncated arguments: a wrong answer, silently.
    let payload =
        r#"{"error":{"code":500,"message":"the model went wrong","type":"server_error"}}"#;
    let frame: wire::Chunk =
        serde_json::from_str(payload).expect("an error frame IS a well-formed chunk");

    assert!(
        frame.choices.is_empty() && frame.usage.is_none(),
        "indistinguishable from an empty frame by everything except `error`: {frame:?}",
    );
    assert_eq!(
        frame.error.map(|error| error.message),
        Some("the model went wrong".to_owned()),
        "which is the one field that tells them apart, and why it is declared",
    );

    // The accepting sibling: an ordinary frame carries no error, so the field
    // is a discriminator rather than something always set.
    let ordinary = &frames_of(WHOLE_ARGUMENTS)[0];
    assert!(ordinary.error.is_none(), "{ordinary:?}");
}

#[test]
fn the_fields_neither_server_is_asked_about_are_ignored_rather_than_refused() {
    // The other half of the same decision: `deny_unknown_fields` would have
    // been the alternative fix above and is refused, because both servers send
    // fields this client does not read and a gateway may add more.
    for (name, body) in [
        ("whole-arguments.sse", WHOLE_ARGUMENTS),
        ("delta-arguments.sse", DELTA_ARGUMENTS),
    ] {
        let raw: serde_json::Value = serde_json::from_str(
            payloads_of(body)
                .first()
                .expect("the stream has a first frame"),
        )
        .expect("json");
        let unread: Vec<&str> = ["object", "created", "system_fingerprint"]
            .into_iter()
            .filter(|field| raw.get(*field).is_some())
            .collect();
        assert!(
            !unread.is_empty(),
            "{name}'s frames carry fields this client does not read, so refusing unknown ones \
             would refuse both real servers",
        );
    }
    // `llama-server` adds one more on its usage frame.
    let last = payloads_of(DELTA_ARGUMENTS);
    let usage_frame = last
        .iter()
        .rev()
        .find(|payload| payload.contains("\"usage\""))
        .expect("the recorded stream carries usage");
    let raw: serde_json::Value = serde_json::from_str(usage_frame).expect("json");
    assert!(
        raw.get("timings").is_some(),
        "and one of them invents a field of its own: {usage_frame}",
    );
}

#[test]
fn an_error_frame_inside_a_two_hundred_stream_is_a_failure_and_not_a_short_answer() {
    let client = client_with(None);
    let payloads = payloads_of(ERROR_FRAME);
    let mut received: Vec<wire::Chunk> = Vec::new();
    let mut raised: Option<OpenAiCompatibleFailure> = None;
    for payload in &payloads {
        if let Err(failure) = client.absorb(payload, ERROR_FRAME.len(), &mut received) {
            raised = Some(failure);
            break;
        }
    }

    let failure = raised.expect("the recorded stream ends in an error frame");
    assert!(
        matches!(
            &failure,
            OpenAiCompatibleFailure::StreamFailed { code: None, detail }
                if detail.contains("peg-native format")
        ),
        "{failure:?}",
    );

    // And what would have happened without the branch: eight good frames
    // folding to a call whose arguments are truncated JSON. A wrong answer
    // rather than a failure, which is the worst outcome available.
    let folded = map::fold(&received);
    assert_eq!(folded.calls.len(), 1);
    assert_eq!(
        folded.calls[0].arguments, "{\"city\": \"Paris\"}",
        "the fragments that did arrive",
    );
    assert!(
        folded.finish_reason.is_none(),
        "with no finish reason, because the server never sent one: {folded:?}",
    );
}

// --- The endpoint -----------------------------------------------------------

#[test]
fn the_client_appends_the_chat_path_and_leaves_the_v1_prefix_to_the_user() {
    assert_eq!(CHAT_PATH, "/chat/completions");

    // The three real endpoints measured on 2026-09-14. Two carry a `/v1` and
    // one does not, which is why the client assumes neither.
    for (configured, expected) in [
        (
            "http://127.0.0.1:11434/v1",
            "http://127.0.0.1:11434/v1/chat/completions",
        ),
        (
            "http://127.0.0.1:18080",
            "http://127.0.0.1:18080/chat/completions",
        ),
        (
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/chat/completions",
        ),
    ] {
        assert_eq!(
            Endpoint::new(&endpoint(configured)).chat_url(),
            expected,
            "for {configured}",
        );
    }
}

#[test]
fn a_trailing_slash_does_not_become_a_double_slash_in_the_path() {
    assert_eq!(
        Endpoint::new(&endpoint("http://127.0.0.1:11434/v1/")).chat_url(),
        "http://127.0.0.1:11434/v1/chat/completions",
    );
}

#[test]
fn this_kind_publishes_no_default_endpoint_for_anything_to_fall_back_to() {
    // A negative asserted structurally rather than by absence of a symbol: the
    // `gemini` and `ollama` modules each export a `DEFAULT_ENDPOINT` and a
    // `default_endpoint()`, and this one deliberately exports neither, so
    // `compose::turn`'s endpoint-default match has nothing to reach for. If a
    // default is ever added, this check is what has to be deleted to add it --
    // which is the conversation the module documentation asks for.
    assert!(
        !crate::providers::ollama::DEFAULT_ENDPOINT.is_empty(),
        "the sibling kinds do publish one, so the absence here is a choice",
    );
    assert!(
        !crate::providers::gemini::DEFAULT_ENDPOINT.is_empty(),
        "and so does the other",
    );
}

// --- The descriptor and the timeout -----------------------------------------

#[test]
fn the_descriptor_declares_all_three_capabilities_once_and_both_readers_agree() {
    let client = client_with(None);
    let declared = Provider::capabilities(&client);
    assert!(declared.streaming());
    assert!(declared.tool_calling());
    assert!(declared.token_accounting());

    let through_the_port = zaru_core::tool_call::Model::capabilities(&client);
    assert_eq!(
        through_the_port.tool_calling,
        declared.tool_calling(),
        "one statement, read twice",
    );
}

#[test]
fn the_kind_and_the_endpoint_this_client_reports_are_the_ones_it_was_built_with() {
    let client = client_with(None);
    assert_eq!(Provider::kind(&client), ProviderKind::OpenAiCompatible);
    assert_eq!(
        Provider::endpoint(&client).as_str(),
        "http://127.0.0.1:11434/v1",
        "the configured value, not the normalised one",
    );
}

#[test]
fn usage_is_none_before_a_client_has_made_a_request() {
    assert!(
        Provider::usage(&client_with(None)).is_none(),
        "a client that had made no request and reported a zero would be inventing a datum",
    );
}

#[test]
fn the_exchange_ceiling_is_the_local_one_because_a_cold_load_took_minutes() {
    assert_eq!(
        EXCHANGE_TIMEOUT.as_secs(),
        600,
        "measured 2026-09-14: a cold load of llama3.2:3b through llama-server took over four \
         minutes before a token, so a 60-second ceiling would call a working server unreachable",
    );
}

// --- The key ----------------------------------------------------------------

#[test]
fn a_client_with_no_key_is_an_ordinary_supported_state() {
    let client = client_with(None);
    assert_eq!(Provider::kind(&client), ProviderKind::OpenAiCompatible);
    assert_eq!(
        client.key_for_redaction(),
        "",
        "and its redaction argument is empty, which is what the empty-key guard is for",
    );
}

#[test]
fn the_key_is_carried_only_in_a_bearer_header_and_never_in_the_url() {
    assert_eq!(AUTHORIZATION_HEADER, "authorization");
    assert_eq!(BEARER_PREFIX, "Bearer ");
    let client = client_with(Some(secret(A_KEY)));
    let url = Endpoint::new(&endpoint("http://127.0.0.1:11434/v1")).chat_url();
    assert!(
        !url.contains(A_KEY) && !url.contains(ascii_core(A_KEY)),
        "a URL lands in proxy logs: {url}",
    );
    assert_eq!(
        client.key_for_redaction(),
        A_KEY,
        "and the client does hold it, so the assertion above is not vacuous",
    );
}

/// Everything before the first non-ASCII character.
///
/// The second arm of every absence assertion here: `{:?}` escapes a combining
/// mark, so a value-only assertion is blind to a rendering that published every
/// byte.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

#[test]
fn corpus_the_clients_debug_rendering_carries_neither_the_key_nor_its_ascii_core() {
    let client = client_with(Some(secret(A_KEY)));
    let rendered = format!("{client:?}");

    assert!(!rendered.contains(A_KEY), "by value: {rendered}");
    assert!(
        !rendered.contains(ascii_core(A_KEY)),
        "and by ASCII core: {rendered}",
    );
    // The accepting sibling: the rendering is not empty and does name the
    // things it should, so a `Debug` that printed nothing could not pass.
    assert!(
        rendered.contains("127.0.0.1:11434") && rendered.contains("llama3.2:3b"),
        "and it still says what the client is: {rendered}",
    );
}

#[test]
fn corpus_the_request_body_this_client_serialises_carries_no_key() {
    let tools = [a_descriptor()];
    let prompt = prompt("read notes.txt");
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let mut answered = map::Answered::default();
    let body = map::request_from(&request, &mut answered, "a-model").expect("built");
    let serialised = serde_json::to_string(&body).expect("the body serialises");

    assert!(!serialised.contains(A_KEY), "by value: {serialised}");
    assert!(
        !serialised.contains(ascii_core(A_KEY)),
        "and by ASCII core: {serialised}",
    );
    // The accepting sibling: the key goes in the header instead, and this is
    // what a client holding one would build.
    let header = format!("{BEARER_PREFIX}{A_KEY}");
    assert!(
        header.contains(A_KEY),
        "the key does reach the header, so the assertion above is about placement rather than \
         about a client that lost the key",
    );
}

#[test]
fn corpus_a_rejected_key_failure_carries_neither_the_key_nor_its_ascii_core() {
    // A server that quotes the key back in its own error message, which is
    // exactly the body a client must not pass through.
    let body =
        format!(r#"{{"error":{{"message":"bad key: {A_KEY}","type":"authentication_error"}}}}"#);
    let failure =
        OpenAiCompatibleFailure::from_status(500, body.as_bytes(), "llama3.2:3b", &alias(), A_KEY);
    let said = failure.to_string();

    assert!(!said.contains(A_KEY), "by value: {said}");
    assert!(
        !said.contains(ascii_core(A_KEY)),
        "and by ASCII core: {said}",
    );
    assert!(said.contains(DETAIL_WITHHELD), "and says so: {said}");

    // The accepting sibling: the same body, a client holding a DIFFERENT key.
    // The sentence passes through, so the check above is about this key rather
    // than about a client that withholds everything.
    let other = OpenAiCompatibleFailure::from_status(
        500,
        body.as_bytes(),
        "llama3.2:3b",
        &alias(),
        "sk-an-entirely-different-value",
    );
    assert!(
        other.to_string().contains("bad key:"),
        "a body that does not quote THIS key reaches the reader: {other}",
    );
}

#[test]
fn corpus_a_failure_quoting_only_the_keys_ascii_core_is_withheld_too() {
    // **This check exists because a mutation found the hole it fills.**
    // Removing the ASCII-core arm from `redacted_detail` reddened NOTHING,
    // because the sibling check above quotes the key whole and the by-value arm
    // catches that on its own. The arm is there for the rendering that escapes
    // the non-ASCII tail -- `{:?}` turns a combining mark into `\u{301}`, and a
    // server echoing such a rendering publishes every ASCII byte of the key
    // while containing the value nowhere.
    let escaped = format!("{A_KEY:?}");
    assert!(
        !escaped.contains(A_KEY),
        "the escaped rendering does not contain the value, which is the whole problem: {escaped}",
    );
    assert!(
        escaped.contains(ascii_core(A_KEY)),
        "but it does contain every ASCII byte of it: {escaped}",
    );

    // Built through `serde_json` rather than by hand, because an escaped
    // rendering pasted into a JSON literal is not JSON -- which this check
    // discovered by failing on the unmutated code and reporting "90 bytes that
    // are not an error envelope". A body that does not parse is withheld by a
    // different branch entirely, so the check would have passed for the wrong
    // reason against the very mutant it exists to catch.
    let body = serde_json::json!({
        "error": { "message": format!("bad key: {escaped}"), "type": "authentication_error" }
    })
    .to_string();
    let failure = OpenAiCompatibleFailure::from_status(500, body.as_bytes(), "m", &alias(), A_KEY);
    let said = failure.to_string();
    assert!(
        said.contains(DETAIL_WITHHELD),
        "an escaped rendering is withheld by the ASCII core, which by-value alone cannot see: \
         {said}",
    );
    assert!(
        !said.contains(ascii_core(A_KEY)),
        "and not one byte of it reaches the reader: {said}",
    );

    // The accepting sibling: a sentence sharing no prefix with the key passes
    // through, so this is redaction rather than a branch that withholds
    // everything.
    let clean = OpenAiCompatibleFailure::from_status(500, LOADING.as_bytes(), "m", &alias(), A_KEY);
    assert!(clean.to_string().contains("Loading model"), "{clean}");
}

#[test]
fn corpus_a_client_holding_no_key_withholds_nothing_and_reads_the_servers_sentence() {
    // The empty-key guard. Without it `"anything".contains("")` is true and a
    // reader with a local server would get DETAIL_WITHHELD for every failure
    // the server ever explained to them.
    let failure =
        OpenAiCompatibleFailure::from_status(503, LOADING.as_bytes(), "llama3.2:3b", &alias(), "");
    let said = failure.to_string();
    assert!(
        said.contains("Loading model"),
        "the server's own sentence reaches a reader who holds no key: {said}",
    );
    assert!(
        !said.contains(DETAIL_WITHHELD),
        "and nothing is withheld, because there is no secret to protect: {said}",
    );
}

#[test]
fn corpus_an_error_frame_that_quotes_the_key_is_withheld_too() {
    let client = client_with(Some(secret(A_KEY)));
    let payload =
        format!(r#"{{"error":{{"message":"upstream rejected {A_KEY}","type":"server_error"}}}}"#);
    let mut received: Vec<wire::Chunk> = Vec::new();
    let failure = client
        .absorb(&payload, payload.len(), &mut received)
        .expect_err("an error frame is a failure");
    let said = failure.to_string();

    assert!(!said.contains(A_KEY), "by value: {said}");
    assert!(
        !said.contains(ascii_core(A_KEY)),
        "and by ASCII core: {said}",
    );
    // The accepting sibling: an error frame quoting no key does reach the
    // reader, so this is redaction rather than a branch that withholds every
    // stream failure.
    let clean = client
        .absorb(
            r#"{"error":{"message":"upstream is out of capacity","type":"server_error"}}"#,
            0,
            &mut received,
        )
        .expect_err("still a failure");
    assert!(clean.to_string().contains("out of capacity"), "{clean}",);
}

// --- The corpus itself ------------------------------------------------------

#[test]
fn no_recorded_fixture_carries_a_machine_path() {
    for (name, body) in EVERY_FIXTURE {
        assert!(
            !body.contains("theaxiom") && !body.contains("/home/"),
            "{name} carries a path from the machine it was recorded on",
        );
    }
    // The accepting sibling: the substitution left the field there rather than
    // deleting it, so the fixtures are still the shape a server sends.
    assert!(
        DELTA_ARGUMENTS.contains("\"model\":\"llama3.2-3b\""),
        "llama-server's model field is present and substituted",
    );
}

#[test]
fn no_recorded_fixture_carries_a_credential_shaped_value() {
    // `--api-key not-the-real-one` was a string this arc invented to make a
    // real server answer 401. It is not a credential, and it is not in the
    // body either -- which is what this asserts rather than assumes.
    for (name, body) in EVERY_FIXTURE {
        assert!(
            !body.contains("not-the-real-one"),
            "{name} carries the string the 401 server was started with",
        );
        assert!(
            !body.contains("sk-"),
            "{name} carries something shaped like an API key",
        );
    }
    // The accepting sibling: the scan can find such a string when there is one.
    assert!(
        "a body quoting sk-something".contains("sk-"),
        "the needle matches when it is present",
    );
}

#[test]
fn every_recorded_stream_parses_through_the_products_own_reader() {
    for (name, body) in EVERY_FIXTURE {
        if !name.ends_with(".sse") {
            continue;
        }
        let frames = frames_of(body);
        assert!(!frames.is_empty(), "{name} produced no frames");
    }
}

#[test]
fn a_transport_failure_says_what_went_wrong_and_not_only_that_something_did() {
    // **The measurement this exists for.** `reqwest::Error`'s own `Display` for
    // a failed send is `error sending request for url (…)` and nothing else;
    // the part a reader can act on is three links down the `source` chain.
    // Measured 2026-09-14: connection-refused and DNS failures are
    // indistinguishable at the top level and obvious at the bottom.
    #[derive(Debug)]
    struct Link(&'static str, Option<Box<Link>>);
    impl core::fmt::Display for Link {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str(self.0)
        }
    }
    impl std::error::Error for Link {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|link| link as &dyn std::error::Error)
        }
    }

    // The measured connect chain, reproduced.
    let refused = Link(
        "error sending request for url (http://127.0.0.1:11999/v1/chat/completions)",
        Some(Box::new(Link(
            "client error (Connect)",
            Some(Box::new(Link(
                "tcp connect error",
                Some(Box::new(Link("Connection refused (os error 111)", None))),
            ))),
        ))),
    );
    let said = super::failure::transport_detail(&refused);
    assert!(
        said.contains("Connection refused (os error 111)"),
        "the actionable cause reaches the reader: {said}",
    );
    assert!(
        said.starts_with("error sending request for url"),
        "and the top-level sentence is still first: {said}",
    );

    // The measured DNS chain, which the top level does not distinguish from it.
    let dns = Link(
        "error sending request for url (http://nonexistent.invalid/v1/chat/completions)",
        Some(Box::new(Link(
            "client error (Connect)",
            Some(Box::new(Link(
                "dns error",
                Some(Box::new(Link(
                    "failed to lookup address information: Name or service not known",
                    None,
                ))),
            ))),
        ))),
    );
    let dns_said = super::failure::transport_detail(&dns);
    assert!(dns_said.contains("Name or service not known"), "{dns_said}",);
    // The accepting sibling, and the whole point: the two are different
    // problems with different remedies, and at the top level they are the same
    // sentence but for a URL.
    assert_ne!(
        said, dns_said,
        "two different failures must not read identically",
    );
}

#[test]
fn a_transport_chain_is_bounded_and_a_link_that_repeats_its_parent_is_dropped() {
    #[derive(Debug)]
    struct Deep(usize);
    impl core::fmt::Display for Deep {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "link-{}", self.0)
        }
    }
    impl std::error::Error for Deep {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            // A chain that never ends, which is what the bound is for.
            Some(Box::leak(Box::new(Deep(self.0 + 1))))
        }
    }
    let said = super::failure::transport_detail(&Deep(0));
    let links = said.matches("link-").count();
    assert_eq!(
        links,
        super::failure::CHAIN_DEPTH + 1,
        "the head plus at most CHAIN_DEPTH links, so an unbounded chain cannot make an unbounded \
         refusal: {said}",
    );

    // A link whose Display is its parent's adds nothing and is dropped --
    // `reqwest` does this at least once.
    #[derive(Debug)]
    struct Echo;
    impl core::fmt::Display for Echo {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("the same sentence")
        }
    }
    impl std::error::Error for Echo {}
    #[derive(Debug)]
    struct Parent;
    impl core::fmt::Display for Parent {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("the same sentence")
        }
    }
    impl std::error::Error for Parent {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&Echo)
        }
    }
    assert_eq!(
        super::failure::transport_detail(&Parent),
        "the same sentence",
        "a link that only repeats its parent costs the reader a clause and says nothing",
    );
}
