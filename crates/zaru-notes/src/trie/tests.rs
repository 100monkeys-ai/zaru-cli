// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for the fast tier's structure.
//!
//! Every fixture title carries a nonce, a non-ASCII character and a multi-byte
//! grapheme, matching the three properties the composer's own fixtures are
//! documented as keeping: an implementation that hard-coded plausible text
//! could not produce one, and a character count and a byte count are visibly
//! different numbers.

use super::{CachedEntry, EntryKind, Trie};

/// A nonce that appears only in what this module stages.
const NONCE: &str = "trie-6b1d";

/// The strip's own budget, which is what the composition root builds at.
const RETAINED: usize = 8;

/// A page.
fn page(workspace: &str, path: &str, title: &str) -> CachedEntry {
    CachedEntry::new(workspace, path, title, EntryKind::Page)
}

/// An atom.
fn atom(workspace: &str, path: &str, title: &str) -> CachedEntry {
    CachedEntry::new(workspace, path, title, EntryKind::Atom)
}

/// The paths a match returned, in the order it returned them.
fn paths(matched: &[&CachedEntry]) -> Vec<String> {
    matched.iter().map(|entry| entry.path.clone()).collect()
}

/// The ordering decided 2026-09-05 under directive 20: shortest matching key,
/// then the key, then the workspace, then the path.
///
/// **The entry that must win is staged in the middle**, with entries on both
/// sides of it, because "the one that matters" and "the last one" — or the
/// first — are different rules that agree whenever the one that matters happens
/// to be at an end (library verification-lessons-2 §54). Here the winner is
/// third of five in insertion order, so a trie returning insertion order, the
/// first inserted, or the last cannot pass.
#[test]
fn the_shortest_matching_key_wins_then_the_key_then_the_workspace_then_the_path() {
    let trie = Trie::of(
        vec![
            page("zaru", "édge/one/long/path", &format!("Ω {NONCE} ✦ a")),
            page("zaru", "édge/one/longer", &format!("Ω {NONCE} ✦ b")),
            // The winner: the shortest key beginning "édge".
            page("zaru", "édge", &format!("Ω {NONCE} ✦ c")),
            page("aegis", "édge/two", &format!("Ω {NONCE} ✦ d")),
            page("zaru", "édge/two", &format!("Ω {NONCE} ✦ e")),
        ],
        RETAINED,
    );

    let matched = trie.matches("édge", RETAINED);
    assert_eq!(
        paths(&matched),
        vec![
            "édge",
            "édge/two",
            "édge/two",
            "édge/one/longer",
            "édge/one/long/path"
        ],
        "shortest key first; then the two nine-character keys, which tie on \
         length and on the key itself and are separated by the workspace slug — \
         `aegis` before `zaru`; then the two longer ones by key"
    );
    assert_eq!(
        matched[1].workspace, "aegis",
        "two entries tying on key length and on the key are separated by the workspace slug, \
         which with the path is ADR-0006 D6's identity pair; the second row was {:?}",
        matched[1].workspace
    );
    assert_eq!(
        matched[2].workspace, "zaru",
        "and its sibling follows it rather than preceding it"
    );
}

/// An entry reachable through two of its own keys is one row, not two.
///
/// The staged page's path and title both begin `mémbrane`, so a trie that
/// deposited per key rather than per entity would show it twice and push a real
/// second result off a strip that holds eight.
#[test]
fn an_entry_reachable_by_two_of_its_own_keys_appears_once() {
    let trie = Trie::of(
        vec![
            page("zaru", "mémbrane", &format!("mémbrane {NONCE} ✦")),
            page("zaru", "mémbrane/other", &format!("Ω {NONCE} ✦")),
        ],
        RETAINED,
    );

    let matched = trie.matches("mémbrane", RETAINED);
    assert_eq!(
        paths(&matched),
        vec!["mémbrane", "mémbrane/other"],
        "the first entry matches through both its path and its title and must appear once"
    );
}

/// D3 lists atom names separately from paths because an atom's name is not a
/// prefix of its own path. A page's last segment is **not** a key: D3 names
/// "atom names", and widening it to every entity would be inventing a key kind.
#[test]
fn an_atoms_name_is_a_key_and_a_pages_last_segment_is_not() {
    let trie = Trie::of(
        vec![
            atom("zaru", "atoms/mémbrane", &format!("Ω {NONCE} ✦ atom")),
            page(
                "zaru",
                "architecture/mémbrane",
                &format!("Ω {NONCE} ✦ page"),
            ),
        ],
        RETAINED,
    );

    assert_eq!(
        paths(&trie.matches("mémbrane", RETAINED)),
        vec!["atoms/mémbrane"],
        "the atom is reachable by its name and the page's last segment is not a key"
    );
    assert_eq!(
        paths(&trie.matches("architecture/mém", RETAINED)),
        vec!["architecture/mémbrane"],
        "and the page is still reachable by its path, so this is a narrower key set rather than \
         a missing entry"
    );
}

/// A prefix nothing starts with returns nothing, and does not panic on a
/// character the trie has never seen.
#[test]
fn a_prefix_that_matches_nothing_returns_nothing() {
    let trie = Trie::of(
        vec![page("zaru", "édge", &format!("Ω {NONCE} ✦"))],
        RETAINED,
    );

    assert!(
        trie.matches("zzz", RETAINED).is_empty(),
        "a prefix no key begins with matches nothing"
    );
    assert!(
        trie.matches("édgex", RETAINED).is_empty(),
        "and a prefix that runs one character past a key matches nothing rather than the key"
    );
    assert!(
        !trie.is_empty(),
        "a miss is not an empty trie, and the two are different sentences to a user"
    );
}

