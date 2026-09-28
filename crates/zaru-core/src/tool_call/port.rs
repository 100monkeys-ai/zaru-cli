// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The ports the tool-call loop calls out through, and the values that cross
//! them.
//!
//! Every port is a trait declared here and implemented elsewhere. **Nothing
//! in this crate's product tree implements one**, and nothing here opens a
//! socket, spawns a process, touches a file, or calls a provider. That is the
//! same discipline [`crate::iteration::port`] already holds for the inner
//! loop, and it is what makes ADR-0008 D2's headless requirement a property
//! of the code rather than a claim about it.
//!
//! # What this crate knows about a tool, which is almost nothing
//!
//! A name, an opaque argument string, an opaque result, and a sentence
//! somebody else composed. It does not know [ADR-0011]'s seven built-ins, its
//! three permission modes, its working-directory boundary, or its allowlist —
//! all of those are `zaru-cli`'s, where [Bounded Contexts] puts them. The
//! executing half implements [`ToolExecutor`] over the permission decision
//! that record already built; this crate learns the outcome and the sentence
//! and nothing else.
//!
//! **The arguments and the result are opaque strings on purpose.** Typing
//! them would mean declaring a schema for each of D1's seven, and no record
//! declares one — see the open question this arc raised on ADR-0011.
//!
//! # The model is a port and no provider is called
//!
//! [ADR-0012] D3 decides what is behind [`Model`]: four provider kinds, one
//! trait, "streaming, tool calling, token accounting, and a capability
//! descriptor". Three of those four are here. **Streaming is not**, because a
//! stream contract with no provider behind it is a shape nobody chose; it
//! lands with that record's own arc, and its absence is recorded on ADR-0012.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

use crate::conversation::Message;
use crate::iteration::port::{PortFailure, Prompt};
use crate::redaction::{Redacted, Redactor};
use core::fmt;
use core::future::Future;
use serde::{Deserialize, Serialize};

/// One tool the model may ask for, as the model is told about it.
///
/// The parameter schema is an opaque string. ADR-0011 D1 names seven tools
/// and no argument shapes for any of them, so a type here would be a schema
/// this crate invented — recorded as an open question on that record instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDescriptor {
    /// The tool's name, as the surface that owns it spells it.
    pub name: String,
    /// What it does, in the owning surface's words.
    pub description: String,
    /// The parameter schema, opaque to this crate.
    pub parameters: String,
}

/// One tool call the model asked for.
///
/// Serialisable because it is part of a [`Message`] the transcript keeps: a
/// resumed session sends the model its earlier calls with the arguments it
/// asked them with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    /// The provider's own identifier for this call.
    ///
    /// Carried rather than generated, because a provider that correlates
    /// results to calls by its own id and a harness that renumbers them
    /// disagree about which result answered which call.
    pub id: String,
    /// Which tool. Meaningful to the executor and to nothing here.
    pub name: String,
    /// The arguments, opaque to this crate.
    pub arguments: String,
}

/// What the harness had to do before a call could act, in the decider's own
/// words.
///
/// Composed by whoever made the decision and carried through unchanged, so
/// that what the user was told and what this loop reports cannot drift apart
/// — the shape ADR-0011's `Question` and ADR-0007 D8's confirmation already
/// use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDecision {
    /// The whole sentence, including any marking the deciding surface applied.
    pub statement: String,
    /// Whether the call was allowed to act.
    pub permitted: bool,
}

/// What one tool call produced, as the model is given it.
///
/// The content is [`Redacted`], because this type is the only text on a
/// [`ModelRequest`] besides the [`Prompt`] and it is therefore the second
/// half of ADR-0008 clause 6's type gate. **A tool's output reaches a model
/// through here and not through a prompt**, so a `Redacted`-only `Prompt`
/// would not cover it.
///
/// Distinct from [`ToolOutcome`], which carries the executing surface's own
/// raw bytes: ADR-0010's transcript records what the session contained, and
/// the event stream is what that transcript is written from. The redaction
/// happens at exactly one point, where a `ToolOutcome` becomes model-bound
/// text — [`ToolOutcome::for_the_model`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// The request's id, so a provider can match it to what it asked for.
    pub id: String,
    /// What goes back to the model.
    pub content: Redacted,
    /// Whether the tool itself reported a failure.
    ///
    /// A tool that failed still produced a result; ADR-0016 D1 row 1 puts
    /// that in the expected register rather than the error one, and the loop
    /// carries it on rather than stopping.
    pub failed: bool,
}

