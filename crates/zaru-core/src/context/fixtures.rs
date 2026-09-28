// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staged implementations of the two ports, and the texts the checks measure.
//!
//! These are the test tree. Nothing here has a counterpart in the product
//! tree, and nothing here reaches a network or a model.
//!
//! Three properties are load-bearing and must survive anybody tidying this
//! file up.
//!
//! **Every staged text carries a nonce, a newline and a non-ASCII character**,
//! so that an implementation which produced a plausible constant could not
//! produce it. A fixture too well-behaved to separate a right implementation
//! from a wrong one is the second of the three failure shapes the testing
//! contract names.
//!
//! **The counter is a rule the test can compute in its head.** One token per
//! whitespace-separated word, so a text built with `words` words costs
//! exactly `words`. That is what lets a check assert the announcement's
//! before-and-after against the number the *fixture* staged, rather than
//! against a second reading taken through the code under test — the same
//! discipline `ManualClock` gives the loop's elapsed times.
//!
//! **The summariser records what it was handed**, so oldest-first can be
//! asserted against what actually crossed the port rather than against what
//! the context has left over afterwards.

use crate::context::assembly::Context;
use crate::context::exchange::Exchange;
use crate::context::history::IterationRecord;
use crate::context::item::{AttachedItem, ItemId};
use crate::context::port::{Span, Summariser, TokenCounter};
use crate::iteration::port::{ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
use crate::redaction::Redactor;
use crate::redaction::fixtures::NothingHeld;
use std::sync::Mutex;

/// A nonce no implementation could produce without carrying it.
pub(super) const NONCE: &str = "zaru-ctx-nonce-4d7e";

/// One token per whitespace-separated word.
///
/// Deliberately not a real tokeniser: ADR-0003 D2's table carries none, and
/// the point of the [`TokenCounter`] port is that the count is a measurement
/// the caller supplies. What this fixture buys is that every number in every
/// assertion below is one the test itself chose.
#[derive(Debug, Default)]
pub(super) struct WordCounter;

impl TokenCounter for WordCounter {
    fn count(&self, text: &str) -> u64 {
        text.split_whitespace().count() as u64
    }
}

/// A text of exactly `words` words, carrying the nonce, an embedded newline
/// and a non-ASCII character.
pub(super) fn staged_text(label: &str, words: usize) -> String {
    assert!(words >= 1, "a staged text needs at least its label");
    let mut out = format!("{NONCE}-{label}");
    for i in 1..words {
        // Every third separator is a newline, so any text of four words or
        // more carries one.
        out.push(if i % 3 == 0 { '\n' } else { ' ' });
        out.push_str(&format!("mot-é-{i}"));
    }
    out
}

/// Exchange `n`, costing exactly `words` tokens under [`WordCounter`].
pub(super) fn staged_exchange(n: u32, words: usize) -> Exchange {
    Exchange::of_turn(vec![crate::conversation::Message::User {
        text: staged_text(&format!("exchange-{n}"), words),
    }])
}

/// An attachment the user chose, costing exactly `words` tokens.
pub(super) fn staged_attachment(n: u32, words: usize) -> AttachedItem {
    AttachedItem::new(
        ItemId::new("zaru", format!("adrs/{NONCE}-{n}")).expect("a staged identity is complete"),
        staged_text(&format!("attachment-{n}"), words),
        "[[",
    )
    .expect("a staged attachment carries its re-attachment instruction")
}

/// Iteration `n`'s record, whose verbatim failure carries the nonce, a
/// newline and a non-ASCII character.
pub(super) fn staged_iteration(n: u32) -> IterationRecord {
    IterationRecord {
        n,
        tried: format!("{NONCE}-candidate-{n}\nand a second line nobody should see"),
        failed: format!("{NONCE}-validator-{n}"),
        why: format!("{NONCE}-because-{n}"),
        verbatim: verbatim_failure_for(n),
    }
}

/// The whole failure output iteration `n` produced.
pub(super) fn verbatim_failure_for(n: u32) -> String {
    format!("{NONCE}\niteration {n}: assertion failed — left ≠ right\n  left: 1\n  right: 2\n")
}

/// Summarises by returning a staged text of a chosen size, and keeps every
/// span it was handed.
#[derive(Debug)]
pub(super) struct StagedSummariser {
    words: usize,
    fails: bool,
    spans: Mutex<Vec<Span>>,
}

impl StagedSummariser {
    /// A summariser whose every summary costs `words` tokens.
    pub(super) fn costing(words: usize) -> Self {
        Self {
            words,
            fails: false,
            spans: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn failing() -> Self {
        Self {
            words: 1,
            fails: true,
            spans: Mutex::new(Vec::new()),
        }
    }

    /// Every span this summariser was asked about, in order.
    pub(super) fn spans(&self) -> Vec<Span> {
        self.spans.lock().expect("spans poisoned").clone()
    }

    /// How many times it was asked. Zero is the assertion ADR-0013 D7 needs.
    pub(super) fn calls(&self) -> usize {
        self.spans.lock().expect("spans poisoned").len()
    }
}

impl Summariser for StagedSummariser {
    async fn summarise(&self, span: &Span) -> Result<String, PortFailure> {
        self.spans
            .lock()
            .expect("spans poisoned")
            .push(span.clone());
        if self.fails {
            return Err(PortFailure::new(format!("{NONCE} summariser unreachable")));
        }
        Ok(staged_text(
            &format!("summary-of-{}", span.len()),
            self.words,
        ))
    }
}

/// A context policy over a real [`Context`], which is how the loop reaches
/// one.
///
/// It holds the context by shared reference and therefore **cannot compact**:
/// that is ADR-0013 D7 made structural rather than remembered, and it is the
/// shape `zaru-cli` will build the product adapter in.
pub(super) struct PolicyOver<'a> {
    context: &'a Context,
    counter: &'a WordCounter,
    redactor: &'a (dyn Redactor + Sync),
}

impl core::fmt::Debug for PolicyOver<'_> {
    /// Written by hand because a `&dyn Redactor` is not `Debug` and giving
    /// the trait that bound would put a redactor's contents -- which is a set
    /// of held secrets -- into every `{:?}` in the program. `Secret`'s
    /// discipline, one layer up.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PolicyOver")
            .field("context", &self.context)
            .field("counter", &self.counter)
            .finish_non_exhaustive()
    }
}