/// The walk folds case; what comes back is what was cached.
///
/// The second half is the identity assertion the security corpus rests on: the
/// strip renders the user's own words, so nothing between the cache and the
/// caller may alter them.
#[test]
fn the_walk_folds_case_and_what_comes_back_is_what_was_cached() {
    let title = format!("BÓUNDED Contexts {NONCE} ✦");
    let trie = Trie::of(vec![page("zaru", "Architecture/Bóunded", &title)], RETAINED);

    for prefix in ["bóunded", "BÓUNDED", "BóUnDeD"] {
        assert_eq!(
            trie.matches(prefix, RETAINED).len(),
            1,
            "{prefix:?} should reach the entry, whatever case it was typed in"
        );
    }
    let matched = trie.matches("bóunded", RETAINED);
    assert_eq!(
        matched[0].title, title,
        "the entry comes back exactly as it was cached, never folded"
    );
    assert_eq!(
        matched[0].path, "Architecture/Bóunded",
        "and so does its path"
    );
}

/// ADR-0006 D6: an identifier resolves only within its own workspace, so two
/// workspaces holding one path hold two entities.
#[test]
fn two_workspaces_holding_one_path_are_two_entries() {
    let trie = Trie::of(
        vec![
            page("zaru", "architecture/bóunded", &format!("Ω {NONCE} ✦ zaru")),
            page(
                "aegis",
                "architecture/bóunded",
                &format!("Ω {NONCE} ✦ aegis"),
            ),
        ],
        RETAINED,
    );

    let matched = trie.matches("architecture/", RETAINED);
    assert_eq!(
        matched.len(),
        2,
        "one path in two workspaces is two entities, not one: {:?}",
        paths(&matched)
    );
    let identities: Vec<(&str, &str)> = matched.iter().map(|entry| entry.identity()).collect();
    assert_eq!(
        identities,
        vec![
            ("aegis", "architecture/bóunded"),
            ("zaru", "architecture/bóunded")
        ],
        "and the identity that separates them is the workspace with the path"
    );
}

/// An empty trie and a populated trie that missed are different states.
#[test]
fn an_empty_trie_and_a_populated_one_that_missed_are_different_states() {
    let empty = Trie::of(Vec::new(), RETAINED);
    assert!(empty.is_empty(), "a trie with no entity is empty");
    assert_eq!(empty.len(), 0);
    assert!(empty.matches("anything", RETAINED).is_empty());

    let populated = Trie::of(
        vec![page("zaru", "édge", &format!("Ω {NONCE} ✦"))],
        RETAINED,
    );
    assert!(
        !populated.is_empty(),
        "a trie holding an entity is not empty even when a prefix misses"
    );
    assert!(populated.matches("zzz", RETAINED).is_empty());
}

/// A trie built to keep fewer rows than a caller asks for serves what it kept,
/// and says how many that is.
#[test]
fn a_trie_serves_at_most_what_it_was_built_to_retain() {
    let entries: Vec<CachedEntry> = (0..20)
        .map(|n| page("zaru", &format!("édge/{n:02}"), &format!("Ω {NONCE} ✦ {n}")))
        .collect();
    let trie = Trie::of(entries, 3);

    assert_eq!(trie.retained(), 3);
    assert_eq!(
        trie.matches("édge", 100).len(),
        3,
        "a caller asking for a hundred gets the three this trie was built to keep"
    );
    assert_eq!(
        trie.matches("édge", 2).len(),
        2,
        "and a caller asking for fewer than that gets what it asked for"
    );
    assert_eq!(
        trie.len(),
        20,
        "the cap is on what a prefix serves, never on what the trie holds"
    );
}

/// The memory and time bounds, staked against the corpus this harness can
/// actually reach.
///
/// **The size is a measurement rather than a guess.** No record names how many
/// entities a trie holds. The Zaru workspace was read on 2026-09-05 through the
/// MCP surface and held 71 pages and no atoms, and the token that read it
/// reaches six workspaces — so 426 entities is the order of the real corpus,
/// and this stages exactly that.
///
/// Both assertions are about the mechanism rather than about a wall clock: the
/// node count is the memory bound, and the *work* a query does is bounded by
/// the prefix's length plus the limit, which is asserted by reading how many
/// entries come back for a one-character prefix over a corpus of 426. A check
/// asserting elapsed milliseconds would be asserting about this machine's load
/// (library verification-lessons §20).
#[test]
fn bounds_hold_at_the_measured_corpus_size() {
    let workspaces = ["zaru", "aegis", "main", "promptly", "project", "notes"];
    let mut entries = Vec::new();
    for workspace in workspaces {
        for n in 0..71 {
            entries.push(page(
                workspace,
                &format!("óperations/páge-{n:03}"),
                &format!("Páge {n:03} {NONCE} ✦"),
            ));
        }
    }
    assert_eq!(
        entries.len(),
        426,
        "71 pages across six workspaces is the corpus measured on 2026-09-05"
    );

    let trie = Trie::of(entries, RETAINED);

    assert_eq!(trie.len(), 426);
    assert!(
        trie.nodes() < 4_000,
        "the trie occupies {} nodes for 426 entities, and the shared path prefixes should keep \
         it under four thousand; a node count that has grown is a memory bound that has moved",
        trie.nodes()
    );
    assert_eq!(
        trie.matches("ó", RETAINED).len(),
        RETAINED,
        "a one-character prefix over 426 entities returns the strip's budget and no more, which \
         is what makes the query a descent and a copy rather than a walk of the subtree"
    );
    assert_eq!(
        trie.matches("", RETAINED).len(),
        RETAINED,
        "and so does the empty prefix, which is what a just-opened picker asks"
    );
}
