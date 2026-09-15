// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the tool-call loop.
//!
//! Every count asserted here comes from what a fixture was **staged with**,
//! never from a second reading taken through the loop — library
//! [Verification lessons] §11's rule that one arm of a comparison must not
//! travel through the thing being checked.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::iteration::fixtures::ManualClock;
use crate::iteration::port::{ContextRefusal, Interruption};
use crate::redaction::Redacted;
use crate::redaction::fixtures::{
    HoldingOne, NothingHeld, ascii_core as redaction_ascii_core, staged_secret,
};
use crate::tool_call::error::{PortKind, ToolCallError};
use crate::tool_call::event::{Event, TurnEnding};
use crate::tool_call::fixtures::{
    Act, Answer, Projector, Recorder, RecordingContext, RefusingContext, StagedInner, StagedModel,
    StagedTools, ascii_core, descriptor, nonce, request, tag,
};
use crate::tool_call::limits::ToolCallCeiling;
use crate::tool_call::machine::{Outcome, Start, run};
use crate::tool_call::port::{Ports, ToolCalling};
use core::time::Duration;
use std::sync::Arc;

/// A ceiling every check that is not about the ceiling can share.
fn roomy() -> ToolCallCeiling {
    ToolCallCeiling::new(8).expect("eight is a usable ceiling")
}

fn manual_clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::default())
}

/// The two descriptors every check offers, unless it is about descriptors.
fn tools() -> Vec<crate::tool_call::port::ToolDescriptor> {
    vec![descriptor("fs.read"), descriptor("cmd.run")]
}

/// ADR-0012 clause 3: "A provider declaring no tool-call capability fails at
/// **configuration time** with a clear message, **not mid-loop**."
///
/// The mutant: making `required` return `Ok` regardless. There is no mutant
/// that makes the loop discover it late, because there is no path that
/// reaches the loop without this value — which is the point of the check as
/// much as its assertions are.
#[test]
fn a_model_that_cannot_call_tools_is_refused_before_the_loop_can_start() {
    let clock = manual_clock();
    let model = StagedModel::without_tool_calling(Arc::clone(&clock));
    let core = ascii_core("gpt-nothing");
    let name = format!("  {core}\nnai\u{0301}ve-\u{1F980}  ");

    let refused = ToolCalling::required(&model, &name)
        .expect_err("a model that cannot call tools must be refused");

    assert_eq!(
        refused.model, name,
        "the refusal must carry the model the caller named, unaltered, so a caller can render it          however it renders names"
    );
    let said = refused.to_string();
    // The ASCII core rather than the whole nonce, because the message quotes
    // the name through `{:?}` -- which is correct for a name that arrives
    // from configuration and could carry a control character, and which
    // escapes the combining mark so that an assertion against the value as
    // typed misses a message that does name it. Library verification lessons
    // §50 from the presence side; this check failed on its first run for
    // exactly that reason.
    assert!(
        said.contains(&core),
        "the message does not name the model: {said:?}"
    );
    assert!(
        said.contains("cannot call tools"),
        "the message does not say what is wrong: {said:?}"
    );

    let able = StagedModel::new(Vec::new(), clock, Duration::ZERO);
    assert!(
        ToolCalling::required(&able, "able").is_ok(),
        "a model that can call tools must be admitted, or this check is satisfied by a \
         gate that refuses everything"
    );
}

