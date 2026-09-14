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
/// **The sentence that said "nothing yet reads a stored token into a session"
/// stopped being true on 2026-09-14 and is corrected rather than left.** A
/// session now reads one and builds a corpus from it. What this line means is
/// therefore narrower and exact: **there is no token at all**. The states that
/// used to fall under it — a population still running, and a token whose
/// instance would not answer — have their own lines below, because telling a
/// person "add a token" when they have added one is an instruction they cannot
/// act on.
///
/// It is one line and it is short, because the composer's own frame is as
/// narrow as forty columns and a longer sentence is clipped rather than
/// wrapped.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const NOTHING_CACHED: &str = "nothing cached to search · add a token: zaru notes tokens add";

/// What the strip says while the corpus is still being fetched.
///
/// **Authored 2026-09-14 under a delegated coordinator ruling, open to
/// Jeshua's veto**, and named on [ADR-0005]'s amendments page with the other
/// two. A population was measured against the live server at one to two
/// seconds, and the shell deliberately opens without waiting for it — so
/// there is a real interval in which a person types and the strip has nothing,
/// and [Operating Principles]' "legibility beats smoothness" is the whole
/// reason it says so rather than staying blank.
///
/// It is present tense and promises nothing about the outcome, because the
/// outcome is not known yet: a token whose instance refuses ends at
/// [`UNREACHABLE`] and not here. One line, short, for the reason
/// [`NOTHING_CACHED`] is short — the composer's frame is as narrow as forty
/// columns and a longer sentence is clipped rather than wrapped.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
pub const LOOKING: &str = "looking in your notes…";

/// What the strip says when the token's instance could not be reached.
///
/// **Authored 2026-09-14 under the same ruling and equally open to veto.**
/// [ADR-0005] D8 is "degrade honestly", and the dishonest option here is the
/// tempting one: a strip that silently stayed empty would make an unreachable
/// cortex indistinguishable from an empty one, which is exactly the failure
/// `zaru_notes::session::found` exists because of.
///
/// **The server's own sentence is appended and never paraphrased.** That is
/// the rule `zaru_notes`' `innermost` already follows — it walks to the end of
/// the error chain precisely so a person reads `Auth required` rather than
/// three generic parameters — and it is what [ADR-0006] D7 requires of a
/// refusal the harness cannot attribute: the server does not reveal which gate
/// tripped, so the harness reports what it was told rather than inventing a
/// diagnosis.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub const UNREACHABLE: &str = "notes unreachable";

/// How far the fast tier has got with the corpus it was asked for.
///
/// **Four states and not a `bool` with a `String` beside it**, because three
/// of the four need different words in front of a person and a pair of fields
/// can hold combinations none of them describe — "reached, and also carrying a
/// refusal" being the one that would render as both.
#[derive(Debug)]
enum Population {
    /// No token was selected, so nothing was asked for. Today's line.
    NoToken,
    /// A corpus is being fetched. See [`LOOKING`].
    InFlight,
    /// The listings came back. What they held is in `per_workspace`, which
    /// may legitimately be empty — a cortex can hold nothing.
    Reached,
    /// The instance refused or could not be reached, carrying **its own**
    /// sentence. See [`UNREACHABLE`].
    Unreachable(String),
}

/// The composer's fast tier, over the trie `zaru-notes` owns.
/// What a [`NotesTrie`] holds, behind one lock.
///
/// The corpus and the state it is in are locked **together** rather than
/// separately, so a reader cannot observe "reached" beside an empty map that
/// is about to be filled a microsecond later. One lock, one consistent
/// answer to both questions.
#[derive(Debug)]
struct Corpus {
    /// One trie per workspace the harness has cached. See the module note.
    per_workspace: BTreeMap<String, Trie>,
    /// How far the population has got.
    population: Population,
}

/// The composer's fast tier, over the trie `zaru-notes` owns.
///
/// # Why this is shared and mutable where it used to be neither
///
/// Until 2026-09-14 this was a value built once and handed to the pump, which
/// was right while nothing could fill it. A population against a real server
/// was then measured at one to two seconds — an attach plus two listings — and
/// blocking the shell's first frame on that is one to two seconds in which a
/// person sees nothing at all. So the shell opens first and the corpus arrives
/// into it.
///
/// **That needed no port change and no dependency.**
/// [`Entries::matches`] already takes
/// `&self`, so the corpus sits behind a [`std::sync::RwLock`] and the task
/// that fills it holds an [`Arc`](std::sync::Arc) of the same value the pump
/// is reading. Reads are the common case by a wide margin — every keystroke —
/// and there is exactly one write, which is what an `RwLock` is for.
///
/// A poisoned lock is treated as a lock: every access below recovers the guard
/// rather than panicking. A panic in the one task that writes must not take
/// the composer's strip down with it, and the worst a recovered guard can hold
/// here is a corpus that is half built — which renders as fewer matches, not
/// as a wrong one.
#[derive(Debug)]
pub struct NotesTrie {
    /// The corpus and its state, locked together. See [`Corpus`].
    corpus: std::sync::RwLock<Corpus>,
    /// The workspace [ADR-0006] D2 calls the attached one.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    attached: String,
}

