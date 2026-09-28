// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside this crate drives [ADR-0001]'s runtime tiers through the
//! crate's own public door.
//!
//! # What this establishes, and what it does not
//!
//! [Verification lessons] §25: a per-property check cannot see a defect that
//! lives in a seam, and "there is no mutant for a call that was never
//! written". The unit checks reach the module's internals; this one reaches
//! only what `zaru-cli` exports, so a type or method that was never made
//! public fails here and nowhere else.
//!
//! **It is not evidence about the `zaru` binary.** No binary resolves a tier.
//! `zaru` takes no arguments, prints its composition and exits 0, and this
//! file changes none of that: [ADR-0001] D2's `--runtime` needs the argument
//! parser [ADR-0003] D2 leaves undecided, its status line is `zaru-tui`'s, and
//! its `/runtime` is [ADR-0015] D2's namespace. What this file prints is
//! evidence about the mechanism and must not be quoted as evidence about the
//! binary.
//!
//! # The layers a file would supply are still supplied by this check
//!
//! [ADR-0014]'s layers 2, 3 and 5 have semantics and no reader, because
//! ADR-0003 D2's table names no TOML crate and no argument parser. So the
//! contributions here are constructed rather than parsed, which is the same
//! seam `config_from_outside.rs` already stands in for. Every rule ADR-0001
//! D2 states about *resolving* a tier is exercised; only the reading of a file
//! and a command line waits.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zaru_cli::config::{
    ConfigRefused, Contribution, Key, Layer, Resolution, Schema, Source, Table, Value,
};
use zaru_cli::runtime::{
    Inference, Placement, ResolvedTier, Runtime, Tier, TierRefused, ceiling, iterations,
};
use zaru_cli::session::{Meta, MetaFailure, MetaStore, Millis};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call produces, awkward on purpose.
fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{label}-{}-{nanos}-{seq}-e\u{301}\u{e9}\u{1f701}",
        std::process::id()
    )
}

/// The schema a caller builds, using the declaration ADR-0001 owns.
///
/// **The key is not spelled here.** That is the whole point of `runtime::key`
/// and `runtime::field`: a caller asks the record that owns the key, and a
/// second spelling cannot drift from the first.
fn schema() -> Schema {
    Schema::new().with(zaru_cli::runtime::key(), zaru_cli::runtime::field())
}

/// One layer offering a tier.
fn tier_at(layer: Layer, tier: &str) -> Contribution {
    let mut document = Table::new();
    document.insert_path(
        &Key::new("runtime.tier").expect("a well-formed key"),
        Value::Text(tier.to_owned()),
    );
    Contribution::new(layer, Source::named(layer.label()), document)
}

