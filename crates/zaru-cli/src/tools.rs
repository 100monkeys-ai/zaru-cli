// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The local tool surface: ADR-0011's seven built-ins and its permission
//! model.
//!
//! # What this module is, and what it is not
//!
//! ADR-0011 is a security boundary — it decides what a model-driven action
//! may reach — so [Testing]'s rule applies to every check here: "Every escape
//! found at a security boundary... joins a permanent hostile-input corpus as
//! its reproduction", and the corpus never shrinks.
//!
//! **Nothing here executes anything.** `cmd.run` needs a subprocess and
//! `web.fetch` needs a network, and the harness has neither. What is built is
//! the model, the classification, the permission decision and the refusals;
//! the acting half sits behind ports with no implementation in this crate's
//! product tree, exactly as `zaru-core` declares five ports it does not
//! implement and [`credentials`](crate::credentials) declares two.
//!
//! # Where it lives, and why here
//!
//! [Bounded Contexts] names no crate for the tool surface. It sits in
//! `zaru-cli` under a delegated coordinator ruling of 2026-09-04, recorded on
//! that page and on the record, because every input the permission decision
//! needs is a property of the whole program rather than of any one part: the
//! runtime tier ([ADR-0001] D2 resolves it once at session start), the
//! permission mode ([ADR-0014]'s five configuration layers), the project
//! allowlist (layer 3), the working directory, and the session directory
//! [ADR-0011] D5 writes overflow into. No other crate holds them.
//!
//! The tool-call loop, when it is built, reaches this through a port
//! `zaru-core` declares — the same dependency inversion the composer used for
//! its `Entries` trait. **Nothing is added to `zaru-core` by this module**, so
//! no ADR-0003 D8 edge moves and `zaru-core` gains no dependency.
//!
//! # The two loops are not the same, and this is the outer one
//!
//! [ADR-0008] D1 separates the **tool-call loop** (the model requests a tool,
//! the harness executes it, the result returns) from the **iteration loop**
//! (generate, execute, evaluate, refine). This module belongs to the first.
//! `zaru-core`'s `Executor` port belongs to the second, and its
//! `ExecutionOutcome` is deliberately not reused here: sharing it would make
//! `zaru-core` a shared-types crate, which [ADR-0016]'s Status tracking
//! records as deliberately avoided so `zaru-seal` stays standalone
//! publishable. The cost of that is one duplicated shape, recorded rather
//! than hidden.
//!
//! # The rendering is not here either
//!
//! [ADR-0011] D3's prompt is rendered by `zaru-tui`; the decision is not the
//! terminal's. What this module owns is *what the user is told*, which is
//! passed in as a sentence rather than composed where it is shown, so that
//! what the user was told and what the harness believes it said cannot drift
//! apart — the same shape [`credentials::Confirm`] uses for ADR-0007 D8's
//! apex confirmation, and the same shape [`SessionNotice`] uses for D2's
//! not-a-sandbox line.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`credentials::Confirm`]: crate::credentials::Confirm

pub mod decision;
pub mod mode;
pub mod name;
pub mod notice;
pub mod output;
pub mod port;
pub mod tree;

pub use decision::{
    Assessment, DESTRUCTIVE_MARKING, Decision, Invocation, InvocationRefused, Permission,
    RefusedBecause, Requirement, Subject, TranscriptEntry,
};
pub use mode::{Layer, Mode, ModeRefused, Tier};
pub use name::{Effect, ToolName};
pub use notice::SessionNotice;
pub use output::{
    BudgetIsZero, Captured, ELISION_PREFIX, Excerpt, OutputBudget, Overflow, OverflowFailure,
    PresentationRefused, Presented,
};
pub use port::{Allowlist, Confirm, DestructiveMatch, Question};
pub use tree::{Placement, Target, TreeError, WorkingDirectory};

// `pub(crate)` rather than private, for the reason `credentials::fixtures`
// and `config::fixtures` already are: `crate::session`'s checks need a
// working directory with a real symlinked route and a real out-of-tree
// sibling, so that the line ADR-0011 D4 renders into ADR-0010 D2's transcript
// is the line the real classification produced. A second scratch tree beside
// the session checks would be one fixture in two places, which is a fixture
// that diverges -- and the divergence would be in exactly the seventeen-path
// hostile corpus this record's security corpus is made of.
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
