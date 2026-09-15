// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The working directory, as [ADR-0005]'s third strip corpus.
//!
//! [`zaru_tui::composer::Paths`] is the port; this is the only implementation.
//! What it does is walk the working directory once, keep the spellings, and
//! answer a prefix from them.
//!
//! # The bound is [ADR-0011] D4's and it is reused rather than copied
//!
//! Every candidate goes through [`WorkingDirectory::classify`] — the same
//! function `fs.read` is decided by — and only [`Placement::InTree`] survives.
//! Nothing here re-implements "inside the tree", so a change to D4's
//! classification moves this corpus with it, and the two cannot drift into
//! disagreeing about a path.
//!
//! **A symlink is never followed and never offered**, which is the rule
//! `tools::files::search` already states: a link is what lets a walk leave the
//! tree the root was classified against. That is the first of the two
//! defences and `classify` is the second.
//!
//! **Each is sufficient on its own, and that was measured rather than
//! assumed.** `corpus_a_symlink_out_of_the_tree_is_never_offered` stays green
//! with the symlink skip deleted — `classify` resolves `project/escape` to the
//! directory it points at and refuses it — and stays green with the `classify`
//! call deleted, because the walk never yields the link's contents. It reddens
//! only when **both** go, printing *"a symbolic link out of the tree reached
//! the corpus: [\"escape\", \"inside/\", \"inside/file\"]"*. So neither is
//! decoration and neither is load-bearing alone, which is what defence in
//! depth is supposed to mean and is usually only asserted.
//!
//! # What is excluded, in one sentence
//!
//! Hidden entries, and whatever `.gitignore` names. Nothing else.
//!
//! The ignore rule is not an ergonomic nicety. Measured on the machine this
//! was written on: this repository walks to **434 entries in 0.6 ms** with its
//! own `.gitignore` honoured and **101,493 entries in 60 ms warm, 1.31 s
//! cold** without it, because `target/` is in there. A corpus built without it
//! would be a corpus of build artefacts.
//!
//! # The walk is lazy, and it is re-walked when a turn ends
//!
//! Nothing is walked until the first `@` of a session, because a person who
//! never types one should pay nothing. After that the answer is kept, and
//! [`Paths::turn_ended`] drops it when a turn ends — a turn is when the
//! tree can have changed, because a turn is when the model writes files.
//! Nothing watches the filesystem and nothing walks on the beat: a corpus
//! rebuilt a hundred times a second would be this surface's own version of the
//! defect [ADR-0005] D3's debounce exists against.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [`Placement::InTree`]: crate::tools::Placement::InTree
//! [`WorkingDirectory::classify`]: crate::tools::WorkingDirectory::classify

use crate::tools::WorkingDirectory;
use std::cell::RefCell;
use zaru_tui::composer::{PathEntry, Paths};

/// What the strip says when the working directory offers nothing at all.
///
/// **Authored here, drafted under a delegated coordinator ruling of
/// 2026-09-15 and open to Jeshua's veto**, in the same shape as the hint
/// strip's four absence lines and for the same reason: no record supplies the
/// words, one is needed, and it is named once here rather than typed at a call
/// site.
///
/// It says what is true of the *corpus* rather than of the directory — a tree
/// holding nothing but hidden files and ignored build output is not empty, and
/// telling a person it is would be a claim about their project rather than
/// about what can be offered. Short for the reason every line on this surface
/// is short: the composer's own check frame is forty columns and a longer
/// sentence is elided rather than wrapped. It names no decision record, which
/// the `record-citations` gate enforces.
pub const NOTHING_TO_OFFER: &str = "nothing here to name · hidden and ignored files are not shown";

/// How many entries the walk keeps.
///
/// **Ruled 2026-09-15, batched for Jeshua as a number, and silent when it is
/// reached.** A working directory that is a repository is nowhere near it —
/// this one holds 434 offerable entries — and the case the cap exists for is a
/// person opening a session somewhere that is not a project: measured at
/// `~/git_repos`, twenty-six repositories side by side with no `.gitignore` at
/// the root to prune their `target/` directories, the walk reaches **227,253
/// entries in 2.1 seconds**. Capped here it stops at **11 ms**.
///
/// # Why reaching it says nothing
///
/// [`composer::render::fitted`] already paints `… N more · type to narrow`
/// whenever the strip holds more matches than rows, which is the instruction a
/// person acts on and is true whether the corpus was capped or not; and
/// [`MATCH_LIMIT`]'s own documentation already records that such a count is
/// "of matches **the strip holds** — a lower bound on the corpus, never an
/// over-count". A second sentence about the cap would be a second authored
/// line saying the same thing in a place a person cannot act on differently.
///
/// [`MATCH_LIMIT`]: zaru_tui::composer::MATCH_LIMIT
/// [`composer::render::fitted`]: zaru_tui::composer::render
pub const WALK_CEILING: usize = 10_000;

