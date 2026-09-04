// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives ADR-0009's manifest from outside the crate, over real files.
//!
//! Two things this reaches that no unit check can. It implements
//! [`ManifestSource`] **from outside `zaru-cli`**, using only what the crate
//! exports — a mechanism whose only implementations are inside the crate is
//! one nobody has been shown able to supply ([Verification lessons] §25). And
//! it takes a manifest all the way through [ADR-0014]'s fold as layer 3, so
//! that "`[runtime]` reaches the hierarchy with no second declaration of
//! `max_iterations`" is a run rather than a claim: the key is spelled in the
//! [`Schema`] this file builds and nowhere else, and the value comes back out
//! of a [`Resolution`].
//!
//! The working directory and the manifest live on a scratch root this check
//! owns, at the paths the product would actually read, and the root is removed
//! afterwards with a sibling control that must survive.
//!
//! **Evidence about the mechanism, not about the `zaru` binary**, which
//! reaches none of it, and not about TOML, which is never parsed: the source
//! implemented here builds a `Manifest` from values this file chose.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::{Path, PathBuf};
use zaru_cli::config::{ConfigRefused, Schema};
use zaru_cli::config::{
    Contribution, Field, FieldKind, Key, Layer, Resolution, Source, SourceFailure, Table, Value,
};
use zaru_cli::failure::Statement;
use zaru_cli::manifest::{Manifest, ManifestRefused, ManifestSource, MissingManifest};
use zaru_cli::tools::WorkingDirectory;
use zaru_core::iteration::validator::{Declared, Expect, Name, Plan, Run, SchemaPath};

/// A tree this check owns, with a sibling that must survive its removal.
struct ScratchRoot {
    base: PathBuf,
}

impl ScratchRoot {
    fn new() -> Self {
        let unique = format!(
            "mv-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the system clock is before the unix epoch")
                .as_nanos(),
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

/// A `zaru.toml` reader, implemented from outside `zaru-cli`.
///
/// It reads the file to decide whether the project HAS a manifest — which is
/// the only question ADR-0009 D4 needs answered and the one a caller would
/// really ask the filesystem — and builds the manifest from values this check
/// chose, because **no TOML parser exists anywhere in this workspace**.
/// ADR-0003 D2's table names none and the amendment that would add one is
/// proposed and not accepted.
struct FileBackedSource {
    working_directory: WorkingDirectory,
    path: PathBuf,
    validators: Vec<Declared>,
}

impl ManifestSource for FileBackedSource {
    fn source(&self) -> Source {
        Source::named(self.path.display().to_string())
    }

    fn read(&self) -> Result<Option<Manifest>, SourceFailure> {
        if !self.path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&self.path)
            .map_err(|error| SourceFailure::new(format!("{}: {error}", self.path.display())))?;
        // The one thing this reads out of the bytes: the ceiling, so that the
        // value the fold resolves came off disk rather than out of this file.
        let ceiling: i64 = text
            .lines()
            .find_map(|line| line.strip_prefix("max_iterations = "))
            .ok_or_else(|| SourceFailure::new("the staged manifest names no ceiling"))?
            .trim()
            .parse()
            .map_err(|_| SourceFailure::new("the staged ceiling is not a whole number"))?;

        let mut runtime = Table::new();
        runtime.insert("max_iterations", Value::Integer(ceiling));
        let mut project = Table::new();
        project.insert("workspace", Value::Text("acme-engineering".to_owned()));

        Manifest::build(
            project,
            runtime,
            self.validators.clone(),
            &self.working_directory,
        )
        .map(Some)
        .map_err(|refusal| SourceFailure::new(refusal.to_string()))
    }
}

/// The keys a caller declares. **This is the only place `max_iterations` is
/// spelled**, which is the whole point of the manifest not having a field for
/// it: ADR-0014's Neutral section says each record owns its own keys.
fn schema() -> Schema {
    Schema::new()
        .with(
            Key::new("runtime.max_iterations").expect("a key"),
            // ADR-0014 D6: a project may lower its own iteration ceiling.
            Field::ceiling(),
        )
        .with(
            Key::new("project.workspace").expect("a key"),
            Field::free(FieldKind::Text),
        )
        .with(
            Key::new("runtime.tier").expect("a key"),
            // ADR-0014 D6's escalation, declared by whoever owns the key.
            Field::refused_to_projects(FieldKind::Text, "the runtime tier is the user's to choose"),
        )
}

/// ADR-0009 D1's three validators, with the schema path inside the tree.
///
/// **Declared with the dependent first**, and that is the check rather than a
/// detail. Its first version listed them in the record's own order, which is
/// already dependency order -- so the mutation that sorts by declaration index
/// produced exactly the expected answer and survived. Verification lessons §9,
/// found by running: a fixture whose declaration order and dependency order
/// coincide cannot tell the two apart.
fn declared_validators() -> Vec<Declared> {
    vec![
        Declared::new(
            Name::new("shape").expect("a name"),
            Run::new("cargo run -- --emit-schema").expect("a command"),
            Expect::JsonSchema(SchemaPath::new("schema/output.json").expect("a path")),
        )
        .after([Name::new("build").expect("a name")]),
        Declared::new(
            Name::new("test").expect("a name"),
            Run::new("cargo test --all").expect("a command"),
            Expect::ExitZero,
        )
        .after([Name::new("build").expect("a name")]),
        Declared::new(
            Name::new("build").expect("a name"),
            Run::new("cargo build --locked").expect("a command"),
            Expect::ExitZero,
        ),
    ]
}

#[test]
fn a_caller_outside_the_crate_reads_a_manifest_and_resolves_its_runtime_as_layer_three() {
    let scratch = ScratchRoot::new();
    let working_directory =
        WorkingDirectory::at(scratch.project()).expect("the project directory exists");
    let manifest_path = scratch.project().join("zaru.toml");

    // ADR-0009 D1's own file, on disk, at the path the record names.
    std::fs::write(
        &manifest_path,
        "[project]\nname = \"acme-api\"\n\n[runtime]\nmax_iterations = 5\n",
    )
    .expect("could not write the manifest");

    let source = FileBackedSource {
        working_directory,
        path: manifest_path.clone(),
        validators: declared_validators(),
    };
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
    let working_directory =
        WorkingDirectory::at(scratch.project()).expect("the project directory exists");
    let manifest_path = scratch.project().join("zaru.toml");
    std::fs::write(&manifest_path, "[runtime]\nmax_iterations = 99\n")
        .expect("could not write the manifest");

    let source = FileBackedSource {
        working_directory,
        path: manifest_path,
        validators: Vec::new(),
    };
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
    let working_directory =
        WorkingDirectory::at(scratch.project()).expect("the project directory exists");
    let manifest_path = scratch.project().join("zaru.toml");
    assert!(!manifest_path.exists(), "this project has no manifest");

    let source = FileBackedSource {
        working_directory,
        path: manifest_path,
        validators: Vec::new(),
    };
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
