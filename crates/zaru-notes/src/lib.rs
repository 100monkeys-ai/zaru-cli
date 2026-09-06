// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The Nuclear Notes client: a session over MCP against a cortex workspace,
//! and the workspace-scoped addressing every such call requires.
//!
//! # Boundary
//!
//! This crate depends on no sibling crate. It speaks to Nuclear Notes and to
//! nothing else, so it has no reason to know that a loop, a terminal, or an
//! orchestrator exists. The consequence that bites is not architectural but
//! practical: it cannot reuse `zaru-cli`'s `Secret`, nor that crate's test
//! fixtures, because `scripts/check-crate-boundaries.py` counts a
//! *dev*-dependency as a sibling edge exactly as it counts a normal one.
//! [`session::Bearer`] is therefore this crate's own type and the nonce
//! fixtures beside its checks are this crate's own too.
//!
//! Errors raised here are this crate's own; `zaru-cli` maps them into the
//! ADR-0016 taxonomy.
//!
//! # What is here, and what is deliberately not
//!
//! What is here is a session that can be driven end to end: it attaches, holds
//! what it negotiated, lists tools, resolves a slug and switches by id, reads a
//! page with the workspace named explicitly, and reports the three signals
//! [ADR-0007] D6 invalidates a tool-scope cache on.
//!
//! **What is here as of 2026-09-05 is a transport**, and the two sentences that
//! stood here until then said the opposite. They said "what is not here is a
//! transport" and that [`session::Endpoint`] "is a port with no implementation
//! in this crate's product tree", and both stopped being true when a token was
//! issued that an arc could open a session with: [`session::HttpEndpoint`] is
//! that implementation, over the streamable HTTP surface [ADR-0103] mounts. The
//! sentences are corrected rather than left, because a crate whose own
//! documentation says it cannot reach a network is worse than one that says
//! nothing.
//!
//! What replaces the claim they made is narrower and is still structural. A
//! [`session::Session`] reaches a network **only** through the
//! [`session::Endpoint`] it was handed, and it is handed one per call rather
//! than finding one: every check in this crate hands it the in-process `rmcp`
//! server over `tokio::io::duplex` instead, so no check here can open a socket
//! and none does. See [`session::transport`] for the three dependency shapes
//! that were measured and why the generic one was taken.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0103]: https://cortex.page/adrs/p/0103-mcp-server-transport-mount

pub mod session;
pub mod trie;

/// The name of this crate, read from its `Cargo.toml` at compile time.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// The version of this crate, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only -- see the note in `zaru-core`.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(NAME, "zaru-notes");
        assert_eq!(
            VERSION.split('.').count(),
            3,
            "version {VERSION} is not three dot-separated components"
        );
    }
}
