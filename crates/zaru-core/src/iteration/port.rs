// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The ports the loop calls out through, and the values that cross them.
//!
//! Every port is a trait declared here and implemented elsewhere. **Nothing
//! in this crate's product tree implements one**, and nothing in this crate
//! opens a socket, spawns a process, or reads a clock outside
//! [`SystemClock`]. That is what makes ADR-0008 D2's headless requirement a
//! property of the code rather than a claim about it.
//!
//! Each port belongs to a decision this crate does not make. Generation is
//! ADR-0012's; the declared validators behind [`Validators`] are ADR-0009's;
//! the layering and compaction behind [`ContextPolicy`] are ADR-0013's. What
//! this crate owns is the dispatch and the assembly — the code that iterates
//! validators, emits their events, and invokes the context policy at an
//! iteration boundary — and each individual validator and the policy itself
//! sit behind these traits as a seam inside the crate.
//!
//! The port methods return `impl Future` rather than being `async fn` so that
//! the `Send` bound is stated rather than inferred, and so that this crate
//! needs no asynchronous runtime of its own.

use crate::conversation::Message;
use crate::iteration::event::ValidatorOutcome;
use crate::iteration::refinement::RefinementPrompt;
use crate::redaction::Redacted;
use core::fmt;
use core::future::Future;
use core::time::Duration;

/// A port refused or failed.
///
/// The loop never interprets one. It carries the detail out unchanged, and
/// `zaru-cli` classifies it under ADR-0016's taxonomy — a provider outage is
/// environmental, a missing credential is user-correctable, and neither is
/// the loop failing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortFailure {
    /// What the port said went wrong, in its own words.
    pub detail: String,
}

impl PortFailure {
    /// Report a failure with the port's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PortFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

/// What the loop hands a model: the system text, the earlier conversation,
/// and this turn's own message.
///
/// Assembled by the [`ContextPolicy`], never by the loop: what a model
/// actually sees is ADR-0013's layering applied to what this crate produced.
///
/// # Three parts, because a provider has three places to put them
///
/// - **The system text** is ADR-0013 D1's layers 1 to 4, the stable prefix.
///   Every provider this harness speaks to has a role for it that is not the
///   person's: a `system` message, or Gemini's `systemInstruction`. Sending it
///   as a person's message would make the model read the harness's own
///   instructions as something the person typed.
/// - **The history** is layer 6: every earlier turn, as the
///   [`Message`]s it was.
/// - **The task** is this turn's own message: what the person asked, or the
///   refinement the iteration loop built, with anything ADR-0013 renders
///   beside it (layers 5 and 7).
///
/// # It can only be built from redacted text, and that is the mechanism
///
/// ADR-0008's trigger clause 6 was decided on 2026-09-05: every path from
/// captured bytes into a model prompt passes one
/// [`Redactor`](crate::redaction::Redactor). [`Self::new`] takes a
/// [`Redacted`]; [`Self::assembled`] takes the redactor itself and passes
/// every part through it. A path that forgot the port does not compile. See
/// [`crate::redaction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    system: Option<Redacted>,
    history: Vec<Message>,
    task: Redacted,
}

impl Prompt {
    /// A prompt that is a task and nothing else: no system text and no
    /// earlier conversation.
    #[must_use]
    pub const fn new(task: Redacted) -> Self {
        Self {
            system: None,
            history: Vec::new(),
            task,
        }
    }

    /// A whole prompt, every part of it passed through `redactor`.
    ///
    /// An empty `system` is no system text at all, rather than an empty one.
    #[must_use]
    pub fn assembled<R: crate::redaction::Redactor + ?Sized>(
        redactor: &R,
        system: &str,
        history: &[Message],
        task: &str,
    ) -> Self {
        Self {
            system: (!system.is_empty()).then(|| Redacted::by(redactor, system)),
            history: history
                .iter()
                .map(|message| message.redacted(redactor))
                .collect(),
            task: Redacted::by(redactor, task),
        }
    }

