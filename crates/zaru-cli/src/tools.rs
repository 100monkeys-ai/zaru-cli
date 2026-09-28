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
//! **All seven execute.** [`files`] holds the five
//! filesystem acts, all on `std::fs` inside D4's boundary and all reading
//! their path out of the [`Target`] the decision was reached about; `fs.write`
//! and `fs.edit` replace a whole file through [`crate::atomic`] at the file's
//! own mode, and `fs.search` walks under the classified root without ever
//! following a link. `cmd.run` acts through [`crate::process`], which is the
//! one place this workspace starts a child process; a command is not measured
//! against D4 as a path, because its boundary is the working directory it is
//! started in. **`web.fetch` acts as of 2026-09-05**, through [`crate::web`],
//! which is the one place a model-chosen URL is retrieved and the one place
//! this workspace builds an HTTP client: `http` and `https` only, no redirect
//! across a host, this machine and the link-local range refused by name, and
//! a body over a caller-passed ceiling refused whole rather than cut short.
//! **No built-in sits behind a port with no implementation any more** —
//! [`allowlist`] and [`destructive`] stopped on 2026-09-05, and so did the
//! prompt. What still has none is [`seal`]'s membrane, which ADR-0004 is
//! blocked upstream on.
//!
//! Every call's arguments arrive as one JSON object and are read in
//! [`arguments`], which is the only door from a request's text into a call —
//! and it is reached **before** the permission decision, because a path that
//! has not been extracted from the arguments is not yet a target.
//!
//! # Where it lives, and why here
//!
//! [Bounded Contexts] names no crate for the tool surface. It sits in
//! `zaru-cli` under a delegated coordinator ruling of 2026-09-04, recorded on
//! that page and on the record, because every input the permission decision
//! needs is a property of the whole program rather than of any one part: the
//! runtime tier ([ADR-0001] D2 resolves it once at session start), the
//! permission mode ([ADR-0014]'s five configuration layers), the user's
//! allowlist (layer 2), the working directory, and the session directory
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

pub mod allowlist;
pub mod arguments;
mod codebase;
pub mod decision;
pub mod declared;
pub mod destructive;
pub mod execute;
pub mod files;
pub mod grants;
pub mod mode;
pub mod name;
pub mod notice;
pub mod output;
pub mod port;
pub mod preview;
pub mod prompt;
pub mod seal;
pub mod tree;

pub use allowlist::{Allowed, AllowlistRefused, Entry};
pub use arguments::{ArgumentsRefused, Call, schema};
pub use decision::{
    Assessment, DESTRUCTIVE_MARKING, Decision, Invocation, InvocationRefused, Permission,
    RefusedBecause, Requirement, Subject, TranscriptEntry,
};
pub use declared::{Refused as RegistrationRefused, surface};
pub use destructive::{Category, Shapes};
pub use execute::{
    Executor, NotACall, OVERFLOW_PREFIX, SessionOverflow, descriptor_set, descriptors,
};
pub use mode::{Layer, Mode, ModeRefused, Tier};
pub use name::{Called, Effect, REMOTE_MARKING, SubjectKind, ToolName};
pub use notice::SessionNotice;
pub use output::{
    BudgetIsZero, Captured, ELISION_PREFIX, Excerpt, OutputBudget, Overflow, OverflowFailure,
    PresentationRefused, Presented,
};
pub use port::{
    About, Allowlist, Asking, Confirm, ConfirmFailure, DestructiveMatch, Fetch, NoProjection,
    Projected, Question, Retrieved, Shown, Subprocess,
};
// `prompt` is deliberately **not** re-exported here. `zaru-core` already has
// an `iteration::Prompt` and several checks in this crate import it, so a
// second `tools::Prompt` at the same level would be two different things one
// `use` line away from each other. Callers say `tools::prompt::Prompt`, and
// `line` and `answer` are far too generic to sit beside `Mode` and `Target`.
pub use seal::{NoMembrane, Verdict, Verdicts};
pub use tree::{Placement, Target, TreeError, WorkingDirectory};

// `pub(crate)` rather than private, for the reason `credentials::fixtures`
// and `config::fixtures` already are: `crate::session`'s checks need a
// working directory with a real symlinked route and a real out-of-tree
// sibling, so that the line ADR-0011 D4 renders into ADR-0010 D2's transcript
// is the line the real classification produced. A second scratch tree beside
// the session checks would be one fixture in two places, which is a fixture
// that diverges -- and the divergence would be in exactly the seventeen-path
// hostile corpus this record's security corpus is made of.
//
// `crate::manifest`'s checks are the second consumer, and they want the same
// tree for the same reason: ADR-0009 D3's `json_schema` path is measured
// against ADR-0011 D4's boundary, so the hostile corpus the two records share
// has to be one corpus.
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
