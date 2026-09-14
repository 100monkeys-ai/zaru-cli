// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What context assembly is asserted to do.
//!
//! Two rules govern the numbers here, both written against a failure shape
//! the testing contract names.
//!
//! **Counts are asserted against what the fixture was staged with.** Every
//! staged text is built with a chosen word count and the staged counter costs
//! one token per word, so the number in an assertion is a number this file
//! chose rather than a second reading taken through the code under test.
//!
//! **Every staged text carries a nonce, a newline and a non-ASCII
//! character**, so no implementation can produce it without carrying it.

use super::fixtures::{
    FrozenClock, InertExecutor, InertGenerator, NONCE, PolicyOver, ScriptedValidators,
    StagedSummariser, WordCounter, staged_attachment, staged_exchange, staged_iteration,
    staged_text, verbatim_failure_for,
};
use crate::context::announcement::Announcement;
use crate::context::assembly::Context;
use crate::context::exchange::{Exchange, ExchangeKind};
use crate::context::history::{self, IterationRecord};
use crate::context::item::{AttachedItem, ItemId, ItemRefused};
use crate::context::layer::{Layer, Retention};
use crate::context::limits::{ContextLimits, ContextWindow, LimitsRefused, PressureThreshold};
use crate::context::prefix::{PrefixParts, StablePrefix};
use crate::iteration::machine::run;
use crate::iteration::{Ceiling, Limits, Ports, TruncationBudget};
use crate::redaction::fixtures::{HoldingOne, NothingHeld, ascii_core};

/// How many words each of the four prefix layers is staged with.
const PREFIX_WORDS: usize = 6;

fn limits(window: u64, threshold: u64) -> ContextLimits {
    ContextLimits::new(
        ContextWindow::new(window).expect("window"),
        PressureThreshold::new(threshold).expect("threshold"),
    )
    .expect("a threshold at or below the window is usable")
}

fn staged_prefix() -> StablePrefix {
    StablePrefix::assembled_once(PrefixParts {
        system_prompt_and_persona: staged_text("persona", PREFIX_WORDS),
        grounding: staged_text("grounding", PREFIX_WORDS),
        relationship_memory: staged_text("memory", PREFIX_WORDS),
        project_manifest_summary: staged_text("manifest", PREFIX_WORDS),
    })
}

// --- D1: the layers and their precedence -----------------------------------

#[test]
fn every_layer_the_enum_declares_appears_once_in_all_and_all_is_in_d1_order() {
    // The population comes from the enum rather than from a list retyped
    // beside this check, so an eighth layer fails to compile here.
    const fn accounted_for(layer: Layer) -> bool {
        match layer {
            Layer::SystemPromptAndPersona
            | Layer::Grounding
            | Layer::RelationshipMemory
            | Layer::ProjectManifestSummary
            | Layer::UserAttachments
            | Layer::ConversationAndToolResults
            | Layer::IterationHistory => true,
        }
    }

    for layer in Layer::ALL {
        assert!(accounted_for(layer));
        assert_eq!(
            Layer::ALL.iter().filter(|l| **l == layer).count(),
            1,
            "{layer:?} appears more than once in ALL"
        );
    }

    let mut sorted = Layer::ALL;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        Layer::ALL,
        "ALL must be in ascending order, because the derived Ord IS D1's precedence and a list \
         that disagrees with it is a second table"
    );

    // D1's stable prefix is the first four rows of its table, so every prefix
    // layer must sort before every discardable one. Without this, reordering
    // the enum could put an attachment ahead of the persona and every other
    // check here would still pass.
    let highest_prefix = Layer::ALL
        .iter()
        .filter(|l| l.in_stable_prefix())
        .max()
        .copied()
        .expect("D1 has a stable prefix");
    let lowest_discardable = Layer::ALL
        .iter()
        .filter(|l| !l.in_stable_prefix())
        .min()
        .copied()
        .expect("D1 has discardable layers");
    assert!(
        highest_prefix < lowest_discardable,
        "the stable prefix must come first: {highest_prefix:?} sorts at or after \
         {lowest_discardable:?}, so a cache would have nothing stable to match"
    );
}

