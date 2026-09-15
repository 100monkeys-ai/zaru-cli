// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a person allowed for the rest of this session, and nothing longer.
//!
//! # Why this exists and why it is not the allowlist
//!
//! [ADR-0011] D3's prompt offered allow-once and decline, which the
//! look-and-feel survey's row 10 recorded as a gap: "there is no third
//! option". D3's allowlist is the durable answer and it is deliberately at the
//! user's own configuration layer — that record's 2026-09-05 amendment argued
//! against letting the prompt write it, because a one-keystroke answer
//! becoming a grant a user never wrote down is exactly what an allowlist at
//! layer 2 exists to prevent.
//!
//! **This is the answer that is not durable.** A grant lives in memory for the
//! life of the process, is never written to any configuration layer, and is
//! gone when the session's process ends. `tools.allowlist` is untouched: there
//! is no writer to that key anywhere in this crate, and
//! `no_permission_answer_writes_a_configuration_layer` walks the tree to say
//! so.
//!
//! # It matches exactly what the allowlist matches
//!
//! The tool and [`Invocation::subject_text`], byte for byte, with no glob, no
//! prefix and no normalisation — the same comparison
//! [`allowlist::Entry::approves`](crate::tools::allowlist::Entry::approves)
//! makes, reached through that type rather than spelled again. "Allow this
//! line" therefore means the line that was on the screen and nothing near it.
//!
//! # Why it is a value and not a global
//!
//! A `static` would need no wiring at all and is refused: this crate takes its
//! inputs as arguments everywhere — `config::environment::read` takes its
//! variables, `Files::at` takes its paths, `Prompt::over` takes its handles —
//! and a process-global would make every check that touches a grant depend on
//! the order the checks ran in.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::tools::allowlist::Entry;
use crate::tools::decision::Invocation;
use std::sync::Mutex;

/// The lines this session's user has allowed, in the order they allowed them.
///
/// Interior mutability because [`Executor`](crate::tools::Executor) holds its
/// ports by shared reference and requires them `Sync`, and because a grant is
/// made in the middle of a call the executor is already inside.
#[derive(Debug, Default)]
pub struct SessionGrants {
    granted: Mutex<Vec<Entry>>,
}

impl SessionGrants {
    /// Nothing granted, which is what every session starts with.
    ///
    /// **Including a resumed one.** A grant is not persisted, so a session
    /// reopened in a new process has none — which is the property, not a
    /// shortcoming of the resume path.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Remember that the user allowed this exact call for the session.
    ///
    /// Idempotent: allowing the same line twice leaves one entry, so the count
    /// is the number of distinct lines a person said yes to.
    pub fn allow(&self, invocation: &Invocation<'_>) {
        let entry = Entry::of(invocation.called().clone(), invocation.subject_text());
        let mut granted = match self.granted.lock() {
            Ok(granted) => granted,
            // A poisoned lock means a check or a caller panicked while holding
            // it. Failing to record a grant makes the next call prompt again,
            // which is the safe direction on a permission boundary, so this
            // does not panic in turn.
            Err(_) => return,
        };
        if !granted.contains(&entry) {
            granted.push(entry);
        }
    }

    /// Whether the user has already allowed this exact call this session.
    #[must_use]
    pub fn approves(&self, invocation: &Invocation<'_>) -> bool {
        self.granted
            .lock()
            // A poisoned lock answers "no", which prompts. See `allow`.
            .is_ok_and(|granted| granted.iter().any(|entry| entry.approves(invocation)))
    }

    /// How many distinct lines have been allowed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.granted.lock().map_or(0, |granted| granted.len())
    }

    /// Whether nothing has been allowed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
