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
//! *cannot* compact". [`TurnContext`](crate::compose::TurnContext) is that
//! implementation and it holds the context by shared borrow, which is what
//! makes D7 structural rather than remembered.
//!
//! But something has to own the context *mutably* between turns, or nothing
//! can ever compact. [`SessionContext`] is that owner, and the property is
//! inherited one layer out rather than re-established:
//! [`SessionContext::policy`] takes `&self` and hands out a `TurnContext`, so
//! **a caller holding a policy cannot reach
//! [`SessionContext::at_turn_boundary`]** — the borrow checker refuses it. The
//! mutation that would break D7 does not compile here either.
//!
//! # What a turn boundary is, and what happens at one
//!
//! A turn boundary is between turns: after one turn's exchange has been
//! recorded and before the next turn assembles. At one, and nowhere else:
//!
//! 1. [`SessionContext::record`] adds the turn that just finished to layer 6.
//! 2. [`SessionContext::at_turn_boundary`] relieves pressure if there is any
//!    — D2's summarise-and-replace on layer 6, then D4's attachments — and
//!    hands back what it did, for the transcript and for the user.
//! 3. [`SessionContext::policy`] is borrowed for the turn, and cannot compact.
//!
//! **Nothing here decides when a turn ends.** That is the caller's, and today
//! there are two: `crate::compose::turn` runs exactly one turn, so its
//! boundary call has nothing to compact and does nothing; the in-session
//! shell, which is what makes a session hold more than one turn, is a
//! separate arc's. This type is what both of them use, so the rule lives in
//! one place rather than in each caller.
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
use core::fmt;
use zaru_core::context::{
    Compaction, Context, ContextLimits, Exchange, StablePrefix, Summariser, Usage,
};
use zaru_core::iteration::PortFailure;
use zaru_core::redaction::Redactor;

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
    pub const fn opened(prefix: StablePrefix, limits: ContextLimits) -> Self {
        Self {
            context: Context::opened(prefix, limits),
        }
    }

    /// The port the loop calls, borrowing the context **shared**.
    ///
    /// This is where D7 becomes structural one layer out: the returned value
    /// borrows `self` immutably for as long as it lives, so
    /// [`Self::at_turn_boundary`] — which needs `&mut self` — cannot be
    /// called while a turn is in progress.
    #[must_use]
    pub fn policy<'a>(&'a self, redactor: &'a (dyn Redactor + Sync)) -> TurnContext<'a> {
        TurnContext::over(&self.context, redactor)
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
        checkpoint: &serde_json::Value,
    ) -> Result<Self, serde_json::Error> {
        let stored = checkpoint
            .get(EXCHANGES)
            .unwrap_or(&serde_json::Value::Null);
        let exchanges: Vec<Exchange> = serde_json::from_value(stored.clone())?;
        let mut context = Context::opened(prefix, limits);
        for exchange in exchanges {
            context.record_exchange(exchange);
        }
        Ok(Self { context })
    }
}
