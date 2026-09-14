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

/// **ADR-0002 D8's "at most once ever" outlives the process, and D4's
/// condition is still re-read.**
///
/// The two mutants this catches are the two ways the rule can be wrong.
/// **Ignoring the witness** makes "once ever" mean once per process, which is
/// what `--resume` did until 2026-09-05. **Reading the other line's field**
/// decides the two lines by one rule, and the arm that separates them is a
/// session that has said ADR-0011 D2's notice and never this one — which is
/// every session that has run a turn at `bare` in a project that had a
/// manifest.
///
/// D4's own condition is re-read on every process, and the last two arms are
/// why it must be. A project that **gained** a `zaru.toml` between processes
/// is not owed the line at all rather than owed it and suppressed, because D4
/// is about "a project with no `zaru.toml`" and that project runs the
/// iteration loop instead. A project that **lost** one is owed it for the
/// first time, however many turns it has had — which is what makes "a turn has
/// happened" the wrong derivation.
#[test]
fn the_missing_manifest_line_is_owed_once_per_session_and_the_manifest_is_read_again() {
    use crate::session::fixtures::already_said;
    let unavailable = || Statement::new("the iteration loop is unavailable").expect("a statement");
    let how = || Statement::new("declare validators in ./zaru.toml").expect("a statement");

    assert!(
        MissingManifest::for_manifest_in_session(
            None,
            unavailable(),
            how(),
            &crate::session::AlreadySaid::none()
        )
        .is_some(),
        "a project with no manifest, in a session that has said nothing, is owed D4's line"
    );
    assert!(
        MissingManifest::for_manifest_in_session(
            None,
            unavailable(),
            how(),
            &already_said(false, true)
        )
        .is_none(),
        "this session's transcript says it already showed the line, and D8 says at most once ever"
    );
    // The arm that tells the two rules apart: the *other* line was said and
    // this one was not.
    assert!(
        MissingManifest::for_manifest_in_session(
            None,
            unavailable(),
            how(),
            &already_said(true, false)
        )
        .is_some(),
        "a session told that `bare` is not a sandbox has not been told its project declares no \
         validators; deciding this line by that one's witness is two rules in one place"
    );

    // The manifest half, which the transcript never overrides in either
    // direction. A project that gained one is owed nothing; a project that
    // lost one is owed the line for the first time.
    let tree = ScratchTree::new();
    let working_directory = WorkingDirectory::at(tree.project()).expect("the tree is staged");
    let manifest = Manifest::build(Table::new(), Table::new(), Vec::new(), &working_directory)
        .expect("an empty manifest is a manifest");
    assert!(
        MissingManifest::for_manifest_in_session(
            Some(&manifest),
            unavailable(),
            how(),
            &crate::session::AlreadySaid::none()
        )
        .is_none(),
        "a project WITH a manifest must not be told it has none, whatever its session has said"
    );
    assert!(
        MissingManifest::for_manifest_in_session(
            None,
            unavailable(),
            how(),
            &already_said(false, false)
        )
        .is_some(),
        "a project that had a manifest on turn one and lost it before the resume was never owed \
         this line and is owed it now, however many turns the session has had"
    );
}

// --- ADR-0009 D6 -----------------------------------------------------------

