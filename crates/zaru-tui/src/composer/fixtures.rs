// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Staged implementations for the composer's checks.
//!
//! These are the test tree. Nothing here has a counterpart in the product tree
//! and nothing here reaches a network.
//!
//! Three properties are load-bearing and must survive anybody tidying this
//! file up.
//!
//! **The two tiers carry disjoint nonces.** [`TRIE_NONCE`] appears only in what
//! the trie returns and [`SERVER_NONCE`] only in what the server returns, so a
//! composer that rendered one tier while claiming the other cannot pass. A
//! fixture whose two sets agreed would separate nothing.
//!
//! **They overlap by exactly one entity, with different titles.** That is the
//! only shape that can tell a merge keyed on identity from one that appends,
//! and it is the only shape that can say which tier's copy survived.
//!
//! **Every title carries a non-ASCII character and a multi-byte grapheme**, so
//! an implementation that hard-coded plausible text could not produce one, and
//! so a character count and a byte count are visibly different numbers.

use crate::composer::Composer;
use crate::composer::entries::{Entries, Entry, EntryKind};
use core::time::Duration;
use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::layout::Position;
use std::cell::Cell;
use tui_textarea::{Input, Key};

/// A nonce that appears only in what the fast tier returns.
pub(crate) const TRIE_NONCE: &str = "trie-9f2a";

/// A nonce that appears only in what the slow tier returns.
pub(crate) const SERVER_NONCE: &str = "server-4c7e";

/// A trie that counts how many times it was consulted.
///
/// The count lives here rather than in the composer, so "the composer says it
/// consulted the trie twice" and "the trie was consulted twice" stay two
/// readings of two mechanisms.
#[derive(Debug)]
pub(crate) struct CountingTrie {
    entries: Vec<Entry>,
    calls: Cell<usize>,
}

impl CountingTrie {
    /// One page and one atom, so a picker that offered everything is visible.
    pub(crate) fn staged() -> Self {
        Self {
            entries: vec![
                Entry::new(
                    "zaru",
                    "architecture/bounded-contexts",
                    format!("édge·{TRIE_NONCE} ✦"),
                    EntryKind::Page,
                ),
                Entry::new(
                    "zaru",
                    "atoms/membrane",
                    format!("atóm·{TRIE_NONCE} ✦"),
                    EntryKind::Atom,
                ),
            ],
            calls: Cell::new(0),
        }
    }

    /// How many times [`Entries::matches`] was called.
    pub(crate) fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl Entries for CountingTrie {
    fn matches(&self, _prefix: &str, limit: usize) -> Vec<Entry> {
        self.calls.set(self.calls.get() + 1);
        self.entries.iter().take(limit).cloned().collect()
    }
}

/// What the slow tier returns: one entity the trie also holds, carrying a
/// different title, and one the trie does not.
pub(crate) fn server_results() -> Vec<Entry> {
    vec![
        Entry::new(
            "zaru",
            "atoms/membrane",
            format!("atóm·{SERVER_NONCE} ✦"),
            EntryKind::Atom,
        ),
        Entry::new(
            "zaru",
            "operations/testing",
            format!("tésting·{SERVER_NONCE} ✦"),
            EntryKind::Page,
        ),
    ]
}

/// Type `text` into `composer`, every keystroke at `now`.
pub(crate) fn typing(composer: &mut Composer, text: &str, now: Duration, entries: &dyn Entries) {
    for ch in text.chars() {
        composer.key(
            Input {
                key: Key::Char(ch),
                ctrl: false,
                alt: false,
                shift: false,
            },
            now,
            entries,
        );
    }
}

/// A trie that returns exactly `count` entries, so a check can stage a strip
/// of a chosen height without changing the text in the input.
#[derive(Debug)]
pub(crate) struct TrieOf {
    entries: Vec<Entry>,
}

impl TrieOf {
    pub(crate) fn new(count: usize) -> Self {
        Self {
            entries: (0..count)
                .map(|i| {
                    Entry::new(
                        "zaru",
                        format!("architecture/páge-{i}"),
                        format!("títle-{i}·{TRIE_NONCE} ✦"),
                        EntryKind::Page,
                    )
                })
                .collect(),
        }
    }
}

impl Entries for TrieOf {
    fn matches(&self, _prefix: &str, limit: usize) -> Vec<Entry> {
        self.entries.iter().take(limit).cloned().collect()
    }
}

/// Paint a composer and read the cells back out of the buffer.
///
/// The reader on this side is ratatui's own `TestBackend` buffer and not
/// anything the composer wrote — which is what stops a frame check comparing
/// the composer's formatter with itself. Every expected value in a check that
/// uses this is a literal written in the check.
pub(crate) fn painted(composer: &Composer, width: u16, height: u16) -> (Vec<String>, Position) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| composer.render(frame, frame.area()))
        .expect("draw");
    let cursor = terminal
        .backend_mut()
        .get_cursor_position()
        .expect("the test backend records the cursor");
    let buffer = terminal.backend().buffer();
    let rows = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    (rows, cursor)
}
