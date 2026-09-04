// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Values the tool-surface checks are built from. Compiled only under
//! `cfg(test)`.
//!
//! # Why the nonces are deliberately awkward
//!
//! [Verification lessons] §9: a fixture can be too well-behaved. A check that
//! asserts a refusal quoted back the key it was handed is only as good as the
//! chance that key would have shown up by accident, so every nonce here is
//! unique per call and carries text no implementation would produce on its
//! own.
//!
//! Nothing here is a credential. These are uniqueness devices, which is why
//! `std` alone makes one and no random-number crate is needed.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Distinguishes two nonces taken inside one clock tick.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call to this function will produce.
///
/// `label` is there so a failure names which fixture it came from.
pub(crate) fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}", std::process::id())
}