/// ADR-0008 D1: "the model requests a tool, the harness executes it, the
/// result returns, the model continues."
///
/// The mutant: dropping the result rather than pushing it, or reassembling
/// the prompt each round. Both leave the second exchange's `results` empty,
/// and the nonce means no implementation can produce the content without
/// having carried it.
#[tokio::test]
async fn a_tool_result_returns_to_the_model_byte_for_byte_and_the_model_continues() {
    let clock = manual_clock();
    let produced = nonce("what-the-tool-produced");
    let answered = nonce("the-answer");
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("call-1", "fs.read")]),
            Answer::Text(answered.clone()),
        ],
        Arc::clone(&clock),
        Duration::from_millis(3),
    );
    let mut executor = StagedTools::new(
        vec![Act::Return(produced.clone())],
        tools(),
        Arc::clone(&clock),
        Duration::from_millis(5),
    );
    let context = RecordingContext::default();
    let seen = Arc::clone(&model.seen);
    let mut recorder = Recorder::default();

    let outcome = run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("the task"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("staged model can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    let seen = seen.lock().expect("seen poisoned").clone();
    assert_eq!(
        seen.len(),
        2,
        "the model was staged to answer twice, so it should have been asked twice"
    );
    assert!(
        seen[0].is_empty(),
        "the first exchange cannot carry a result, and carried {:?}",
        seen[0]
    );
    assert_eq!(
        seen[1].len(),
        1,
        "the second exchange should carry exactly the one result the tool produced"
    );
    assert_eq!(
        seen[1][0].content.as_str(),
        produced,
        "the tool's output did not reach the model byte for byte"
    );
    assert_eq!(
        seen[1][0].id, "call-1",
        "the result must carry the id the model asked under, or a provider cannot match them"
    );

    match outcome {
        Outcome::Answered { text, rounds, .. } => {
            assert_eq!(
                text, answered,
                "the model's answer did not come back verbatim"
            );
            assert_eq!(rounds, 2, "two exchanges were staged");
        }
        other => panic!("the model answered, so the turn should have: {other:?}"),
    }
}

/// The ADR-0016 ruling of 2026-09-04: a declined prompt is not a failure, so
/// it becomes the next model turn's content.
///
/// Two mutants. Turning a refusal into an `Err` — which cannot be written,
/// because `ToolCallError` has no variant for it, and that is asserted here
/// by the outcome being `Ok`. And dropping a refusal rather than pushing it,
/// which empties the second exchange's `results`.
#[tokio::test]
async fn a_refusal_becomes_the_next_model_turns_content_and_is_never_an_error() {
    let clock = manual_clock();
    let because = nonce("the-user-said-no");
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("call-1", "cmd.run")]),
            Answer::Text(nonce("fine-then")),
        ],
        Arc::clone(&clock),
        Duration::from_millis(1),
    );
    let mut executor = StagedTools::new(
        vec![Act::Refuse(because.clone())],
        tools(),
        Arc::clone(&clock),
        Duration::from_millis(1),
    );
    let context = RecordingContext::default();
    let seen = Arc::clone(&model.seen);
    let mut recorder = Recorder::default();

    let outcome = run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("the task"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("a refusal is not a port failure and must not surface as one");

    assert!(
        matches!(outcome, Outcome::Answered { .. }),
        "the turn carried on after the refusal and should have ended on the answer: {outcome:?}"
    );

    let seen = seen.lock().expect("seen poisoned").clone();
    assert_eq!(seen[1].len(), 1, "the refusal should have become a result");
    assert_eq!(
        seen[1][0].content.as_str(),
        because,
        "the refusal's own sentence did not reach the model byte for byte"
    );
    assert!(
        !seen[1][0].failed,
        "a refusal is not a tool failure -- the tool did not run, and marking it failed would \
         tell the model something untrue about what happened"
    );

    let refusals: Vec<&Event> = recorder
        .events
        .iter()
        .filter(|event| matches!(event, Event::ToolRefused { .. }))
        .collect();
    assert_eq!(refusals.len(), 1, "one refusal was staged");
    assert!(
        !recorder
            .events
            .iter()
            .any(|event| matches!(event, Event::ToolCompleted { .. })),
        "nothing acted, so nothing should have reported completing"
    );
    match refusals[0] {
        Event::ToolRefused {
            because: said,
            name,
            ..
        } => {
            assert_eq!(said, &because, "the event paraphrased the refusal");
            assert_eq!(name, "cmd.run");
        }
        other => panic!("filtered for a refusal and got {other:?}"),
    }
}

