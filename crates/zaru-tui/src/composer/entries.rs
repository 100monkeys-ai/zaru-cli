// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The fast tier: what the local trie returns, and the port it returns it
//! through.
//!
//! ADR-0005 D3's tier one is "a prefix trie over page paths, titles, tags, and
//! atom names for every reachable workspace, built at session start and
//! refreshed on write. Zero network, instant, works offline." The trie itself
//! belongs to `zaru-notes` — [Bounded Contexts] gives that crate "Nuclear
//! Notes client, the local trie, embedding, workspace pointer".
//!
//! # Why the port is declared here and not there
//!
//! ADR-0003 D8 permits `zaru-tui` exactly one sibling dependency, `zaru-core`,
//! and `scripts/check-crate-boundaries.py` fails on any other edge. So a trait
//! declared in `zaru-notes` could not be named from this crate at all. The
//! consumer declares the port, the owner implements it, and `zaru-cli` — the
//! composition root, which depends on both — writes the adapter. No edge in
//! the D8 table moves, and `zaru-core` does not become a shared-types crate,
//! which [ADR-0016]'s Status tracking records as deliberately avoided.
//!
//! **Nothing in this crate's product tree implements [`Entries`]**, exactly as
//! nothing in `zaru-core`'s implements one of the loop's ports.
//!
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

/// What kind of thing an entry names.
///
/// Two kinds and not three. ADR-0005 D4 has `[[` pick "pages and atoms" and
/// `@` pick "atoms and media", but ADR-0006 D4 scopes the composer's token to
/// `pages.{list,read}`, `atoms.{list,read}`, `search.global`,
/// `kg.{related,list_cross_links}` and `discovery.entities` — no `media.*`
/// tool at all. The media half of `@` is unreachable with the credential as
/// scoped, so it is not modelled here. The two records disagree and that
/// disagreement is recorded on ADR-0005 rather than settled in this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A page.
    Page,
    /// An atom.
    Atom,
}

/// One thing the strip can show, as the trie or the server returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The workspace slug the entity lives in.
    pub workspace: String,
    /// The entity's path inside that workspace.
    pub path: String,
    /// The entity's title, which is what the strip renders.
    pub title: String,
    /// Page or atom.
    pub kind: EntryKind,
}

impl Entry {
    /// Take an entry.
    #[must_use]
    pub fn new(
        workspace: impl Into<String>,
        path: impl Into<String>,
        title: impl Into<String>,
        kind: EntryKind,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            path: path.into(),
            title: title.into(),
            kind,
        }
    }

    /// What makes two entries the same entity.
    ///
    /// The workspace is part of the identity and not decoration. ADR-0006's
    /// Context records, measured against the live substrate, that an
    /// identifier resolves only within its own workspace — including a UUID.
    /// Two workspaces may each hold `architecture/bounded-contexts`, and they
    /// are different pages; a merge keyed on the path alone would collapse
    /// them into one and show the user something they did not ask for.
    #[must_use]
    pub fn identity(&self) -> (&str, &str) {
        (&self.workspace, &self.path)
    }
}

/// The fast tier. Local, synchronous, and unable to reach a network.
///
/// Synchronous on purpose: D3 calls this tier "instant", and a port that
/// returns a future would put an asynchronous runtime under every check on the
/// composer and make each rendered frame depend on a poll order.
pub trait Entries {
    /// Every entry whose prefix matches, best first, at most `limit` of them.
    fn matches(&self, prefix: &str, limit: usize) -> Vec<Entry>;
}
