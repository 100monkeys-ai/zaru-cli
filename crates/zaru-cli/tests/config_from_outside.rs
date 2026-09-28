// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The configuration hierarchy driven from outside the crate, over real files.
//!
//! # What this is evidence about
//!
//! [Verification lessons] §25: "For any capability a user interacts with, one
//! check drives the interaction end to end and reads the outcome." This file is
//! that check for the hierarchy: it uses only what `zaru-cli` exports, drives
//! the product's own [`UserFile`] and [`ProjectFile`] over real files on a
//! scratch root at the paths the product actually reads, and prints D3's block.
//!
//! **Until 2026-09-05 it implemented [`LayerSource`] itself**, with a reader
//! that took one `key = value` per line and said in its own documentation that
//! it was not TOML, because [ADR-0003] D2's table named no parser. That
//! implementation is deleted: the product has readers now, and a check standing
//! in for one that exists is asserting about a fake.
//!
//! **Evidence about the mechanism, not about the `zaru` binary.**
//! `tests/cli_from_outside.rs` is where the binary itself is driven.
//!
//! # The scratch root, and the reading that discriminates
//!
//! The root is removed at the end and its absence is checked three ways, with
//! a **sibling control that must survive**. A checker that reports "gone" for
//! everything passes on the root and fails on the control, which is what
//! makes the control the reading that discriminates rather than decoration.
//!
//! `~/.zaru/` is never touched. The configuration loader does not create that
//! directory: it has exactly one owner of its `0700` mode — the credential
//! store, which re-asserts it on every open — and two creators would leave
//! the mode set by whichever ran first.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use std::path::{Path, PathBuf};

use zaru_cli::cli::layers::{ProjectFile, UserFile};
use zaru_cli::config::{
    ConfigRefused, Contribution, Key, Layer, LayerSource, Resolution, Value, environment, gather,
};
use zaru_cli::tools::WorkingDirectory;

/// Every key this binary declares, from the records that own them.
///
/// **This file spelled three of them itself until 2026-09-05**, which was one
/// record's key written inside another check. `zaru_cli::cli::layers::schema`
/// is what the binary folds against, so this drives what a user drives.
fn schema() -> zaru_cli::config::Schema {
    zaru_cli::cli::layers::schema()
}

/// Layer 2 and layer 3, as the product reads them.
fn files(scratch: &ScratchRoot) -> (UserFile, ProjectFile) {
    (
        UserFile::under(&scratch.dir()),
        ProjectFile::in_directory(
            WorkingDirectory::at(scratch.project()).expect("the project directory exists"),
        ),
    )
}

/// A tree this check owns, with a sibling that must survive its removal.
///
/// The configuration files go in `<base>/zaru/` and the control sits beside
/// it in `<base>/control/`. Removing `<base>/zaru` is what the check verifies;
/// **the control must still be there afterwards**, and that is the reading
/// that discriminates. A checker reporting absence for everything passes on
/// the removed directory and fails on the control.
struct ScratchRoot {
    base: PathBuf,
}

impl ScratchRoot {
    fn new() -> Self {
        let unique = format!(
            "ch-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the system clock is before the unix epoch")
                .as_nanos(),
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("control")).expect("could not stage the scratch tree");
        std::fs::create_dir_all(base.join("zaru")).expect("could not stage the scratch tree");
        std::fs::create_dir_all(base.join("project")).expect("could not stage the scratch tree");
        std::fs::write(base.join("control").join("keep"), b"survives")
            .expect("could not stage the control");
        Self { base }
    }

    /// Layer 2's file, at the name ADR-0014 D1 gives it.
    fn user_file(&self) -> PathBuf {
        self.dir().join("config.toml")
    }

    /// The working directory layer 3 is read from.
    fn project(&self) -> PathBuf {
        self.base.join("project")
    }

    /// Layer 3's file, at the name ADR-0009 D1 gives it.
    fn project_file(&self) -> PathBuf {
        self.project().join("zaru.toml")
    }

    /// The directory the configuration files are in.
    fn dir(&self) -> PathBuf {
        self.base.join("zaru")
    }

    /// The sibling that must survive.
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

