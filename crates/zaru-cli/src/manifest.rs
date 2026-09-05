// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009]'s `zaru.toml`: the project manifest as typed data, and the line
//! a project without one is owed.
//!
//! # One file, two records, one reader
//!
//! [ADR-0009] D1's `./zaru.toml` **is** [ADR-0014] D1's layer 3. That is not a
//! coincidence to be managed; it is why the reader lives here, in the crate
//! [Bounded Contexts] gives configuration to, and why there is exactly one of
//! it. Two ports reading one file would be two parses that can disagree about
//! what the file says.
//!
//! # This module enumerates no configuration key
//!
//! [`Manifest`] carries `[project]` and `[runtime]` as
//! [`Table`](crate::config::Table)s and hands them to the hierarchy as a
//! layer-3 [`Contribution`](crate::config::Contribution). It has **no typed
//! field for `max_iterations`, for `tier`, or for `workspace`**, and that is
//! ADR-0014's own instruction rather than an omission: "Nothing here specifies
//! the schema. Each record owns its own keys; this one owns how they resolve."
//! A caller builds the [`Schema`](crate::config::Schema), and
//! `runtime.max_iterations` is spelled there and nowhere else. Spelling it
//! here as well would be [Agent lessons] §5 — one record's key fixed inside
//! another record's implementation, in a second place that can drift from the
//! first.
//!
//! **`[[validator]]` is lifted out and is not a configuration key at all.**
//! Under a delegated coordinator ruling of 2026-09-04 that is the proposed
//! reading of ADR-0014 D2's wholesale array replacement against ADR-0009 D1:
//! validators are per-project by D1's "one file per project", so the merge rule
//! never reaches them. It is drafted as an Update on ADR-0009 with the other
//! reading — that a project declaring any validator discards the user's whole
//! set — recorded beside it. **Neither is settled here.**
//!
//! # What is built and what waits
//!
//! | ADR-0009 | Built here |
//! | --- | --- |
//! | D1 — one file, `[project]`, `[runtime]`, `[[validator]]` | the shape, as data a caller builds; the reader is a port |
//! | D2 — dependency order, `skipped` distinct | `zaru-core`'s, and it is where [Bounded Contexts] puts dispatch |
//! | D3 — four `expect` kinds | the vocabulary is `zaru-core`'s; two of the four are decided in [`crate::validators`], and the `json_schema` path's **boundary** is measured here and again there |
//! | D4 — the missing-manifest line, once | [`MissingManifest`], as data, with the wording a parameter |
//! | D5 — validator output into refinement | `zaru-core`'s, asserted through the loop |
//! | D6 — read, never written | [`ManifestSource`] has one method and there is no writer anywhere |
//!
//! [`ManifestSource`] is **not implemented in this crate's product tree**, for
//! the reason [`crate::config::port`] gives at length: [ADR-0003] D2's table
//! names no TOML crate, and a third proposed amendment to that record is
//! drafted there. Until one is accepted the honest shape is a declared seam.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Agent lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/agent-lessons
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

pub mod absent;
pub mod document;
pub mod file;
pub mod init;
pub mod port;

pub use absent::{MissingManifest, Recommendation};
pub use document::{
    MANIFEST_FILE, Manifest, ManifestRefused, NAME_KEY, PROJECT_TABLE, RUNTIME_TABLE,
    VALIDATOR_TABLE, WORKSPACE_KEY, declare, fields,
};
pub use file::{ManifestFile, ManifestNotRead};
pub use init::{InitRefused, TEMPLATE};
pub use port::ManifestSource;

#[cfg(test)]
mod tests;
