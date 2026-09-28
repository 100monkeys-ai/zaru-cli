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
//! 1. [`rebuilt_from_the_transcript`] rebuilds layer 6 from the session's
//!    transcript, which now holds the turn that just ended, and
//!    [`checkpointed`] rewrites [ADR-0010] D3's `context.json` over it.
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
//! caller.
//!
//! # The transcript is the source of layer 6
//!
//! Every message a turn adds to the conversation — what the person asked,
//! each of the model's messages with the calls it made, and the result of
//! every call — is an event the loop emits and the transcript records, as it
//! happens. Layer 6 is **rebuilt from those records** at every turn boundary,
//! and a resumed session is rebuilt from them by the same function
//! ([`conversation_of`]). So the live session and the resumed one are sent
//! the same conversation by construction, rather than by two paths that
//! agree.
//!
//! Until 2026-09-28 layer 6 was a rendering of what the pane showed — the
//! task, one line per tool call, and the answer — kept in `context.json` and
//! read back from there on resume. A line such as "`cmd.run reported a failure
//! · 1256 bytes`" told the next turn's model that a command failed and nothing
//! of what it printed, so the model could not see what it had read or run one
//! turn earlier.
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
//! [`SessionContext::checkpoint`] produces it, and since 2026-09-28 nothing
//! reads it back — a resumed session is rebuilt from its transcript by
//! [`SessionContext::rebuilt`].
//!
//! **Layer 6 and nothing else**, written after every turn so a person can read
//! with `cat` exactly what the model will be sent next. Nothing reads it back:
//! the transcript is the source. Layers 1 to 4 are the stable prefix, which
//! D1 forbids rewriting mid-session and which a resumed session builds fresh
//! from its own configuration — storing it would make a stale grounding
//! outlive the session that read it, which is the opposite of what D1's
//! caching argument wants. Layer 5 has no producer. Layer 7 belongs to an
//! iteration, and D7 puts every boundary outside one.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

use crate::compose::{ByteCounter, TurnContext, prose};
use crate::session::{
    Checkpoint, CheckpointError, Record, Session, Transcript, TranscriptError, Utterance, Voice,
};
use core::fmt;
use zaru_core::context::{
    Compaction, Context, ContextLimits, Exchange, StablePrefix, Summariser, Usage,
};
use zaru_core::conversation::{Message, closed};
use zaru_core::iteration::PortFailure;
use zaru_core::redaction::{Redacted, Redactor};

/// The key `context.json` holds layer 6 under.
///
/// Named rather than written inline at both ends, because a writer and a
/// reader that spell one key twice are two spellings that can drift.
const EXCHANGES: &str = "exchanges";

/// How a session's context is sized: its limits and what a request spends
/// outside it.
///
/// # One value because they are one decision
///
/// Both come from [ADR-0012] D3's capability descriptor for the kind that
/// answered — the window it declares, and the bytes its own wire mapping
/// makes of the tool surface every exchange carries. They are resolved
/// together in `crate::compose::turn::prepare` and used together by
/// [`SessionContext::opened`] and [`SessionContext::rebuilt`], and passing
/// them separately through two callers was what took
/// `crate::compose::turn::start` over clippy's argument bound — which is the
/// lint doing its job: two parameters that always travel together are one
/// value with no name yet.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextShape {
    limits: ContextLimits,
    reserved: u64,
}

impl ContextShape {
    /// The limits a context is held under and what a request spends beside it.
    #[must_use]
    pub const fn of(limits: ContextLimits, reserved: u64) -> Self {
        Self { limits, reserved }
    }