/// The ceiling is the caller's, and it bounds exchanges with the model.
///
/// The mutant: a hard-coded bound. Three ceilings with one fixture means a
/// constant disagrees with at least two of the three.
#[tokio::test]
async fn the_ceiling_comes_from_the_caller_and_bounds_the_exchanges() {
    for staged in [1_u32, 2, 4] {
        let clock = manual_clock();
        // Always asks for a tool, so only the ceiling can stop it.
        let answers = (0..staged)
            .map(|_| Answer::Calls(vec![request("c", "fs.read")]))
            .collect();
        let acts = (0..staged).map(|_| Act::Return(nonce("out"))).collect();
        let model = StagedModel::new(answers, Arc::clone(&clock), Duration::ZERO);
        let mut executor = StagedTools::new(acts, tools(), Arc::clone(&clock), Duration::ZERO);
        let context = RecordingContext::default();
        let mut recorder = Recorder::default();

        let outcome = run::<_, _, _, _, _, StagedInner>(
            1,
            Start::Task("t"),
            ToolCallCeiling::new(staged).expect("non-zero"),
            ToolCalling::required(&model, "staged").expect("can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &context,
                clock: &*clock,
                redactor: &NothingHeld,
            },
            None,
            &mut [&mut recorder],
        )
        .await
        .expect("no port failed");

        match outcome {
            Outcome::Exhausted { rounds, calls, .. } => {
                assert_eq!(
                    rounds, staged,
                    "a ceiling of {staged} should stop after {staged} exchanges"
                );
                assert_eq!(
                    calls, staged,
                    "each exchange asked for one tool, so {staged} should have run"
                );
            }
            other => panic!("a model that never answers should exhaust the turn: {other:?}"),
        }

        let ended: Vec<&Event> = recorder
            .events
            .iter()
            .filter(|event| matches!(event, Event::TurnEnded { .. }))
            .collect();
        assert_eq!(ended.len(), 1, "a turn ends exactly once");
        assert!(
            matches!(
                ended[0],
                Event::TurnEnded {
                    ending: TurnEnding::CeilingReached,
                    ..
                }
            ),
            "the ending should say which way it went: {:?}",
            ended[0]
        );
    }
}

