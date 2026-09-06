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
//! [`ExecutionOutcome`]'s `exit_code` is therefore `0` applied whole and `1`
//! otherwise, with the two non-zero cases distinguished by their streams and
//! never by the number.
//!
//! # One `zaru-core` bound had to be stated for any of this to run
//!
//! [ADR-0009] D4's branch is
//! [`InnerLoop`], whose `iterate` declares
//! `impl Future<…> + Send`. [`iteration::run`](zaru_core::iteration::run)
//! holds `sinks: &mut [&mut dyn EventSink]` across every await point, so
//! until 2026-09-05 the two could not both be satisfied and **no inhabited
//! implementation of `InnerLoop` could exist**. It went unnoticed because
//! none did: `tool_call::run` is awaited straight from `block_on` with no
//! `Send` bound on the path, and the uninhabited stand-in this composition
//! passed at that branch satisfied the bound vacuously — it is deleted now
//! that there is something to pass instead.
//!
//! [`EventSink`] now carries `Send`. Every
//! implementation in both crates already did — the whole workspace compiled
//! with the bound added and nothing else changed — so it records what was
//! already true. Decided under directive 20 and on that trait's own
//! documentation.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [`ModelRequest::tools`]: zaru_core::tool_call::ModelRequest
//! [`Turn`]: zaru_core::iteration::Turn
//! [`ToolResult`]: zaru_core::tool_call::ToolResult