/// How one tool call ended.
///
/// **Neither variant is an error**, and that is held by the type: see
/// [`crate::tool_call::error::ToolCallError`], which has no variant a refusal
/// could reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutcome {
    /// The call acted and returned.
    Completed {
        /// What the harness decided before it acted.
        decision: ToolDecision,
        /// What it produced.
        result: ToolResult,
        /// What a person is shown of it, when the surface composed a view.
        ///
        /// Never sent to the model: it is carried here and on
        /// [`Event::ToolShown`](crate::tool_call::Event), and no message is
        /// built from it. See [`crate::tool_call::view`].
        view: Option<crate::tool_call::ResultView>,
    },
    /// The call did not act.
    ///
    /// ADR-0011 D6 gives the harness no veto, so the only ways here are that
    /// the user said no or that there was nobody to ask. Under a delegated
    /// coordinator ruling of 2026-09-04 recorded on ADR-0011 and ADR-0016, a
    /// declined prompt **is not a failure at all** — so this becomes the next
    /// model turn's content and never a classified failure.
    Refused {
        /// What the harness decided.
        decision: ToolDecision,
        /// The request's id.
        id: String,
        /// Why it did not act, in the refusing surface's own words.
        because: String,
    },
}

impl ToolOutcome {
    /// The decision this outcome was reached under.
    #[must_use]
    pub const fn decision(&self) -> &ToolDecision {
        match self {
            Self::Completed { decision, .. } | Self::Refused { decision, .. } => decision,
        }
    }

    /// What the model is told, whichever way this ended.
    ///
    /// A refusal becomes an ordinary tool result carrying the refusal's own
    /// sentence. That is the whole of "a refusal becomes the next model
    /// turn's content": there is no other path out of this type.
    ///
    /// **This is the one point where a tool call's text becomes something a
    /// model will read**, which is why it takes ADR-0008 clause 6's port. The
    /// completed arm's content already passed a redactor in the executing
    /// surface, where the raw capture and the transcript both live; passing
    /// it again costs nothing, because redaction is idempotent. The refused
    /// arm's sentence has not, because it is composed here and can quote the
    /// target a model asked for.
    #[must_use]
    pub fn for_the_model<R: Redactor + ?Sized>(&self, redactor: &R) -> ToolResult {
        match self {
            Self::Completed { result, .. } => result.clone(),
            Self::Refused { id, because, .. } => ToolResult {
                id: id.clone(),
                content: Redacted::by(redactor, because),
                failed: false,
            },
        }
    }
}

/// Tokens a provider reported for one exchange. ADR-0012 D7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    /// Tokens in what was sent.
    pub prompt: u64,
    /// Tokens in what came back.
    pub completion: u64,
}

impl TokenUsage {
    /// Both halves together.
    #[must_use]
    pub const fn total(self) -> u64 {
        self.prompt + self.completion
    }
}

/// What the loop hands the model.
///
/// # The whole conversation, in the roles a provider defines
///
/// A provider's API is stateless, so every request carries everything: the
/// [`Prompt`] (the system text, every earlier turn and this turn's task) and
/// then [`Self::turn`], which is what has happened inside this turn so far —
/// the model's own messages with the calls it asked for, and the result of
/// every call. A provider maps each part to its own shape and keeps nothing
/// between two requests.
///
/// # The prompt is assembled once per turn, and the turn grows after it
///
/// [ADR-0013] D7 confines context assembly and compaction to turn boundaries,
/// and everything in [`Self::turn`] is what happened *inside* the turn. So the
/// loop calls
/// [`ContextPolicy::assemble`](crate::iteration::ContextPolicy::assemble)
/// exactly once, at the start of the turn, and appends each round's messages
/// to `turn` rather than reassembling.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[derive(Debug)]
pub struct ModelRequest<'a> {
    /// What the context policy assembled. ADR-0013 owns what is in it.
    pub prompt: &'a Prompt,
    /// The tools the model may ask for.
    pub tools: &'a [ToolDescriptor],
    /// This turn so far, oldest first: each of the model's messages, each
    /// followed by the results of the calls it asked for.
    ///
    /// Empty on the first exchange. A refused call has a result too, carrying
    /// the refusal's own sentence, which is the whole of "a refusal becomes
    /// the next model turn's content".
    pub turn: &'a [Message],
}