#[test]
fn no_product_source_writes_a_manifest_except_the_one_that_may() {
    // ADR-0009 D6: "The manifest is read, never written ... and `zaru init`
    // writes it once, on an explicit command, only when absent." The
    // type-level half is that `ManifestSource` has one method and there is no
    // counterpart -- the forbidden act has nothing to call. This is the other
    // half: a product source that wrote one anyway would be outside the type.
    //
    // **There is exactly one exception and it is named here**, added
    // 2026-09-05 with D6's second half. `manifest/init.rs` is the writer, and
    // the population it is excluded from is the whole of both crates -- so a
    // SECOND writer anywhere, including a second file inside the manifest
    // module, reddens this. That is what makes adding one a visible act rather
    // than a diff nobody reads.
    //
    // The exception is a file path rather than a flag on the walk, because a
    // predicate that could be satisfied by more than one file is not an
    // exception, it is a category.
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

    /// The one file ADR-0009 D6 permits to write a manifest.
    const WRITER: &str = "init.rs";

    let mut offenders = Vec::new();
    let mut permitted = 0usize;
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
        let is_the_one_writer = path
            .components()
            .any(|component| component.as_os_str() == "manifest")
            && path.file_name().is_some_and(|name| name == WRITER);
        if is_the_one_writer {
            permitted += 1;
            continue;
        }
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
        "ADR-0009 D6 says the manifest is read and never written, except by `zaru init`; found {} \
         across {} file(s) and {lines} line(s): {offenders:?}",
        offenders.len(),
        sources.len(),
    );
    // The exception is asserted to have been USED, not merely allowed. An
    // exception nobody exercises is a permanent exemption dressed as a promise
    // (Verification lessons §36), and it would also mean the walk stopped
    // seeing the writer -- which is exactly how this check would go quiet.
    assert_eq!(
        permitted, 1,
        "exactly one product file may write a manifest and it is `manifest/{WRITER}`; {permitted} \
         were skipped, so either the writer moved or a second one arrived"
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

// ---------------------------------------------------------------------------
// D1's file, read
// ---------------------------------------------------------------------------

/// A ceiling roomy enough that no manifest check meets it by accident.
fn roomy() -> crate::config::SizeCeiling {
    crate::config::SizeCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

/// [ADR-0009] D1's corrected worked manifest, read off a real file.
///
/// The example is D1's own, and **the fixture perturbs it where the check is
/// about ordering** — it does not assert dependency order here, because that is
/// `zaru-core`'s and D1's example is already written in it, which is the
/// fidelity trap [Verification lessons] §55 names. What is asserted is that
/// every declared validator arrives with its own `expect`, including the two
/// spellings D1 shows.
///
/// The mutant is dropping the `[[validator]]` arm, which reddens with none.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2
#[test]
fn adr_0009_d1s_worked_manifest_is_read_off_a_real_file() {
    use crate::manifest::file::ManifestFile;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    std::fs::write(
        tree.project().join(crate::manifest::MANIFEST_FILE),
        br#"[project]
name = "acme-api"
workspace = "acme-engineering"

[runtime]
max_iterations = 3

[[validator]]
name = "build"
run  = "cargo build --locked"
expect = "exit-zero"

[[validator]]
name = "test"
run  = "cargo test --all"
expect = "exit-zero"
after = ["build"]

[[validator]]
name = "shape"
run  = "cargo run -- --emit-schema"
expect = { json_schema = "schema/output.json" }
"#,
    )
    .expect("could not stage the manifest");
    std::fs::create_dir_all(tree.project().join("schema")).expect("staging: schema/");
    std::fs::write(tree.project().join("schema").join("output.json"), b"{}")
        .expect("staging: the schema");

    let manifest = ManifestFile::in_directory(working, roomy())
        .parse()
        .expect("D1's own example is a manifest this harness reads")
        .expect("the file is there");

    assert_eq!(
        manifest.project().get("name"),
        Some(&Value::Text("acme-api".to_owned()))
    );
    assert_eq!(
        manifest.project().get("workspace"),
        Some(&Value::Text("acme-engineering".to_owned()))
    );
    assert_eq!(
        manifest.runtime().get("max_iterations"),
        Some(&Value::Integer(3)),
        "D1's `[runtime]` lowers a ceiling and no longer sets a tier"
    );

    let declared: Vec<(&str, &str)> = manifest
        .validators()
        .iter()
        .map(|validator| (validator.name.as_str(), validator.expect.kind()))
        .collect();
    assert_eq!(
        declared,
        vec![
            ("build", "exit-zero"),
            ("test", "exit-zero"),
            ("shape", "json_schema"),
        ],
        "every validator D1 declares arrives with its own kind"
    );
    let after: Vec<&str> = manifest.validators()[1]
        .after
        .iter()
        .map(Name::as_str)
        .collect();
    assert_eq!(after, vec!["build"], "D2's `after` reaches the declaration");
    println!("D1's manifest, read: {} validator(s)", declared.len());
}

/// An absent manifest is D4's datum rather than a failure.
///
/// The mutant is refusing where there is no file, which reddens with the
/// refusal printed.
#[test]
fn an_absent_manifest_is_no_manifest_rather_than_a_refusal() {
    use crate::manifest::file::ManifestFile;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    let file = ManifestFile::in_directory(working, roomy());
    assert!(!file.path().exists(), "the fixture stages no manifest");

    assert_eq!(
        file.parse().expect("an absent manifest is not a failure"),
        None,
        "ADR-0009 D4 makes a project without one an ordinary thing"
    );
    assert!(
        !file.path().exists(),
        "nothing may be created in order to find out there is nothing"
    );
}

/// A manifest that links out of the tree is refused **before** it is read.
///
/// **The fixture's target is malformed TOML on purpose.** An implementation
/// that read first and classified after would refuse it as unparsable, so the
/// two are told apart by which refusal arrives — which is what makes this a
/// check about ordering rather than about containment alone.
///
/// For the security corpus, which only grows.
#[test]
fn a_manifest_that_links_out_of_the_working_directory_is_refused_before_it_is_read() {
    use crate::manifest::file::{ManifestFile, ManifestNotRead};

    let tree = ScratchTree::new();
    let outside = tree.base().join("elsewhere").join("planted.toml");
    std::fs::write(&outside, b"this is not toml = = =\n").expect("staging: the planted file");
    std::os::unix::fs::symlink(
        &outside,
        tree.project().join(crate::manifest::MANIFEST_FILE),
    )
    .expect("staging: the escaping manifest");

    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    let refusal = ManifestFile::in_directory(working, roomy())
        .parse()
        .expect_err("a manifest outside the working directory is refused");

    let ManifestNotRead::OutsideTheWorkingDirectory { resolved, .. } = &refusal else {
        panic!(
            "a manifest that links out of the tree must be refused before it is read; any other \
             refusal here means the bytes were read first: {refusal:?}"
        );
    };
    assert_eq!(
        resolved,
        &std::fs::canonicalize(&outside).expect("the target exists"),
        "the refusal names where the link actually reached"
    );
    println!("refused: {refusal}");
}

/// A top-level name that is none of D1's three is refused, not dropped.
///
/// Dropping it would put the name beyond ADR-0014 D5's reach, because the fold
/// only ever sees what the contribution carries — D5's own silent-typo failure
/// arriving one layer before that clause can fire.
///
/// The mutant is ignoring an unknown table, which reddens with the manifest
/// printed instead of a refusal.
#[test]
fn a_top_level_name_that_is_none_of_d1s_three_is_refused_naming_the_nearest() {
    use crate::manifest::file::{ManifestFile, ManifestNotRead};

    let tree = ScratchTree::new();
    std::fs::write(
        tree.project().join(crate::manifest::MANIFEST_FILE),
        b"[projekt]\nname = \"typo\"\n",
    )
    .expect("could not stage the manifest");
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");

    let refusal = ManifestFile::in_directory(working, roomy())
        .parse()
        .expect_err("`projekt` is not a table this record declares");
    let ManifestNotRead::UnknownTable {
        offered, nearest, ..
    } = &refusal
    else {
        panic!("expected an unknown-table refusal, got {refusal:?}");
    };
    assert_eq!((offered.as_str(), *nearest), ("projekt", PROJECT_TABLE));
    println!("refused: {refusal}");
}

/// Each of D3's four kinds is read from a file, and a fifth is refused.
///
/// **The population is [`Expect::KINDS`]** rather than a list typed here, so a
/// fifth kind cannot be added to that record without this check being answered
/// for it ([Verification lessons] §17).
///
/// The mutant is dropping any one kind's arm.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn each_of_adr_0009_d3s_four_expect_kinds_is_read_from_a_file_and_a_fifth_is_refused() {
    use crate::manifest::file::{ManifestFile, ManifestNotRead};

    let spelled = |body: &str| -> Result<Vec<Declared>, ManifestNotRead> {
        let tree = ScratchTree::new();
        std::fs::write(tree.project().join(crate::manifest::MANIFEST_FILE), body)
            .expect("could not stage the manifest");
        let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
        ManifestFile::in_directory(working, roomy())
            .parse()
            .map(|manifest| manifest.expect("the file is there").validators().to_vec())
    };

    let written = [
        ("exit-zero", "expect = \"exit-zero\"".to_owned()),
        ("exit-code", "expect = { exit-code = 2 }".to_owned()),
        ("matches", "expect = { matches = \"^ok$\" }".to_owned()),
        (
            "json_schema",
            "expect = { json_schema = \"shape.json\" }".to_owned(),
        ),
    ];
    assert_eq!(
        written.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        Expect::KINDS.to_vec(),
        "this check's population is ADR-0009 D3's own list, and it has moved"
    );

    for (kind, expect) in &written {
        let declared = spelled(&format!(
            "[[validator]]\nname = \"v\"\nrun = \"r\"\n{expect}\n"
        ))
        .unwrap_or_else(|refusal| {
            panic!("`{expect}` is D3's own spelling for `{kind}`: {refusal}")
        });
        assert_eq!(declared.len(), 1);
        assert_eq!(&declared[0].expect.kind(), kind);
        println!("{expect} -> {:?}", declared[0].expect);
    }

    let refusal = spelled("[[validator]]\nname = \"v\"\nrun = \"r\"\nexpect = { exit-cod = 2 }\n")
        .expect_err("`exit-cod` names no kind D3 defines");
    let ManifestNotRead::NoSuchExpectKind { nearest, .. } = &refusal else {
        panic!("expected a no-such-kind refusal, got {refusal:?}");
    };
    assert_eq!(*nearest, "exit-code");
    println!("refused: {refusal}");
}

/// A validator missing a field is refused naming its position and the field.
///
/// The position rather than the name, because the name is one of the things
/// that can be missing.
///
/// The mutant is defaulting a missing `run` to the empty string, which reddens
/// with the manifest printed instead of a refusal.
#[test]
fn a_validator_missing_a_field_is_refused_naming_its_position_and_the_field() {
    use crate::manifest::file::{ManifestFile, ManifestNotRead};

    let tree = ScratchTree::new();
    // The interesting entry is in the middle, so the check cannot pass against
    // "take the first" or "take the last" -- Verification lessons §54.
    std::fs::write(
        tree.project().join(crate::manifest::MANIFEST_FILE),
        b"[[validator]]\nname = \"a\"\nrun = \"ra\"\nexpect = \"exit-zero\"\n\n\
          [[validator]]\nname = \"b\"\nexpect = \"exit-zero\"\n\n\
          [[validator]]\nname = \"c\"\nrun = \"rc\"\nexpect = \"exit-zero\"\n",
    )
    .expect("could not stage the manifest");
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");

    let refusal = ManifestFile::in_directory(working, roomy())
        .parse()
        .expect_err("a validator with no `run` is not a validator");
    let ManifestNotRead::ValidatorMissing { position, field } = &refusal else {
        panic!("expected a missing-field refusal, got {refusal:?}");
    };
    assert_eq!(
        (*position, *field),
        (2, "run"),
        "the second entry is the one with no `run`: {refusal}"
    );
    println!("refused: {refusal}");
}

// ---------------------------------------------------------------------------
// D6's other half: `zaru init`
// ---------------------------------------------------------------------------

/// What `init` writes is a manifest this harness can read and fold.
///
/// **The template is transcribed from a record and the check is what stops that
/// from being a claim.** Nothing about a `&'static str` says it parses, and the
/// failure it prevents is the one this arc found before writing any of it:
/// until the three keys ADR-0009 D1 sets were declared, a file in D1's own
/// shape was refused by ADR-0014 D5 the moment layer 3 could be read — so
/// `zaru init` would have written a file `zaru config explain` refused to fold.
///
/// Three arms, and the third is the one that would have caught it: the bytes
/// parse, they become a manifest with D1's three validators, and they resolve
/// through **the binary's own schema** as layer 3.
///
/// The mutant is any edit to [`TEMPLATE`] that stops it loading.
#[test]
fn adr_0009_d1s_worked_manifest_is_what_init_writes_and_it_folds() {
    use crate::config::Resolution;
    use crate::manifest::file::ManifestFile;
    use crate::manifest::init::{self, TEMPLATE};

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    let file = ManifestFile::in_directory(working, roomy());

    let written = init::write(&file).expect("a project with no manifest gets one");
    assert_eq!(
        std::fs::read_to_string(&written).expect("the file is on disk"),
        TEMPLATE,
        "what is on disk is the record's own example, byte for byte"
    );

    let manifest = file
        .parse()
        .expect("what `init` wrote is a manifest this harness reads")
        .expect("the file is there");
    let names: Vec<&str> = manifest
        .validators()
        .iter()
        .map(|validator| validator.name.as_str())
        .collect();
    assert_eq!(names, vec!["build", "test", "shape"], "D1's three");

    let resolution = Resolution::resolve(
        &crate::cli::layers::schema(),
        vec![manifest.contribution(Source::named("./zaru.toml"))],
    )
    .expect(
        "what `init` writes must fold through the binary's own schema, or it wrote a file `zaru \
         config explain` refuses",
    );
    for spelling in [
        crate::manifest::NAME_KEY,
        crate::manifest::WORKSPACE_KEY,
        crate::runtime::MAX_ITERATIONS_KEY,
    ] {
        let key = Key::new(spelling).expect("a well-formed key");
        assert!(
            resolution.get(&key).is_some(),
            "`{spelling}` is in the template and resolved to nothing"
        );
    }
    println!("{}", std::fs::read_to_string(&written).expect("on disk"));
}

/// `init` never overwrites, and the refusal names the file that is there.
///
/// **The staged file is not a manifest**, which is what makes this a check
/// about not overwriting rather than about idempotence: a second `init` that
/// rewrote the file would destroy bytes a person put there, and the assertion
/// is that those exact bytes survive.
///
/// The mutant is `fs::rename` in place of `fs::hard_link`, which takes the name
/// from whoever holds it.
#[test]
fn init_writes_once_and_never_over_what_is_already_there() {
    use crate::manifest::file::ManifestFile;
    use crate::manifest::init::{self, InitRefused};

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    let file = ManifestFile::in_directory(working, roomy());

    let theirs = "# a manifest a person was in the middle of writing\n";
    std::fs::write(file.path(), theirs).expect("could not stage their file");

    let refusal = init::write(&file).expect_err("there is already a manifest");
    let InitRefused::AlreadyThere { path } = &refusal else {
        panic!("expected the already-there refusal, got {refusal:?}");
    };
    assert_eq!(path, file.path());
    assert_eq!(
        std::fs::read_to_string(file.path()).expect("their file is still there"),
        theirs,
        "ADR-0009 D6: configuration a tool silently rewrites is configuration the user stops \
         trusting"
    );
    println!("{refusal}");

    // And the sibling the write goes through does not survive a refusal, or
    // the next `zaru config explain` in this directory would see a stray file.
    let leftovers: Vec<String> = std::fs::read_dir(tree.project())
        .expect("the project is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.contains(crate::atomic::TEMPORARY_SUFFIX))
        .collect();
    assert!(
        leftovers.is_empty(),
        "the sibling temporary was left behind: {leftovers:?}"
    );
}

/// The file `init` writes carries the mode a session's files carry.
///
/// The mutant is writing at the process umask.
#[test]
fn what_init_writes_carries_the_mode_it_was_created_at() {
    use crate::manifest::file::ManifestFile;
    use crate::manifest::init;
    use std::os::unix::fs::PermissionsExt;

    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory exists");
    let file = ManifestFile::in_directory(working, roomy());
    let written = init::write(&file).expect("written");

    let mode = std::fs::metadata(&written)
        .expect("on disk")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode,
        crate::session::store::FILE_MODE,
        "a hard link carries the mode the content was created at, so the file is never briefly \
         wider than it ends up"
    );
}

// ADR-0006 D5's pin, and ADR-0009 D1's key getting its first reader.
//
// The key has been declared into the product schema since this module was
// written, so a `zaru.toml` naming a workspace has always validated. Nothing
// read it, so it was validated and discarded -- a user could write the key,
// see no complaint, and get nothing. These assert the reader, both arms.

/// D5: "`zaru.toml` pins the workspace per project."
///
/// The mutant is the `None` this replaced: a reader that always answers
/// nothing passes every assertion about the unpinned case perfectly, which is
/// why the pinned case is asserted first and by value.
#[test]
fn adr_0006_d5s_pin_is_read_out_of_the_project_layer() {
    use crate::config::Resolution;
    use crate::config::fixtures::{at, document, nonce, schema, text};

    let pinned = nonce("pinned-workspace");
    let resolved = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::Project,
            "./zaru.toml",
            document([("project.workspace", text(pinned.clone()))]),
        )],
    )
    .expect("the fixture resolves");

    assert_eq!(
        crate::manifest::attached_workspace(&resolved).as_deref(),
        Some(pinned.as_str()),
        "ADR-0009 D1's `project.workspace` did not reach the reader ADR-0006 D5 needs"
    );
}

