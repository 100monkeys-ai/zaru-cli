// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The framing, the mapping both ways, and the failure classes, with no socket
//! anywhere.
//!
//! Every check here is a pure function over a value. [Testing] forbids a check
//! calling a provider, and the CI runner has no Ollama — so the framing and
//! the mapping are exercised offline or they are not exercised at all.
//!
//! # What the fixtures are
//!
//! Bytes recorded from a **real** Ollama on 2026-09-14 — v0.34.0 serving
//! `llama3.2:3b` — and committed verbatim. Not one byte was edited, and
//! nothing was scrubbed because there was nothing to scrub: this kind sends no
//! credential, so a recorded exchange cannot contain one.
//! [`no_recorded_fixture_carries_a_credential`] asserts that rather than
//! trusting it.
//!
//! The real Ollama an arc installs is a **real provider**, not a fake of one,
//! which is the ruling in force: the mock the testing page refuses is a
//! loopback listener serving a provider's responses, and there is none here.
//! The live run is the artefact.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use super::endpoint::{CHAT_PATH, DEFAULT_ENDPOINT, Endpoint, NUM_THREAD};
use super::failure::OllamaFailure;
use super::wire;
use super::{map, stream};
use crate::providers::ProviderKind;
use crate::providers::endpoint::ProviderEndpoint;
use crate::providers::port::Provider;
use crate::providers::resolution::{ModelId, ModelTable};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::Redacted;
use zaru_core::tool_call::{ModelRequest, ModelResponse, ToolDescriptor, ToolResult};

/// The recorded tool-call exchange: a call in one frame, its reason in another.
const RECORDED_CALLS: &str = include_str!("recorded/calls.ndjson");
/// The recorded text answer, six frames of deltas.
const RECORDED_TEXT: &str = include_str!("recorded/text.ndjson");
/// The recorded second round: a tool result answered in words.
const RECORDED_ANSWERED: &str = include_str!("recorded/answered.ndjson");
/// The recorded call against the harness's own seven built-in descriptors.
const RECORDED_SEVEN: &str = include_str!("recorded/seven-tools.ndjson");
/// The recorded refusal of a model the server does not have.
const RECORDED_NOT_FOUND: &str = include_str!("recorded/model-not-found.json");
/// The recorded refusal of a malformed request body.
const RECORDED_BAD_REQUEST: &str = include_str!("recorded/bad-request.json");

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
    // get one is the door the product uses: stage a configuration and read the
    // alias back out of it, exactly as the binary does.
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

/// Every frame of a recorded body, parsed.
fn frames_of(body: &str) -> Vec<wire::Response> {
    let mut frames = stream::Frames::new();
    let mut out: Vec<wire::Response> = Vec::new();
    for payload in frames.feed(body.as_bytes()) {
        out.push(serde_json::from_str(&payload).expect("a recorded frame is JSON"));
    }
    if let Some(payload) = frames.finish() {
        out.push(serde_json::from_str(&payload).expect("a recorded trailing frame is JSON"));
    }
    out
}