/// What the model answered.
///
/// Three arms, which is ADR-0008 D1's cycle stated as a type: the model
/// answers, or it asks for tools and the loop goes round again, or it stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelResponse {
    /// Text for the user. The turn ends.
    Text {
        /// What the model said.
        text: String,
        /// The provider's own record of this message. See
        /// [`Message::Assistant`]'s `echo`.
        echo: Option<String>,
        /// What it cost.
        tokens: TokenUsage,
    },
    /// One or more tool calls. The loop executes them and asks again.
    Calls {
        /// What it asked for, in the order it asked.
        calls: Vec<ToolRequest>,
        /// Any text the model wrote beside the calls. Usually empty.
        text: String,
        /// The provider's own record of this message. See
        /// [`Message::Assistant`]'s `echo`.
        echo: Option<String>,
        /// What it cost.
        tokens: TokenUsage,
    },
    /// The model stopped without text and without asking for a tool.
    ///
    /// A real outcome rather than an error: a provider that hit its own
    /// length limit, or a refusal, is the mechanism working and reporting it.
    Stopped {
        /// Why, in the provider's own words.
        reason: String,
        /// What it cost.
        tokens: TokenUsage,
    },
}

impl ModelResponse {
    /// What this exchange cost, whichever arm it took.
    #[must_use]
    pub const fn tokens(&self) -> TokenUsage {
        match self {
            Self::Text { tokens, .. }
            | Self::Calls { tokens, .. }
            | Self::Stopped { tokens, .. } => *tokens,
        }
    }
}

/// What a provider says it can do. ADR-0012 D3's capability descriptor.
///
/// One field today, and it is the one D3 calls load-bearing: "a provider that
/// cannot do tool calling must say so, because discovering it mid-loop
/// produces a failure the user reads as the harness being broken". Streaming
/// is D3's too and is deliberately absent — see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether this provider can be asked for a tool call at all.
    pub tool_calling: bool,
}

/// A provider that cannot do what the loop needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCannotCallTools {
    /// The model, as the caller named it.
    pub model: String,
}

impl fmt::Display for ModelCannotCallTools {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the model {:?} reports that it cannot call tools, and the tool-call loop has nothing \
             to run without that. A provider that cannot do tool calling must say so, because \
             discovering it mid-loop produces a failure the user reads as the harness being \
             broken. Configure a model that can, or run a surface that asks for no tools",
            self.model
        )
    }
}

impl std::error::Error for ModelCannotCallTools {}

/// Proof that the model was asked whether it can call tools, and said yes.
///
/// # This is what makes ADR-0012 clause 3 structural rather than checked
///
/// Clause 3: "A provider declaring no tool-call capability fails at
/// **configuration time** with a clear message, **not mid-loop**."
///
/// [`crate::tool_call::machine::run`] takes one of these, and the only way to
/// get one is [`ToolCalling::required`], which asks the model and refuses.
/// So a model that cannot call tools cannot reach the loop at all: there is
/// no path that emits an event first and discovers it later, because the
/// value that lets you start does not exist until the question has been
/// answered. A check asserting "it fails early" could only assert it about
/// the call sites somebody remembered to write.
///
/// It is the shape [`Ceiling`](crate::iteration::Ceiling),
/// `OutputBudget` and `Ttl` already use in this workspace: a value refused at
/// a boundary, carried thereafter as evidence that the boundary was crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolCalling(());

impl ToolCalling {
    /// Ask the model whether it can call tools, and refuse it if it cannot.
    ///
    /// `model` is the caller's own name for it and is quoted back in the
    /// refusal; this crate resolves no alias and names no provider, because
    /// ADR-0012 D1 and D2 own aliases and neither is built.
    ///
    /// # Errors
    ///
    /// [`ModelCannotCallTools`] when the descriptor says it cannot.
    pub fn required<M: Model + ?Sized>(
        model: &M,
        name: &str,
    ) -> Result<Self, ModelCannotCallTools> {
        if model.capabilities().tool_calling {
            Ok(Self(()))
        } else {
            Err(ModelCannotCallTools {
                model: name.to_owned(),
            })
        }
    }
}

