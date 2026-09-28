// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives **all four** of [ADR-0009] D3's `expect`
//! kinds, from a real manifest, over real child processes.
//!
//! This is the file that makes that record's Trigger clause 3 whole. Until
//! 2026-09-05 two of the four were decided over a real exit status and two
//! reached only as far as a port, because [ADR-0003] D2's table named no
//! regular-expression engine and no JSON Schema validator. It names both now,
//! and every kind here passes **and** fails with nothing staged in between:
//! `ManifestFile` reads the declarations off disk, `Plan` orders them,
//! `Dispatch` walks them, `Spawn` runs them as children, and `Patterns` and
//! `SchemaFiles` decide the two that need a crate.
//!
//! Every command is a real program resolved through `PATH` — `true`, `false`
//! and `printf` — and every one of them lives outside the working directory.
//!
//! **Evidence about the mechanism through the crates' public doors, and it
//! must never be quoted as evidence about the `zaru` binary.** No command that
//! binary runs declares a validator or runs one; `tests/cli_from_outside.rs`
//! is where the binary runs.
//!
//! Nothing here opens a socket. The only real effects are child processes and
//! a project tree under the system temporary directory, removed when the check
//! ends.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators

use core::time::Duration;
use std::collections::BTreeMap;
use zaru_cli::cli::{FILE_CEILING_BYTES, PATTERN_CEILING_BYTES};
use zaru_cli::config::SizeCeiling;
use zaru_cli::manifest::{MANIFEST_FILE, ManifestFile};
use zaru_cli::process::{Environment, ProcessCeiling, Spawn};
use zaru_cli::tools::WorkingDirectory;
use zaru_cli::validators::{PatternCeiling, Patterns, SchemaFiles};
use zaru_core::iteration::validator::{Dispatch, Plan};
use zaru_core::iteration::{ExecutionOutcome, ValidatorOutcome, Validators};

/// A project tree the check owns, with a URI-safe name.
///
/// A `$ref` naming a path with a combining mark in it is refused by the
/// 2020-12 metaschema before any loader is reached, so a scratch name built
/// for an absence assertion would make a boundary check pass without the
/// boundary running.
struct Project {
    base: std::path::PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos();
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!("vro-{label}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(base.join("schema")).expect("staging: the project");
        Self { base }
    }

    fn working_directory(&self) -> WorkingDirectory {
        WorkingDirectory::at(&self.base).expect("the project resolves")
    }