#[test]
fn each_layer_keeps_the_retention_rule_d1_gives_it() {
    // D1's table, retyped here on purpose: this arm does not travel through
    // `Layer::retention`, so the two can disagree.
    let d1 = [
        (Layer::SystemPromptAndPersona, Retention::NeverDiscarded),
        (Layer::Grounding, Retention::NeverDiscarded),
        (Layer::RelationshipMemory, Retention::NeverDiscarded),
        (Layer::ProjectManifestSummary, Retention::NeverDiscarded),
        (Layer::UserAttachments, Retention::DiscardedLast),
        (Layer::ConversationAndToolResults, Retention::CompactedFirst),
        (Layer::IterationHistory, Retention::CompactedFirst),
    ];
    assert_eq!(d1.len(), Layer::ALL.len(), "D1's table lost a row");
    for (layer, retention) in d1 {
        assert_eq!(
            layer.retention(),
            retention,
            "ADR-0013 D1 gives {layer:?} the retention {retention:?}"
        );
        assert_eq!(
            layer.in_stable_prefix(),
            retention == Retention::NeverDiscarded,
            "the stable prefix is exactly what D1 never discards, and {layer:?} disagrees"
        );
    }
}

// --- Trigger clause 1: the stable prefix -----------------------------------

#[test]
fn the_prefix_renders_its_four_layers_in_d1_order_and_reports_each_back() {
    let prefix = staged_prefix();
    let rendered = prefix.as_str();

    let positions: Vec<usize> = ["persona", "grounding", "memory", "manifest"]
        .iter()
        .map(|label| {
            rendered
                .find(&format!("{NONCE}-{label}"))
                .unwrap_or_else(|| panic!("layer {label} is missing from the rendered prefix"))
        })
        .collect();
    let mut ascending = positions.clone();
    ascending.sort_unstable();
    assert_eq!(
        positions, ascending,
        "the four prefix layers must render in D1's order and rendered at {positions:?}"
    );

    for layer in Layer::ALL {
        assert_eq!(
            prefix.layer(layer).is_some(),
            layer.in_stable_prefix(),
            "{layer:?} is reported as part of the prefix and D1 disagrees"
        );
    }
}

#[tokio::test]
async fn layers_one_to_four_are_byte_identical_across_every_turn_of_a_long_session() {
    // Trigger clause 1. Twelve turns, with an attachment, an exchange and an
    // iteration added on every one, a compaction crossed partway through and
    // an attachment dropped — so the session genuinely changes underneath the
    // prefix rather than sitting still while the check watches it.
    const TURNS: u32 = 12;

    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    let prefix = staged_prefix();
    let expected: String = prefix.as_str().to_owned();
    let mut context = Context::opened(prefix, limits(400, 60), 0);

    let mut compactions = 0_usize;
    let mut drops = 0_usize;
    for turn in 1..=TURNS {
        context.attach(staged_attachment(turn, 9));
        context.record_exchange(staged_exchange(turn, 11));
        context.record_iteration(staged_iteration(turn));

        let compaction = context
            .compact(&summariser, &counter, &NothingHeld)
            .await
            .expect("the staged summariser does not fail here");
        for announcement in &compaction.announcements {
            match announcement {
                Announcement::Compacted { .. } => compactions += 1,
                Announcement::AttachmentDropped { .. } => drops += 1,
            }
        }

        let assembled = context
            .assemble(
                &counter,
                &NothingHeld,
                &staged_text(&format!("tail-{turn}"), 4),
            )
            .expect("the staged window admits this context");

        assert!(
            assembled.as_str().starts_with(&expected),
            "turn {turn}: the assembled context does not begin with the stable prefix, so a \
             prompt cache would have nothing to match"
        );
        assert_eq!(
            context.prefix().as_str(),
            expected,
            "turn {turn}: layers 1 to 4 are not byte-identical to the ones the session opened with"
        );
    }

    // Assert the staging, so this cannot pass over a session in which nothing
    // happened: a prefix is trivially identical across turns that changed
    // nothing underneath it.
    assert!(
        compactions >= 1,
        "the staged session was meant to cross the threshold and compacted {compactions} times"
    );
    assert!(
        drops >= 1,
        "the staged session was meant to drop an attachment and dropped {drops}"
    );
}

// --- Trigger clause 2: compaction ------------------------------------------

