// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The repository's shell gates, driven from outside as the runner drives them.
//!
//! These are checks about `scripts/`, not about any crate, and they live here
//! because the `test` gate already runs and a ninth CI job would be a change to
//! the workflow rather than to the script it runs.
//!
//! What they pin is one property, stated in library verification lessons volume
//! 4 §85: **a verdict derived from a sub-process must separate "it looked and
//! found nothing" from "it could not look".** `grep` answers 0 for found, 1 for
//! not found, and 2 or above for "I could not run"; a status of 128 or more is a
//! signal, and 141 is SIGPIPE. Every idiom that reads such a status as a boolean
//! — `if ! ... | grep -q` above all — collapses the third case into the second,
//! and the gate then prints a confident, specific, wrong finding naming a file
//! that is perfectly correct.
//!
//! Both gates did exactly that until 2026-09-05, and the licence gate was
//! observed doing it by four separate arcs before the cause was found.
//!
//! # Why the race itself is pinned structurally
//!
//! The defect that actually reddened was a race: `printf '%s\n' "$header" |
//! grep -qxF ...` under `set -o pipefail`, where `grep -q` exits on match and
//! can be gone before `printf` finishes writing, so `printf` dies of SIGPIPE,
//! `pipefail` promotes 141 to the pipeline's status, and `if !` reads it as
//! absence. Its rate is a probability — 41 and 76 SIGPIPEs per 4000 pipelines
//! with the match on lines 1 and 2, against 0 with the match on the last line
//! and 0 with no match at all — and §57's rule is that an instrument whose
//! failure rate is a probability is not an instrument.
//!
//! So the race is pinned by its **cause** rather than by its symptom:
//! [`neither_gate_script_reads_a_verdict_out_of_a_pipeline_into_grep`] fails the
//! moment the pipeline comes back. The forced-status checks below cover the
//! other half — a status the loop reads and cannot interpret — and 141 is used
//! as one of the forced statuses precisely because it is the status the race
//! produced.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

/// The repository root, from this crate's manifest rather than from the cwd.
///
/// `cargo test` sets the cwd to the package directory, but nothing promises
/// that, and the scripts resolve their own root through git in any case.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("the repository root resolves from this crate's manifest")
}

fn nonce(tag: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    format!("zaru-gate-{tag}-{stamp}-{}", std::process::id())
}

/// A directory holding one executable shim, to be prepended to `PATH`.
///
/// The shim is how a status the machine would only produce under load is
/// produced on demand. Forcing it is the point: waiting for the load to
/// reproduce it is what cost three arcs an afternoon.
fn shim(tag: &str, program: &str, exit_code: u8) -> PathBuf {
    let dir = std::env::temp_dir().join(nonce(tag));
    fs::create_dir_all(&dir).expect("could not stage the shim directory");
    let path = dir.join(program);
    fs::write(
        &path,
        format!(
            "#!/usr/bin/env bash\n# Forced by gate_scripts_from_outside.rs\nexit {exit_code}\n"
        ),
    )
    .expect("could not write the shim");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("could not make the shim executable");
    }

    dir
}

/// A command with every inherited `GIT_*` variable stripped.
///
/// **Every child process this file spawns goes through here, and the reason is
/// not hygiene.** `git rebase --exec`, a git hook and `git bisect run` all
/// *export* `GIT_DIR` and `GIT_INDEX_FILE`, and any git a child runs then
/// answers about the repository those name rather than about the directory it
/// was pointed at. A "scratch" repository built under them is not scratch at
/// all: `git init` fails with `fatal: could not set
/// 'core.repositoryformatversion' to '0'`, and the `git add -A` after it stages
/// the scratch tree into the **real** repository's index and marks every real
/// file deleted.
///
/// Measured here on 2026-09-05, by this arc's own
/// `git rebase --exec 'cargo test --workspace --locked'`, which is exactly the
/// step that exists to run each commit alone. Six checks reddened and the
/// rebase stopped with 300 files staged for deletion in a worktree nobody had
/// touched. Nothing on disk was lost — the index alone was rewritten, and
/// `git reset` restored it — but a check that can do that to the tree it is
/// running in is the shape verification lessons Zaru §17 names: a check that
/// changes shared state owes all of it back.
///
/// The gate scripts need it as much as the scratch repository does. Both
/// resolve their root with `git rev-parse --show-toplevel` and their population
/// with `git ls-files`, so an inherited `GIT_DIR` silently points them at this
/// repository while the check believes it is asking about a temporary one.
/// Removed whether or not this process carries them, so that the removal is a
/// property of the command rather than of the environment the check happened to
/// run in — which is what lets [`every_child_process_is_spawned_outside_this_repository`]
/// assert it without arranging to be inside a rebase.
const EXPORTED_BY_GIT: [&str; 6] = [
    "GIT_DIR",
    "GIT_INDEX_FILE",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

fn git_free_command(program: impl AsRef<std::ffi::OsStr>) -> owned::Owning {
    let mut command = owned::command(program);
    for name in EXPORTED_BY_GIT {
        command.env_remove(name);
    }
    // And anything else git exports that the list above does not name, so a new
    // variable in a future git does not quietly reopen this.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&key);
        }
    }
    command
}