    /// [ADR-0013]'s window and pressure threshold.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub const fn limits(self) -> ContextLimits {
        self.limits
    }

    /// What every request spends that the context does not contain, in bytes.
    ///
    /// See [`zaru_core::context::Context::reserved`].
    #[must_use]
    pub const fn reserved(self) -> u64 {
        self.reserved
    }
}

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
    pub const fn opened(prefix: StablePrefix, shape: ContextShape) -> Self {
        Self {
            context: Context::opened(prefix, shape.limits(), shape.reserved()),
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

    /// Replace layer 6 with what `records` hold. A turn-boundary act.
    ///
    /// See [`conversation_of`] for how records become exchanges.
    pub fn rebuild_from(&mut self, records: &[Record]) -> Rebuilt {
        let rebuilt = conversation_of(records);
        self.context.replace_exchanges(rebuilt.exchanges.clone());
        rebuilt
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

    /// A resumed session's context: its own prefix, and layer 6 rebuilt from
    /// its transcript's records.
    ///
    /// The prefix and the limits are the caller's, built fresh from this
    /// invocation's configuration, which is what D1's "never rewritten
    /// mid-session" means read across a resume: the new session gets its own
    /// prefix rather than the old one's. Layer 6 is [`conversation_of`] the
    /// records — the same function every turn boundary of a live session
    /// calls, so a resumed session is sent what the live one was.
    #[must_use]
    pub fn rebuilt(
        prefix: StablePrefix,
        shape: ContextShape,
        records: &[Record],
    ) -> (Self, Rebuilt) {
        let mut context = Self::opened(prefix, shape);
        let rebuilt = context.rebuild_from(records);
        (context, rebuilt)
    }
}

/// Layer 6 as a transcript's records hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebuilt {
    exchanges: Vec<Exchange>,
    unrecorded: usize,
}

impl Rebuilt {
    /// The exchanges, oldest first.
    #[must_use]
    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    /// How many tool calls the transcript says were made in turns recorded
    /// before calls and results were kept, and so cannot be given back.
    ///
    /// Zero for a transcript written since 2026-09-28.
    #[must_use]
    pub const fn unrecorded_calls(&self) -> usize {
        self.unrecorded
    }
}

/// Rebuild layer 6 from a transcript's records.
///
/// # One turn is one exchange, and a turn is found by what the person asked
///
/// A turn's records begin with the person's message
/// ([`Record::Conversation`] in the person's voice), and the loop's own
/// [`Message`] events follow: the task as the model was given it, each of the
/// model's messages with the calls it asked for, and each result. Those
/// messages, in order, are the exchange.
///
/// **A call with no result is closed** with [`prose::CALL_DID_NOT_COMPLETE`],
/// marked as a failure: its turn was interrupted, or the process died while
/// it ran. See [`zaru_core::conversation::closed`].
///
/// **A compaction is applied where it happened.** Its record carries the
/// summary the model was given in place of the oldest exchanges, so the
/// exchanges it replaced are replaced here too, and a rebuilt session is sent
/// the same summary the live one was.
///
/// # A turn recorded before 2026-09-28 is rebuilt with what it holds
///
/// Such a turn has the person's message and the answer
/// ([`Record::Conversation`] in both voices) and none of the loop's messages.
/// It becomes the person's message and the model's answer, and its tool calls
/// — which the transcript names but never kept — are counted in
/// [`Rebuilt::unrecorded_calls`]. A compaction recorded before then carries
/// no summary and is not applied: the exchanges it replaced stay.
#[must_use]
pub fn conversation_of(records: &[Record]) -> Rebuilt {
    let mut rebuilt = Rebuilt {
        exchanges: Vec::new(),
        unrecorded: 0,
    };
    let mut turn: Option<Turn> = None;
    for record in records {
        match record {
            Record::Conversation(Utterance {
                voice: Voice::User,
                text,
                ..
            }) => {
                close(&mut rebuilt, turn.take());
                turn = Some(Turn::asked(text));
            }
            Record::Conversation(Utterance {
                voice: Voice::Zaru,
                text,
                ..
            }) => {
                if let Some(turn) = turn.as_mut() {
                    turn.answer = Some(text.clone());
                }
            }
            Record::TurnLoop(zaru_core::tool_call::Event::Message(message)) => {
                let turn = turn.get_or_insert_with(Turn::default);
                turn.recorded = true;
                turn.messages.push(message.clone());
            }
            Record::TurnLoop(zaru_core::tool_call::Event::ToolRequested { .. }) => {
                if let Some(turn) = turn.as_mut() {
                    turn.requested += 1;
                }
            }
            Record::Compacted(compaction) => {
                close(&mut rebuilt, turn.take());
                if let (Some(summary), Some(raw)) = (&compaction.summary, &compaction.raw) {
                    let replaced = raw.len().min(rebuilt.exchanges.len());
                    rebuilt.exchanges.drain(..replaced);
                    rebuilt
                        .exchanges
                        .insert(0, Exchange::summary(summary.clone()));
                }
            }
            _ => {}
        }
    }
    close(&mut rebuilt, turn);
    rebuilt
}

/// One turn's records, as [`conversation_of`] gathers them.
#[derive(Debug, Default)]
struct Turn {
    /// What the person asked, as their own record keeps it.
    asked: Option<String>,
    /// The answer, as its own record keeps it.
    answer: Option<String>,
    /// The loop's messages, in order.
    messages: Vec<Message>,
    /// Whether any message was recorded: a turn written since 2026-09-28.
    recorded: bool,
    /// How many tool calls the loop's stream says were asked for.
    requested: usize,
}

impl Turn {
    fn asked(text: &str) -> Self {
        Self {
            asked: Some(text.to_owned()),
            ..Self::default()
        }
    }
}

/// Close a gathered turn into an exchange, if it holds anything.
fn close(rebuilt: &mut Rebuilt, turn: Option<Turn>) {
    let Some(turn) = turn else {
        return;
    };
    let messages = if turn.recorded {
        turn.messages
    } else {
        // Recorded before the loop's messages were: what the person asked
        // and what the model answered is all there is.
        rebuilt.unrecorded += turn.requested;
        let mut messages = Vec::with_capacity(2);
        if let Some(text) = turn.asked {
            messages.push(Message::User { text });
        }
        if let Some(text) = turn.answer {
            messages.push(Message::Assistant {
                text,
                calls: Vec::new(),
                echo: None,
            });
        }
        messages
    };
    if messages.is_empty() {
        return;
    }
    rebuilt.exchanges.push(Exchange::of_turn(closed(
        messages,
        prose::CALL_DID_NOT_COMPLETE,
    )));
}

/// Rebuild layer 6 from the session's transcript as it is on disk now.
///
/// Called at the end of every turn, before [`checkpointed`], by both places
/// a turn ends. The transcript holds the turn that just ended, so the next
/// turn is sent it exactly as the model saw it.
///
/// # Errors
///
/// [`TranscriptError`] when the transcript cannot be read.
pub fn rebuilt_from_the_transcript(
    context: &mut SessionContext,
    session: &Session,
) -> Result<(), TranscriptError> {
    let reading = Transcript::read(&session.transcript_path())?;
    context.rebuild_from(&reading.records);
    Ok(())
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
/// # These two are built **here**, one row of clause 6's enumeration
///
/// [ADR-0008] clause 6's check is a walk over every product file that calls
/// `Redacted::by`. These records were built here because this file was
/// already a row of it, for the finished turn it composed into layer 6; since
/// 2026-09-28 layer 6 is rebuilt from the transcript and these two are what
/// keeps the file on the list, which the list's own description now says.
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
/// **Called after [`rebuilt_from_the_transcript`] and nowhere else**, so the
/// file holds what the next turn will be sent. Nothing reads it back: it is
/// there for a person to read with `cat`.
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
