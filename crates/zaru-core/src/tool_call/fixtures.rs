// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staging for the tool-call loop's checks.
//!
//! Every port implementation in this crate lives here or in `tests/`. The
//! product tree implements none of them, which is what makes ADR-0008 D2's
//! headless requirement structural.
//!
//! # The fixtures are deliberately awkward, and on named axes
//!
//! Library [Verification lessons] §51: awkwardness is a property per axis,
//! not a general quality. Every staged string here carries a nonce, an
//! embedded newline, a combining mark, an astral-plane character **and**
//! leading and trailing whitespace — the last because an identity seam
//! survived a `trim_end` mutant in this workspace on 2026-09-04 by being
//! awkward on encoding and ordinary on whitespace.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2

use crate::iteration::fixtures::ManualClock;
use crate::iteration::port::{ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
use crate::redaction::Redacted;
use crate::redaction::fixtures::NothingHeld;
use crate::tool_call::event::{Event, EventSink};
use crate::tool_call::port::{
    Capabilities, InnerLoop, Model, ModelRequest, ModelResponse, TokenUsage, ToolDecision,
    ToolDescriptor, ToolExecutor, ToolOutcome, ToolRequest, ToolResult,
};
use core::time::Duration;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A value no implementation could produce without carrying it.
///
/// Unique per call, and awkward on five axes at once: whitespace at both
/// ends, an embedded newline, a combining mark, an astral-plane character,
/// and a per-process, per-call number.
pub(super) fn nonce(label: &str) -> String {
    format!("  {}\nnai\u{0301}ve-\u{1F980}  ", ascii_core(label))
}

/// The part of a nonce that no escaping can alter.
///
/// Library [Verification lessons] §50: a renderer that escapes non-ASCII
/// bytes publishes a value in a form an assertion written against the value
/// as typed does not recognise. That cuts both ways — it hides a leak from an
/// absence assertion, and it hides a correct mention from a presence one. So
/// every check that looks for a nonce in rendered text looks for this,
/// chosen so that `{:?}`, `{}` and a serialiser all leave it untouched.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2
pub(super) fn ascii_core(label: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{seq}", std::process::id())
}

/// The variant name of an event, so a trace can be read without matching on
/// payloads.
pub(super) const fn tag(event: &Event) -> &'static str {
    match event {
        Event::TurnStarted { .. } => "TurnStarted",
        Event::ModelResponded { .. } => "ModelResponded",
        Event::ToolRequested { .. } => "ToolRequested",
        Event::ToolPermissionDecided { .. } => "ToolPermissionDecided",
        Event::ToolCompleted { .. } => "ToolCompleted",
        Event::ToolRefused { .. } => "ToolRefused",
        Event::TurnEnded { .. } => "TurnEnded",
    }
}

/// A sink that keeps every event it was handed.
#[derive(Debug, Default)]
pub(super) struct Recorder {
    pub(super) events: Vec<Event>,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: &Event) {
        self.events.push(event.clone());
    }
}

/// A second, deliberately unlike sink: it keeps only variant names.
///
/// Unlike `Recorder` so that "one emission reached both" is a claim about two
/// different consumers rather than about two copies of one.
#[derive(Debug, Default)]
pub(super) struct Projector {
    pub(super) tags: Vec<&'static str>,
}

impl EventSink for Projector {
    fn emit(&mut self, event: &Event) {
        self.tags.push(tag(event));
    }
}

/// What a staged model answers, in order.
#[derive(Debug, Clone)]
pub(super) enum Answer {
    /// Ask for these tools.
    Calls(Vec<ToolRequest>),
    /// Answer the user.
    Text(String),
    /// Stop without answering.
    Stopped(String),
    /// Fail the port.
    Fail(String),
}

/// A model that answers from a script, and records what it was shown.
#[derive(Debug)]
pub(super) struct StagedModel {
    answers: Mutex<std::collections::VecDeque<Answer>>,
    /// What `results` carried on each request, in order. The turn's
    /// accumulation, read from the model's side rather than the loop's.
    pub(super) seen: Arc<Mutex<Vec<Vec<ToolResult>>>>,
    /// What `prompt` carried on each request.
    pub(super) prompts: Arc<Mutex<Vec<String>>>,
    /// What `tools` carried on the first request.
    pub(super) offered: Arc<Mutex<Vec<ToolDescriptor>>>,
    clock: Arc<ManualClock>,
    cost: Duration,
    tokens: TokenUsage,
    capabilities: Capabilities,
}

impl StagedModel {
    pub(super) fn new(answers: Vec<Answer>, clock: Arc<ManualClock>, cost: Duration) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            seen: Arc::new(Mutex::new(Vec::new())),
            prompts: Arc::new(Mutex::new(Vec::new())),
            offered: Arc::new(Mutex::new(Vec::new())),
            clock,
            cost,
            tokens: TokenUsage {
                prompt: 7,
                completion: 11,
            },
            capabilities: Capabilities { tool_calling: true },
        }
    }

    /// A model that says it cannot call tools. ADR-0012 clause 3's subject.
    pub(super) fn without_tool_calling(clock: Arc<ManualClock>) -> Self {
        let mut model = Self::new(Vec::new(), clock, Duration::ZERO);
        model.capabilities = Capabilities {
            tool_calling: false,
        };
        model
    }
}

