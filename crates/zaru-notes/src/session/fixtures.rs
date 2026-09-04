// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Values the checks are built from. Compiled only under `cfg(test)`.
//!
//! # Every test credential is a generated nonce, and a nonce is not a secret
//!
//! No real credential enters this crate, its checks, or its fixtures. The
//! nonces here authenticate nothing — they are **uniqueness devices**, which is
//! why `std` alone can make one and no random-number crate is needed. What they
//! have to be is *recognisable*: a check asserts that a value it planted does
//! not appear in some rendered text, and that assertion is only as good as the
//! chance the value would have shown up if the redaction failed.
//!
//! # Why they are deliberately awkward
//!
//! Every nonce carries non-ASCII text, a multi-byte grapheme cluster built from
//! a combining mark, and an astral-plane character, so that a renderer that
//! hard-coded a plausible mask cannot match one by accident, a redaction that
//! truncated at a byte boundary would produce mojibake a check can see, and a
//! check that passed by matching a constant fails on the next run.
//!
//! # Why this is not `zaru-cli`'s copy
//!
//! `zaru-cli` carries the same fixtures as `pub(crate)`, and reusing them here
//! is not possible: [ADR-0003] D8 permits `zaru-notes` no sibling dependency,
//! and `scripts/check-crate-boundaries.py` counts a **dev**-dependency as a
//! sibling edge exactly as it counts a normal one — measured, not assumed. The
//! duplication is the crate boundary's price and is cheaper than the edge.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Distinguishes two nonces taken inside one clock tick.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The awkward tail every nonce carries.
///
/// A decomposed grapheme cluster (`e` plus a combining acute), a precomposed
/// one, and an astral-plane character.
pub(crate) const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

/// A value no other call to this function will produce.
pub(crate) fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}{AWKWARD_TAIL}", std::process::id())
}

/// The part of a nonce that no formatter can alter, and why a check needs it.
///
/// **This exists because of a surviving mutation elsewhere in this workspace.**
/// A check asserting that a rendered string does not `contain` a planted nonce
/// is blind to a formatter that escapes: `{:?}` on a `String` renders a
/// combining mark as `\u{301}`, so the raw nonce is genuinely absent from a
/// rendering that published every byte of it. The core is pure ASCII — label,
/// pid, nanos and sequence — so no escaping scheme changes it, and it is on its
/// own enough to identify the value. A check asserts the absence of both: the
/// raw value catches a verbatim leak, the core catches an escaped one.
pub(crate) fn ascii_core(value: &str) -> &str {
    value.strip_suffix(AWKWARD_TAIL).unwrap_or(value)
}

/// A bearer value shaped like a personal token and not being one.
pub(crate) fn bearer_nonce() -> String {
    format!("nn_mcp_{}", nonce("bearer"))
}

/// Assert that `rendered` carries neither `planted` nor its ASCII core.
///
/// Two arms, and the second is the one that catches an escaped leak. Each names
/// which arm fired, because a failure that says only "the value is present"
/// leaves the reader unable to tell a verbatim leak from an escaped one.
pub(crate) fn assert_absent(what: &str, rendered: &str, planted: &str) {
    assert!(
        !rendered.contains(planted),
        "{what} published the bearer value verbatim; it is in {rendered:?}"
    );
    let core = ascii_core(planted);
    assert!(
        !rendered.contains(core),
        "{what} published the bearer value in an escaped form; its ASCII core {core:?} is in \
         {rendered:?}"
    );
}
