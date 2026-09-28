// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every check in a test binary, re-run under a home and an environment none
//! of them was handed.
//!
//! # The two defects this exists for
//!
//! **The home, measured on 2026-09-27 by the `test-home-isolation` arc.** The
//! workspace suite was red on any machine whose owner uses Zaru and green on a
//! CI runner: 1,502 passed and 1 failed under the real `HOME`, 1,503 and 0
//! under an empty one. Checks minted under a scratch root and then read
//! configuration and the credential store through the process's own `$HOME`.
//! `zaru_cli::config::Home` is the fix.
//!
//! **The environment, measured the same day by the `test-env-isolation`
//! arc.** With one undeclared `ZARU_` variable exported, three checks went
//! red — `terminal::tests::a_slash_command_produces_what_its_subcommand_spelling_produces`,
//! `a_caller_outside_this_crate_opens_a_shell_over_a_session_and_leaves` and
//! `corpus_a_bare_zaru_at_a_terminal_opens_a_new_sessions_shell` — because
//! configuration layer 4 was folded from `std::env::vars()` inside the
//! product, and [ADR-0014] D5 refused a variable nobody had handed those
//! checks. The sealing key was read from the process the same way.
//! `zaru_cli::config::Variables` is the fix.
//!
//! # Why a re-run, and not a check of each reader
//!
//! A check cannot set `HOME` or a variable for itself — `set_var` is `unsafe`
//! in this edition and the workspace denies `unsafe_code` — so the only way to
//! put a binary's checks under a home and an environment they were not handed
//! is to start them again in a process that has them. What that asserts is the
//! property rather than a list of readers: **a check added tomorrow that reads
//! the process's home or environment fails here without anybody remembering to
//! add it.** Every test binary in this crate carries one, and
//! `corpus_every_test_binary_re_runs_itself_under_a_decoy` in
//! `tests/files_from_outside.rs` walks for a binary that does not.
//!
//! # What the decoy is
//!
//! **A canary is something whose reading changes an answer.** Access times are
//! mount options, and a check that relied on `atime` would be green on every
//! `noatime` machine for the reason it exists to catch. So:
//!
//! - the home's `config.toml` names a key no record declares, which
//!   [ADR-0014] D5 refuses whole, and its `credentials.json` is not a store;
//! - every `ZARU_` variable the developer's shell exported is removed, by
//!   name — this module reads names and never a value — so a check that
//!   passes only because of one is found;
//! - an undeclared `ZARU_` variable is planted, which D5 refuses;
//! - `ZARU_CREDENTIAL_KEY` is planted holding text that is not a key, so a
//!   check that unseals with the process's key fails where it would have
//!   passed under a developer's real one.
//!
//! A variable a check plants for a process **it** spawns is that check's own
//! and is untouched: the removal is of what this process inherited, and every
//! spawn of `zaru` already clears its environment before it sets anything.
//!
//! # What it asserts
//!
//! Four arms, and the first two are what keep the others from passing
//! vacuously: the re-run ran every other check this binary lists — no more,
//! no fewer — and every one of them kept its verdict; the home is byte for
//! byte what was planted; and nothing quoted the canary without failing.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use std::path::{Path, PathBuf};

use super::owned;

/// What the decoy's files and variables hold, and the word a failure quotes.
const CANARY: &str = "canary_a_check_read_what_it_was_not_handed";

/// A `ZARU_` name no record declares, so [ADR-0014] D5 refuses any fold that
/// reads it.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
const UNDECLARED: &str = "ZARU_CANARY_NO_RECORD_DECLARES";

/// The prefix layer 4 reads, whose inherited names the re-run does not get.
const PREFIX: &str = "ZARU_";

/// A home nobody handed to any check, removed when the guard is done.
struct Home {
    path: PathBuf,
}