#[tokio::test]
async fn crossing_the_threshold_compacts_the_oldest_span_of_layer_six_first() {
    // Staged: five exchanges of ten tokens each, a prefix of twenty-four, and
    // a threshold twenty below the total, so the overage is covered by the
    // two oldest exchanges and no more.
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    let mut context = Context::opened(staged_prefix(), limits(400, 54), 0);
    for n in 1..=5 {
        context.record_exchange(staged_exchange(n, 10));
    }

    let compaction = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("the staged summariser does not fail here");

    let span = compaction
        .raw
        .as_ref()
        .expect("crossing the threshold compacts layer 6, so a raw span exists");
    let taken: Vec<&str> = span.exchanges().iter().map(Exchange::as_str).collect();
    assert!(
        taken.iter().enumerate().all(|(i, text)| {
            let n = u32::try_from(i).expect("small") + 1;
            text.contains(&format!("{NONCE}-exchange-{n}"))
        }),
        "the span must be the OLDEST exchanges, in order. It was {:?}",
        taken
            .iter()
            .map(|t| t.split_whitespace().next().unwrap_or(""))
            .collect::<Vec<_>>()
    );

    // What is left: the summary first, then every exchange the span did not
    // take, still in order.
    let left = context.exchanges();
    assert_eq!(
        left[0].kind(),
        ExchangeKind::Summary,
        "the summary replaces the span at the position the span occupied"
    );
    let survivors: Vec<u32> = ((span.len() as u32 + 1)..=5).collect();
    for (offset, n) in survivors.iter().enumerate() {
        assert!(
            left[offset + 1]
                .as_str()
                .contains(&format!("{NONCE}-exchange-{n}")),
            "exchange {n} should have survived at position {}, and the surviving texts were {:?}",
            offset + 1,
            left.iter()
                .map(|e| e.as_str().split_whitespace().next().unwrap_or(""))
                .collect::<Vec<_>>()
        );
    }

    // The span that crossed the port is the span that was removed. Asserted
    // against what the summariser recorded rather than against what the
    // context has left, so the two arms do not both travel through the
    // compaction.
    assert_eq!(summariser.calls(), 1, "one compaction is one model call");
    assert_eq!(
        summariser.spans()[0],
        *span,
        "the summariser was handed a different span from the one that was replaced"
    );
}

#[tokio::test]
async fn the_announcement_carries_the_counts_the_counter_measured() {
    // Staged so the arithmetic is the test's own. Four exchanges of ten
    // tokens beside a twenty-four-token prefix is sixty-four, and a threshold
    // of forty-four leaves an overage of twenty — which the two oldest
    // exchanges cover exactly. The summary costs three.
    const EXCHANGE_WORDS: usize = 10;
    const SUMMARY_WORDS: usize = 3;

    let counter = WordCounter;
    let summariser = StagedSummariser::costing(SUMMARY_WORDS);
    let prefix = staged_prefix();
    let mut context = Context::opened(prefix, limits(400, 44), 0);
    for n in 1..=4 {
        context.record_exchange(staged_exchange(n, EXCHANGE_WORDS));
    }

    let compaction = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("the staged summariser does not fail here");

    let compacted: Vec<&Announcement> = compaction
        .announcements
        .iter()
        .filter(|a| matches!(a, Announcement::Compacted { .. }))
        .collect();
    assert_eq!(
        compacted.len(),
        1,
        "ADR-0013 D3: compaction is announced ONCE. It was announced {} times",
        compacted.len()
    );
    let Announcement::Compacted {
        turns,
        before,
        after,
    } = compacted[0]
    else {
        unreachable!("filtered above")
    };
    let span_len = compaction.raw.as_ref().expect("a span was replaced").len();
    assert_eq!(
        *turns as usize, span_len,
        "the announcement counts a different number of exchanges from the span it replaced"
    );
    assert_eq!(
        *before,
        (span_len * EXCHANGE_WORDS) as u64,
        "`before` must be what the replaced exchanges actually cost: {span_len} exchanges of \
         {EXCHANGE_WORDS} tokens each, as the fixture staged them"
    );
    assert_eq!(
        *after, SUMMARY_WORDS as u64,
        "`after` must be what the summary costs, and the fixture staged a {SUMMARY_WORDS}-token \
         summary"
    );
}

#[tokio::test]
async fn nothing_below_the_threshold_is_compacted_and_no_model_is_called() {
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    let mut context = Context::opened(staged_prefix(), limits(4_000, 3_000), 0);
    for n in 1..=4 {
        context.record_exchange(staged_exchange(n, 10));
    }
    context.attach(staged_attachment(1, 9));
    let before = context.clone();

    let compaction = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("nothing to do cannot fail");

    // Assert the staging: this check is vacuous if the context was already
    // over the threshold and something simply refused to run.
    assert!(
        context.usage(&counter, &NothingHeld).used() <= 3_000,
        "the staged context was meant to be UNDER the threshold and used {} tokens",
        context.usage(&counter, &NothingHeld).used()
    );
    assert!(
        compaction.announcements.is_empty() && compaction.raw.is_none(),
        "a context under the threshold announces nothing: {compaction:?}"
    );
    assert_eq!(
        summariser.calls(),
        0,
        "ADR-0013's Negative consequence is that summarisation costs a model call; a compaction \
         nobody needed spends one for nothing"
    );
    assert_eq!(context, before, "nothing was to be taken and something was");
}

