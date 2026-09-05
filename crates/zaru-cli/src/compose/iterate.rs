// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0008]'s inner loop, as the composition reaches it.
//!
//! # The two questions this module answers, and both were reserved
//!
//! **[ADR-0012]: "whether one provider implementation satisfies both traits is
//! this record's to decide when it has one."** It has one. `GeminiClient`
//! implements [`Model`] already; [`Generating`] is the same client reached
//! through the same [`Model::respond`], so the answer is *yes, one
//! implementation satisfies both* — and `Generator` is widened by nothing,
//! which is what that record's argument actually protects. Its own words:
//! "Widening `Generator` to carry tool calling would give the iteration loop a
//! capability ADR-0008 D1 assigns to the other loop." Nothing here widens it.
//! The candidate is [`Candidate`], a `zaru-cli` type opaque to `zaru-core`,
//! which sees only its `AsRef<str>`.
//!
//! **[ADR-0008]: "what an execution *is*, when a candidate is an edit rather
//! than a script, is left to whoever implements the executor port."** It is a
//! candidate applied through **the same tool surface a turn uses** — see
//! [`Applying`] and [`crate::compose::shared`]. So a candidate cannot do what
//! a turn cannot: the same [ADR-0011] D4 boundary classifies its paths, the
//! same permission decision asks about them, and the same transcript records
//! them.
//!
//! Both were decided on 2026-09-05 under directive 20 and are recorded as
//! accepted Updates on those records, open to Jeshua's veto.
//!
//! # No prompt wording is authored here, and that is what fixes the shape
//!
//! A candidate has to reach this harness somehow, and the two ways are a
//! prose instruction ("answer with a JSON object of tool calls") or the
//! provider's own function-calling contract. **The first is impossible**:
//! [`refinement::construct`](zaru_core::iteration::refinement) is fixed prose
//! in `zaru-core` and [`TurnContext::assemble`](crate::compose::TurnContext)
//! "adds no prose to any of" [`Turn`]'s three variants, so there is nowhere
//! to say it that is not a sentence this arc wrote. Authoring one would be
//! user-facing-adjacent prose deciding what a candidate is, in code.
//!
//! So the second: [`ModelRequest::tools`] already carries
//! [`descriptor_set`](crate::tools::descriptor_set), exactly as a turn's
//! first exchange does, and what comes back is what the candidate is. **One
//! client, one exchange shape, no wording.** The three arms of
//! [`ModelResponse`] map without interpretation:
//!
//! | The model answered | The candidate is |
//! | --- | --- |
//! | `Calls` | those calls, applied in order through the tool surface |
//! | `Text` | that text, with nothing to apply |
//! | `Stopped` | the provider's own reason, with nothing to apply |
//!
//! The last two are degenerate and they are **honest rather than an error**:
//! the model proposed nothing the harness can apply, the execution says so,
//! and the validators then report on a tree nothing changed — which is a
//! failing iteration, which is the loop working.
//!
//! # One gap, quoted rather than closed
//!
//! The ruling this module implements says a failed call's execution "carries
//! that call's own exit code and captured streams". **[`ToolResult`] carries
//! no exit code** — it has `content` and `failed: bool`, and `cmd.run`'s own
//! `Captured` exit code is folded into the content by the time the tool
//! surface hands a result back. So the streams travel and the number cannot.
//!
//! Nothing was widened to make it travel. That is the same shape ADR-0008's
//! Status tracking already records for a validator — "`ValidatorOutput` cannot
//! say who killed a command … the room for it has to be made in ADR-0009's
//! shape or this record's, and no code has picked an answer" — arriving one
//! port over, and it is recorded on [ADR-0011] rather than settled here.
//! [`ExecutionOutcome::exit_code`] is therefore `0` applied whole and `1`
//! otherwise, with the two non-zero cases distinguished by their streams and
//! never by the number.
//!
//! # Nothing here runs yet, and the reason is a `zaru-core` bound
//!
//! [ADR-0009] D4's branch is
//! [`InnerLoop`](zaru_core::tool_call::InnerLoop), whose `iterate` declares
//! `impl Future<…> + Send`. [`iteration::run`](zaru_core::iteration::run)
//! takes `sinks: &mut [&mut dyn EventSink]`, and
//! [`EventSink`](zaru_core::iteration::EventSink) carries no `Send` bound —
//! so the slice is `!Send`, it is alive across the run's await points, and
//! **the future can never satisfy the bound**. The compiler's own words:
//! "the trait `Send` is not implemented for `dyn zaru_core::iteration::EventSink`
//! … `[&mut events]` … has type `[&mut dyn EventSink; 1]` which is not `Send`".
//!
//! It has never bitten because nothing implemented `InnerLoop`:
//! `tool_call::run` is awaited straight from `block_on` with no `Send` bound
//! anywhere, and `compose::NoInnerLoop` satisfies the bound vacuously by being
//! uninhabited. **A transcript is not optional** — ADR-0010 D2 makes it the
//! replayable record and ADR-0008 D3 makes the event stream the contract — so
//! a run with no sink is not the way out.
//!
//! Closing it is a change to a `zaru-core` port, which is a decision rather
//! than an import, so this module carries the three implementations D4 needs
//! and stops at the seam. See the arc report.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`ModelRequest::tools`]: zaru_core::tool_call::ModelRequest
//! [`Turn`]: zaru_core::iteration::Turn

