// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0013]'s layered context, as the port the loop calls it through.
//!
//! # The value and the port are two different things, and that is D7
//!
//! `zaru-core`'s [`Context`] is a value with two operations and the whole of
//! D7 is the difference between their signatures: `assemble` takes `&self` and
//! `compact` takes `&mut self`, so "a `ContextPolicy` implementation holding a
//! `Context` behind the shared borrow `assemble` gives it therefore *cannot*
//! compact — the method is not callable from there."
//!
//! [`TurnContext`] is that implementation, and it holds the context by shared
//! borrow. **The mutation that would break D7 does not compile here either**,
//! which is the property being inherited rather than re-established.
//!
//! # Compaction is somewhere else, and that is the whole of D7
//!
//! D2's compaction is a turn-boundary act, and the type that can perform one
//! is [`SessionContext`](crate::compose::SessionContext) — which owns the
//! context mutably and hands out one of these for the turn. So the borrow this
//! type holds is what makes D7 structural at both layers: while a `TurnContext`
//! exists, nothing can compact, because `at_turn_boundary` needs `&mut` and
//! this has the shared half.
//!
//! A context that will not fit still **refuses**, with [ADR-0013] D7's own
//! answer — "an iteration that would exceed the window fails as exhausted with
//! a clear reason rather than continuing on a rewritten context" — carried out
//! as `ContextRefusal::WindowExceeded` with both numbers on it. That is the
//! answer *inside* a turn, where D7 forbids rewriting; relieving the pressure
//! is what the boundary before the next turn is for.
//!
//! # What is in each of D1's seven layers today
//!
//! | Layer | This composition |
//! | --- | --- |
//! | 1 system prompt and persona | [`prose::NO_PERSONA`], because ADR-0027's fetch does not exist — see below |
//! | 2 grounding, session-start | empty: [ADR-0006]'s client reaches no network, so nothing is read at session start |
//! | 3 relationship memory | empty: [ADR-0031] D3 delivers it *inside* the served prompt and forbids a second fetch path, so it is absent exactly when layer 1 is |
//! | 4 project manifest summary | empty: no record says what a manifest summary is, and inventing a shape would settle it |
//! | 5 user attachments | empty: [ADR-0005] D5's attachments are not built and the trie is `zaru-notes`' |
//! | 6 conversation and tool results | empty on the first turn; the turn's own results ride on `ModelRequest.results` rather than here, which is [ADR-0013] D7 as `tool_call::run` reads it, and a finished turn joins it through [`Exchange::of_turn`](zaru_core::context::Exchange::of_turn) at the boundary |
//! | 7 iteration history | empty: no iteration runs, because there is no inner loop |
//!
//! **Six of the seven are empty and the prefix says so about the one that
//! matters.** An empty layer contributes nothing to the rendered text rather
//! than a blank section, which is `StablePrefix`' own rule, so what a model
//! actually receives is the one absence line and the task. That is a small
//! prompt and it is an honest one; the layers exist, they are reached, and
//! what fills them is other records' work.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
//! [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
//! [`Context`]: zaru_core::context::Context
//! [`prose::NO_PERSONA`]: crate::compose::prose::NO_PERSONA

use crate::compose::count::ByteCounter;
use crate::compose::prose;
use zaru_core::context::{Context, PrefixParts, StablePrefix};
use zaru_core::iteration::{ContextPolicy, ContextRefusal, Prompt, Turn};
use zaru_core::redaction::Redactor;

/// [ADR-0013] D1's layers 1 to 4 for a session this harness can actually
/// assemble.
///
/// Layer 1 carries [`prose::NO_PERSONA`] and the other three are empty — see
/// the module documentation for what each is waiting on. The prefix is built
/// **once** and has no method that changes it, which is that record's trigger
/// clause 1 held by the type rather than by a rule anybody keeps.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn prefix_for() -> StablePrefix {
    StablePrefix::assembled_once(PrefixParts {
        system_prompt_and_persona: prose::NO_PERSONA.to_owned(),
        grounding: String::new(),
        relationship_memory: String::new(),
        project_manifest_summary: String::new(),
    })
}

/// [ADR-0013]'s context, as [ADR-0008]'s loop reaches it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub struct TurnContext<'a> {
    context: &'a Context,
    counter: ByteCounter,
    redactor: &'a (dyn Redactor + Sync),
}

impl core::fmt::Debug for TurnContext<'_> {
    /// Names what it holds and renders none of it.
    ///
    /// A context is the whole of what a model is about to be shown, so a
    /// derived `Debug` would put a session's conversation into a panic
    /// message. The redactor cannot be rendered at all — it holds the
    /// harness's own bearer values in memory, which is why
    /// [`HeldSecrets`](crate::redaction::HeldSecrets) writes its own `Debug`
    /// by hand — so this reports the usage instead, which is a number.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TurnContext")
            .field("usage", &self.usage())
            .finish_non_exhaustive()
    }
}

impl<'a> TurnContext<'a> {
    /// Assemble against this context, counting bytes, redacting held secrets.
    #[must_use]
    pub const fn over(context: &'a Context, redactor: &'a (dyn Redactor + Sync)) -> Self {
        Self {
            context,
            counter: ByteCounter,
            redactor,
        }
    }

    /// What the context costs right now. [ADR-0013] D6's continuous number.
    ///
    /// Measured through the same counter and the same redactor the assembly
    /// uses, because "a marker is not the same length as the value it replaced
    /// and the threshold is compared against that count".
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub fn usage(&self) -> zaru_core::context::Usage {
        self.context.usage(&self.counter, self.redactor)
    }
}

impl ContextPolicy for TurnContext<'_> {
    /// Assemble the prompt for the turn about to begin.
    ///
    /// The tail is whichever of [`Turn`]'s three variants the loop passed, and
    /// **this function adds no prose to any of them**. A resumed turn's tail is
    /// the interrupted call's own rendered line, which is [ADR-0010] D4's
    /// datum: that record calls the line "what the user saw", so wrapping it
    /// in a sentence of this module's own would be a second description of one
    /// call.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let tail = match turn {
            Turn::Initial { task } => (*task).to_owned(),
            Turn::Refinement { refinement } => refinement.as_str().to_owned(),
            Turn::Resumed { interrupted } => interrupted.call().to_owned(),
        };
        let assembled = self
            .context
            .assemble(&self.counter, self.redactor, &tail)
            .map_err(ContextRefusal::from)?;
        Ok(Prompt::new(assembled.into_redacted()))
    }
}
