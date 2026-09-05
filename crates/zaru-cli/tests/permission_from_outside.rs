// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0011]'s permission model driven from outside the crate, over real
//! files and real handles.
//!
//! # What this is evidence about
//!
//! [Verification lessons] §25: "For any capability a user interacts with, one
//! check drives the interaction end to end and reads the outcome." This file
//! is that check for the three ports the permission decision calls out
//! through — the allowlist, the destructive matcher, and the confirmation —
//! all three of which had no product implementation anywhere in this
//! workspace until 2026-09-05.
//!
//! Everything here uses only what `zaru-cli` exports, and the configuration
//! arrives through the product's own [`UserFile`] and [`ProjectFile`] over
//! real files in a scratch root, at the paths the product actually reads.
//!
//! **Evidence about the mechanism, not about the `zaru` binary.**
//! `tests/cli_from_outside.rs` drives the binary. And nothing in the binary
//! reaches the confirmation at all today, because no command reaches the tool
//! surface — that is stated on the record rather than implied here.
//!
//! # The security corpus
//!
//! [Testing] puts ADR-0011's surface among the boundaries whose escapes join
//! a permanent corpus that never shrinks. **Every hostile case here has an
//! accepting sibling**, because a table with no accepting arm is satisfied by
//! an implementation that refuses everything.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::PathBuf;

use zaru_cli::cli::layers::{LoadFailure, ProjectFile, UserFile};
use zaru_cli::config::{LayerSource, Resolution, gather};
use zaru_cli::tools::{
    Allowed, Allowlist, Invocation, ToolName, WorkingDirectory, allowlist, mode,
};

/// A tree this check owns, with the two files ADR-0014 D1's layers 2 and 3
/// are read from and a sibling that must survive its removal.
///
/// The control is the reading that discriminates: a cleanup checker reporting
/// absence for everything passes on the removed root and fails on the
/// control.
struct ScratchRoot {
    base: PathBuf,
}

impl ScratchRoot {
    fn new(label: &str) -> Self {
        let unique = format!(
            "pp-outside-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the system clock is before the unix epoch")
                .as_nanos(),
        );
        let base = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(base.join("zaru")).expect("could not stage the scratch tree");
        std::fs::create_dir_all(base.join("project").join("inside"))
            .expect("could not stage the scratch tree");
        std::fs::create_dir_all(base.join("control")).expect("could not stage the control");
        std::fs::write(base.join("control").join("keep"), b"survives")
            .expect("could not stage the control");
        std::fs::write(base.join("project").join("inside").join("file"), b"in")
            .expect("could not stage the in-tree file");
        Self { base }
    }

    /// The directory layer 2's file lives in — a `~/.zaru` this check owns.
    fn home(&self) -> PathBuf {
        self.base.join("zaru")
    }

    /// Layer 2's file, at the name ADR-0014 D1 gives it.
    fn user_file(&self) -> PathBuf {
        self.home().join("config.toml")
    }

    /// The working directory layer 3 is read from.
    fn project(&self) -> PathBuf {
        self.base.join("project")
    }

    /// Layer 3's file, at the name ADR-0009 D1 gives it.
    fn project_file(&self) -> PathBuf {
        self.project().join("zaru.toml")
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// `tools.allowlist`'s table and leaf, from the product's own constant.
fn table() -> &'static str {
    allowlist::KEY
        .split_once('.')
        .expect("ADR-0011 D3's key is a dotted path")
        .0
}

/// The leaf half of the same key.
fn leaf() -> &'static str {
    allowlist::KEY
        .split_once('.')
        .expect("ADR-0011 D3's key is a dotted path")
        .1
}

/// Fold the two file layers the product reads, against the binary's own
/// schema.
///
/// `zaru_cli::cli::layers::schema` is what the binary folds, so this drives
/// what a user drives rather than a schema retyped in a check.
///
/// **The failure type is the product's own [`LoadFailure`], carrying both
/// halves**, because a project's `./zaru.toml` can be refused in two
/// different places and a helper that only carried one of them would decide
/// which refusal this file is able to see — see
/// `the_escalation_ceiling_is_shadowed_by_adr_0009s_manifest_vocabulary`.
/// Reading the file is [ADR-0009]'s manifest reader and can fail there;
/// folding the value is [ADR-0014]'s and can fail there.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn fold(scratch: &ScratchRoot) -> Result<Resolution, LoadFailure> {
    let user = UserFile::under(&scratch.home());
    let project = ProjectFile::in_directory(
        WorkingDirectory::at(scratch.project()).expect("the project directory exists"),
    );
    let sources: Vec<&dyn LayerSource> = vec![&user, &project];
    let contributions = gather(sources).map_err(LoadFailure::Source)?;
    Resolution::resolve(&zaru_cli::cli::layers::schema(), contributions)
        .map_err(LoadFailure::Refused)
}