/// ADR-0009 D4: "A project with no `zaru.toml` runs the tool-call loop only."
///
/// Two mutants, and the second is the one that matters. Entering the inner
/// loop when none was supplied cannot be written, because there is nothing to
/// enter. Asking the model when one *was* supplied is the mutant this check
/// catches, and it catches it from both sides: the inner loop records the
/// tasks it was given, and the model records the prompts it was shown.
#[tokio::test]
async fn declared_validators_decide_whether_the_turn_is_an_iteration_or_a_tool_cycle() {
    // -- No manifest: the tool-call loop runs alone. ------------------------
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![Answer::Text(nonce("answer"))],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RecordingContext::default();
    let prompts = Arc::clone(&model.prompts);
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    assert_eq!(
        prompts.lock().expect("prompts poisoned").len(),
        1,
        "with no validators declared the model is asked, which is D4's \"runs the tool-call loop \
         only\""
    );
    assert!(
        !recorder.events.iter().any(|event| matches!(
            event,
            Event::TurnEnded {
                ending: TurnEnding::Iterated { .. },
                ..
            }
        )),
        "no iteration loop was supplied, so no turn can have ended in one"
    );

    // -- Declared validators: the iteration loop is the turn's body. --------
    let clock = manual_clock();
    let model = StagedModel::new(Vec::new(), Arc::clone(&clock), Duration::ZERO);
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RecordingContext::default();
    let inner = StagedInner::new(crate::iteration::Outcome::Succeeded {
        iterations: 3,
        total_elapsed: Duration::from_millis(9),
    });
    let tasks = Arc::clone(&inner.tasks);
    let prompts = Arc::clone(&model.prompts);
    let mut recorder = Recorder::default();

    let outcome = run(
        2,
        Start::Task("the declared task"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        Some(&inner),
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    assert_eq!(
        tasks.lock().expect("tasks poisoned").as_slice(),
        ["the declared task"],
        "the iteration loop should have been entered with the turn's task"
    );
    assert!(
        prompts.lock().expect("prompts poisoned").is_empty(),
        "the iteration loop was the turn's body, so this loop asked no model of its own -- the \
         inner loop has a generator for that, and asking both would ask twice for one turn"
    );
    assert!(
        matches!(
            outcome,
            Outcome::Iterated(crate::iteration::Outcome::Succeeded { iterations: 3, .. })
        ),
        "the inner loop's outcome should be the turn's: {outcome:?}"
    );
    let ended: Vec<&Event> = recorder
        .events
        .iter()
        .filter(|event| matches!(event, Event::TurnEnded { .. }))
        .collect();
    assert_eq!(
        ended[0],
        &Event::TurnEnded {
            n: 2,
            ending: TurnEnding::Iterated {
                iterations: 3,
                succeeded: true,
            },
            rounds: 0,
            elapsed: Duration::ZERO,
        },
        "the ending should carry the inner loop's own numbers"
    );
}

/// ADR-0010 D4: "An interrupted tool call is recorded as `Interrupted` and
/// **the model is told it did not complete**."
///
/// This is the second half, which had no carrier before `Turn::Resumed`
/// existed. The mutant: assembling `Turn::Initial` on a resumed turn, which
/// loses the interruption entirely and is exactly what happened before this
/// variant was added.
#[tokio::test]
async fn a_resumed_turn_tells_the_model_what_did_not_complete_and_iterates_nothing() {
    let clock = manual_clock();
    let line = nonce("fs.write /tmp/x  [OUTSIDE the working directory]");
    let interrupted = Interruption::of(Redacted::by(&NothingHeld, &line));
    let model = StagedModel::new(
        vec![Answer::Text(nonce("carrying on"))],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RecordingContext::default();
    let inner = StagedInner::new(crate::iteration::Outcome::Succeeded {
        iterations: 1,
        total_elapsed: Duration::ZERO,
    });
    let tasks = Arc::clone(&inner.tasks);
    let turns = Arc::clone(&context.turns);
    let prompts = Arc::clone(&model.prompts);
    let mut recorder = Recorder::default();

    run(
        4,
        Start::Resumed(&interrupted),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        // Supplied, and deliberately: a resumed turn must not enter it even
        // when a project declares validators, because there is no new task.
        Some(&inner),
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    let turns = turns.lock().expect("turns poisoned").clone();
    assert_eq!(turns.len(), 1, "one turn, one assembly");
    assert_eq!(
        turns[0],
        format!("resumed::{line}"),
        "the policy was handed the wrong turn variant, so the model was never told"
    );

    let shown = prompts.lock().expect("prompts poisoned").clone();
    assert_eq!(shown.len(), 1);
    assert!(
        shown[0].contains(&line),
        "the interrupted call's own line did not reach the prompt the model was shown: {:?}",
        shown[0]
    );

    assert!(
        tasks.lock().expect("tasks poisoned").is_empty(),
        "a resumed turn carries no task, so the iteration loop must not be entered -- entering \
         it would iterate on nothing"
    );
}

/// ADR-0013 D7: assembly happens at turn boundaries, never inside one.
///
/// The mutant: moving the `assemble` call inside the exchange loop, which is
/// the shape that reassembles between a tool result and the next model call.
/// Three exchanges then produce three assemblies rather than one.
#[tokio::test]
async fn the_context_is_assembled_once_at_the_turn_boundary_and_never_inside_the_turn() {
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("a", "fs.read")]),
            Answer::Calls(vec![request("b", "fs.read")]),
            Answer::Text(nonce("done")),
        ],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(
        vec![Act::Return(nonce("one")), Act::Return(nonce("two"))],
        tools(),
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let context = RecordingContext::default();
    let turns = Arc::clone(&context.turns);
    let seen = Arc::clone(&model.seen);
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    assert_eq!(
        turns.lock().expect("turns poisoned").len(),
        1,
        "three exchanges happened in one turn and the context was assembled more than once, \
         which is the rewrite ADR-0013 D7 forbids inside a turn"
    );
    let seen = seen.lock().expect("seen poisoned").clone();
    assert_eq!(seen.len(), 3, "three exchanges were staged");
    assert_eq!(
        seen.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "results should accumulate across the turn rather than being replaced each round"
    );
}

/// ADR-0008 D3's one-emission requirement, stated for this stream.
///
/// The mutant: emitting to only the first sink. The count is asserted against
/// what the fixture was staged with rather than against a second reading
/// taken through the loop.
#[tokio::test]
async fn two_dissimilar_subscribers_receive_the_same_events_from_one_emission() {
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("a", "fs.read"), request("b", "cmd.run")]),
            Answer::Text(nonce("done")),
        ],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(
        vec![Act::Return(nonce("one")), Act::Refuse(nonce("no"))],
        tools(),
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();
    let mut projector = Projector::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder, &mut projector],
    )
    .await
    .expect("no port failed");

    // Staged: 1 TurnStarted + 2 ModelResponded + 2 ToolRequested
    //       + 2 ToolPermissionDecided + 1 ToolCompleted + 1 ToolRefused
    //       + 1 TurnEnded = 10. Counted from the staging, not from either sink.
    let staged = 10;
    assert_eq!(
        recorder.events.len(),
        staged,
        "the recording sink saw a different number of events from the {staged} the fixture \
         stages: {:?}",
        recorder.events.iter().map(tag).collect::<Vec<_>>()
    );
    assert_eq!(
        projector.tags.len(),
        staged,
        "the projecting sink saw a different number of events from the {staged} staged"
    );
    let from_recorder: Vec<&'static str> = recorder.events.iter().map(tag).collect();
    assert_eq!(
        from_recorder, projector.tags,
        "two subscribers disagreed about what happened, in order"
    );
}

