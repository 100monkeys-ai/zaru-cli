// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks over ADR-0009 D1, D4 and D6, and over D3's path against ADR-0011 D4.

use crate::config::{Key, Layer, Source, Table, Value};
use crate::failure::Statement;
use crate::manifest::absent::MissingManifest;
use crate::manifest::document::{Manifest, ManifestRefused, PROJECT_TABLE, RUNTIME_TABLE};
use crate::tools::WorkingDirectory;
use crate::tools::fixtures::ScratchTree;
use zaru_core::iteration::validator::{Declared, Expect, Name, Run, SchemaPath};

/// A validator expecting a schema at `path`.
fn expecting_schema(name: &str, path: &str) -> Declared {
    Declared::new(
        Name::new(name).expect("a fixture name"),
        Run::new(format!("run-{name}")).expect("a fixture command"),
        Expect::JsonSchema(SchemaPath::new(path).expect("a fixture path")),
    )
}

/// A validator expecting exit zero, which names no path at all.
fn expecting_exit_zero(name: &str) -> Declared {
    Declared::new(
        Name::new(name).expect("a fixture name"),
        Run::new(format!("run-{name}")).expect("a fixture command"),
        Expect::ExitZero,
    )
}

/// A table with one key under one name.
fn table(name: &str, value: Value) -> Table {
    let mut built = Table::new();
    built.insert(name, value);
    built
}

// --- ADR-0009 D3's path against ADR-0011 D4's boundary ---------------------
//
// For the security corpus, which only grows.

#[test]
fn a_schema_path_that_leaves_the_working_directory_is_refused() {
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");

    // Five spellings of one escape, and each is here because a different
    // wrong implementation accepts it. A rule that only refused a leading
    // slash takes the first four; a byte-wise prefix test takes `projectevil`;
    // a purely lexical normalisation takes the symlink.
    let hostile = [
        ("dotdot", "../elsewhere/secret.json"),
        ("absolute", "/etc/passwd"),
        ("deep-dotdot", "inside/../../elsewhere/secret.json"),
        ("through-a-symlink", "escape/secret.json"),
        (
            "sibling-prefix",
            tree.base()
                .join("projectevil")
                .join("loot.json")
                .to_str()
                .expect("a utf-8 path")
                .to_owned()
                .leak(),
        ),
    ];

    let mut accepted = Vec::new();
    for (label, path) in hostile {
        let refusal = Manifest::build(
            Table::new(),
            Table::new(),
            vec![expecting_schema("shape", path)],
            &working_directory,
        );
        match refusal {
            Err(ManifestRefused::SchemaPathLeavesTheWorkingDirectory {
                validator,
                declared,
                resolved,
                ..
            }) => {
                assert_eq!(validator.as_str(), "shape");
                assert_eq!(declared, path, "the refusal quotes the declared spelling");
                assert!(
                    !resolved.starts_with(working_directory.root()),
                    "{label}: the refusal claims a path outside the tree that is inside it: {}",
                    resolved.display()
                );
            }
            Ok(_) => accepted.push(format!("{label} ({path})")),
        }
    }

    // Every validator is measured, not only the first. A manifest whose
    // escaping declaration is the second one is the ordinary shape -- ADR-0009
    // D1's own example puts `json_schema` last -- and a loop that stopped at
    // the first validator would take it.
    let second = Manifest::build(
        Table::new(),
        Table::new(),
        vec![
            expecting_exit_zero("build"),
            expecting_schema("shape", "../elsewhere/secret.json"),
        ],
        &working_directory,
    );
    if second.is_ok() {
        accepted.push("second-validator (../elsewhere/secret.json)".to_owned());
    }

    // Every case reported, not the first: a refusal that reports one escape
    // and stops leaves the others unmeasured (Verification lessons §36).
    assert!(
        accepted.is_empty(),
        "ADR-0009 D3's `json_schema` path arrives from a cloned repository, and {} of the 6 \
         hostile spellings were accepted: {accepted:?}",
        accepted.len()
    );
}

#[test]
fn a_schema_path_inside_the_working_directory_is_taken() {
    // The arm that makes the refusals mean anything. An implementation that
    // refuses every path passes every case above perfectly, which is
    // Verification lessons §13 -- an invariant holding because both sides are
    // wrong together.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");

    let mut refused = Vec::new();
    for (label, path) in [
        ("relative", "schema/output.json"),
        ("nested-existing", "inside/file"),
        ("dot-prefixed", "./schema/output.json"),
        ("down-and-back", "inside/../schema/output.json"),
    ] {
        let built = Manifest::build(
            Table::new(),
            Table::new(),
            vec![expecting_schema("shape", path)],
            &working_directory,
        );
        if let Err(refusal) = built {
            refused.push(format!("{label} ({path}): {refusal}"));
        }
    }
    assert!(
        refused.is_empty(),
        "a schema path below the working directory is ordinary, and {} of 4 were refused: \
         {refused:?}",
        refused.len()
    );
}