#[tokio::test]
async fn a_failing_summariser_leaves_every_exchange_where_it_was() {
    let counter = WordCounter;
    let summariser = StagedSummariser::failing();
    let mut context = Context::opened(staged_prefix(), limits(400, 1), 0);
    for n in 1..=4 {
        context.record_exchange(staged_exchange(n, 10));
    }
    let before = context.clone();

    let failure = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect_err("a failing summariser is a failure");

    assert!(
        failure.to_string().contains(NONCE),
        "the port's own wording must reach the caller unaltered: {failure}"
    );
    assert_eq!(
        context, before,
        "ADR-0013 D2 preserves history; a span removed before its replacement existed is a span \
         nothing can put back"
    );
}

#[tokio::test]
async fn a_summary_is_compacted_again_like_any_other_exchange() {
    // Delegated coordinator ruling of 2026-09-04, recorded on ADR-0013 as an
    // open question: pinning summaries would eventually leave compaction with
    // nothing it is allowed to free, which routes straight to D7.
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(4);
    let mut context = Context::opened(staged_prefix(), limits(400, 1), 0);
    for n in 1..=4 {
        context.record_exchange(staged_exchange(n, 10));
    }

    context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("first compaction");
    for n in 5..=8 {
        context.record_exchange(staged_exchange(n, 10));
    }
    let second = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("second compaction");

    let span = second
        .raw
        .as_ref()
        .expect("the second compaction took a span");
    assert!(
        span.exchanges()
            .iter()
            .any(|exchange| exchange.kind() == ExchangeKind::Summary),
        "the oldest thing in layer 6 was the first summary, and oldest-first means it goes into \
         the next span. The second span held {:?}",
        span.exchanges()
            .iter()
            .map(Exchange::kind)
            .collect::<Vec<_>>()
    );
}

// --- Trigger clause 3: dropping an attachment ------------------------------

#[tokio::test]
async fn a_dropped_attachment_is_named_with_its_workspace_and_says_how_to_get_it_back() {
    // Layer 6 is empty, so there is nothing to compact and the pressure can
    // only be relieved by layer 5 — which is what D1 means by "discarded
    // last": last, but not never.
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    let mut context = Context::opened(staged_prefix(), limits(400, 40), 0);
    for n in 1..=3 {
        context.attach(staged_attachment(n, 12));
    }

    let compaction = context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("no summariser call is needed when layer 6 is empty");

    let dropped: Vec<(&str, &str, &str)> = compaction
        .announcements
        .iter()
        .filter_map(|a| match a {
            Announcement::AttachmentDropped {
                identity,
                how_to_reattach,
            } => Some((
                identity.workspace(),
                identity.path(),
                how_to_reattach.as_str(),
            )),
            Announcement::Compacted { .. } => None,
        })
        .collect();

    assert!(
        !dropped.is_empty(),
        "the staged context was over its threshold with nothing but attachments to give"
    );
    assert_eq!(
        dropped[0].1,
        format!("adrs/{NONCE}-1"),
        "attachments go oldest first, and the first dropped was {:?}",
        dropped[0].1
    );
    for (workspace, path, reattach) in &dropped {
        assert_eq!(
            *workspace, "zaru",
            "ADR-0013 D4's announcement names the attachment, and a path with no workspace names \
             nothing that resolves"
        );
        assert!(!path.is_empty());
        assert_eq!(
            *reattach, "[[",
            "D4: the announcement states how to re-attach, in the words whoever attached it used"
        );
    }
    assert_eq!(
        context.attachments().len(),
        3 - dropped.len(),
        "every announced drop must be a drop that happened"
    );
    assert!(
        !dropped.is_empty() && dropped.len() < 3,
        "only as many attachments as the pressure called for: {} of 3 went",
        dropped.len()
    );
    assert_eq!(
        summariser.calls(),
        0,
        "there was nothing in layer 6 to summarise"
    );
}