/// ADR-0008 D6's elapsed time, stated for the outer loop.
///
/// The mutant: reading the machine's clock instead of the caller's. The
/// staged costs are distinct and exact, so any real clock disagrees.
#[tokio::test]
async fn elapsed_comes_from_the_callers_clock_and_not_from_the_machines() {
    let clock = manual_clock();
    let model_cost = Duration::from_millis(17);
    let tool_cost = Duration::from_millis(41);
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("a", "fs.read")]),
            Answer::Text(nonce("done")),
        ],
        Arc::clone(&clock),
        model_cost,
    );
    let mut executor = StagedTools::new(
        vec![Act::Return(nonce("out"))],
        tools(),
        Arc::clone(&clock),
        tool_cost,
    );
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    for event in &recorder.events {
        match event {
            Event::ModelResponded { elapsed, round, .. } => assert_eq!(
                *elapsed, model_cost,
                "exchange {round} should have cost exactly what the staged clock was advanced by"
            ),
            Event::ToolCompleted { elapsed, .. } => assert_eq!(
                *elapsed, tool_cost,
                "the call should have cost exactly what the staged clock was advanced by"
            ),
            Event::TurnEnded { elapsed, .. } => assert_eq!(
                *elapsed,
                model_cost * 2 + tool_cost,
                "the turn's elapsed should be the sum of what happened inside it"
            ),
            _ => {}
        }
    }
}

/// A turn whose context will not assemble carries both numbers out, and the
/// model is never asked.
///
/// # The two halves, and why neither alone would do
///
/// [ADR-0013] D7's window-pressure route reaches *this* loop as well as the
/// iteration loop's, and until 2026-09-15 it left here as a `PortFailure`
/// holding a **string**: the refusal's two numbers were rendered into prose by
/// `to_string()` and there was nothing structured left for `zaru-cli` to
/// classify by, so a reader whose own `provider.<kind>.context_tokens` was too
/// small was told they had found a bug in the harness. **So the first half is
/// that the numbers survive the trip**, asserted as the two integers the
/// policy refused with rather than as a substring of a sentence.
///
/// **The second half is that the model was never asked**, and it is what makes
/// the arrangement checkable without a provider at all: assembly happens at
/// the turn boundary and the socket only after it, so a turn that cannot
/// assemble spends nothing. `StagedModel` records every prompt it is handed,
/// and the assertion is that it recorded none.
///
/// The mutant: folding the refusal back into a `PortFailure`, which loses the
/// numbers; and assembling after the first exchange rather than before it,
/// which would spend a model call on a turn that cannot run.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[tokio::test]
async fn a_context_that_will_not_fit_carries_both_numbers_out_and_asks_no_model() {
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![Answer::Text(nonce("an answer no turn should reach"))],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RefusingContext {
        refusal: ContextRefusal::WindowExceeded {
            needed: 9_001,
            window: 4_096,
        },
    };
    let mut recorder = Recorder::default();

    let error = run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect_err("a turn that cannot assemble has no outcome to return");

    match error {
        ToolCallError::ContextWindowExceeded { needed, window } => {
            assert_eq!(
                (needed, window),
                (9_001, 4_096),
                "the refusal's own two numbers did not survive the trip out of the loop, so \
                 nothing downstream can say by how much the window was missed"
            );
        }
        ToolCallError::Port { port, failure, .. } => panic!(
            "a window that will not fit left as a {} port failure saying {failure:?} -- the \
             numbers are prose again and a classifier has nothing to read",
            port.as_str()
        ),
    }

    assert!(
        model.prompts.lock().expect("prompts poisoned").is_empty(),
        "the model was asked {:?} on a turn whose context never assembled; assembly is the turn \
         boundary and nothing should be spent past it",
        model.prompts.lock().expect("prompts poisoned")
    );
}

