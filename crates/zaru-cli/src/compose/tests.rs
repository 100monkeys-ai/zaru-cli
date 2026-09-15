// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the composition's four adapters do, checked one at a time.
//!
//! Every check here is about an adapter's own contract. What the adapters do
//! *together* is a turn, and a turn is checked from outside the crate against
//! the built binary, because a composition asserted from inside its own crate
//! is a composition nobody has been shown to reach ([Verification lessons]
//! §25).
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::compose::{ByteCounter, Records, TurnContext, context, prose};
use crate::credentials::fixtures::ScratchRoot;
use crate::redaction::HeldSecrets;
use crate::session::Record;
use zaru_core::context::{Context, ContextLimits, ContextWindow, PressureThreshold, TokenCounter};
use zaru_core::iteration::{ContextPolicy, ContextRefusal, Turn};
use zaru_core::tool_call::{Event, EventSink, TurnEnding};

/// A staged text with a multi-byte character in it.
///
/// The awkwardness is on the **encoding** axis, which is the axis the counter's
/// two candidate implementations differ on — a byte count and a character
/// count agree on every ASCII input, so an ASCII fixture cannot separate them
/// (library verification lessons §51).
const MULTIBYTE: &str = "réduire l'itération — 日本語";

/// The counter counts bytes, and a character count is a different answer.
///
/// `ByteCounter`'s whole documented property is that it over-counts against a
/// tokeniser and never under-counts, which is what makes
/// `Context::assemble`'s refusal sound. A character count would break that for
/// exactly the text this fixture carries.
///
/// Watched red by counting characters, which printed *"the counter must count
/// bytes: a character count under-reports a multi-byte text and the refusal
/// stops being sound — left: 30, right: 37"*.
#[test]
fn the_counter_counts_bytes_and_never_characters() {
    let counted = ByteCounter.count(MULTIBYTE);
    assert_eq!(
        counted,
        MULTIBYTE.len() as u64,
        "the counter must count bytes: a character count under-reports a multi-byte text and the \
         refusal stops being sound"
    );
    // The discriminating arm: on this text the two candidate rules disagree,
    // so a check that only asserted `count(text) == text.len()` against an
    // ASCII fixture would pass against both.
    assert!(
        counted > MULTIBYTE.chars().count() as u64,
        "this fixture cannot tell a byte count from a character count, so it asserts nothing: \
         {counted} against {}",
        MULTIBYTE.chars().count()
    );
}

/// A turn's `web.fetch` is the real client, under the numbers this binary
/// declares.
///
/// **This replaces `the_unbuilt_built_in_fails_as_the_work_rather_than_as_a_port`,
/// whose subject was `NoFetch` and which went with it on 2026-09-05.** What
/// that check held is not lost: that a failing retrieval is the work's failure
/// and never a `PortFailure` is now a property of the port's signature — see
/// `crate::web::ports`, where the implementation cannot return `Err` — rather
/// than of one stand-in's body, and `crate::web`'s own checks assert it over a
/// real socket.
///
/// What is worth pinning here instead is the seam this module owns: that the
/// composition hands the executor a client built from
/// [`layers::fetch_bounds`](crate::cli::layers::fetch_bounds) rather than from
/// numbers invented at the call site.
#[test]
fn a_turn_fetches_through_the_real_client_with_the_binary_s_own_bounds() {
    let bounds = crate::cli::layers::fetch_bounds();
    let client = crate::web::WebClient::new(bounds)
        .expect("this machine builds an HTTP client, or every other check here is meaningless");
    assert_eq!(
        client.bounds(),
        bounds,
        "the composition's fetch carries the binary's declared bounds and not a second set"
    );
    assert_eq!(
        bounds.body.get(),
        crate::cli::layers::FETCH_BODY_CEILING_BYTES,
        "the body ceiling is the declared constant"
    );
    assert_eq!(
        bounds.redirects.get(),
        crate::cli::layers::FETCH_REDIRECT_LIMIT,
        "the redirect limit is the declared constant"
    );
    assert_eq!(
        bounds.timeout.get(),
        crate::cli::layers::FETCH_TIMEOUT,
        "the timeout is the declared constant"
    );
}

/// The prefix carries the absence line in layer 1 and nothing in the other
/// three.
///
/// ADR-0027's decision of 2026-09-05 is that the harness assembles no layer-1
/// identity text and says so; this holds both halves, because a prefix that
/// carried a persona *and* the line would satisfy the first alone.
///
/// Watched red by leaving layer 1 empty, which printed *"ADR-0027's decision
/// is that the absence is visible rather than inferred, and the assembled
/// prefix does not say so"*.
#[test]
fn the_stable_prefix_says_that_no_persona_was_supplied() {
    use zaru_core::context::Layer;

    let prefix = context::prefix_for(None);
    assert!(
        prefix.as_str().contains(prose::NO_PERSONA),
        "ADR-0027's decision is that the absence is visible rather than inferred, and the \
         assembled prefix does not say so: {:?}",
        prefix.as_str()
    );
    assert_eq!(
        prefix.layer(Layer::SystemPromptAndPersona),
        Some(prose::NO_PERSONA)
    );
    for empty in [
        Layer::Grounding,
        Layer::RelationshipMemory,
        Layer::ProjectManifestSummary,
    ] {
        assert_eq!(
            prefix.layer(empty),
            Some(""),
            "layer {empty:?} carries something this composition did not put there"
        );
    }
    // No sentence of persona is invented: the line is about the harness, and
    // the record's own name for what is missing is in it.
    assert!(prose::NO_PERSONA.contains("no persona"));
}

/// A context that will not fit refuses with both numbers, and does not
/// compact.
///
/// ADR-0013 D7: an assembly that would exceed the window "fails as exhausted
/// with a clear reason rather than continuing on a rewritten context". The
/// clear reason is the pair, because "a reader cannot act on 'the window was
/// exceeded' without knowing by how much".
///
/// # The window admits the prefix and refuses the tail, and that is the whole
/// fixture
///
/// A window too small for the prefix *alone* cannot discriminate: under it,
/// every candidate implementation refuses, including one that drops the tail
/// and assembles what is left. Measured — a first fixture at 64 bytes let the
/// mutation "on a refusal, assemble the empty tail instead" **survive**, and
/// the fixture was restaked rather than the mutant dismissed (library
/// verification lessons §51 and §52). At 512 the prefix fits and the task does
/// not, so the two implementations give different answers.
///
/// Watched red by carrying the refusal out as `ContextRefusal::Failed`, which
/// printed *"a context that does not fit is ADR-0008 D5's own register and not
/// an error"*; and by the empty-tail fallback above, which now reddens with
/// *"a tail past the window must refuse rather than be assembled"*.
#[test]
fn a_turn_past_the_window_refuses_with_both_numbers() {
    let limits = ContextLimits::new(
        ContextWindow::new(512).expect("not zero"),
        PressureThreshold::new(256).expect("not zero"),
    )
    .expect("the threshold is below the window");
    let context = Context::opened(context::prefix_for(None), limits, 0);
    let held = HeldSecrets::none();
    let policy = TurnContext::over(&context, &held, false);

    let task = "x".repeat(4096);
    let refusal = futures_lite_block_on(policy.assemble(&Turn::Initial { task: &task }))
        .expect_err("a tail past the window must refuse rather than be assembled");

    match refusal {
        ContextRefusal::WindowExceeded { needed, window } => {
            assert_eq!(
                window, 512,
                "the window reported is not the one it was given"
            );
            // The staging is asserted rather than assumed: a window that could
            // not hold the prefix on its own would make every implementation
            // refuse, and this check would be about nothing.
            assert!(
                (context::prefix_for(None).as_str().len() as u64) < window,
                "the prefix alone does not fit this window, so the refusal says nothing about the \
                 tail: {} against {window}",
                context::prefix_for(None).as_str().len()
            );
            assert!(
                needed > window,
                "a refusal that does not exceed its own window says nothing: {needed} of {window}"
            );
        }
        ContextRefusal::Failed(failure) => {
            panic!(
                "a context that does not fit is ADR-0008 D5's own register and not an error: {failure}"
            )
        }
    }
}

