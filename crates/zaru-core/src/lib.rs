// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The agent loop, the iteration state machine, the validator contract, and
//! the typed event stream every other surface consumes.
//!
//! # Boundary
//!
//! This crate is headless. It renders nothing, it depends on no terminal, and
//! it depends on no sibling crate at all. ADR-0008 D2 requires the first two:
//! a loop reachable only through a rendered interface cannot be tested at the
//! speed the inner loop needs, and cannot be driven by a future non-terminal
//! surface. ADR-0003 D8 states the same rule from the other side --
//! `zaru-core` must not depend on `zaru-tui` -- and because `zaru-tui`
//! depends on this crate, Cargo's refusal of dependency cycles enforces that
//! clause without anybody having to remember it.
//!
//! Rendering never reads loop internals. What a consumer needs to display,
//! the loop emits as an event (ADR-0008 D3).
//!
//! Every path from captured bytes into a model prompt passes one port,
//! [`redaction::Redactor`], and the type it produces is the only thing a
//! [`Prompt`](iteration::Prompt) or a
//! [`ToolResult`](tool_call::ToolResult) can be built from. That is
//! [ADR-0008]'s trigger clause 6, decided on 2026-09-05.
//!
//! Errors raised here are this crate's own. `zaru-cli` owns the ADR-0016
//! taxonomy and the mapping to exit codes; this crate is deliberately not a
//! shared-types crate for the workspace. Where a shared error type should
//! live is an open question the skeleton left open on purpose.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop

pub mod context;
pub mod conversation;
pub mod iteration;
pub mod redaction;
pub mod tool_call;

/// The name of this crate, read from its `Cargo.toml` at compile time.
pub const NAME: &str = env!("CARGO_PKG_NAME");

/// The version of this crate, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    // Liveness only. This proves the test harness runs and that this crate is
    // linkable from a test target. It is not evidence about behaviour, because
    // there is no behaviour here yet, and it must not be quoted as if it were.
    //
    // The two sides are genuinely different readers: `NAME` comes from Cargo's
    // package metadata and the literal comes from this file. The mutant that
    // makes them disagree is renaming the package in `Cargo.toml`.
    #[test]
    fn package_metadata_reaches_the_test_binary() {
        assert_eq!(NAME, "zaru-core");
        assert_eq!(
            VERSION.split('.').count(),
            3,
            "version {VERSION} is not three dot-separated components"
        );
    }
}