#[test]
fn an_item_that_could_not_be_announced_is_refused_at_the_boundary() {
    // Each guard needs an input it must reject, and the reason is asserted as
    // well as the rejection, so an accidental refusal for the wrong cause is
    // not counted as a pass.
    assert_eq!(ItemId::new("", "a/path"), Err(ItemRefused::NoWorkspace));
    assert_eq!(ItemId::new("zaru", ""), Err(ItemRefused::NoPath));

    let id = ItemId::new("zaru", "a/path").expect("a complete identity");
    assert_eq!(
        AttachedItem::new(id.clone(), "body", ""),
        Err(ItemRefused::NoReattachInstruction)
    );
    assert!(
        AttachedItem::new(id.clone(), "", "[[").is_ok(),
        "an empty body is the user attaching an empty page, which is their choice and not this \
         crate's"
    );

    for refusal in [
        ItemRefused::NoWorkspace,
        ItemRefused::NoPath,
        ItemRefused::NoReattachInstruction,
    ] {
        let said = refusal.to_string();
        // Rewritten 2026-09-13 by the `record-citations` arc, under the
        // coordinator's ruling of that date: this asserted
        // `said.contains("ADR-0013 D4")`. A refusal should say what the reader
        // can act on rather than which record refused it, and `which` is that
        // half -- it names the missing field by name for each of the three.
        assert!(
            said.contains("could not be announced if it were dropped")
                && said.contains("resolves nowhere"),
            "a refusal should say what is missing and why it matters: {said}"
        );
    }
}

// --- Trigger clause 4: iteration history -----------------------------------

#[test]
fn older_iterations_are_one_line_and_the_newest_keeps_its_output_verbatim() {
    let records: Vec<IterationRecord> = (1..=4).map(staged_iteration).collect();
    let rendered = history::render(&records);

    for n in 1..=3_u32 {
        let line = records[(n - 1) as usize].one_line();
        assert!(
            rendered.contains(&line),
            "iteration {n} should appear as D5's one line, and the rendering was:\n{rendered}"
        );
        assert!(
            !rendered.contains(&verbatim_failure_for(n)),
            "iteration {n} is older than the newest, so its full output is transcript material \
             and not context material"
        );
        assert!(
            !line.contains('\n'),
            "D5 says one LINE, and iteration {n}'s was {line:?}"
        );
    }

    assert!(
        rendered.contains(&verbatim_failure_for(4)),
        "ADR-0013 D5 and ADR-0008 D4: the most recent failure keeps its verbatim output, because \
         paraphrasing it converts iteration back into retry. The rendering was:\n{rendered}"
    );
}

#[tokio::test]
async fn the_newest_failure_reaches_the_model_byte_for_byte_through_a_real_loop_run() {
    // Asserted jointly with ADR-0008 D4, by driving the loop itself rather
    // than by rendering layer 7 on its own. The bytes compared come from the
    // fixture, not from anything the renderer computed.
    let counter = WordCounter;
    let mut context = Context::opened(staged_prefix(), limits(100_000, 90_000), 0);
    for n in 1..=3 {
        context.record_iteration(staged_iteration(n));
    }
    let policy = PolicyOver::new(&context, &counter);

    let generator = InertGenerator::default();
    let validators = ScriptedValidators::new(vec![true]);

    run(
        "make the tests pass",
        Limits {
            ceiling: Ceiling::new(1).expect("ceiling"),
            budget: TruncationBudget::new(4096).expect("budget"),
        },
        Ports {
            generator: &generator,
            executor: &InertExecutor,
            validators: &validators,
            context: &policy,
            clock: &FrozenClock,
            redactor: &NothingHeld,
        },
        &mut [],
    )
    .await
    .expect("the staged run reaches an outcome");

    // Read from the generator rather than from the policy. The policy is the
    // thing under test; what the generator was handed is what the model would
    // actually have seen, which is one layer further out.
    let assembled = generator.prompts();
    assert_eq!(assembled.len(), 1, "one iteration is one assembly");
    assert!(
        assembled[0].contains(&verbatim_failure_for(3)),
        "the newest failure must reach the model verbatim, and what it was given was:\n{}",
        assembled[0]
    );
    assert!(
        !assembled[0].contains(&verbatim_failure_for(1)),
        "iteration 1 is older than the newest and must have been compacted to one line"
    );
}

// --- ADR-0008 trigger clause 6, decided 2026-09-05 -------------------------

