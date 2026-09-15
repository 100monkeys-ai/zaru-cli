// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0002] D6's two retrieval commands and D8's `tips` key, driven the way
//! a person drives them.
//!
//! # Why the binary rather than more unit checks
//!
//! D6's claim is that "growth is always available on demand", and what a
//! person can demand is a word typed at a shell. A check over
//! `Run::execute` asserts that a request maps to lines; only the artefact
//! asserts that `zaru inbox` is a command at all — which it was not until
//! 2026-09-15, when it was one of four namespaces the parser refused before
//! it reached a request.
//!
//! The scratch home and the cleared environment are
//! `cli_from_outside`'s, for the reasons that file gives.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output

use std::path::PathBuf;
use std::process::{Command, Output};
use zaru_cli::cli::Namespace;
use zaru_cli::compose::tips::{NO_DEPOSITS, NOTHING_LEARNED};

/// A scratch `$HOME` that removes itself.
struct Home {
    path: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-tips-outside-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(path.join("project")).expect("staging: the project");
        Self { path }
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn project(&self) -> PathBuf {
        self.path.join("project")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// What one run of the binary produced.
struct Ran {
    stdout: String,
    stderr: String,
    code: i32,
}

/// Run the built binary with a scratch home and a cleared environment.
fn zaru(home: &Home, arguments: &[&str]) -> Ran {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .current_dir(home.project())
        .output()
        .expect("failed to execute the built zaru binary");
    let ran = Ran {
        stdout: String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("zaru printed invalid UTF-8 on stderr"),
        code: output
            .status
            .code()
            .expect("the binary was killed by a signal rather than exiting"),
    };
    println!("-- zaru {} --", arguments.join(" "));
    for line in ran.stdout.lines() {
        println!("   1| {line}");
    }
    for line in ran.stderr.lines() {
        println!("   2| {line}");
    }
    println!("   exit {}", ran.code);
    ran
}

/// [ADR-0002] D6: "`/learned` — what this session wrote … `/inbox` — pending
/// deposits. Growth is always available on demand."
///
/// # What discriminates
///
/// Both **answer**, on standard output, at `0`. Until 2026-09-15 each was
/// refused on standard error at `2` as a namespace this harness "does not
/// implement yet", and the discriminating arm is the stream and the code
/// rather than the words: a refusal that happened to carry the same sentence
/// would still be a refusal, and D6's clause is about a command that answers.
///
/// The sentences are compared against the product's own constants rather than
/// against literals written here, so a reworded line is one edit and the two
/// surfaces cannot drift.
///
/// The mutant: `Namespace::is_built` answering `false` for either.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn adr_0002_d6s_two_retrieval_commands_answer_rather_than_refuse() {
    let home = Home::new("d6-answers");

    for (word, expected) in [("inbox", NO_DEPOSITS), ("learned", NOTHING_LEARNED)] {
        let ran = zaru(&home, &[word]);
        assert_eq!(
            ran.code, 0,
            "`zaru {word}` did not answer; it exited {} saying {:?}",
            ran.code, ran.stderr
        );
        assert_eq!(
            ran.stdout.lines().collect::<Vec<&str>>(),
            vec![expected],
            "`zaru {word}` printed something other than its one line"
        );
        assert!(
            ran.stderr.is_empty(),
            "`zaru {word}` answered and also wrote to standard error: {:?}",
            ran.stderr
        );
    }
}

/// [ADR-0015] D2's flag surface: a namespace that takes no verb is a whole
/// command, and a word after it is refused naming the word.
///
/// # What discriminates
///
/// The accepting sibling is the bare spelling in the check above. Without
/// this arm, a namespace that silently absorbed its arguments would pass —
/// which is the defect `keys-in-session` measured on `/notes tokens add` and
/// filed, where a command ran a *different* command and said nothing about
/// the words it dropped.
///
/// The mutant: `Namespace::Inbox` reaching `Request::Inbox` without going
/// through `whole`.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[test]
fn a_word_after_either_retrieval_command_is_refused_naming_the_word() {
    let home = Home::new("d6-extra-word");

    for word in ["inbox", "learned"] {
        let ran = zaru(&home, &[word, "please"]);
        assert_ne!(
            ran.code, 0,
            "`zaru {word} please` was answered as though the word were not there"
        );
        assert!(
            ran.stderr.contains("please"),
            "the refusal does not name the word that was dropped: {:?}",
            ran.stderr
        );
    }
}

/// [ADR-0015] D2's `--help` "lists exactly what runs and nothing else".
///
/// # What discriminates
///
/// The two rows are asserted present **and** the two namespaces this build
/// still does not implement are asserted absent, so a help text that listed
/// every namespace would fail on the second arm. Both sides are walked from
/// [`Namespace::ALL`] rather than listed here.
///
/// The mutant: `cli::help`'s rows for either namespace removed.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[test]
fn the_help_text_lists_the_two_retrieval_commands_and_not_the_two_unbuilt_ones() {
    let home = Home::new("d6-help");
    let ran = zaru(&home, &["--help"]);
    assert_eq!(ran.code, 0);

    let listed: Vec<&str> = ran
        .stdout
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.is_empty())
        .collect();