/// The whole of ADR-0001's invariant half, driven from outside: resolve a
/// tier, refuse a project's, record it on a session, read D1's row and D3's
/// ceilings, and read D2's datum.
#[test]
fn an_outside_caller_resolves_a_tier_records_it_and_reads_what_it_engages() {
    // --- D2: resolution through the layers, naming the one that supplied it.
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            tier_at(Layer::BuiltIn, "bare"),
            tier_at(Layer::User, "contained"),
            tier_at(Layer::Flag, "linked"),
        ],
    )
    .expect("three layers a user owns resolve");

    let tier = ResolvedTier::from_configuration(&resolved).expect("a tier was set");
    assert_eq!(tier.tier(), Tier::Linked, "the highest layer did not win");
    assert_eq!(tier.supplied_by(), Layer::Flag);
    println!("resolved: {tier}");

    // --- D2 as corrected against ADR-0014 D6: a project may not, a user may.
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            tier_at(Layer::BuiltIn, "bare"),
            tier_at(Layer::Project, "contained"),
        ],
    )
    .expect_err("ADR-0014 D6 refuses a project setting the runtime tier");
    let ConfigRefused::ProjectMayNotSet { key, .. } = &refusal else {
        panic!("expected D6's escalation refusal, got {refusal:?}");
    };
    assert_eq!(key.as_str(), "runtime.tier");
    println!("refused: {refusal}");

    let users_own = Resolution::resolve(&schema(), vec![tier_at(Layer::User, "contained")])
        .expect("D6 constrains the project layer, not the user's own grant");
    assert_eq!(
        ResolvedTier::from_configuration(&users_own)
            .expect("a tier was set")
            .tier(),
        Tier::Contained,
    );

    // --- ADR-0010 D1: the session records it, and cannot then change it.
    let workspace = nonce("workspace");
    let meta = Meta::new(
        tier,
        Some(workspace.clone()),
        Some("a-provider".to_owned()),
        std::path::PathBuf::from("/tmp/somewhere"),
        Millis::new(1_700_000_000_000),
    );
    assert_eq!(meta.tier(), Tier::Linked);
    assert_eq!(meta.resolved_tier().supplied_by(), Layer::Flag);

    let mut held = Recorded::default();
    held.write(&meta).expect("the port refused a write");
    let read_back = held.read().expect("the port lost what it was given");
    assert_eq!(
        read_back.tier(),
        Tier::Linked,
        "the tier a session recorded is not the tier that was resolved",
    );

    // --- D1: what the tier engages, for every tier.
    for candidate in Tier::ALL {
        let row = candidate.engagement();
        println!(
            "{candidate}: membrane {}, loop {}, cortex {}, network {}",
            row.membrane, row.r#loop, row.cortex, row.network,
        );
        assert_eq!(
            candidate.has_membrane(),
            candidate != Tier::Bare,
            "ADR-0011 D2 gives `bare` no enforcement and the other two a membrane",
        );
    }

    // --- D3: the ceilings, for every tier and both of its columns.
    for candidate in Tier::ALL {
        for inference in Inference::ALL {
            for placement in Placement::ALL {
                let count = iterations(candidate, inference, placement);
                let taken = ceiling(candidate, inference, placement);
                assert_eq!(
                    count,
                    taken.map(zaru_core::iteration::Ceiling::get),
                    "({candidate}, {inference}, {placement}): the table and the ceiling disagree",
                );
                println!("{candidate} / {inference} / {placement}: {count:?}");
            }
        }
    }
    assert_eq!(
        iterations(Tier::Linked, Inference::Frontier, Placement::Offloaded),
        Some(12),
        "ADR-0001 D3's largest cell",
    );
    assert_eq!(
        iterations(Tier::Contained, Inference::Local, Placement::Offloaded),
        None,
        "nothing offloads at `contained`, which is D1's Loop column",
    );

    // --- D2's datum, including what changing the tier would alter.
    let datum = Runtime::of(tier);
    assert_eq!(datum.tier, Tier::Linked);
    assert_eq!(datum.would_change.len(), Tier::ALL.len() - 1);
    for (other, differences) in &datum.would_change {
        let rendered: Vec<String> = differences
            .iter()
            .map(|change| format!("{} {} -> {}", change.column, change.here, change.there))
            .collect();
        println!("moving to {other} would alter: {}", rendered.join("; "));
    }
    assert_eq!(
        datum
            .would_change_to(Tier::Bare)
            .expect("bare is another tier")
            .len(),
        4,
        "every one of ADR-0001 D1's four columns differs between `linked` and `bare`",
    );

    // The planted workspace went to a port with no product implementation, so
    // it reached no file. Both the raw value and its ASCII core, because a
    // formatter that escaped rather than redacted would defeat the first arm
    // alone ([Verification lessons] §50).
    let about_the_session = format!("{tier:?} {datum:?} {refusal}");
    assert!(!about_the_session.contains(&workspace));
    assert!(!about_the_session.contains(ascii_core(&workspace)));
}

/// An unset tier is refused from outside too, and the refusal names the key.
#[test]
fn an_outside_caller_is_refused_a_tier_no_layer_set() {
    let empty = Resolution::resolve(&schema(), Vec::new()).expect("an empty fold resolves");
    let refusal = ResolvedTier::from_configuration(&empty).expect_err("no layer set the tier");
    assert!(matches!(refusal, TierRefused::NotSet { .. }), "{refusal:?}");

    let rendered = refusal.to_string();
    assert!(rendered.contains("runtime.tier"), "{rendered}");
    assert!(
        rendered.contains("names no default tier"),
        "the refusal must say that no default was chosen, because choosing one would hand a user \
         with a typo the tier with no membrane: {rendered}",
    );
    println!("{rendered}");
}

