// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0005] D3's fast tier: a prefix trie over what this crate has cached.
//!
//! D3: "**Tier one — local trie.** A prefix trie over page paths, titles, tags,
//! and atom names for every reachable workspace, built at session start and
//! refreshed on write. Zero network, instant, works offline."
//!
//! [Bounded Contexts] gives this crate "Nuclear Notes client, the local trie,
//! embedding, workspace pointer", which is why the structure is here and not in
//! the crate that renders it. `zaru-tui` declares the port
//! (`composer::Entries`), this crate owns the structure, and `zaru-cli` — the
//! composition root, which depends on both — writes the adapter. No [ADR-0003]
//! D8 edge moves.
//!
//! # Nothing here can reach a network, and that is structural
//!
//! This module names `std` and nothing else. It holds no session, no endpoint
//! and no bearer; it cannot be given one, because there is no field to put one
//! in. D3's "zero network, instant, works offline" is therefore a property of
//! what exists rather than a claim about a code path — the same argument the
//! boundary gate makes one crate over about `zaru-tui`'s dependency closure.
//!
//! # Three key kinds, and the fourth is named rather than invented
//!
//! D3 names four: page paths, titles, tags, and atom names. Three are indexed:
//! the **path**, the **title**, and — for an atom alone — its **name**, which
//! is the last segment of its path and is the thing D3 lists separately from
//! the path because it is not reachable as a prefix of one.
//!
//! **Tags are not indexed, and that is deliberate.** No record gives an entity
//! a tag: [ADR-0005] D1's table has no row a tag could be rendered in, that
//! record's own Update says so in as many words — "the half that is not built
//! is a tag picker … D1 gives them no row to be rendered in and one was not
//! invented" — and [`CachedEntry`] has no field for one. Indexing a key nothing
//! can carry would be a mechanism with no population, which is worse than an
//! absence because it reads as coverage.
//!
//! # "Best first" is defined here because the record did not define it
//!
//! The port this feeds says "best first" and [ADR-0005] D3 states no ordering
//! at all. The order is **the shortest matching key first, then the key
//! lexicographically, then the workspace slug, then the path** — accepted
//! 2026-09-05 under directive 20 and recorded as an accepted Update on that
//! record, open to Jeshua's veto.
//!
//! The reason for the first term is the only one that is about the user: among
//! the entries a prefix reaches, the one whose key is *shortest* is the one the
//! typed characters come closest to being the whole of, so it is the most
//! specific completion of what was typed. The remaining three terms carry no
//! judgement — they exist so that two entries never trade places between two
//! builds of the same corpus, and the last two are exactly [ADR-0006] D6's
//! identity pair.
//!
//! # The bound this buys, and what it costs
//!
//! Each node carries the already-ordered indices of the best [`Trie::retained`]
//! entries under it, computed once at build time, so a query is a descent and a
//! copy: **`O(|prefix| log A + limit)`**, where `A` is the branching at a node.
//! The obvious alternative — descend, walk the subtree, sort — is `O(N log N)`
//! for a one-character prefix, which is not "instant" as an asymptote however
//! small `N` happens to be today.
//!
//! What it costs is `retained` indices per node. `bounds_hold_at_the_measured_corpus_size`
//! stakes both numbers against a corpus the size of the one this harness can
//! actually reach, measured rather than assumed: the Zaru workspace held 71
//! pages and no atoms on 2026-09-05, and the token that read it reaches six
//! workspaces.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

use std::collections::BTreeMap;

/// What kind of entity an entry names.
///
/// Two kinds and not three, for the reason `zaru-tui`'s own mirror of this enum
/// gives: [ADR-0005] D4 has `@` pick "atoms and media", and [ADR-0006] D4
/// scopes the composer's token to a set carrying no `media.*` tool at all, so
/// there is nothing a media entry could have been listed by.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A page.
    Page,
    /// An atom.
    Atom,
}

/// One entity this crate has cached, as the trie holds it.
///
/// # Deliberately this crate's own type
///
/// `zaru-tui` has a structurally identical `Entry` and this is not it.
/// [ADR-0003] D8 gives this crate no sibling dependency at all, so the type the
/// composer renders is unreachable from here — the same constraint that makes
/// [`Bearer`](crate::session::Bearer) this crate's own rather than `zaru-cli`'s
/// `Secret`, and that gives `zaru-core`'s context module its own `ItemId`
/// beside [`Attachment`](crate::session::Attachment). `zaru-cli` is the
/// composition root and converts.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedEntry {
    /// The workspace slug the entity lives in.
    ///
    /// Part of the identity and not decoration. [ADR-0006]'s Context records,
    /// measured against the live substrate, that an identifier resolves only
    /// within its own workspace — including a UUID — so two workspaces holding
    /// one path hold two different entities.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub workspace: String,
    /// The entity's path inside that workspace.
    pub path: String,
    /// The entity's title.
    pub title: String,
    /// Page or atom.
    pub kind: EntryKind,
}

impl CachedEntry {
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

    /// What makes two entries the same entity: the workspace and the path.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    #[must_use]
    pub fn identity(&self) -> (&str, &str) {
        (&self.workspace, &self.path)
    }

    /// The keys this entry is reachable by, in no particular order.
    ///
    /// The path and the title always; an atom's name additionally, which is the
    /// last segment of its path. See the module documentation for why tags are
    /// not among them.
    fn keys(&self) -> Vec<&str> {
        let mut keys = vec![self.path.as_str(), self.title.as_str()];
        if self.kind == EntryKind::Atom {
            let name = self.path.rsplit('/').next().unwrap_or(&self.path);
            if name != self.path {
                keys.push(name);
            }
        }
        keys
    }
}

