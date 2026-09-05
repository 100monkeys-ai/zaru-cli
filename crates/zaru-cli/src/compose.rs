// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The composition: what turns eleven landed modules into one turn.
//!
//! # This module builds nothing new and decides nothing new
//!
//! Every capability a turn needs was already built by another arc, and each
//! one is reached through a port some record declares. What was missing was
//! the place where a product implementation of each is constructed from the
//! resolved configuration and handed to the loop — `main.rs` and this module,
//! which is where [ADR-0003] D8's six crates meet.
//!
//! So there is **no new port here, no new dependency, and no second
//! declaration of anything a module already declares.** The ceilings are
//! [`crate::cli::layers`]'s, the keys are the records' own `declare`
//! functions', the permission decision is [`crate::tools`]'s, the transcript
//! is [`crate::session`]'s, and the two loops are `zaru-core`'s. This module
//! calls constructors and passes values.
//!
//! # The four things it does add, and each is an adapter
//!
//! There were five until 2026-09-05. The fifth was `NoFetch`, a stand-in for
//! ADR-0011 D1's seventh built-in, and its own module said it would go on the
//! day a real one landed; [`crate::web`] is that, so it did. **It was the one
//! entry here that was not an adapter** — the other four exist because a
//! `zaru-core` port needs a `zaru-cli` value, while that one existed because a
//! capability did not.
//!
//! | Here | Why it is not somewhere else |
//! | --- | --- |
//! | [`ByteCounter`] | ADR-0003 D2's table names no tokeniser, and `zaru-core` may not invent one |
//! | [`Classifying`] | ADR-0016 D1's class of a provider failure is read from the typed failure, which only the surface sees |
//! | [`Records`] | ADR-0010 D2's transcript is [ADR-0008] D3's stream, and the loop's sink is a `zaru-core` trait |
//! | [`TurnContext`] | ADR-0013's `Context` is a value; `ContextPolicy` is the port the loop calls it through |
//! | [`Shared`] | `tool_call::run` takes the tool surface by `&mut` and [ADR-0009] D4's branch in one call, so one executor needs two handles |
//!
//! # What a turn does not have, stated here rather than discovered
//!
//! **No inner loop.** [ADR-0009] D4's branch is `Option<&I>` and this
//! composition passes `None` always, because `iteration::run` needs a
//! `Generator` and an `Executor` and neither can be written: ADR-0012's Status
//! tracking reserves "whether one provider implementation satisfies both
//! traits" to that record, and [ADR-0008]'s reserves "what an execution *is*,
//! when a candidate is an edit rather than a script" to whoever implements the
//! executor port. So a task in a project whose manifest **declares validators**
//! is refused naming the missing wiring, and is never run as a bare tool-call
//! turn — running one over a project that asked for validation is ADR-0009 D2's
//! silent green arriving a layer up.
//!
//! **No summariser, and therefore no compaction.** [`ContextPolicy::assemble`]
//! takes `&self` and `Context::compact` takes `&mut self`, so a policy cannot
//! compact and nothing here calls the other half. A turn assembles once, and a
//! context that will not fit refuses with [ADR-0013] D7's own answer rather
//! than being rewritten. The layer-6 path is unreached by absence rather than
//! by a stub.
//!
//! **No persona.** [ADR-0013] D1's layer 1 is "system prompt and persona" and
//! [ADR-0027] D1 serves it from a prompt server this build reaches at no tier.
//! That record's own Status tracking reserves what a `bare`-tier harness does
//! without the fetch to a decision in a record; it was decided on 2026-09-05
//! under directive 20 as the third of its three answers, and [`prose`] carries
//! the one line the prefix says instead. **No sentence of persona is invented
//! here.**
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
//! [`ContextPolicy::assemble`]: zaru_core::iteration::ContextPolicy::assemble

pub mod context;
pub mod count;
pub mod iterate;
pub mod model;
pub mod prose;
pub mod shared;
pub mod sink;
pub mod turn;

pub use context::{TurnContext, prefix_for};
pub use count::ByteCounter;
pub use iterate::{Applying, Candidate, Generating, Iterations};
pub use model::Classifying;
pub use shared::Shared;
pub use sink::Records;
pub use turn::{KINDS_WITH_A_CLIENT, Owed, Prepared, Ran};

/// The inner loop this composition never supplies.
///
/// [ADR-0009] D4's branch takes an `Option<&I>` and `I` still has to be a
/// type. This is the type: **uninhabited**, so there is no value of it to
/// pass and `None` is the only thing the branch can be given. A struct with a
/// panicking body would be a stub that could be constructed by mistake; an
/// empty enum cannot.
///
/// See the module documentation for why the branch is `None`: `iteration::run`
/// needs a `Generator` and an `Executor`, and both are questions ADR-0012 and
/// [ADR-0008] reserve to themselves.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug)]
pub enum NoInnerLoop {}

impl zaru_core::tool_call::InnerLoop for NoInnerLoop {
    /// Unreachable: there is no value of `Self` to have called it on.
    async fn iterate(
        &self,
        _task: &str,
    ) -> Result<zaru_core::iteration::Outcome, zaru_core::iteration::PortFailure> {
        match *self {}
    }
}

#[cfg(test)]
pub(crate) mod tests;