impl<'a> PolicyOver<'a> {
    pub(super) const fn new(context: &'a Context, counter: &'a WordCounter) -> Self {
        Self {
            context,
            counter,
            redactor: &NothingHeld,
        }
    }

    /// The same policy over a redactor that holds something.
    pub(super) const fn holding(mut self, redactor: &'a (dyn Redactor + Sync)) -> Self {
        self.redactor = redactor;
        self
    }
}

impl ContextPolicy for PolicyOver<'_> {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let tail = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
        };
        let assembled = self.context.assemble(self.counter, self.redactor, &tail)?;
        Ok(assembled.into_prompt())
    }
}

// --- Driving the real loop, as a caller of it ------------------------------
//
// `zaru-core`'s own loop fixtures exist to check the loop. These exist to
// *use* it: two checks here drive `iteration::run` so that ADR-0013 D5's
// verbatim clause and D7's turn-boundary clause are asserted against the real
// machine rather than against a rendering of layer 7 on its own. They are
// deliberately the dullest possible ports — nothing here is under test.

use crate::iteration::event::ValidatorOutcome;
use crate::iteration::port::{
    Clock, ExecutionOutcome, Executor, Generated, Generator, ValidatorReport, Validators,
};
use core::time::Duration;

/// A clock that never moves. Nothing here asserts on elapsed time.
#[derive(Debug, Default)]
pub(super) struct FrozenClock;

impl Clock for FrozenClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// Produces a candidate and records the prompt it was handed.
#[derive(Debug, Default)]
pub(super) struct InertGenerator {
    prompts: Mutex<Vec<String>>,
}

impl InertGenerator {
    /// Every prompt this generator was given, in order. This is what the
    /// model would have seen.
    pub(super) fn prompts(&self) -> Vec<String> {
        self.prompts.lock().expect("prompts poisoned").clone()
    }
}

impl Generator for InertGenerator {
    type Candidate = String;

    async fn generate(&self, prompt: &Prompt) -> Result<Generated<Self::Candidate>, PortFailure> {
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(prompt.rendered());
        Ok(Generated {
            candidate: format!("{NONCE}-candidate"),
            tokens: 1,
        })
    }
}

/// Executes nothing.
#[derive(Debug, Default)]
pub(super) struct InertExecutor;

impl Executor for InertExecutor {
    type Candidate = String;

    async fn execute(&self, _candidate: &Self::Candidate) -> Result<ExecutionOutcome, PortFailure> {
        Ok(ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

/// Reports pass or fail from a script, the last entry repeating.
#[derive(Debug)]
pub(super) struct ScriptedValidators {
    script: Vec<bool>,
    calls: Mutex<usize>,
}

impl ScriptedValidators {
    pub(super) fn new(script: Vec<bool>) -> Self {
        assert!(!script.is_empty(), "a validators fixture needs a script");
        Self {
            script,
            calls: Mutex::new(0),
        }
    }
}

impl Validators for ScriptedValidators {
    async fn evaluate(
        &self,
        _execution: &ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        let n = {
            let mut calls = self.calls.lock().expect("calls poisoned");
            *calls += 1;
            *calls
        };
        let passed = self.script[(n - 1).min(self.script.len() - 1)];
        Ok(vec![ValidatorReport {
            name: "test".to_owned(),
            outcome: if passed {
                ValidatorOutcome::Passed
            } else {
                ValidatorOutcome::Failed
            },
            detail: verbatim_failure_for(u32::try_from(n).unwrap_or(u32::MAX)),
        }])
    }
}