impl Home {
    fn planted(binary: &Path) -> Self {
        let name = binary
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let path = std::env::temp_dir().join(format!("zaru-decoy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let zaru = path.join(".zaru");
        std::fs::create_dir_all(&zaru).expect("a decoy home");
        std::fs::write(zaru.join("config.toml"), format!("{CANARY} = true\n"))
            .expect("the configuration canary");
        std::fs::write(zaru.join("credentials.json"), format!("{CANARY}\n"))
            .expect("the credential canary");
        Self { path }
    }

    /// Every file under the decoy with its bytes, so a write is seen as well
    /// as a read.
    fn contents(&self) -> Vec<(PathBuf, Vec<u8>)> {
        every_file_under(&self.path)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn every_file_under(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the decoy is readable") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                found.push((path.clone(), Vec::new()));
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("a decoy file is readable");
                found.push((path, bytes));
            }
        }
    }
    found.sort();
    found
}

/// How many checks this binary holds, as its own harness lists them.
fn listed(binary: &Path) -> usize {
    let output = owned::command(binary)
        .args(["--list", "--format", "terse"])
        .output()
        .expect("the test binary lists its checks");
    assert!(
        output.status.success(),
        "the test binary would not list its checks: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.ends_with(": test"))
        .count()
}

/// The number after `word` in a `test result:` line, summed over every such
/// line.
fn summed(stdout: &str, word: &str) -> usize {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("test result: "))
        .flat_map(|result| result.split("; "))
        .filter_map(|part| part.strip_suffix(word))
        .filter_map(|count| count.trim().rsplit(' ').next()?.parse::<usize>().ok())
        .sum()
}

/// Re-run every check in this binary but `guard` under the decoy, and fail
/// if any of them changes its verdict.
///
/// `guard` is the calling check's own name, which the re-run skips or it
/// would start itself for ever.
pub fn every_other_check_keeps_its_verdict(guard: &str) {
    let binary = std::env::current_exe().expect("the test binary knows where it is");
    let others = listed(&binary) - 1;

    let home = Home::planted(&binary);
    let planted = home.contents();

    let mut rerun = owned::command(&binary);
    rerun.args(["--skip", guard, "--exact"]);
    rerun.env("HOME", &home.path);
    // Names only. The value of an inherited `ZARU_` variable is never read
    // here: it is removed, and what it held is the developer's business.
    let mut removed = Vec::new();
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with(PREFIX) {
            rerun.env_remove(&name);
            removed.push(name);
        }
    }
    rerun.env(UNDECLARED, CANARY);
    rerun.env(zaru_cli::credentials::CREDENTIAL_KEY_VARIABLE, CANARY);
    let output = rerun.output().expect("the test binary re-runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let passed = summed(&stdout, " passed");
    let ignored = summed(&stdout, " ignored");
    let failed: Vec<&str> = stdout
        .split("\nfailures:\n")
        .nth(2)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("test result"))
        .collect();
    println!(
        "-- re-run under {} with {} inherited `{PREFIX}` name(s) removed and two planted --",
        home.path.display(),
        removed.len()
    );
    println!("   {passed} passed, {ignored} ignored, of {others} listed; failed: {failed:?}");

    assert!(
        output.status.success(),
        "{} check(s) here change their verdict under a home and an environment none of them was \
         handed, so each reads the process's own `~/.zaru` or `ZARU_*` variables instead of what \
         it was given, and is red or green by whose machine runs it: {failed:?}\n{stdout}\n{stderr}",
        failed.len(),
    );
    assert_eq!(
        passed + ignored,
        others,
        "the re-run accounted for {passed} passed and {ignored} ignored check(s) where this binary \
         lists {others} besides the guard, so it did not put every check under the decoy:\n{stdout}"
    );
    assert_eq!(
        home.contents(),
        planted,
        "a check wrote into a home it was not handed"
    );
    assert!(
        !stdout.contains(CANARY) && !stderr.contains(CANARY),
        "the re-run passed and still quoted the canary, so something read the decoy and said so \
         without failing:\n{stdout}\n{stderr}"
    );
}
