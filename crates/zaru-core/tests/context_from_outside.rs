// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives a long session through the door a caller uses.
//!
//! Every other check on context assembly lives inside `zaru-core` and reaches
//! its subject directly. That proves the mechanism and says nothing about
//! whether the mechanism can be reached: a capability whose only callers are
//! unit tests is a capability nobody has been shown able to use, and the
//! missing piece is invisible to a green suite because there is no mutant for
//! a declaration that was never made public.
//!
//! So this file implements both context ports and all five loop ports using
//! only what `zaru-core` exports, with no terminal, no network and no model,
//! and runs a session long enough to cross the threshold, compact, drop an
//! attachment and finally refuse an iteration that would not fit.

use core::time::Duration;
use std::sync::Mutex;
use zaru_core::context::{
    Announcement, Context, ContextLimits, ContextWindow, Exchange, ExchangeKind, ItemId,
    IterationRecord, Layer, PrefixParts, PressureThreshold, Span, StablePrefix, Summariser,
    TokenCounter,
};
use zaru_core::context::{AttachedItem, Usage};
use zaru_core::iteration::{
    Ceiling, Clock, ContextPolicy, ContextRefusal, Event, EventSink, ExecutionOutcome, Executor,
    ExhaustionReason, Generated, Generator, Limits, Outcome, PortFailure, Ports, Prompt, State,
    TruncationBudget, Turn, ValidatorOutcome, ValidatorReport, Validators, run,
};
use zaru_core::redaction::{Redacted, Redactor};

/// A nonce no implementation could produce without carrying it.
const NONCE: &str = "outside-caller-6b1f";

/// A text of exactly `words` words, carrying the nonce, a newline and a
/// non-ASCII character.
fn text_of(label: &str, words: usize) -> String {
    let mut out = format!("{NONCE}-{label}");
    for i in 1..words {
        out.push(if i % 3 == 0 { '\n' } else { ' ' });
        out.push_str(&format!("słowo-{i}"));
    }
    out
}

/// One token per whitespace-separated word, so every number this file asserts
/// is a number this file chose.
struct Words;

impl TokenCounter for Words {
    fn count(&self, text: &str) -> u64 {
        text.split_whitespace().count() as u64
    }
}

/// Summarises to a fixed size and keeps every span it was handed.
struct Shrink {
    words: usize,
    spans: Mutex<Vec<Span>>,
}

impl Shrink {
    fn to(words: usize) -> Self {
        Self {
            words,
            spans: Mutex::new(Vec::new()),
        }
    }

    fn spans(&self) -> Vec<Span> {
        self.spans.lock().expect("spans poisoned").clone()
    }
}

impl Summariser for Shrink {
    async fn summarise(&self, span: &Span) -> Result<String, PortFailure> {
        self.spans
            .lock()
            .expect("spans poisoned")
            .push(span.clone());
        Ok(text_of("summary", self.words))
    }
}

/// The adapter `zaru-cli` will write: a loop-facing context policy over a
/// context it holds by shared reference and therefore cannot compact.
struct Policy<'a> {
    context: &'a Context,
    counter: &'a Words,
}

impl ContextPolicy for Policy<'_> {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let tail = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
            Turn::Resumed { interrupted } => format!(
                "the previous session was interrupted and this call never completed: {}",
                interrupted.call()
            ),
        };
        Ok(Prompt::new(Redacted::by(
            &NothingHeld,
            self.context
                .assemble(self.counter, &NothingHeld, &tail)?
                .as_str(),
        )))
    }
}

struct FrozenClock;

impl Clock for FrozenClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<String>>);

impl Recorder {
    fn prompts(&self) -> Vec<String> {
        self.0.lock().expect("prompts poisoned").clone()
    }
}

impl Generator for Recorder {
    type Candidate = String;

    async fn generate(&self, prompt: &Prompt) -> Result<Generated<Self::Candidate>, PortFailure> {
        self.0
            .lock()
            .expect("prompts poisoned")
            .push(prompt.as_str().to_owned());
        Ok(Generated {
            candidate: format!("{NONCE}-candidate"),
            tokens: 1,
        })
    }
}