use crate::compose::Shared;
use crate::session::{Record, Transcript, TranscriptError};
use crate::tools::{Fetch, Subprocess};
use zaru_core::iteration::{
    Event, EventSink, ExecutionOutcome, Executor, Generated, Generator, PortFailure, Prompt,
};
use zaru_core::tool_call::{
    Model, ModelRequest, ModelResponse, ToolExecutor, ToolOutcome, ToolRequest,
};

/// What a generator produced, as this harness expresses it.
///
/// Opaque to `zaru-core`, which sees only [`AsRef<str>`] — the text it puts
/// under `--- ATTEMPT n ---` in the refinement prompt so the model is shown
/// its own previous attempt.
///
/// `calls` is empty on the two degenerate arms; see the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    calls: Vec<ToolRequest>,
    rendered: String,
}

impl Candidate {
    /// The calls this candidate proposes, in the order the model asked.
    #[must_use]
    pub fn calls(&self) -> &[ToolRequest] {
        &self.calls
    }

    /// One call, rendered as the request's own two fields and nothing else.
    ///
    /// **No sentence of this module's is in it.** ADR-0008 D4 forbids
    /// paraphrase on the failure-text path and the same reasoning holds one
    /// step earlier: the previous candidate goes into the next prompt, and a
    /// harness sentence wrapped around it would be the model reading a
    /// description of what it asked for rather than what it asked for.
    fn render(calls: &[ToolRequest]) -> String {
        calls
            .iter()
            .map(|call| format!("{} {}", call.name, call.arguments))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl AsRef<str> for Candidate {
    fn as_ref(&self) -> &str {
        &self.rendered
    }
}

/// The provider, as the **inner** loop's port.
///
/// Holds the same [`Model`] the outer loop holds. See the module
/// documentation for why this is one implementation of two traits rather than
/// one widened trait.
#[derive(Debug)]
pub struct Generating<'a, M: ?Sized> {
    model: &'a M,
}

impl<'a, M: ?Sized> Generating<'a, M> {
    /// Generate candidates from this model.
    #[must_use]
    pub const fn over(model: &'a M) -> Self {
        Self { model }
    }
}

impl<M> Generator for Generating<'_, M>
where
    M: Model + Sync + ?Sized,
{
    type Candidate = Candidate;

    /// Ask once, with the tools a turn would offer, and take what came back.
    ///
    /// `results` is empty because an iteration's exchange is a first exchange
    /// every time: the loop's memory is the refinement prompt, which
    /// [`ContextPolicy::assemble`] has already folded into `prompt`.
    async fn generate(&self, prompt: &Prompt) -> Result<Generated<Candidate>, PortFailure> {
        let response = self
            .model
            .respond(&ModelRequest {
                prompt,
                tools: crate::tools::descriptor_set(),
                results: &[],
            })
            .await?;
        let tokens = response.tokens().total();
        let candidate = match response {
            ModelResponse::Calls { calls, .. } => Candidate {
                rendered: Candidate::render(&calls),
                calls,
            },
            ModelResponse::Text { text, .. } => Candidate {
                calls: Vec::new(),
                rendered: text,
            },
            ModelResponse::Stopped { reason, .. } => Candidate {
                calls: Vec::new(),
                rendered: reason,
            },
        };
        Ok(Generated { candidate, tokens })
    }
}

/// Making a candidate's effect real, through the tool surface a turn uses.
///
/// See [`crate::compose::shared`]: this holds the **same** executor, so every
/// boundary, permission decision and transcript record a turn's call gets is
/// a candidate's call's too, by construction rather than by rule.
#[derive(Debug)]
pub struct Applying<'m, 'e, C, F> {
    surface: Shared<'m, 'e, C, F>,
}

impl<'m, 'e, C, F> Applying<'m, 'e, C, F> {
    /// Apply candidates through this surface.
    #[must_use]
    pub const fn through(surface: Shared<'m, 'e, C, F>) -> Self {
        Self { surface }
    }
}

impl<C, F> Executor for Applying<'_, '_, C, F>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
{
    type Candidate = Candidate;