/// A task that fits reaches the model with the absence line in front of it.
///
/// The accepting sibling of the refusal above: without it, an implementation
/// that refused every assembly would pass that check.
#[test]
fn a_turn_that_fits_carries_the_prefix_and_then_the_task() {
    let context = Context::opened(
        context::prefix_for(None),
        crate::cli::layers::context_limits(crate::providers::gemini::CONTEXT_WINDOW_TOKENS),
        0,
    );
    let held = HeldSecrets::none();
    let policy = TurnContext::over(&context, &held, false);

    let prompt = futures_lite_block_on(policy.assemble(&Turn::Initial {
        task: "read src/lib.rs and say what it is",
    }))
    .expect("a short task fits a megabyte window");

    assert!(
        prompt.as_str().starts_with(prose::NO_PERSONA),
        "ADR-0013 D1 puts the prefix first, so a cache has something stable to match: {:?}",
        prompt.as_str()
    );
    assert!(
        prompt
            .as_str()
            .ends_with("read src/lib.rs and say what it is")
    );
}

/// The sink writes one transcript line per event, as a `TurnLoop` record.
///
/// The count is asserted against what the check staged rather than against
/// what the sink reports about itself, and the file is read back with
/// `std::fs` rather than through the sink.
///
/// Watched red by emitting only the first event, which printed *"the sink
/// wrote 1 of the 3 events it was handed"*.
#[test]
fn every_event_the_loop_emits_becomes_one_transcript_line() {
    let scratch = ScratchRoot::new();
    let path = scratch.store_root().join("transcript.jsonl");
    std::fs::create_dir_all(path.parent().expect("the path has a parent"))
        .expect("the scratch root is writable");

    let staged = [
        Event::TurnStarted { n: 1, of: 8 },
        Event::ToolRequested {
            round: 1,
            call: 1,
            name: "fs.read".to_owned(),
        },
        Event::TurnEnded {
            n: 1,
            ending: TurnEnding::Answered,
            rounds: 2,
            elapsed: core::time::Duration::from_millis(42),
        },
    ];

    let mut sink = Records::appending_to(&path).expect("the transcript opens");
    for event in &staged {
        sink.emit(event);
    }
    assert!(
        sink.first_failure().is_none(),
        "a write failed: {:?}",
        sink.first_failure()
    );
    assert_eq!(
        sink.written(),
        staged.len(),
        "the sink wrote {} of the {} events it was handed",
        sink.written(),
        staged.len()
    );

    // Read back off the filesystem, not through the sink.
    let bytes = std::fs::read_to_string(&path).expect("the transcript is there");
    let lines: Vec<&str> = bytes.lines().collect();
    assert_eq!(lines.len(), staged.len(), "one line per event: {bytes}");
    for (line, event) in lines.iter().zip(staged.iter()) {
        let record: Record = serde_json::from_str(line).expect("every line is a Record");
        match record {
            Record::TurnLoop(written) => assert_eq!(
                &written, event,
                "the line on disk is not the event that was emitted"
            ),
            other => panic!(
                "the outer loop's events are ADR-0010 D2's fourth producer and this is {}",
                other.producer()
            ),
        }
    }
}

/// Poll a future to completion on this thread, with no runtime.
///
/// The adapters' futures never yield — `TurnContext::assemble` does its whole
/// work synchronously and hands back a future that is already resolved,
/// exactly as `ValidatorRunner`'s product implementation does. So a check can
/// poll one once rather than taking a reactor, and these checks stay free of
/// the runtime the *binary* needs for `reqwest`.
///
/// **`NoFetch` was named here too until 2026-09-05 and was the one that would
/// have stopped being true.** A real `web.fetch` awaits a socket, so its future
/// yields and this helper would panic on it — which is why `crate::web`'s own
/// checks take a `#[tokio::test]` runtime and none of them comes through here.
///
/// It panics rather than looping if a future ever does yield, because that
/// would mean an adapter had grown a real await and this helper had silently
/// stopped being appropriate for it.
pub(crate) fn futures_lite_block_on<F: core::future::Future>(future: F) -> F::Output {
    use core::task::{Context as TaskContext, Poll, Waker};

    // `Waker::noop` rather than a hand-built `RawWaker`: this crate denies
    // `unsafe_code` and `RawWaker::new` would need it. A no-op waker is enough
    // because a future that yields is refused below rather than re-polled.
    let mut task = TaskContext::from_waker(Waker::noop());
    let mut future = core::pin::pin!(future);
    match future.as_mut().poll(&mut task) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!(
            "an adapter's future yielded, so it has grown a real await and this helper is no \
             longer the right instrument for it"
        ),
    }
}

// --- ADR-0008's inner loop: the two reserved questions, as ports ------------

/// A model that answers with whatever it was staged with, and records the
/// request it was given.
///
/// The request is kept because the property under test is partly about **what
/// the generator asked for** — the seven tools a turn offers — and a check
/// that only read the answer could not see it.
struct Staged {
    answer: std::sync::Mutex<Option<zaru_core::tool_call::ModelResponse>>,
    asked: std::sync::Mutex<Vec<String>>,
}