/// **ADR-0001 D2's immutability, as a property of the product tree.**
///
/// D2: "A membrane that can be dropped mid-session is not a membrane." The
/// unit checks show a session's tier has no setter; this shows there is no
/// *other* route either, by reading the product sources rather than by
/// asserting about the types a check happens to hold.
///
/// Three properties, each scoped to the thing it is about rather than to a
/// substring anywhere in a file. The `struct Meta` block declares no `pub`
/// tier. The `impl Meta` and `impl ResolvedTier` blocks declare no method
/// taking `&mut self`. And nothing under `src/runtime/` names an
/// interior-mutability type, which is the route a `&self` setter would take.
///
/// **The scoping is the check rather than a detail.** Its first version
/// searched whole files and reported two routes that are not routes: the
/// `Runtime` datum's own `pub tier`, which is a snapshot a renderer reads and
/// not a session's state, and `MetaStore::write(&mut self)`, which is the
/// port's method and not `Meta`'s. A check that cannot tell those from a
/// setter would be satisfied by removing them, which would make the harness
/// worse.
///
/// **What this does not prove**, stated rather than glossed: a caller may
/// still resolve twice and hold two `ResolvedTier` values. What is
/// unrepresentable is a *session's* tier changing, which is what D2 and
/// ADR-0010 D2 state.
///
/// The mutant: making `Meta`'s tier public again, or adding a `set_tier`.
#[test]
fn no_product_source_offers_a_way_to_change_a_sessions_tier() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<String> = Vec::new();
    let mut blocks_read: Vec<&str> = Vec::new();
    let mut runtime_files = 0usize;

    for (path, source) in product_sources(&src) {
        let name = path.file_name().unwrap_or_default().to_owned();
        let in_runtime = path.components().any(|part| part.as_os_str() == "runtime");

        if name == "meta.rs" {
            let fields = block(&source, "pub struct Meta {")
                .expect("session::meta declares `pub struct Meta`");
            blocks_read.push("struct Meta");
            for (number, line) in fields {
                if line.trim_start().starts_with("pub tier") {
                    offenders.push(format!(
                        "{}:{number}: `Meta` declares a public tier field",
                        path.display()
                    ));
                }
            }

            let methods =
                block(&source, "impl Meta {").expect("session::meta declares `impl Meta`");
            blocks_read.push("impl Meta");
            for (number, line) in methods {
                if line.contains("&mut self") {
                    offenders.push(format!(
                        "{}:{number}: `Meta` has a method taking &mut self",
                        path.display()
                    ));
                }
            }
        }

        if in_runtime {
            runtime_files += 1;

            if let Some(methods) = block(&source, "impl ResolvedTier {") {
                blocks_read.push("impl ResolvedTier");
                for (number, line) in methods {
                    if line.contains("&mut self") {
                        offenders.push(format!(
                            "{}:{number}: `ResolvedTier` has a method taking &mut self",
                            path.display()
                        ));
                    }
                }
            }

            for (number, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                for interior in [
                    "RefCell",
                    "Cell<",
                    "Mutex",
                    "RwLock",
                    "AtomicU",
                    "UnsafeCell",
                ] {
                    if code.contains(interior) {
                        offenders.push(format!(
                            "{}:{}: interior mutability via {interior}",
                            path.display(),
                            number + 1
                        ));
                    }
                }
            }
        }
    }

    // The staging, asserted rather than assumed: without it every clause above
    // is vacuously true over a walk that found nothing
    // ([Verification lessons] §4).
    blocks_read.sort_unstable();
    assert_eq!(
        blocks_read,
        vec!["impl Meta", "impl ResolvedTier", "struct Meta"],
        "the scan did not reach all three blocks it is about",
    );
    assert!(
        runtime_files >= 4,
        "the scan reached {runtime_files} files under src/runtime/, which is a broken walk rather \
         than a clean tree",
    );

    assert!(
        offenders.is_empty(),
        "{} route(s) exist by which a session's runtime tier could change. ADR-0001 D2: \"Tier is \
         resolved at session start and is immutable for the life of a session... A membrane that \
         can be dropped mid-session is not a membrane.\" Found: {offenders:#?}",
        offenders.len(),
    );
    println!(
        "no route to change a session's tier: 3 blocks read, {runtime_files} runtime source \
         file(s) scanned for interior mutability"
    );
}

/// The lines inside the first block opened by `header`, with their 1-based
/// numbers.
///
/// Brace counting rather than a parser, which is enough because every block
/// this check reads is ordinary Rust with balanced braces in its source form.
/// `None` when the header is absent, so a caller can tell "no such block" from
/// "an empty one" -- the same distinction ADR-0010 D1 draws for `ended`.
fn block<'a>(source: &'a str, header: &str) -> Option<Vec<(usize, &'a str)>> {
    let mut lines = source.lines().enumerate();
    let opened = lines.find(|(_, line)| line.trim_start().starts_with(header))?;

    let mut depth = 1i32;
    let mut inside = Vec::new();
    for (index, line) in lines {
        depth += i32::try_from(line.matches('{').count()).expect("a short line");
        depth -= i32::try_from(line.matches('}').count()).expect("a short line");
        if depth <= 0 {
            break;
        }
        inside.push((index + 1, line));
    }
    let _ = opened;
    Some(inside)
}

/// Every `.rs` file under a directory that is not a test tree, with its text.
fn product_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut found = Vec::new();
    let mut frontier = vec![root.to_path_buf()];

    while let Some(directory) = frontier.pop() {
        let entries = std::fs::read_dir(&directory).expect("a source directory is readable");
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                frontier.push(path);
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            // The checks and their fixtures are not the product.
            if path
                .file_name()
                .is_some_and(|name| name == "tests.rs" || name == "fixtures.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a source file is readable");
            found.push((path, text));
        }
    }

    found
}

/// The part of a nonce no formatter can alter.
fn ascii_core(value: &str) -> &str {
    value
        .strip_suffix("-e\u{301}\u{e9}\u{1f701}")
        .unwrap_or(value)
}

/// A [`MetaStore`] this check owns.
///
/// The product implements none — ADR-0003 D2's table names no TOML crate — so
/// an outside caller standing in for that implementation is what shows the
/// port carries what ADR-0010 D1 asks of it.
#[derive(Default)]
struct Recorded {
    held: Option<Meta>,
}

impl MetaStore for Recorded {
    fn write(&mut self, meta: &Meta) -> Result<(), MetaFailure> {
        self.held = Some(meta.clone());
        Ok(())
    }

    fn read(&self) -> Result<Meta, MetaFailure> {
        self.held
            .clone()
            .ok_or_else(|| MetaFailure::new("nothing has been recorded for this session"))
    }
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
