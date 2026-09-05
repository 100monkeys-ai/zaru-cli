// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the composition's five adapters do, checked one at a time.
//!
//! Every check here is about an adapter's own contract. What the adapters do
//! *together* is a turn, and a turn is checked from outside the crate against
//! the built binary, because a composition asserted from inside its own crate
//! is a composition nobody has been shown to reach ([Verification lessons]
//! §25).
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::compose::{ByteCounter, NoFetch, Records, TurnContext, context, prose};
use crate::credentials::fixtures::ScratchRoot;
use crate::redaction::HeldSecrets;
use crate::session::Record;
use crate::tools::port::Fetch;
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

/// `web.fetch` reports the work's failure and never a port failure.
///
/// The difference is a whole turn: a `PortFailure` reaches
/// `ToolCallError::Port` and ends the run, while a `Captured` with a non-zero
/// exit code is ADR-0016 D1 row 1's expected register and the loop carries on.
///
/// Watched red by returning `Err(PortFailure::new(NOT_BUILT))`, which printed
/// *"web.fetch must report the work's failure rather than a port failure: a
/// port failure ends the whole turn"*.
#[test]
fn the_unbuilt_built_in_fails_as_the_work_rather_than_as_a_port() {
    let captured = futures_lite_block_on(NoFetch.retrieve("https://example.invalid/"));
    let captured = captured.expect(
        "web.fetch must report the work's failure rather than a port failure: a port failure ends \
         the whole turn",
    );
    assert_ne!(
        captured.exit_code, 0,
        "a tool that did nothing must not report success"
    );
    assert!(
        captured.stderr.contains(crate::compose::fetch::NOT_BUILT),
        "the model is told nothing it can act on: {captured:?}"
    );
    // The URL came from the model and is not echoed back at it.
    assert!(!captured.stderr.contains("example.invalid"));
    assert!(captured.stdout.is_empty());
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
/// The adapters' futures never yield — `NoFetch` and `TurnContext::assemble`
/// both do their whole work synchronously and hand back a future that is
/// already resolved, exactly as `ValidatorRunner`'s product implementation
/// does. So a check can poll one once rather than taking a reactor, and these
/// checks stay free of the runtime the *binary* needs for `reqwest`.
///
/// It panics rather than looping if a future ever does yield, because that
/// would mean an adapter had grown a real await and this helper had silently
/// stopped being appropriate for it.
fn futures_lite_block_on<F: core::future::Future>(future: F) -> F::Output {
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