/// **Corpus case, the first arm — a cloned repository cannot grant itself an
/// allowlist, and the user's own file can.**
///
/// The hostile arm is a `./zaru.toml` carrying `[tools] allowlist = [...]`.
/// It is refused, and the **accepting sibling is the same key with the same
/// entries in `~/.zaru/config.toml`** — without it, an implementation that
/// refused the key from every layer would pass.
///
/// # Which rule does the refusing, measured rather than assumed
///
/// **Not [ADR-0014] D6, today.** ADR-0014 D1's layer 3 is not a raw TOML
/// document: it is [ADR-0009] D1's manifest, read through
/// [`ProjectFile`], and that record's vocabulary is closed to `[project]`,
/// `[runtime]` and `[[validator]]`. A `[tools]` table is therefore refused by
/// the *manifest reader*, one step before the fold's escalation ceiling ever
/// sees a value — so the sentence the user gets names ADR-0009's vocabulary
/// and offers a spelling suggestion, where the rule that actually governs is
/// a security rule.
///
/// That shadowing is pinned by
/// `the_escalation_ceiling_is_shadowed_by_adr_0009s_manifest_vocabulary`
/// below, so it is a measured fact rather than a remark. What this check
/// asserts is what is **true**: the project's file is refused, naming the
/// file and the table. `allowlist::field` still declares the key
/// [`Refused`](zaru_cli::config::ProjectPolicy::Refused) — that is the arm
/// which becomes load-bearing the day ADR-0009's vocabulary admits `[tools]`,
/// and the declaration is asserted in the crate's own
/// `the_allowlist_key_is_declared_once_holds_a_list_and_is_refused_to_a_project`.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn a_project_may_not_grant_itself_an_allowlist_and_the_user_may() {
    // Built from the product's own key rather than typed out, so renaming
    // `tools.allowlist` reddens this check instead of quietly making it test
    // a key nothing declares. The spelling itself is pinned against a literal
    // in the crate's own `the_allowlist_key_is_declared_once_...`.
    let entries = format!("[{}]\n{} = [\"cmd.run cargo test\"]\n", table(), leaf());
    let entries = entries.as_str();

    // The hostile arm: the entries arrive in the file a cloned repository
    // carries.
    let cloned = ScratchRoot::new("cloned");
    std::fs::write(cloned.project_file(), entries).expect("could not write the project's file");
    let rendered = match fold(&cloned) {
        Ok(resolution) => Allowed::from_configuration(&resolution)
            .expect_err("a project layer must not reach an allowlist at all")
            .to_string(),
        Err(refusal) => refusal.to_string(),
    };
    assert!(
        rendered.contains("tools"),
        "the refusal must name what the project wrote, or the reader cannot find it: {rendered:?}"
    );
    assert!(
        rendered.contains(&cloned.project_file().display().to_string()),
        "the refusal must name the file the reader has to edit: {rendered:?}"
    );

    // The accepting sibling: the same key, the same entries, in the user's
    // own file. A fold that refused this too would be refusing the key rather
    // than the layer.
    let granted = ScratchRoot::new("granted");
    std::fs::write(granted.user_file(), entries).expect("could not write the user's file");
    let resolution = fold(&granted).expect("the user's own configuration is the grant");
    let allowed = Allowed::from_configuration(&resolution)
        .expect("the user's own layer is not bound by D6's escalation ceiling");
    assert_eq!(
        allowed.len(),
        1,
        "the user granted one entry and {} survived the fold",
        allowed.len()
    );

    let line = zaru_cli::process::CommandLine::split("cargo test").expect("a command line");
    assert!(
        allowed.approves(&Invocation::running(&line)),
        "the entry the user wrote does not approve the call it names"
    );
}

