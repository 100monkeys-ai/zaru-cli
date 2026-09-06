// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The adapter between `zaru-notes`' trie and the composer's port.
//!
//! [ADR-0005]'s own Status tracking says where this has to live: "[ADR-0003] D8
//! permits `zaru-tui` exactly one sibling dependency, `zaru-core` … The
//! consumer therefore declares the port, the owner implements it later, and
//! `zaru-cli` — the composition root, which already depends on both — writes
//! the adapter. No edge in the D8 table moves." This is that adapter, and it is
//! the fourth port in this module crossing exactly that way, after
//! [`CommandVocabulary`], [`TranscriptSource`] and [`Confirm`].
//!
//! # One trie per workspace, and the reason is a truncation bug avoided
//!
//! [ADR-0005] D3 has the trie cover "every reachable workspace"; D6 admits only
//! the `workspace` and `all_mine` scopes to the strip; [ADR-0006] D2 makes the
//! attached workspace move only by user action. The obvious shape — one trie
//! over everything, filtered to the attached workspace at query time — is
//! **wrong**, and not by taste: the trie returns its best `limit` before the
//! filter runs, so a prefix whose best eight are another workspace's would show
//! a user an empty strip while their own workspace held eight matches. The
//! filter would have removed rows the strip had no way to replace.
//!
//! So the corpus is kept per workspace and the attached one is consulted.
//! Nothing is filtered, nothing is truncated twice, and the shape `all_mine`
//! will need — consult them all and merge — is the one already here.
//!
//! # Only one scope can reach this today, and that is said rather than assumed
//!
//! `Composer::scope` starts at `Scope::Workspace` and `set_scope` is reachable
//! only from a user action, which [ADR-0006] D5 spells `/notes workspace
//! <slug>` — **a verb no surface implements**, out of a session or in one. So
//! `Scope::AllMine` cannot be reached from the binary at all, and an adapter
//! branching on the scope would carry a branch nothing can take. What is built
//! is the attached workspace's, which is what D5 calls the default and what
//! every session has.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [`CommandVocabulary`]: zaru_tui::shell::CommandVocabulary
//! [`Confirm`]: crate::tools::port::Confirm
//! [`TranscriptSource`]: zaru_tui::shell::TranscriptSource

use std::collections::BTreeMap;
use zaru_notes::trie::{CachedEntry, EntryKind as CachedKind, Trie};
use zaru_tui::composer::{Entries, Entry, EntryKind, MATCH_LIMIT};

/// What the strip says when the fast tier has nothing to search.
///
/// # One true fact, and now a command
///
/// **Drafted 2026-09-05 under a delegated coordinator ruling, open to Jeshua's
/// veto**, and the words are user-facing prose rather than an implementation
/// detail. It said two things, and **the first stopped being true later the
/// same day**: that "nothing here can open a transport to Nuclear Notes —
/// `zaru_notes::session::Endpoint` has no implementation in any product tree".
/// `zaru_notes::session::HttpEndpoint` is that implementation. What remains
/// true is the second half alone: nothing is cached, because no stored token
/// is read by the composer and nothing populates a trie.
///
/// **It named no command on purpose and now names one**, because the reason it
/// named none has gone. That reason was that `notes tokens add` "needs a server
/// to authenticate against" and did not exist, so a line pointing at it would
/// be [ADR-0016] D2's "an error message whose reader cannot act". It exists,
/// it is in `--help`, and a reader who runs it stores a credential — so the
/// line names it, and the rule is the same rule: a remedy names something the
/// binary runs.
///
/// **It still promises nothing about what happens next.** Storing a token is
/// not the same as the strip filling, because nothing yet reads a stored token
/// into a session — so the line says what to run and does not say the search
/// will then work.
///
/// It is one line and it is short, because the composer's own frame is as
/// narrow as forty columns and a longer sentence is clipped rather than
/// wrapped.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const NOTHING_CACHED: &str = "nothing cached to search · add a token: zaru notes tokens add";

/// The composer's fast tier, over the trie `zaru-notes` owns.
#[derive(Debug)]
pub struct NotesTrie {
    /// One trie per workspace the harness has cached. See the module note.
    per_workspace: BTreeMap<String, Trie>,
    /// The workspace [ADR-0006] D2 calls the attached one.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    attached: String,
}

impl NotesTrie {
    /// A fast tier over what has been cached, attached to one workspace.
    ///
    /// Entries are grouped by their own workspace slug, so a corpus spanning
    /// every reachable workspace is what this takes and one workspace's is what
    /// it serves.
    #[must_use]
    pub fn attached_to(entries: Vec<CachedEntry>, attached: impl Into<String>) -> Self {
        let mut grouped: BTreeMap<String, Vec<CachedEntry>> = BTreeMap::new();
        for entry in entries {
            grouped
                .entry(entry.workspace.clone())
                .or_default()
                .push(entry);
        }
        Self {
            per_workspace: grouped
                .into_iter()
                .map(|(workspace, entries)| (workspace, Trie::of(entries, MATCH_LIMIT)))
                .collect(),
            attached: attached.into(),
        }
    }

    /// A fast tier with nothing in it, which is what this build can offer.
    ///
    /// The workspace is named anyway, so that the day a transport lands the
    /// only change is where the entries come from.
    #[must_use]
    pub fn nothing_cached(attached: impl Into<String>) -> Self {
        Self::attached_to(Vec::new(), attached)
    }

    /// What the strip should say when there is nothing to search, if anything.
    ///
    /// `None` once the attached workspace holds an entry, so the line stops
    /// being shown without anybody removing it — which is what makes it a
    /// statement about the state rather than a note about the build.
    #[must_use]
    pub fn absence(&self) -> Option<String> {
        self.attached_trie()
            .is_none_or(Trie::is_empty)
            .then(|| NOTHING_CACHED.to_owned())
    }

    /// How many entities the attached workspace holds.
    #[must_use]
    pub fn cached(&self) -> usize {
        self.attached_trie().map_or(0, Trie::len)
    }

    fn attached_trie(&self) -> Option<&Trie> {
        self.per_workspace.get(&self.attached)
    }
}

impl Entries for NotesTrie {
    fn matches(&self, prefix: &str, limit: usize) -> Vec<Entry> {
        self.attached_trie()
            .map(|trie| {
                trie.matches(prefix, limit)
                    .into_iter()
                    .map(|entry| {
                        Entry::new(
                            &entry.workspace,
                            &entry.path,
                            &entry.title,
                            match entry.kind {
                                CachedKind::Page => EntryKind::Page,
                                CachedKind::Atom => EntryKind::Atom,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