    /// The system text, if there is any.
    #[must_use]
    pub fn system(&self) -> Option<&str> {
        self.system.as_ref().map(Redacted::as_str)
    }

    /// Every earlier turn, oldest first.
    #[must_use]
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// This turn's own message.
    #[must_use]
    pub fn task(&self) -> &str {
        self.task.as_str()
    }

    /// Everything as one text: the system text, each earlier message and the
    /// task, separated by a blank line.
    ///
    /// **Not what a provider is sent**; each provider maps the parts to its
    /// own roles. This is the text a length is counted over, so that every
    /// byte of every part is counted once.
    #[must_use]
    pub fn rendered(&self) -> String {
        let mut out = String::new();
        let mut push = |part: &str| {
            if part.is_empty() {
                return;
            }
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(part);
        };
        push(self.system().unwrap_or_default());
        for message in &self.history {
            push(&message.rendered());
        }
        push(self.task.as_str());
        out
    }
}

/// What a generator returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated<C> {
    /// The candidate, opaque to the loop.
    pub candidate: C,
    /// Tokens the provider reported for this generation.
    pub tokens: u64,
}

/// What making a candidate's effect real produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutcome {
    /// The exit code the execution reported.
    pub exit_code: i32,
    /// Everything the execution wrote to standard output.
    pub stdout: String,
    /// Everything the execution wrote to standard error.
    pub stderr: String,
}

/// What one declared validator reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorReport {
    /// The validator's declared name, per ADR-0009 D1.
    pub name: String,
    /// What it reported.
    pub outcome: ValidatorOutcome,
    /// The validator's own output. ADR-0009 D5 sends this into refinement
    /// verbatim, so nothing on this path may paraphrase it.
    pub detail: String,
}

/// What the loop asks the context policy to assemble a prompt from.
#[derive(Debug)]
pub enum Turn<'a> {
    /// The first iteration, carrying the caller's task and nothing else.
    Initial {
        /// The work the user asked for.
        task: &'a str,
    },
    /// Any later iteration, carrying the refinement this crate constructed.
    Refinement {
        /// The refinement prompt, built from the previous iteration.
        refinement: &'a RefinementPrompt,
    },
}

/// Produces candidates. ADR-0012 owns what is behind this.
pub trait Generator {
    /// A candidate. The loop moves it from here to [`Executor`] and takes its
    /// text for the refinement prompt, which ADR-0008 D1 requires the prompt
    /// to carry, and does nothing else with it.
    type Candidate: AsRef<str>;

    /// Produce one candidate for this prompt.
    fn generate(
        &self,
        prompt: &Prompt,
    ) -> impl Future<Output = Result<Generated<Self::Candidate>, PortFailure>> + Send;
}

/// Makes a candidate's effect real. ADR-0003 D2 forbids this crate carrying a
/// container library, so what an execution *is* — an edit applied, a command
/// run, a container started through the orchestrator — belongs to whoever
/// implements this and to no decision made here.
pub trait Executor {
    /// The candidate this executor accepts.
    type Candidate;

    /// Execute one candidate and report what happened.
    fn execute(
        &self,
        candidate: &Self::Candidate,
    ) -> impl Future<Output = Result<ExecutionOutcome, PortFailure>> + Send;
}

/// Runs the declared validators against an execution. ADR-0009 owns the
/// manifest, the four `expect` kinds, and the `after` ordering; this port is
/// how their reports reach the loop.
pub trait Validators {
    /// Report on one execution, one entry per validator that was considered,
    /// in the order ADR-0009 D2's declared dependencies put them.
    fn evaluate(
        &self,
        execution: &ExecutionOutcome,
    ) -> impl Future<Output = Result<Vec<ValidatorReport>, PortFailure>> + Send;
}

