// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0005] trigger clause 10b, driven by a caller outside this crate.
//!
//! "**10b — survive a session restart.** This is D8's on-disk persistence into
//! [ADR-0010]'s session directory, and it needs a decision about the on-disk
//! format under that record's D5." Every check below drives the same calls
//! `terminal::open` makes — [`CorpusCache`] for the file, [`refresh_from`] for
//! what an answer does to it, and [`NotesTrie`] for what the strip then says —
//! using only what this crate exports.
//!
//! # What these are evidence about, and what they are not
//!
//! **The mechanism, not the binary.** `terminal::open::shell_for` read
//! `~/.zaru` from the process's own `HOME` until 2026-09-27, which no check
//! here could set: this workspace denies `unsafe_code` and
//! `std::env::set_var` is unsafe in this edition. It takes a
//! `zaru_cli::config::Home` now, so that limit is gone and these checks have
//! simply not been moved onto it; what they are evidence about is unchanged.
//! The arc's artefact drives the release binary over a
//! real pseudo-terminal against a fake instance on loopback, two sessions in
//! one directory, and that is what covers `shell_for`'s own lines.
//!
//! **No instance is opened.** [`refresh_from`] takes the answer rather than
//! making the call, precisely so the three arms — reached, no transport,
//! refused — can each be driven without a socket. The split between the last
//! two is the whole of D8's eviction rule and it is where a check is worth
//! most.
//!
//! **Everything here is a generated nonce and no credential is held.**
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use zaru_cli::credentials::ReachFailure;
use zaru_cli::terminal::corpus::stamp;
use zaru_cli::terminal::{CorpusCache, FROM_CACHE, LOOKING, NotesTrie, Refresh, refresh_from};
use zaru_notes::session::CallRefused;
use zaru_notes::trie::{CachedEntry, EntryKind};
use zaru_tui::composer::Entries;

/// The instance every check below pretends to have read.
const HOST: &str = "play.cortex.page";
/// The workspace every check below pins, unless it is testing a changed pin.
const WORKSPACE: &str = "docs";
/// A time in the past, so a stamp is a fixed string rather than a clock read.
const FETCHED: u128 = 1_789_448_582_743;

/// A scratch `~/.zaru`-equivalent of this check's own.
struct Scratch {
    path: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!("zaru-corpus-out-{label}-{nonce}"));
        std::fs::create_dir_all(&path).expect("a scratch root");
        Self { path }
    }

    fn cache(&self) -> CorpusCache {
        CorpusCache::under(&self.path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

fn corpus(workspace: &str, titles: &[&str]) -> Vec<CachedEntry> {
    titles
        .iter()
        .enumerate()
        .map(|(at, title)| {
            CachedEntry::new(workspace, format!("notes/{at}"), *title, EntryKind::Page)
        })
        .collect()
}

/// What `shell_for` does: ask the file, and open over what it holds.
fn open_over(cache: &CorpusCache, host: &str, workspace: &str) -> NotesTrie {
    match cache.read(host, workspace).expect("the file reads") {
        Some(cached) => NotesTrie::from_cache(cached.entries, workspace, cached.fetched),
        None => NotesTrie::awaiting(workspace),
    }
}

fn titles(trie: &NotesTrie, prefix: &str) -> Vec<String> {
    trie.matches(prefix, 8)
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}

/// **Clause 10b.** A second session completes from the file, before anything
/// the network might say has arrived.
///
/// The ordering is the assertion and it is why the refresh is applied *after*
/// the matches are read: at that instant the only thing this session has ever
/// had is the file the last one left.
#[test]
fn a_second_session_completes_from_the_file_before_any_refresh_answers() {
    let scratch = Scratch::new("second-session");
    let cache = scratch.cache();

    // The first session's refresh lands and records what it fetched.
    let first = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        first.absence().as_deref(),
        Some(LOOKING),
        "a machine with no file starts exactly as it did before this arc"
    );
    assert!(
        titles(&first, "hom").is_empty(),
        "and it has nothing to complete against"
    );
    let fetched = corpus(WORKSPACE, &["Home", "Homework", "Ω ✦ elsewhere"]);
    match refresh_from(Ok(fetched), &cache, HOST, WORKSPACE, FETCHED) {
        Refresh::Reached(entries) => first.reached(entries),
        other => panic!("a landed fetch is Reached; it gave {other:?}"),
    }

    // The second session, in a new process, over the same file.
    let second = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        titles(&second, "hom"),
        vec!["Home".to_owned(), "Homework".to_owned()],
        "the second session did not complete from the file"
    );
    assert_eq!(
        second.absence(),
        None,
        "and it has nothing to say, because it is not waiting for anything"
    );
    assert_eq!(
        second.cached(),
        3,
        "the whole corpus came back, not a prefix"
    );
}

