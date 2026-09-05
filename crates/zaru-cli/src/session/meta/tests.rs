// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D1's `meta.toml`, written and read back.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::config::Layer;
use crate::runtime::{ResolvedTier, Tier};
use crate::session::fixtures::ScratchRoot;
use crate::session::id::Millis;
use crate::session::meta::file::{ENDED_KEY, MetaFile, TIER_FROM_KEY, TIER_KEY};
use crate::session::meta::{Meta, MetaStore};
use crate::session::store::FILE_MODE;
use std::os::unix::fs::PermissionsExt;

/// A session's metadata, with values a `format!` would get wrong.
fn awkward() -> Meta {
    Meta::new(
        ResolvedTier::supplied(Tier::Contained, Layer::User),
        // A quote, a backslash, a newline, a tab and a control character, plus
        // a combining mark and an astral character. Every one of these is a
        // byte a hand-written emitter escapes wrongly, and the reason
        // ADR-0010's Update refused one.
        Some("a\"b\\c\nd\te\u{7}f é\u{301}𝄞".to_owned()),
        Some("anthropic".to_owned()),
        Millis::new(1_788_579_000_000),
    )
}

/// What a session recorded about itself comes back byte for byte.
///
/// **The fixture is awkward on the axis this check is about**: the values carry
/// a quote, a backslash, a newline, a tab, a control character and a combining
/// mark, which is the escaping a `std`-only emitter would get nearly right —
/// the exact thing ADR-0010's own Update refused to accept.
///
/// The mutant is rendering the file with `format!` rather than through the
/// crate that defines the format.
#[test]
fn what_a_session_records_about_itself_comes_back_whole() {
    let root = ScratchRoot::new();
    let path = root.base().join("meta.toml");
    let mut store = MetaFile::at(&path);
    let written = awkward();

    store.write(&written).expect("the metadata is written");
    let read = store.read().expect("and read back");

    assert_eq!(read, written, "a round trip loses nothing");
    // The tier and the layer that supplied it are both properties of a
    // `ResolvedTier`, and neither can be reconstructed without the other.
    assert_eq!(read.tier(), Tier::Contained);
    assert_eq!(read.resolved_tier().supplied_by(), Layer::User);

    let bytes = std::fs::read_to_string(&path).expect("the file is on disk");
    println!("{bytes}");
    assert!(
        bytes.contains(TIER_KEY) && bytes.contains(TIER_FROM_KEY),
        "the file records the tier and the layer that supplied it: {bytes}"
    );
    assert!(
        !bytes.contains(ENDED_KEY),
        "a session that has not ended records no `ended`; D1 makes that field's absence mean \
         still running: {bytes}"
    );
}

/// The supplying layer is recorded by the word ADR-0014 D3's block prints.
///
/// **One vocabulary rather than two.** A file that spelled the layer `2` or
/// `user` would be a second name for the thing `config explain` calls `user
/// config`, and the two would drift.
///
/// The mutant is writing the layer's number instead of its label.
#[test]
fn the_supplying_layer_is_recorded_by_the_word_the_explain_block_prints() {
    let root = ScratchRoot::new();
    let mut store = MetaFile::at(root.base().join("meta.toml"));

    for layer in Layer::ALL {
        let meta = Meta::new(
            ResolvedTier::supplied(Tier::Bare, layer),
            None,
            None,
            Millis::new(1),
        );
        store.write(&meta).expect("written");
        let bytes = std::fs::read_to_string(root.base().join("meta.toml")).expect("on disk");
        assert!(
            bytes.contains(&format!("{TIER_FROM_KEY} = \"{}\"", layer.label())),
            "the file records `{}`, which is what D3's supplier column prints: {bytes}",
            layer.label()
        );
        assert_eq!(
            store
                .read()
                .expect("read back")
                .resolved_tier()
                .supplied_by(),
            layer,
            "and every one of D1's five layers survives the round trip"
        );
    }
}

/// A session with no `meta.toml` is a datum rather than a fault.
///
/// The binary starts no session, so every session directory on every machine
/// predates this writer. `read_if_present` is what a caller asks; the port's own
/// `read` turns the absence into a failure because its signature has no room
/// for one.
#[test]
fn a_session_that_recorded_nothing_is_absent_rather_than_broken() {
    let root = ScratchRoot::new();
    let store = MetaFile::at(root.base().join("meta.toml"));
    assert!(!store.path().exists(), "the fixture writes nothing");

    assert_eq!(
        store
            .read_if_present()
            .expect("an absent file is not a failure"),
        None
    );
    let failure = store
        .read()
        .expect_err("the port's read has no absent case");
    println!("{failure}");
    assert!(
        !store.path().exists(),
        "and nothing was created to find out"
    );
}

/// The file carries ADR-0010 D5's mode, read off the filesystem.
///
/// The mutant is writing at the process umask rather than at [`FILE_MODE`].
#[test]
fn meta_toml_carries_the_mode_d5_makes_the_only_protection_a_session_has() {
    let root = ScratchRoot::new();
    let path = root.base().join("meta.toml");
    MetaFile::at(&path).write(&awkward()).expect("written");

    let mode = std::fs::metadata(&path)
        .expect("the file is on disk")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode,
        FILE_MODE,
        "{} does not carry {FILE_MODE:o}, and ADR-0010 D5 says the mode is the only protection a \
         session's files have",
        path.display()
    );
}

/// A `meta.toml` this harness did not write is refused rather than half-read.
///
/// Every arm names the file and the key and **carries no value out of it**,
/// which is what [`MetaFailure`](crate::session::MetaFailure)'s own
/// documentation requires: `workspace` and `provider` arrive from outside.
///
/// The mutant is defaulting a missing `tier` rather than refusing.
#[test]
fn a_meta_toml_that_is_not_what_this_harness_wrote_is_refused_naming_the_key() {
    let root = ScratchRoot::new();
    let path = root.base().join("meta.toml");
    let planted = crate::credentials::fixtures::nonce("workspace");

    for (label, body) in [
        (
            "no tier",
            "tier_from = \"user config\"\nstarted = 1\n".to_owned(),
        ),
        (
            "a tier that names none",
            "tier = \"sandboxed\"\ntier_from = \"user config\"\nstarted = 1\n".to_owned(),
        ),
        (
            "a layer that names none",
            "tier = \"bare\"\ntier_from = \"layer two\"\nstarted = 1\n".to_owned(),
        ),
        (
            "no start",
            "tier = \"bare\"\ntier_from = \"user config\"\n".to_owned(),
        ),
        (
            "a workspace that is not text",
            "tier = \"bare\"\ntier_from = \"user config\"\nstarted = 1\nworkspace = 7\n".to_owned(),
        ),
        (
            "a start that is not a number",
            format!(
                "tier = \"bare\"\ntier_from = \"user config\"\nstarted = \"soon\"\nworkspace = \
                 \"{planted}\"\n"
            ),
        ),
    ] {
        std::fs::write(&path, &body).expect("could not stage the file");
        let read = MetaFile::at(&path).read_if_present();
        let Err(failure) = read else {
            panic!(
                "`{label}` is not a meta.toml this harness wrote and must be refused; it read as \
                 {read:?}"
            )
        };
        let rendered = failure.to_string();
        assert!(
            rendered.contains(&path.display().to_string()),
            "`{label}`: the refusal names the file: {rendered}"
        );
        assert!(
            !rendered.contains(&planted),
            "`{label}`: the refusal published a value out of the file: {rendered}"
        );
        println!("{label}: {rendered}");
    }
}