struct Inert;

impl Executor for Inert {
    type Candidate = String;

    async fn execute(&self, _candidate: &Self::Candidate) -> Result<ExecutionOutcome, PortFailure> {
        Ok(ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

struct AlwaysPasses;

impl Validators for AlwaysPasses {
    async fn evaluate(
        &self,
        _execution: &ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        Ok(vec![ValidatorReport {
            name: "test".to_owned(),
            outcome: ValidatorOutcome::Passed,
            detail: String::new(),
        }])
    }
}

#[derive(Default)]
struct Collect(Vec<Event>);

impl EventSink for Collect {
    fn emit(&mut self, event: &Event) {
        self.0.push(event.clone());
    }
}

/// Everything one iteration's failure is remembered by.
fn iteration_record(n: u32) -> IterationRecord {
    IterationRecord {
        n,
        tried: text_of(&format!("candidate-{n}"), 3),
        failed: text_of(&format!("validator-{n}"), 2),
        why: text_of(&format!("because-{n}"), 2),
        verbatim: format!("{NONCE}\niteration {n}: left ≠ right\n  left: 1\n  right: 2\n"),
    }
}

#[tokio::test]
async fn a_caller_outside_this_crate_drives_a_long_session_to_a_compaction_and_a_drop() {
    const TURNS: u32 = 10;
    const EXCHANGE_WORDS: usize = 12;
    const SUMMARY_WORDS: usize = 2;
    const PREFIX_WORDS: usize = 5;

    let counter = Words;
    let summariser = Shrink::to(SUMMARY_WORDS);

    // Layers 1 to 4, assembled once. Nothing after this line can change them:
    // the value has no method that does.
    let prefix = StablePrefix::assembled_once(PrefixParts {
        system_prompt_and_persona: text_of("persona", PREFIX_WORDS),
        grounding: text_of("grounding", PREFIX_WORDS),
        relationship_memory: text_of("memory", PREFIX_WORDS),
        project_manifest_summary: text_of("manifest", PREFIX_WORDS),
    });
    let opened_with = prefix.as_str().to_owned();
    assert!(
        !opened_with.is_empty(),
        "the staged prefix must have content, or byte-identity below is trivial"
    );

    let limits = ContextLimits::new(
        ContextWindow::new(4_000).expect("a window of four thousand tokens"),
        PressureThreshold::new(80).expect("a threshold of eighty tokens"),
    )
    .expect("a threshold below the window");
    let mut context = Context::opened(prefix, limits, 0);

    // Per compaction: the announcement's three numbers, and the span's texts
    // as this file read them off the raw span.
    let mut compacted: Vec<(u32, u64, u64, Vec<String>)> = Vec::new();
    let mut span_texts: Vec<String> = Vec::new();
    let mut dropped: Vec<(String, String, String)> = Vec::new();
    let mut spans_seen = 0_usize;

    for turn in 1..=TURNS {
        context.attach(
            AttachedItem::new(
                ItemId::new("zaru", format!("adrs/{NONCE}-{turn}")).expect("a complete identity"),
                text_of(&format!("attachment-{turn}"), 7),
                "[[",
            )
            .expect("a staged attachment says how to re-attach itself"),
        );
        context.record_exchange(Exchange::verbatim(text_of(
            &format!("exchange-{turn}"),
            EXCHANGE_WORDS,
        )));
        context.record_iteration(iteration_record(turn));

        // What layer 6 held before this turn's compaction, so the raw span
        // can be compared against something that did not travel through the
        // compaction itself.
        let before_texts: Vec<String> = context
            .exchanges()
            .iter()
            .map(|exchange| exchange.as_str().to_owned())
            .collect();

        let compaction = context
            .compact(&summariser, &counter, &NothingHeld)
            .await
            .expect("the staged summariser does not fail");

        if let Some(span) = compaction.raw.as_ref() {
            spans_seen += 1;
            let taken: Vec<String> = span
                .exchanges()
                .iter()
                .map(|exchange| exchange.as_str().to_owned())
                .collect();
            assert_eq!(
                taken,
                before_texts[..taken.len()].to_vec(),
                "turn {turn}: the raw span must be the OLDEST exchanges, byte for byte, so the \
                 transcript keeps what the model stopped seeing"
            );
            assert_eq!(
                context.exchanges()[0].kind(),
                ExchangeKind::Summary,
                "turn {turn}: the summary takes the place the span occupied"
            );
            span_texts = taken;
        }

        for announcement in &compaction.announcements {
            match announcement {
                Announcement::Compacted {
                    turns,
                    before,
                    after,
                } => compacted.push((*turns, *before, *after, span_texts.clone())),
                Announcement::AttachmentDropped {
                    identity,
                    how_to_reattach,
                } => dropped.push((
                    identity.workspace().to_owned(),
                    identity.path().to_owned(),
                    how_to_reattach.clone(),
                )),
            }
        }

        let assembled = context
            .assemble(&counter, &NothingHeld, &text_of(&format!("tail-{turn}"), 3))
            .expect("the staged window is roomy");

        assert!(
            assembled.as_str().starts_with(&opened_with),
            "turn {turn}: trigger clause 1 — layers 1 to 4 must lead every assembled context, or \
             prompt caching has nothing to match"
        );
        assert_eq!(
            context.prefix().as_str(),
            opened_with,
            "turn {turn}: layers 1 to 4 are no longer the bytes the session opened with"
        );

        let usage: Usage = assembled.usage();
        assert_eq!(
            usage.window(),
            4_000,
            "turn {turn}: usage reports its window"
        );
        assert_eq!(
            usage.used() + usage.remaining(),
            usage.window(),
            "turn {turn}: what is used and what is left must account for the whole window"
        );
    }

    // Assert the staging. Without these, a session in which nothing crossed
    // the threshold would satisfy every clause above by doing nothing.
    assert!(
        spans_seen >= 1,
        "the staged session was meant to cross the threshold and compacted {spans_seen} times"
    );
    assert_eq!(
        compacted.len(),
        spans_seen,
        "ADR-0013 D3: one compaction is one announcement, and there were {spans_seen} compactions \
         against {} announcements",
        compacted.len()
    );
    assert_eq!(
        summariser.spans().len(),
        spans_seen,
        "one compaction is one model call"
    );
    for (turns, before, after, texts) in &compacted {
        assert!(
            *turns >= 1,
            "an announcement of no turns is not a compaction"
        );
        assert_eq!(
            *turns as usize,
            texts.len(),
            "the announcement counts a different number of exchanges from the span it replaced"
        );
        // Counted here, off the span this file read, rather than taken from
        // the announcement: the two arms must not both come through the
        // compaction. A span mixes staged exchanges with summaries a previous
        // compaction left behind, so it is a sum and not a multiplication.
        let counted: u64 = texts
            .iter()
            .map(|text| text.split_whitespace().count() as u64)
            .sum();
        assert_eq!(
            *before,
            counted,
            "`before` must be what the replaced exchanges actually cost. This file counted \
             {counted} across {} of them, of which the staged verbatim ones are \
             {EXCHANGE_WORDS} tokens each",
            texts.len()
        );
        assert_eq!(
            *after, SUMMARY_WORDS as u64,
            "`after` must be what the summary costs"
        );
    }

    assert!(
        !dropped.is_empty(),
        "the staged session was meant to run out of layer 6 and reach layer 5"
    );
    assert_eq!(
        dropped[0].1,
        format!("adrs/{NONCE}-1"),
        "ADR-0013 D1 discards attachments last and oldest first"
    );
    for (workspace, path, reattach) in &dropped {
        assert_eq!(
            workspace, "zaru",
            "a path with no workspace resolves nowhere"
        );
        assert!(!path.is_empty());
        assert_eq!(
            reattach, "[[",
            "ADR-0013 D4: the announcement states how to re-attach"
        );
    }
}

#[tokio::test]
async fn an_iteration_that_would_exceed_the_window_is_exhausted_and_not_an_error() {
    // ADR-0013 D7's second route, driven through the loop by a caller. The
    // context is far too large for the window and compaction is not on the
    // table, because the loop assembles at an iteration boundary and D7
    // confines compaction to turn boundaries.
    let counter = Words;
    let window = 20_u64;
    let mut context = Context::opened(
        StablePrefix::assembled_once(PrefixParts {
            system_prompt_and_persona: text_of("persona", 30),
            ..PrefixParts::default()
        }),
        ContextLimits::new(
            ContextWindow::new(window).expect("window"),
            PressureThreshold::new(10).expect("threshold"),
        )
        .expect("a threshold below the window"),
        0,
    );
    for n in 1..=4 {
        context.record_exchange(Exchange::verbatim(text_of(&format!("exchange-{n}"), 20)));
    }
    let before = context.exchanges().len();

    let policy = Policy {
        context: &context,
        counter: &counter,
    };
    let generator = Recorder::default();
    let mut events = Collect::default();

    let outcome = run(
        "make the tests pass",
        Limits {
            ceiling: Ceiling::new(5).expect("ceiling"),
            budget: TruncationBudget::new(512).expect("budget"),
        },
        Ports {
            generator: &generator,
            executor: &Inert,
            validators: &AlwaysPasses,
            context: &policy,
            clock: &FrozenClock,
            redactor: &NothingHeld,
        },
        &mut [&mut events],
    )
    .await
    .expect("ADR-0008 D5: exhaustion is not an error, so this must not be Err");

    match outcome {
        Outcome::Exhausted {
            iterations,
            reason: ExhaustionReason::ContextWindowExceeded { needed, window: w },
            last_failure,
        } => {
            assert_eq!(iterations, 0, "nothing reached an evaluation");
            assert_eq!(
                w, window,
                "the refusal reports the window it measured against"
            );
            assert!(needed > w, "a refusal must need MORE than the window");
            assert_eq!(
                last_failure, None,
                "no validator ran, so there is no failure text and an empty string would say one \
                 ran and said nothing"
            );
        }
        other => panic!(
            "ADR-0013 D7: an iteration that would exceed the window fails as exhausted with a \
             clear reason, and this was {other:?}"
        ),
    }

    assert!(
        generator.prompts().is_empty(),
        "nothing may be generated from a context that did not fit"
    );
    assert_eq!(
        events.0.last().map(Event::state),
        Some(State::Exhausted),
        "the run ends in the exhausted state, and its events were {:?}",
        events.0.iter().map(Event::state).collect::<Vec<_>>()
    );
    assert_eq!(
        context.exchanges().len(),
        before,
        "D7: the iteration fails rather than continuing on a rewritten context"
    );
}

#[test]
fn the_layers_a_caller_sees_are_d1s_seven_in_d1s_order() {
    // The population comes from the crate's own export rather than from a
    // list retyped here, so a layer that stops being public is a compile
    // error and a layer that is added without an order is a failure.
    assert_eq!(Layer::ALL.len(), 7, "ADR-0013 D1 names seven layers");
    let prefix: Vec<Layer> = Layer::ALL
        .into_iter()
        .filter(|layer| layer.in_stable_prefix())
        .collect();
    assert_eq!(prefix.len(), 4, "D1's stable prefix is layers 1 to 4");
    assert_eq!(
        prefix,
        Layer::ALL[..4].to_vec(),
        "the stable prefix must be the FIRST four, or a cache has nothing to match"
    );
}

/// A redactor holding nothing, which is therefore the identity.
///
/// Every outside caller has to supply one, because a `Prompt` can only be
/// built from text that has passed [ADR-0008] clause 6's port — which is the
/// whole point of that type. It is declared here rather than shared because
/// an integration test cannot see another crate's test tree and [ADR-0003] D8
/// forbids the dependency that would let it, the same cost `zaru-cli`'s
/// Nuclear Notes fixture server already pays.
///
/// Holding nothing is also the **discriminating** arm: a check asserting that
/// a value is absent from a prompt is worthless unless the same run with
/// nothing held carries that value through byte for byte.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}