fn client() -> super::OllamaClient {
    super::OllamaClient::new(
        Endpoint::default_endpoint(),
        model("llama3.2:3b"),
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("an HTTP client builds without touching the network")
}

// --- The framing ------------------------------------------------------------

// The whole reason this reader exists rather than the SSE one being reused.
#[test]
fn the_recorded_bodies_are_newline_delimited_and_not_server_sent_events() {
    // An SSE reader looks for `data:` fields and a blank-line terminator.
    // Neither appears in a recorded body, so the two framings are not
    // interchangeable and sharing a reader was never available.
    for body in [RECORDED_CALLS, RECORDED_TEXT, RECORDED_ANSWERED] {
        assert!(
            !body.contains("data:"),
            "a recorded body carries an SSE field name, which would mean the framing is not \
             NDJSON after all: {body}"
        );
        assert!(
            !body.contains("\n\n"),
            "a recorded body carries a blank line, which an NDJSON reader would take as an empty \
             frame: {body}"
        );
    }
}

#[test]
fn a_frame_split_across_reads_is_reassembled() {
    let body = RECORDED_CALLS.as_bytes();
    let cut = body.len() / 3;
    let mut frames = stream::Frames::new();

    let first = frames.feed(&body[..cut]);
    let mut all = first;
    all.extend(frames.feed(&body[cut..]));
    if let Some(last) = frames.finish() {
        all.push(last);
    }

    assert_eq!(
        all.len(),
        2,
        "a body cut mid-frame produced {} frames rather than the two it holds; a reader that \
         treats a read as a frame works on a fast connection and fails on a slow one",
        all.len()
    );
    for frame in &all {
        serde_json::from_str::<wire::Response>(frame)
            .expect("each reassembled frame is a whole document");
    }
}

#[test]
fn two_frames_in_one_read_are_two_frames() {
    let mut frames = stream::Frames::new();
    let taken = frames.feed(RECORDED_CALLS.as_bytes());
    assert_eq!(
        taken.len(),
        2,
        "one read carrying both frames yielded {} rather than 2",
        taken.len()
    );
}

#[test]
fn a_final_frame_without_a_newline_is_not_lost() {
    // The frame carrying `done_reason` and the ENTIRE usage is the last one,
    // so a reader that requires a trailing newline loses the only frame that
    // reports what the exchange cost.
    let without = RECORDED_CALLS.trim_end_matches('\n');
    let mut frames = stream::Frames::new();
    let mut all = frames.feed(without.as_bytes());
    if let Some(last) = frames.finish() {
        all.push(last);
    }
    assert_eq!(
        all.len(),
        2,
        "a body ending without a newline yielded {} frames; the lost one is the only frame \
         carrying prompt_eval_count and eval_count",
        all.len()
    );
}

#[test]
fn a_blank_line_is_not_a_frame() {
    let mut frames = stream::Frames::new();
    let taken = frames.feed(b"\n\n\n");
    assert!(
        taken.is_empty(),
        "a blank line was taken as a frame, which hands an empty string to a JSON parser and \
         turns a producer's padding into a reported defect"
    );
}

// --- The fold ---------------------------------------------------------------

// The measurement that makes folding load-bearing rather than tidy.
#[test]
fn a_recorded_tool_call_and_its_reason_arrive_in_different_frames() {
    let frames = frames_of(RECORDED_CALLS);
    assert_eq!(frames.len(), 2, "the recorded exchange holds two frames");

    let calling = frames[0]
        .message
        .as_ref()
        .expect("the first frame carries a message");
    assert_eq!(
        calling.tool_calls.len(),
        1,
        "the first recorded frame does not carry the call"
    );
    assert!(
        frames[0].done_reason.is_none(),
        "the frame carrying the call also carried the reason, which would make folding \
         unnecessary -- and this check is the record that it is necessary"
    );
    assert_eq!(
        frames[1].done_reason.as_deref(),
        Some("stop"),
        "the second recorded frame does not carry the reason"
    );
}

#[test]
fn a_streamed_tool_call_and_its_reason_are_one_response() {
    // Mapping frame by frame would answer one question twice, as `Calls` and
    // then `Stopped`, and the second answer would win -- losing the call.
    let folded = map::fold(&frames_of(RECORDED_CALLS));
    let mapped = map::response_from(&folded, RECORDED_CALLS.len()).expect("the fold maps");

    match mapped {
        ModelResponse::Calls { calls, .. } => {
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].name, "get_weather");
            assert_eq!(
                calls[0].id, "call_b86hkyop",
                "the provider's own call id was not carried; renumbering is the failure \
                 ToolRequest::id is documented against"
            );
        }
        other => panic!(
            "the folded tool-call exchange mapped to {other:?} rather than to Calls; a client \
             that reads the reason before the calls loses the call"
        ),
    }
}

#[test]
fn a_streamed_tool_call_arrives_whole_in_one_frame() {
    // Built to the measured contract rather than to a guess: nothing
    // accumulates partial arguments, so if this ever stops holding it stops
    // loudly at the serde boundary rather than quietly truncating.
    let frames = frames_of(RECORDED_CALLS);
    let call = &frames[0].message.as_ref().expect("a message").tool_calls[0];
    assert_eq!(call.function.name, "get_weather");
    assert_eq!(
        call.function.arguments["city"], "Paris",
        "the recorded call's arguments are not whole in the frame that carried it"
    );
}

#[test]
fn the_arguments_are_an_object_rather_than_a_string() {
    // This is the measured difference between Ollama's own API and the
    // OpenAI-shaped one, and it is why `openai-compatible` stays a separate
    // kind with no client rather than being served by this module.
    let frames = frames_of(RECORDED_CALLS);
    let arguments = &frames[0].message.as_ref().expect("a message").tool_calls[0]
        .function
        .arguments;
    assert!(
        arguments.is_object(),
        "the recorded arguments are {arguments:?} rather than an object; if this API ever sends a \
         string, the mapping re-serialises a string instead of an object and the model receives \
         quoted JSON"
    );
}

