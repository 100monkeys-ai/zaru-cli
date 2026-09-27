// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives ADR-0009's manifest from outside the crate, over real files.
//!
//! Two things this reaches that no unit check can. It drives the product's own
//! [`ManifestFile`] **from outside `zaru-cli`**, using only what the crate
//! exports, over a real `zaru.toml` on disk — every value asserted below came
//! off that file through the parser rather than out of this one. And it takes
//! the manifest all the way through [ADR-0014]'s fold as layer 3, so that
//! "`[runtime]` reaches the hierarchy with no second declaration of
//! `max_iterations`" is a run rather than a claim: the key is spelled in the
//! [`Schema`] this file builds and nowhere else, and the value comes back out
//! of a [`Resolution`].
//!
//! **Until 2026-09-05 this file implemented [`ManifestSource`] itself**, with a
//! reader that found the ceiling by string-matching a line, because ADR-0003
//! D2's table named no TOML crate. That implementation is deleted: the product
//! has one, and a check standing in for a parser that now exists would be
//! asserting about a fake.
//!
//! The working directory and the manifest live on a scratch root this check
//! owns, at the paths the product would actually read, and the root is removed
//! afterwards with a sibling control that must survive.
//!
//! **Evidence about the mechanism, not about the `zaru` binary**, which reaches
//! none of this.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::{Path, PathBuf};
use zaru_cli::config::{
    ConfigRefused, Contribution, Key, Layer, Resolution, Schema, SizeCeiling, Source, Table, Value,
};
use zaru_cli::failure::Statement;
use zaru_cli::manifest::{
    MANIFEST_FILE, Manifest, ManifestFile, ManifestRefused, ManifestSource, MissingManifest,
};
use zaru_cli::tools::WorkingDirectory;
use zaru_core::iteration::validator::{Declared, Expect, Name, Plan, Run, SchemaPath};

/// A tree this check owns, with a sibling that must survive its removal.
struct ScratchRoot {
    base: PathBuf,
}