#[test]
fn the_hierarchy_resolves_from_outside_the_crate_over_real_files() {
    let scratch = ScratchRoot::new();

    // Layer 2: the user's file, on disk.
    std::fs::write(
        scratch.user_file(),
        "# the user's own configuration\n\
         [project]\n\
         name = \"from-the-user\"\n\
         workspace = \"acme-engineering\"\n\
         \n\
         [runtime]\n\
         max_iterations = 8\n\
         tier = \"contained\"\n",
    )
    .expect("could not write the user's file");

    // Layer 3: the project's file, lowering the ceiling and renaming.
    std::fs::write(
        scratch.project_file(),
        "[project]\n\
         name = \"from-the-project\"\n\
         \n\
         [runtime]\n\
         max_iterations = 3\n",
    )
    .expect("could not write the project's file");

    let (user, project) = files(&scratch);

    let schema = schema();
    let sources: Vec<&dyn LayerSource> = vec![&user, &project];
    let mut contributions = gather(sources).expect("both files read");

    // Layer 4, built by the product's own reader from pairs this check owns.
    let from_environment = environment::read(
        &schema,
        vec![(
            "ZARU_PROJECT_WORKSPACE".to_owned(),
            "acme-platform".to_owned(),
        )],
    )
    .expect("the environment reads");
    contributions.push(Contribution::new(
        Layer::Environment,
        Layer::Environment.default_source(),
        from_environment,
    ));

    let resolved = Resolution::resolve(&schema, contributions).expect("the fixture resolves");

    // D1 — the project beat the user on `project.name`, and the environment
    // beat both on `project.workspace`.
    assert_eq!(
        resolved.get(&Key::new("project.name").expect("a well-formed key")),
        Some(&Value::Text("from-the-project".to_owned())),
    );
    assert_eq!(
        resolved.get(&Key::new("project.workspace").expect("a well-formed key")),
        Some(&Value::Text("acme-platform".to_owned())),
    );

    // D2 — the user's `runtime.tier` survived a project that never mentioned
    // it.
    assert_eq!(
        resolved.get(&Key::new("runtime.tier").expect("a well-formed key")),
        Some(&Value::Text("contained".to_owned())),
    );

    // D6 — the project lowered the ceiling, which it may.
    assert_eq!(
        resolved.get(&Key::new("runtime.max_iterations").expect("a well-formed key")),
        Some(&Value::Integer(3)),
    );

    // D3 — the block, printed so a person can read what this run resolved.
    let block = resolved
        .explain(&Key::new("project.workspace").expect("a well-formed key"))
        .to_string();
    println!("--- ADR-0014 D3, from outside the crate ---");
    print!("{block}");
    println!("-------------------------------------------");

    assert_eq!(
        block.matches("← effective").count(),
        1,
        "exactly one layer is marked effective:\n{block}",
    );
    assert!(
        block.contains("ZARU_PROJECT_WORKSPACE"),
        "the environment row names the variable the key maps to:\n{block}",
    );
    assert!(
        block.contains("zaru.toml"),
        "the project row names the file it was read from:\n{block}",
    );

    // The configuration directory is removed and its absence read three
    // ways, with a control beside it that must survive.
    let removed = scratch.dir();
    let file = scratch.user_file();
    let control = scratch.control();
    std::fs::remove_dir_all(&removed).expect("could not remove the configuration directory");

    // 1. The directory itself.
    assert!(
        !removed.exists(),
        "the configuration directory is still there: {}",
        removed.display(),
    );
    // 2. The control, which is what discriminates: a checker that reported
    //    absence for everything would fail here.
    assert!(
        control.exists(),
        "the control at {} went with it, so nothing above says anything about the removal",
        control.display(),
    );
    // 3. The parent's listing, read independently of both.
    let mut remaining: Vec<String> = std::fs::read_dir(scratch.base())
        .expect("the base is readable")
        .map(|entry| {
            entry
                .expect("a readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    remaining.sort();
    assert_eq!(
        remaining,
        vec!["control".to_owned(), "project".to_owned()],
        "the configuration directory went and the project directory and control did not"
    );
    // 4. A file inside it reads back as absent rather than as forbidden.
    assert_eq!(
        std::fs::read(&file).map(|_| ()).unwrap_err().kind(),
        std::io::ErrorKind::NotFound,
        "the user's file reads back as something other than absent",
    );

    println!(
        "scratch: removed {} -- control {} survives; base holds {:?}",
        removed.display(),
        control.display(),
        remaining,
    );
}

#[test]
fn a_project_file_that_raises_a_ceiling_is_refused_from_outside_the_crate() {
    let scratch = ScratchRoot::new();

    std::fs::write(scratch.user_file(), "[runtime]\nmax_iterations = 3\n")
        .expect("could not write the user's file");
    std::fs::write(scratch.project_file(), "[runtime]\nmax_iterations = 99\n")
        .expect("could not write the project's file");

    let (user, project) = files(&scratch);
    let sources: Vec<&dyn LayerSource> = vec![&user, &project];
    let contributions = gather(sources).expect("both files read");

    let refusal = Resolution::resolve(&schema(), contributions)
        .expect_err("a project file raised a ceiling the user had lowered");

    println!("--- ADR-0014 D6, from outside the crate ---");
    println!("{refusal}");
    println!("-------------------------------------------");

    assert!(matches!(refusal, ConfigRefused::ProjectMayNotRaise { .. }));

    let removed = scratch.dir();
    std::fs::remove_dir_all(&removed).expect("could not remove the configuration directory");
    assert!(!removed.exists());
    assert!(scratch.control().exists(), "the control must survive");
}

#[test]
fn a_project_file_carrying_a_bearer_shaped_value_is_refused_without_quoting_it() {
    let scratch = ScratchRoot::new();

    // A nonce, not a credential: it authenticates nothing and exists only so
    // that an absence assertion could have found something.
    let planted = format!(
        "nn_mcp_outside-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is before the unix epoch")
            .as_nanos(),
    );
    std::fs::write(
        scratch.project_file(),
        format!("[project]\nname = \"{planted}\"\n"),
    )
    .expect("could not write the project's file");

    let (_, project) = files(&scratch);
    let sources: Vec<&dyn LayerSource> = vec![&project];
    let contributions = gather(sources).expect("the file reads");

    let refusal = Resolution::resolve(&schema(), contributions)
        .expect_err("a project file carried a bearer-shaped value and the load accepted it");
    let rendered = refusal.to_string();

    println!("--- ADR-0014 D4, from outside the crate ---");
    println!("{rendered}");
    println!("-------------------------------------------");

    assert!(
        !rendered.contains(&planted),
        "the refusal published the planted value verbatim",
    );
    assert!(
        !format!("{refusal:?}").contains(&planted),
        "the refusal's Debug published the planted value",
    );
    assert!(
        rendered.contains("credential store"),
        "the refusal points at ADR-0007's store: {rendered}",
    );

    let removed = scratch.dir();
    std::fs::remove_dir_all(&removed).expect("could not remove the configuration directory");
    assert!(!removed.exists());
    assert!(scratch.control().exists(), "the control must survive");
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