impl Model for StagedModel {
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        self.seen
            .lock()
            .expect("seen poisoned")
            .push(request.results.to_vec());
        self.prompts
            .lock()
            .expect("prompts poisoned")
            .push(request.prompt.as_str().to_owned());
        {
            let mut offered = self.offered.lock().expect("offered poisoned");
            if offered.is_empty() {
                *offered = request.tools.to_vec();
            }
        }
        self.clock.advance(self.cost);
        let answer = self
            .answers
            .lock()
            .expect("answers poisoned")
            .pop_front()
            .expect("the staged model was asked more times than it was staged for");
        Ok(match answer {
            Answer::Calls(calls) => ModelResponse::Calls {
                calls,
                tokens: self.tokens,
            },
            Answer::Text(text) => ModelResponse::Text {
                text,
                tokens: self.tokens,
            },
            Answer::Stopped(reason) => ModelResponse::Stopped {
                reason,
                tokens: self.tokens,
            },
            Answer::Fail(detail) => return Err(PortFailure::new(detail)),
        })
    }
}

/// What a staged executor does with one call.
#[derive(Debug, Clone)]
pub(super) enum Act {
    /// Return this content.
    Return(String),
    /// Return this content, marked as the tool having failed.
    Failed(String),
    /// Refuse, with this sentence.
    Refuse(String),
    /// Fail the port.
    Fail(String),
}

/// A tool surface that acts from a script, and records what it was asked.
#[derive(Debug)]
pub(super) struct StagedTools {
    acts: Mutex<std::collections::VecDeque<Act>>,
    /// Every request it was handed, in order.
    pub(super) asked: Arc<Mutex<Vec<ToolRequest>>>,
    descriptors: Vec<ToolDescriptor>,
    clock: Arc<ManualClock>,
    cost: Duration,
}

impl StagedTools {
    pub(super) fn new(
        acts: Vec<Act>,
        descriptors: Vec<ToolDescriptor>,
        clock: Arc<ManualClock>,
        cost: Duration,
    ) -> Self {
        Self {
            acts: Mutex::new(acts.into()),
            asked: Arc::new(Mutex::new(Vec::new())),
            descriptors,
            clock,
            cost,
        }
    }
}

impl ToolExecutor for StagedTools {
    fn descriptors(&self) -> &[ToolDescriptor] {
        &self.descriptors
    }

    async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
        self.asked
            .lock()
            .expect("asked poisoned")
            .push(request.clone());
        self.clock.advance(self.cost);
        let act = self
            .acts
            .lock()
            .expect("acts poisoned")
            .pop_front()
            .expect("the staged tool surface was asked more times than it was staged for");
        let decision = ToolDecision {
            statement: format!("Allow {}?", request.name),
            permitted: !matches!(act, Act::Refuse(_)),
        };
        Ok(match act {
            Act::Return(content) => ToolOutcome::Completed {
                decision,
                result: ToolResult {
                    id: request.id.clone(),
                    // The executing surface redacts before it hands a
                    // completed result on -- `zaru-cli`'s does, over the
                    // credential store -- because that is where the raw
                    // capture and the transcript both live. This fixture
                    // stands where that surface stands and holds nothing.
                    content: Redacted::by(&NothingHeld, &content),
                    failed: false,
                },
            },
            Act::Failed(content) => ToolOutcome::Completed {
                decision,
                result: ToolResult {
                    id: request.id.clone(),
                    content: Redacted::by(&NothingHeld, &content),
                    failed: true,
                },
            },
            Act::Refuse(because) => ToolOutcome::Refused {
                decision,
                id: request.id.clone(),
                because,
            },
            Act::Fail(detail) => return Err(PortFailure::new(detail)),
        })
    }
}

/// A context policy that renders the turn it was handed and records it.
#[derive(Debug, Default)]
pub(super) struct RecordingContext {
    /// One entry per `assemble`, naming the variant and its payload.
    pub(super) turns: Arc<Mutex<Vec<String>>>,
}

impl ContextPolicy for RecordingContext {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => format!("initial::{task}"),
            Turn::Refinement { refinement } => format!("refinement::{}", refinement.as_str()),
            Turn::Resumed { interrupted } => format!("resumed::{}", interrupted.call()),
        };
        self.turns
            .lock()
            .expect("turns poisoned")
            .push(rendered.clone());
        Ok(Prompt::new(Redacted::by(&NothingHeld, &rendered)))
    }
}

/// A context policy that refuses with whatever refusal it was built from.
///
/// [`RecordingContext`] always succeeds, which is right for every check about
/// what a turn does once it has a prompt and useless for the one about what it
/// does when it cannot get one.
#[derive(Debug)]
pub(super) struct RefusingContext {
    pub(super) refusal: ContextRefusal,
}

impl ContextPolicy for RefusingContext {
    async fn assemble(&self, _turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        Err(self.refusal.clone())
    }
}

/// An iteration loop that answers from a staged outcome and records that it
/// was entered.
#[derive(Debug)]
pub(super) struct StagedInner {
    outcome: Mutex<Option<crate::iteration::Outcome>>,
    /// Every task it was asked to iterate on. Empty means it was never
    /// entered, which is ADR-0009 D4's no-manifest case as an observation
    /// rather than an assumption.
    pub(super) tasks: Arc<Mutex<Vec<String>>>,
}

impl StagedInner {
    pub(super) fn new(outcome: crate::iteration::Outcome) -> Self {
        Self {
            outcome: Mutex::new(Some(outcome)),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl InnerLoop for StagedInner {
    async fn iterate(&self, task: &str) -> Result<crate::iteration::Outcome, PortFailure> {
        self.tasks
            .lock()
            .expect("tasks poisoned")
            .push(task.to_owned());
        Ok(self
            .outcome
            .lock()
            .expect("outcome poisoned")
            .take()
            .expect("the staged inner loop was entered more than once"))
    }
}

/// A tool descriptor with an awkward name, for a check that reads it back.
pub(super) fn descriptor(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        description: nonce("description"),
        parameters: nonce("parameters"),
    }
}

/// A request for one tool.
pub(super) fn request(id: &str, name: &str) -> ToolRequest {
    ToolRequest {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: nonce("arguments"),
    }
}
