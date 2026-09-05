// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The tool-call loop: [ADR-0008]'s outer cycle.
//!
//! Two loops exist in this harness and they are not the same. The **tool-call
//! loop** is what every agentic harness runs — the model requests a tool, the
//! harness executes it, the result returns, the model continues. The
//! **iteration loop** is the 100monkeys cycle, and it is
//! [`crate::iteration`]. This module is the first one.
//!
//! D1 makes the outer loop run at every tier, and [ADR-0001] D1's table gives
//! `bare` no iteration loop at all — [ADR-0009] D4 says the same thing from
//! the project's side: "A project with no `zaru.toml` runs the tool-call loop
//! only." So this module runs alone at the on-ramp, which is not a degraded
//! mode: it is a complete agentic harness.
//!
//! # What this module owns, and what it does not
//!
//! It owns the cycle, its event stream, the accumulation of a turn's tool
//! results, the ceiling, and the branch that decides whether a turn's body is
//! an iteration loop. It owns **nothing about tools**: a name, an opaque
//! argument string, an opaque result and a sentence somebody else composed
//! are the whole of what crosses [`ToolExecutor`]. [ADR-0011]'s seven
//! built-ins, its three permission modes, its working-directory boundary and
//! its allowlist are `zaru-cli`'s, and this crate cannot see them.
//!
//! It owns no numbers. [`ToolCallCeiling`] arrives from the caller and is
//! refused at zero, because no record carries a bound for this loop — see
//! [`limits`].
//!
//! # Headless, and nothing here implements a port
//!
//! ADR-0008 D2. Nothing in this crate's product tree implements [`Model`],
//! [`ToolExecutor`] or [`InnerLoop`], nothing here opens a socket, spawns a
//! process, touches a file or calls a provider, and the only reader of the
//! machine's clock in the crate is still
//! [`SystemClock`](crate::iteration::SystemClock).
//!
//! # Three things this module deliberately does not decide
//!
//! **Where inside a turn the iteration loop is entered.** D1 says "nested"
//! and nothing more. The branch is taken at the turn boundary and a proposed
//! Update on ADR-0008 D1 says so.
//!
//! **What a tool's arguments look like.** ADR-0011 D1 names seven tools and
//! no argument schema for any of them; [`ToolDescriptor::parameters`] is an
//! opaque string and the question is raised on that record.
//!
//! # Redaction, which this module no longer leaves open
//!
//! [ADR-0008]'s trigger clause 6 was decided on 2026-09-05 and a tool's
//! output becoming the next turn's content is one of the paths it names.
//! [`ToolResult::content`] is a [`Redacted`](crate::redaction::Redacted), so
//! this loop cannot hand a model text that did not pass the port, and
//! [`ToolOutcome::for_the_model`] is the one place a refusal's own sentence
//! does. **The event stream is deliberately not redacted**: it carries byte
//! counts rather than content, [ADR-0010] D2's transcript is written from it,
//! and that record keeps whatever the session kept.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

pub mod error;
pub mod event;
pub mod limits;
pub mod machine;
pub mod port;

pub use error::{PortKind, ToolCallError};
pub use event::{Event, EventSink, TurnEnding};
pub use limits::{CeilingIsZero, ToolCallCeiling};
pub use machine::{Outcome, Start, run};
pub use port::{
    Capabilities, InnerLoop, Model, ModelCannotCallTools, ModelRequest, ModelResponse, Ports,
    TokenUsage, ToolCalling, ToolDecision, ToolDescriptor, ToolExecutor, ToolOutcome, ToolRequest,
    ToolResult,
};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
