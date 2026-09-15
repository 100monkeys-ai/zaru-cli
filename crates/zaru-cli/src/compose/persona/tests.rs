// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0027] D1's persona, from inside the crate that assembles it.
//!
//! Everything that needs a socket is in `tests/persona_from_outside.rs`, over
//! `rmcp` and `tokio::io::duplex`. What is here is the file discipline, the
//! three freshness arms, the key's project-layer refusal, and the two things
//! about layer 1 that a check can hold: that a body becomes it and that an
//! absence is byte-identical to what it was.
//!
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract

use super::{
    CachedPersona, DEFAULT_PATH, Fetched, PERSONA_FILE, PersonaCache, fetched_from, key, path_in,
};
use crate::compose::prose;
use crate::credentials::ReachFailure;
use crate::redaction::HeldSecrets;
use std::path::PathBuf;

/// A directory this check owns, removed when it drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-persona-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }

    fn cache(&self) -> PersonaCache {
        PersonaCache::under(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// The file is the tenth thing under `~/.zaru/` and it is named for what it
/// holds.
#[test]
fn the_file_sits_beside_the_others_under_the_harness_directory() {
    let scratch = Scratch::new("named");
    let cache = scratch.cache();
    assert_eq!(
        cache.path(),
        scratch.0.join(PERSONA_FILE),
        "the cache is not under the root it was given"
    );
    assert_eq!(PERSONA_FILE, "persona.jsonl");
}

/// A body written is a body read back, keyed by host, workspace and path.
#[test]
fn a_persona_round_trips_through_the_file_under_its_own_key() {
    let scratch = Scratch::new("roundtrip");
    let cache = scratch.cache();
    let nothing = HeldSecrets::none();

    cache
        .append(
            "play2.cortex.page",
            "a-workspace",
            "zaru/persona",
            "Ω ✦ you are Zaru\nand this is a second line",
            17,
            &nothing,
        )
        .expect("the append lands");

    let read = cache
        .read("play2.cortex.page", "a-workspace", "zaru/persona")
        .expect("the file reads")
        .expect("the line is there");
    assert_eq!(
        read,
        CachedPersona {
            body: "Ω ✦ you are Zaru\nand this is a second line".to_owned(),
            fetched: 17,
        },
        "the body did not come back as it went in"
    );

    // Every part of the key discriminates, so a persona cached for one
    // workspace cannot be served to another and a path change is a miss
    // rather than a stale hit.
    for (host, workspace, path) in [
        ("other.cortex.page", "a-workspace", "zaru/persona"),
        ("play2.cortex.page", "other-workspace", "zaru/persona"),
        ("play2.cortex.page", "a-workspace", "other/page"),
    ] {
        assert!(
            cache
                .read(host, workspace, path)
                .expect("the file reads")
                .is_none(),
            "a persona was served under the wrong key: {host}/{workspace}/{path}"
        );
    }
}

/// The last matching line wins, and a compaction leaves one line per key.
#[test]
fn an_append_supersedes_and_a_compaction_leaves_one_line_per_key() {
    let scratch = Scratch::new("supersede");
    let cache = scratch.cache();
    let nothing = HeldSecrets::none();

    for (body, at) in [("older", 1_u128), ("newer", 2)] {
        cache
            .append("h", "w", "p", body, at, &nothing)
            .expect("the append lands");
    }
    assert_eq!(
        cache.lines().expect("the file reads").len(),
        2,
        "the second append did not land beside the first"
    );
    assert_eq!(
        cache
            .read("h", "w", "p")
            .expect("the file reads")
            .expect("a line is there")
            .body,
        "newer",
        "the older line won, so an append does not supersede"
    );

    assert!(cache.compact().expect("the rewrite lands"), "nothing moved");
    let lines = cache.lines().expect("the file reads");
    assert_eq!(lines.len(), 1, "the compaction left more than one line");
    assert_eq!(lines[0].body, "newer", "the compaction kept the older line");
}

/// A line that was in flight when the process died is never counted.
#[test]
fn a_partial_trailing_line_is_not_a_line() {
    let scratch = Scratch::new("partial");
    let cache = scratch.cache();
    let nothing = HeldSecrets::none();
    cache
        .append("h", "w", "p", "whole", 1, &nothing)
        .expect("the append lands");

    let mut raw = std::fs::read_to_string(cache.path()).expect("the file reads");
    raw.push_str("{\"host\":\"h\",\"workspace\":\"w\",\"path\":\"p\",\"fetch");
    std::fs::write(cache.path(), raw).expect("the file writes");

    let lines = cache.lines().expect("the file reads past the torn line");
    assert_eq!(
        lines.len(),
        1,
        "the line that was in flight was counted, so a killed process costs more than one append"
    );
}

/// The file carries [ADR-0010] D5's mode, read off the filesystem.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn the_file_carries_the_mode_that_is_its_only_protection() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("mode");
    let cache = scratch.cache();
    cache
        .append("h", "w", "p", "a body", 1, &HeldSecrets::none())
        .expect("the append lands");

    let mode = std::fs::metadata(cache.path())
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode,
        crate::credentials::store::FILE_MODE,
        "{} does not carry {:o}, and ADR-0010 D5 says the mode is the only protection a file \
         holding what a model is told has",
        cache.path().display(),
        crate::credentials::store::FILE_MODE
    );
}