#[test]
fn a_streamed_answer_is_folded_into_the_whole_text() {
    // Text is a delta to be concatenated. Replacing instead of appending
    // leaves the last frame's fragment, which here is a single full stop.
    let folded = map::fold(&frames_of(RECORDED_TEXT));
    let mapped = map::response_from(&folded, RECORDED_TEXT.len()).expect("the fold maps");

    match mapped {
        ModelResponse::Text { text, .. } => assert_eq!(
            text, "Hello there, friend.",
            "the six recorded deltas did not concatenate into the whole answer"
        ),
        other => panic!("the folded text exchange mapped to {other:?} rather than to Text"),
    }
}

#[test]
fn the_folded_usage_is_the_last_frames_and_is_not_a_sum() {
    // On this API only the terminal frame carries a count at all, so summing
    // would be summing one number -- and would silently become wrong the day
    // the server reports per-frame counts. The rule is stated as "the last
    // frame that carried any" and checked as such.
    let folded = map::fold(&frames_of(RECORDED_TEXT));
    assert_eq!(folded.prompt_eval_count, Some(31));
    assert_eq!(folded.eval_count, Some(6));

    let mapped = map::response_from(&folded, RECORDED_TEXT.len()).expect("the fold maps");
    assert_eq!(mapped.tokens().prompt, 31);
    assert_eq!(
        mapped.tokens().completion,
        6,
        "the completion count is not the terminal frame's; a sum over six frames would report 6 \
         here only by accident, since five of them carry no count"
    );
}

#[test]
fn only_the_terminal_frame_carries_usage_on_this_api() {
    // The property the fold rests on, asserted rather than assumed -- and the
    // reason this client's fold does NOT rest on the `gemini` client's reason
    // for taking the last frame's usage, which is that its counts accumulate.
    let frames = frames_of(RECORDED_TEXT);
    let (terminal, rest) = frames.split_last().expect("the recorded body has frames");
    assert!(
        terminal.prompt_eval_count.is_some() && terminal.eval_count.is_some(),
        "the terminal frame carries no usage, so nothing does"
    );
    for frame in rest {
        assert!(
            frame.prompt_eval_count.is_none() && frame.eval_count.is_none(),
            "a non-terminal frame carried usage, which means the counts may be cumulative after \
             all and the fold's stated reason is wrong: {frame:?}"
        );
    }
}

#[test]
fn a_stream_of_one_frame_folds_to_that_frame() {
    let frames = frames_of(RECORDED_CALLS);
    let one = frames.last().expect("a frame").clone();
    let folded = map::fold(std::slice::from_ref(&one));
    assert_eq!(folded.done, one.done);
    assert_eq!(folded.done_reason, one.done_reason);
    assert_eq!(folded.eval_count, one.eval_count);
}

// --- The request ------------------------------------------------------------

#[test]
fn a_model_request_becomes_ollamas_documented_chat_body() {
    let text = "read notes.txt and tell me what it says";
    let prompt = prompt(text);
    let tools = [ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "Read a file".to_owned(),
        parameters: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.to_owned(),
    }];
    let mut answered = map::Answered::default();
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let body = map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("the schema is JSON");
    let json = serde_json::to_value(&body).expect("the body serialises");

    assert_eq!(json["model"], "llama3.2:3b");
    assert_eq!(json["stream"], true, "this client has one request shape");
    assert_eq!(json["messages"][0]["role"], "user");
    assert_eq!(json["messages"][0]["content"], text);
    assert_eq!(
        json["tools"][0]["type"], "function",
        "the tool declaration is missing its type, which Ollama requires"
    );
    assert_eq!(json["tools"][0]["function"]["name"], "fs.read");
    assert_eq!(
        json["tools"][0]["function"]["parameters"]["properties"]["path"]["type"], "string",
        "the schema was not offered whole; unlike the gemini client this one narrows nothing"
    );
    assert_eq!(
        json["options"]["num_thread"], NUM_THREAD,
        "the thread bound is missing, so one exchange may take the whole machine the harness is \
         rendering on"
    );
}

