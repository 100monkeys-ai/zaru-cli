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
//! # The six things it does add, and each is an adapter
//!
//! There were five until 2026-09-05, then four, and now six. The fifth was
//! `NoFetch`, a stand-in for
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
//! | [`SessionContext`] | the same `Context` owned mutably, so ADR-0013 D2's compaction has a caller and D7 stays structural |
//! | [`ModelSummariser`] | ADR-0013 D2's summary is a model call, and `zaru-core` may not make one |
//!
//! # What a turn does not have, stated here rather than discovered
//!
//! **The inner loop, since 2026-09-05.** [ADR-0009] D4's branch is
//! `Option<&I>` and this composition supplies `Some` where a project declares
//! validators. The two questions that held it — ADR-0012's "whether one
//! provider implementation satisfies both traits" and [ADR-0008]'s "what an
//! execution *is*" — were decided under directive 20 and are built in
//! [`iterate`]: one client through one exchange, and an execution that is a
//! candidate applied through the same tool surface a turn uses.
//!
//! **A summariser, a turn boundary, and one turn to run between them.**
//! [`ContextPolicy::assemble`] takes `&self` and `Context::compact` takes
//! `&mut self`, so a policy still cannot compact — [ADR-0013] D7 held by the
//! signatures. What arrived on 2026-09-05 is the other half: [`SessionContext`]
//! owns the context mutably between turns and is the only thing that can call
//! `compact`, and [`ModelSummariser`] is D2's generated summary over the
//! provider the turn is already using.
//!
//! **This composition runs one turn, so its boundary compacts nothing**, and
//! that is a session's shape rather than a missing implementation: layer 6 is
//! empty before a first turn, so `Context::compact` returns through its own
//! threshold check without spending a model call. The call is real, the
//! summariser is real, and what is absent is a second turn — which the
//! in-session shell supplies and which is a separate arc's.
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

pub mod boundary;
pub mod context;
pub mod count;
pub mod iterate;
pub mod model;
pub mod prose;
pub mod shared;
pub mod sink;
pub mod summarise;
pub mod tips;
pub mod turn;

pub use boundary::{ContextShape, SessionContext};
pub use context::{TurnContext, prefix_for};
pub use count::ByteCounter;
pub use iterate::{Applying, Candidate, Generating, Inner, Iterations, Kept, Narrated, Narrator};
pub use model::Classifying;
pub use shared::Shared;
pub use sink::{Records, ToolLines};
pub use summarise::ModelSummariser;
pub use tips::{Conditions, Tip, Tips};
pub use turn::{KINDS_WITH_A_CLIENT, Owed, Prepared, Ran};

#[cfg(test)]
pub(crate) mod tests;
