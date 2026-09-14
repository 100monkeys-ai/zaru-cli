// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The turn boundary: the one place [ADR-0013] D2's compaction may happen.
//!
//! # Why this type exists at all, and why it is not the `ContextPolicy`
//!
//! D7 is the difference between two signatures.
//! [`Context::assemble`](zaru_core::context::Context::assemble) takes `&self`
//! and [`Context::compact`](zaru_core::context::Context::compact) takes `&mut
//! self`, so — in that module's own words — "a `ContextPolicy` implementation
//! holding a `Context` behind the shared borrow `assemble` gives it therefore
//! *cannot* compact". [`TurnContext`] is that
//! implementation and it holds the context by shared borrow, which is what
//! makes D7 structural rather than remembered.
//!
//! But something has to own the context *mutably* between turns, or nothing
//! can ever compact. [`SessionContext`] is that owner, and the property is
//! inherited one layer out rather than re-established:
//! [`SessionContext::policy`] takes `&self` and hands out a `TurnContext`, so
//! **a caller holding a policy cannot reach
//! [`SessionContext::at_turn_boundary`]**, which needs `&mut self`. Measured
//! rather than asserted: the code that would do it is
//! `error[E0502]: cannot borrow ... as mutable because it is also borrowed as
//! immutable`, and it is quoted in
//! `a_policy_in_hand_is_a_turn_in_progress_and_cannot_reach_the_boundary`.
//!
//! **What that does *not* say is that the signature is unchangeable.** Giving
//! `policy` a `&mut self` compiles today, because no call site holds a policy
//! across a boundary call — measured, 2026-09-05, and recorded here rather
//! than left as a claim the code does not support. The guarantee is about a
//! caller that tries, and it arrives the moment one does.
//!
//! # What a turn boundary is, and what happens at one
//!
//! A turn boundary is between turns: after one turn's exchange has been
//! recorded and before the next turn assembles. At one, and nowhere else:
//!
//! 1. [`exchange_of_turn`] renders what just happened and
//!    [`SessionContext::record`] adds it to layer 6, then [`checkpointed`]
//!    rewrites [ADR-0010] D3's `context.json` over it.
//! 2. [`SessionContext::at_turn_boundary`] relieves pressure if there is any
//!    — D2's summarise-and-replace on layer 6, then D4's attachments — and
//!    hands back what it did, for the transcript and for the user.
//! 3. [`SessionContext::policy`] is borrowed for the turn, and cannot compact.
//!
//! **Nothing here decides when a turn ends.** That is the caller's, and there
//! are two: [`crate::compose::turn::task`], which runs one turn in a session
//! it mints, and [`crate::terminal::driver::run_a_turn`], which runs each turn
//! a person types into a session already open. Both reach act 1 through the
//! two functions below, so the rule lives here rather than once in each
//! caller — which matters most for the part of it that is a *seam*: every byte
//! of an exchange passes [ADR-0008] clause 6's `Redactor`, and two callers
//! each doing that themselves are the two places it can be forgotten.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//!
//! # The checkpoint is this type's, because its contents are ADR-0013's
//!
//! [ADR-0010] D3: "`context.json` holds what the model needs to continue —
//! the compacted conversation, per ADR-0013. It is overwritten each turn."
//! `crate::session::checkpoint` says of itself that "what is in the
//! checkpoint is ADR-0013's layering and compaction", and writes an opaque
//! value it never interprets. This is the type that knows what goes in it:
//! [`SessionContext::checkpoint`] produces it and
//! [`SessionContext::restored`] reads it back.
//!
//! **Layer 6 and nothing else.** Layers 1 to 4 are the stable prefix, which
//! D1 forbids rewriting mid-session and which a resumed session builds fresh
//! from its own configuration — storing it would make a stale grounding
//! outlive the session that read it, which is the opposite of what D1's
//! caching argument wants. Layer 5 has no producer. Layer 7 belongs to an
//! iteration, and D7 puts every boundary outside one.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

use crate::compose::{ByteCounter, TurnContext};
use crate::session::{Checkpoint, CheckpointError, Record, Session, Utterance, Voice};
use core::fmt;
use zaru_core::context::{
    Compaction, Context, ContextLimits, Exchange, StablePrefix, Summariser, Usage,
};
use zaru_core::iteration::PortFailure;
use zaru_core::redaction::{Redacted, Redactor};

/// The key `context.json` holds layer 6 under.
///
/// Named rather than written inline at both ends, because a writer and a
/// reader that spell one key twice are two spellings that can drift.
const EXCHANGES: &str = "exchanges";

/// A session's context, owned across the turns it holds.
///
/// See the module documentation: this is the only thing that can compact, and
/// the policy it hands out is the only thing the loop can see.
pub struct SessionContext {
    context: Context,
}

impl fmt::Debug for SessionContext {
    /// Names what it holds and renders none of it.
    ///
    /// A context is the whole of what a model is about to be shown, so a
    /// derived `Debug` would put a session's conversation into a panic
    /// message. What is reported is how many exchanges it holds, which is a
    /// number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionContext")
            .field("exchanges", &self.context.exchanges().len())
            .finish_non_exhaustive()
    }
}

