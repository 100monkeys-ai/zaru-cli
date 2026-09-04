// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The hint strip's content: one variant per row of ADR-0005 D1's table.
//!
//! D1 makes the strip's content "a pure function of composer state", which is
//! why this is a value the composer derives rather than a widget that reaches
//! back into it. Two modes and never two strips: the prompt is either empty or
//! it is not, and the states never contend.

use crate::composer::entries::Entry;
use crate::composer::search::SearchState;

/// Which of D1's two modes the strip is in.
///
/// Named from the record's own first column. The transition between them is
/// prompt emptiness and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripMode {
    /// The prompt is empty.
    Empty,
    /// The user is typing.
    Typing,
}

/// Which picker the explicit grammar opened.
///
/// ADR-0005 D4 reuses Nuclear Notes' editor grammar unchanged, so muscle
/// memory transfers between writing and building. `#` is not here: D4 has it
/// scope the live search and attach nothing, so it opens no picker and changes
/// no mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerKind {
    /// `[[` — pages and atoms, attaching as a citation.
    PagesAndAtoms,
    /// `@` — atoms, attaching as a transclusion.
    ///
    /// D4 says "atoms and media"; ADR-0006 D4's composer-token scope reaches
    /// no `media.*` tool, so the media half is not built. Recorded on
    /// ADR-0005 rather than settled here.
    Atoms,
}

/// What the strip is showing.
///
/// One variant per row of D1's table. [`StripContent::Staged`] is deliberately
/// absent: D5's running token cost needs a tokeniser and a counting rule no
/// record names, so attachments are out of scope and the grammar in
/// [`PickerKind`] detects and filters while attaching nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripContent {
    /// Empty, deposits pending. D1 row 1.
    ///
    /// The count is handed to the composer. Its producer is ADR-0002 D3's
    /// deposit channel and `/inbox`, neither of which exists.
    Deposits {
        /// How many deposits are waiting.
        count: u32,
    },
    /// Empty, a standing tip is eligible. D1 row 2.
    ///
    /// The text is handed to the composer. Its producer is ADR-0002 D8, whose
    /// one-per-session budget and three-displays-without-action suppression
    /// need persistence that does not exist.
    Tip {
        /// The tip, one line.
        text: String,
    },
    /// Empty, neither. D1 row 3 — the strip collapses to zero height.
    Collapsed,
    /// Typing, under three characters: local trie matches only. D1 row 4.
    Trie {
        /// The trie's matches, in the trie's order.
        matches: Vec<Entry>,
    },
    /// Typing, three characters or more: trie matches, then server results as
    /// they arrive. D1 row 5.
    Merged {
        /// The merge, trie group first.
        entries: Vec<Entry>,
        /// Where the slow tier has got to.
        search: SearchState,
    },
    /// `[[` or `@` entered: the explicit picker, filtered by what follows.
    /// D1 row 6.
    Picker {
        /// Which grammar opened it.
        kind: PickerKind,
        /// What the user has typed after the sigil.
        filter: String,
        /// The trie's matches for that filter, of the kinds this picker offers.
        matches: Vec<Entry>,
    },
}

impl StripContent {
    /// Which of D1's two modes this content belongs to.
    #[must_use]
    pub const fn mode(&self) -> StripMode {
        match self {
            Self::Deposits { .. } | Self::Tip { .. } | Self::Collapsed => StripMode::Empty,
            Self::Trie { .. } | Self::Merged { .. } | Self::Picker { .. } => StripMode::Typing,
        }
    }
}