    for namespace in Namespace::ALL {
        let row = listed
            .iter()
            .any(|line| line.starts_with(namespace.subcommand()));
        assert_eq!(
            row,
            namespace.is_built(),
            "`--help` and `Namespace::is_built` disagree about `{}`",
            namespace.subcommand()
        );
    }
}

/// Exactly two of D2's twelve namespaces have no answer at all, and they are
/// named.
///
/// # What discriminates, and why a count alone would not
///
/// A count pins the number and a name pins which, and this asserts both: an
/// arm moved to the wrong side leaves the count right and the set wrong.
/// `/stack` waits on [ADR-0003] D7's component fetch and `/memory` on
/// [ADR-0031]'s relationship memory — neither of which is a sentence this
/// harness could state, where D6's two are.
///
/// The mutant: `Stack` or `Memory` moved into the built arm.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
#[test]
fn exactly_two_namespaces_are_unbuilt_and_they_are_stack_and_memory() {
    let unbuilt: Vec<&str> = Namespace::ALL
        .into_iter()
        .filter(|namespace| !namespace.is_built())
        .map(Namespace::subcommand)
        .collect();
    assert_eq!(
        unbuilt,
        vec!["stack", "memory"],
        "the set of namespaces with no answer is not the two that have none to give"
    );
}

/// [ADR-0002] D8: "`tips = false` in `zaru.toml` disables both."
///
/// # What discriminates, and the half of D8's sentence that does not hold
///
/// A declared key is one [ADR-0014] D5 does **not** refuse, and one D3's
/// explain block can render. The key is asserted settable at layer 2 and at
/// layer 4, each shown effective on the block, with an undeclared sibling that
/// is refused — so this cannot pass against a binary that accepts everything.
///
/// **And the third arm is the measurement rather than the mechanism.**
/// `zaru.toml` is the file D8 names and it is *also* [ADR-0009] D1's project
/// manifest, whose reader declares three top-level tables; `tips = false`
/// there is refused by name. That is pinned here so it cannot change
/// silently, with both readings recorded on
/// [`zaru_cli::compose::tips::KEY`] and neither taken.
///
/// The mutant: `compose::tips::declare` removed from `cli::layers::schema`.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn d8s_tips_key_is_declared_at_the_layers_that_can_carry_it() {
    let home = Home::new("d8-key");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("staging: the harness directory");
    std::fs::write(home.path().join(".zaru/config.toml"), "tips = false\n")
        .expect("staging: layer 2");

    let layer_two = zaru(&home, &["config", "explain", "tips"]);
    assert_eq!(
        layer_two.code, 0,
        "`tips` is not a key this binary declares: {:?}",
        layer_two.stderr
    );
    assert!(
        layer_two
            .stdout
            .lines()
            .any(|line| line.contains("config.toml") && line.contains("← effective")),
        "layer 2's value is not the effective one: {}",
        layer_two.stdout
    );

    // Layer 4, which outranks it, so the two arms are not one fact twice.
    let layer_four: Output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(["config", "explain", "tips"])
        .env_clear()
        .env("HOME", home.path())
        .env("ZARU_TIPS", "false")
        .current_dir(home.project())
        .output()
        .expect("failed to execute the built zaru binary");
    let rendered = String::from_utf8(layer_four.stdout).expect("zaru printed invalid UTF-8");
    assert_eq!(layer_four.status.code(), Some(0));
    assert!(
        rendered
            .lines()
            .any(|line| line.contains("ZARU_TIPS") && line.contains("← effective")),
        "layer 4's value is not the effective one: {rendered}"
    );

    // The sibling: a key that is genuinely not declared.
    let sibling = zaru(&home, &["config", "explain", "tips.enabled"]);
    assert_ne!(
        sibling.code, 0,
        "an undeclared key was explained, so the arms above assert nothing"
    );

    // D8's own spelling, measured. See this check's documentation.
    std::fs::write(home.project().join("zaru.toml"), "tips = false\n")
        .expect("staging: the manifest");
    let manifest = zaru(&home, &["config", "explain", "tips"]);
    assert_ne!(
        manifest.code, 0,
        "`zaru.toml` now carries `tips`; ADR-0009 D1's manifest gained a top-level key and this \
         check and `compose::tips::KEY`'s documentation both want rewriting"
    );
    assert!(
        manifest
            .stderr
            .contains("is not something a manifest declares"),
        "the manifest refused `tips` for some other reason: {:?}",
        manifest.stderr
    );
}