impl SessionContext {
    /// Open a session's context around a prefix that is now fixed.
    #[must_use]
    pub const fn opened(prefix: StablePrefix, limits: ContextLimits, reserved: u64) -> Self {
        Self {
            context: Context::opened(prefix, limits, reserved),
        }
    }

    /// The port the loop calls, borrowing the context **shared**.
    ///
    /// This is where D7 becomes structural one layer out: the returned value
    /// borrows `self` immutably for as long as it lives, so
    /// [`Self::at_turn_boundary`] — which needs `&mut self` — cannot be
    /// called while a turn is in progress.
    ///
    /// `iterating` is passed straight through to [`TurnContext::over`] and
    /// decides whether the assembled prompt tells the model an iteration is
    /// one exchange. It is the composition's boolean rather than one derived
    /// here, because this type cannot see a manifest.
    #[must_use]
    pub fn policy<'a>(
        &'a self,
        redactor: &'a (dyn Redactor + Sync),
        iterating: bool,
    ) -> TurnContext<'a> {
        TurnContext::over(&self.context, redactor, iterating)
    }

    /// What the context costs right now. [ADR-0013] D6's continuous number.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub fn usage(&self, redactor: &(dyn Redactor + Sync)) -> Usage {
        self.context.usage(&ByteCounter, redactor)
    }

    /// Layer 6, oldest first.
    #[must_use]
    pub fn exchanges(&self) -> &[Exchange] {
        self.context.exchanges()
    }

    /// Add the turn that just finished to layer 6. A turn-boundary act.
    pub fn record(&mut self, exchange: Exchange) {
        self.context.record_exchange(exchange);
    }

    /// Relieve window pressure. [ADR-0013] D2 and D4, at a turn boundary.
    ///
    /// Does nothing at all when usage is at or below the threshold, which is
    /// `Context::compact`'s own early return: D2 compacts "when the window
    /// pressure threshold is crossed", and a compaction nobody needed still
    /// costs a model call and still announces itself.
    ///
    /// # Errors
    ///
    /// [`PortFailure`] when the summariser fails. **The context is left
    /// exactly as it was** — `Context::compact` obtains the summary before it
    /// removes anything — so a failed summarisation loses no history, and the
    /// caller reports the failure rather than continuing on a context that
    /// was half rewritten.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    pub async fn at_turn_boundary<S: Summariser + Sync>(
        &mut self,
        summariser: &S,
        redactor: &(dyn Redactor + Sync),
    ) -> Result<Compaction, PortFailure> {
        self.context
            .compact(summariser, &ByteCounter, redactor)
            .await
    }

    /// [ADR-0010] D3's checkpoint: what the model needs to continue.
    ///
    /// Layer 6 and nothing else — see the module documentation for why the
    /// prefix is deliberately not in it.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn checkpoint(&self) -> serde_json::Value {
        serde_json::json!({ EXCHANGES: self.context.exchanges() })
    }

    /// Restore a session's layer 6 from [ADR-0010] D3's checkpoint.
    ///
    /// The prefix and the limits are the caller's, built fresh from this
    /// invocation's configuration, which is what D1's "never rewritten
    /// mid-session" means read across a resume: the new session gets its own
    /// prefix rather than the old one's.
    ///
    /// # Errors
    ///
    /// [`serde_json::Error`] when the stored document is not what this type
    /// writes. `crate::session::checkpoint` treats the file as opaque and
    /// this is the one place it is interpreted, so a checkpoint written by
    /// something else is refused here rather than silently read as an empty
    /// conversation — which would drop a session's whole history and look
    /// exactly like a session that had none.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub fn restored(
        prefix: StablePrefix,
        limits: ContextLimits,
        reserved: u64,
        checkpoint: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        let stored = checkpoint
            .get(EXCHANGES)
            .unwrap_or(&serde_json::Value::Null);
        let exchanges: Vec<Exchange> = serde_json::from_value(stored.clone())?;
        let mut context = Context::opened(prefix, limits, reserved);
        for exchange in exchanges {
            context.record_exchange(exchange);
        }
        Ok(Self { context })
    }
}

/// What one finished turn becomes in [ADR-0013] D1's layer 6.
///
/// # Three parts, and every one of them is redacted here
///
/// D1's layer 6 is "conversation **and tool results**", and
/// [`Exchange::of_turn`] is `zaru-core`'s declared shape for all three: the
/// task the user typed, the rendered line of every tool call the turn made on
/// the way, and the answer it ended with.
///
/// **This is [ADR-0008] clause 6's seventh path**, and it is here rather than
/// in either caller for the reason that clause exists: a session's next turn
/// assembles over what this returns, so every byte of it is about to become
/// prompt text. Two callers each building a redacted exchange would be two
/// places the port can be forgotten, and the clause's enumerating check names
/// this file for that reason. It was `crate::terminal::driver`'s until
/// 2026-09-05, when `crate::compose::turn::task` became the second caller;
/// the row moved rather than a ninth arriving, which is recorded on ADR-0008
/// before it was changed.
///
/// `answer` is what the turn printed, which is [`crate::compose::Ran`]'s
/// lines joined — the same text the user was shown, so what the next turn
/// remembers and what the person remembers are one thing.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn exchange_of_turn(
    redactor: &(dyn Redactor + Sync),
    task: &str,
    tool_lines: &[String],
    answer: &str,
) -> Exchange {
    let redacted = |text: &str| Redacted::by(redactor, text).as_str().to_owned();
    let results: Vec<String> = tool_lines.iter().map(|line| redacted(line)).collect();
    Exchange::of_turn(
        &redacted(&spoken_as(Voice::User, task)),
        &results,
        &redacted(&spoken_as(Voice::Zaru, answer)),
    )
}