/// ADR-0008 D5's three-way split, stated for this loop: a port failure is an
/// error, and neither an answer nor an exhaustion.
///
/// The mutant: swallowing a port failure into `Outcome::Stopped`, which would
/// report an environmental failure as the model choosing to stop.
#[tokio::test]
async fn a_port_failure_is_an_error_and_neither_an_answer_nor_an_exhaustion() {
    for (which, answers, acts) in [
        (
            PortKind::Model,
            vec![Answer::Fail(nonce("provider unreachable"))],
            vec![],
        ),
        (
            PortKind::Tools,
            vec![Answer::Calls(vec![request("a", "fs.read")])],
            vec![Act::Fail(nonce("the sink refused"))],
        ),
    ] {
        let clock = manual_clock();
        let model = StagedModel::new(answers, Arc::clone(&clock), Duration::ZERO);
        let mut executor = StagedTools::new(acts, tools(), Arc::clone(&clock), Duration::ZERO);
        let context = RecordingContext::default();
        let mut recorder = Recorder::default();

        let error = run::<_, _, _, _, _, StagedInner>(
            1,
            Start::Task("t"),
            roomy(),
            ToolCalling::required(&model, "staged").expect("can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &context,
                clock: &*clock,
                redactor: &NothingHeld,
            },
            None,
            &mut [&mut recorder],
        )
        .await
        .expect_err(&format!(
            "a failing {} port should be an error rather than an outcome",
            which.as_str()
        ));

        match error {
            ToolCallError::Port { port, round, .. } => {
                assert_eq!(port, which, "the error named the wrong port");
                assert_eq!(round, 1, "it failed on the first exchange");
            }
            // No staging here refuses on a window, so reaching this arm means
            // a failing port was reported as a context that would not fit --
            // a different register, and a different class in `zaru-cli`.
            ToolCallError::ContextWindowExceeded { needed, window } => panic!(
                "a failing {} port was reported as a window that will not fit, {needed} needed \
                 against {window} allowed, and nothing staged here refuses on a window",
                which.as_str()
            ),
        }
        assert!(
            !recorder
                .events
                .iter()
                .any(|event| matches!(event, Event::TurnEnded { .. })),
            "a turn that could not run did not end, and saying it ended would put a port failure \
             in the same register as the mechanism working"
        );
    }
}

/// The model answers, and nothing follows the turn's end.
///
/// The mutant: emitting after `TurnEnded`, or going round again on a `Text`.
#[tokio::test]
async fn an_answer_ends_the_turn_and_no_event_follows_it() {
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![Answer::Text(nonce("hello"))],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    let tags: Vec<&'static str> = recorder.events.iter().map(tag).collect();
    assert_eq!(
        tags,
        vec!["TurnStarted", "ModelResponded", "TurnEnded"],
        "an answered turn's stream should be exactly these three, in this order"
    );
}

