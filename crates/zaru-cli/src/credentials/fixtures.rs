// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Values the checks are built from. Compiled only under `cfg(test)`.
//!
//! # Every test credential is a generated nonce, and a nonce is not a secret
//!
//! No real credential enters this crate, its checks, or its fixtures. The
//! nonces here authenticate nothing — they are **uniqueness devices**, which
//! is why `std` alone can make one and no random-number crate is needed. What
//! they have to be is *recognisable*: a check asserts that a value it planted
//! does not appear in some rendered text, and that assertion is only as good
//! as the chance the value would have shown up if the redaction failed.
//!
//! # Why they are deliberately awkward
//!
//! [Verification lessons] §9: a fixture can be too well-behaved. Every nonce
//! here carries non-ASCII text, a multi-byte grapheme cluster built from a
//! combining mark, and an astral-plane character, so that:
//!
//! - a renderer that hard-coded a plausible-looking mask cannot match one by
//!   accident;
//! - a redaction that truncated at a byte boundary rather than a character
//!   boundary would produce mojibake a check can see;
//! - and a check that passed by matching a constant fails on the next run,
//!   because the nonce moves every time.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Distinguishes two nonces taken inside one clock tick.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// The awkward tail every nonce carries, and the reason it is a named
/// constant rather than three characters written inline.
///
/// A decomposed grapheme cluster (`e` plus a combining acute), a precomposed
/// one, and an astral-plane character.
pub(crate) const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

/// A value no other call to this function will produce.
///
/// `label` is there so a failure names which fixture it came from. The tail
/// is: a decomposed grapheme cluster (`e` plus a combining acute, one
/// grapheme over three bytes and two chars), a precomposed one, and an
/// astral-plane character over four bytes.
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
/// **This exists because of a surviving mutation.** A check asserting that a
/// rendered string does not `contain` a planted nonce is blind to a formatter
/// that escapes: `{:?}` on a `String` renders a combining mark as `\u{301}`,
/// so the raw nonce is genuinely absent from a rendering that published every
/// byte of it. The mutation that put the bearer value into
/// `SecretRefused`'s `Display` through `{:?}` therefore survived, and the
/// green was about the escaping rather than about the redaction.
///
/// The core is pure ASCII — label, pid, nanos and sequence — so no escaping
/// scheme changes it, and it is on its own enough to identify the value. A
/// check asserts the absence of both: the raw value catches a verbatim leak,
/// the core catches an escaped one.
///
/// [Verification lessons] §9 is the general shape: the fixture was too
/// well-behaved in a direction nobody expects, by being *more* awkward rather
/// than less.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub(crate) fn ascii_core(value: &str) -> &str {
    value.strip_suffix(AWKWARD_TAIL).unwrap_or(value)
}

/// A bearer value that is shaped like a personal token and is not one.
pub(crate) fn personal_secret_nonce() -> String {
    format!("nn_mcp_{}", nonce("secret"))
}

/// A bearer value that is shaped like an app token and is not one.
pub(crate) fn app_secret_nonce() -> String {
    format!("nn_app_{}", nonce("appsecret"))
}

/// A directory tree a check owns, removed when the check ends.
///
/// [Testing]'s rule: "Each test owns its own state... its own configuration
/// directory, its own session store, and its own working directory, writing
/// to the paths the product actually writes to inside that root — which is
/// the only honest way to exercise a path with no seam to inject a fake
/// into." The store takes its root as a parameter because the product needs
/// it to, so a check here is an ordinary caller rather than a fake.
///
/// The `control` directory beside the store's root is not decoration. It is
/// the reading that discriminates when a check claims a root is gone: a
/// checker that reports absence for everything passes on the root and fails
/// on the control. The library's [Credentials] page requires exactly this —
/// "verify with more than one check, and know which of them discriminates".
///
/// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
/// [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials
pub(crate) struct ScratchRoot {
    base: std::path::PathBuf,
}

impl ScratchRoot {
    /// Make a tree no other check is using.
    pub(crate) fn new() -> Self {
        let base = std::env::temp_dir().join(nonce("cs-scratch"));
        std::fs::create_dir_all(base.join("control")).expect("could not stage the scratch tree");
        Self { base }
    }

    /// Where the store goes. It does not exist until the store makes it.
    pub(crate) fn store_root(&self) -> std::path::PathBuf {
        self.base.join("zaru")
    }

    /// A sibling that must survive whatever happens to the store's root.
    pub(crate) fn control(&self) -> std::path::PathBuf {
        self.base.join("control")
    }

    /// The directory both of the above sit in.
    pub(crate) fn base(&self) -> &std::path::Path {
        &self.base
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A confirmer that answers as it was built to, and records what it was told.
pub(crate) struct StagedConfirmer {
    answer: bool,
    told: std::cell::RefCell<Vec<String>>,
}

impl StagedConfirmer {
    /// A confirmer that says yes.
    pub(crate) fn accepting() -> Self {
        Self {
            answer: true,
            told: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// A confirmer that says no.
    pub(crate) fn declining() -> Self {
        Self {
            answer: false,
            told: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Every sentence the store asked this confirmer to state.
    pub(crate) fn told(&self) -> Vec<String> {
        self.told.borrow().clone()
    }
}

impl crate::credentials::port::Confirm for StagedConfirmer {
    fn confirm_apex(&self, _alias: &crate::credentials::alias::Alias, grants: &str) -> bool {
        self.told.borrow_mut().push(grants.to_owned());
        self.answer
    }
}
