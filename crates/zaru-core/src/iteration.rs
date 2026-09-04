// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The iteration loop: ADR-0008's inner cycle.
//!
//! Two loops exist in this harness and they are not the same. The **tool-call
//! loop** is what every agentic harness runs — the model requests a tool, the
//! harness executes it, the result returns, the model continues. The
//! **iteration loop** is the 100monkeys cycle: generate a candidate, execute
//! it, evaluate it against deterministic validators, and refine using the
//! failure text as input. This module is the second one. It runs only where
//! validators are declared; at `bare` tier there is nothing to validate
//! against and so nothing to refine, and the tool-call loop runs alone.
//!
//! **A retry is not an iteration.** A retry repeats an operation hoping for a
//! different outcome; an iteration reads the failure and changes the next
//! attempt because of it. Nothing here is named `attempt`, `retry` or `turn`,
//! for the reason [Ubiquitous Language] gives: code that uses the words
//! interchangeably cannot honour the distinction, and the distinction is the
//! entire product claim.
//!
//! # What this module owns, and what it does not
//!
//! It owns the state machine, the event stream, the construction of the
//! refinement prompt, validator dispatch, and context assembly — meaning the
//! code that iterates the validators' reports, emits their events, and
//! invokes the context policy at an iteration boundary. Each individual
//! validator and the policy itself sit behind a trait in [`port`], as a seam
//! inside this crate rather than a dependency out of it.
//!
//! It owns no numbers. The iteration ceiling is ADR-0001 D3's and arrives as
//! a parameter; no record carries a truncation budget, so that arrives as a
//! parameter too.
//!
//! # Headless
//!
//! ADR-0008 D2: this crate renders nothing and depends on no terminal.
//! Consumers subscribe to [`EventSink`] and are given what they need;
//! rendering never reads loop internals. Nothing in the product tree here
//! implements a port, opens a socket, or spawns a process, and
//! [`port::SystemClock`] is the only thing that reads the machine's clock.
//!
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod error;
pub mod event;
pub mod limits;
pub mod machine;
pub mod port;
pub mod refinement;

pub use error::{IterationError, PortKind};
pub use event::{Event, EventSink, ExhaustionReason, ValidatorOutcome};
pub use limits::{Ceiling, ConfigurationError, Limits, TruncationBudget};
pub use machine::{Outcome, State, run};
pub use port::{
    Clock, ContextPolicy, ContextRefusal, ExecutionOutcome, Executor, Generated, Generator,
    PortFailure, Ports, Prompt, SystemClock, Turn, ValidatorReport, Validators,
};
pub use refinement::{RefinementInput, RefinementPrompt};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