/// The three freshness arms, driven one after the other over one file.
///
/// **This is [ADR-0005] D8's rule and the discriminator is whether the
/// instance answered.** Silence is not a refusal, so a cache survives it; an
/// answer that refuses is the instance saying this token may not have this
/// page, so the file forgets it at once rather than at the next compaction.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
#[test]
fn the_cache_stands_on_silence_and_is_evicted_on_a_refusal() {
    let scratch = Scratch::new("arms");
    let cache = scratch.cache();
    let nothing = HeldSecrets::none();

    let reached = fetched_from(
        Ok("a served persona".to_owned()),
        &cache,
        "h",
        "w",
        "p",
        1,
        &nothing,
    );
    assert!(matches!(reached, Fetched::Reached(_)), "{reached:?}");
    assert_eq!(reached.body(), Some("a served persona"));
    assert!(
        cache.read("h", "w", "p").expect("reads").is_some(),
        "a reached page did not land in the file"
    );

    for silence in [
        ReachFailure::Endpoint("no TLS backend".to_owned()),
        ReachFailure::Session("error sending request".to_owned()),
    ] {
        let quiet = fetched_from(Err(silence), &cache, "h", "w", "p", 2, &nothing);
        assert!(matches!(quiet, Fetched::Unreachable(_)), "{quiet:?}");
        assert_eq!(
            quiet.body(),
            None,
            "an unreachable instance served a body, so the caller would prefer it to the cache"
        );
        assert!(
            cache.read("h", "w", "p").expect("reads").is_some(),
            "silence evicted the cache, and a person on a train would lose their persona"
        );
    }

    let refused = fetched_from(
        Err(ReachFailure::Refused(zaru_notes::session::CallRefused {
            tool: "pages.read".to_owned(),
            code: -32002,
            detail: "You are not a member of that workspace.".to_owned(),
        })),
        &cache,
        "h",
        "w",
        "p",
        3,
        &nothing,
    );
    assert!(matches!(refused, Fetched::Refused(_)), "{refused:?}");
    assert!(
        cache.read("h", "w", "p").expect("reads").is_none(),
        "the instance answered and refused and the file kept serving its old body as a system \
         prompt"
    );
}

/// A served body becomes layer 1, and an absence is byte-identical to what it
/// was.
#[test]
fn a_served_body_is_layer_one_and_an_absence_is_exactly_what_it_was() {
    let served = "Ω ✦ you are Zaru, and you are direct";
    let with = crate::compose::prefix_for(Some(served));
    assert!(
        with.as_str().starts_with(served),
        "the served persona did not reach layer 1: {}",
        with.as_str()
    );
    assert!(
        !with.as_str().contains(prose::NO_PERSONA),
        "a session with a persona still carried the absence line"
    );

    // The absence, in all three of its spellings, byte-identical to each other
    // and to what the prefix was before this module existed.
    let none = crate::compose::prefix_for(None);
    let empty = crate::compose::prefix_for(Some(""));
    assert_eq!(
        none.as_str(),
        prose::NO_PERSONA,
        "the absent prefix is not the absence line, so a reader could not tell a harness with no \
         persona from one whose page was empty"
    );
    assert_eq!(
        empty.as_str(),
        none.as_str(),
        "a page that came back empty produced a different prefix from no page at all"
    );
}