/// A stop is reported as itself.
///
/// The mutant: folding `Stopped` into `Answered` with an empty text, which
/// tells a consumer the model answered and said nothing.
#[tokio::test]
async fn a_stop_is_reported_as_itself_and_is_neither_an_answer_nor_an_exhaustion() {
    let clock = manual_clock();
    let reason = nonce("length");
    let model = StagedModel::new(
        vec![Answer::Stopped(reason.clone())],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(Vec::new(), tools(), Arc::clone(&clock), Duration::ZERO);
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();

    let outcome = run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("a model that stopped is not a port that failed");

    match outcome {
        Outcome::Stopped { reason: said, .. } => {
            assert_eq!(said, reason, "the provider's own words were not carried");
        }
        other => panic!("a stop should be reported as itself: {other:?}"),
    }
    assert!(
        recorder.events.iter().any(|event| matches!(
            event,
            Event::TurnEnded {
                ending: TurnEnding::Stopped,
                ..
            }
        )),
        "the stream should say the turn stopped rather than that it answered"
    );
}

/// ADR-0011 D4: "Mode may remove the prompt; it never removes the record."
///
/// The decision is reported for every call whichever way it went, so a
/// consumer sees it at every mode. The mutant: emitting the decision only
/// when a prompt was raised.
#[tokio::test]
async fn the_decision_is_reported_for_every_call_whichever_way_it_went() {
    let clock = manual_clock();
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![
                request("a", "fs.read"),
                request("b", "cmd.run"),
                request("c", "fs.read"),
            ]),
            Answer::Text(nonce("done")),
        ],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(
        vec![
            Act::Return(nonce("ok")),
            Act::Refuse(nonce("declined")),
            Act::Failed(nonce("the tool itself failed")),
        ],
        tools(),
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    let decided: Vec<(bool, &str)> = recorder
        .events
        .iter()
        .filter_map(|event| match event {
            Event::ToolPermissionDecided {
                permitted,
                statement,
                ..
            } => Some((*permitted, statement.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        decided.len(),
        3,
        "three calls were staged, so three decisions are owed -- one per call, whichever way it \
         went: {decided:?}"
    );
    assert_eq!(
        decided.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        vec![true, false, true],
        "the second call was refused and the other two acted"
    );
    for (_, statement) in &decided {
        assert!(
            !statement.is_empty(),
            "a decision with no sentence tells a consumer nothing about what was decided"
        );
    }
}

/// ADR-0008's open trigger clause 6 arrives here for a third time, and this
/// stream deliberately does not become a fourth place a secret can land.
///
/// The mutant: carrying the content on the event rather than its length,
/// which is what would write a tool's output into `transcript.jsonl` a second
/// time and put it on a path no redaction decision covers.
#[tokio::test]
async fn the_stream_carries_a_byte_count_and_never_the_tools_output() {
    let clock = manual_clock();
    let produced = nonce("SECRET-SENTINEL");
    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("a", "fs.read")]),
            Answer::Text(nonce("done")),
        ],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(
        vec![Act::Return(produced.clone())],
        tools(),
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let context = RecordingContext::default();
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    let completed = recorder
        .events
        .iter()
        .find_map(|event| match event {
            Event::ToolCompleted { content_bytes, .. } => Some(*content_bytes),
            _ => None,
        })
        .expect("one call completed");
    assert_eq!(
        completed,
        produced.len(),
        "the byte count should be the output's actual length"
    );

    // Both the raw value and an ASCII-only core of it, because a debug
    // rendering escapes the nonce's combining mark and an absence assertion
    // against the raw value alone is blind to that -- library verification
    // lessons §50, reproduced here rather than inherited.
    let core = "SECRET-SENTINEL";
    let rendered = format!("{:?}", recorder.events);
    assert!(
        !rendered.contains(&produced),
        "the tool's output reached the event stream, which is a path no redaction decision covers"
    );
    assert!(
        !rendered.contains(core),
        "the tool's output reached the event stream in an escaped form: {rendered}"
    );
}

/// The set the model is offered is the set the executor will accept.
///
/// The mutant: a second list of descriptors kept beside the executor, which
/// is the rule-in-two-places shape that lets a model be offered a tool the
/// surface refuses.
#[tokio::test]
async fn the_tools_the_model_is_offered_come_from_the_executor_itself() {
    let clock = manual_clock();
    let staged = tools();
    let model = StagedModel::new(
        vec![Answer::Text(nonce("done"))],
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let mut executor = StagedTools::new(
        Vec::new(),
        staged.clone(),
        Arc::clone(&clock),
        Duration::ZERO,
    );
    let context = RecordingContext::default();
    let offered = Arc::clone(&model.offered);
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("t"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("no port failed");

    assert_eq!(
        offered.lock().expect("offered poisoned").as_slice(),
        staged.as_slice(),
        "the model was offered a different set from the one the executor declares"
    );
}

// --- ADR-0008 trigger clause 6, decided 2026-09-05 -------------------------

#[tokio::test]
async fn a_refusals_sentence_is_redacted_before_it_becomes_the_next_turns_content() {
    // The third of the paths that decision names, at the point this crate
    // owns. A completed call's content was redacted by the executing surface,
    // where the raw capture and the transcript both live; a refusal's
    // sentence is composed here and can quote the target the model asked for,
    // so `for_the_model` is where it passes the port.
    //
    // The event stream is asserted to keep the raw sentence in the same
    // check, because that is what ADR-0010's transcript is written from and
    // the difference between redacting a prompt and redacting a record is the
    // whole shape of this decision.
    let clock = manual_clock();
    let secret = staged_secret();
    // This crate has two notions of an ASCII core and they are not the
    // same function: `tool_call::fixtures`' strips a known nonce tail, and
    // the redaction fixtures' takes the ASCII prefix. The staged secret comes
    // from the second, so the core asserted here must come from the second
    // too -- a mismatch there is what made an earlier draft of the product
    // check fail against its own staging rather than against the code.
    let core = redaction_ascii_core(&secret);
    assert!(
        !core.is_empty() && core != secret,
        "the staged secret must have an ASCII core distinct from itself"
    );
    let because = format!("the user declined `cmd.run curl -H 'Bearer {secret}'`");
    let holding = HoldingOne::new(secret.clone(), "work");

    let model = StagedModel::new(
        vec![
            Answer::Calls(vec![request("call-1", "cmd.run")]),
            Answer::Text(nonce("fine-then")),
        ],
        Arc::clone(&clock),
        Duration::from_millis(1),
    );
    let mut executor = StagedTools::new(
        vec![Act::Refuse(because.clone())],
        tools(),
        Arc::clone(&clock),
        Duration::from_millis(1),
    );
    let context = RecordingContext::default();
    let seen = Arc::clone(&model.seen);
    let mut recorder = Recorder::default();

    run::<_, _, _, _, _, StagedInner>(
        1,
        Start::Task("the task"),
        roomy(),
        ToolCalling::required(&model, "staged").expect("can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &context,
            clock: &*clock,
            redactor: &holding,
        },
        None,
        &mut [&mut recorder],
    )
    .await
    .expect("a refusal is not a port failure");

    let seen = seen.lock().expect("seen poisoned").clone();
    let content = seen[1][0].content.as_str();
    assert!(
        !content.contains(&secret),
        "a held value reached the model in a refusal's sentence: {content:?}"
    );
    assert!(
        !content.contains(core),
        "a held value's ASCII core reached the model in a refusal's sentence, \
         so an escaping renderer would publish it: {content:?}"
    );
    assert!(
        content.contains("<redacted: work>"),
        "nothing marks where the value was, and a `for_the_model` that \
         returned an empty result would satisfy both assertions above on its \
         own: {content:?}"
    );

    // The other half, and it is the one whose sign is inverted. The event is
    // what ADR-0010 D2's transcript is written from, and that record's own
    // Negative section says the transcript "contain\[s\] whatever the session
    // contained". Redacting here would be redacting the record.
    let refusal = recorder
        .events
        .iter()
        .find_map(|event| match event {
            Event::ToolRefused { because, .. } => Some(because.clone()),
            _ => None,
        })
        .expect("one refusal was staged");
    assert_eq!(
        refusal, because,
        "the event stream must carry the refusal's raw sentence, because that \
         is what the transcript is written from and ADR-0010 keeps whatever \
         the session contained"
    );
}