/// One node: where to go next, and the best entries anywhere below.
#[derive(Debug, Default)]
struct Node {
    children: BTreeMap<char, usize>,
    /// Indices into [`Trie::entries`], already in the order the module
    /// documentation defines, capped at [`Trie::retained`].
    best: Vec<usize>,
}

/// A prefix trie over cached entities.
///
/// Built once through [`Trie::of`] and read many times through
/// [`Trie::matches`]. There is no mutation surface: D3 has the trie "built at
/// session start and refreshed on write", and a refresh is a rebuild rather
/// than an edit, so that a half-updated trie is not a state this program can
/// hold.
#[derive(Debug)]
pub struct Trie {
    nodes: Vec<Node>,
    entries: Vec<CachedEntry>,
    retained: usize,
}

impl Trie {
    /// Build a trie holding at most `retained` matches per prefix.
    ///
    /// `retained` is a **rendering** budget rather than a retrieval one — it is
    /// how many rows the strip can hold — and it is the caller's, because the
    /// number [ADR-0005] does not name lives beside the surface that renders
    /// it. A `retained` of zero builds a trie that matches nothing and is
    /// permitted: it is what a caller asking for no rows means.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    #[must_use]
    pub fn of(entries: Vec<CachedEntry>, retained: usize) -> Self {
        let mut trie = Self {
            nodes: vec![Node::default()],
            entries,
            retained,
        };
        trie.build();
        trie
    }

    /// Every entry whose prefix matches, best first, at most `limit` of them.
    ///
    /// The comparison is case-insensitive: both the prefix and every key are
    /// folded with [`str::to_lowercase`], so a cortex full of capitalised
    /// titles answers a lower-case prefix. What comes back is the entry as it
    /// was cached, never the folded form — the strip renders the user's own
    /// words.
    ///
    /// **At most `min(limit, self.retained())`.** A caller asking for more rows
    /// than the trie was built to keep gets what it kept, which is why
    /// [`Self::retained`] is readable and why the composition root builds at
    /// the strip's own budget.
    #[must_use]
    pub fn matches(&self, prefix: &str, limit: usize) -> Vec<&CachedEntry> {
        let Some(node) = self.walk(&prefix.to_lowercase()) else {
            return Vec::new();
        };
        self.nodes[node]
            .best
            .iter()
            .take(limit)
            .map(|index| &self.entries[*index])
            .collect()
    }

    /// How many entities the trie holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the trie holds no entity at all.
    ///
    /// **This is the question the honest empty state is asked**, and it is a
    /// different question from "did this prefix match nothing". A user with an
    /// empty cache and a user who typed a prefix nothing starts with are owed
    /// different sentences, and a surface that could not tell them apart would
    /// have to guess.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many matches per prefix this trie was built to keep.
    #[must_use]
    pub const fn retained(&self) -> usize {
        self.retained
    }

    /// How many nodes the trie occupies. The memory bound, made readable.
    ///
    /// Public so that a check can assert the bound rather than a comment
    /// asserting it.
    #[must_use]
    pub fn nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Descend to the node `prefix` names, if there is one.
    fn walk(&self, prefix: &str) -> Option<usize> {
        let mut at = 0;
        for character in prefix.chars() {
            at = *self.nodes[at].children.get(&character)?;
        }
        Some(at)
    }

    /// Fill every node's `best` list, in one pass over globally sorted keys.
    ///
    /// # Why sorting first is what makes this correct
    ///
    /// A node's `best` must be the top `retained` entries under it. Inserting
    /// the (key, entry) pairs in the *global* order the module documentation
    /// defines means every node's list is filled best-first as it is built, so
    /// the first `retained` distinct entries a node ever sees are exactly its
    /// top `retained`. Sorting afterwards, per node, would be the same answer
    /// computed once per node instead of once.
    fn build(&mut self) {
        let mut pairs: Vec<(String, usize)> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            for key in entry.keys() {
                pairs.push((key.to_lowercase(), index));
            }
        }
        pairs.sort_by(|(left, left_index), (right, right_index)| {
            let left_entry = &self.entries[*left_index];
            let right_entry = &self.entries[*right_index];
            left.chars()
                .count()
                .cmp(&right.chars().count())
                .then_with(|| left.cmp(right))
                .then_with(|| left_entry.workspace.cmp(&right_entry.workspace))
                .then_with(|| left_entry.path.cmp(&right_entry.path))
        });

        for (key, index) in pairs {
            let mut at = 0;
            self.deposit(at, index);
            for character in key.chars() {
                at = match self.nodes[at].children.get(&character) {
                    Some(next) => *next,
                    None => {
                        let next = self.nodes.len();
                        self.nodes.push(Node::default());
                        self.nodes[at].children.insert(character, next);
                        next
                    }
                };
                self.deposit(at, index);
            }
        }
    }

    /// Add `index` to a node's list, unless it is full or already there.
    ///
    /// The containment scan is linear in `retained`, which is a strip's height
    /// rather than a corpus size. An entry reachable by two of its own keys
    /// appears once, which is why the scan is here at all.
    fn deposit(&mut self, node: usize, index: usize) {
        let retained = self.retained;
        let best = &mut self.nodes[node].best;
        if best.len() < retained && !best.contains(&index) {
            best.push(index);
        }
    }
}

#[cfg(test)]
mod tests;