/// Runs a gate script with `shim_dir` ahead of everything else on `PATH`.
fn run_with_shim(script: &str, working_directory: &Path, shim_dir: Option<&Path>) -> Output {
    let inherited = std::env::var("PATH").unwrap_or_default();
    let path = match shim_dir {
        Some(dir) => format!("{}:{inherited}", dir.display()),
        None => inherited,
    };

    git_free_command(repo_root().join("scripts").join(script))
        .current_dir(working_directory)
        .env("PATH", path)
        .output()
        .unwrap_or_else(|error| panic!("could not execute scripts/{script}: {error}"))
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A throwaway git repository, so a check about the gate's verdict never
/// depends on — or disturbs — the state of the repository it is running in.
struct ScratchRepo {
    root: PathBuf,
}

impl ScratchRepo {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(nonce(tag));
        fs::create_dir_all(&root).expect("could not stage the scratch repository");
        let repo = ScratchRepo { root };
        repo.git(&["init", "--quiet"]);
        repo.write("THIRD_PARTY.md", "# Third-party files\n\nNothing lifted.\n");
        repo
    }

    fn git(&self, arguments: &[&str]) {
        // The strip happens first and the identity is set after it, so the four
        // variables below survive while `GIT_DIR` and `GIT_INDEX_FILE` do not.
        let status = git_free_command("git")
            .args(arguments)
            .current_dir(&self.root)
            .env("GIT_AUTHOR_NAME", "Gate Check")
            .env("GIT_AUTHOR_EMAIL", "gate@example.invalid")
            .env("GIT_COMMITTER_NAME", "Gate Check")
            .env("GIT_COMMITTER_EMAIL", "gate@example.invalid")
            .output()
            .expect("could not run git in the scratch repository");
        assert!(
            status.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("could not stage the scratch tree");
        }
        fs::write(path, contents).expect("could not write into the scratch repository");
    }

    /// A file the licence gate is content with.
    fn write_good_source(&self, relative: &str) {
        self.write(
            relative,
            "// Copyright 2026 100monkeys AI, Inc.\n\
             // SPDX-License-Identifier: Apache-2.0\n\
             \n\
             //! A file this gate has no complaint about.\n\
             \n\
             pub fn nothing() {}\n",
        );
    }

    fn stage(&self) {
        self.git(&["add", "-A"]);
    }

    fn licence_gate(&self, shim_dir: Option<&Path>) -> Output {
        run_with_shim("check-license-headers.sh", &self.root, shim_dir)
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// A repository with three correct files, which the gate must pass.
fn staged_clean_repo(tag: &str) -> ScratchRepo {
    let repo = ScratchRepo::new(tag);
    repo.write_good_source("src/one.rs");
    repo.write_good_source("src/two.rs");
    repo.write_good_source("src/three.rs");
    repo.stage();
    repo
}

// ---------------------------------------------------------------------------
// The forced-status arm: a sub-process that could not look.
// ---------------------------------------------------------------------------

/// The mutant: restoring `if ! printf ... | grep -qxF -- "$SPDX"; then`, or any
/// other form that reads a non-zero status as "the line is absent".
#[test]
fn a_grep_that_could_not_look_is_not_read_as_a_missing_header() {
    let repo = staged_clean_repo("grep2");
    let shim_dir = shim("grep2-shim", "grep", 2);

    let output = repo.licence_gate(Some(&shim_dir));
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("could not be checked at all, which is not the same as failing the check"),
        "a grep that could not run must be reported as such; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("(the SPDX grep exited 2)"),
        "the failure must name the command and the status it gave; stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("have no '// SPDX-License-Identifier: Apache-2.0' line"),
        "a grep that could not look must NOT be reported as a missing header -- that is \
         the defect. stderr was:\n{stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the gate must fail, and fail as a gate rather than by dying"
    );
}

/// 141 is SIGPIPE, and SIGPIPE on `printf` is precisely what the race produced:
/// `grep` had *found* the line and exited 0, and the pipeline still answered
/// non-zero. So this check pins the exact status the live defect generated,
/// which no amount of waiting for load can be relied on to reproduce.
#[test]
fn a_grep_killed_by_a_signal_is_not_read_as_a_missing_header() {
    let repo = staged_clean_repo("grep141");
    let shim_dir = shim("grep141-shim", "grep", 141);

    let output = repo.licence_gate(Some(&shim_dir));
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("(the SPDX grep exited 141)"),
        "a signalled grep must name its status; stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("have no '// SPDX-License-Identifier: Apache-2.0' line"),
        "141 read as absence is the defect three arcs lost an afternoon to; stderr was:\n{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// The mutant: dropping `|| header_status=$?`, which puts the read back under
/// `set -e`. That aborts mid-loop, and because the verdict is printed *after*
/// the loop the gate then exits non-zero having printed nothing whatever —
/// verification lessons §25, a gate that dies mid-run prints no failure.
#[test]
fn a_head_that_could_not_read_names_the_file_rather_than_dying_silently() {
    let repo = staged_clean_repo("head2");
    let shim_dir = shim("head2-shim", "head", 2);

    let output = repo.licence_gate(Some(&shim_dir));
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("could not be checked at all"),
        "a head that could not read must produce a gate sentence, not silence; \
         stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("(head exited 2)"),
        "the failure must name head and its status; stderr was:\n{stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the gate must exit 1 as a gate, not 2 by inheriting head's status"
    );
}

/// The same property on the other gate, which shares the shape. Forced rather
/// than observed: this one has never been seen to redden, because its needle
/// spans a whole single-line trailer and `grep` therefore always reads to the
/// end. That is a property of the payload, and a payload is not a guarantee.
#[test]
fn a_grep_that_could_not_look_is_not_read_as_a_missing_signoff() {
    let repo = ScratchRepo::new("dco2");
    repo.write("README.md", "first\n");
    repo.stage();
    repo.git(&[
        "commit",
        "--quiet",
        "-m",
        "first\n\nSigned-off-by: Gate Check <gate@example.invalid>",
    ]);
    repo.write("README.md", "second\n");
    repo.stage();
    repo.git(&[
        "commit",
        "--quiet",
        "-m",
        "second\n\nSigned-off-by: Gate Check <gate@example.invalid>",
    ]);

    let shim_dir = shim("dco2-shim", "grep", 2);
    let output = run_with_shim("check-dco.sh", &repo.root, Some(&shim_dir));
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("could not be checked at all, which is not the same as failing the check"),
        "a grep that could not run must be reported as such; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("(the sign-off grep exited 2)"),
        "the failure must name the command and the status; stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("carry no Signed-off-by matching their author"),
        "a grep that could not look must NOT be reported as a missing sign-off; \
         stderr was:\n{stderr}"
    );
}