#[tokio::test]
async fn a_held_secret_in_layer_seven_is_absent_from_the_prompt_the_model_is_given() {
    // ADR-0013 D5's layer 7 is the second of the paths that decision names,
    // and this record's own Status tracking has carried it as an open finding
    // since 2026-09-04: "layer 7 carries the most recent failure's verbatim
    // output into the assembled context, which is a model prompt, without
    // passing through that seam". It passes the port now.
    //
    // Driven through a real loop run and read out of the *generator*, which is
    // one layer beyond the policy under test. The discriminating arm is
    // `the_newest_failure_reaches_the_model_byte_for_byte_through_a_real_loop_run`
    // above, which drives the same staging with nothing held and asserts
    // these exact bytes are present.
    let held = verbatim_failure_for(3);
    let core = ascii_core(&held);
    assert!(
        !core.is_empty() && core != held,
        "the staged failure must have an ASCII core distinct from itself, or \
         the escaped-form arm asserts nothing: {held:?}"
    );
    let holding = HoldingOne::new(held.clone(), "work");

    let counter = WordCounter;
    let mut context = Context::opened(staged_prefix(), limits(100_000, 90_000), 0);
    for n in 1..=3 {
        context.record_iteration(staged_iteration(n));
    }
    let policy = PolicyOver::new(&context, &counter).holding(&holding);
    let generator = InertGenerator::default();
    let validators = ScriptedValidators::new(vec![true]);

    run(
        "make the tests pass",
        Limits {
            ceiling: Ceiling::new(1).expect("ceiling"),
            budget: TruncationBudget::new(4096).expect("budget"),
        },
        Ports {
            generator: &generator,
            executor: &InertExecutor,
            validators: &validators,
            context: &policy,
            clock: &FrozenClock,
            redactor: &holding,
        },
        &mut [],
    )
    .await
    .expect("the staged run reaches an outcome");

    let assembled = generator.prompts();
    assert_eq!(assembled.len(), 1, "one iteration is one assembly");
    assert!(
        !assembled[0].contains(&held),
        "layer 7's verbatim output carried a held value into the model's \
         prompt:\n{}",
        assembled[0]
    );
    assert!(
        !assembled[0].contains(core),
        "layer 7 carried a held value's ASCII core into the model's prompt, \
         so an escaping renderer would publish it:\n{}",
        assembled[0]
    );
    assert!(
        assembled[0].contains("<redacted: work>"),
        "nothing marks where the value was, and a policy that assembled an \
         empty context would satisfy both assertions above on its own:\n{}",
        assembled[0]
    );
}

#[test]
fn a_held_secret_in_layer_six_is_absent_from_the_assembled_context() {
    // ADR-0013 D1's layer 6 is "conversation **and tool results**", so it
    // carries captured bytes the moment anything writes a tool result into an
    // exchange. Redacting at the assembly boundary rather than inside layer 7
    // is what makes that path covered before it has a producer, and this
    // check is what says so rather than a comment claiming it.
    let held = format!("{NONCE}-tool-result-e\u{301}\u{1f701}");
    let core = ascii_core(&held);
    let holding = HoldingOne::new(held.clone(), "work");
    let counter = WordCounter;

    let mut context = Context::opened(staged_prefix(), limits(100_000, 90_000), 0);
    context.record_exchange(Exchange::verbatim(held.clone()));

    let assembled = context
        .assemble(&counter, &holding, "the task")
        .expect("the staged context fits the window");
    assert!(
        !assembled.as_str().contains(&held),
        "layer 6 carried a held value into the assembled context: {:?}",
        assembled.as_str()
    );
    assert!(
        !assembled.as_str().contains(core),
        "layer 6 carried a held value's ASCII core into the assembled \
         context: {:?}",
        assembled.as_str()
    );
    assert!(
        assembled.as_str().contains("<redacted: work>"),
        "nothing marks where the value was: {:?}",
        assembled.as_str()
    );

    // The discriminating arm: the same exchange with nothing held reaches the
    // model unaltered, so the assertions above are about redaction rather
    // than about an assembler that drops layer 6.
    let carried = context
        .assemble(&counter, &NothingHeld, "the task")
        .expect("the staged context fits the window");
    assert!(
        carried.as_str().contains(&held),
        "with nothing held, layer 6 must reach the model unaltered: {:?}",
        carried.as_str()
    );
}

// --- Trigger clause 6 and D7: compaction never runs during an iteration ----