/// Why the context policy did not return a prompt.
///
/// Two things, kept apart because ADR-0008 D5 puts them in different
/// registers. A policy that could not run is an error: something the loop
/// depended on was unavailable, and `zaru-cli` classifies it. A context that
/// would not fit is not an error at all — it is ADR-0013 D7's second route to
/// exhaustion, and D5 says exhaustion is neither an error nor a success. A
/// single [`PortFailure`] could carry only the first, so the second would
/// have arrived in the error register and been reported as the mechanism
/// breaking rather than as the mechanism reaching its limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRefusal {
    /// The policy failed. This is an error.
    Failed(PortFailure),
    /// Assembling this iteration would exceed the context window.
    ///
    /// ADR-0013 D7: an iteration that would exceed the window "fails as
    /// exhausted with a clear reason rather than continuing on a rewritten
    /// context". Compaction is not available here, because D7 confines it to
    /// turn boundaries and an iteration happens inside a turn.
    WindowExceeded {
        /// Tokens the assembled context would have needed.
        needed: u64,
        /// Tokens the window allows.
        window: u64,
    },
}

impl From<PortFailure> for ContextRefusal {
    fn from(failure: PortFailure) -> Self {
        Self::Failed(failure)
    }
}

impl fmt::Display for ContextRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed(failure) => failure.fmt(f),
            Self::WindowExceeded { needed, window } => write!(
                f,
                "the assembled context needs {needed} tokens and the window allows {window}"
            ),
        }
    }
}

/// Assembles what the model actually sees. ADR-0013 owns the layering, the
/// compaction, and the thresholds; the loop only says when.
///
/// **Assembly cannot compact.** ADR-0013 D7 confines compaction to turn
/// boundaries and this method is called at an iteration boundary, which is
/// inside a turn — so a policy under pressure refuses with
/// [`ContextRefusal::WindowExceeded`] rather than rewriting what the model
/// was looking at partway through a cycle.
pub trait ContextPolicy {
    /// Assemble the prompt for the iteration that is about to begin.
    fn assemble(
        &self,
        turn: &Turn<'_>,
    ) -> impl Future<Output = Result<Prompt, ContextRefusal>> + Send;
}

/// Where elapsed time comes from.
///
/// ADR-0008 D6 puts elapsed time on every iteration, and the testing contract
/// forbids asserting on wall-clock time. Readings are monotonic offsets
/// rather than instants because an instant cannot be constructed at a chosen
/// value, and a clock a test cannot set is a clock a test cannot assert on.
/// The loop only ever takes differences.
pub trait Clock {
    /// How long since this clock started.
    fn now(&self) -> Duration;
}

/// The clock the product uses.
///
/// **This is the only place in this crate that reads the machine's clock.**
/// Everything else takes a [`Clock`], which is what makes every elapsed time
/// in the event stream assertable against exact values in a test.
#[derive(Debug)]
pub struct SystemClock {
    started: std::time::Instant,
}

impl SystemClock {
    /// Start a clock now.
    #[must_use]
    pub fn started_now() -> Self {
        Self {
            started: std::time::Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::started_now()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

/// The six ports one run of the loop needs.
///
/// Bundled because the loop takes them together and a function taking each
/// separately is a function whose argument order is a thing to get wrong.
#[derive(Debug)]
pub struct Ports<'a, G, X, V, P, K, R: ?Sized> {
    /// Produces candidates.
    pub generator: &'a G,
    /// Makes a candidate's effect real.
    pub executor: &'a X,
    /// Runs the declared validators.
    pub validators: &'a V,
    /// Assembles what the model sees.
    pub context: &'a P,
    /// Supplies elapsed time.
    pub clock: &'a K,
    /// Removes the harness's own secrets from what reaches the model.
    ///
    /// ADR-0008 trigger clause 6's port, decided 2026-09-05. The loop hands
    /// it to the refinement construction, which is the first of the paths
    /// from captured bytes into a prompt; see [`crate::redaction`].
    pub redactor: &'a R,
}
