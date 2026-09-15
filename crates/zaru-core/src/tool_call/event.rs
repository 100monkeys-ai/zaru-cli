// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The tool-call loop's own event stream.
//!
//! # Why this is a second enum rather than more variants on D3's
//!
//! [ADR-0008] D3 lists eight events and every one of them is iteration-shaped
//! — `IterationStarted`, `CandidateGenerated`, `ValidatorEvaluated`,
//! `RefinementConstructed`. D1 makes the two loops different loops on
//! purpose, and the outer one has no iterations, no candidates, no validators
//! and no refinement. Folding its events into that enum would give every
//! consumer of the inner loop's stream six variants it can never see, and
//! would make "the event stream" mean two things.
//!
//! So the outer loop has its own list, in D3's style, with its own sink. That
//! is the same shape [ADR-0011] D5's `Captured` takes beside
//! [`ExecutionOutcome`](crate::iteration::ExecutionOutcome) — two loops, two
//! shapes, the duplication recorded rather than hidden. It is a **delegated
//! coordinator ruling of 2026-09-04**, open to Jeshua's veto, and a proposed
//! Update on ADR-0008 D3 carries it.
//!
//! A consumer that wants both implements both sinks. `zaru-cli`'s transcript
//! writer does, and each producer is a variant of that crate's `Record`.
//!
//! # These carry byte counts, never the bytes
//!
//! `content_bytes` rather than the content, which is D3's own
//! `stdout_bytes`/`stderr_bytes` precedent. Two reasons, and the second is
//! the load-bearing one. [ADR-0010] D2 writes every event to
//! `transcript.jsonl`, and ADR-0011 D4 already writes the tool call there, so
//! carrying the output here would put it on disk twice. And a tool's output
//! is one of the paths into a model prompt that [ADR-0008]'s trigger clause 6
//! covers, decided on 2026-09-05 — so this stream deliberately does not
//! become one more place a secret can land.
//!
//! **This stream carries raw text and that is not an oversight.** ADR-0010 D2
//! makes the transcript this stream, and that record's Negative section says
//! the transcript holds whatever the session held. Redaction applies to what
//! a model reads, which is `ToolResult::content`, and not to the record.
//! `a_refusals_sentence_is_redacted_before_it_becomes_the_next_turns_content`
//! asserts both halves at once: the value absent from what the model saw, and
//! the raw sentence present on `ToolRefused`.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// How a turn finished.
///
/// Four ways, and none of them is an error: a turn that ended is a turn the
/// mechanism completed. A port that failed does not reach here at all — see
/// [`ToolCallError`](crate::tool_call::ToolCallError).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnEnding {
    /// The model answered the user.
    Answered,
    /// The model stopped without answering and without asking for a tool.
    Stopped,
    /// The turn's ceiling was reached with the model still asking for tools.
    CeilingReached,
    /// The iteration loop ran as this turn's body and finished.
    ///
    /// ADR-0009 D4: a project with declared validators runs it; a project
    /// with no manifest runs the tool-call loop only, and then no turn ever
    /// ends this way.
    Iterated {
        /// How many iterations ran.
        iterations: u32,
        /// Whether every validator passed.
        ///
        /// A boolean rather than the inner loop's `Outcome`, because that
        /// type is not serialisable and this event is written to disk. The
        /// whole outcome is returned from
        /// [`run`](crate::tool_call::run) for a caller that needs it, and the
        /// inner loop emits its own stream regardless.
        succeeded: bool,
    },
}

/// One thing the tool-call loop did, as the loop reports it.
///
/// Named in [ADR-0008] D3's style, with `elapsed` on every event that ends
/// something, which is D6's requirement stated for the outer loop.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A turn began. `n` is the caller's, because a session spans many turns
    /// and one call to the loop is one of them.
    TurnStarted {
        /// Which turn of the session this is.
        n: u32,
        /// The turn's finite ceiling, when one was configured.
        of: Option<u32>,
    },
    /// The model answered one exchange.
    ModelResponded {
        /// Which exchange within this turn, counting from one.
        round: u32,
        /// Tokens the provider reported for it. ADR-0012 D7.
        tokens: u64,
        /// How many tools it asked for. Zero when it answered or stopped.
        calls: usize,
        /// Time the exchange took, measured in the caller's clock.
        elapsed: Duration,
    },
    /// The model asked for a tool.
    ToolRequested {
        /// Which exchange asked for it.
        round: u32,
        /// Which call within that exchange, counting from one.
        call: u32,
        /// The tool's name.
        name: String,
    },
    /// The harness decided whether the call could act.
    ///
    /// ADR-0011 D4: "Mode may remove the prompt; it never removes the
    /// record." This event is emitted whether or not a prompt was raised, so
    /// the stream carries the decision at every mode.
    ToolPermissionDecided {
        /// Which exchange.
        round: u32,
        /// Which call.
        call: u32,
        /// The sentence the deciding surface composed, unaltered.
        statement: String,
        /// Whether the call was allowed to act.
        permitted: bool,
    },
    /// The call acted and returned.
    ToolCompleted {
        /// Which exchange.
        round: u32,
        /// Which call.
        call: u32,
        /// The tool's name.
        name: String,
        /// Whether the tool itself reported a failure.
        failed: bool,
        /// How many bytes it produced for the model. **Not the bytes.**
        content_bytes: usize,
        /// Time the call took, measured in the caller's clock.
        elapsed: Duration,
    },
    /// The call did not act, and this is why.
    ///
    /// **Not a failure.** Under a delegated coordinator ruling of 2026-09-04,
    /// recorded on ADR-0011 and ADR-0016, a declined prompt is a permission
    /// outcome and not one of D1's five classes. A consumer renders it in
    /// whatever register it renders a decision in, and never in the error
    /// one.
    ToolRefused {
        /// Which exchange.
        round: u32,
        /// Which call.
        call: u32,
        /// The tool's name.
        name: String,
        /// Why it did not act, in the refusing surface's own words.
        because: String,
        /// Time it took to reach that, measured in the caller's clock.
        elapsed: Duration,
    },
    /// The turn ended.
    TurnEnded {
        /// Which turn.
        n: u32,
        /// How it finished.
        ending: TurnEnding,
        /// How many exchanges with the model it took.
        rounds: u32,
        /// Time the whole turn took, measured in the caller's clock.
        elapsed: Duration,
    },
}

/// A consumer of the tool-call loop's stream.
///
/// Deliberately a second trait beside
/// [`EventSink`](crate::iteration::EventSink) rather than a widening of it —
/// see the module documentation. A consumer that wants both implements both,
/// which is what `zaru-cli`'s transcript writer does.
///
/// The loop constructs each event once and hands the same value to every
/// registered sink in turn, so two consumers cannot disagree about what
/// happened.
pub trait EventSink {
    /// Receive one event. Called once per event, in emission order.
    fn emit(&mut self, event: &Event);
}