    fn write(&self, relative: &str, body: &str) {
        std::fs::write(self.base.join(relative), body).expect("staging: a file");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The binary's own numbers, so this check runs what the composition would.
fn ceilings() -> (SizeCeiling, PatternCeiling, ProcessCeiling) {
    (
        SizeCeiling::new(FILE_CEILING_BYTES).expect("a mebibyte"),
        PatternCeiling::new(PATTERN_CEILING_BYTES).expect("ten mebibytes"),
        ProcessCeiling::new(Duration::from_secs(30)).expect("thirty seconds"),
    )
}

/// **ADR-0009 Trigger clause 3, whole.** Each of D3's four `expect` kinds
/// passes and fails, over a manifest read from disk and commands run as real
/// children.
///
/// Eight validators, two per kind, and the manifest declares them **shuffled**
/// rather than kind by kind — the record's own worked example is already in the
/// shape that makes an ordering rule look right ([Verification lessons] §55),
/// and staging a kind's passing case immediately before its failing one would
/// let "the previous one's answer" reproduce the expected sequence
/// (§54). `after` on the last one is what keeps a skip in the picture.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn a_caller_outside_the_crate_passes_and_fails_all_four_expect_kinds_over_real_children() {
    println!("== ADR-0009 D3, all four kinds, from a real manifest, over real children ==");
    let project = Project::new("four-kinds");
    project.write(
        "schema/output.json",
        r#"{"type":"object","required":["ok"],"properties":{"ok":{"type":"boolean"}}}"#,
    );
    project.write(
        MANIFEST_FILE,
        r#"
[project]
name = "acme-api"

[[validator]]
name = "shape-fails"
run  = "printf '{\"ok\": \"yes\"}'"
expect = { json_schema = "schema/output.json" }

[[validator]]
name = "after-a-failure"
run  = "printf 'this must never run'"
expect = "exit-zero"
after = ["zero-fails"]

[[validator]]
name = "code-passes"
run  = "false"
expect = { exit-code = 1 }

[[validator]]
name = "match-fails"
run  = "printf 'tests: none ran'"
expect = { matches = '[0-9]+ passed' }

[[validator]]
name = "zero-passes"
run  = "true"
expect = "exit-zero"

[[validator]]
name = "shape-passes"
run  = "printf '{\"ok\": true}'"
expect = { json_schema = "schema/output.json" }

[[validator]]
name = "code-fails"
run  = "false"
expect = { exit-code = 7 }

[[validator]]
name = "match-passes"
run  = "printf 'tests: 128 passed'"
expect = { matches = '[0-9]+ passed' }

[[validator]]
name = "zero-fails"
run  = "false"
expect = "exit-zero"
"#,
    );

    let working = project.working_directory();
    let (files, pattern_ceiling, process_ceiling) = ceilings();

    // Read, rather than construct.
    let manifest = ManifestFile::in_directory(working.clone(), files)
        .parse()
        .expect("the manifest is well formed")
        .expect("the manifest is there");
    let plan = Plan::from_declared(manifest.validators().to_vec()).expect("no cycle, no unknown");
    println!(
        "   declared order: {:?}",
        manifest
            .validators()
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
    );

    let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
        "PATH",
        "/usr/bin:/bin",
    )]))
    .expect("the harness's own values pass on");
    let spawn = Spawn::new(&working, environment, process_ceiling);
    let patterns = Patterns::new(pattern_ceiling);
    let schemas = SchemaFiles::new(&working, files);
    let dispatch = Dispatch::new(&plan, &spawn, &patterns, &schemas);

    let reports = dispatch
        .evaluate(&ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
        .await
        .expect("every declaration here is usable");

    let outcomes: BTreeMap<&str, ValidatorOutcome> = reports
        .iter()
        .map(|report| (report.name.as_str(), report.outcome))
        .collect();
    for report in &reports {
        println!("   {:>16}  {:?}", report.name, report.outcome);
    }

    // The denominator is the number DECLARED, not the number reported.
    assert_eq!(
        reports.len(),
        manifest.validators().len(),
        "every declared validator is reported on",
    );

    for passing in ["zero-passes", "code-passes", "match-passes", "shape-passes"] {
        assert_eq!(
            outcomes.get(passing),
            Some(&ValidatorOutcome::Passed),
            "ADR-0009 D3: `{passing}` is the passing half of its kind",
        );
    }
    for failing in ["zero-fails", "code-fails", "match-fails", "shape-fails"] {
        assert_eq!(
            outcomes.get(failing),
            Some(&ValidatorOutcome::Failed),
            "ADR-0009 D3: `{failing}` is the failing half of its kind. A kind that only ever \
             passes is not a validator, which is why the clause says each kind passes AND fails",
        );
    }
    assert_eq!(
        outcomes.get("after-a-failure"),
        Some(&ValidatorOutcome::Skipped),
        "D2: a validator whose prerequisite failed does not run",
    );

    // And the skipped one's command really did not run: `printf` writes
    // nothing to disk, so the evidence is that its bytes are in no report.
    let skipped = reports
        .iter()
        .find(|r| r.name == "after-a-failure")
        .expect("it is reported");
    assert!(
        skipped.detail.is_empty(),
        "a skipped validator carries no detail: {:?}",
        skipped.detail,
    );

    // The two kinds that need a crate carry the command's own bytes into
    // refinement, exactly as the two that do not.
    let matched = reports
        .iter()
        .find(|r| r.name == "match-fails")
        .expect("it is reported");
    assert_eq!(
        matched.detail, "tests: none ran",
        "ADR-0009 D5 sends the failing command's captured output verbatim, and a `matches` \
         failure is no different from an `exit-zero` one",
    );
    let shaped = reports
        .iter()
        .find(|r| r.name == "shape-fails")
        .expect("it is reported");
    assert_eq!(shaped.detail, "{\"ok\": \"yes\"}");
}