#[test]
fn a_second_round_carries_the_model_turn_and_names_the_tool_that_answered() {
    let prompt = prompt("read notes.txt");
    let tools: [ToolDescriptor; 0] = [];
    let results = [ToolResult {
        id: "call_a1".to_owned(),
        content: Redacted::by(&NothingHeld, "the file says hello"),
        failed: false,
    }];
    let mut answered = map::Answered::default();
    answered.remember(
        wire::Message {
            role: map::ROLE_ASSISTANT.to_owned(),
            content: String::new(),
            tool_calls: vec![wire::ToolCall {
                id: Some("call_a1".to_owned()),
                function: wire::CalledFunction {
                    name: "fs.read".to_owned(),
                    arguments: serde_json::json!({ "path": "notes.txt" }),
                    index: Some(0),
                },
            }],
            tool_name: None,
        },
        vec![wire::ToolCall {
            id: Some("call_a1".to_owned()),
            function: wire::CalledFunction {
                name: "fs.read".to_owned(),
                arguments: serde_json::json!({ "path": "notes.txt" }),
                index: Some(0),
            },
        }],
    );

    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    let body = map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("no schema to read");
    let json = serde_json::to_value(&body).expect("the body serialises");

    assert_eq!(
        json["messages"][1]["role"], "assistant",
        "the model's own turn is missing from the history, so the result below answers a call the \
         model has no record of making: {json}"
    );
    assert_eq!(
        json["messages"][1]["tool_calls"][0]["function"]["name"],
        "fs.read"
    );
    assert_eq!(json["messages"][2]["role"], "tool");
    assert_eq!(
        json["messages"][2]["tool_name"], "fs.read",
        "the result is not named for the tool that answered; naming it for the call id is half of \
         the defect that made the gemini client re-read one file to its ceiling: {json}"
    );
    assert_eq!(json["messages"][2]["content"], "the file says hello");
}

#[test]
fn a_new_turn_forgets_what_the_last_turn_asked_for() {
    let prompt = prompt("a new task");
    let tools: [ToolDescriptor; 0] = [];
    let mut answered = map::Answered::default();
    answered.remember(
        wire::Message {
            role: map::ROLE_ASSISTANT.to_owned(),
            content: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
        },
        vec![wire::ToolCall {
            id: Some("call_old".to_owned()),
            function: wire::CalledFunction {
                name: "fs.read".to_owned(),
                arguments: serde_json::json!({}),
                index: None,
            },
        }],
    );

    // An empty `results` is the only turn-boundary signal the port gives, and
    // reading it inside `request_from` is what makes the reset structural.
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let body = map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("no schema to read");
    assert_eq!(
        body.messages.len(),
        1,
        "a new turn carried the last turn's history, so the model is told it made calls in a \
         conversation it is not in"
    );
}

#[test]
fn results_that_do_not_match_the_calls_are_refused_rather_than_paired_wrongly() {
    let prompt = prompt("a task");
    let tools: [ToolDescriptor; 0] = [];
    let results = [ToolResult {
        id: "call_a1".to_owned(),
        content: Redacted::by(&NothingHeld, "one"),
        failed: false,
    }];
    let mut answered = map::Answered::default();
    answered.remember(
        wire::Message {
            role: map::ROLE_ASSISTANT.to_owned(),
            content: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
        },
        vec![
            wire::ToolCall {
                id: Some("a".to_owned()),
                function: wire::CalledFunction {
                    name: "fs.read".to_owned(),
                    arguments: serde_json::json!({}),
                    index: None,
                },
            },
            wire::ToolCall {
                id: Some("b".to_owned()),
                function: wire::CalledFunction {
                    name: "fs.list".to_owned(),
                    arguments: serde_json::json!({}),
                    index: None,
                },
            },
        ],
    );

    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    match map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    ) {
        Err(OllamaFailure::ResultsDoNotMatchCalls { results, calls }) => {
            assert_eq!((results, calls), (1, 2));
        }
        other => panic!(
            "one result for two calls was not refused ({other:?}); pairing is by position, so \
             building past a mismatch names a result for the wrong tool"
        ),
    }
}

#[test]
fn more_results_than_calls_is_refused_too_and_only_the_count_guard_sees_it() {
    // The mirror of the check above, and it exists because a mutation showed
    // the other one could not tell this client's two guards apart. With FEWER
    // results than calls the loop runs out of results and the inner guard
    // refuses, so disabling the outer count guard changed nothing. With MORE
    // results than calls the loop never runs out, so the count guard is the
    // only thing between this and a request that silently drops a result.
    let prompt = prompt("a task");
    let tools: [ToolDescriptor; 0] = [];
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
    let mut answered = map::Answered::default();
    answered.remember(
        wire::Message {
            role: map::ROLE_ASSISTANT.to_owned(),
            content: String::new(),
            tool_calls: Vec::new(),
            tool_name: None,
        },
        vec![wire::ToolCall {
            id: Some("a".to_owned()),
            function: wire::CalledFunction {
                name: "fs.read".to_owned(),
                arguments: serde_json::json!({}),
                index: None,
            },
        }],
    );

    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &results,
    };
    match map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    ) {
        Err(OllamaFailure::ResultsDoNotMatchCalls { results, calls }) => {
            assert_eq!((results, calls), (2, 1));
        }
        other => panic!(
            "two results for one call was not refused ({other:?}); the extra result would be \
             dropped silently, and this is the direction the inner guard cannot see"
        ),
    }
}