#[test]
fn the_working_directory_is_canonicalised_before_anything_is_measured_against_it() {
    // The root reached through a symlink. Without canonicalisation at
    // construction every later comparison compares two spellings of one
    // directory, and an ordinary in-tree path reads as an escape.
    let tree = ScratchTree::new();
    let by_link = WorkingDirectory::at(tree.project_by_link()).expect("the symlink resolves");
    let built = Manifest::build(
        Table::new(),
        Table::new(),
        vec![expecting_schema("shape", "inside/file")],
        &by_link,
    );
    assert!(
        built.is_ok(),
        "a working directory reached through a symlink must measure the same paths the same way: \
         {built:?}"
    );
}

#[test]
fn a_validator_that_names_no_path_is_not_measured_against_the_boundary() {
    // `exit-zero`, `exit-code` and `matches` carry no path, so there is
    // nothing to classify and nothing to refuse.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let built = Manifest::build(
        Table::new(),
        Table::new(),
        vec![expecting_exit_zero("build"), expecting_exit_zero("test")],
        &working_directory,
    );
    assert!(built.is_ok(), "{built:?}");
}

// --- ADR-0009 D1 against ADR-0014 D1's layer 3 ------------------------------

#[test]
fn the_manifest_contributes_project_and_runtime_at_layer_three() {
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let manifest = Manifest::build(
        table("workspace", Value::Text("acme-engineering".to_owned())),
        table("max_iterations", Value::Integer(5)),
        vec![expecting_exit_zero("build")],
        &working_directory,
    )
    .expect("ADR-0009 D1's own example, minus the tier");

    let contribution = manifest.contribution(Source::named("./zaru.toml"));
    assert_eq!(
        contribution.layer,
        Layer::Project,
        "ADR-0009 D1's `./zaru.toml` IS ADR-0014 D1's layer 3 -- one file, two records"
    );

    // The dotted keys a schema would declare, read back out of the document
    // rather than asked of the manifest that built it.
    for (dotted, expected) in [
        (
            "project.workspace",
            Value::Text("acme-engineering".to_owned()),
        ),
        ("runtime.max_iterations", Value::Integer(5)),
    ] {
        let key = Key::new(dotted).expect("a key");
        assert_eq!(
            contribution.document.get_path(&key),
            Some(&expected),
            "the layer-3 contribution does not carry {dotted}, which ADR-0014 D3 explains by name"
        );
    }
}

#[test]
fn the_validators_are_not_part_of_the_layer_three_contribution() {
    // The delegated ruling of 2026-09-04, pinned. `[[validator]]` is not a
    // configuration key, so ADR-0014 D2's wholesale array replacement never
    // reaches it. Proposed as an Update on ADR-0009; correcting the record the
    // other way reddens this.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let manifest = Manifest::build(
        Table::new(),
        Table::new(),
        vec![expecting_exit_zero("build"), expecting_exit_zero("test")],
        &working_directory,
    )
    .expect("two validators and no configuration");

    assert_eq!(manifest.validators().len(), 2, "the manifest carries them");
    let contribution = manifest.contribution(Source::named("./zaru.toml"));
    assert!(
        contribution.document.is_empty(),
        "a manifest whose only content is `[[validator]]` contributes nothing to the \
         configuration hierarchy, and this one contributed {:?}",
        contribution.document
    );
}

#[test]
fn the_manifest_names_no_configuration_key_of_its_own() {
    // ADR-0014's own instruction: "Nothing here specifies the schema. Each
    // record owns its own keys." The only names this module writes are the two
    // TABLE names D1 gives, and a check reads them from the constants rather
    // than retyping them.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let manifest = Manifest::build(
        table("name", Value::Text("acme-api".to_owned())),
        table("max_iterations", Value::Integer(5)),
        Vec::new(),
        &working_directory,
    )
    .expect("two tables");

    let contribution = manifest.contribution(Source::named("./zaru.toml"));
    let top: Vec<&String> = contribution.document.iter().map(|(name, _)| name).collect();
    assert_eq!(
        top,
        vec![PROJECT_TABLE, RUNTIME_TABLE],
        "the contribution's top level is exactly ADR-0009 D1's two table names; anything else \
         would be a key this module chose"
    );

    // And the leaves are the caller's, unchanged: nothing here renames,
    // defaults, or adds one.
    let key = Key::new("project.name").expect("a key");
    assert_eq!(
        contribution.document.get_path(&key),
        Some(&Value::Text("acme-api".to_owned()))
    );
}

// --- ADR-0009 D4 -----------------------------------------------------------