/// The key is refused to the project layer, and the refusal names where it
/// does belong.
///
/// [ADR-0014] D6: a repository the user cloned "must not be able to configure
/// its way to more privilege than the user granted". A persona is a system
/// prompt.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn a_cloned_repository_cannot_choose_what_a_model_is_told_it_is() {
    // Read off the schema the binary actually folds, not off this module's
    // own `field()`: a key declared here and never declared *there* would pass
    // an assertion against itself and be an unknown key in the product.
    let field = crate::cli::layers::schema()
        .field(&key())
        .expect("the binary declares `persona.path`")
        .clone();
    assert_eq!(
        field.kind,
        crate::config::FieldKind::Text,
        "a persona path is a page path"
    );
    let crate::config::ProjectPolicy::Refused { reason } = &field.project else {
        panic!(
            "the project layer may set `persona.path`, so a repository a person cloned can hand \
             their model a system prompt of its own choosing"
        )
    };
    assert!(
        reason.contains("~/.zaru/config.toml"),
        "the refusal does not say where the key does belong: {reason}"
    );
}

/// Absent is the default path, and a set value wins.
#[test]
fn the_path_defaults_without_a_built_in_row_and_a_set_value_wins() {
    let bare = crate::config::Resolution::resolve(&crate::cli::layers::schema(), Vec::new())
        .expect("an empty configuration resolves");
    assert_eq!(
        path_in(&bare),
        DEFAULT_PATH,
        "an unset key did not fall back to the default page"
    );
    assert!(
        bare.get(&key()).is_none(),
        "`persona.path` has a built-in row, so `zaru config explain` would claim a layer nobody \
         set"
    );

    // The accepting sibling: a set value wins, so the default above is a
    // fallback rather than a constant nothing can move.
    let mut document = crate::config::Table::new();
    document.insert_path(
        &key(),
        crate::config::Value::Text("notes/who-i-am".to_owned()),
    );
    let set = crate::config::Resolution::resolve(
        &crate::cli::layers::schema(),
        [crate::config::Contribution::new(
            crate::config::Layer::User,
            crate::config::Source::named("~/.zaru/config.toml (staged)"),
            document,
        )],
    )
    .expect("a user layer naming the page resolves");
    assert_eq!(path_in(&set), "notes/who-i-am");
}

/// A cache hit owes a refresh; nothing else does.
///
/// **This is the callee's half of the defect the artefact of 2026-09-15
/// found.** The caller's half was `compose::turn::task` taking this and
/// dropping it, which made [ADR-0005] D8's eviction unreachable on the
/// one-shot surface — a page the instance refuses served from the file for
/// ever. Both halves are needed: a `Serving` that owed nothing would make the
/// caller correct and the behaviour still wrong.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
#[test]
fn a_cache_hit_owes_a_refresh_and_taking_it_owes_it_once() {
    let scratch = Scratch::new("owed");
    let cache = scratch.cache();
    cache
        .append("h", "w", "p", "a cached persona", 1, &HeldSecrets::none())
        .expect("the append lands");

    let mut owed = super::Serving::over(
        Some("a cached persona".to_owned()),
        Some(super::Refreshing::of(
            "h".to_owned(),
            "w".to_owned(),
            "p".to_owned(),
            crate::credentials::Secret::notes("nn_mcp_stagedvalueforthischeck".to_owned())
                .expect("nn_mcp_ names a kind"),
            cache,
            HeldSecrets::none(),
        )),
    );
    assert!(
        owed.pending(),
        "a cache hit owes no refresh, so a revoked page would be served from the file for ever \
         and ADR-0005 D8's eviction would be unreachable"
    );
    assert!(
        owed.take_refreshing().is_some(),
        "the refresh was not there"
    );
    assert!(
        !owed.pending() && owed.take_refreshing().is_none(),
        "the refresh is owed twice, so a caller could run it twice against one instance"
    );

    let mut nothing = super::Serving::nothing();
    assert!(
        !nothing.pending() && nothing.take_refreshing().is_none(),
        "a session with no persona at all owes a refresh"
    );
    assert!(
        nothing.body().is_none(),
        "a session with no persona at all serves one"
    );
}
