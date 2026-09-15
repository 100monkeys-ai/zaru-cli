// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D1's **Command**: a named prompt template with arguments, which
//! cannot execute anything.
//!
//! # What this module is, in the record's own words
//!
//! D1's table: "**Command** | A named prompt template with arguments | No —
//! expands to text", and under it: "**A command cannot execute anything.** It
//! expands into the conversation. That makes sharing commands safe in a way
//! sharing servers is not, and it is why the cheap-to-share kind is the inert
//! one."
//!
//! That inertness is a property of these types rather than a rule somebody
//! remembered. A [`Command`] holds a name, an optional description and a
//! **body of text**; there is no field on it, and none on
//! [`Expanded`](crate::commands::Expanded), that a tool name could ride. The
//! expansion is a `String`, and its one consumer is the task a turn runs.
//!
//! # The two locations, and the third that is not here
//!
//! D3 names three:
//!
//! ```text
//! ~/.zaru/commands/<name>.md      user
//! ./.zaru/commands/<name>.md      project
//! <mcp server>                    served
//! ```
//!
//! **Two of the three load.** The served location is deliberately not built
//! and is not half-built: it needs an MCP client for an arbitrary server
//! (`zaru-notes` speaks to Nuclear Notes and to nothing else), a credential
//! per server, and a registration point — and before any of that it is
//! **decision-blocked** on the open question "whether the built-in tool names
//! share a namespace with MCP server tools", [ADR-0011] D1 against [ADR-0015]
//! D2, which is on the catalogue's open-questions page with no owner.
//! **Clause 3 therefore does not move**: two of three locations load, said
//! exactly.
//!
//! # A project's commands are inert until admitted
//!
//! D4: "**A cloned repository's commands and skills do not load on first
//! run.** The harness reports what the project offers and the user admits them
//! once; the decision is recorded per project." [`admission`] is where that
//! record lives, and [`load`] is what refuses to hand a project's commands to
//! anything until it says so. The user's own directory needs no admission,
//! because D4's reason is about repositories: "a user who trusts one
//! repository has said nothing about the next."
//!
//! # Where each piece is
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`document`] | [`Command`], [`Source`] and every way a file is refused |
//! | [`front_matter`] | D3's `+++`-fenced TOML head, split from the body |
//! | [`placeholder`] | `$ARGUMENTS` and `$1` to `$9`, and the one-pass expansion |
//! | [`load`] | the two locations, the precedence, and clause 5's collision |
//! | [`admission`] | D4's record, `~/.zaru/admissions.jsonl` |
//! | [`date`] | the civil date an admission is stamped with, without a dependency |
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

pub mod admission;
pub mod date;
pub mod document;
pub mod front_matter;
pub mod load;
pub mod placeholder;

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;

pub use admission::{Admission, AdmissionError, Admissions, ADMISSIONS_FILE};
pub use document::{
    Command, CommandRefused, Expanded, Source, ADMISSION_STATEMENT, COMMANDS_DIRECTORY,
    COMMAND_EXTENSION,
};
pub use load::{load_from, Loaded, Offer};