/// A refresh that lands replaces the corpus in hand and the line on disk.
#[test]
fn a_refresh_that_lands_replaces_the_corpus_and_the_file() {
    let scratch = Scratch::new("replace");
    let cache = scratch.cache();
    drop(refresh_from(
        Ok(corpus(WORKSPACE, &["Home"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED,
    ));

    let trie = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        titles(&trie, "hom"),
        vec!["Home".to_owned()],
        "the first fetch was never written, so nothing below is about a replacement"
    );

    match refresh_from(
        Ok(corpus(WORKSPACE, &["Homestead", "Homily"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED + 1,
    ) {
        Refresh::Reached(entries) => trie.reached(entries),
        other => panic!("a landed fetch is Reached; it gave {other:?}"),
    }
    assert_eq!(
        titles(&trie, "hom"),
        vec!["Homily".to_owned(), "Homestead".to_owned()],
        "the strip is still serving what the first fetch left"
    );

    let after = cache
        .read(HOST, WORKSPACE)
        .expect("the file reads")
        .expect("the key is there");
    assert_eq!(after.fetched, FETCHED + 1, "the file kept the older time");
    let stored: Vec<&str> = after
        .entries
        .iter()
        .map(|entry| entry.title.as_str())
        .collect();
    assert_eq!(
        stored,
        vec!["Homestead", "Homily"],
        "the file is still holding the first fetch's corpus"
    );
}

/// An instance that never answered keeps the corpus, and the strip says when
/// it was taken.
#[test]
fn an_instance_that_cannot_be_reached_keeps_the_corpus_and_says_when_it_was_taken() {
    let scratch = Scratch::new("unreachable");
    let cache = scratch.cache();
    drop(refresh_from(
        Ok(corpus(WORKSPACE, &["Home"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED,
    ));

    // **Both silent shapes, because the arm covers both and the one that
    // actually happens is the second.** `Endpoint` is "no HTTP client could be
    // built"; a machine with no network answers `Session`, carrying reqwest's
    // own sentence. Neither is the instance saying anything about this token.
    for silence in [
        ReachFailure::Endpoint("could not build an HTTP client".to_owned()),
        ReachFailure::Session(
            "could not attach a session: error sending request for url              (https://play.cortex.page/api/mcp): dns error"
                .to_owned(),
        ),
    ] {
        let trie = open_over(&cache, HOST, WORKSPACE);
        let detail = silence.to_string();
        match refresh_from(Err(silence), &cache, HOST, WORKSPACE, FETCHED + 1) {
            Refresh::Unreachable(said) => {
                assert_eq!(said, detail, "the client's own sentence is not paraphrased");
                trie.unreachable(said);
            }
            other => panic!("silence is Unreachable; it gave {other:?}"),
        }
        assert_eq!(
            titles(&trie, "hom"),
            vec!["Home".to_owned()],
            "a person on a train lost the notes they already had"
        );
        assert_eq!(
            trie.absence(),
            Some(format!("{FROM_CACHE} {}", stamp(FETCHED))),
            "and was not told the strip is out of date"
        );
    }
    let trie = open_over(&cache, HOST, WORKSPACE);

    assert_eq!(
        titles(&trie, "hom"),
        vec!["Home".to_owned()],
        "a third session, after two silent refreshes, has lost the corpus"
    );
    assert!(
        cache
            .read(HOST, WORKSPACE)
            .expect("the file reads")
            .is_some(),
        "an instance that said nothing must not cost the file its entry"
    );
}

/// An instance that answered and refused loses the entry, in memory and on
/// disk, so the next session starts cold.
#[test]
fn a_refusal_evicts_the_entry_and_the_next_session_starts_cold() {
    let scratch = Scratch::new("refused");
    let cache = scratch.cache();
    drop(refresh_from(
        Ok(corpus(WORKSPACE, &["Home"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED,
    ));
    // A second key, so an eviction that took everything is distinguishable
    // from one that took what it was asked for.
    drop(refresh_from(
        Ok(corpus("main", &["Elsewhere"])),
        &cache,
        HOST,
        "main",
        FETCHED,
    ));

    let trie = open_over(&cache, HOST, WORKSPACE);
    // The instance answering is a `CallRefused`, which is the one shape that
    // says anything about what this token may reach. It is the exact value
    // `play.cortex.page` returns for a workspace the token is not a member of,
    // measured on 2026-09-15.
    let refused = CallRefused {
        tool: "pages.list".to_owned(),
        code: -32002,
        detail: "You are not a member of that workspace.".to_owned(),
    };
    let detail = refused.to_string();
    match refresh_from(
        Err(ReachFailure::Refused(refused)),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED + 1,
    ) {
        Refresh::Refused(said) => {
            assert_eq!(said, detail, "the server's own sentence is not paraphrased");
            trie.refused(said);
        }
        other => panic!("an answered refusal is Refused; it gave {other:?}"),
    }

    assert!(
        titles(&trie, "hom").is_empty(),
        "a token that has lost the workspace is still completing against it"
    );
    assert!(
        cache
            .read(HOST, WORKSPACE)
            .expect("the file reads")
            .is_none(),
        "and the next session would read it straight back off disk"
    );
    assert!(
        cache.read(HOST, "main").expect("the file reads").is_some(),
        "a neighbour key was taken with it, which a remover of everything would pass"
    );

    let next = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        next.absence().as_deref(),
        Some(LOOKING),
        "the session after a refusal starts cold, which is the point of evicting"
    );
}

/// A project that pins a different workspace does not read another one's
/// corpus.
#[test]
fn a_changed_pin_misses_the_cache() {
    let scratch = Scratch::new("pin");
    let cache = scratch.cache();
    drop(refresh_from(
        Ok(corpus(WORKSPACE, &["Home"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED,
    ));

    let moved = open_over(&cache, HOST, "main");
    assert!(
        titles(&moved, "hom").is_empty(),
        "a session pinned to `main` was served `docs`'s corpus"
    );
    assert_eq!(
        moved.absence().as_deref(),
        Some(LOOKING),
        "and it was not told it is waiting for a corpus of its own"
    );

    let unmoved = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        titles(&unmoved, "hom"),
        vec!["Home".to_owned()],
        "and the pin that did not change lost its corpus, so the miss is about the key"
    );
}

/// A composer token pointing at a different instance does not read the old
/// instance's corpus, even for the same workspace slug.
#[test]
fn a_changed_composer_token_misses_the_cache() {
    let scratch = Scratch::new("token");
    let cache = scratch.cache();
    drop(refresh_from(
        Ok(corpus(WORKSPACE, &["Home"])),
        &cache,
        HOST,
        WORKSPACE,
        FETCHED,
    ));

    let elsewhere = open_over(&cache, "other.cortex.page", WORKSPACE);
    assert!(
        titles(&elsewhere, "hom").is_empty(),
        "a slug that exists on two instances is not one corpus"
    );
    assert_eq!(
        elsewhere.absence().as_deref(),
        Some(LOOKING),
        "and it was not told it is waiting for a corpus of its own"
    );

    let same = open_over(&cache, HOST, WORKSPACE);
    assert_eq!(
        titles(&same, "hom"),
        vec!["Home".to_owned()],
        "and the instance that did not change lost its corpus, so the miss is about the key"
    );
}

/// **The security corpus.** A listing row carrying more than this client reads
/// puts none of it in the file.
///
/// `session::listing::read` accepts extra fields on purpose — its own
/// `a_row_carrying_more_than_this_client_reads_is_still_read` says so — so the
/// server may grow a row at any time and a body may one day ride on one. What
/// stops the body reaching the disk is that [`Row`](zaru_cli::terminal::corpus::Row)
/// has three fields, and this is what says so from outside the crate.
///
/// The stored bearer is a second nonce and is here for the same reason: a file
/// under `~/.zaru/` that could grow a credential is the failure worth a
/// permanent check, not the one worth a comment.
#[test]
fn the_cache_file_holds_no_page_body_and_no_token() {
    let scratch = Scratch::new("no-body");
    let cache = scratch.cache();

    let body = "body-nonce-3f9a2c-Ω-do-not-store";
    let bearer = "nn_mcp_bearer-nonce-7c41d8-do-not-store";
    let answer = format!(
        r#"[{{"kind":"page","id":"b73ea90d","path":"adrs/0005","title":"Ω ✦ the composer","body":"{body}","excerpt":"{body}","updatedAt":"2026-09-15"}}]"#
    );
    let page = zaru_notes::session::listing::read("pages.list", &answer)
        .expect("the shape the server sends is read");
    assert_eq!(
        page.listed.len(),
        1,
        "the row did not parse, so nothing below is about a row"
    );

    let entries: Vec<CachedEntry> = page
        .listed
        .into_iter()
        .map(|listed| CachedEntry::new(WORKSPACE, listed.path, listed.title, EntryKind::Page))
        .collect();
    drop(refresh_from(Ok(entries), &cache, HOST, WORKSPACE, FETCHED));

    let raw = std::fs::read_to_string(cache.path()).expect("the file was written");
    assert!(
        !raw.contains(body),
        "a page body reached {}: {raw}",
        cache.path().display()
    );
    assert!(
        !raw.contains("excerpt"),
        "a field this client does not read reached the file: {raw}"
    );
    assert!(
        !raw.contains(bearer) && !raw.contains("nn_mcp_"),
        "a credential reached the file: {raw}"
    );
}

/// The accepting sibling: the file **does** hold what the strip completes
/// against, and at the mode the directory's own files take.
///
/// Without this, every assertion above is satisfied by writing nothing at all.
#[test]
fn the_cache_file_holds_the_paths_and_titles_the_strip_completes_against() {
    let scratch = Scratch::new("sibling");
    let cache = scratch.cache();

    let answer = r#"[{"kind":"page","id":"b73ea90d","path":"adrs/0005","title":"Ω ✦ the composer","body":"body-nonce-3f9a2c","updatedAt":"2026-09-15"}]"#;
    let page = zaru_notes::session::listing::read("pages.list", answer).expect("the row parses");
    let entries: Vec<CachedEntry> = page
        .listed
        .into_iter()
        .map(|listed| CachedEntry::new(WORKSPACE, listed.path, listed.title, EntryKind::Page))
        .collect();
    drop(refresh_from(Ok(entries), &cache, HOST, WORKSPACE, FETCHED));

    let raw = std::fs::read_to_string(cache.path()).expect("the file was written");
    assert!(
        raw.contains("adrs/0005"),
        "the path is not in the file: {raw}"
    );
    assert!(
        raw.contains("Ω ✦ the composer"),
        "the title is not in the file, byte for byte as the server spelled it: {raw}"
    );
    assert!(
        raw.contains(HOST),
        "the key's host is not in the file: {raw}"
    );
    assert!(
        raw.contains(WORKSPACE),
        "the key's workspace is not in the file: {raw}"
    );

    // ADR-0010 D5's `cat` claim, read against this file: it is one line of
    // plain text per key and it parses as JSON, so `cat` and `grep` both work.
    assert!(raw.ends_with('\n') && raw.lines().count() == 1);
    assert!(
        serde_json::from_str::<serde_json::Value>(raw.trim_end()).is_ok(),
        "the line is not JSON: {raw}"
    );

    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(cache.path())
        .expect("the file is there")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the file is not 0600, it is {mode:o}");
}