use crate::compose::Shared;
use crate::session::{Record, Transcript, TranscriptError};
use crate::tools::{Fetch, Subprocess};
use zaru_core::iteration::{
    Clock, ContextPolicy, Event, EventSink, ExecutionOutcome, Executor, Generated, Generator,
    IterationError, Limits, Outcome, PortFailure, Ports, Prompt, Validators,
};
use zaru_core::redaction::Redactor;
use zaru_core::tool_call::{
    InnerLoop, Model, ModelRequest, ModelResponse, ToolExecutor, ToolOutcome, ToolRequest,
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

/// What an iteration left behind for the composition to classify.
#[derive(Debug, Default)]
pub struct Kept {
    /// The typed error, where a port failed.
    pub error: Option<IterationError>,
    /// The first transcript write that failed, where one did.
    pub transcript: Option<TranscriptError>,
    /// How many iteration events reached the file.
    pub written: usize,
}

/// A consumer of the inner loop's stream, reached through a shared reference.
///
/// [ADR-0028] D3: "The harness renders the loop's typed events per [ADR-0008]
/// D3 … **Neither reconstructs the narrative from inference**." This is the
/// seam that consumer arrives on.
///
/// # Why a port and not a second `EventSink` on a slice
///
/// The outer loop takes its extra consumers as `&mut [&mut dyn EventSink]`,
/// which [`crate::compose::turn::run_one`] fills from the terminal. The inner
/// loop cannot: [`InnerLoop::iterate`] takes `&self`, so a caller holding an
/// `&mut` to a sink for the length of a turn has nothing to hand it. Widening
/// `iterate` to `&mut self` is a `zaru-core` change, and [ADR-0008] D2 keeps
/// that crate headless — the seam belongs on this side.
///
/// So the sink is reached by shared reference and the adapter that makes it an
/// [`EventSink`] is built **inside** the run,
/// where the `&mut` it needs lives for exactly as long as the slice does. An
/// implementation is therefore responsible for its own interior mutability;
/// [`crate::terminal::driver::PaneNarrator`] holds a `Mutex` it already shared with
/// the pane's other consumers.
///
/// `Sync` because [`iterate`](InnerLoop::iterate) declares
/// `impl Future<…> + Send` and this reference is held across every await point
/// in the run — the same bound, for the same reason, that
/// [`EventSink`] carries.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
pub trait Narrator: Sync {
    /// Receive one event. Called once per event, in emission order.
    fn narrate(&self, event: &zaru_core::iteration::Event);

    /// The turn stopped because the user interrupted it, and the session was
    /// kept.
    ///
    /// # Why this is on the port rather than beside it
    ///
    /// It is the one thing a narrating consumer must be told that
    /// [`zaru_core::iteration::Event`] cannot carry. The interruption is a
    /// **dropped future**: the turn's own poll ends between two events, so
    /// there is no event to emit and [ADR-0008] D2 keeps `zaru-core` headless,
    /// which makes widening D3's event list that record's decision rather than
    /// an implementer's — the same boundary [`Narrator`] itself exists to
    /// respect.
    ///
    /// So the port gains a method and no second seam is built. A consumer that
    /// paints the loop's narrative is the consumer that should say the
    /// narrative stopped; routing it through [`crate::terminal::driver::Pane`]
    /// directly would be a second place the pane is told about a turn, and
    /// [ADR-0028] D3's "the harness renders the loop's typed events" would
    /// then be true of every line but this one.
    ///
    /// **It takes no argument**, because there is nothing about the
    /// interruption a caller knows that the transcript does not already hold:
    /// a call in flight left a `Phase::Started` with no `Phase::Completed`, and
    /// that pair *is* the interruption [ADR-0010] D4 derives on the next read.
    /// Authoring anything else here would be inventing a record of what
    /// happened.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    fn announce_interrupted(&self);

    /// Say it, and hand back the proof that it was said.
    ///
    /// # The witness, because the mutation reddened nothing
    ///
    /// `run_a_turn` needs a [`Prepared`](crate::compose::Prepared) to reach,
    /// which needs a provider client and a key, so **no offline check can
    /// drive it** — and a mutation that deleted the call from its interrupted
    /// arm left every check green. That is the same finding
    /// [`crate::terminal::driver::Pane`]'s `Drop` records for the streamed
    /// line, and it has the same answer: put the property where the compiler
    /// holds it rather than where somebody remembers it.
    ///
    /// [`Narrated`] has one constructor and it is inside this module, so the
    /// only way to obtain one is this method, which always announces first.
    /// `driver::Turned::Interrupted` carries one, so an interrupted turn that
    /// did not tell the pane **does not compile**.
    ///
    /// Provided rather than required, so an implementer writes
    /// [`Self::announce_interrupted`] and cannot accidentally mint a witness
    /// without announcing.
    fn interrupted(&self) -> Narrated {
        self.announce_interrupted();
        Narrated(())
    }
}

/// Proof that a [`Narrator`] was told a turn had been interrupted.
///
/// One private field and no constructor outside this module, which is the
/// absent-constructor mechanism [ADR-0008] clause 6's `Redacted` already uses:
/// a rule the type system holds rather than one a reader keeps. See
/// [`Narrator::interrupted`] for the mutation that produced it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[derive(Debug)]
pub struct Narrated(());

/// A [`Narrator`] as the slice's [`EventSink`].
///
/// One field and no state of its own, so nothing here can hold a lock guard
/// across an await: `narrate` takes `&self`, returns before the next line, and
/// whatever it locked is released inside it.
struct Narrating<'a>(&'a dyn Narrator);

impl zaru_core::iteration::EventSink for Narrating<'_> {
    fn emit(&mut self, event: &zaru_core::iteration::Event) {
        self.0.narrate(event);
    }
}

/// [ADR-0009] D4's inner loop, as the outer loop reaches it.
///
/// # The typed error is kept rather than widened
///
/// [`InnerLoop::iterate`] returns `Result<Outcome, PortFailure>`, so the
/// [`IterationError`]'s own `PortKind` — which of the six ports failed, and on
/// which iteration — is flattened away before the composition sees it. A
/// provider outage and an unusable `matches` pattern are different classes
/// under [ADR-0016] D1 and a `String` cannot tell them apart.
///
/// So the typed value is kept here and read back afterwards, which is the
/// shape [`Classifying`](crate::compose::Classifying) already uses for a
/// `GeminiFailure` and for the same reason. **No port was widened**: the
/// alternative was a second error type on `InnerLoop`, which would decide for
/// `zaru-core` that the outer loop knows about the inner loop's ports.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub struct Inner<'a, G, X, V, P, K, R: ?Sized> {
    ports: Ports<'a, G, X, V, P, K, R>,
    limits: Limits,
    transcript: &'a std::path::Path,
    narrator: Option<&'a dyn Narrator>,
    kept: std::sync::Mutex<Kept>,
}

