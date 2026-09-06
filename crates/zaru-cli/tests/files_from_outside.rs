// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every file this harness reads, driven from outside the crate — and the
//! security corpus a real file makes reachable.
//!
//! # What became reachable on 2026-09-05
//!
//! Seven cases below were unreachable until a file could be read at all. Some
//! were already refused *at the type* — a schema path outside the working
//! directory, a project naming a provider endpoint — and being refused at the
//! type is a different claim from being refused when it arrives the way it will
//! actually arrive. [operations/testing]'s rule is that the security corpus only
//! grows, so each is here in the shape a user could produce it in.
//!
//! # What this is evidence about
//!
//! The crate's public door, over real files on a scratch root at the paths the
//! product reads. **Not evidence about the `zaru` binary** —
//! `tests/cli_from_outside.rs` is where that runs — and not evidence about
//! anything a process does, because nothing here starts one.
//!
//! Every planted secret is a generated nonce. It authenticates nothing and
//! exists so that an absence assertion could have found something
//! ([Verification lessons] §26).
//!
//! [operations/testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::{Path, PathBuf};

use zaru_cli::cli::Overrides;
use zaru_cli::cli::layers::{self, Files, ProjectFile, UserFile};
use zaru_cli::config::{ConfigRefused, LayerSource, SizeCeiling, TomlFile};
use zaru_cli::manifest::{MANIFEST_FILE, ManifestFile, ManifestNotRead};
use zaru_cli::runtime::{ResolvedTier, Tier};
use zaru_cli::session::{Meta, MetaFile, MetaStore, Millis};
use zaru_cli::tools::WorkingDirectory;

/// A value that exists nowhere else, so that finding it means something.
fn nonce(label: &str) -> String {
    // **A counter, not only a clock.** The process id is shared by every check
    // in this binary and they run in parallel, so the timestamp was the only
    // thing separating two scratch roots -- and two threads reading the clock
    // in the same tick get the same one. Observed once on 2026-09-05 under a
    // machine carrying several builds: two checks shared a root, one wrote the
    // `zaru.toml` whose `shape` validator points out of the tree (its own
    // security-corpus case), and the other read it as ADR-0014 layer 3 and
    // failed with that refusal -- a red in a check that had nothing to do with
    // it. Three targeted re-runs passed, which is what a shared-state flake
    // looks like from the outside.
    //
    // The counter makes two roots distinct whatever the clock does; the
    // timestamp stays so a leftover directory still says when it was made.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{label}-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is before the unix epoch")
            .as_nanos(),
    )
}

