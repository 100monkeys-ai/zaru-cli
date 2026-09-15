// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The hint strip's content: one variant per row of ADR-0005 D1's table.
//!
//! D1 makes the strip's content "a pure function of composer state", which is
//! why this is a value the composer derives rather than a widget that reaches
//! back into it. Two modes and never two strips: the prompt is either empty or
//! it is not, and the states never contend.

use crate::composer::entries::{Entry, EntryKind};
use crate::composer::search::SearchState;
use crate::shell::port::{Extension, Namespace};

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
/// One variant per row of D1's table. A `Staged` variant is deliberately
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
    /// The line is a command, so the strip shows the command namespaces.
    /// **Not one of D1's rows.**
    ///
    /// # Which record supplies this, and why D1's table is unchanged
    ///
    /// [ADR-0015] D2 gives a session two grammars over one vocabulary, and
    /// inside a session "a leading `/` says command and everything else is the
    /// task". ADR-0005 D1's table is keyed on prompt emptiness and predates
    /// that grammar reaching the composer, so it has no row for a line that is
    /// not a search at all — and inventing one there would be reading a
    /// decision into a record that does not carry it.
    ///
    /// So the row is **ADR-0015 D2's**, and the mode stays
    /// [`StripMode::Typing`], because the prompt is not empty and D1's two
    /// modes are keyed on exactly that; a command line reported as the empty
    /// mode would say the user had typed nothing.
    ///
    /// # What it renders, since 2026-09-15
    ///
    /// It rendered **nothing** until then, which is what the look-and-feel
    /// survey's row 6 is about: a person typing `/` met six blank rows and no
    /// way to find out what the session could do. [ADR-0005]'s amendments page
    /// narrows the 2026-09-05 Update's last clause alone — the row D2 supplies
    /// carries a **second corpus**, shown in place of the hint strip while the
    /// line begins with `/` and gone the moment it does not. The hint tiers
    /// themselves stay closed to `/`: no keystroke of a command line reaches
    /// the trie and none emits a search request.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Command {
        /// The namespaces whose slash spelling begins with the word being
        /// typed, in D2's own table order, **whole**. Empty when the word
        /// matches none, which is what a miss looks like everywhere else in
        /// this composer, and empty once the line carries a space, because the
        /// namespace has then been named.
        ///
        /// It was "capped at the rows the strip can paint" until 2026-09-15,
        /// when the cap moved to
        /// [`render::fitted`](crate::composer::render) so that every corpus
        /// gets it rather than this one alone. The painted rows are unchanged.
        matches: Vec<Namespace>,
        /// The **second corpus**, since 2026-09-15: [ADR-0015] D1's commands
        /// this session has loaded, narrowed by the same prefix and shown
        /// after the namespaces.
        ///
        /// **Empty everywhere it was empty before**, which is what keeps every
        /// existing assertion about this variant byte-identical — a project
        /// whose commands nobody has admitted, and a session with no command
        /// files at all, both show exactly the rows they showed on
        /// 2026-09-14.
        ///
        /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
        extensions: Vec<Extension>,
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

impl PickerKind {
    /// Whether this picker offers entries of that kind.
    #[must_use]
    pub const fn admits(self, kind: EntryKind) -> bool {
        match self {
            Self::PagesAndAtoms => matches!(kind, EntryKind::Page | EntryKind::Atom),
            Self::Atoms => matches!(kind, EntryKind::Atom),
        }
    }
}

impl StripContent {
    /// Which of D1's two modes this content belongs to.
    #[must_use]
    pub const fn mode(&self) -> StripMode {
        match self {
            Self::Deposits { .. } | Self::Tip { .. } | Self::Collapsed => StripMode::Empty,
            Self::Trie { .. }
            | Self::Merged { .. }
            | Self::Picker { .. }
            | Self::Command { .. } => StripMode::Typing,
        }
    }
}