/// Written rather than derived, because `Narrator` is a trait object and a
/// derived `Debug` would either require every implementation to be `Debug` or
/// bound this impl on six type parameters that have no reason to be. What a
/// reader wants from it is the limits and whether anything is subscribed.
impl<G, X, V, P, K, R: ?Sized> core::fmt::Debug for Inner<'_, G, X, V, P, K, R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Inner")
            .field("limits", &self.limits)
            .field("transcript", &self.transcript)
            .field("narrating", &self.narrator.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a, G, X, V, P, K, R: ?Sized> Inner<'a, G, X, V, P, K, R> {
    /// Run the iteration loop over these ports, under these limits.
    ///
    /// `narrator` is [ADR-0028] D3's subscriber, and `None` is the composition
    /// with no terminal — `zaru "<task>"` writing its transcript and printing
    /// an outcome. See [`Narrator`] for why it is a port taking `&self` rather
    /// than a second `EventSink` on a slice.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    #[must_use]
    pub fn over(
        ports: Ports<'a, G, X, V, P, K, R>,
        limits: Limits,
        transcript: &'a std::path::Path,
        narrator: Option<&'a dyn Narrator>,
    ) -> Self {
        Self {
            ports,
            limits,
            transcript,
            narrator,
            kept: std::sync::Mutex::new(Kept::default()),
        }
    }

    /// What the run left behind, read after it.
    ///
    /// A poisoned lock means a previous holder panicked while moving three
    /// small values, which cannot happen; the value is taken either way rather
    /// than propagating a panic out of a run that finished.
    #[must_use]
    pub fn kept(&self) -> Kept {
        let mut slot = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        core::mem::take(&mut *slot)
    }
}

impl<G, X, V, P, K, R> InnerLoop for Inner<'_, G, X, V, P, K, R>
where
    G: Generator + Sync,
    G::Candidate: Send,
    X: Executor<Candidate = G::Candidate> + Sync,
    V: Validators + Sync,
    P: ContextPolicy + Sync,
    K: Clock + Sync,
    R: Redactor + Sync + ?Sized,
{
    async fn iterate(&self, task: &str) -> Result<Outcome, PortFailure> {
        // The sink is opened here rather than held, so that no lock guard is
        // alive across the run's await points -- `iterate` takes `&self` and a
        // `std::sync::MutexGuard` held across an await would make this future
        // `!Send`, which the port declares it is not.
        let mut events = Iterations::appending_to(self.transcript)
            .map_err(|failure| PortFailure::new(failure.to_string()))?;

        // ADR-0008 clause 3's slice, for the loop whose eight events D3
        // actually enumerates. The transcript writer first, for the reason
        // `compose::turn` gives on the outer loop's slice: a renderer that
        // painted an event the file does not hold would be showing the user
        // something a resume could not reproduce.
        let mut narrating = self.narrator.map(Narrating);
        let mut sinks: Vec<&mut dyn zaru_core::iteration::EventSink> = vec![&mut events];
        if let Some(narrating) = narrating.as_mut() {
            sinks.push(narrating);
        }

        let outcome = zaru_core::iteration::run(
            task,
            self.limits,
            Ports {
                generator: self.ports.generator,
                executor: self.ports.executor,
                validators: self.ports.validators,
                context: self.ports.context,
                clock: self.ports.clock,
                redactor: self.ports.redactor,
            },
            &mut sinks,
        )
        .await;

        let (written, transcript) = events.into_report();
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.written = written;
        kept.transcript = transcript;
        match outcome {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let said = error.to_string();
                kept.error = Some(error);
                Err(PortFailure::new(said))
            }
        }
    }
}