/// The accepting siblings: no pin at all, and a pin that is only whitespace.
///
/// The second is not pedantry. `Field::free(FieldKind::Text)` accepts
/// `project.workspace = ""`, and an empty slug would reach the trie as a key
/// nothing is grouped under -- indistinguishable from the unpinned case from
/// the strip's side, but arrived at from a value the user wrote. Told apart
/// once, in the reader, rather than at each place that reads it.
#[test]
fn an_absent_pin_and_an_empty_pin_are_both_no_workspace() {
    use crate::config::Resolution;
    use crate::config::fixtures::{at, document, schema, text};

    let unpinned = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::Project,
            "./zaru.toml",
            document([("project.name", text("named-but-unpinned"))]),
        )],
    )
    .expect("the fixture resolves");
    assert_eq!(
        crate::manifest::attached_workspace(&unpinned),
        None,
        "a manifest that pins nothing must not invent a workspace"
    );

    for blank in ["", "   ", "\t"] {
        let empty = Resolution::resolve(
            &schema(),
            vec![at(
                Layer::Project,
                "./zaru.toml",
                document([("project.workspace", text(blank))]),
            )],
        )
        .expect("the fixture resolves");
        assert_eq!(
            crate::manifest::attached_workspace(&empty),
            None,
            "the pin {blank:?} is not a workspace slug and must not be carried as one"
        );
    }
}
