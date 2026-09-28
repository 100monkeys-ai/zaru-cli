// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Retrieval by meaning, from outside the binary: off unless a person turns
//! it on, and nothing fetched unless a person answers yes.
//!
//! Every check runs the built `zaru` under a home it owns, with a cleared
//! environment. None needs a network or the model.

use std::path::PathBuf;

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "zaru-meaning-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory can be created");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `zaru` with `arguments` in `home`, with `environment` and nothing else,
/// and standard input that is not a terminal.
fn zaru(home: &Scratch, arguments: &[&str], environment: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let output = owned::command(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .current_dir(&home.0)
        .env_clear()
        .env("HOME", &home.0)
        .envs(environment.iter().copied())
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the built zaru runs");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// **`search.meaning` is off at layer 1**, and `ZARU_SEARCH_MEANING=true`
/// at layer 4 turns it on.
///
/// **The mutant:** the key not declared, which prints the refusal `config
/// explain` gives an unknown key.
#[test]
fn retrieval_by_meaning_is_off_by_default_and_explained_by_layer() {
    let home = Scratch::new("explain");
    let (code, stdout, stderr) = zaru(&home, &["config", "explain", "search.meaning"], &[]);
    assert_eq!(code, Some(0), "`zaru config explain search.meaning` refused: {stderr}");
    assert!(
        stdout.lines().next() == Some("search.meaning = false")
            && stdout.lines().any(|line| line.contains("built-in")
                && line.contains("false")
                && line.contains("← effective")),
        "retrieval by meaning is not off at layer 1: {stdout}"
    );
    let (code, stdout, stderr) = zaru(
        &home,
        &["config", "explain", "search.meaning"],
        &[("ZARU_SEARCH_MEANING", "true")],
    );
    assert_eq!(code, Some(0), "the layer-4 spelling refused: {stderr}");
    assert!(
        stdout.lines().next() == Some("search.meaning = true"),
        "ZARU_SEARCH_MEANING=true is not the effective answer: {stdout}"
    );
}

/// **A repository cannot turn it on.** A `zaru.toml` that tries is refused,
/// and the accepting arm is the same home with no such file.
#[test]
fn a_project_cannot_turn_retrieval_by_meaning_on() {
    let home = Scratch::new("project");
    std::fs::write(home.0.join("zaru.toml"), "[search]\nmeaning = true\n").expect("staging");
    let (code, stdout, _) = zaru(&home, &["config", "explain", "search.meaning"], &[]);
    assert_ne!(code, Some(0), "a project's zaru.toml turned retrieval by meaning on: {stdout}");
    std::fs::remove_file(home.0.join("zaru.toml")).expect("staging");
    let (code, _, stderr) = zaru(&home, &["config", "explain", "search.meaning"], &[]);
    assert_eq!(code, Some(0), "the accepting arm: {stderr}");
}

/// **`zaru index` on a fresh home says it is off and not fetched, and makes
/// nothing.**
#[test]
fn zaru_index_on_a_fresh_home_says_off_and_not_fetched_and_makes_nothing() {
    let home = Scratch::new("status");
    let (code, stdout, stderr) = zaru(&home, &["index"], &[]);
    assert_eq!(code, Some(0), "`zaru index` refused: {stderr}");
    assert!(
        stdout.contains("Retrieval by meaning is off.")
            && stdout.contains("are not fetched")
            && stdout.contains("This project has no index yet."),
        "{stdout}"
    );
    assert!(
        !home.0.join(".zaru/meaning").exists(),
        "`zaru index` made the folder it only reports on"
    );
}

/// **`zaru index fetch` says what it would fetch, from where, how large and
/// where it would keep it, and with no terminal to answer on it fetches
/// nothing.**
///
/// **The mutant:** the question skipped when there is no terminal, which
/// starts a download (and makes `~/.zaru/meaning`).
#[test]
fn zaru_index_fetch_says_what_it_would_fetch_and_without_a_terminal_fetches_nothing() {
    let home = Scratch::new("fetch");
    let (code, stdout, stderr) = zaru(&home, &["index", "fetch"], &[]);
    assert_ne!(code, Some(0), "a fetch nobody answered succeeded: {stdout}");
    assert!(
        stderr.contains("asks before it fetches anything"),
        "the refusal does not say why: {stderr}"
    );
    for said in [
        "The model bge-base-en-v1.5 (MIT licence): 5 files, 436.5 MB in all.",
        "https://huggingface.co/Xenova/bge-base-en-v1.5, revision 4d6cd88e18e51a5e020c2c305726d76ada9c03cf",
        "The ONNX Runtime library 1.28.2 (MIT licence)",
        "https://github.com/microsoft/onnxruntime/releases/download/v1.28.2/",
        "A file that does not match is deleted and never loaded.",
        "No text of your code is sent anywhere to be indexed.",
    ] {
        assert!(stdout.contains(said), "the question does not say {said:?}: {stdout}");
    }
    assert!(
        stdout.contains(&home.0.join(".zaru/meaning/model").display().to_string()),
        "the question does not say where the model is kept: {stdout}"
    );
    assert!(
        !home.0.join(".zaru/meaning").exists(),
        "a fetch nobody answered made {}",
        home.0.join(".zaru/meaning").display()
    );
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs`.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