#[test]
fn the_missing_manifest_line_is_stated_once_and_never_again() {
    let unavailable = Statement::new("the iteration loop is unavailable").expect("a statement");
    let how = Statement::new("declare validators in ./zaru.toml").expect("a statement");
    let mut owed = MissingManifest::for_manifest(None, unavailable.clone(), how.clone())
        .expect("a project with no manifest is owed the line");

    assert!(owed.is_owed());
    let first = owed.state_once().expect("the line is owed the first time");
    assert_eq!(first.unavailable(), &unavailable);
    assert_eq!(first.how_to_get_it(), &how);
    assert!(
        first.to_string().contains(unavailable.as_str())
            && first.to_string().contains(how.as_str()),
        "ADR-0009 D4's single line names what is unavailable AND how to get it: {first}"
    );

    assert!(
        !owed.is_owed(),
        "the line is no longer owed after stating it"
    );
    assert_eq!(
        owed.state_once(),
        None,
        "ADR-0009 D4: the harness says so once \"and never mentions it again\"; a second call \
         produced a second line"
    );
    assert_eq!(owed.state_once(), None, "nor a third");
}

#[test]
fn a_project_with_a_manifest_is_owed_no_line_at_all() {
    // Absence rather than a branch: there is no value to state, so there is
    // nothing for a renderer to decide about.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let manifest = Manifest::build(Table::new(), Table::new(), Vec::new(), &working_directory)
        .expect("an empty manifest is a manifest");

    let owed = MissingManifest::for_manifest(
        Some(&manifest),
        Statement::new("the iteration loop is unavailable").expect("a statement"),
        Statement::new("declare validators in ./zaru.toml").expect("a statement"),
    );
    assert!(
        owed.is_none(),
        "a project WITH a manifest must not be told it has none"
    );
}

// --- ADR-0009 D6 -----------------------------------------------------------

#[test]
fn no_product_source_writes_a_manifest() {
    // ADR-0009 D6: "The manifest is read, never written." The type-level half
    // is that `ManifestSource` has one method and there is no counterpart --
    // the forbidden act has nothing to call. This is the other half: a
    // product source that wrote one anyway would be outside the type.
    //
    // Two arms, both mechanical, both printing what they scanned. Agent
    // lessons §44 is why: when a rule is enforced by matching source text, the
    // matching is part of the rule, so a walk that found too little must fail
    // rather than pass, and comment lines are stripped so a doc comment
    // naming the file is not an offender.
    //
    // What it can and cannot see is stated rather than implied: it sees
    // literal spellings of `zaru.toml` and of the write verbs below, and it
    // would not see a filename assembled at run time. That limit is the reason
    // the first arm exists -- inside the manifest module there is no write
    // verb at all, whatever it is applied to.
    let writes = [
        "fs::write",
        "File::create",
        "OpenOptions",
        "create_new",
        "write_all",
        "create_dir",
    ];

    let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("zaru-core")
        .join("src");
    let sources: Vec<(std::path::PathBuf, String)> = product_sources(&cli)
        .into_iter()
        .chain(product_sources(&core))
        .collect();
    let lines: usize = sources.iter().map(|(_, body)| body.lines().count()).sum();
    assert!(
        sources.len() >= 30 && lines >= 4000,
        "scanned {} product source file(s) and {lines} line(s) across two crates, which is less \
         than they hold; the walk is broken rather than the tree clean",
        sources.len()
    );

    let mut offenders = Vec::new();
    for (path, body) in &sources {
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Component equality, never a substring of the whole path. The
        // substring form said "/manifest" and matched every file in the
        // repository, because the worktree this was written in is called
        // `manifest-validators` -- so the first arm reported the credential
        // store as an offender. Agent lessons §44 twice over: the matching is
        // part of the rule, and the rule was wrong in the direction that
        // reports a false offender rather than the one that goes quiet.
        let in_the_manifest_module = path
            .components()
            .any(|component| component.as_os_str() == "manifest")
            || path.file_name().is_some_and(|name| name == "manifest.rs");
        for verb in writes {
            if !code.contains(verb) {
                continue;
            }
            if in_the_manifest_module {
                offenders.push(format!("{} uses {verb}", path.display()));
            }
            for line in code.lines().filter(|line| line.contains(verb)) {
                if line.contains("zaru.toml") {
                    offenders.push(format!(
                        "{} writes zaru.toml: {}",
                        path.display(),
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "ADR-0009 D6 says the manifest is read and never written; found {} across {} file(s) and \
         {lines} line(s): {offenders:?}",
        offenders.len(),
        sources.len(),
    );
}

/// Every `.rs` file under `root` that is not part of a module's test tree.
fn product_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut found = Vec::new();
    let mut frontier = vec![root.to_path_buf()];
    while let Some(here) = frontier.pop() {
        let entries = std::fs::read_dir(&here)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", here.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                frontier.push(path);
                continue;
            }
            if path.extension().is_some_and(|extension| extension == "rs")
                && !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("fixtures.rs" | "tests.rs")
                )
            {
                let body = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
                found.push((path, body));
            }
        }
    }
    found
}