/// A tree this file owns: a home, a working directory, a neighbour to escape
/// to, and a control that must survive the removal.
struct Scratch {
    base: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("fr-outside"));
        for directory in ["home", "project", "elsewhere", "control"] {
            std::fs::create_dir_all(base.join(directory))
                .expect("could not stage the scratch tree");
        }
        std::fs::write(base.join("control").join("keep"), b"survives")
            .expect("could not stage the control");
        Self { base }
    }

    fn home(&self) -> PathBuf {
        self.base.join("home")
    }

    fn project(&self) -> PathBuf {
        self.base.join("project")
    }

    fn elsewhere(&self) -> PathBuf {
        self.base.join("elsewhere")
    }

    fn control(&self) -> PathBuf {
        self.base.join("control")
    }

    fn base(&self) -> &Path {
        &self.base
    }

    fn working_directory(&self) -> WorkingDirectory {
        WorkingDirectory::at(self.project()).expect("the project directory exists")
    }

    /// Layer 2's file, written.
    fn user_config(&self, body: &str) -> PathBuf {
        let path = self.home().join("config.toml");
        std::fs::write(&path, body).expect("could not write the user's file");
        path
    }

    /// Layer 3's file, written.
    fn manifest(&self, body: &str) -> PathBuf {
        let path = self.project().join(MANIFEST_FILE);
        std::fs::write(&path, body).expect("could not write the project's file");
        path
    }

    fn files(&self) -> Files {
        Files::at(Some(&self.home()), Some(self.working_directory()))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// All five of ADR-0014 D1's layers, from outside the crate, over real files,
/// with the supplying layer named for each.
///
/// **This is the whole of that record's trigger clause 1**, and the two layers
/// that make it whole are the two this arc added. It resolves `project.name`,
/// which every layer may set, rather than `runtime.tier`, which D6 refuses to
/// the project layer.
///
/// The effective row is read out of the **rendered block** rather than asked of
/// the resolution, so the two sides do not travel through one path
/// ([Verification lessons] §11).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_value_set_in_all_five_layers_resolves_to_the_flag_and_every_layer_names_its_supplier() {
    let scratch = Scratch::new();
    scratch.user_config("[project]\nname = \"layer-two\"\n");
    scratch.manifest("[project]\nname = \"layer-three\"\n");

    // Layers 2 and 3 alone, so the block shows both files and the project wins.
    let files = scratch.files();
    let both = layers::resolve(&Overrides::default(), [], &files).expect("both files read");
    let key = zaru_cli::config::Key::new(zaru_cli::manifest::NAME_KEY).expect("a key");
    let block = both.explain(&key).to_string();
    println!("--- layers 2 and 3, from outside the crate ---\n{block}");
    assert!(
        block.contains("config.toml") && block.contains(MANIFEST_FILE),
        "both files name themselves in D3's source column:\n{block}"
    );
    let marked: Vec<&str> = block
        .lines()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(marked.len(), 1, "one row is marked:\n{block}");
    assert!(
        marked[0].trim_start().starts_with("3 ") && marked[0].contains("layer-three"),
        "ADR-0014 D1: layer 3 beats layer 2:\n{block}"
    );

    // And with the environment and a flag above them. `project.name` has no
    // flag, so the top of this stack is layer 4.
    let with_environment = layers::resolve(
        &Overrides::default(),
        [("ZARU_PROJECT_NAME".to_owned(), "layer-four".to_owned())],
        &files,
    )
    .expect("four layers read");
    let block = with_environment.explain(&key).to_string();
    println!("--- layers 2, 3 and 4 ---\n{block}");
    let marked: Vec<&str> = block
        .lines()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert!(
        marked[0].trim_start().starts_with("4 ") && marked[0].contains("layer-four"),
        "the environment beats both files:\n{block}"
    );

    // The one key a flag can set, through all five.
    let all_five = layers::resolve(
        &Overrides {
            tier: None,
            model: Some("layer-five".to_owned()),
            mode: None,
        },
        [("ZARU_MODEL_DEFAULT".to_owned(), "layer-four".to_owned())],
        &Files::at(Some(&scratch.home()), Some(scratch.working_directory())),
    )
    .expect("five layers read");
    let block = all_five
        .explain(&zaru_cli::providers::ModelAlias::Default.key())
        .to_string();
    println!("--- the flag over everything ---\n{block}");
    let marked: Vec<&str> = block
        .lines()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert!(
        marked[0].trim_start().starts_with("5 ") && marked[0].contains("layer-five"),
        "ADR-0014 D1: there is no layer above flags:\n{block}"
    );
}

/// ADR-0010 D1's `meta.toml`, round-tripped from outside the crate.
///
/// The workspace carries the bytes a hand-written emitter escapes wrongly, and
/// the mode is read off the filesystem rather than taken from what the code
/// asked for.
#[test]
fn a_session_records_what_it_is_and_reads_it_back_from_outside_the_crate() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    let path = scratch.project().join("meta.toml");
    let planted = format!("{}\"\\\n\t é\u{301}𝄞", nonce("workspace"));

    let mut store = MetaFile::at(&path);
    let written = Meta::new(
        ResolvedTier::supplied(Tier::Linked, zaru_cli::config::Layer::Environment),
        Some(planted.clone()),
        Some("anthropic".to_owned()),
        std::path::PathBuf::from("/tmp/a-project"),
        Millis::new(1_788_580_000_000),
    );
    store.write(&written).expect("a session records itself");

    assert_eq!(
        store.read().expect("and reads it back"),
        written,
        "a round trip through a real file loses nothing"
    );
    println!(
        "--- meta.toml ---\n{}",
        std::fs::read_to_string(&path).expect("on disk")
    );
    assert_eq!(
        std::fs::metadata(&path)
            .expect("on disk")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "ADR-0010 D5 makes the mode the only protection a session's files have"
    );
}

