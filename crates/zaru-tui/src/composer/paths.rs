// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The third strip corpus: the working directory, and the port it arrives
//! through.
//!
//! [ADR-0005] D1's strip had two corpora — the Nuclear Notes trie and
//! [ADR-0015] D2's command vocabulary — and neither of them is the working
//! directory the tools are bounded by. A person's most common first task in a
//! code harness is "read this file and tell me about it", and until now it had
//! no supported spelling: the path was typed from memory and the model was
//! trusted to guess. The second look-and-feel audit's row 10 is that gap.
//!
//! # What this corpus is, and what it deliberately is not
//!
//! It is a list of **spellings**. `@` opens it, typing narrows it, `Tab`
//! completes the chosen spelling into the prompt **as text**, and that is the
//! whole of it. **No file is read by the composer.** Whether the model then
//! reads the path it was handed is that model's `fs.read` under
//! [ADR-0011] D3's permission model, prompted and recorded exactly as it would
//! be for a path the person typed by hand — so this surface adds a way to
//! *name* a file and adds no way to *open* one.
//!
//! # The bound is [ADR-0011] D4's, reused rather than widened
//!
//! D4 makes the working directory the boundary every tool call is measured
//! against. A spelling offered here is one whose resolved path is
//! `Placement::InTree` — the same classification `fs.read` is decided by, from
//! the same function — so the corpus cannot offer what the tool it exists to
//! feed would have to prompt about. That is stricter than D4 itself, which
//! permits an out-of-tree path and marks it; the corpus simply does not carry
//! one. The reason is that a completion is a *suggestion*, and suggesting a
//! path outside the tree would put the harness in the position of proposing
//! the act D4 exists to make visible.
//!
//! # Why the port is declared here
//!
//! The same reason [`Entries`](super::entries::Entries) is: ADR-0003 D8 gives
//! `zaru-tui` exactly one sibling dependency and
//! `scripts/check-crate-boundaries.py` fails on any other edge, so the walk,
//! the ignore rule and the boundary check all live in `zaru-cli` and this
//! crate names only the question. **Nothing in this crate's product tree
//! implements [`Paths`]**, and nothing in this crate touches a filesystem.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

/// One spelling the path corpus can offer.
///
/// # One field, because two would be two sources of one truth
///
/// A directory is offered with a trailing `/` and an ordinary file without
/// one, and that separator is both what the strip paints and what `Tab`
/// inserts. Carrying a separate `directory: bool` beside the spelling would
/// let the two disagree about a name ending in a slash, so the separator *is*
/// the statement and [`PathEntry::is_directory`] reads it back.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathEntry {
    spelling: String,
}

impl PathEntry {
    /// Take a spelling, relative to the working directory.
    ///
    /// A directory's spelling ends with `/`; nothing here adds or removes one,
    /// because the implementation that walked the tree is the thing that knows
    /// which it found.
    #[must_use]
    pub fn new(spelling: impl Into<String>) -> Self {
        Self {
            spelling: spelling.into(),
        }
    }

    /// The spelling, which is what the strip paints and what `Tab` inserts.
    #[must_use]
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    /// Whether this names a directory, which is to say whether `Tab` on it
    /// leaves the person inside it rather than finished.
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.spelling.ends_with('/')
    }
}

/// The working directory, as the strip needs it.
///
/// Synchronous for [`Entries`](super::entries::Entries)' own reason: a port
/// that returned a future would put an asynchronous runtime under every check
/// on the composer and make each rendered frame depend on a poll order. The
/// walk that backs it is cheap enough for that to be honest — measured at
/// 0.6 ms over this harness's own tree with its `.gitignore` honoured, against
/// 60 ms over the same tree without it.
pub trait Paths {
    /// Every spelling that begins with `prefix` **and is longer than it**,
    /// best first, at most `limit` of them.
    ///
    /// "Best first" is lexicographic by the spelling, which puts a directory
    /// ahead of everything under it because its spelling is their prefix. No
    /// record names an order and this one is chosen here, named, rather than
    /// falling out of whatever order the filesystem answered in.
    ///
    /// # Why the prefix itself is not a match
    ///
    /// A person who has typed `@src/` has `src/` already: offering it back is
    /// a row that adds nothing and, worse, it is a row that would stop `Tab`
    /// descending — the longest prefix shared by `src/` and `src/lib.rs` is
    /// `src/`, which is what was typed, so the completion would have nothing
    /// to add for as long as the directory was in its own answer. Stating the
    /// rule here rather than in the composer keeps it one sentence in the
    /// place an implementer reads, and keeps the truncation arithmetic at the
    /// call site honest: no answer contains an entry that cannot be offered.
    fn matches(&self, prefix: &str, limit: usize) -> Vec<PathEntry>;

    /// Tell the corpus a turn has ended.
    ///
    /// A turn is the only thing that changes the working directory while the
    /// person stays in the composer, because a turn is when `fs.write` runs.
    /// An implementation that keeps what it walked drops it here and walks
    /// again on the next `@`; one that walks every time does nothing.
    ///
    /// **This is a message and not a question**, which is why it is on the
    /// port rather than on the type the host happens to hold: the pump knows
    /// when a turn ended and knows nothing else about a corpus, and a pump
    /// holding a concrete implementation to call one method on it would be a
    /// second edge for no gain. **Defaulted to nothing**, so a staged
    /// implementation ignores it.
    fn turn_ended(&self) {}

    /// What the strip should say when this corpus has nothing to offer at all,
    /// if anything.
    ///
    /// The same shape, and the same reason, as
    /// [`Entries::absence`](super::entries::Entries::absence): whether there
    /// is anything to offer is the host's knowledge, the sentence is the
    /// host's to compose, and a prefix matching nothing in a corpus that
    /// *has* entries is an ordinary miss rather than an absence. **Defaulted
    /// to `None`**, so a staged implementation says nothing.
    fn absence(&self) -> Option<String> {
        None
    }
}
