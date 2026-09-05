// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The terminal user interface and the composer.
//!
//! # Boundary
//!
//! This crate subscribes to [`zaru_core`]'s event stream and renders it. It
//! never reads loop internals: ADR-0008 D3 makes the event stream the whole
//! contract between the loop and every consumer, so what this surface needs
//! to display, the loop emits.
//!
//! The dependency runs one way only. ADR-0003 D8 forbids `zaru-core`
//! depending on `zaru-tui`, and because this crate depends on `zaru-core`,
//! Cargo's refusal of dependency cycles enforces that clause mechanically.
//!
//! Errors raised here are this crate's own.

pub mod composer;
pub mod shell;

/// The name of this crate, read from its `Cargo.toml` at compile time.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// The version of this crate, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The crate whose event stream this surface renders.
pub const RENDERS_FOR: &str = zaru_core::NAME;

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only -- see the note in `zaru-core`.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(NAME, "zaru-tui");
        assert_eq!(
            VERSION.split('.').count(),
            3,
            "version {VERSION} is not three dot-separated components"
        );
    }

    // Asserts the one sibling edge this crate is allowed, by reading a value
    // that only exists if `zaru-core` actually linked.
    #[test]
    fn the_core_dependency_is_linked() {
        assert_eq!(RENDERS_FOR, "zaru-core");
    }
}
