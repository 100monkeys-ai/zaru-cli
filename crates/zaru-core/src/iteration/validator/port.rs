// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The three ports deciding a validator calls out through, none of them
//! implemented here.
//!
//! **Nothing in this crate's product tree implements any of the three**,
//! exactly as nothing implements the loop's five. A check implements them; the
//! product does not, which is why building [ADR-0009]'s dispatch spawns no
//! process, compiles no pattern and opens no file.
//!
//! # Why a validator's command has a port of its own
//!
//! It is not [`Executor`](crate::iteration::port::Executor). That port makes
//! *the candidate's* effect real, and [ADR-0008] D1 separates the tool-call
//! loop from the iteration loop precisely so that the two are not one thing:
//! "Conflating them produces a retry wrapper wearing the vocabulary of
//! iteration." A validator's `run` is neither loop's step — it is what
//! *evaluating* an execution costs.
//!
//! It is not [ADR-0011]'s `cmd.run` either. That record's permission model
//! decides what a **model-driven** action may reach, and every input its
//! decision needs is about a call the model chose. A validator command is
//! declared by the project, in a file the user can read, before the model says
//! anything. Routing it through a permission decision would invent a question
//! no record asks and would put a project-declared command under a model's
//! permission model.
//!
//! So there are three ports, and this is the third. **Its product
//! implementation is a subprocess and belongs in `zaru-cli`**, with whatever
//! record decides what running a project's command on a user's machine is
//! allowed to be; that is a later arc's and is deliberately not this one's.
//!
//! # Why the two evaluators are ports
//!
//! [ADR-0003] D2's table names no regular-expression engine and no JSON Schema
//! validator, and its Trigger clause 7 treats the table as closed in the other
//! direction too — a dependency the harness turns out not to need is removed
//! "by amendment rather than left standing unused". So declaring `regex` or a
//! schema crate is an amendment to that record rather than an import, and the
//! honest shape until one is accepted is a declared seam with no
//! implementation: the shape ADR-0007's sealing, ADR-0014's file layers and
//! ADR-0011's three ports all took on 2026-09-04.
//!
//! # What is a failing validator and what is a broken declaration
//!
//! `Ok(false)` is **a validator that failed** — the mechanism operating, which
//! [ADR-0016] D1 puts in its first class and explicitly not in the error
//! register. `Err(PortFailure)` is **the declaration being unusable**: a
//! pattern that is not a valid regular expression, a schema file that cannot
//! be read. The loop already carries a `PortFailure` from the validators out
//! as `IterationError::Port`, so nothing new is added to it.
//!
//! That split is a reading. [ADR-0009] D3 says a `json_schema` validator
//! passes when "Stdout parses as JSON and validates", which makes unparseable
//! output a *failure* rather than a port error — the command produced the
//! wrong thing, which is the validator doing its job. It is recorded on that
//! record rather than left for a reader to discover from behaviour.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::iteration::port::PortFailure;
use crate::iteration::validator::name::{Pattern, Run, SchemaPath};
use core::future::Future;

/// What running one validator's command produced.
///
/// Deliberately **not** [`ExecutionOutcome`](crate::iteration::port::ExecutionOutcome),
/// even though the three fields coincide and both live in this crate. That
/// type is what making a candidate's effect real produced; this is what
/// evaluating the candidate cost. Sharing one shape between them would make
/// the two loops [ADR-0008] D1 separates read as one, and the cost of keeping
/// them apart is three duplicated field names, recorded here rather than
/// hidden.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorOutput {
    /// The exit code the command reported.
    pub exit_code: i32,
    /// Everything the command wrote to standard output.
    pub stdout: String,
    /// Everything the command wrote to standard error.
    pub stderr: String,
}

/// Runs one validator's declared command.
///
/// **No implementation exists in any product tree.** See the module
/// documentation for why this is a third port rather than one of the two that
/// already exist.
pub trait ValidatorRunner {
    /// Run a command and report what it produced.
    ///
    /// A command that ran and failed is `Ok` carrying its exit code; `Err` is
    /// for a command that could not be run at all.
    fn run(
        &self,
        command: &Run,
    ) -> impl Future<Output = Result<ValidatorOutput, PortFailure>> + Send;
}

/// Decides [ADR-0009] D3's `matches = "<regex>"`.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub trait PatternMatch {
    /// Whether the command's standard output matches the declared pattern.
    ///
    /// # Errors
    ///
    /// [`PortFailure`] when the pattern is not a usable one. A pattern that is
    /// valid and does not match is `Ok(false)`.
    fn matches(
        &self,
        pattern: &Pattern,
        stdout: &str,
    ) -> impl Future<Output = Result<bool, PortFailure>> + Send;
}

/// Decides [ADR-0009] D3's `json_schema = "<path>"`.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub trait SchemaValidate {
    /// Whether the command's standard output parses as JSON and validates.
    ///
    /// # Errors
    ///
    /// [`PortFailure`] when the schema itself cannot be read or is not a
    /// schema. Output that is not JSON, or that is JSON and does not validate,
    /// is `Ok(false)` — the validator failing rather than the harness.
    fn validates(
        &self,
        schema: &SchemaPath,
        stdout: &str,
    ) -> impl Future<Output = Result<bool, PortFailure>> + Send;
}
