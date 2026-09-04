// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The Nuclear Notes client: reads and writes against a cortex workspace over
//! MCP, and the workspace-scoped addressing every such call requires.
//!
//! # Boundary
//!
//! This crate depends on no sibling crate. It speaks to Nuclear Notes and to
//! nothing else, so it has no reason to know that a loop, a terminal, or an
//! orchestrator exists.
//!
//! Errors raised here are this crate's own; `zaru-cli` maps them into the
//! ADR-0016 taxonomy.

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