impl Staged {
    fn answering(answer: zaru_core::tool_call::ModelResponse) -> Self {
        Self {
            answer: std::sync::Mutex::new(Some(answer)),
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn tools_it_was_offered(&self) -> Vec<String> {
        self.asked.lock().expect("not poisoned").clone()
    }
}

impl zaru_core::tool_call::Model for Staged {
    fn capabilities(&self) -> zaru_core::tool_call::Capabilities {
        zaru_core::tool_call::Capabilities { tool_calling: true }
    }

    async fn respond(
        &self,
        request: &zaru_core::tool_call::ModelRequest<'_>,
    ) -> Result<zaru_core::tool_call::ModelResponse, zaru_core::iteration::PortFailure> {
        *self.asked.lock().expect("not poisoned") =
            request.tools.iter().map(|tool| tool.name.clone()).collect();
        Ok(self
            .answer
            .lock()
            .expect("not poisoned")
            .take()
            .expect("staging: the model was asked twice and staged once"))
    }
}

fn usage() -> zaru_core::tool_call::TokenUsage {
    zaru_core::tool_call::TokenUsage {
        prompt: 11,
        completion: 7,
    }
}

fn prompt(text: &str) -> zaru_core::iteration::Prompt {
    zaru_core::iteration::Prompt::new(zaru_core::redaction::Redacted::by(
        &HeldSecrets::none(),
        text,
    ))
}

/// The generator asks with the seven tools a turn offers, and authors no prose.
///
/// This is the whole of how a candidate reaches the harness without a sentence
/// anybody wrote. ADR-0008 D1's refinement prompt is fixed in `zaru-core` and
/// `TurnContext::assemble` adds nothing to it, so the only place a "produce
/// tool calls" instruction could live is a sentence this arc invented — and
/// the provider's own function-calling contract makes one unnecessary.
///
/// Asserted against `ToolName::ALL` rather than against a literal list, so the
/// day an eighth built-in exists this check is about the eighth too.
///
/// Watched red by offering the model no tools, which printed *"the generator
/// asked with 0 tools and a turn's first exchange offers 7; a candidate that
/// is a tool call needs the same contract a turn's call uses"*.
#[tokio::test]
async fn the_generator_asks_with_the_same_tools_a_turn_offers() {
    use zaru_core::iteration::Generator as _;

    let model = Staged::answering(zaru_core::tool_call::ModelResponse::Text {
        text: "nothing to apply".to_owned(),
        tokens: usage(),
    });
    let generating = crate::compose::Generating::over(&model);
    generating
        .generate(&prompt("do the work"))
        .await
        .expect("the staged model answered");

    let offered = model.tools_it_was_offered();
    assert_eq!(
        offered.len(),
        crate::tools::ToolName::ALL.len(),
        "the generator asked with {} tools and a turn's first exchange offers {}; a candidate \
         that is a tool call needs the same contract a turn's call uses",
        offered.len(),
        crate::tools::ToolName::ALL.len()
    );
    for tool in crate::tools::ToolName::ALL {
        assert!(
            offered.iter().any(|name| name == tool.as_str()),
            "the generator did not offer {}, so a candidate could not ask for it",
            tool.as_str()
        );
    }
}

/// A candidate's text is what the model asked for, with no sentence around it.
///
/// The text is what `zaru-core` puts under `--- ATTEMPT n ---` in the next
/// refinement prompt, so the model is shown its own previous attempt. ADR-0008
/// D4 forbids paraphrase on the failure-text path and the same reasoning holds
/// one step earlier: a harness sentence wrapped around the attempt would be
/// the model reading a description of what it asked for.
///
/// The staged arguments carry a nonce so that an implementation which
/// hard-coded a plausible rendering could not produce them.
///
/// Watched red by wrapping the rendering in *"the model proposed: "*, which
/// printed *"the candidate's text carries a sentence this harness wrote:
/// \"the model proposed: fs.write ...\""*.
#[tokio::test]
async fn a_candidates_text_is_the_calls_it_asked_for_and_no_sentence_of_ours() {
    use zaru_core::iteration::Generator as _;

    let arguments = r#"{"path":"notes.txt","contents":"rehearsal 4173"}"#;
    let model = Staged::answering(zaru_core::tool_call::ModelResponse::Calls {
        calls: vec![zaru_core::tool_call::ToolRequest {
            id: "call-1".to_owned(),
            name: "fs.write".to_owned(),
            arguments: arguments.to_owned(),
        }],
        tokens: usage(),
    });
    let generating = crate::compose::Generating::over(&model);
    let generated = generating
        .generate(&prompt("write the file"))
        .await
        .expect("the staged model answered");

    let text = generated.candidate.as_ref();
    assert_eq!(
        text,
        format!("fs.write {arguments}"),
        "the candidate's text carries a sentence this harness wrote: {text:?}"
    );
    assert_eq!(
        generated.candidate.calls().len(),
        1,
        "a `Calls` answer is a candidate that proposes those calls"
    );
    assert_eq!(
        generated.tokens,
        usage().total(),
        "the candidate reports what the exchange cost, from the provider's own number"
    );
}

/// A `Text` answer is a candidate with nothing to apply, and says so by being
/// empty rather than by an error.
///
/// The degenerate arm is honest: the model proposed nothing the harness can
/// apply, the execution will say so, and the validators then report on a tree
/// nothing changed — a failing iteration, which is the loop working, and not
/// ADR-0016's error register.
///
/// Watched red by treating a `Text` answer as a port failure, which printed
/// *"a model that answered in prose is not a port failing"*.
#[tokio::test]
async fn a_prose_answer_is_a_candidate_with_nothing_to_apply() {
    use zaru_core::iteration::Generator as _;

    let said = "I would rather not — 日本語 — nonce-4173";
    let model = Staged::answering(zaru_core::tool_call::ModelResponse::Text {
        text: said.to_owned(),
        tokens: usage(),
    });
    let generating = crate::compose::Generating::over(&model);
    let generated = generating
        .generate(&prompt("write the file"))
        .await
        .expect("a model that answered in prose is not a port failing");

    assert!(
        generated.candidate.calls().is_empty(),
        "a prose answer proposes no calls, so there is nothing to apply"
    );
    assert_eq!(
        generated.candidate.as_ref(),
        said,
        "the candidate's text is what the model said, byte for byte"
    );
}

// ---------------------------------------------------------------------------
// ADR-0013 D2 — the generated summary, as a request to a provider
// ---------------------------------------------------------------------------

/// A model that records what it was asked and answers however it was staged.
struct StagedModel {
    answer: zaru_core::tool_call::ModelResponse,
    asked: std::sync::Mutex<Vec<String>>,
    tools_offered: std::sync::Mutex<Vec<usize>>,
}

impl StagedModel {
    fn answering(answer: zaru_core::tool_call::ModelResponse) -> Self {
        Self {
            answer,
            asked: std::sync::Mutex::new(Vec::new()),
            tools_offered: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn text(text: &str) -> Self {
        Self::answering(zaru_core::tool_call::ModelResponse::Text {
            text: text.to_owned(),
            tokens: zaru_core::tool_call::TokenUsage {
                prompt: 700,
                completion: 40,
            },
        })
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("no panic holds this").clone()
    }
}

impl zaru_core::tool_call::Model for StagedModel {
    fn capabilities(&self) -> zaru_core::tool_call::Capabilities {
        zaru_core::tool_call::Capabilities { tool_calling: true }
    }

    async fn respond(
        &self,
        request: &zaru_core::tool_call::ModelRequest<'_>,
    ) -> Result<zaru_core::tool_call::ModelResponse, zaru_core::iteration::PortFailure> {
        self.asked
            .lock()
            .expect("no panic holds this")
            .push(request.prompt.as_str().to_owned());
        self.tools_offered
            .lock()
            .expect("no panic holds this")
            .push(request.tools.len());
        Ok(self.answer.clone())
    }
}

/// A span of layer 6, oldest first.
fn span_of(exchanges: &[&str]) -> zaru_core::context::Span {
    zaru_core::context::Span::new(
        exchanges
            .iter()
            .map(|text| zaru_core::context::Exchange::verbatim(*text))
            .collect(),
    )
}

/// D2: "the oldest span of layer 6 is replaced by a **generated summary**".
///
/// What is sent is the record's own instruction and then the span itself,
/// oldest first and unaltered — a summariser that paraphrased the exchanges
/// on the way in would be summarising twice.
///
/// The mutant: reversing the span, which reddens the ordering assertion.
#[tokio::test]
async fn a_summarisation_sends_the_records_instruction_and_the_span_oldest_first() {
    use zaru_core::context::Summariser as _;

    let model = StagedModel::text("they agreed on four spaces and no tabs");
    let held = HeldSecrets::none();
    let summariser = crate::compose::ModelSummariser::over(&model, &held);

    let summary = summariser
        .summarise(&span_of(&["the oldest thing", "the newest thing"]))
        .await
        .expect("the staged model answers with text");

    assert_eq!(summary, "they agreed on four spaces and no tabs");
    let asked = model.asked();
    assert_eq!(asked.len(), 1, "one span is one request; got {asked:?}");
    let sent = &asked[0];
    assert!(
        sent.starts_with(prose::SUMMARISE_SPAN),
        "the instruction leads, and it is ADR-0013's own words rather than this module's: {sent:?}"
    );
    let oldest = sent.find("the oldest thing").expect("the span was sent");
    let newest = sent.find("the newest thing").expect("the span was sent");
    assert!(
        oldest < newest,
        "D2 compacts oldest first and the span arrives in that order; the request put the newest \
         at {newest} and the oldest at {oldest}"
    );
}

/// A summarisation offers no tools, and the wire layer turns that into no
/// `tools` key at all rather than an empty array.
///
/// The mutant: passing a descriptor through, which reddens the count.
#[tokio::test]
async fn a_summarisation_offers_the_model_no_tools() {
    use zaru_core::context::Summariser as _;

    let model = StagedModel::text("a summary");
    let held = HeldSecrets::none();
    let summariser = crate::compose::ModelSummariser::over(&model, &held);
    summariser
        .summarise(&span_of(&["something"]))
        .await
        .expect("the staged model answers");

    let offered = model
        .tools_offered
        .lock()
        .expect("no panic holds this")
        .clone();
    assert_eq!(
        offered,
        vec![0],
        "a summarisation is not a turn and has nothing to call, so `ModelRequest.tools` is empty \
         -- which `wire::Request` renders as no `tools` key at all"
    );
}

/// Neither of the two non-text arms becomes a summary.
///
/// An empty string here would replace a span of real conversation with
/// nothing and announce that it had summarised it. `Context::compact` obtains
/// the summary before it removes anything, so a failure leaves the context
/// exactly as it was — which is the honest outcome and is asserted from
/// outside the crate.
///
/// The mutant: returning `Ok(String::new())` on either arm, which reddens the
/// refusal assertions.
#[tokio::test]
async fn a_model_that_stops_or_asks_for_a_tool_has_not_summarised_anything() {
    use zaru_core::context::Summariser as _;

    let held = HeldSecrets::none();

    let stopped = StagedModel::answering(zaru_core::tool_call::ModelResponse::Stopped {
        reason: "MAX_TOKENS".to_owned(),
        tokens: zaru_core::tool_call::TokenUsage::default(),
    });
    let failure = crate::compose::ModelSummariser::over(&stopped, &held)
        .summarise(&span_of(&["something"]))
        .await
        .expect_err("a stop is not a summary");
    assert!(
        failure.to_string().contains("MAX_TOKENS"),
        "the provider's own word is carried rather than paraphrased: {failure}"
    );

    let calling = StagedModel::answering(zaru_core::tool_call::ModelResponse::Calls {
        calls: vec![zaru_core::tool_call::ToolRequest {
            id: "call-1".to_owned(),
            name: "fs.read".to_owned(),
            arguments: "{}".to_owned(),
        }],
        tokens: zaru_core::tool_call::TokenUsage::default(),
    });
    let failure = crate::compose::ModelSummariser::over(&calling, &held)
        .summarise(&span_of(&["something"]))
        .await
        .expect_err("a tool call is not a summary");
    assert!(
        failure.to_string().contains("offered it none"),
        "the refusal says the request offered no tools, so a reader knows the model asked for \
         something it was never given: {failure}"
    );
}

/// ADR-0012 D7: "Every request records prompt tokens, completion tokens".
///
/// A summarisation is a request, so what it spent is readable. `None` before
/// the first, because a client that had made no request and reported a zero
/// would be inventing a datum.
///
/// The mutant: not recording the usage, which reddens the `Some` assertion.
#[tokio::test]
async fn a_summarisation_reports_what_it_spent() {
    use zaru_core::context::Summariser as _;

    let model = StagedModel::text("a summary");
    let held = HeldSecrets::none();
    let summariser = crate::compose::ModelSummariser::over(&model, &held);

    assert_eq!(
        summariser.spent(),
        None,
        "nothing has been asked yet, and a zero here would be a datum nobody measured"
    );
    summariser
        .summarise(&span_of(&["something"]))
        .await
        .expect("the staged model answers");
    let spent = summariser
        .spent()
        .expect("ADR-0012 D7: every request records its tokens");
    assert_eq!((spent.prompt, spent.completion), (700, 40));
}

/// The summariser's `Debug` is a number and never the session's conversation.
///
/// The mutant: `#[derive(Debug)]`, which renders the redactor and would put
/// the harness's own held bearer values into a panic message.
#[test]
fn the_summarisers_debug_renders_no_text_at_all() {
    let model = StagedModel::text("a summary");
    let held = HeldSecrets::none();
    let rendered = format!("{:?}", crate::compose::ModelSummariser::over(&model, &held));
    assert!(
        rendered.contains("ModelSummariser") && !rendered.contains("summary"),
        "a Debug is what ends up in a panic message, so it names what it holds and renders none \
         of it: {rendered:?}"
    );
}

// ---------------------------------------------------------------------------
// ADR-0013 D2 and D7 — the turn boundary, and the only thing that may compact
// ---------------------------------------------------------------------------

/// Small enough that a handful of staged exchanges crosses the threshold.
fn tight_limits(window: u64, threshold: u64) -> zaru_core::context::ContextLimits {
    ContextLimits::new(
        ContextWindow::new(window).expect("not zero"),
        PressureThreshold::new(threshold).expect("not zero"),
    )
    .expect("the threshold is below the window")
}

/// A summariser that answers a fixed sentence and counts how often it is asked.
struct Counting {
    answer: String,
    asked: std::sync::Mutex<Vec<Vec<String>>>,
}

impl Counting {
    fn answering(answer: &str) -> Self {
        Self {
            answer: answer.to_owned(),
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn spans(&self) -> Vec<Vec<String>> {
        self.asked.lock().expect("no panic holds this").clone()
    }
}

impl zaru_core::context::Summariser for Counting {
    async fn summarise(
        &self,
        span: &zaru_core::context::Span,
    ) -> Result<String, zaru_core::iteration::PortFailure> {
        self.asked.lock().expect("no panic holds this").push(
            span.exchanges()
                .iter()
                .map(|exchange| exchange.as_str().to_owned())
                .collect(),
        );
        Ok(self.answer.clone())
    }
}

/// A summariser that always fails, for the history-preserving check.
struct Failing;

impl zaru_core::context::Summariser for Failing {
    async fn summarise(
        &self,
        _span: &zaru_core::context::Span,
    ) -> Result<String, zaru_core::iteration::PortFailure> {
        Err(zaru_core::iteration::PortFailure::new(
            "the provider was unreachable".to_owned(),
        ))
    }
}

/// D2: compaction happens "when the window pressure threshold is crossed".
///
/// Below it nothing is spent — which is what makes the one-turn binary's own
/// boundary call free rather than a wasted model call on every invocation.
///
/// The mutant: removing `Context::compact`'s early return, which makes the
/// summariser be asked and reddens both assertions.
#[test]
fn a_turn_boundary_under_the_threshold_spends_nothing() {
    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(tight_limits(100_000, 75_000), 0),
    );
    session.record(zaru_core::context::Exchange::verbatim("a short exchange"));
    let summariser = Counting::answering("never asked");

    let compaction = futures_lite_block_on(session.at_turn_boundary(&summariser, &held))
        .expect("nothing to do is not a failure");

    assert!(
        summariser.spans().is_empty(),
        "the threshold was not crossed, so no model call is owed; the summariser was asked {:?}",
        summariser.spans()
    );
    assert!(
        compaction.announcements.is_empty() && compaction.raw.is_none(),
        "a compaction that took nothing announces nothing: {compaction:?}"
    );
}

/// D2 end to end through the product's own types: crossing the threshold
/// replaces the oldest span with a generated summary, announces once with
/// real counts, and hands the raw span back for ADR-0010 D2's transcript.
///
/// The mutant: compacting newest-first, which reddens the span assertion.
#[test]
fn crossing_the_threshold_replaces_the_oldest_span_and_hands_the_raw_one_back() {
    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(tight_limits(8_000, 1_200), 0),
    );
    for nth in 0..8 {
        session.record(zaru_core::context::Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }
    let before: Vec<String> = session
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();
    let summariser = Counting::answering("they settled on eight spaces");

    let compaction = futures_lite_block_on(session.at_turn_boundary(&summariser, &held))
        .expect("the staged summariser answers");

    let raw = compaction.raw.as_ref().expect("layer 6 was compacted");
    let taken: Vec<String> = raw
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();
    assert!(
        !taken.is_empty() && taken.len() < before.len(),
        "the span is some of layer 6 and not all of it; {} of {} were taken",
        taken.len(),
        before.len()
    );
    // Neither arm travels through the compaction: `before` was read off the
    // context prior to it, and `taken` off the returned span.
    assert_eq!(
        taken,
        before[..taken.len()],
        "D2 compacts oldest first, so the span must be the oldest exchanges in order"
    );
    assert_eq!(
        summariser.spans(),
        vec![taken.clone()],
        "the summariser is handed exactly the span that was removed, once"
    );
    assert_eq!(
        session.exchanges()[0].as_str(),
        "they settled on eight spaces",
        "the summary replaces the span at the front of layer 6"
    );
    assert_eq!(
        session.exchanges()[0].kind(),
        zaru_core::context::ExchangeKind::Summary,
        "a summary is marked as one, so a renderer can say which it is"
    );

    match &compaction.announcements[..] {
        [
            zaru_core::context::Announcement::Compacted {
                turns,
                before: cost,
                after,
            },
        ] => {
            assert_eq!(
                *turns as usize,
                taken.len(),
                "D3's count is how many exchanges were replaced"
            );
            let measured: u64 = taken.iter().map(|text| text.len() as u64).sum();
            assert_eq!(
                *cost, measured,
                "D3 asks for real before-and-after counts, so `before` is what the replaced \
                 exchanges actually cost through the counter rather than how many there were"
            );
            assert_eq!(*after, "they settled on eight spaces".len() as u64);
        }
        other => panic!("D3 announces exactly once per compaction; got {other:?}"),
    }
}

/// D2: "History is preserved on disk." A summariser that fails must not cost
/// the session its conversation.
///
/// The mutant: draining the span before awaiting the summary, which reddens
/// the byte-identity assertion.
#[test]
fn a_failing_summariser_leaves_layer_six_exactly_as_it_was() {
    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(tight_limits(8_000, 1_200), 0),
    );
    for nth in 0..8 {
        session.record(zaru_core::context::Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }
    let before: Vec<String> = session
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();

    let failure = futures_lite_block_on(session.at_turn_boundary(&Failing, &held))
        .expect_err("the staged summariser fails");
    assert!(failure.to_string().contains("unreachable"));

    let after: Vec<String> = session
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();
    assert_eq!(
        after, before,
        "the summary is obtained before anything is removed, so a failed summarisation loses no \
         history"
    );
}

/// Directive 20 decision (11): "ADR-0013's reading that a summary may itself
/// be re-compacted is accepted."
///
/// Asserted through the product's own boundary rather than through
/// `zaru-core`'s fixture: compact twice, and find the first summary inside
/// the second span. Exempting summaries would eventually leave compaction
/// with nothing it is allowed to free, which routes straight to D7's
/// exhaustion.
///
/// The mutant: skipping `ExchangeKind::Summary` when choosing the span.
#[test]
fn a_summary_is_compacted_again_like_any_other_exchange() {
    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(tight_limits(8_000, 900), 0),
    );
    for nth in 0..8 {
        session.record(zaru_core::context::Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }

    let first = Counting::answering("the first summary, which is itself layer 6");
    futures_lite_block_on(session.at_turn_boundary(&first, &held)).expect("the first compaction");
    assert!(
        session
            .exchanges()
            .iter()
            .any(|exchange| exchange.kind() == zaru_core::context::ExchangeKind::Summary),
        "the first compaction left a summary to be found"
    );

    // More pressure, so a second compaction has to reach past the summary.
    for nth in 8..16 {
        session.record(zaru_core::context::Exchange::verbatim(format!(
            "exchange {nth}: {}",
            "detail ".repeat(30)
        )));
    }
    let second = Counting::answering("the second summary");
    futures_lite_block_on(session.at_turn_boundary(&second, &held)).expect("the second compaction");

    let spans = second.spans();
    let span = spans.first().expect("the second compaction took a span");
    assert!(
        span.iter()
            .any(|text| text == "the first summary, which is itself layer 6"),
        "a summary is a layer-6 exchange like any other and may be re-compacted, oldest first; \
         the second span held {span:?}"
    );
}

/// ADR-0010 D3's checkpoint holds "what the model needs to continue — the
/// compacted conversation, per ADR-0013", and a resumed session reads it back.
///
/// The prefix is deliberately not in it: D1 forbids rewriting layers 1 to 4
/// mid-session, and a resumed session builds its own from its own
/// configuration.
///
/// The mutant: writing the rendered text instead of the exchanges, which
/// reddens the restore.
#[test]
fn the_checkpoint_carries_layer_six_and_a_resumed_session_reads_it_back() {
    let limits = tight_limits(100_000, 75_000);
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(limits, 0),
    );
    session.record(zaru_core::context::Exchange::of_turn(
        "read notes.txt and tell me the rehearsal number",
        &["fs.read notes.txt -- 82 bytes".to_owned()],
        "the rehearsal number is 4173",
    ));
    session.record(zaru_core::context::Exchange::summary("an older stretch"));

    let stored = session.checkpoint();
    let restored = crate::compose::SessionContext::restored(
        context::prefix_for(None),
        crate::compose::ContextShape::of(limits, 0),
        &stored,
    )
    .expect("what this type wrote, it reads");

    let there: Vec<&str> = session
        .exchanges()
        .iter()
        .map(zaru_core::context::Exchange::as_str)
        .collect();
    let back: Vec<&str> = restored
        .exchanges()
        .iter()
        .map(zaru_core::context::Exchange::as_str)
        .collect();
    assert_eq!(there, back, "layer 6 comes back as what it was");
    assert_eq!(
        restored.exchanges()[1].kind(),
        zaru_core::context::ExchangeKind::Summary,
        "a summary comes back marked as one, or a re-compaction would treat it as fresh \
         conversation"
    );
    assert!(
        !stored.to_string().contains(prose::NO_PERSONA),
        "the stable prefix is not in the checkpoint: D1 forbids rewriting layers 1 to 4 \
         mid-session, and a stored prefix would outlive the session that read it"
    );

    // A document this type did not write is refused rather than read as an
    // empty conversation, which would drop a session's history and look
    // exactly like a session that had none.
    crate::compose::SessionContext::restored(
        context::prefix_for(None),
        crate::compose::ContextShape::of(limits, 0),
        &serde_json::json!({ "exchanges": "not a list" }),
    )
    .expect_err("a checkpoint this type did not write is refused");
}

/// ADR-0013 D1's layer 6 is "conversation **and tool results**", so a turn is
/// all three parts.
///
/// The mutant: dropping the tool results from `of_turn`, which reddens the
/// middle assertion — and the middle is where a coding session's facts are.
#[test]
fn one_turn_in_layer_six_carries_the_task_the_tool_results_and_the_answer() {
    let exchange = zaru_core::context::Exchange::of_turn(
        "read notes.txt",
        &[
            "fs.read notes.txt -- 82 bytes".to_owned(),
            "cmd.run cargo test -- exit 0".to_owned(),
        ],
        "the rehearsal number is 4173",
    );
    let text = exchange.as_str();
    for part in [
        "read notes.txt",
        "fs.read notes.txt -- 82 bytes",
        "cmd.run cargo test -- exit 0",
        "the rehearsal number is 4173",
    ] {
        assert!(
            text.contains(part),
            "layer 6 is conversation and tool results, and {part:?} is missing from {text:?}"
        );
    }
    // An empty part contributes nothing rather than a blank stretch: a turn
    // with no tool calls is the ordinary case.
    let quiet = zaru_core::context::Exchange::of_turn("a question", &[], "an answer");
    assert_eq!(quiet.as_str(), "a question\n\nan answer");
}

/// D7, one layer out: **a turn in progress cannot compact.**
///
/// This is the shape of the property rather than an assertion about it. While
/// the `TurnContext` returned by `policy(&self)` is alive, `self` is borrowed
/// shared, so `at_turn_boundary(&mut self)` is not callable — the code below,
/// uncommented, is `error[E0502]: cannot borrow `session` as mutable because
/// it is also borrowed as immutable`.
///
/// It is left as text on purpose: a check that *ran* would have to compile,
/// and the whole point is that it does not. The compiler is the mechanism, as
/// it is for ADR-0013 clause 6 one layer in. Verified by uncommenting it.
#[test]
fn a_policy_in_hand_is_a_turn_in_progress_and_cannot_reach_the_boundary() {
    // let held = HeldSecrets::none();
    // let mut session = SessionContext::opened(context::prefix_for(None), limits);
    // let policy = session.policy(&held, false);
    // futures_lite_block_on(session.at_turn_boundary(&Failing, &held));  // E0502
    // drop(policy);

    // What *is* runnable is the other half: once the policy is dropped, the
    // boundary is reachable again, which is what makes a many-turn session
    // possible at all rather than a context nobody can ever compact.
    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(tight_limits(8_000, 1_200), 0),
    );
    {
        let policy = session.policy(&held, false);
        let _ = futures_lite_block_on(policy.assemble(&Turn::Initial { task: "a task" }));
    }
    futures_lite_block_on(session.at_turn_boundary(&Counting::answering("x"), &held))
        .expect("the boundary is reachable once no turn holds the context");
}

/// [ADR-0010] D2's seventh producer records the **answer**, not the turn's
/// output, and three of the five outcomes answer nothing.
///
/// # What this is defending
///
/// By the time `run_one` records a turn, `Ran::lines` also carries
/// [ADR-0011] D2's not-a-sandbox notice, [ADR-0013] D2's compaction
/// announcements and [ADR-0012] D7's usage line. The first two are already on
/// the transcript as `Record::Said` and `Record::Compacted`, written by the
/// same function a few lines earlier. So recording the joined lines as the
/// answer would put those sentences on that file a **second** time — which is
/// exactly the second store ADR-0010 D3 separates the checkpoint from the
/// transcript to prevent, arriving inside the transcript instead of beside it.
///
/// It is checked here, on a pure function over the outcome, because `run_one`
/// itself needs a real provider client to reach — `Prepared` holds a
/// `GeminiClient` and no stub can be substituted for it — so the decision was
/// given a seam that an offline check can drive rather than being left where
/// only the artefact could see it.
///
/// # The mutants
///
/// **`Some` for the three that answered nothing**, which is what an
/// `unwrap_or_default` or a `format!` over the outcome produces: the harness
/// would claim it said something on exactly the turns where it did not, and a
/// reader could no longer tell a stopped turn from an interrupted one by the
/// file. **`None` for `Answered`**, which is the arc's whole subject going
/// missing. Both are held by the table below, which walks every variant.
///
/// The accepting half is inside the same table: two outcomes must answer and
/// carry their own words, so a function returning `None` for everything — the
/// cheapest way to satisfy the three absences — fails on the first two rows.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[test]
fn adr_0010_d2s_conversation_records_the_answer_and_not_the_turns_output() {
    use crate::compose::turn::answer_of;
    use zaru_core::iteration::{ExhaustionReason, Outcome as Inner};
    use zaru_core::tool_call::Outcome;

    let answered = Outcome::Answered {
        text: "one, two, three".to_owned(),
        rounds: 1,
        tokens: 12,
    };
    assert_eq!(
        answer_of(&answered).as_deref(),
        Some("one, two, three"),
        "the model's own words are the answer, and nothing composes around them"
    );

    let iterated = Outcome::Iterated(Inner::Succeeded {
        iterations: 2,
        total_elapsed: core::time::Duration::from_secs(3),
    });
    let satisfied = answer_of(&iterated).expect("a turn whose validators passed answered");
    assert!(
        satisfied.contains("satisfied after 2 iteration(s)"),
        "an iterating turn's answer is the sentence the reader was shown, in one spelling \
         rather than two: {satisfied:?}"
    );

    // The three that answered nothing. Each already has a carrier a reader
    // sees -- `Event::TurnEnded` -- so an empty `zaru` half would be the
    // harness claiming it spoke.
    for (what, outcome) in [
        (
            "a model that stopped",
            Outcome::Stopped {
                reason: "SAFETY".to_owned(),
                rounds: 1,
                tokens: 4,
            },
        ),
        (
            "a turn that reached its ceiling",
            Outcome::Exhausted {
                rounds: 8,
                calls: 8,
                tokens: 99,
            },
        ),
        (
            "an iteration loop that was exhausted",
            Outcome::Iterated(Inner::Exhausted {
                iterations: 3,
                reason: ExhaustionReason::CeilingReached,
                last_failure: Some("greets: failed".to_owned()),
            }),
        ),
    ] {
        assert_eq!(
            answer_of(&outcome),
            None,
            "{what} answered nothing, and recording an answer for it would be the harness \
             saying it spoke when it did not"
        );
    }
}

// --------------- ADR-0010 D2's eighth producer: the run that failed after it

/// A refused **checkpoint write** leaves the transcript carrying its headline,
/// and one function writes the record for both of the places a run can fail.
///
/// # Why this exists beside the out-of-tree check rather than inside it
///
/// `turn_from_outside.rs` holds the turn's own refusal against the built
/// binary, staged with a closed loopback port. The refusal *after* the turn
/// cannot be staged that way: `compose::turn::task` mints a fresh session and
/// writes [ADR-0010] D3's checkpoint into it in the same process, so making
/// that directory unwritable between the mint and the write is a race, and
/// [Verification lessons] §57 is exactly about not watching a defect through
/// a widened window. The condition itself is deterministic here — a session
/// directory at `0500`, which `crate::atomic::write` cannot create a
/// temporary file in while an already-open transcript handle keeps working,
/// because a directory's write bit governs creation and not writes to an open
/// file.
///
/// # Two arms, and the second is the one that would rot
///
/// **The behavioural arm** stages the condition and asserts the headline
/// reaches the file, with the class beside it: `Classify::checkpoint` is a
/// **defect**, where the turn's own provider refusal is environmental, so a
/// recorder that hard-coded either class fails one of the two checks.
///
/// **The call-site arm** asserts that the record is built in exactly one place
/// and reached from exactly two, read off the module's own source. Without it
/// a later arc could satisfy the first arm and leave `task`'s tail recording
/// nothing, which is the state this check was written to end.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn adr_0010_d2s_failure_record_is_written_by_one_function_for_both_callers() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = ScratchRoot::new();
    let root = scratch.store_root();
    let store = crate::session::SessionStore::reading(root);
    let id = crate::session::SessionId::mint(&crate::session::SystemWallClock)
        .expect("a ULID is minted");
    let session = store.start(id).expect("the scratch root takes a session");
    std::fs::write(session.transcript_path(), b"").expect("the transcript file exists");

    // The condition, deterministic: the directory cannot take a new file and
    // the transcript already in it can still be appended to.
    std::fs::set_permissions(session.directory(), std::fs::Permissions::from_mode(0o500))
        .expect("the scratch directory takes a mode");

    let context = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(
            crate::cli::layers::context_limits(crate::providers::gemini::CONTEXT_WINDOW_TOKENS),
            0,
        ),
    );
    let refusal = crate::compose::boundary::checkpointed(&context, &session)
        .expect_err("a session directory at 0500 cannot take a checkpoint's temporary file");
    let classified = crate::cli::classify::Surface::checkpoint(&refusal, session.evidence());
    let headline = crate::failure::Presentation::of(&classified).headline;

    crate::compose::turn::record_the_failure(&session, &classified);

    std::fs::set_permissions(session.directory(), std::fs::Permissions::from_mode(0o700))
        .expect("the scratch directory takes a mode back");
    let transcript =
        std::fs::read_to_string(session.transcript_path()).expect("the transcript reads back");
    let last = transcript.lines().next_back().unwrap_or_else(|| {
        panic!(
            "a run whose checkpoint was refused wrote nothing at all, so `cat` shows a person a \
             turn and no reason it stopped"
        )
    });
    assert!(
        last.starts_with(r#"{"failure":"#),
        "the checkpoint was refused and the transcript's last record is not what refused it: \
         {last}"
    );
    assert!(
        last.contains(&headline),
        "the record does not carry the headline the reader was shown. Shown: {headline:?}. \
         Recorded: {last}"
    );
    assert!(
        last.contains(r#""class":"defect""#),
        "a refused checkpoint is ADR-0016 D1's defect and the record says otherwise, so the \
         class is being invented rather than projected: {last}"
    );

    // The call-site arm, over this module's own product source.
    let source = include_str!("turn.rs");
    let built = source.matches("Record::Failure(").count();
    assert_eq!(
        built, 1,
        "ADR-0010 D2's eighth producer is constructed in {built} places in `compose::turn`; one \
         construction is what keeps the two failing paths from disagreeing the day the record \
         gains a field"
    );
    let called = source.matches("record_the_failure(").count();
    assert_eq!(
        called, 3,
        "`record_the_failure` appears {called} time(s) in `compose::turn`: its definition, \
         `run_one`'s wrapper for a turn that was refused, and `task`'s tail for a checkpoint \
         that would not write. A run can fail in both places and both owe the reader a record"
    );
}

/// The client-bearing kinds are in landing order, and a third one appended.
///
/// **What this guards is a tie-break, not a list.** Part 2 of
/// [`crate::providers::select`] takes the first kind whose requirement holds,
/// so this array's order decides which provider answers on a machine where two
/// requirements hold at once. Appending can only give an answer to a machine
/// that had none; inserting silently moves a working machine to a different
/// provider, which is a behaviour change arriving with no release note.
///
/// So the order is asserted literally rather than as a set, and the name says
/// why, because the failure this prevents is invisible in a diff that merely
/// looks like a reordering.
#[test]
fn the_client_bearing_kinds_are_in_landing_order_so_a_later_one_cannot_displace_an_earlier() {
    use crate::providers::ProviderKind;

    assert_eq!(
        crate::compose::KINDS_WITH_A_CLIENT.to_vec(),
        vec![
            ProviderKind::Gemini,
            ProviderKind::Ollama,
            ProviderKind::OpenAiCompatible,
        ],
        "landing order: `gemini` 2026-09-05, `ollama` and `openai-compatible` 2026-09-14. A kind \
         inserted rather than appended re-tie-breaks every machine that already resolved one of \
         the kinds it was placed before",
    );

    // And it is deliberately NOT `ProviderKind::ALL`'s order, which would put
    // `openai-compatible` first and `ollama` before `gemini`. Asserting the
    // difference is what stops somebody "tidying" the array into the other one.
    let in_all_order: Vec<ProviderKind> = ProviderKind::ALL
        .into_iter()
        .filter(|kind| crate::compose::KINDS_WITH_A_CLIENT.contains(kind))
        .collect();
    assert_ne!(
        crate::compose::KINDS_WITH_A_CLIENT.to_vec(),
        in_all_order,
        "this array is landing order and `ProviderKind::ALL` is declaration order; sorting one \
         into the other would change which provider answers on a machine configured for two",
    );
}

/// A machine that resolved a kind before the third client landed still does.
///
/// The property the append was chosen for, asserted directly rather than
/// argued: for every machine state expressible as "holds these keys, has these
/// endpoints", adding `openai-compatible` to the array changes the answer only
/// where there was no answer before.
#[test]
fn appending_the_third_client_changes_no_machine_that_already_had_an_answer() {
    use crate::providers::{ModelAlias, ProviderKind, select};

    const BEFORE: [ProviderKind; 2] = [ProviderKind::Gemini, ProviderKind::Ollama];

    // Every combination of "holds a gemini key" x "has an ollama endpoint" x
    // "has an openai-compatible endpoint".
    for gemini_key in [false, true] {
        for ollama_endpoint in [false, true] {
            for compatible_endpoint in [false, true] {
                let holds = |kind: ProviderKind| gemini_key && kind == ProviderKind::Gemini;
                let has = |kind: ProviderKind| match kind {
                    ProviderKind::Ollama => ollama_endpoint,
                    ProviderKind::OpenAiCompatible => compatible_endpoint,
                    _ => false,
                };
                let before = select(ModelAlias::Default, None, &BEFORE, holds, has);
                let after = select(
                    ModelAlias::Default,
                    None,
                    &crate::compose::KINDS_WITH_A_CLIENT,
                    holds,
                    has,
                );
                match before {
                    Ok(kind) => assert_eq!(
                        after.as_ref().ok(),
                        Some(&kind),
                        "a machine that resolved `{kind}` before must still resolve it \
                         (gemini_key={gemini_key}, ollama_endpoint={ollama_endpoint}, \
                         compatible_endpoint={compatible_endpoint})",
                    ),
                    Err(_) => assert_eq!(
                        after.is_ok(),
                        compatible_endpoint,
                        "a machine with no answer before gains one exactly when it configured \
                         the new kind's endpoint (compatible_endpoint={compatible_endpoint})",
                    ),
                }
            }
        }
    }
}

/// A `ModelId` through the door the product uses.
///
/// `ModelId` is constructible only inside the resolution table, which is
/// ADR-0012 D1 as a compile error — so a check stages a configuration and
/// reads the alias back out of it, exactly as the binary does. The same
/// reasoning, and the same shape, as `providers::ollama::tests::model`.
fn model_named(name: &str) -> crate::providers::ModelId {
    use crate::config::{Contribution, Layer, Resolution};
    use crate::providers::{ModelAlias, ResolvedModel, declare, resolution::ModelTable};

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

/// A provider that reports a prompt count and nothing else, for the one
/// assertion that needs a real one beside a counted context.
#[derive(Debug)]
struct Reporting {
    endpoint: crate::providers::ProviderEndpoint,
    prompt_tokens: u64,
}

impl crate::providers::Provider for Reporting {
    fn kind(&self) -> crate::providers::ProviderKind {
        crate::providers::ProviderKind::Ollama
    }

    fn endpoint(&self) -> &crate::providers::ProviderEndpoint {
        &self.endpoint
    }

    fn capabilities(&self) -> crate::providers::ProviderCapabilities {
        crate::providers::ProviderCapabilities::declared(
            true,
            true,
            true,
            Some(crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS),
        )
    }

    fn usage(&self) -> Option<crate::providers::TokenUsage> {
        Some(crate::providers::TokenUsage::counted(
            self.prompt_tokens,
            23,
        ))
    }
}

/// The counted context carries the tool surface, so the harness's number is
/// not below the provider's own.
///
/// # The measurement this is built from, and why it is not a round trip
///
/// Measured 2026-09-14 from the release binary at `4c89977` against a local
/// Ollama through a logging proxy, one real turn:
///
/// ```text
/// exchange 1  whole request 1,967 bytes  message content 231  prompt_eval_count 465
/// ```
///
/// `compose::count`'s whole soundness argument was `bytes >= tokens`, and
/// **231 is not at least 465**. The gap is the tool surface, which every
/// request carries and the context does not contain. So this stages that
/// exact shape — a conversation of the measured size, the reserve taken from
/// the **real** `ollama` client's own wire mapping of the **real** built-in
/// descriptor set, and a provider reporting the number that server actually
/// reported — and requires the harness's count to be at least the provider's.
///
/// The reserve is not a literal here, and deliberately: what this client
/// sends is `serde_json`'s compact form, which came to **1,619** bytes for
/// the seven built-ins when this check was written. Taking it from the client
/// rather than writing the number down is the point — a literal would be a
/// measurement of a request nobody sends, and it would go stale the first
/// time a descriptor's wording changed.
///
/// Watched red once, the mutation confirmed applied on disk and restored:
/// opening the context with a reserve of zero, which is what the harness did
/// until this landed.
#[test]
fn the_counted_context_carries_the_tool_surface_and_is_not_below_the_providers_own_count() {
    use crate::providers::Provider as _;

    let client = crate::providers::ollama::OllamaClient::new(
        crate::providers::ProviderEndpoint::new("http://127.0.0.1:11434")
            .expect("a well-formed origin"),
        model_named("llama3.2:3b"),
        crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("an HTTP client builds without touching the network");
    let reserved = client
        .tool_surface_bytes(crate::tools::descriptor_set())
        .expect("the built-in descriptors' schemas are JSON this client can map");
    assert!(
        reserved > 1_000,
        "the seven built-in descriptors are a real surface, not a rounding: {reserved} bytes"
    );

    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(
            crate::cli::layers::context_limits(
                crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
            ),
            reserved,
        ),
    );
    // The measured 231 bytes of message content, as one exchange.
    session.record(zaru_core::context::Exchange::verbatim("m".repeat(231)));

    let reported = Reporting {
        endpoint: crate::providers::ProviderEndpoint::new("http://127.0.0.1:11434")
            .expect("a well-formed origin"),
        prompt_tokens: 465,
    }
    .usage()
    .expect("this provider accounts");

    let counted = session.usage(&held).used();
    assert!(
        counted >= reported.prompt_tokens(),
        "the harness counted {counted} where the provider counted {} prompt tokens for the same \
         request. Measured 2026-09-14: 231 bytes of message content and {reserved} bytes of tool \
         schema reached a provider that reported 465, so a count that leaves the tool surface out \
         is BELOW the provider's own and overflows the window in silence",
        reported.prompt_tokens()
    );
}

/// The crossing reached the way a reader reaches it: a configured window.
///
/// # What this asserts that the checks above do not
///
/// Those stage `ContextLimits` directly, which is the seam. This one goes
/// through the whole path a person walks — `provider.ollama.context_tokens`
/// set at the project layer, resolved through [ADR-0014]'s five layers,
/// turned into limits by `cli::layers::context_limits`, and crossed by
/// ordinary turns — so it is the reachability half that no mutant of the
/// arithmetic can see, and it is what makes [ADR-0013] clause 2's crossing a
/// thing the binary does rather than a thing a check stages.
///
/// The window is 3,000 and the threshold therefore 2,250. The reserve is the
/// real tool surface, because it is on every request and a reader's session
/// crosses with it: **the crossing is reached sooner than the conversation
/// alone would reach it**, which is the whole point of counting it. The
/// numbers are chosen so that the span taken is a *proper prefix* of layer 6
/// — a window small enough to take everything would satisfy an oldest-first
/// assertion and a newest-first implementation equally, which is exactly the
/// weakness the mutation below found when this check was first written at a
/// window of 2,000.
///
/// Watched red three ways, each mutation confirmed applied on disk and the
/// file restored byte-identical:
///
/// - the threshold made equal to the window (`window / 4 * 3` to `window`) —
///   *"three quarters of 3,000"*, left 3000, right 2250, so compaction would
///   have fired only once the context already did not fit;
/// - `Context::compact` compacting newest-first — the raw span came back as
///   the newest exchanges where the oldest are required. **This is the
///   mutation that found the check's own first weakness**: at a window of
///   2,000 the whole of layer 6 was taken, so oldest and newest were the same
///   span and the mutant passed. The window is 3,000 for that reason, and at
///   it the mutant prints the newest two exchanges where the oldest two are
///   required;
/// - the announcement reporting `after` twice — *"the announcement's counts
///   are the span's own, measured here from the staged text rather than read
///   back through the code under test"*, left `(2, 44)`, right `(2, 212)`.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn a_small_configured_window_is_crossed_by_a_session_and_announced_with_real_counts() {
    use crate::config::{Contribution, Layer, Resolution, Source, Table, Value};

    let key = crate::providers::ProviderKind::Ollama.context_tokens_key();
    let mut project = Table::new();
    project.insert_path(&key, Value::Integer(3_000));
    let resolution = Resolution::resolve(
        &crate::cli::layers::schema(),
        vec![
            Contribution::new(Layer::BuiltIn, Layer::BuiltIn.default_source(), {
                use crate::config::LayerSource;
                crate::cli::layers::BuiltIn::new()
                    .read()
                    .expect("layer 1 reads")
            }),
            Contribution::new(Layer::Project, Source::named("./zaru.toml"), project),
        ],
    )
    .expect("a project lowering a window is what ADR-0014 D6 permits");

    let Some(Value::Integer(resolved)) = resolution.get(&key) else {
        panic!("the project's window is the effective one");
    };
    let window = u64::try_from(*resolved).expect("a window fits");
    assert_eq!(
        window, 3_000,
        "the configured window, not the built-in 4,096"
    );

    let limits = crate::cli::layers::context_limits(window);
    assert_eq!(limits.threshold().get(), 2_250, "three quarters of 3,000");

    let client = crate::providers::ollama::OllamaClient::new(
        crate::providers::ProviderEndpoint::new("http://127.0.0.1:11434")
            .expect("a well-formed origin"),
        model_named("llama3.2:3b"),
        window,
    )
    .expect("an HTTP client builds without touching the network");
    let reserved = client
        .tool_surface_bytes(crate::tools::descriptor_set())
        .expect("the built-in descriptors' schemas are JSON this client can map");

    let held = HeldSecrets::none();
    let mut session = crate::compose::SessionContext::opened(
        context::prefix_for(None),
        crate::compose::ContextShape::of(limits, reserved),
    );

    // Ordinary turns, each the size of a short answer, until the threshold is
    // behind us. Asserted rather than assumed: a session that felt no
    // pressure would satisfy every assertion below by standing still.
    for nth in 0..6 {
        session.record(zaru_core::context::Exchange::verbatim(format!(
            "user: what did we decide about item {nth}?\n\nzaru: {}",
            "we settled it. ".repeat(4)
        )));
    }
    let used = session.usage(&held).used();
    assert!(
        used > limits.threshold().get(),
        "the configured window is crossed by this conversation: {used} used against a threshold \
         of {}, with {reserved} bytes of that the tool surface every request carries",
        limits.threshold().get()
    );

    let before: Vec<String> = session
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();
    let summariser = Counting::answering("they went through six items and settled each");

    let compaction = futures_lite_block_on(session.at_turn_boundary(&summariser, &held))
        .expect("the staged summariser answers");

    let raw = compaction.raw.as_ref().expect("layer 6 was compacted");
    let taken: Vec<String> = raw
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().to_owned())
        .collect();
    assert!(
        !taken.is_empty() && taken.len() < before.len(),
        "the span is SOME of layer 6 and not all of it, or oldest-first and newest-first are the \
         same span and this check cannot tell them apart; {} of {} were taken",
        taken.len(),
        before.len()
    );
    assert_eq!(
        taken,
        before[..taken.len()],
        "ADR-0013 D2 compacts oldest first, and the raw span is what ADR-0010 D2's transcript keeps"
    );

    let Some(zaru_core::context::Announcement::Compacted {
        turns,
        before: cost_before,
        after,
    }) = compaction.announcements.first()
    else {
        panic!("a crossing announces itself once, with counts: {compaction:?}");
    };
    let staged: u64 = taken.iter().map(|text| text.len() as u64).sum();
    assert_eq!(
        (*turns as usize, *cost_before),
        (taken.len(), staged),
        "the announcement's counts are the span's own, measured here from the staged text rather \
         than read back through the code under test"
    );
    assert_eq!(
        *after,
        "they went through six items and settled each".len() as u64,
        "and the after-count is the summary's own bytes"
    );
    assert!(
        *after < *cost_before,
        "a compaction that grew the context is not a compaction: {after} against {cost_before}"
    );
}

/// The session's once-ever notice is routed to the narrator where there is
/// one, in the one place it is said.
///
/// # Why this is a source walk
///
/// `compose::turn::ran` needs a [`Prepared`](crate::compose::Prepared), which
/// needs a provider client and a stored key, so **no offline check can drive
/// it** — the finding `Narrator::announce_interrupted`'s own documentation
/// records for the neighbouring method, whose answer there was a witness type.
/// A witness will not carry this one: the notice is owed on some sessions and
/// not others, so a value proving it was announced cannot be required of every
/// turn. What is held instead is that the block which spends
/// `SessionNotice::state_once` hands the sentence to the narrator, in the shape
/// `only_one_place_in_the_product_records_a_tip_showing` already uses.
///
/// The carrier's own order is
/// `the_sessions_once_ever_notice_is_painted_above_the_turns_own_lines`, and
/// the artefact is the evidence about the binary.
///
/// # The mutant and the accepting sibling
///
/// Pushing onto `lines` unconditionally, which is the block as it stood: the
/// walk finds no call beside the spend. Watched red.
///
/// The sibling is the `None` arm, which must stay — `zaru "<task>"` has no
/// narrator and prints `Ran::lines` in order, so deleting the push would take
/// the notice off the out-of-session surface entirely.
#[test]
fn the_once_ever_notice_is_handed_to_the_narrator_where_there_is_one() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/compose/turn.rs"),
    )
    .expect("this crate's own turn module reads");
    let code: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();

    let spend = code
        .iter()
        .position(|line| line.contains("notice.state_once()"))
        .expect("the notice is spent somewhere in this module");
    let block = &code[spend..spend + 12];
    let joined = block.join("\n");

    assert!(
        joined.contains("narrator.announce_session_notice(&sentence)"),
        "the block that spends ADR-0011 D2's notice does not hand it to the narrator, so it \
         lands inside the turn's own lines again: {joined:?}"
    );
    assert!(
        joined.contains("lines.push(sentence.clone())"),
        "the out-of-session arm is gone, so `zaru \"<task>\"` says nothing about the membrane at \
         all: {joined:?}"
    );
    let announce = joined
        .find("narrator.announce_session_notice")
        .expect("asserted above");
    let push = joined.find("lines.push(sentence").expect("asserted above");
    assert!(
        announce < push,
        "the push is the first arm, so a session with a pane takes it: {joined:?}"
    );
}
