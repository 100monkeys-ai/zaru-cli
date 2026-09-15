// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for [ADR-0005] D8's on-disk corpus.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer

use super::{CorpusCache, Row, RowKind, stamp};
use zaru_notes::trie::{CachedEntry, EntryKind};

/// A scratch directory of this check's own, named so two cannot collide.
fn scratch(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let path = std::env::temp_dir().join(format!("zaru-corpus-{label}-{nonce}"));
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

fn entry(workspace: &str, path: &str, title: &str, kind: EntryKind) -> CachedEntry {
    CachedEntry::new(workspace, path, title, kind)
}

/// A written corpus comes back as it went in, and the key is the pair.
#[test]
fn a_corpus_comes_back_for_its_own_host_and_workspace_and_for_no_other() {
    let root = scratch("roundtrip");
    let cache = CorpusCache::under(&root);

    cache
        .append(
            "play.cortex.page",
            "docs",
            &[
                entry("docs", "adrs/0005", "Ω ✦ the composer", EntryKind::Page),
                entry("docs", "atoms/glossary", "Glossary", EntryKind::Atom),
            ],
            1_789_448_582_743,
        )
        .expect("the cache is writable");

    let mine = cache
        .read("play.cortex.page", "docs")
        .expect("the file reads")
        .expect("this key was written");
    assert_eq!(mine.fetched, 1_789_448_582_743);
    assert_eq!(
        mine.entries,
        vec![
            entry("docs", "adrs/0005", "Ω ✦ the composer", EntryKind::Page),
            entry("docs", "atoms/glossary", "Glossary", EntryKind::Atom),
        ],
        "a title comes back exactly as the server spelled it"
    );

    assert!(
        cache
            .read("play.cortex.page", "main")
            .expect("the file reads")
            .is_none(),
        "a different workspace on the same instance is a different key"
    );
    assert!(
        cache
            .read("other.cortex.page", "docs")
            .expect("the file reads")
            .is_none(),
        "a different instance with the same workspace slug is a different key"
    );

    std::fs::remove_dir_all(&root).expect("the scratch directory goes");
}

/// An append supersedes before a compaction, and the compaction leaves one.
#[test]
fn the_last_line_for_a_key_wins_and_compaction_leaves_exactly_it() {
    let root = scratch("supersede");
    let cache = CorpusCache::under(&root);

    for (title, at) in [("old", 1), ("newer", 2), ("newest", 3)] {
        cache
            .append(
                "play.cortex.page",
                "docs",
                &[entry("docs", "p", title, EntryKind::Page)],
                at,
            )
            .expect("the cache is writable");
    }
    cache
        .append(
            "play.cortex.page",
            "main",
            &[entry("main", "q", "other", EntryKind::Page)],
            9,
        )
        .expect("the cache is writable");

    assert_eq!(
        cache
            .read("play.cortex.page", "docs")
            .expect("the file reads")
            .expect("this key was written")
            .entries[0]
            .title,
        "newest",
        "the last matching line is the answer before any compaction"
    );

    assert!(
        cache.compact().expect("the file rewrites"),
        "three lines for one key became one"
    );
    let lines = cache.lines().expect("the file reads");
    assert_eq!(lines.len(), 2, "one line per key: {lines:?}");
    assert_eq!(lines[0].workspace, "docs", "the file's own order is kept");
    assert_eq!(lines[0].entries[0].title, "newest");
    assert_eq!(lines[1].workspace, "main");
    assert!(
        !cache.compact().expect("the file rewrites"),
        "a compacted file is not rewritten again"
    );

    std::fs::remove_dir_all(&root).expect("the scratch directory goes");
}

/// Eviction takes one key and leaves its neighbours alone.
#[test]
fn eviction_takes_one_key_and_a_neighbour_survives() {
    let root = scratch("evict");
    let cache = CorpusCache::under(&root);
    cache
        .append("h", "docs", &[entry("docs", "p", "t", EntryKind::Page)], 1)
        .expect("the cache is writable");
    cache
        .append("h", "main", &[entry("main", "q", "u", EntryKind::Page)], 2)
        .expect("the cache is writable");

    assert!(cache.evict("h", "docs").expect("the file rewrites"));
    assert!(
        cache.read("h", "docs").expect("the file reads").is_none(),
        "the evicted key is gone"
    );
    assert!(
        cache.read("h", "main").expect("the file reads").is_some(),
        "the neighbour survives, which is what a remover of everything would fail"
    );
    assert!(
        !cache.evict("h", "docs").expect("the file rewrites"),
        "evicting a key that is not there changes nothing"
    );

    std::fs::remove_dir_all(&root).expect("the scratch directory goes");
}

/// A line that was in flight when the machine died is never counted.
#[test]
fn a_trailing_fragment_is_never_counted_as_a_line() {
    let root = scratch("fragment");
    let cache = CorpusCache::under(&root);
    cache
        .append("h", "docs", &[entry("docs", "p", "t", EntryKind::Page)], 1)
        .expect("the cache is writable");

    let mut raw = std::fs::read_to_string(cache.path()).expect("the file reads");
    raw.push_str(r#"{"host":"h","workspace":"main","fetched":2,"entr"#);
    std::fs::write(cache.path(), raw).expect("the file writes");

    let lines = cache.lines().expect("a fragment is tolerated");
    assert_eq!(
        lines.len(),
        1,
        "the half-written line was counted: {lines:?}"
    );
    assert!(
        cache.read("h", "main").expect("the file reads").is_none(),
        "and it answers for no key"
    );

    std::fs::remove_dir_all(&root).expect("the scratch directory goes");
}

/// An absent file is no corpus rather than a failure.
#[test]
fn a_machine_that_has_never_fetched_reads_as_no_corpus() {
    let root = scratch("absent");
    let cache = CorpusCache::under(&root);
    assert!(
        cache
            .lines()
            .expect("an absent file is not an error")
            .is_empty()
    );
    assert!(
        cache
            .read("h", "docs")
            .expect("an absent file is not an error")
            .is_none()
    );
    assert!(!cache.compact().expect("an absent file is not an error"));
    std::fs::remove_dir_all(&root).expect("the scratch directory goes");
}

/// The row carries three fields and a kind, and the kind round-trips.
#[test]
fn a_row_is_the_three_things_the_trie_indexes_and_the_kind_survives_the_file() {
    let rendered = serde_json::to_string(&Row {
        path: "atoms/x".to_owned(),
        title: "Ω".to_owned(),
        kind: RowKind::Atom,
    })
    .expect("a row renders");
    assert_eq!(rendered, r#"{"path":"atoms/x","title":"Ω","kind":"atom"}"#);
    assert_eq!(RowKind::of(EntryKind::Page).cached(), EntryKind::Page);
    assert_eq!(RowKind::of(EntryKind::Atom).cached(), EntryKind::Atom);
}

/// The stamp is the civil date in UTC, on both sides of a leap day.
#[test]
fn a_stored_time_renders_as_a_civil_date_a_person_can_read() {
    assert_eq!(stamp(0), "1970-01-01 00:00 UTC");
    assert_eq!(stamp(1_789_448_582_743), "2026-09-15 05:03 UTC");
    // 2024-02-29T12:34:00Z, the day a month-length table gets wrong.
    assert_eq!(stamp(1_709_210_040_000), "2024-02-29 12:34 UTC");
    // 2024-03-01T00:00:00Z, the day after it.
    assert_eq!(stamp(1_709_251_200_000), "2024-03-01 00:00 UTC");
}