// ---------------------------------------------------------------------------
// The security corpus, which only grows
// ---------------------------------------------------------------------------

/// A bearer-shaped value in a real project file is refused, and the refusal
/// carries neither it nor its ASCII core.
///
/// ADR-0014 Trigger clause 3, from the shape a user can actually produce.
/// **Two arms on the absence** per [Verification lessons] §50: the raw value and
/// an ASCII core no escaping alters.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn corpus_a_bearer_value_in_a_real_file_is_refused_without_being_quoted() {
    let scratch = Scratch::new();
    let core = nonce("planted");
    // The awkward tail is what §50 exists for: an escaping formatter alters it
    // and leaves the ASCII core intact, so the core is the arm that discriminates.
    let planted = format!("nn_mcp_{core}\u{301}𝄞");
    scratch.manifest(&format!("[project]\nname = \"{planted}\"\n"));

    let failure = layers::resolve(&Overrides::default(), [], &scratch.files())
        .expect_err("ADR-0014 D4 refuses a credential-shaped value in a config file");
    let rendered = failure.to_string();
    println!("{rendered}");

    assert!(
        !rendered.contains(&planted),
        "the refusal published the planted value: {rendered}"
    );
    assert!(
        !rendered.contains(&core),
        "the refusal published the planted value's ASCII core: {rendered}"
    );
    assert!(
        !format!("{failure:?}").contains(&core),
        "the refusal's Debug published the core: {failure:?}"
    );
    assert!(
        rendered.contains("credential store"),
        "it points at ADR-0007's store: {rendered}"
    );
}

/// A project file naming a provider endpoint is refused — D6's fifth
/// escalation, from a file.
#[test]
fn corpus_a_project_file_may_not_name_a_provider_endpoint() {
    let scratch = Scratch::new();
    scratch.manifest("[project]\nname = \"p\"\n");
    // The endpoint has to come through layer 3, and the manifest carries only
    // `[project]` and `[runtime]` -- so this is the shape a project would have
    // to use, and the refusal is what says it cannot.
    let refused = ProjectFile::in_directory(scratch.working_directory());
    let mut document = refused.read().expect("the manifest reads");
    document.insert_path(
        &zaru_cli::providers::ProviderKind::Anthropic.endpoint_key(),
        zaru_cli::config::Value::Text("https://not-the-users-choice.example".to_owned()),
    );

    let failure = zaru_cli::config::Resolution::resolve(
        &layers::schema(),
        vec![zaru_cli::config::Contribution::new(
            zaru_cli::config::Layer::Project,
            refused.source(),
            document,
        )],
    )
    .expect_err("ADR-0014 D6's fifth escalation");
    let ConfigRefused::ProjectMayNotSet { key, .. } = &failure else {
        panic!("expected D6's escalation refusal, got {failure:?}");
    };
    assert_eq!(key.as_str(), "provider.anthropic.endpoint");
    println!("{failure}");
}

/// A manifest that is a link out of the tree is refused before it is read.
#[test]
fn corpus_a_manifest_that_links_out_of_the_tree_is_refused() {
    let scratch = Scratch::new();
    let outside = scratch.elsewhere().join("planted.toml");
    std::fs::write(&outside, "[project]\nname = \"theirs\"\n").expect("staging the neighbour");
    std::os::unix::fs::symlink(&outside, scratch.project().join(MANIFEST_FILE))
        .expect("staging the escaping manifest");

    let file = ManifestFile::in_directory(
        scratch.working_directory(),
        SizeCeiling::new(1 << 20).expect("a mebibyte"),
    );
    let refusal = file.parse().expect_err("a manifest outside the tree");
    assert!(
        matches!(refusal, ManifestNotRead::OutsideTheWorkingDirectory { .. }),
        "expected the boundary refusal, got {refusal:?}"
    );
    println!("{refusal}");

    // And through the layer, which is what a caller of the hierarchy meets.
    let failure = layers::resolve(&Overrides::default(), [], &scratch.files())
        .expect_err("layer 3 refuses it too");
    println!("{failure}");
}

