// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The built-in that acts and is not implemented, and why it is a port rather
//! than code.
//!
//! **Nothing in this crate's product tree implements this.** A check
//! implements it; the product does not.
//!
//! # `fs.write` and `fs.edit` were here until 2026-09-05
//!
//! `FileWrites` was declared here because [ADR-0011] D4's classification
//! resolves a candidate through its **longest existing ancestor**, so a write
//! is classified against a tree that does not yet contain what it is about to
//! make. That is still true. What changed is that it is now recognised as the
//! same check-at-a-moment the read path always had, costing more rather than
//! being a different kind of gap — and a port whose only implementations were
//! four test doubles was a seam a check could substitute for the real thing
//! rather than a decision waiting to be made. The two act in
//! [`crate::tools::files`], which says exactly what is and is not claimed, and
//! the port is deleted rather than kept beside them.
//!
//! D6's destructive-pattern matcher is still unimplemented, so a write's
//! prompt cannot be given the prominence D6 requires — a gap in the prompt
//! rather than in the act.
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

use crate::tools::output::Captured;
use core::future::Future;
use zaru_core::iteration::PortFailure;

/// Content and filename search. `fs.search`.
pub trait Search {
    /// Search for what `query` describes.
    fn find(&self, query: &str) -> impl Future<Output = Result<Captured, PortFailure>> + Send;
}