#[tokio::test]
async fn a_loop_run_under_pressure_compacts_nothing() {
    // ADR-0013 D7. The context is genuinely over its threshold for the whole
    // run — asserted, so this cannot pass over a run that felt no pressure —
    // and comes out of the run byte-for-byte as it went in.
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    // The threshold sits ABOVE what the prefix alone costs, so the pressure
    // this check stages genuinely comes from the layers compaction would
    // take. A threshold the prefix already crosses would leave this arm
    // satisfied by a context with no layer 6 in it at all — awkward on the
    // wrong axis, which is how the first version of this check was found.
    let mut context = Context::opened(staged_prefix(), limits(100_000, 60), 0);
    for n in 1..=6 {
        context.record_exchange(staged_exchange(n, 10));
    }
    context.attach(staged_attachment(1, 9));
    let before = context.clone();

    assert!(
        context.usage(&counter, &NothingHeld).used() > 60,
        "the staged context was meant to be over its threshold on the strength of its \
         compactable layers, and used {} tokens",
        context.usage(&counter, &NothingHeld).used()
    );

    let policy = PolicyOver::new(&context, &counter);
    let generator = InertGenerator::default();
    let validators = ScriptedValidators::new(vec![false, false, true]);

    run(
        "make the tests pass",
        Limits {
            ceiling: Ceiling::new(5).expect("ceiling"),
            budget: TruncationBudget::new(4096).expect("budget"),
        },
        Ports {
            generator: &generator,
            executor: &InertExecutor,
            validators: &validators,
            context: &policy,
            clock: &FrozenClock,
            redactor: &NothingHeld,
        },
        &mut [],
    )
    .await
    .expect("the staged run reaches an outcome");

    assert_eq!(
        generator.prompts().len(),
        3,
        "the staged run is three iterations, so the context was assembled three times under \
         pressure"
    );
    assert_eq!(
        context, before,
        "ADR-0013 D7: compaction happens at turn boundaries only, and three iterations inside one \
         turn changed the context"
    );
    assert_eq!(
        summariser.calls(),
        0,
        "no model call may be spent on compaction during an iteration"
    );
}

#[test]
fn assembly_refuses_rather_than_rewriting_when_the_window_would_be_exceeded() {
    let counter = WordCounter;
    let mut context = Context::opened(staged_prefix(), limits(30, 20), 0);
    for n in 1..=6 {
        context.record_exchange(staged_exchange(n, 10));
    }
    let before = context.clone();

    let exceeded = context
        .assemble(&counter, &NothingHeld, "a tail")
        .expect_err("sixty tokens of layer 6 do not fit a thirty-token window");

    assert_eq!(
        exceeded.window, 30,
        "the refusal reports the window it was measured against"
    );
    assert!(
        exceeded.needed > exceeded.window,
        "a refusal must report needing MORE than the window: {exceeded}"
    );
    assert_eq!(
        context, before,
        "ADR-0013 D7: an iteration that would exceed the window fails rather than continuing on a \
         rewritten context"
    );
}

// --- D6: usage is a number, not a rendered line ----------------------------

#[tokio::test]
async fn usage_is_readable_between_turns_and_falls_when_a_compaction_frees_room() {
    let counter = WordCounter;
    let summariser = StagedSummariser::costing(3);
    let mut context = Context::opened(staged_prefix(), limits(400, 40), 0);
    for n in 1..=5 {
        context.record_exchange(staged_exchange(n, 10));
    }

    let before = context.usage(&counter, &NothingHeld);
    assert_eq!(
        before.window(),
        400,
        "usage reports the window it was given"
    );
    assert_eq!(
        before.remaining(),
        400 - before.used(),
        "remaining is what is left of the window"
    );
    assert!(
        before.used() > 40,
        "the staged context is over its threshold"
    );

    context
        .compact(&summariser, &counter, &NothingHeld)
        .await
        .expect("the staged summariser does not fail here");
    let after = context.usage(&counter, &NothingHeld);

    assert!(
        after.used() < before.used(),
        "compaction freed nothing: {} tokens before and {} after",
        before.used(),
        after.used()
    );
}

// --- The boundary ----------------------------------------------------------