/// The provider. ADR-0012 owns what is behind this.
///
/// Distinct from [`Generator`](crate::iteration::Generator), which is the
/// **inner** loop's port, and deliberately so: a `Generator` returns a
/// candidate and has no way to express a tool call, and widening it would
/// give the iteration loop a capability ADR-0008 D1 says belongs to the other
/// loop. Two ports for two loops, recorded on ADR-0012's Status tracking
/// rather than merged.
pub trait Model {
    /// What this provider can do, asked before the loop starts.
    fn capabilities(&self) -> Capabilities;

    /// Ask the model once.
    fn respond(
        &self,
        request: &ModelRequest<'_>,
    ) -> impl Future<Output = Result<ModelResponse, PortFailure>> + Send;
}

/// Executes a tool call. ADR-0011 owns what is behind this.
///
/// The implementation lives in `zaru-cli`, over the permission decision that
/// record already built, and it is what writes ADR-0010 D2's transcript
/// records around the act. Nothing in this crate implements it.
pub trait ToolExecutor {
    /// Every tool the model may be told about.
    ///
    /// Asked by the loop once, before the first exchange, so that the set the
    /// model is offered and the set the executor will accept are the same
    /// set — rather than two lists that can disagree.
    fn descriptors(&self) -> &[ToolDescriptor];

    /// Execute one call and report how it ended.
    ///
    /// `&mut self` because executing writes: ADR-0010 D2's transcript is
    /// appended to around every call, and ADR-0011 D5's overflow sink is
    /// written through.
    fn execute(
        &mut self,
        request: &ToolRequest,
    ) -> impl Future<Output = Result<ToolOutcome, PortFailure>> + Send;
}

/// The iteration loop, as the outer loop reaches it.
///
/// # Why the inner loop arrives as a port rather than as a call
///
/// [ADR-0009] D4 decides the branch in its own words: "A project with no
/// `zaru.toml` runs the tool-call loop only." So a caller with declared
/// validators supplies an implementation of this and one without supplies
/// `None`, and the branch sits at a **turn boundary** — which is also where
/// [ADR-0013] D7 puts every boundary, since an iteration happens inside a
/// turn.
///
/// It is a trait rather than a direct call to
/// [`iteration::run`](crate::iteration::run) because that function takes six
/// ports and two limits, and threading them through this loop's signature
/// would decide which context policy an iteration sees and which clock it
/// reads — questions no record answers. The seam settles none of them, and it
/// is the same dependency inversion this crate already uses inside itself for
/// validators and the context policy.
///
/// **Six, not five.** This sentence said five until 2026-09-05, and it was
/// written before ADR-0008 trigger clause 6 was decided and
/// [`Ports::redactor`](crate::iteration::Ports) joined the bundle. A count
/// stated in prose beside a struct that carries the real one is a second
/// declaration of the same fact, so it is the count that moved rather than
/// the argument: threading six is worse than threading five, which makes the
/// case for the seam stronger rather than weaker.
///
/// **Where inside a turn the inner loop is entered is not decided by any
/// record.** ADR-0008 D1 says only that the loops are "nested". This crate
/// enters it at the turn boundary and says so; a proposed Update on that
/// record carries the reading and settles nothing.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub trait InnerLoop {
    /// Run the iteration loop over this turn's task.
    fn iterate(
        &self,
        task: &str,
    ) -> impl Future<Output = Result<crate::iteration::Outcome, PortFailure>> + Send;
}

/// The five ports one turn of the tool-call loop needs.
///
/// Bundled for the reason [`Ports`](crate::iteration::Ports) is: a function
/// taking each separately is a function whose argument order is a thing to
/// get wrong.
#[derive(Debug)]
pub struct Ports<'a, M, X, P, K, R: ?Sized> {
    /// The provider.
    pub model: &'a M,
    /// The tool surface.
    pub tools: &'a mut X,
    /// Assembles what the model sees. ADR-0013's.
    pub context: &'a P,
    /// Supplies elapsed time.
    pub clock: &'a K,
    /// Removes the harness's own secrets from what reaches the model.
    ///
    /// ADR-0008 trigger clause 6's port, decided 2026-09-05. This loop hands
    /// it to [`ToolOutcome::for_the_model`], which is the one point a tool
    /// call's text becomes something a model will read.
    pub redactor: &'a R,
}