/// A manifest whose schema path leaves the tree is refused, from a real file.
#[test]
fn corpus_a_schema_path_that_leaves_the_tree_is_refused_from_a_real_file() {
    let scratch = Scratch::new();
    std::fs::write(scratch.elsewhere().join("secret.json"), b"{}").expect("staging the neighbour");
    scratch.manifest(
        "[[validator]]\nname = \"shape\"\nrun = \"emit\"\nexpect = { json_schema = \
         \"../elsewhere/secret.json\" }\n",
    );

    let file = ManifestFile::in_directory(
        scratch.working_directory(),
        SizeCeiling::new(1 << 20).expect("a mebibyte"),
    );
    let refusal = file.parse().expect_err("a schema path outside the tree");
    let ManifestNotRead::Refused(inner) = &refusal else {
        panic!("expected the manifest's own boundary refusal, got {refusal:?}");
    };
    let rendered = inner.to_string();
    assert!(
        rendered.contains(
            &scratch
                .elsewhere()
                .join("secret.json")
                .display()
                .to_string()
        ),
        "the refusal names where the path actually reached: {rendered}"
    );
    println!("{refusal}");
}

/// A file past the caller's ceiling is refused before it is parsed.
///
/// The staged file is **also malformed**, so an implementation that parsed
/// first would refuse it for the other reason.
#[test]
fn corpus_a_file_past_the_ceiling_is_refused_unparsed() {
    let scratch = Scratch::new();
    let path = scratch.manifest("this is not toml = = =\n");

    let ceiling = SizeCeiling::new(4).expect("four bytes is a ceiling");
    let refusal = TomlFile::at(&path, ceiling)
        .read()
        .expect_err("the file is past the ceiling");
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("refused rather than parsed"),
        "a file past the ceiling is refused before it is parsed: {rendered}"
    );
    println!("{rendered}");
}

/// A key that is both a value and a table is refused by the parser, both ways.
///
/// **The measurement this corrects.** ADR-0012's arc recorded on 2026-09-05
/// that writing the nested key first lets the later scalar replace it "with
/// nothing reported". That is ADR-0014 D2's cross-layer merge; inside one file
/// the parser refuses both orders with a position, so the silent loss is a merge
/// phenomenon and not a parse one.
#[test]
fn corpus_one_file_cannot_make_a_key_both_a_value_and_a_table() {
    let scratch = Scratch::new();
    for body in [
        "[model]\ndefault = \"m\"\n\n[model.default]\ninference = \"local\"\n",
        "[model.default]\ninference = \"local\"\n\n[model]\ndefault = \"m\"\n",
    ] {
        let path = scratch.user_config(body);
        let refusal = TomlFile::at(&path, SizeCeiling::new(1 << 20).expect("a mebibyte"))
            .read()
            .expect_err("one key cannot be a leaf and a branch");
        let rendered = refusal.to_string();
        assert!(
            rendered.contains("duplicate key") && rendered.contains("line"),
            "refused with a position: {rendered}"
        );
        println!("{rendered}");
    }
}

/// A file that is not UTF-8 is refused naming the byte it stopped at.
#[test]
fn corpus_a_file_that_is_not_utf8_is_refused_naming_the_offset() {
    let scratch = Scratch::new();
    let path = scratch.home().join("config.toml");
    std::fs::write(&path, b"[project]\nname = \"\xff\xfe\"\n").expect("staging the bytes");

    let failure = layers::resolve(&Overrides::default(), [], &scratch.files())
        .expect_err("a TOML file is UTF-8 by the format's own definition");
    let rendered = failure.to_string();
    assert!(
        rendered.contains("not UTF-8") && rendered.contains("18 byte"),
        "the refusal names where the valid prefix ends: {rendered}"
    );
    println!("{rendered}");
}

