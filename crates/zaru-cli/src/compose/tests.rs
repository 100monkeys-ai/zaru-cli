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

    let prefix = context::prefix_for();
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
    let context = Context::opened(context::prefix_for(), limits);
    let held = HeldSecrets::none();
    let policy = TurnContext::over(&context, &held);

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
                (context::prefix_for().as_str().len() as u64) < window,
                "the prefix alone does not fit this window, so the refusal says nothing about the \
                 tail: {} against {window}",
                context::prefix_for().as_str().len()
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
    let context = Context::opened(context::prefix_for(), crate::cli::layers::context_limits());
    let held = HeldSecrets::none();
    let policy = TurnContext::over(&context, &held);

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