impl NotesTrie {
    /// Group a corpus by its own workspace slug.
    ///
    /// Entries arrive carrying the workspace they came from, so a corpus
    /// spanning several is what this takes and one workspace's is what the
    /// strip is served. See the module note for why the grouping is not a
    /// filter at query time.
    fn grouped(entries: Vec<CachedEntry>) -> BTreeMap<String, Trie> {
        let mut grouped: BTreeMap<String, Vec<CachedEntry>> = BTreeMap::new();
        for entry in entries {
            grouped
                .entry(entry.workspace.clone())
                .or_default()
                .push(entry);
        }
        grouped
            .into_iter()
            .map(|(workspace, entries)| (workspace, Trie::of(entries, MATCH_LIMIT)))
            .collect()
    }

    fn with(
        per_workspace: BTreeMap<String, Trie>,
        population: Population,
        attached: String,
    ) -> Self {
        Self {
            corpus: std::sync::RwLock::new(Corpus {
                per_workspace,
                population,
            }),
            attached,
        }
    }

    /// A fast tier over a corpus that is already in hand, attached to one
    /// workspace.
    #[must_use]
    pub fn attached_to(entries: Vec<CachedEntry>, attached: impl Into<String>) -> Self {
        Self::with(Self::grouped(entries), Population::Reached, attached.into())
    }

    /// A fast tier with nothing in it, because there is no token to fill it.
    ///
    /// This is the **no-token** state and not a general empty one: its
    /// [`absence`](Self::absence) is [`NOTHING_CACHED`], which tells a person
    /// to add a token. A session that has one and is still fetching gets
    /// [`Self::awaiting`] instead, because telling somebody to add the token
    /// they just added is an instruction they cannot act on.
    #[must_use]
    pub fn nothing_cached(attached: impl Into<String>) -> Self {
        Self::with(BTreeMap::new(), Population::NoToken, attached.into())
    }

    /// A fast tier whose corpus is on its way.
    ///
    /// The shell opens over one of these and the population fills it. See the
    /// type's own documentation for why the shell does not wait.
    #[must_use]
    pub fn awaiting(attached: impl Into<String>) -> Self {
        Self::with(BTreeMap::new(), Population::InFlight, attached.into())
    }

    /// Take the corpus a population fetched.
    ///
    /// Called once, from the task the shell spawned. An empty corpus is a
    /// real answer — a cortex may hold nothing — so this moves to
    /// the reached state whatever came back, and the strip then says
    /// nothing rather than saying it is still looking for ever.
    pub fn reached(&self, entries: Vec<CachedEntry>) {
        let mut corpus = self.write();
        corpus.per_workspace = Self::grouped(entries);
        corpus.population = Population::Reached;
    }

    /// Record that the instance would not answer, in **its own words**.
    ///
    /// `detail` is the sentence the client produced and must not be
    /// paraphrased on the way in; see [`UNREACHABLE`].
    pub fn unreachable(&self, detail: impl Into<String>) {
        self.write().population = Population::Unreachable(detail.into());
    }

    /// What the strip should say when there is nothing to search, if anything.
    ///
    /// `None` once the attached workspace holds an entry, so the line stops
    /// being shown without anybody removing it — which is what makes it a
    /// statement about the state rather than a note about the build.
    ///
    /// **A reached-but-empty corpus says nothing at all.** That is deliberate
    /// and it is the one case worth arguing: a cortex the harness reached and
    /// which holds nothing under the attached workspace is not a failure, and
    /// a line claiming otherwise would be the harness reporting the user's own
    /// empty workspace as a fault.
    #[must_use]
    pub fn absence(&self) -> Option<String> {
        let corpus = self.read();
        if corpus
            .per_workspace
            .get(&self.attached)
            .is_some_and(|trie| !trie.is_empty())
        {
            return None;
        }
        match &corpus.population {
            Population::NoToken => Some(NOTHING_CACHED.to_owned()),
            Population::InFlight => Some(LOOKING.to_owned()),
            Population::Unreachable(detail) => Some(format!("{UNREACHABLE} · {detail}")),
            Population::Reached => None,
        }
    }

    /// How many entities the attached workspace holds.
    #[must_use]
    pub fn cached(&self) -> usize {
        self.read()
            .per_workspace
            .get(&self.attached)
            .map_or(0, Trie::len)
    }

    /// The corpus for reading. A poisoned lock is recovered — see the type.
    fn read(&self) -> std::sync::RwLockReadGuard<'_, Corpus> {
        self.corpus
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The corpus for writing. A poisoned lock is recovered — see the type.
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Corpus> {
        self.corpus
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Entries for NotesTrie {
    fn matches(&self, prefix: &str, limit: usize) -> Vec<Entry> {
        let corpus = self.read();
        corpus
            .per_workspace
            .get(&self.attached)
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

    /// The inherent [`NotesTrie::absence`], through the port the pump asks on.
    ///
    /// Both exist because the host sets the line at session open through
    /// `Composer::set_absence` and the pump re-asks on the beat; one function
    /// answering both is what stops the two ever disagreeing.
    fn absence(&self) -> Option<String> {
        Self::absence(self)
    }
}