/// **Security corpus, from outside.** A manifest whose schema path leaves the
/// tree is refused when the validator runs, not only when the manifest is read.
///
/// The manifest here is read successfully — the path is inside the tree at that
/// moment — and the file is then replaced with a link to a neighbour that
/// *would* accept the output. An implementation classifying only at read time
/// reports `Passed`.
#[tokio::test]
async fn corpus_a_schema_that_leaves_the_tree_between_the_read_and_the_run_is_refused() {
    println!("== a schema path that leaves the tree after the manifest was read ==");
    let project = Project::new("toctou");
    let outside = project.base.parent().expect("a parent").join(format!(
        "vro-neighbour-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    ));
    std::fs::write(&outside, r#"{"type":"object"}"#).expect("staging: the neighbour");

    project.write(
        "schema/output.json",
        r#"{"type":"object","required":["ok"]}"#,
    );
    project.write(
        MANIFEST_FILE,
        r#"
[[validator]]
name = "shape"
run  = "printf '{\"no\": true}'"
expect = { json_schema = "schema/output.json" }
"#,
    );

    let working = project.working_directory();
    let (files, pattern_ceiling, process_ceiling) = ceilings();
    let manifest = ManifestFile::in_directory(working.clone(), files)
        .parse()
        .expect("the manifest is read while the path is still inside the tree")
        .expect("it is there");

    // And now it is not.
    let inside = project.base.join("schema").join("output.json");
    std::fs::remove_file(&inside).expect("staging: removing the file");
    std::os::unix::fs::symlink(&outside, &inside).expect("staging: the escaping link");

    let plan = Plan::from_declared(manifest.validators().to_vec()).expect("one validator");
    let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
        "PATH",
        "/usr/bin:/bin",
    )]))
    .expect("the harness's own values");
    let spawn = Spawn::new(&working, environment, process_ceiling);
    let patterns = Patterns::new(pattern_ceiling);
    let schemas = SchemaFiles::new(&working, files);
    let dispatch = Dispatch::new(&plan, &spawn, &patterns, &schemas);

    let failure = dispatch
        .evaluate(&ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
        .await
        .expect_err("the schema path now resolves outside the working directory");
    let rendered = failure.to_string();
    println!("   {rendered}");
    assert!(
        rendered.contains("outside the working directory"),
        "the run is refused rather than validated against a file the project pointed at: \
         {rendered}",
    );
    assert!(
        rendered.contains(
            &std::fs::canonicalize(&outside)
                .expect("it resolves")
                .display()
                .to_string()
        ),
        "and the refusal names where the path actually reached: {rendered}",
    );

    let _ = std::fs::remove_file(&outside);
}

/// **Security corpus, from outside.** A pattern that is not a regular
/// expression is refused, and the refusal carries no part of it.
///
/// The pattern carries a value that exists nowhere else, so an implementation
/// handing the engine's own message out — which renders the pattern under a
/// caret — is found. The accepting sibling is in the same check: the same
/// value in a pattern that compiles decides normally.
#[tokio::test]
async fn corpus_an_unusable_pattern_is_refused_from_outside_without_being_quoted() {
    println!("== an unusable pattern, refused without being quoted ==");
    let planted = format!(
        "planted-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    );
    let project = Project::new("bad-pattern");
    project.write(
        MANIFEST_FILE,
        &format!(
            "[[validator]]\nname = \"shape\"\nrun  = \"true\"\nexpect = {{ matches = '{planted}[' \
             }}\n"
        ),
    );

    let working = project.working_directory();
    let (files, pattern_ceiling, process_ceiling) = ceilings();
    let manifest = ManifestFile::in_directory(working.clone(), files)
        .parse()
        .expect("an unusable pattern is still well-formed TOML and a non-empty string")
        .expect("it is there");
    let plan = Plan::from_declared(manifest.validators().to_vec()).expect("one validator");
    let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
        "PATH",
        "/usr/bin:/bin",
    )]))
    .expect("the harness's own values");
    let spawn = Spawn::new(&working, environment, process_ceiling);
    let patterns = Patterns::new(pattern_ceiling);
    let schemas = SchemaFiles::new(&working, files);
    let dispatch = Dispatch::new(&plan, &spawn, &patterns, &schemas);

    let failure = dispatch
        .evaluate(&ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
        .await
        .expect_err("an unclosed character class is not a regular expression");
    let rendered = failure.to_string();
    println!("   {rendered}");
    assert!(
        !rendered.contains(&planted),
        "the refusal must carry no part of the pattern; a manifest gets committed and a refusal \
         gets pasted into a report: {rendered}",
    );
    assert!(
        rendered.contains("deliberately not quoted"),
        "and it says so rather than leaving a reader to wonder what was wrong: {rendered}",
    );

    // The accepting sibling, so the assertion above could have found something.
    let sibling = Project::new("good-pattern");
    sibling.write(
        MANIFEST_FILE,
        &format!(
            "[[validator]]\nname = \"shape\"\nrun  = \"printf '{planted}'\"\nexpect = {{ matches \
             = '{planted}' }}\n"
        ),
    );
    let working = sibling.working_directory();
    let manifest = ManifestFile::in_directory(working.clone(), files)
        .parse()
        .expect("well formed")
        .expect("there");
    let plan = Plan::from_declared(manifest.validators().to_vec()).expect("one validator");
    let environment = Environment::inherited_minimum(&zaru_cli::config::Variables::of([(
        "PATH",
        "/usr/bin:/bin",
    )]))
    .expect("the harness's own values");
    let spawn = Spawn::new(&working, environment, process_ceiling);
    let schemas = SchemaFiles::new(&working, files);
    let dispatch = Dispatch::new(&plan, &spawn, &patterns, &schemas);
    let reports = dispatch
        .evaluate(&ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
        .await
        .expect("the same value in a pattern that compiles");
    assert_eq!(reports[0].outcome, ValidatorOutcome::Passed);
    println!("   sibling: the same value in a usable pattern passed");
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