/// The scratch root is removed and its absence reads four ways, with a control.
#[test]
fn the_scratch_root_is_removed_and_its_absence_reads_four_ways() {
    let scratch = Scratch::new();
    let removed = scratch.project();
    let inside = scratch.manifest("[project]\nname = \"p\"\n");
    let control = scratch.control();
    assert!(
        removed.exists() && inside.exists() && control.exists(),
        "staged"
    );

    std::fs::remove_dir_all(&removed).expect("could not remove the project directory");

    // 1. The directory itself.
    assert!(!removed.exists(), "still there: {}", removed.display());
    // 2. The control, which is the reading that discriminates.
    assert!(
        control.exists(),
        "the control went with it, so nothing above says anything"
    );
    // 3. The parent's listing, by name.
    let mut remaining: Vec<String> = std::fs::read_dir(scratch.base())
        .expect("the base is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    remaining.sort();
    assert_eq!(remaining, vec!["control", "elsewhere", "home"]);
    // 4. A read of a file that was inside it, by error kind.
    assert_eq!(
        std::fs::read(&inside).map(|_| ()).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );

    // And the reader is an ordinary caller on a directory that is gone: layer 2
    // is still there, so this is the absent-file case rather than a failure.
    let user = UserFile::under(&scratch.home());
    assert!(
        user.read()
            .expect("an absent file is not a failure")
            .is_empty(),
        "a home with no config.toml contributes an empty layer"
    );
    println!("removed {}; control survives", removed.display());
}

/// One function decides what "the working directory" is, and it canonicalises.
///
/// # Why this is a source walk and not a behavioural check
///
/// Four places want this process's working directory: [ADR-0011] D4's boundary
/// in `compose::turn::prepare`, [ADR-0009] D6's `zaru init`, and the two entry
/// points of [ADR-0010] D4's `--continue`. Each spelled `std::env::current_dir()`
/// followed by `WorkingDirectory::at` until 2026-09-06, and four spellings of
/// one rule are four chances for one of them to skip the canonicalisation.
///
/// **The mutation that removes the canonicalisation is behaviour-neutral for
/// `--continue` on Linux, measured rather than assumed**, which is why the
/// property this holds is *one call* rather than *canonical*. `getcwd(2)`
/// resolves the working directory, so `std::env::current_dir()` returns a path
/// with no symbolic link left in it even for a process started through one —
/// checked by starting a shell in a symlinked directory, where `pwd -P` and
/// `os.getcwd()` both printed the resolved path. And both sides of
/// `most_recent_in`'s comparison come from one call, so a mutant that
/// canonicalises neither still agrees with itself. What a *second* call would
/// break is the pair: a session recorded under one answer and looked for under
/// another, with nothing saying why. That is what this asserts, and its
/// mutant — a planted `current_dir` under one of the four — is caught here.
///
/// The canonicalisation still earns its place for `WorkingDirectory`'s other
/// role, classifying a candidate path against the root under ADR-0011 D4, and
/// that half has its own checks in `tests/tool_surface_from_outside.rs`.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn corpus_one_thing_decides_a_working_directory() {
    /// The one place the process may be asked where it is.
    const DECIDER: &str = "src/tools/tree.rs";
    const NEEDLE: &str = "env::current_dir";

    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut scanned = 0usize;
    let mut lines = 0usize;
    let mut offences: Vec<String> = Vec::new();

    let mut stack = vec![source.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("a product directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            // `#[cfg(test)]` trees are not the product. A check that stages a
            // directory may ask the process where it is; what must have one
            // answer is what the binary does.
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests.rs" || name == "fixtures.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("a readable source file");
            scanned += 1;
            lines += text.lines().count();
            let relative = path
                .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            for (number, line) in text.lines().enumerate() {
                // A doc comment naming the call is prose about the rule, not
                // an instance of it.
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                if line.contains(NEEDLE) && relative != DECIDER {
                    offences.push(format!("{relative}:{}: {}", number + 1, line.trim()));
                }
            }
        }
    }

    // Liveness, so a walk that read the wrong directory fails loudly rather
    // than passing vacuously — the arm `no_network_call_can_originate_from_session_storage`
    // already carries and the one that caught a scan of three files.
    println!("scanned {scanned} product file(s), {lines} line(s)");
    assert!(
        scanned > 100 && lines > 20_000,
        "this scan read {scanned} product file(s) and {lines} line(s), which is too few to have \
         asserted anything about where the working directory is decided",
    );
    assert!(
        std::fs::read_to_string(source.join("tools/tree.rs"))
            .expect("the decider is there")
            .contains(NEEDLE),
        "{DECIDER} is exempted as the one place that asks the process where it is, and it does \
         not ask",
    );
    assert!(
        offences.is_empty(),
        "the working directory has one answer, and {} other place(s) ask for it: {}",
        offences.len(),
        offences.join("\n  "),
    );
}
