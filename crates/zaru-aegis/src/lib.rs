// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The AEGIS orchestrator client. Every interaction crosses a process
//! boundary over MCP or SEAL.
//!
//! # Boundary
//!
//! ADR-0003 D5 forbids linking AGPL code into this process, and the reason
//! given there is architectural before it is legal: a harness that can only
//! reach AEGIS through the protocols any third party would use cannot
//! accumulate private back-doors, because breaking the boundary becomes a
//! visible act rather than an import.
//!
//! The one sibling dependency this crate carries is [`zaru_seal`], because
//! ADR-0004 D1 has the harness attest as a SEAL principal and sign its own
//! envelopes. Errors raised here are this crate's own.

/// The name of this crate, read from its `Cargo.toml` at compile time.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// The version of this crate, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The SEAL implementation this client signs its envelopes with.
///
/// ADR-0004 D5 has the harness carry its own; this names the crate that does.
pub const SIGNS_WITH: &str = zaru_seal::NAME;

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only -- see the note in `zaru-core`.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(NAME, "zaru-aegis");
        assert_eq!(
            VERSION.split('.').count(),
            3,
            "version {VERSION} is not three dot-separated components"
        );
    }

    // Asserts the one sibling edge this crate is allowed, by reading a value
    // that only exists if `zaru-seal` actually linked. The mutant that makes
    // it disagree is removing the dependency from `Cargo.toml`, which stops
    // this crate compiling at all.
    #[test]
    fn the_seal_dependency_is_linked() {
        assert_eq!(SIGNS_WITH, "zaru-seal");
    }
}