/// **A finding, pinned: ADR-0014 D6's escalation ceiling is observable from a
/// real `./zaru.toml` only for keys ADR-0009 D1's manifest vocabulary
/// admits.**
///
/// Measured 2026-09-05. `runtime.tier` sits inside `[runtime]`, which the
/// manifest declares, so a project setting it reaches the fold and is refused
/// with D6's own sentence. `tools.allowlist`, `tools.mode` and
/// `provider.<kind>.endpoint` sit under tables the manifest does not declare,
/// so all three are refused one step earlier, by ADR-0009's vocabulary, with
/// a message about a table name and a spelling suggestion rather than about
/// privilege.
///
/// **Three rather than two since 2026-09-05**, when `tools.mode` was declared
/// and D6's *first* escalation — the permission mode, named in the clause
/// since it was written — got the key it had been waiting for. It landed
/// under `[tools]` beside the allowlist, so it is shadowed on arrival, and
/// this check's population grew rather than its finding changing.
///
/// **Nothing is weakened by that**: a project still cannot set any of the
/// three, which is what D6 requires. What is lost is the *reason the user is told*,
/// and a security refusal that presents as a typo is the kind of thing nobody
/// notices until it matters. It is recorded on ADR-0011 and ADR-0014 for
/// their authors rather than fixed here, because widening ADR-0009 D1's
/// manifest vocabulary is that record's decision and inventing an ordering
/// between two refusals is neither record's implementer's.
///
/// This check exists so the shadowing is a fact somebody has to look at. **If
/// ADR-0009's vocabulary later admits `[tools]`, this reddens**, and whoever
/// changed it sees D6's refusal take over — which is the change everybody
/// wants and nobody would otherwise notice.
#[test]
fn the_escalation_ceiling_is_shadowed_by_adr_0009s_manifest_vocabulary() {
    // Inside the manifest's vocabulary: D6's own refusal is what the user
    // gets, and it names the escalation.
    let inside = ScratchRoot::new("inside-vocab");
    std::fs::write(inside.project_file(), "[runtime]\ntier = \"contained\"\n")
        .expect("could not write the project's file");
    let rendered = fold(&inside)
        .expect_err("ADR-0014 D6 refuses a project moving the runtime tier")
        .to_string();
    assert!(
        rendered.contains("more privilege than the user granted"),
        "a key inside ADR-0009's vocabulary must reach ADR-0014 D6's own refusal: {rendered:?}"
    );

    // Outside it: three of D6's escalations, all refused by the manifest's
    // vocabulary instead. More than one table, so a check that happened to
    // pass on `[tools]` alone cannot be mistaken for a statement about this
    // record only -- and both of `[tools]`'s keys, because the mode arrived
    // under a table the allowlist had already made shadowed and a check
    // staging one of them would not notice the other going missing.
    let entries = format!("[{}]\n{} = [\"cmd.run cargo test\"]\n", table(), leaf());
    // Spelled from the key rather than typed, so renaming it reddens here.
    let staged_mode = format!(
        "[{}]\n{} = \"yolo\"\n",
        mode::KEY
            .split_once('.')
            .expect("ADR-0011 D3's mode key is a dotted path")
            .0,
        mode::KEY
            .split_once('.')
            .expect("ADR-0011 D3's mode key is a dotted path")
            .1,
    );
    for (label, body) in [
        (allowlist::KEY, entries.as_str()),
        (mode::KEY, staged_mode.as_str()),
        (
            "provider.anthropic.endpoint",
            "[provider]\nanthropic = { endpoint = \"https://elsewhere.example\" }\n",
        ),
    ] {
        let outside = ScratchRoot::new(label);
        std::fs::write(outside.project_file(), body).expect("could not write the project's file");
        let rendered = fold(&outside)
            .err()
            .map(|refusal| refusal.to_string())
            .unwrap_or_else(|| {
                panic!(
                    "a project setting `{label}` was accepted by the fold; ADR-0014 D6 forbids it"
                )
            });
        assert!(
            rendered.contains("is not something a manifest declares"),
            "`{label}` is expected to be shadowed by ADR-0009's vocabulary and was refused as: \
             {rendered:?}. If ADR-0009 D1 now declares this table, the shadowing is gone and this \
             check has done its job -- read ADR-0014 D6's refusal instead and update this record"
        );
        assert!(
            !rendered.contains("more privilege than the user granted"),
            "`{label}` reached ADR-0014 D6's refusal, so the shadowing this check pins is over: \
             {rendered:?}"
        );
    }
}

/// **A path entry approves the resolved path the prompt would have shown, and
/// nothing beside it.**
///
/// Driven over a real working directory and a real file, because ADR-0011
/// D4's classification canonicalises and the entry has to be the canonical
/// text. The relative spelling is the hostile arm: it is what a user would
/// write if the allowlist matched paths the way a shell does, and it must
/// **not** approve, because no path resolution happens in the allowlist at
/// all.
///
/// The mutant is resolving an entry's target against the working directory,
/// which is the reading this arc deliberately did not build.
#[test]
fn a_path_entry_is_the_resolved_path_and_a_relative_spelling_does_not_approve() {
    let scratch = ScratchRoot::new("paths");
    let working = WorkingDirectory::at(scratch.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let resolved = target.resolved().display().to_string();

    std::fs::write(
        scratch.user_file(),
        format!("[tools]\nallowlist = [\"fs.read {resolved}\"]\n"),
    )
    .expect("could not write the user's file");
    let allowed = Allowed::from_configuration(&fold(&scratch).expect("the user's file folds"))
        .expect("the user's own layer is the grant");

    let invocation =
        Invocation::on_path(ToolName::FsRead, &target).expect("fs.read addresses a path");
    assert!(
        allowed.approves(&invocation),
        "the resolved path the prompt shows was not approved by an entry naming it"
    );

    // The hostile arm, in its own root so the two grants cannot be confused.
    let relative = ScratchRoot::new("relative");
    std::fs::write(
        relative.user_file(),
        "[tools]\nallowlist = [\"fs.read inside/file\"]\n",
    )
    .expect("could not write the user's file");
    let relative_working =
        WorkingDirectory::at(relative.project()).expect("the project directory resolves");
    let relative_target = relative_working.classify("inside/file");
    let relative_allowed =
        Allowed::from_configuration(&fold(&relative).expect("the user's file folds"))
            .expect("the user's own layer is the grant");
    let relative_invocation =
        Invocation::on_path(ToolName::FsRead, &relative_target).expect("fs.read addresses a path");
    assert!(
        !relative_allowed.approves(&relative_invocation),
        "a relative spelling approved a call; ADR-0011 D3's allowlist matches the line the prompt \
         showed, byte for byte, and resolves nothing"
    );
}