    /// Apply every call in order, and stop at the first refusal.
    ///
    /// **A refusal stops the candidate.** ADR-0011 D3's prompt is a question
    /// about one call, and a candidate whose second write the user declined
    /// is not a candidate that should have its third applied: what the user
    /// refused was a step of a change, and continuing past it would apply
    /// part of something they said no to. So nothing is applied after a
    /// refusal, and the outcome says which call it was in the refusing
    /// surface's own words.
    ///
    /// `exit_code` is `0` when every call completed and none reported a
    /// failure, and `1` otherwise — see the module documentation for the one
    /// number the port cannot carry.
    async fn execute(&self, candidate: &Candidate) -> Result<ExecutionOutcome, PortFailure> {
        let mut surface = self.surface;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut failed = false;

        for call in &candidate.calls {
            match surface.execute(call).await? {
                ToolOutcome::Completed { result, .. } => {
                    if !stdout.is_empty() {
                        stdout.push('\n');
                    }
                    stdout.push_str(result.content.as_str());
                    if result.failed {
                        failed = true;
                    }
                }
                ToolOutcome::Refused { because, .. } => {
                    if !stderr.is_empty() {
                        stderr.push('\n');
                    }
                    stderr.push_str(&because);
                    failed = true;
                    break;
                }
            }
        }

        Ok(ExecutionOutcome {
            exit_code: i32::from(failed),
            stdout,
            stderr,
        })
    }
}

/// Writes [ADR-0008] D3's **inner** loop's events into ADR-0010 D2's
/// transcript.
///
/// A third handle on the one file, for the reason
/// [`Records`](crate::compose::Records) already gives for the second: one
/// writer type with several handles is not several writers with several
/// rules, every record still leaves through `Transcript::record`, and the turn
/// runs on one thread so the interleaving on disk is the real chronological
/// order.
///
/// It writes [`Record::Loop`], which is the variant that has been waiting for
/// a producer since the session lifecycle landed — D3's eight events are
/// iteration-shaped and this is the loop that shapes them.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[derive(Debug)]
pub struct Iterations {
    transcript: Transcript,
    first_failure: Option<TranscriptError>,
    written: usize,
}

impl Iterations {
    /// Open a handle on a session's transcript for the iteration's events.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Io`] when the file cannot be opened for appending.
    pub fn appending_to(path: impl Into<std::path::PathBuf>) -> Result<Self, TranscriptError> {
        Ok(Self {
            transcript: Transcript::append_to(path)?,
            first_failure: None,
            written: 0,
        })
    }

    /// How many events reached the file.
    #[must_use]
    pub const fn written(&self) -> usize {
        self.written
    }

    /// The first write that failed, if one did.
    #[must_use]
    pub const fn first_failure(&self) -> Option<&TranscriptError> {
        self.first_failure.as_ref()
    }

    /// What this sink recorded, taken by value.
    ///
    /// [`TranscriptError`] is deliberately not [`Clone`] — it carries an
    /// `std::io::Error`, which is not — so a caller that needs to keep the
    /// failure past the sink's lifetime takes it rather than copying it.
    #[must_use]
    pub fn into_report(self) -> (usize, Option<TranscriptError>) {
        (self.written, self.first_failure)
    }
}

impl EventSink for Iterations {
    fn emit(&mut self, event: &Event) {
        match self.transcript.record(&Record::Loop(event.clone())) {
            Ok(()) => self.written += 1,
            Err(failure) => {
                if self.first_failure.is_none() {
                    self.first_failure = Some(failure);
                }
            }
        }
    }
}