#[test]
fn limits_that_cannot_describe_a_real_window_are_refused_at_the_boundary() {
    assert_eq!(ContextWindow::new(0), Err(LimitsRefused::WindowIsZero));
    assert_eq!(
        PressureThreshold::new(0),
        Err(LimitsRefused::ThresholdIsZero)
    );
    assert!(ContextWindow::new(1).is_ok());
    assert!(PressureThreshold::new(1).is_ok());

    let window = ContextWindow::new(100).expect("window");
    let above = PressureThreshold::new(101).expect("threshold");
    assert_eq!(
        ContextLimits::new(window, above),
        Err(LimitsRefused::ThresholdAboveWindow {
            threshold: 101,
            window: 100,
        }),
        "a threshold above the window is a warning that arrives after the failure it warns about"
    );
    assert!(
        ContextLimits::new(window, PressureThreshold::new(100).expect("threshold")).is_ok(),
        "a threshold exactly at the window is usable"
    );

    for refusal in [
        LimitsRefused::WindowIsZero,
        LimitsRefused::ThresholdIsZero,
        LimitsRefused::ThresholdAboveWindow {
            threshold: 101,
            window: 100,
        },
    ] {
        let said = refusal.to_string();
        assert!(
            said.contains('0') || said.contains("101"),
            "a refusal should name the value it refused: {said}"
        );
    }
}

/// [`Context::reserved`] is on every whole-context measurement and on no
/// fragment measurement.
///
/// # Why one check covers four call sites
///
/// The reserve is what a request spends outside the context, so it belongs to
/// every number that answers "does this request fit" — `usage`, `assemble`
/// and the threshold comparison inside `compact` — and to no number that
/// answers "how big is this exchange". Asserting the four together is what
/// makes the *asymmetry* the property, rather than four separate assertions
/// any one of which could drift into agreeing with the others.
///
/// The arithmetic is the fixture's own: four prefix layers of
/// `PREFIX_WORDS` words and exchanges staged with a chosen word count, under
/// a counter costing one token per word. So every number below is one this
/// file chose.
///
/// Watched red three ways:
///
/// - reserve left out of `usage` — *"a context reserving 100 reports 124
///   where the same context reserving nothing reports 24, so the reserve is
///   not on the number a window is read against"*;
/// - reserve added inside `oldest_span_covering` — the compaction took **one**
///   exchange where it takes three without it, because each exchange looked
///   100 tokens larger than it is;
/// - reserve added to the announcement's `before` — the line reported 130
///   for a span holding 30 words, which is D3's "real before-and-after
///   counts" reporting bytes the span never held.
#[tokio::test]
async fn the_reserve_is_on_the_whole_context_and_on_no_single_exchange() {
    let prefix = staged_prefix();
    let bare = Context::opened(prefix.clone(), limits(4_000, 3_000), 0);
    let reserving = Context::opened(prefix, limits(4_000, 3_000), 100);
    let counter = WordCounter;
    let held = NothingHeld;

    let empty = bare.usage(&counter, &held).used();
    assert_eq!(
        reserving.usage(&counter, &held).used(),
        empty + 100,
        "a reserve of 100 is 100 more on the number a window is read against"
    );
    assert_eq!(
        reserving
            .assemble(&counter, &held, "")
            .expect("it fits")
            .usage()
            .used(),
        bare.assemble(&counter, &held, "")
            .expect("it fits")
            .usage()
            .used()
            + 100,
        "the reserve is on what `assemble` refuses against too, or a request \
         that does not fit is assembled"
    );

    // Three exchanges of ten words each, a threshold twenty-five words below
    // what they cost together, and a reserve of a hundred on top. The span
    // taken has to be the same span it would be without the reserve: the
    // overage is bigger by the reserve, but each exchange is still ten.
    let mut context = Context::opened(
        staged_prefix(),
        limits(4_000, 24 + 4 * PREFIX_WORDS as u64),
        100,
    );
    for n in 1..=3 {
        context.record_exchange(Exchange::verbatim(staged_text(&format!("said-{n}"), 10)));
    }
    let summariser = StagedSummariser::costing(3);
    let compaction = context
        .compact(&summariser, &counter, &held)
        .await
        .expect("the staged summariser succeeds");

    let taken = summariser.spans();
    assert_eq!(
        taken.len(),
        1,
        "exactly one span crossed the port: {} did",
        taken.len()
    );
    assert_eq!(
        taken[0].len(),
        3,
        "three exchanges of ten cost thirty, and the overage is a hundred and \
         six; each exchange is ten whatever the reserve is, so all three go. \
         {} went",
        taken[0].len()
    );

    let Some(Announcement::Compacted { turns, before, .. }) = compaction.announcements.first()
    else {
        panic!("a compaction that took a span announces it");
    };
    assert_eq!(*turns, 3);
    assert_eq!(
        *before, 30,
        "the announcement's before-count is the span's own thirty words. A \
         reserve on it would report bytes the span never held"
    );
}