/// The working directory's offerable spellings, walked once and kept.
#[derive(Debug)]
pub struct ProjectPaths {
    /// The tree, or `None` where this process could not read one.
    ///
    /// A session whose working directory cannot be resolved offers nothing and
    /// **says nothing**, which is the third of the three silences that fact
    /// already produces: the opening line is not painted and the history is
    /// not recalled for exactly the same `None`. Saying "nothing here to name"
    /// would be a claim about a directory nobody could read.
    here: Option<WorkingDirectory>,
    /// `None` until the first walk, and `None` again after a turn ends.
    walked: RefCell<Option<Vec<PathEntry>>>,
}

impl ProjectPaths {
    /// Take the corpus for a working directory. Nothing is walked yet.
    #[must_use]
    pub fn under(here: Option<WorkingDirectory>) -> Self {
        Self {
            here,
            walked: RefCell::new(None),
        }
    }

    /// The spellings, walking them if this is the first ask since a turn.
    fn spellings(&self) -> std::cell::Ref<'_, Vec<PathEntry>> {
        if self.walked.borrow().is_none() {
            *self.walked.borrow_mut() = Some(self.walk());
        }
        std::cell::Ref::map(self.walked.borrow(), |walked| {
            walked.as_ref().expect("the walk was just placed")
        })
    }

    /// Walk the tree once.
    ///
    /// # Every exclusion is named here rather than left to a default
    ///
    /// `ignore`'s builder has a standard filter set that reads a user's global
    /// gitignore, `.git/info/exclude` and `.ignore` files as well. Those are
    /// three more rules a person would have to know to predict what this
    /// corpus holds, and two of them live outside the project entirely. The
    /// rule this surface states is "hidden entries and whatever `.gitignore`
    /// names", so exactly those two are enabled and the rest are turned off.
    ///
    /// `require_git` is off so that a `.gitignore` is honoured wherever it
    /// sits, rather than only inside a repository: a person who wrote one
    /// meant it.
    fn walk(&self) -> Vec<PathEntry> {
        let Some(here) = &self.here else {
            return Vec::new();
        };
        let root = here.root();
        let mut spellings: Vec<PathEntry> = Vec::new();
        let walker = ignore::WalkBuilder::new(root)
            .hidden(true)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .parents(true)
            .require_git(false)
            .follow_links(false)
            .build();
        for entry in walker.flatten() {
            if spellings.len() >= WALK_CEILING {
                break;
            }
            // The root is the tree rather than something in it.
            if entry.depth() == 0 {
                continue;
            }
            let Some(kind) = entry.file_type() else {
                continue;
            };
            // Never followed and never offered — `fs.search`'s own rule, and
            // the first of this corpus's two defences.
            if kind.is_symlink() {
                continue;
            }
            // The second defence, and the one that reuses ADR-0011 D4 rather
            // than restating it. A path that resolves outside the tree is not
            // offered whatever the walk thought.
            if here.classify(entry.path()).placement().is_out_of_tree() {
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };
            // Lossy, for `fs.read`'s own reason: a name this harness cannot
            // spell is better shown with replacement characters than dropped
            // without a word. It will not complete to anything that opens, and
            // that is visible rather than silent.
            let mut spelling = relative.to_string_lossy().into_owned();
            if kind.is_dir() {
                spelling.push('/');
            }
            spellings.push(PathEntry::new(spelling));
        }
        // The port's stated order, made true here rather than inherited from
        // whatever order the walk answered in. A directory sorts ahead of
        // everything under it because its spelling is their prefix.
        spellings.sort();
        spellings
    }
}

impl Paths for ProjectPaths {
    /// Forget what was walked, so the next `@` walks again.
    fn turn_ended(&self) {
        *self.walked.borrow_mut() = None;
    }

    fn matches(&self, prefix: &str, limit: usize) -> Vec<PathEntry> {
        self.spellings()
            .iter()
            .filter(|entry| entry.spelling().starts_with(prefix) && entry.spelling() != prefix)
            .take(limit)
            .cloned()
            .collect()
    }

    /// [`NOTHING_TO_OFFER`] once a walk has found nothing, and `None` before
    /// any walk.
    ///
    /// **`None` before the walk is not an oversight.** This corpus is lazy on
    /// purpose, and an absence computed at session open would be a walk every
    /// session pays for whether or not anybody types `@`. The pump asks again
    /// on the beat, so a directory that offers nothing says so on the beat
    /// after the first `@` — the one hundred milliseconds
    /// `Entries::absence`'s own "still looking" state already spans.
    fn absence(&self) -> Option<String> {
        self.here.as_ref()?;
        match self.walked.borrow().as_deref() {
            Some([]) => Some(NOTHING_TO_OFFER.to_owned()),
            Some(_) | None => None,
        }
    }
}

#[cfg(test)]
mod tests;
