// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0013 D6's number, as data.
//!
//! "The status line carries context usage continuously. Approaching the
//! threshold is not an event to announce — it is a number that has been
//! visible all along."
//!
//! **There is no status line here and this module does not render one.**
//! ADR-0008 D2 makes this crate headless; `zaru-tui` subscribes and renders.
//! What this crate owes D6 is a number a renderer can carry, measured through
//! [`TokenCounter`](crate::context::TokenCounter) rather than guessed at.

use serde::{Deserialize, Serialize};

/// How much of the window the assembled context occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    used: u64,
    window: u64,
}

impl Usage {
    /// Report a usage.
    #[must_use]
    pub const fn new(used: u64, window: u64) -> Self {
        Self { used, window }
    }

    /// Tokens the assembled context occupies.
    #[must_use]
    pub const fn used(self) -> u64 {
        self.used
    }

    /// Tokens the window allows.
    #[must_use]
    pub const fn window(self) -> u64 {
        self.window
    }

    /// Tokens left before the window is full.
    ///
    /// Saturating, because a context that has already outgrown its window is
    /// a real state — it is the one ADR-0013 D7 turns into exhaustion — and
    /// a negative remainder is not a thing a status line can carry.
    #[must_use]
    pub const fn remaining(self) -> u64 {
        self.window.saturating_sub(self.used)
    }
}