// ---------------------------------------------------------------------------
// The structural arm: the race, pinned by its cause.
// ---------------------------------------------------------------------------

/// The mutant this exists for: writing `printf '%s\n' "$x" | grep -q ...` back
/// into either script. Every forced-status check above would still pass, because
/// the pipeline only misreports under a scheduling race that no check can be
/// made to lose on demand.
///
/// Comment lines are excluded deliberately — both scripts *describe* the
/// forbidden shape at length, and a check that could not tell an explanation
/// from an instruction would forbid explaining the defect it prevents.
#[test]
fn neither_gate_script_reads_a_verdict_out_of_a_pipeline_into_grep() {
    let scripts = ["check-license-headers.sh", "check-dco.sh"];

    for script in scripts {
        let path = repo_root().join("scripts").join(script);
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("could not read scripts/{script}: {error}"));

        let offenders: Vec<(usize, &str)> = body
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim_start().starts_with('#'))
            .filter(|(_, line)| {
                line.split('|')
                    .skip(1)
                    .any(|after| after.trim_start().starts_with("grep"))
            })
            .map(|(index, line)| (index + 1, line))
            .collect();

        assert!(
            offenders.is_empty(),
            "scripts/{script} pipes into grep on {} line(s): {offenders:?}. \
             A pipeline's status under `set -o pipefail` is the last non-zero of ANY \
             element, so `grep -q` exiting early on a match can kill the writer with \
             SIGPIPE and the pipeline answers 141 having FOUND what it was looking for. \
             Read the needle from a here-string and the status from an explicit capture.",
            offenders.len()
        );
    }
}

