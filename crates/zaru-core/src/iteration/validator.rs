// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009]'s declared validators: what one is, what order they run in, and
//! what deciding whether one passed actually needs.
//!
//! # What this module owns, and what it does not
//!
//! It owns the **dispatch** — [`Plan`]'s dependency order, and [`Dispatch`]'s
//! walk over it emitting one report per validator considered. That is the
//! responsibility [Bounded Contexts] gives this crate, and it is the seam
//! [`Validators`](crate::iteration::port::Validators) was declared for.
//!
//! It owns **no manifest and no file**. [ADR-0009] D1's `zaru.toml` is also
//! [ADR-0014] D1's layer 3 — one file, two records — and configuration is
//! `zaru-cli`'s. A caller builds [`Declared`] values and hands them here; this
//! crate parses nothing, opens nothing, and names no configuration key.
//!
//! It owns **nothing that runs**. Each validator's `run` command leaves
//! through [`ValidatorRunner`], and the two `expect` kinds that need a crate
//! [ADR-0003] D2's table does not name leave through [`PatternMatch`] and
//! [`SchemaValidate`]. **Nothing in this crate's product tree implements any
//! of the three**, exactly as nothing implements the loop's five ports.
//!
//! # Three vocabularies that are not this one
//!
//! **A validator is not a test.** [Ubiquitous Language] gives "validator" for
//! "a deterministic check that decides whether an iteration succeeded" and
//! names *test*, *assertion* and *judge* as the anti-terms. Nothing here is
//! called any of those, and nothing is called *verdict*, which that page
//! reserves for a SEAL policy evaluation.
//!
//! **A validator's command is not the loop's execution.** [ADR-0008] D1
//! separates the tool-call loop from the iteration loop on purpose, and
//! [`Executor`](crate::iteration::port::Executor) makes *the candidate's*
//! effect real. A validator's `run` is a third thing and it has its own port.
//! See [`port`] for the whole of that reasoning.
//!
//! **A validator's command is not a tool call.** [ADR-0011]'s permission model
//! decides what a *model-driven* action may reach; a validator command is
//! declared by the project in a file the user can read, so routing it through
//! a permission decision would invent a question no record asks.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod declaration;
pub mod dispatch;
pub mod expectation;
pub mod name;
pub mod plan;
pub mod port;

pub use declaration::Declared;
pub use dispatch::Dispatch;
pub use expectation::Expect;
pub use name::{Name, NameRefused, Pattern, Run, SchemaPath, TextRefused};
pub use plan::{Plan, PlanRefused};
pub use port::{PatternMatch, SchemaValidate, ValidatorOutput, ValidatorRunner};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