#[test]
fn a_tool_schema_that_is_not_json_is_a_defect_and_is_named() {
    let prompt = prompt("a task");
    let tools = [ToolDescriptor {
        name: "fs.read".to_owned(),
        description: "Read a file".to_owned(),
        parameters: "{not json".to_owned(),
    }];
    let mut answered = map::Answered::default();
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    match map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    ) {
        Err(OllamaFailure::ToolSchemaUnreadable { tool, .. }) => assert_eq!(tool, "fs.read"),
        other => panic!("an unreadable tool schema was not named as a defect: {other:?}"),
    }
}

// The seven built-ins are what a real turn offers, and the recorded exchange
// is the model choosing one of them by the harness's own dotted name.
#[test]
fn the_recorded_seven_tool_exchange_chose_a_built_in_by_its_dotted_name() {
    let folded = map::fold(&frames_of(RECORDED_SEVEN));
    match map::response_from(&folded, RECORDED_SEVEN.len()).expect("the fold maps") {
        ModelResponse::Calls { calls, .. } => {
            assert_eq!(
                calls[0].name, "fs.read",
                "a dot in a tool name did not survive the round trip"
            );
            assert_eq!(calls[0].arguments, r#"{"path":"notes.txt"}"#);
        }
        other => panic!("the recorded seven-tool exchange mapped to {other:?}"),
    }
}

#[test]
fn the_recorded_second_round_maps_to_the_answer_in_words() {
    let folded = map::fold(&frames_of(RECORDED_ANSWERED));
    match map::response_from(&folded, RECORDED_ANSWERED.len()).expect("the fold maps") {
        ModelResponse::Text { text, .. } => assert_eq!(
            text, "The current weather in Paris is 18C and raining.",
            "the recorded answer to a tool result did not fold into its whole text"
        ),
        other => panic!("the recorded second round mapped to {other:?} rather than to Text"),
    }
}

// --- The failure classes ----------------------------------------------------

#[test]
fn a_model_the_server_does_not_have_is_the_users_and_names_the_remedy() {
    // ADR-0016 D1 row 2's "bad config". There is no `gemini` counterpart: a
    // key is rejected there before a model name is considered, so for a
    // keyless kind this is the first thing a misconfigured machine meets.
    let failure =
        OllamaFailure::from_status(404, RECORDED_NOT_FOUND.as_bytes(), "no-such-model:1b");
    match &failure {
        OllamaFailure::ModelNotFound { model, .. } => assert_eq!(model, "no-such-model:1b"),
        other => panic!("a 404 was not read as a missing model: {other:?}"),
    }
    let said = failure.to_string();
    assert!(
        said.contains("ollama pull") && said.contains("model.default"),
        "the refusal does not name a remedy, which ADR-0016 D2 requires of the user-correctable \
         class: {said}"
    );
}

#[test]
fn a_refused_request_shape_is_ours_rather_than_the_users() {
    // The harness built the request, so there is nothing the reader changes.
    match OllamaFailure::from_status(400, RECORDED_BAD_REQUEST.as_bytes(), "llama3.2:3b") {
        OllamaFailure::RequestRefused { code, detail } => {
            assert_eq!(code, 400);
            assert!(
                detail.contains("cannot unmarshal"),
                "the server's own sentence was not carried, and it is what a maintainer needs: \
                 {detail}"
            );
        }
        other => panic!("a 400 was not read as a refused request shape: {other:?}"),
    }
}

#[test]
fn a_server_side_failure_is_neithers() {
    match OllamaFailure::from_status(503, b"{\"error\":\"overloaded\"}", "llama3.2:3b") {
        OllamaFailure::Unavailable { code, .. } => assert_eq!(code, 503),
        other => panic!("a 5xx was not read as unavailable: {other:?}"),
    }
}

#[test]
fn an_unreachable_local_endpoint_names_both_ways_out() {
    // ADR-0016 D1 row 2 names "unreachable endpoint" in as many words, and a
    // local server is the user's to start -- which is why this is NOT the
    // environmental class the gemini client's equivalent reaches.
    let failure = OllamaFailure::Unreachable {
        endpoint: ProviderEndpoint::new(DEFAULT_ENDPOINT).expect("well-formed"),
        detail: "connection refused".to_owned(),
    };
    let said = failure.to_string();
    assert!(
        said.contains(DEFAULT_ENDPOINT),
        "the refusal does not say which endpoint was tried: {said}"
    );
    assert!(
        said.contains("provider.ollama.endpoint"),
        "the refusal does not name the key that redirects it, so a reader whose server is \
         elsewhere is told nothing they can act on: {said}"
    );
}

#[test]
fn an_unreadable_body_reports_its_length_and_never_its_content() {
    let body = b"a body that is not the documented envelope";
    match OllamaFailure::from_status(500, body, "llama3.2:3b") {
        OllamaFailure::Unavailable { detail, .. } => {
            assert!(
                detail.contains(&body.len().to_string()),
                "the length is not reported: {detail}"
            );
            assert!(
                !detail.contains("documented envelope"),
                "the body's own bytes reached the failure sentence, which is what a reader pastes \
                 into a bug report: {detail}"
            );
        }
        other => panic!("an unparsable body was not carried by length: {other:?}"),
    }
}

// --- The endpoint and the descriptor ----------------------------------------

#[test]
fn the_url_is_the_origin_and_this_clients_own_path() {
    let endpoint = Endpoint::new(&ProviderEndpoint::new("http://box:11434").expect("well-formed"));
    assert_eq!(endpoint.chat_url(), "http://box:11434/api/chat");
}

#[test]
fn a_trailing_slash_does_not_become_a_double_slash() {
    let endpoint = Endpoint::new(&ProviderEndpoint::new("http://box:11434/").expect("well-formed"));
    assert_eq!(
        endpoint.chat_url(),
        "http://box:11434/api/chat",
        "a configured origin's trailing slash produced a double slash, which some gateways treat \
         as a different route"
    );
}

#[test]
fn the_path_is_ollamas_own_and_not_the_openai_compatible_one() {
    // Reaching Ollama through `/v1/chat/completions` would be building
    // ADR-0012 D3's OTHER kind inside this module.
    assert_eq!(CHAT_PATH, "/api/chat");
    assert!(
        !CHAT_PATH.contains("/v1/"),
        "this client points at the OpenAI-compatible surface, which is a different one of D3's \
         five kinds"
    );
}

#[test]
fn the_default_endpoint_is_the_origin_alone() {
    // D5's default is proposed as the origin and nothing else, because a path
    // would be this client's business and a model identifier is D1's.
    assert_eq!(DEFAULT_ENDPOINT, "http://localhost:11434");
    assert!(
        !DEFAULT_ENDPOINT.contains("/api"),
        "the default endpoint carries a path, so the origin a user configures and the path this \
         client composes would be two answers to where a request goes"
    );
    ProviderEndpoint::new(DEFAULT_ENDPOINT).expect("this module's own default is well-formed");
}

#[test]
fn the_descriptor_says_this_client_streams_and_the_two_readings_agree() {
    let client = client();
    let configured = Provider::capabilities(&client);
    let asked = zaru_core::tool_call::Model::capabilities(&client);
    assert!(configured.streaming());
    assert!(configured.tool_calling());
    assert!(configured.token_accounting());
    assert_eq!(
        configured.tool_calling(),
        asked.tool_calling,
        "the two capability readings disagree, which is exactly the drift the From conversion \
         exists to prevent"
    );
    assert_eq!(Provider::kind(&client), ProviderKind::Ollama);
}

#[test]
fn usage_is_none_before_the_first_exchange() {
    // A client that had made no request and reported a zero would be
    // inventing a datum, which is what `providers::usage` refuses to do.
    assert!(
        Provider::usage(&client()).is_none(),
        "a client reported usage before it had exchanged anything"
    );
}

// --- The corpus -------------------------------------------------------------

#[test]
fn no_recorded_fixture_carries_a_credential() {
    // Nothing was scrubbed from these fixtures because this kind sends no
    // credential -- a request with no authorization header of any kind
    // answers 200. That is asserted here rather than trusted, with the same
    // vocabulary the gemini client's fixtures are scanned for.
    const SHAPES: [&str; 6] = [
        "authorization",
        "bearer",
        "api_key",
        "api-key",
        "x-goog",
        "AIza",
    ];
    for (name, body) in [
        ("calls.ndjson", RECORDED_CALLS),
        ("text.ndjson", RECORDED_TEXT),
        ("answered.ndjson", RECORDED_ANSWERED),
        ("seven-tools.ndjson", RECORDED_SEVEN),
        ("model-not-found.json", RECORDED_NOT_FOUND),
        ("bad-request.json", RECORDED_BAD_REQUEST),
    ] {
        let lowered = body.to_lowercase();
        for shape in SHAPES {
            assert!(
                !lowered.contains(&shape.to_lowercase()),
                "the recorded fixture {name} carries {shape:?}, which means a credential-shaped \
                 value reached a committed file"
            );
        }
    }
}

#[test]
fn the_request_body_carries_no_field_a_credential_could_travel_in() {
    // The signature's consequence rather than a habit: `OllamaClient::new`
    // takes no secret and `wire::Request` has no field for one, so there is
    // nowhere on this path for a credential to be put.
    let prompt = prompt("a task");
    let tools: [ToolDescriptor; 0] = [];
    let mut answered = map::Answered::default();
    let request = ModelRequest {
        prompt: &prompt,
        tools: &tools,
        results: &[],
    };
    let body = map::request_from(
        &request,
        &mut answered,
        "llama3.2:3b",
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("no schema");
    let json = serde_json::to_value(&body).expect("the body serialises");
    let object = json.as_object().expect("a request body is an object");
    for field in ["key", "api_key", "apiKey", "authorization", "token"] {
        assert!(
            !object.contains_key(field),
            "the request body has a {field:?} field, so a credential has somewhere to go on a \
             path that is documented as having none"
        );
    }
}

/// The window this client declares is the window it asks the server for.
///
/// # One number, told to the server and obeyed by the harness
///
/// Ollama serves `num_ctx` tokens and silently truncates a longer prompt.
/// Measured on this machine 2026-09-14 against v0.34.0 with `llama3.2:3b`: a
/// 34,941-byte prompt sent with no `num_ctx` came back reporting 2,050 prompt
/// tokens, and the server's own log carried
/// `n_ctx_seq (4096) < n_ctx_train (131072)` and `truncating`. So a client
/// that declared a window without sending it would be compacting against a
/// number the server had never agreed to — and the reader would see a model
/// forget what they remember saying, which is the one failure ADR-0013 exists
/// to prevent.
///
/// Watched red twice. Dropping `num_ctx` from the request: *"the request
/// carries the window this client declares"*, `None` where `4096` is
/// required. Sending `DEFAULT_CONTEXT_TOKENS` instead of the configured
/// value: the body carried 4096 where the descriptor said 2000, which is the
/// two-windows-that-disagree this check exists to forbid.
#[test]
fn the_request_asks_for_exactly_the_window_the_descriptor_declares() {
    use crate::providers::{Provider, ProviderEndpoint};

    for declared in [
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
        2_000,
    ] {
        let client = crate::providers::ollama::OllamaClient::new(
            ProviderEndpoint::new("http://127.0.0.1:11434").expect("a well-formed origin"),
            model("llama3.2:3b"),
            declared,
        )
        .expect("an HTTP client builds");

        assert_eq!(
            Provider::capabilities(&client).context_tokens(),
            Some(declared),
            "the descriptor declares the window it was built with"
        );

        let prompt = prompt("say ok");
        let mut answered = map::Answered::default();
        let request = ModelRequest {
            prompt: &prompt,
            tools: &[],
            results: &[],
        };
        let body = map::request_from(&request, &mut answered, "llama3.2:3b", declared)
            .expect("there is no schema to refuse");
        let json = serde_json::to_value(&body).expect("the body serialises");
        assert_eq!(
            json["options"]["num_ctx"].as_u64(),
            Some(declared),
            "the request carries the window this client declares, or the harness and the server \
             hold two different windows and the server's is the one that truncates. The body was \
             {json}"
        );
    }
}

// --- ADR-0036: a request that outgrows the window ----------------------------

/// What this client's `/api/chat` answers when the request outgrows the
/// window, as Ollama's own source composes it: `llama-server` refuses with
/// HTTP 400 and its own envelope, and Ollama carries that body **verbatim as
/// the string** of its flat `{"error": "…"}`.
///
/// Read in `ollama/ollama` at `16b4376`: `llm/llama_server.go`'s `Chat`
/// returns `api.StatusError{StatusCode: res.StatusCode, ErrorMessage:
/// s.statusErrorMessage(bodyBytes)}`, whose message is the trimmed body; and
/// `server/routes.go`'s `streamResponse` writes an error that arrives before
/// any content as `c.JSON(status, gin.H{"error": e})`. The inner body is
/// `llama.cpp`'s at `4da6337`, `tools/server/server-context.cpp:3220` and
/// `server-common.cpp:68`. The token counts are invented; the words are the
/// server's.
const LLAMA_SERVER_CAPACITY: &str = r#"{"error":"{\"error\":{\"code\":400,\"message\":\"request (5123 tokens) exceeds the available context size (4096 tokens), try increasing it\",\"type\":\"exceed_context_size_error\",\"n_prompt_tokens\":5123,\"n_ctx\":4096}}"}"#;

// ADR-0036 D2 for this kind: a refusal naming a context or token capacity is
// the reader's, names `provider.ollama.context_tokens`, and does not claim the
// harness malfunctioned. Held where a person reads it: the body through the
// client's own `from_status`, `Surface::provider_failure` and
// `Presentation::of`, the text the binary writes and the pane paints. No
// socket and no wire fake: the body is bytes handed to the function the
// transport hands them to.
//
// The accepting sibling is the recorded malformed-request 400, which is still
// the harness's defect, so the capacity reading cannot pass by reclassifying
// every refused request.
#[test]
fn adr_0036_d2_an_ollama_capacity_refusal_names_its_cause_and_its_key() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let surface = Surface::new("0.0.0", "https://example.invalid/report");
    let shown = |code: u16, body: &str| -> Presentation {
        Presentation::of(&surface.provider_failure(
            &ProviderFailure::Ollama(OllamaFailure::from_status(
                code,
                body.as_bytes(),
                "llama3.2:3b",
            )),
            SessionEvidence::NoSessionExists,
        ))
    };

    let capacity = shown(400, LLAMA_SERVER_CAPACITY);
    println!("HTTP 400: {capacity}");
    let misses = crate::providers::capacity::fixtures::misses(
        ProviderKind::Ollama,
        &capacity,
        "exceeds the available context size (4096 tokens)",
    );
    assert!(misses.is_empty(), "{}", misses.join("\n"));
    assert_eq!(capacity.class.exit_code(), 2);

    let malformed = shown(400, RECORDED_BAD_REQUEST);
    assert_eq!(
        malformed.class,
        Class::Defect,
        "a refused request that names no capacity is still the harness's: {malformed}"
    );
}

// ADR-0036 D1 for this kind: "Before a provider sends any exchange, it
// measures the complete provider-native request … A request that cannot fit is
// refused locally before network I/O." The endpoint is a closed port, so an
// attempt to send is distinguishable from the local refusal: it would come
// back as `Unreachable`, naming the endpoint and not the window.
//
// Why this matters more for this kind than for any other: Ollama's `/api/chat`
// **truncates in silence** by default (`server/prompt.go`'s `chatPrompt` and
// `truncateNativeChatMessages` in `server/routes.go` at `16b4376`, both on
// unless the request says `"truncate": false`), dropping the oldest messages
// until the prompt fits `num_ctx` -- ADR-0036 D4's "silent history loss", on
// the server. The preflight is what stops a request reaching that code.
#[tokio::test]
async fn adr_0036_d1_an_oversized_ollama_request_is_refused_before_it_reaches_the_network() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::ProviderFailure;

    let client = super::OllamaClient::new(
        ProviderEndpoint::new("http://127.0.0.1:1").expect("a well-formed endpoint"),
        model("llama3.2:3b"),
        64,
    )
    .expect("constructing a client does not contact the endpoint");
    let prompt = prompt("a request whose body alone is larger than sixty-four bytes");
    let failure = client
        .exchange(&ModelRequest {
            prompt: &prompt,
            tools: &[],
            results: &[],
        })
        .await
        .expect_err("the locally measured request exceeds sixty-four bytes");

    let presented = Presentation::of(
        &Surface::new("0.0.0", "https://example.invalid/report").provider_failure(
            &ProviderFailure::Ollama(failure),
            SessionEvidence::NoSessionExists,
        ),
    );
    let said = presented.to_string();
    println!("{said}");
    assert_eq!(presented.class, Class::UserCorrectable, "{said}");
    // The statement is the one every kind's window refusal renders, the
    // classifier's `context_window_exceeded`; what differs by kind is the key.
    assert!(
        !said.contains("nothing answered")
            && said.contains("window allows 64")
            && said.contains(ProviderKind::Ollama.context_tokens_key().as_str()),
        "the request was not refused locally with its window and the key that sizes it: {said}"
    );
}