/// Distinguishes two scratch roots taken inside one clock tick.
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl ScratchRoot {
    fn new() -> Self {
        // **The counter is what makes this unique, and its absence was a
        // flake.** A process id and a nanosecond reading are not enough: the
        // test harness runs these on several threads at once, two of them can
        // read the same coarse clock tick, and `create_dir_all` succeeds for
        // both. The first to finish then drops and removes the directory the
        // second is still writing into, and the second fails with `NotFound`
        // on a path it created itself -- observed 2026-09-14 as
        // `could not write the manifest: Os { code: 2, kind: NotFound }`.
        //
        // The counter is the shape `credentials::fixtures::nonce` already
        // uses, for the reason its own comment gives: "Distinguishes two
        // nonces taken inside one clock tick."
        let unique = format!(
            "mv-outside-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the system clock is before the unix epoch")
                .as_nanos(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(unique);
        std::fs::create_dir_all(base.join("project").join("schema"))
            .expect("could not stage the project");
        std::fs::create_dir_all(base.join("control")).expect("could not stage the control");
        std::fs::create_dir_all(base.join("elsewhere")).expect("could not stage the neighbour");
        std::fs::write(
            base.join("project").join("schema").join("output.json"),
            b"{}",
        )
        .expect("could not stage the schema");
        std::fs::write(base.join("elsewhere").join("secret.json"), b"{}")
            .expect("could not stage the neighbour's file");
        std::fs::write(base.join("control").join("keep"), b"survives")
            .expect("could not stage the control");
        Self { base }
    }

    /// The working directory a manifest is measured against.
    fn project(&self) -> PathBuf {
        self.base.join("project")
    }

    /// The sibling that must survive removal.
    fn control(&self) -> PathBuf {
        self.base.join("control")
    }

    fn base(&self) -> &Path {
        &self.base
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// ADR-0009 D1's manifest, staged on disk and read by the product's own reader.
///
/// The ceiling is a required argument because the whole point of this file is
/// that the value the fold resolves came off a file rather than out of a
/// literal in the caller.
fn staged(scratch: &ScratchRoot, body: &str) -> ManifestFile {
    std::fs::write(scratch.project().join(MANIFEST_FILE), body)
        .expect("could not write the manifest");
    reader(scratch)
}

/// The reader for a project that may or may not have a manifest.
fn reader(scratch: &ScratchRoot) -> ManifestFile {
    ManifestFile::in_directory(
        WorkingDirectory::at(scratch.project()).expect("the project directory exists"),
        SizeCeiling::new(1 << 20).expect("a mebibyte is not zero"),
    )
}

/// The keys the manifest's own values resolve against.
///
/// **Every one arrives from the record that owns it**, which is ADR-0014's
/// Neutral section: `project.name` and `project.workspace` from
/// `zaru_cli::manifest::declare`, `runtime.tier` and `runtime.max_iterations`
/// from `zaru_cli::runtime`. Until 2026-09-05 this file spelled two of them
/// itself, which was one record's key written inside another check.
fn schema() -> Schema {
    zaru_cli::manifest::declare(Schema::new())
        .with(zaru_cli::runtime::key(), zaru_cli::runtime::field())
        .with(
            zaru_cli::runtime::max_iterations_key(),
            zaru_cli::runtime::max_iterations_field(),
        )
}

/// ADR-0009 D1's three validators, as `[[validator]]` text.
///
/// **Declared with the dependent first**, and that is the check rather than a
/// detail. Its first version listed them in the record's own order, which is
/// already dependency order -- so the mutation that sorts by declaration index
/// produced exactly the expected answer and survived. Verification lessons §9
/// and §55, found by running: a fixture lifted from a record's worked example
/// is already in the answer's shape.
fn declared_validators() -> &'static str {
    r#"
[[validator]]
name = "shape"
run  = "cargo run -- --emit-schema"
expect = { json_schema = "schema/output.json" }
after = ["build"]

[[validator]]
name = "test"
run  = "cargo test --all"
expect = "exit-zero"
after = ["build"]

[[validator]]
name = "build"
run  = "cargo build --locked"
expect = "exit-zero"
"#
}

#[test]
fn a_caller_outside_the_crate_reads_a_manifest_and_resolves_its_runtime_as_layer_three() {
    let scratch = ScratchRoot::new();

    // ADR-0009 D1's own file, on disk, at the path the record names, read by
    // the product's own reader.
    let source = staged(
        &scratch,
        &format!(
            "[project]\nname = \"acme-api\"\nworkspace = \"acme-engineering\"\n\n\
             [runtime]\nmax_iterations = 5\n{}",
            declared_validators()
        ),
    );
    let manifest = source
        .read()
        .expect("the manifest reads")
        .expect("the file is there, so there is a manifest");

    // The three validators become a dependency order, which is ADR-0009 D2's
    // and lives in `zaru-core`. `shape` and `test` both come after `build`.
    let plan = Plan::from_declared(manifest.validators().to_vec()).expect("they resolve");
    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    assert_eq!(
        order,
        vec!["build", "shape", "test"],
        "the manifest declares shape, test, build; ADR-0009 D2 runs `build` first because both \
         others come after it, and the remaining two in the order the file gave them"
    );

    // Layer 1's compiled-in default, which the project may only lower.
    let mut built_in = Table::new();
    let mut runtime = Table::new();
    runtime.insert("max_iterations", Value::Integer(8));
    built_in.insert("runtime", Value::Table(runtime));

    let resolution = Resolution::resolve(
        &schema(),
        vec![
            Contribution::new(Layer::BuiltIn, Source::named("built-in"), built_in),
            manifest.contribution(source.source()),
        ],
    )
    .expect("the manifest is a legal layer 3");

    // The value came off disk, through the manifest, through the fold, and
    // out of a key this file declared once.
    let ceiling = Key::new("runtime.max_iterations").expect("a key");
    assert_eq!(
        resolution.get(&ceiling),
        Some(&Value::Integer(5)),
        "ADR-0009 D1's `[runtime] max_iterations = 5` is ADR-0014 D1's layer 3, and it lowered \
         the built-in 8"
    );

    // ADR-0014 D3's explain block names the manifest as the source, which is
    // what makes the two records one file to a reader.
    let explanation = resolution.explain(&ceiling);
    assert_eq!(
        explanation.effective_layer(),
        Some(Layer::Project),
        "the ceiling must come from LAYER 3; a manifest offered at any other layer still wins \
         over the built-in default and still prints its own file name, so a check that only read \
         the rendered text could not tell the difference -- which is what it did until this \
         mutation was run"
    );
    let block = explanation.to_string();
    let marked: Vec<&str> = block
        .lines()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(marked.len(), 1, "exactly one row is marked:\n{block}");
    assert!(
        marked[0].trim_start().starts_with("3 ") && marked[0].contains("zaru.toml"),
        "the marked row is layer 3 and names the manifest:\n{block}"
    );
    // And `[project]` arrived at its dotted key too.
    assert_eq!(
        resolution.get(&Key::new("project.workspace").expect("a key")),
        Some(&Value::Text("acme-engineering".to_owned())),
    );

    println!("{block}");
    println!("validators, in dependency order: {order:?}");
}

#[test]
fn a_manifest_that_raises_the_ceiling_is_refused_by_adr_0014_d6() {
    // The manifest is layer 3, so D6's ceiling applies to it exactly as it
    // does to any other project contribution. Nothing in `zaru-cli`'s manifest
    // module re-implements that rule -- this check is what says so.
    let scratch = ScratchRoot::new();
    let source = staged(&scratch, "[runtime]\nmax_iterations = 99\n");
    let manifest = source.read().expect("it reads").expect("it is there");

    let mut built_in = Table::new();
    let mut runtime = Table::new();
    runtime.insert("max_iterations", Value::Integer(8));
    built_in.insert("runtime", Value::Table(runtime));

    let refusal = Resolution::resolve(
        &schema(),
        vec![
            Contribution::new(Layer::BuiltIn, Source::named("built-in"), built_in),
            manifest.contribution(source.source()),
        ],
    )
    .expect_err("ADR-0014 D6: a project may LOWER its own iteration ceiling");

    let ConfigRefused::ProjectMayNotRaise { granted, asked, .. } = &refusal else {
        panic!("expected D6's ceiling refusal, got {refusal:?}");
    };
    assert_eq!((*granted, *asked), (8, 99));
    println!("{refusal}");
}

#[test]
fn a_manifest_whose_schema_path_leaves_the_working_directory_is_refused_from_outside() {
    // The security-corpus case, driven through the crate's public door with a
    // real directory on disk and a real neighbour to escape to.
    let scratch = ScratchRoot::new();
    let working_directory =
        WorkingDirectory::at(scratch.project()).expect("the project directory exists");

    let escaping = Declared::new(
        Name::new("shape").expect("a name"),
        Run::new("cargo run -- --emit-schema").expect("a command"),
        Expect::JsonSchema(SchemaPath::new("../elsewhere/secret.json").expect("a path")),
    );

    let refusal = Manifest::build(
        Table::new(),
        Table::new(),
        vec![escaping],
        &working_directory,
    )
    .expect_err("a schema path outside the working directory is refused");

    let ManifestRefused::SchemaPathLeavesTheWorkingDirectory {
        validator,
        declared,
        resolved,
        ..
    } = &refusal;
    assert_eq!(validator.as_str(), "shape");
    assert_eq!(declared, "../elsewhere/secret.json");
    assert_eq!(
        resolved,
        &scratch.base().join("elsewhere").join("secret.json"),
        "the refusal names where the path actually reached rather than how it was spelled"
    );
    println!("{refusal}");
}

#[test]
fn a_project_with_no_manifest_reads_as_absent_and_is_owed_one_line() {
    // ADR-0009 D4's whole condition, from outside: the file is not there, the
    // source says so rather than erroring, and the line is owed once.
    let scratch = ScratchRoot::new();
    let source = reader(&scratch);
    assert!(!source.path().exists(), "this project has no manifest");

    let absent = source.read().expect("an absent manifest is not a failure");
    assert!(
        absent.is_none(),
        "ADR-0009 D4 makes a project with no manifest an ordinary thing; a reader that errored \
         would make that clause unreachable"
    );

    let mut owed = MissingManifest::for_manifest(
        absent.as_ref(),
        Statement::new("no validators are declared, so the iteration loop cannot run")
            .expect("a statement"),
        Statement::new("declare one in ./zaru.toml").expect("a statement"),
    )
    .expect("a project with no manifest is owed the line");

    let line = owed.state_once().expect("owed once");
    println!("{line}");
    assert_eq!(owed.state_once(), None, "and never again");
}

#[test]
fn the_scratch_root_is_removed_and_its_absence_reads_four_ways() {
    let scratch = ScratchRoot::new();
    let removed = scratch.project();
    let control = scratch.control();
    let inside = removed.join("schema").join("output.json");
    let parent = scratch.base().to_path_buf();

    assert!(
        removed.exists() && inside.exists() && control.exists(),
        "staged"
    );
    std::fs::remove_dir_all(&removed).expect("could not remove the project directory");

    // 1. The directory itself.
    assert!(!removed.exists(), "still there: {}", removed.display());
    // 2. The control, which is the reading that discriminates: a checker
    //    reporting absence for everything would fail here.
    assert!(
        control.exists(),
        "the sibling control was removed too, so nothing above says anything"
    );
    // 3. The parent's listing, by name.
    let mut remaining: Vec<String> = std::fs::read_dir(&parent)
        .expect("the parent is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    remaining.sort();
    assert_eq!(remaining, vec!["control", "elsewhere"]);
    // 4. A read of a file that was inside it, by error kind: NotFound rather
    //    than PermissionDenied, which would mean it is still there.
    let error = std::fs::read(&inside).expect_err("the file inside is gone");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn adr_0009_d1s_corrected_manifest_is_accepted_from_a_real_file_and_a_tier_is_still_refused() {
    // **Re-transcribed 2026-09-05.** This check used to stage ADR-0009 D1's
    // manifest as it was then written -- `[runtime] tier = "contained"` -- and
    // assert that ADR-0014 D6 refused it, pinning a contradiction three records
    // carried. Directive 20 resolved it: D6 gave nothing, D1's example dropped
    // its tier and now lowers a ceiling instead, and ADR-0001 D2 was corrected
    // in the same change. That record says the pinning checks are re-transcribed
    // against the corrected examples by the arcs that own those modules, and
    // this is that.
    //
    // So the assertion inverts and gains a half. The corrected manifest is
    // **accepted** -- read off a real file, folded as layer 3, with its lowered
    // ceiling effective -- and a manifest that sets a tier is **still refused**,
    // which is what stops the correction from being read as D6 softening.
    let scratch = ScratchRoot::new();

    let accepted = staged(
        &scratch,
        "[project]\nname = \"acme-api\"\nworkspace = \"acme-engineering\"\n\n\
         [runtime]\nmax_iterations = 3\n",
    );
    let manifest = accepted
        .read()
        .expect("D1's corrected manifest is a manifest")
        .expect("the file is there");

    let mut built_in = Table::new();
    let mut defaults = Table::new();
    defaults.insert("max_iterations", Value::Integer(5));
    built_in.insert("runtime", Value::Table(defaults));

    let resolution = Resolution::resolve(
        &schema(),
        vec![
            Contribution::new(Layer::BuiltIn, Source::named("built-in"), built_in.clone()),
            manifest.contribution(accepted.source()),
        ],
    )
    .expect("ADR-0014 D6 permits a project to lower its own iteration ceiling");
    assert_eq!(
        resolution.get(&Key::new("runtime.max_iterations").expect("a key")),
        Some(&Value::Integer(3)),
        "D1's corrected example lowers the built-in 5 to 3, which is the case D6 permits in as \
         many words"
    );
    println!(
        "{}",
        resolution.explain(&Key::new("runtime.max_iterations").expect("a key"))
    );

    // The half that keeps the correction honest: a tier in a project file is
    // still refused, from a real file, naming the file.
    let with_a_tier = staged(
        &scratch,
        "[runtime]\ntier = \"contained\"\nmax_iterations = 3\n",
    );
    let manifest = with_a_tier
        .read()
        .expect("a tier is a well-formed manifest; the refusal is D6's, not the reader's")
        .expect("the file is there");
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            Contribution::new(Layer::BuiltIn, Source::named("built-in"), built_in),
            manifest.contribution(with_a_tier.source()),
        ],
    )
    .expect_err("ADR-0014 D6 refuses a project setting the runtime tier");

    let ConfigRefused::ProjectMayNotSet { key, .. } = &refusal else {
        panic!("expected D6's escalation refusal, got {refusal:?}");
    };
    assert_eq!(key.as_str(), "runtime.tier");
    assert!(
        with_a_tier
            .path()
            .to_string_lossy()
            .ends_with(&format!("/{MANIFEST_FILE}")),
        "the refusal is about the project's own file: {}",
        with_a_tier.path().display()
    );
    println!("{refusal}");
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
