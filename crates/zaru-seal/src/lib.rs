// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Zaru's own SEAL implementation: the signed envelope, its canonical wire
//! format, the replay window, the `jti` uniqueness check, and the error-code
//! registry the harness renders verdicts from.
//!
//! # Boundary
//!
//! This crate depends on no sibling crate, and that is load-bearing rather
//! than incidental. ADR-0004 D5 has the harness implement SEAL from the
//! published RFC rather than extracting a crate from the AGPL orchestrator,
//! which is what keeps ADR-0003 D5's arm's-length boundary intact; and
//! ADR-0004 keeps open the option of publishing this crate as the canonical
//! Apache-2.0 Rust SEAL SDK. A crate carrying a sibling dependency cannot be
//! published on its own, so the zero-dependency shape is the option staying
//! open.
//!
//! Errors raised here are this crate's own -- SEAL has its own error-code
//! registry and does not borrow the harness's taxonomy.

/// The name of this crate, read from its `Cargo.toml` at compile time.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// The version of this crate, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only -- see the note in `zaru-core`. No SEAL behaviour exists
    // yet, and this test says nothing about any.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(NAME, "zaru-seal");
        assert_eq!(
            VERSION.split('.').count(),
            3,
            "version {VERSION} is not three dot-separated components"
        );
    }
}
