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
//! | D1 — three extension kinds | **no**; nothing is loaded from a file |
//! | D2 — the namespace table | the table as a closed type, and the five subcommands this harness implements |
//! | D3 — commands and skills discovered from three places | **no** |
//! | D4 — a project extension is inert until admitted | **no**; nothing is discovered, so nothing is admitted |
//! | D5 — skills declare their validators | **no** |
//! | D6 — every contribution is attributed in the transcript | **no** |
//! | D7 — no marketplace | yes, by absence, as it always was |
//!
//! **None of this record's six trigger clauses moves**, and that is worth
//! stating rather than leaving to be noticed: all six are about *extensions*,
//! and D2's namespaces have no clause at all. A seventh is drafted on the
//! record for D2 under a delegated coordinator ruling of 2026-09-05.
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
pub use layers::{Flags, LoadFailure};
pub use namespace::Namespace;
pub use parse::{parse, parse_process};
pub use refusal::CommandRefused;
pub use run::{Outcome, Run};

#[cfg(test)]
mod tests;