/// The mutant: dropping the strip from [`git_free_command`], or spawning a
/// child anywhere in this file without it.
///
/// This is a check about the check, and it earns its place because the thing it
/// prevents is damage to the repository rather than a wrong verdict. Under
/// `git rebase --exec` — the step that runs each commit alone, and therefore
/// the step this file is guaranteed to meet — an inherited `GIT_DIR` turned
/// this file's scratch repository into the real one and staged 300 deletions
/// into its index before anything asserted.
#[test]
fn every_child_process_is_spawned_outside_this_repository() {
    let command = git_free_command("git");
    let removed: Vec<String> = command
        .get_envs()
        .filter(|(_, value)| value.is_none())
        .filter_map(|(key, _)| key.into_string().ok())
        .collect();

    for name in EXPORTED_BY_GIT {
        assert!(
            removed.iter().any(|removed| removed == name),
            "{name} is still inherited by a child process. git exports it during \
             `rebase --exec`, a hook and `bisect run`, and a child git then acts on \
             THIS repository while the check believes it is acting on a temporary \
             one. Removed: {removed:?}"
        );
    }

    let body = fs::read_to_string(file!())
        .or_else(|_| fs::read_to_string(repo_root().join(file!())))
        .expect("this check can read its own source");
    // Built at run time rather than written as one literal, so this check's own
    // source line does not carry the shape it is looking for and match itself.
    // Every process in `tests/` starts through `owned::command` (see
    // `corpus_every_process_a_check_starts_is_owned`), so that is the
    // constructor that could bypass the strip here, beside `std`'s own.
    let spawns = [
        format!("Command{}new(", "::"),
        format!("owned{}command(", "::"),
    ];
    let permitted = format!("let mut command = {}program)", spawns[1]);
    let bare: Vec<usize> = body
        .lines()
        .enumerate()
        .filter(|(_, line)| spawns.iter().any(|spawn| line.contains(spawn)))
        .filter(|(_, line)| !line.contains(&permitted))
        .map(|(index, _)| index + 1)
        .collect();
    assert!(
        bare.is_empty(),
        "line(s) {bare:?} spawn a child without git_free_command, so it inherits \
         GIT_DIR. Every spawn in this file goes through git_free_command."
    );
}

// ---------------------------------------------------------------------------
// The positive controls: nothing above weakened what the gate checks.
// ---------------------------------------------------------------------------

/// Verification lessons §8: an absence is evidence only when the instrument
/// could have found something. Without these two, every check above is
/// satisfied by a gate that reports nothing about anything.
#[test]
fn the_licence_gate_still_names_a_file_with_no_spdx_line() {
    let repo = staged_clean_repo("missing");
    repo.write(
        "src/bare.rs",
        "// Copyright 2026 100monkeys AI, Inc.\n\npub fn nothing() {}\n",
    );
    repo.stage();

    let output = repo.licence_gate(None);
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("1 of 4 file(s) have no '// SPDX-License-Identifier: Apache-2.0' line"),
        "the gate must still catch a genuinely missing SPDX line; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("src/bare.rs"),
        "and it must name the file; stderr was:\n{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn the_licence_gate_still_names_a_foreign_holder_with_no_manifest_row() {
    let repo = staged_clean_repo("foreign");
    repo.write(
        "src/lifted.rs",
        "// Copyright 2019 Some Other Project\n\
         // SPDX-License-Identifier: Apache-2.0\n\
         \n\
         pub fn nothing() {}\n",
    );
    repo.stage();

    let output = repo.licence_gate(None);
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("carrying a third-party copyright have no row in THIRD_PARTY.md"),
        "ADR-0003 D6's arm must still fire; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("(holder: Some Other Project)"),
        "and it must name the holder it found; stderr was:\n{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// The gate's own staging assertion, which the fix must not have disturbed: a
/// repository the discovery predicate matches nothing in is a broken gate, not
/// a clean tree. This is the clause ADR-0003's trigger 3 names.
#[test]
fn the_licence_gate_still_refuses_a_population_of_zero() {
    let repo = ScratchRepo::new("empty");
    repo.write("README.md", "no rust here\n");
    repo.stage();

    let output = repo.licence_gate(None);
    let stderr = stderr_of(&output);

    assert!(
        stderr.contains("the discovery predicate matched no files"),
        "a zero count must still be a failure; stderr was:\n{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// And the green sentence itself, so a change that made every arm unreachable
/// would be caught rather than read as a pass.
#[test]
fn the_licence_gate_passes_a_clean_repository_and_says_what_it_checked() {
    let repo = staged_clean_repo("clean");

    let output = repo.licence_gate(None);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    assert!(
        stdout.contains("license-header: OK -- 3 file(s) checked, 0 carrying a third-party"),
        "the gate must print the count it checked; stdout was:\n{stdout}"
    );
    assert_eq!(output.status.code(), Some(0));
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
