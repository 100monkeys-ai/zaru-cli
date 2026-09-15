// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015]'s out-of-session command surface: the parser, the namespaces,
//! and what the binary prints.
//!
//! # This is the half of D2 a user reaches from a shell
//!
//! D2, settled 2026-09-05 under directive 20: "**A namespace has two entry
//! points, and they are one operation.** `/session <verb>` inside a session
//! and `zaru sessions <verb>` outside one are the same commands reached from
//! the two places a user can be." This module is the second of those two. The
//! first needs the terminal, which is [ADR-0005]'s and does not exist, so
//! **nothing here renders anything**: every command produces a `Vec<String>`,
//! and the binary writes them. The day `/runtime` exists it calls the same
//! function and hands the lines to the composer, so there is one projection
//! per datum rather than two that can disagree.
//!
//! # What is built here, and what is not
//!
//! | [ADR-0015] | Built here |
//! | --- | --- |
//! | D1 — three extension kinds | the two that are files, in [`crate::commands`]; the MCP server is not one |
//! | D2 — the namespace table | the table as a closed type, and the subcommands this harness implements |
//! | D3 — commands and skills discovered from three places | two of three; the served location loads nothing |
//! | D4 — a project extension is inert until admitted | yes, for both file kinds, in [`crate::commands::admission`] |
//! | D5 — skills declare their validators | yes, in [`crate::commands::skill`], run by the turn's own plan |
//! | D6 — every contribution is attributed in the transcript | yes, as [`crate::session::Record::Attribution`] |
//! | D7 — no marketplace | yes, by absence, as it always was |
//!
//! **Dated 2026-09-15.** Four of those rows read `**no**` and the paragraph
//! beneath them read "**None of this record's six trigger clauses moves**"
//! until this line, and both had been false since the `command-files` arc
//! landed `0c8ea16..6bdf080` earlier the same day. Nothing here is a gate, so
//! nothing checked it — which is the shape [Agent lessons] §76 records for a
//! user-facing "cannot do yet" list and §69 for an arm written while an
//! operation was a refusal, arriving in module documentation. The table above
//! is re-derived rather than edited row by row.
//!
//! **What the clauses now say**, from the record's own Status tracking rather
//! than from this file: clauses 1 and 5 are whole, clauses 4 and 6 are whole
//! across both file kinds, clause 2 is satisfied by D5's skill, **clause 3
//! does not move** at two of three locations, and clause 7 is unchanged —
//! an admitted command or skill is not a namespace.
//!
//! [Agent lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/agent-lessons-3
//!
//! # It also becomes ADR-0014 layer 5's first reader
//!
//! [`layers::Flags`] implements [`LayerSource`](crate::config::LayerSource)
//! for [`Layer::Flag`](crate::config::Layer), which is the first
//! implementation of that port anywhere in this crate's product tree, and
//! [`layers::BuiltIn`] supplies layer 1's one compiled-in key. Layers 2 and 3
//! still have no reader.
//!
//! # This module is the first thing in the harness a user can run
//!
//! Which makes it the first place several other records become observable
//! rather than merely built: [ADR-0016] D5's exit codes on the real artefact,
//! [ADR-0014] D3's explain block, [ADR-0012] D4's alias listing, [ADR-0001]
//! D2's datum, [ADR-0010] D6's deletion, and one of [ADR-0007] D7's five
//! surfaces. Each of those is a projection of a datum a landed module already
//! produces; **no module's rules are restated here**.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

pub mod classify;
pub mod flag;
pub mod help;
pub mod invocation;
pub mod layers;
pub mod namespace;
pub mod parse;
pub mod refusal;
pub mod render;
pub mod run;

pub use classify::Surface;
pub use flag::Flag;
pub use invocation::{CommandLine, Overrides, Request};
pub use layers::{
    FILE_CEILING_BYTES, Files, Flags, LoadFailure, OUTPUT_BUDGET_BYTES, PATTERN_CEILING_BYTES,
    PROCESS_CEILING, ProjectFile, SEARCH_CEILING_BYTES, UserFile,
};
pub use namespace::Namespace;
pub use parse::{parse, parse_process};
pub use refusal::CommandRefused;
pub use run::{Outcome, Run};

#[cfg(test)]
mod tests;
