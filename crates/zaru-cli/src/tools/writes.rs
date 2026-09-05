// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The three built-ins that act and are not implemented, and why each is a
//! port rather than code.
//!
//! **Nothing in this crate's product tree implements any of these.** A check
//! implements them; the product does not.
//!
//! # `fs.write` and `fs.edit` — a write is classified against a tree that
//! does not contain what it is about to make
//!
//! [ADR-0011] D4's classification resolves a candidate through its **longest
//! existing ancestor**, which is what lets a path that does not exist yet be
//! classified at all — and every `fs.write` that creates a file passes one.
//! What that means for a write is a question the record does not answer: the
//! segment the write is about to create is precisely the segment the
//! classification could not follow, so the boundary is being asserted about a
//! tree that will be different by the time the write lands.
//!
//! Reading has the same gap and a much smaller consequence — a read that
//! escapes discloses, a write that escapes destroys — and D2 already says the
//! `bare` boundary is advisory. So the reads are built and these are not, and
//! the question is raised on the record rather than answered by whoever wrote
//! the executor. [ADR-0004]'s membrane is the answer that does not depend on
//! winning a race.
//!
//! D6's destructive-pattern matcher is the other half and has no
//! implementation either, so a write's prompt could not be given the
//! prominence D6 requires even if the write itself were built.
//!
//! # `fs.search` — a matcher is a dependency or an invented vocabulary
//!
//! D1's row is "Content and filename search", which merges two platform tools
//! ([ADR-048] maps `search.grep` to `fs.grep` and `search.glob` to
//! `fs.glob`). Content search needs a regular-expression engine, which is not
//! in [ADR-0003] D2's table; filename search needs a glob semantics, and
//! choosing one — whether `**` crosses a symlink, whether a leading dot
//! matches — is authoring the surface's search vocabulary.
//!
//! # `web.fetch` — a socket
//!
//! No socket exists anywhere in this workspace, and ADR-0011 D4's boundary is
//! about paths rather than outbound destinations. `cmd.run` was named here
//! until 2026-09-05 and is now built, over [`crate::process`] — though it is
//! still the tool D6's destructive categories are mostly about, and that
//! matcher has no implementation, so a `cmd.run` cannot be given the prompt
//! prominence D6 requires.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-048]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/048-core-mcp-tools-implementation

use crate::tools::name::ToolName;
use crate::tools::output::Captured;
use core::future::Future;
use zaru_core::iteration::PortFailure;

/// Makes a file change real. `fs.write` and `fs.edit`.
///
/// The tool is a parameter rather than two methods because both are one act
/// — putting bytes where a path says — and splitting them here would make
/// the caller decide which is which a second time, after
/// [`ToolName::effect`] already has.
pub trait FileWrites {
    /// Apply the change `target` describes.
    fn apply(
        &self,
        tool: ToolName,
        target: &str,
    ) -> impl Future<Output = Result<Captured, PortFailure>> + Send;
}

/// Content and filename search. `fs.search`.
pub trait Search {
    /// Search for what `query` describes.
    fn find(&self, query: &str) -> impl Future<Output = Result<Captured, PortFailure>> + Send;
}