/// How a turn's two halves are named, wherever one is rendered.
///
/// The two spellings were `format!` calls here and nowhere else until
/// 2026-09-06, when the pane began showing the same two halves back. Naming
/// them once is what stops the screen and [ADR-0013] D1's layer 6 coming to
/// disagree about how a turn is written down.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
fn spoken_as(voice: Voice, text: &str) -> String {
    format!("{}: {text}", voice.spoken_as())
}

/// The task as the user typed it, as [ADR-0010] D2's seventh producer keeps it.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub fn spoken_by_the_user(redactor: &(dyn Redactor + Sync), n: u32, task: &str) -> Record {
    utterance(redactor, n, Voice::User, task)
}

/// The answer as the turn rendered it, as that same producer keeps it.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub fn spoken_by_zaru(redactor: &(dyn Redactor + Sync), n: u32, answer: &str) -> Record {
    utterance(redactor, n, Voice::Zaru, answer)
}

/// One half of a turn's conversation, redacted, as a record.
///
/// # These two are built **here** so that clause 6's enumeration stays at eight
///
/// [ADR-0008] clause 6's check is a walk over every product file that calls
/// `Redacted::by`, and its own failure message tells its reader that "a path
/// added here is a path from captured bytes into a model prompt". A transcript
/// record is not one, so building these in [`crate::compose::turn`] would have
/// grown that list to nine with a row that is not what the list means — the
/// drift the enumeration exists to prevent. This file is **already** row seven,
/// because [`exchange_of_turn`] above redacts these same two strings on their
/// way into layer 6, so the records are built from the same call site and no
/// file joins the set. Recorded on ADR-0008 before this was written.
///
/// # Why a transcript record is redacted at all, when no other one is
///
/// [ADR-0010]'s Negative section says the transcript "contains whatever the
/// session contained, **including secrets that appeared in command output**",
/// and that stays true of every other record on the file. This port is a
/// different thing: it is over values **the harness itself holds**, ADR-0008
/// saying "a secret the harness never held is not redacted, because nothing
/// pattern-based was adopted". So the rule is that the person's words and the
/// harness's answer are raw except for a credential this harness put in its
/// own sealed store.
///
/// **It was forced rather than preferred.**
/// `corpus_a_stored_key_spoken_in_a_task_does_not_reach_the_checkpoint` puts a
/// stored provider key **in the task** and then walks every file under the
/// scratch home asserting the value absent by value and by ASCII core.
/// `transcript.jsonl` is one of those files, so a raw record would have made
/// this the first path in the harness that writes a value it holds into a
/// file — reddening a standing security check rather than raising a question.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn utterance(redactor: &(dyn Redactor + Sync), n: u32, voice: Voice, text: &str) -> Record {
    Record::Conversation(Utterance {
        n,
        voice,
        text: Redacted::by(redactor, text).as_str().to_owned(),
    })
}

/// Rewrite [ADR-0010] D3's `context.json` over what layer 6 now holds.
///
/// # D3 says "each turn", and until 2026-09-05 it was written once
///
/// D3: "`context.json` holds what the model needs to continue — the compacted
/// conversation, per ADR-0013. **It is overwritten each turn.**" What existed
/// was one write, in [`crate::compose::turn::task`], **before** turn 1 and
/// never again — so a session's checkpoint was the empty document a session
/// with no turns has, however many turns it went on to have, and a resume
/// restored nothing. The in-session shell wrote none at all.
///
/// **Called after [`SessionContext::record`] and nowhere else.** Before it,
/// the file holds the previous turn's layer 6 and a resume would come back one
/// turn short — which is a defect nothing else on this path would show,
/// because both documents parse and both look like a session.
///
/// The session-start write is **kept**: ADR-0010 clause 1 is about a session
/// directory holding its three files, and a session whose first turn refused
/// before this point would otherwise have two.
///
/// # Errors
///
/// [`CheckpointError`] for the render, the write, the sync or the rename. The
/// caller classifies it; this function chooses no class, because a checkpoint
/// that cannot be written is the same failure wherever the turn ran.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn checkpointed(context: &SessionContext, session: &Session) -> Result<(), CheckpointError> {
    Checkpoint::at(session.checkpoint_path()).write(&context.checkpoint())
}
