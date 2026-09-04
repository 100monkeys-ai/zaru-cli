// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The typed event stream and the contract every consumer subscribes to.
//!
//! ADR-0008 D3 makes this stream the whole contract between the loop and the
//! terminal, the transcript, a future desktop surface, and tests. Rendering
//! never reads loop internals: what a consumer needs to display, the loop
//! emits.
//!
//! Events are a superset of state transitions rather than a bijection with
//! them. [`Event::ValidatorEvaluated`] fires inside `Evaluate`, once for each
//! validator the validators port reported, and is not a transition.
//!
//! # Why these three types are serialisable, and why nothing else here is
//!
//! [ADR-0010] D2 makes the transcript this stream, written to
//! `transcript.jsonl` one event per line as it occurs. The writer lives in
//! `zaru-cli`, which is where [Bounded Contexts] puts the session lifecycle,
//! and it cannot write an event it cannot serialise. The alternative -- a
//! mirror of this enum in that crate -- would be a **second declaration of
//! D3's event stream**, and a rule that exists in two places diverges; the
//! `Layer` duplication removed from `zaru-cli` on 2026-09-04 is the same
//! shape, measured after the fact to have already drifted. So the derives are
//! here, on the declaration, under a **delegated coordinator ruling of
//! 2026-09-04** open to Jeshua's veto.
//!
//! They add no field, no method, no variant and no sibling edge, so nothing
//! about the loop's behaviour changes. Serialisation is not a *use* of this
//! crate by itself: nothing here writes a byte anywhere, and `serde` on its
//! own emits no format at all -- the format crate is the consumer's, which is
//! [ADR-0003] D2's serialisation row read as its first proposed amendment
//! reads it.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// What one declared validator reported about an execution.
///
/// Three outcomes rather than a boolean, because ADR-0009 D2 makes `skipped`
/// distinct from `passed` and `failed` on purpose: a validator whose
/// prerequisite failed did not run, and reporting that as a pass is exactly
/// the silent green that decision exists to prevent. There is no score —
/// ADR-0009 D3 specifies binary pass and fail, and gradient scoring is an
/// open question that record sends to the backlog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorOutcome {
    /// The validator ran and its expectation held.
    Passed,
    /// The validator ran and its expectation did not hold.
    Failed,
    /// The validator did not run, because a validator it declared `after` failed.
    Skipped,
}

/// Why the loop stopped without succeeding.
///
/// ADR-0008 D5 makes exhaustion a distinct outcome rather than an error or a
/// success. One variant today. ADR-0013 D7 describes a second route — an
/// iteration that would exceed the context window — which is out of this
/// crate's scope until that record is built; this enum is where it attaches,
/// so that adding it is a visible act rather than a new boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExhaustionReason {
    /// The iteration ceiling the caller passed was reached.
    CeilingReached,
}

/// One thing the loop did, as the loop reports it.
///
/// The field names and the variant names are ADR-0008 D3's, with three
/// additions recorded as a proposed Update on that record: `outcome` replaces
/// D3's `passed` and `score` on [`Event::ValidatorEvaluated`], `elapsed`
/// appears on every event that ends an iteration so that D6's per-iteration
/// elapsed time has a carrier, and `reason` appears on
/// [`Event::LoopExhausted`] so that a second exhaustion route would be
/// distinguishable to a consumer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// An iteration began. `of` is the ceiling the caller passed.
    IterationStarted {
        /// Which iteration this is, counting from one.
        n: u32,
        /// The iteration ceiling, so a consumer can render "2 of 5".
        of: u32,
    },
    /// The generation port returned a candidate.
    CandidateGenerated {
        /// Tokens the provider reported for this generation.
        tokens: u64,
        /// Time the generation took, measured in the caller's clock.
        elapsed: Duration,
    },
    /// The execution port made the candidate's effect real and returned.
    ExecutionCompleted {
        /// The exit code the execution reported.
        exit_code: i32,
        /// How many bytes of standard output the execution produced.
        stdout_bytes: usize,
        /// How many bytes of standard error the execution produced.
        stderr_bytes: usize,
        /// Time the execution took, measured in the caller's clock.
        elapsed: Duration,
    },
    /// One declared validator reported on the execution.
    ValidatorEvaluated {
        /// The validator's declared name.
        name: String,
        /// What it reported.
        outcome: ValidatorOutcome,
        /// The validator's own output, unaltered.
        detail: String,
    },
    /// An iteration ended without every validator passing.
    IterationFailed {
        /// Which iteration failed, counting from one.
        n: u32,
        /// The failing validators' own output, verbatim and untruncated.
        reason: String,
        /// Time this whole iteration took, measured in the caller's clock.
        elapsed: Duration,
    },
    /// The refinement prompt for the next iteration was constructed.
    RefinementConstructed {
        /// The iteration whose failure the refinement was built from.
        n: u32,
        /// The failure text as it went into the prompt, after truncation.
        failure_excerpt: String,
    },
    /// Every validator passed and the loop finished.
    LoopSucceeded {
        /// How many iterations ran.
        iterations: u32,
        /// Time the final iteration took, measured in the caller's clock.
        elapsed: Duration,
        /// Time the whole loop took, measured in the caller's clock.
        total_elapsed: Duration,
    },
    /// The loop stopped without succeeding.
    LoopExhausted {
        /// How many iterations ran.
        iterations: u32,
        /// Why it stopped.
        reason: ExhaustionReason,
        /// The final iteration's failing validators' output, verbatim.
        last_failure: String,
    },
}

/// A consumer of the event stream.
///
/// `zaru-tui` implements this and renders; a transcript writer implements it
/// and appends. Both depend on this crate, so this crate depends on neither —
/// which is ADR-0003 D8's prohibition made structural rather than remembered,
/// since Cargo refuses dependency cycles.
///
/// The loop constructs each event once and hands the same value to every
/// registered sink in turn, so two consumers cannot disagree about what
/// happened.
pub trait EventSink {
    /// Receive one event. Called once per event, in emission order.
    fn emit(&mut self, event: &Event);
}
