// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The `zaru` binary, driven the way a person drives it.
//!
//! # Why this file exists rather than more unit checks
//!
//! Every other record's Status tracking in this workspace carries the same
//! sentence: "that capture is evidence about the mechanism and **must not be
//! quoted as evidence about the binary**". This file is the other kind. It
//! runs the artefact Cargo built, with arguments, and reads its standard
//! output, its standard error and its process status.
//!
//! # The scratch home is what makes it honest
//!
//! `SessionStore::default_root` and `CredentialStore::default_root` both go
//! through `std::env::home_dir`, which on this platform reads `$HOME` —
//! measured rather than assumed, by running a probe binary with `HOME` set and
//! with it unset. So each check gives the child its own `HOME` and the child
//! writes to the paths the product actually writes to, inside it. That is
//! [Testing]'s "what a test's own root is here", applied to a process.
//!
//! The environment is **cleared** rather than inherited, and that is not
//! tidiness. [ADR-0014] D5 refuses any `ZARU_*` variable that maps to no
//! declared key, so a developer with one exported would see every check here
//! fail for a reason that has nothing to do with the check.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A scratch `$HOME` that removes itself.
struct Home {
    path: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-cli-from-outside-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch home under the temporary directory");
        std::fs::create_dir_all(path.join("project")).expect("a scratch working directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// The directory the binary is run **in**.
    ///
    /// Every run is given one, and that is a safety property rather than a
    /// convenience: `zaru init` writes into the working directory, and a runner
    /// that inherited the test process's would write a `zaru.toml` into this
    /// repository the first time somebody ran the suite.
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

impl Ran {
    fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }
}

/// Run the built binary with a scratch home and a cleared environment.
fn zaru(home: &Home, arguments: &[&str]) -> Ran {
    zaru_in(home, &home.project(), arguments)
}

/// The same, run **in** a named directory.
///
/// [ADR-0010] D4's `--continue` is scoped to the directory it is run in, so a
/// check of that clause needs more than one, and the working directory a run
/// is given is the thing under test rather than a detail of the runner.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn zaru_in(home: &Home, directory: &Path, arguments: &[&str]) -> Ran {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .current_dir(directory)
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

    println!("-- zaru {} (in {}) --", arguments.join(" "), directory.display());
    for line in ran.stdout.lines() {
        println!("   {line}");
    }
    for line in ran.stderr.lines() {
        println!(" ! {line}");
    }
    println!("   exit {}", ran.code);
    ran
}

/// Run the built binary with something on its standard input.
///
/// The sibling of [`zaru`], and separate rather than an extra argument on it,
/// because only the two `add` commands read standard input at all and a helper
/// that always opened a pipe would change what every other check exercises.
fn zaru_with_input(home: &Home, arguments: &[&str], input: &str) -> Ran {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .current_dir(home.project())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to execute the built zaru binary");
    child
        .stdin
        .as_mut()
        .expect("the child's standard input was piped")
        .write_all(input.as_bytes())
        .expect("the child took its input");
    let output = child.wait_with_output().expect("the child exited");

    let ran = Ran {
        stdout: String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("zaru printed invalid UTF-8 on stderr"),
        code: output
            .status
            .code()
            .expect("the binary was killed by a signal rather than exiting"),
    };
    println!("-- zaru {} (with input) --", arguments.join(" "));
    for line in ran.stdout.lines() {
        println!("   {line}");
    }
    for line in ran.stderr.lines() {
        println!(" ! {line}");
    }
    println!("   exit {}", ran.code);
    ran
}

/// ADR-0016 D5's `0` and `2`, on the artefact rather than on the mapping.
///
/// **This is the clause moving.** That record's Status tracking has said since
/// 2026-09-04 that "from the real artefact only `0` is observable" and that
/// "`zaru` takes no arguments, so nothing a user can do makes it fail". Both
/// halves change here, and neither `1`, `3`, `4` nor `70` does: there is still
/// no loop to exhaust, no network to be unreachable, no tier that withholds
/// anything, and no honest way to make the binary panic.
#[test]
fn the_built_binary_reaches_adr_0016_d5s_zero_and_two_and_no_other_code() {
    let home = Home::new("exit-codes");

    let succeeded: Vec<(&[&str], i32)> = vec![
        (&["--help"], 0),
        (&["--version"], 0),
        (&["runtime"], 0),
        (&["models"], 0),
        (&["config", "explain", "runtime.tier"], 0),
    ];
    let refused: Vec<(&[&str], i32)> = vec![
        (&["runtim"], 2),
        (&["stack"], 2),
        (&["--runtime", "sandboxed", "runtime"], 2),
        (&["config", "explain", "runtime.nonsense"], 2),
        (&["--runtime", "bare", "--runtime", "linked"], 2),
    ];

    let mut wrong = Vec::new();
    for (arguments, expected) in succeeded.iter().chain(refused.iter()) {
        let ran = zaru(&home, arguments);
        if ran.code != *expected {
            wrong.push((arguments.join(" "), ran.code, *expected));
        }
    }

    assert!(
        wrong.is_empty(),
        "the built binary exited with something other than ADR-0016 D5's code for what it did \
         (command, got, wanted): {wrong:?}"
    );
}

/// A refusal goes to standard error and the data goes to standard output.
///
/// D5's argument is that CI wraps this harness. A wrapper reading `zaru
/// models` off a pipe must not one day take a refusal for a row.
#[test]
fn what_a_wrapper_reads_off_the_pipe_is_never_a_refusal() {
    let home = Home::new("streams");

    let listing = zaru(&home, &["models"]);
    assert_eq!(listing.code, 0);
    assert!(
        listing.stderr.is_empty(),
        "a successful listing wrote to standard error: {:?}",
        listing.stderr
    );
    assert_eq!(
        listing.lines().len(),
        5,
        "ADR-0012 D2's five aliases, one line each: {:?}",
        listing.lines()
    );

    let refusal = zaru(&home, &["runtim"]);
    assert_eq!(refusal.code, 2);
    assert!(
        refusal.stdout.is_empty(),
        "a refusal reached standard output, where a wrapper reads its data: {:?}",
        refusal.stdout
    );
    assert!(
        refusal.stderr.contains("zaru runtime"),
        "the refusal must name what to run instead: {:?}",
        refusal.stderr
    );
}

/// A bare `zaru` prints the help, and its first line is still its version.
///
/// The version line is load-bearing twice: `tests/version.rs` reads it for the
/// release checklist's reason, and a user typing the name of a program and
/// nothing else is owed an answer to the question they asked.
#[test]
fn a_bare_zaru_prints_the_help_and_the_composition_moved_behind_a_flag() {
    let home = Home::new("bare");

    let bare = zaru(&home, &[]);
    assert_eq!(bare.code, 0);
    assert_eq!(
        bare.lines().first().copied(),
        Some(concat!("zaru ", env!("CARGO_PKG_VERSION"))),
        "the artefact's first line is its own version"
    );
    assert!(
        bare.stdout.contains("usage:"),
        "a bare `zaru` prints the help: {}",
        bare.stdout
    );
    assert!(
        !bare.stdout.contains("zaru-core"),
        "the composition list is `--version`'s now, not a bare `zaru`'s: {}",
        bare.stdout
    );

    let version = zaru(&home, &["--version"]);
    assert_eq!(version.code, 0);
    assert!(
        version.stdout.contains("zaru-core") && version.stdout.contains("zaru-aegis"),
        "`--version` prints what the binary is composed of: {}",
        version.stdout
    );
}

/// ADR-0014 D3's block, printed by the binary, over the layers it can read.
///
/// The effective row is read out of the **rendered text** rather than from the
/// explanation's own accessor, so the two sides of the assertion do not travel
/// through one code path.
#[test]
fn adr_0014_d3s_block_is_printed_and_a_flag_moves_the_effective_row() {
    let home = Home::new("explain");

    let built_in = zaru(&home, &["config", "explain", "runtime.tier"]);
    assert_eq!(built_in.code, 0);
    let effective: Vec<&str> = built_in
        .lines()
        .into_iter()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(
        effective.len(),
        1,
        "D3 marks exactly one row, and it is the highest layer that *set* the key: {:?}",
        built_in.lines()
    );
    assert!(
        effective[0].contains("built-in") && effective[0].contains("bare"),
        "with nothing configured the effective row is layer 1: {:?}",
        effective[0]
    );
    assert_eq!(
        built_in.lines().len(),
        6,
        "D3's block is the key's line and one row per layer, always five: {:?}",
        built_in.lines()
    );

    let flagged = zaru(
        &home,
        &[
            "--runtime",
            "contained",
            "config",
            "explain",
            "runtime.tier",
        ],
    );
    let effective: Vec<&str> = flagged
        .lines()
        .into_iter()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(effective.len(), 1);
    assert!(
        effective[0].contains("flag") && effective[0].contains("contained"),
        "a flag is layer 5 and wins, and the block says which layer supplied it: {:?}",
        effective[0]
    );
}

/// ADR-0012 D4's listing, printed by the binary, naming the supplying layer.
///
/// Both arms: an alias nothing set renders as D3's `(not set)`, and one a flag
/// set names the layer. The unresolved arm alone would pass against a listing
/// that never resolves anything.
#[test]
fn adr_0012_d4s_listing_names_what_each_alias_resolved_to_and_which_layer() {
    let home = Home::new("models");

    let nothing = zaru(&home, &["models"]);
    assert_eq!(nothing.code, 0);
    for line in nothing.lines() {
        assert!(
            line.contains("(not set)"),
            "with nothing configured every alias is unresolved, in D3's own spelling: {line:?}"
        );
    }

    let flagged = zaru(&home, &["--model", "a-model-identifier", "models"]);
    assert_eq!(flagged.code, 0);
    let default: Vec<&str> = flagged
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("  default"))
        .collect();
    assert_eq!(default.len(), 1, "one row per alias: {:?}", flagged.lines());
    assert!(
        default[0].contains("a-model-identifier") && default[0].contains("flag"),
        "D4 prints what the alias resolved to and which layer supplied it: {:?}",
        default[0]
    );
    assert_eq!(
        flagged
            .lines()
            .into_iter()
            .filter(|line| line.contains("(not set)"))
            .count(),
        4,
        "`--model` sets `model.default` and nothing else: {:?}",
        flagged.lines()
    );
}

/// ADR-0001 D2's datum, printed, with D1's cells and the diff to each tier.
#[test]
fn adr_0001_d2s_datum_is_printed_with_d1s_own_cells() {
    let home = Home::new("runtime");

    let ran = zaru(&home, &["--runtime", "contained", "runtime"]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.lines()[0].contains("runtime.tier = contained (from flag)"),
        "the first line is the tier and where it came from: {:?}",
        ran.lines()[0]
    );
    assert!(
        ran.stdout.contains("local containers"),
        "D1's Membrane cell for `contained`, in the record's own wording: {}",
        ran.stdout
    );

    let mut altered: Vec<&str> = ran
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("changing to "))
        .collect();
    altered.sort_unstable();
    assert_eq!(
        altered,
        vec![
            "changing to bare would alter",
            "changing to linked would alter"
        ],
        "D2's \"what changing it would alter\" is answered for every other tier, and only for the \
         others: {:?}",
        ran.lines()
    );
}

/// The binary creates nothing in `~/.zaru` merely by being asked a question.
///
/// ADR-0010's own inode consequence, and ADR-0014's "a loader that created a
/// directory in order to find nothing in it would be creating state to read
/// state". The control is that the scratch home exists throughout, so a check
/// that reported absence for everything would fail on it.
#[test]
fn asking_the_binary_a_question_writes_nothing_to_the_users_home() {
    let home = Home::new("no-writes");

    for arguments in [
        vec!["--help"],
        vec!["runtime"],
        vec!["models"],
        vec!["config", "explain", "runtime.tier"],
    ] {
        let ran = zaru(&home, &arguments);
        assert_eq!(ran.code, 0);
    }

    assert!(
        home.path().is_dir(),
        "the scratch home itself must survive, or this check reports absence for everything"
    );
    let zaru_home = home.path().join(".zaru");
    assert!(
        !zaru_home.exists(),
        "reading configuration created {}, which is creating state in order to read state",
        zaru_home.display()
    );
}

/// Stage a session under a scratch home, through the crate's own door.
///
/// The store is opened for **writing** here, because a check that stages a
/// session is doing what the product does when it starts one. What the binary
/// under test does is read, and it reads through `SessionStore::reading`.
fn stage_a_session(home: &Home, minted_at: u64) -> zaru_cli::session::SessionId {
    stage_a_session_in(home, minted_at, &home.project(), 1)
}

/// The same, for a session that began in a named directory.
///
/// It writes the `meta.toml` the product writes, through the product's own
/// [`MetaFile`](zaru_cli::session::MetaFile), because [ADR-0010] D4's
/// `--continue` selects on the `directory` that file records and a staging
/// that wrote its own would be asserting against a second format.
///
/// `entropy` distinguishes two sessions minted in the same millisecond; the
/// minting time is named rather than read off the machine's clock, so the
/// order `--continue` picks from is this check's rather than the scheduler's.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn stage_a_session_in(
    home: &Home,
    minted_at: u64,
    directory: &Path,
    entropy: u8,
) -> zaru_cli::session::SessionId {
    use zaru_cli::config::Layer;
    use zaru_cli::runtime::{ResolvedTier, Tier};
    use zaru_cli::session::{
        Meta, MetaFile, MetaStore, Millis, Record, SessionId, SessionStore, Transcript,
    };

    let store = SessionStore::open(home.path().join(".zaru")).expect("a scratch session store");
    let id = SessionId::from_parts(Millis::new(minted_at), [entropy; 10])
        .expect("a well-formed ULID");
    let session = store.start(id.clone()).expect("a session directory");

    // The directory as the product records it: canonical, because
    // `WorkingDirectory::of_this_process` is what `compose::turn::prepare`
    // canonicalises and `most_recent_in` compares against. A staging that
    // wrote the uncanonicalised path would pass against a filter that also
    // skipped the canonicalisation, which is one of this clause's mutants.
    let canonical = std::fs::canonicalize(directory).expect("the staged directory exists");
    let mut meta_file = MetaFile::at(session.meta_path());
    meta_file
        .write(&Meta::new(
            ResolvedTier::supplied(Tier::Bare, Layer::BuiltIn),
            None,
            Some("gemini".to_owned()),
            canonical,
            Millis::new(minted_at),
        ))
        .expect("a staged session records itself");

    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("could not open the transcript");
    for n in 1..=2u32 {
        transcript
            .record(&Record::Loop(
                zaru_core::iteration::Event::IterationStarted { n, of: 3 },
            ))
            .expect("could not append");
    }
    id
}

/// ADR-0010 D6's deletion, reached from a command, with no tombstone.
///
/// **This is the clause moving.** That record's Status tracking has said
/// "what is missing is `sessions rm` as a command" since 2026-09-04.
///
/// Asserted four ways as the pruning check already is — the directory gone,
/// the parent listing, the command's own second listing, and **a sibling
/// session that must survive**, which is the reading that discriminates: a
/// remover that deleted everything passes the first three and fails the
/// fourth.
#[test]
fn adr_0010_d6s_sessions_rm_removes_the_directory_and_spares_its_neighbour() {
    let home = Home::new("sessions-rm");
    let doomed = stage_a_session(&home, 1_700_000_000_000);
    let sibling = stage_a_session(&home, 1_700_000_001_000);
    assert_ne!(doomed, sibling, "the two staged sessions must be distinct");

    let listed = zaru(&home, &["sessions", "list"]);
    assert_eq!(listed.code, 0);
    assert_eq!(
        listed.lines().len(),
        2,
        "both sessions listed: {:?}",
        listed.lines()
    );

    let removed = zaru(&home, &["sessions", "rm", doomed.as_str()]);
    assert_eq!(removed.code, 0);
    assert!(removed.stdout.contains(doomed.as_str()));

    let directory = home
        .path()
        .join(".zaru")
        .join("sessions")
        .join(doomed.as_str());
    assert!(
        !directory.exists(),
        "D6 removes the directory rather than marking it deleted: {} is still there",
        directory.display()
    );

    let remaining: Vec<String> = std::fs::read_dir(home.path().join(".zaru").join("sessions"))
        .expect("the sessions directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        remaining,
        vec![sibling.as_str().to_owned()],
        "the neighbour must survive, or a remover that deletes everything passes"
    );

    let after = zaru(&home, &["sessions", "list"]);
    assert_eq!(after.lines().len(), 1);
    assert!(after.stdout.contains(sibling.as_str()));

    let again = zaru(&home, &["sessions", "rm", doomed.as_str()]);
    assert_eq!(
        again.code, 2,
        "removing a session that is not there is the user's, and the remedy is the listing"
    );
}

/// `sessions list` on a machine that has never had one creates nothing.
///
/// Both halves: the answer is a sentence rather than silence, because an empty
/// listing and a listing that failed look identical; and `~/.zaru` is not
/// created in order to find nothing in it.
#[test]
fn listing_sessions_on_a_fresh_machine_says_so_and_creates_nothing() {
    let home = Home::new("sessions-empty");

    let ran = zaru(&home, &["sessions", "list"]);
    assert_eq!(ran.code, 0);
    assert_eq!(ran.lines(), vec!["no sessions"]);
    assert!(
        home.path().is_dir(),
        "the scratch home must survive, or this reports absence for everything"
    );
    assert!(
        !home.path().join(".zaru").exists(),
        "listing sessions created ~/.zaru, which is creating state in order to read state"
    );
}

/// ADR-0010 D4's resume restores, prints the transcript, and stops.
///
/// The lines printed are the transcript's **own bytes**, read back off the file
/// by this check rather than through the binary, so the two sides of the
/// comparison do not travel through one code path.
///
/// **This was named `..._and_then_refuses` and asserted `2` until
/// 2026-09-05**, when D4's recorded asymmetry was answered: a bare `--resume`
/// asks for no task, so a refusal about there being no provider was an answer
/// to a question nobody put, and the run exits `0`. The name changed with the
/// behaviour, because a check whose name states the old rule is a second
/// statement of it that nothing keeps true.
#[test]
fn adr_0010_d4s_resume_prints_the_transcripts_own_bytes_and_stops() {
    let home = Home::new("resume");
    let id = stage_a_session(&home, 1_700_000_002_000);

    let on_disk: Vec<String> = std::fs::read_to_string(
        home.path()
            .join(".zaru")
            .join("sessions")
            .join(id.as_str())
            .join("transcript.jsonl"),
    )
    .expect("the staged transcript")
    .lines()
    .map(str::to_owned)
    .collect();
    assert_eq!(on_disk.len(), 2, "the staging wrote two records");

    let ran = zaru(&home, &["--resume", id.as_str()]);
    assert_eq!(
        ran.code, 0,
        "a resume that printed the transcript it was asked for did what it was asked; ADR-0010 \
         D4 asks it for no task, so there is nothing left for it to have failed at"
    );
    for line in &on_disk {
        assert!(
            ran.stdout.contains(line.as_str()),
            "the transcript's own bytes must reach the reader: {line:?} is not in {:?}",
            ran.stdout
        );
    }
    assert!(
        ran.stdout.contains("2 record(s) in the transcript"),
        "the restore says what it restored: {}",
        ran.stdout
    );
    assert!(
        ran.stderr.is_empty(),
        "a resume that succeeded wrote to standard error, and ADR-0016 D5 keeps that channel for \
         refusals so a wrapper can tell one from the other: {}",
        ran.stderr
    );

    // `--continue` is the same operation reaching the same reader, so it ends
    // the same way. D4 names both spellings and gives them one behaviour.
    let continued = zaru(&home, &["--continue"]);
    assert_eq!(
        continued.code, 0,
        "`--continue` is `--resume` on the most recent session and must not end differently"
    );
    assert!(
        continued.stdout.contains(id.as_str()),
        "`--continue` takes the most recent session, which a ULID's own order decides: {}",
        continued.stdout
    );
}

/// `--continue` with no sessions says so rather than failing obscurely.
#[test]
fn continuing_with_no_sessions_says_there_is_nothing_to_continue() {
    let home = Home::new("continue-empty");
    let ran = zaru(&home, &["--continue"]);
    assert_eq!(ran.code, 2);
    assert!(
        ran.stderr.contains("no session to continue"),
        "the refusal must say what is missing: {}",
        ran.stderr
    );
    assert!(
        ran.stderr.contains("in this directory"),
        "ADR-0010 D4 scopes `--continue` to a directory, so the refusal says which: {}",
        ran.stderr
    );
}

/// [ADR-0010] D4's `--continue` is **this directory's** most recent session.
///
/// D4, in as many words: "`zaru --continue` for the **most recent session in
/// this directory**". Until 2026-09-06 both entry points took
/// `store.ids().last()`, which is a recency test where the clause asks for a
/// locality one; the `harness-look-and-feel` survey measured it from the built
/// binary at `8179f8a` resuming a session created in a different checkout, and
/// `operations/known-defects` carried it as a `Diagnosed` row.
///
/// Three directories under one home, and each arm kills a different mutant.
///
/// - **A** holds two sessions with the older staged *second*, so a resume that
///   took whatever `read_dir` yielded, or the one it created last, answers
///   differently from one that reads the ULID's own order. This is the arm the
///   check had before the directory term existed, kept.
/// - **B** holds one, so a filter that matched every session answers with A's.
/// - **C** has never held one, so a resume with no directory term — today's
///   code — answers with A's rather than refusing. **This is the arm that
///   discriminates**, and it is the survey's own reproduction.
/// - **`--resume` from C succeeds**, which is the accepting sibling: D4 scopes
///   only `--continue`, so a build that refused everything outside the
///   directory would pass the first three arms and fail this one.
///
/// The staged directories are canonicalised on the way in, so an
/// implementation that compared uncanonicalised paths fails here whenever the
/// temporary directory is reached through a symbolic link — which it is on
/// most machines.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn corpus_continue_is_the_most_recent_session_started_in_this_directory() {
    let home = Home::new("continue-scope");
    let a = home.path().join("a");
    let b = home.path().join("b");
    let c = home.path().join("c");
    for directory in [&a, &b, &c] {
        std::fs::create_dir_all(directory).expect("a scratch working directory");
    }

    let newer_in_a = stage_a_session_in(&home, 1_700_000_009_000, &a, 1);
    let older_in_a = stage_a_session_in(&home, 1_700_000_003_000, &a, 2);
    let only_in_b = stage_a_session_in(&home, 1_700_000_005_000, &b, 3);
    assert!(
        older_in_a < newer_in_a,
        "the ULIDs must sort by their minting time"
    );

    let from_a = zaru_in(&home, &a, &["--continue"]);
    assert_eq!(from_a.code, 0, "stderr: {}", from_a.stderr);
    assert!(
        from_a.stdout.contains(newer_in_a.as_str()),
        "`--continue` must take this directory's most recent session: {}",
        from_a.stdout
    );
    assert!(
        !from_a.stdout.contains(older_in_a.as_str()),
        "and not the one created last: {}",
        from_a.stdout
    );

    let from_b = zaru_in(&home, &b, &["--continue"]);
    assert_eq!(from_b.code, 0, "stderr: {}", from_b.stderr);
    assert!(
        from_b.stdout.contains(only_in_b.as_str()),
        "`--continue` in B must take B's session and not A's newer one: {}",
        from_b.stdout
    );

    // The arm that discriminates. A machine-wide `--continue` answers here
    // with A's session and exits 0; D4's says there is nothing to continue.
    let from_c = zaru_in(&home, &c, &["--continue"]);
    assert_eq!(
        from_c.code, 2,
        "a session started elsewhere is not continued here; stdout: {}",
        from_c.stdout
    );
    assert!(
        !from_c.stdout.contains(newer_in_a.as_str())
            && !from_c.stdout.contains(only_in_b.as_str()),
        "no session from another directory may be resumed by `--continue`: {}",
        from_c.stdout
    );

    // The accepting sibling: `--resume` names a session and is not scoped.
    let resumed = zaru_in(&home, &c, &["--resume", newer_in_a.as_str()]);
    assert_eq!(
        resumed.code, 0,
        "`--resume <id>` names a session and D4 scopes only `--continue`; stderr: {}",
        resumed.stderr
    );
    assert!(
        resumed.stdout.contains(newer_in_a.as_str()),
        "`--resume` must reach a session started in another directory: {}",
        resumed.stdout
    );
}

/// The key port, implemented from outside the crate that declares it.
///
/// The store now seals what it holds, so a check that wants a *populated* store
/// supplies the key it seals under. Until 2026-09-05 this stood in for sealing
/// itself, because ADR-0007 D3's `aes-gcm` and `keyring` had no caller; they do
/// now, and what a check has to supply is a key rather than a cipher.
///
/// A real machine reaches its own keyring through
/// `zaru_cli::credentials::OsKeyring`, which this deliberately does not use:
/// `zaru notes tokens` is being driven here as a binary, and a check that
/// wrote to the developer's own keyring to do it would be a check that changes
/// shared state it does not own.
struct StagedKey(zaru_cli::credentials::SealingKey);

impl StagedKey {
    fn minted() -> Self {
        Self(zaru_cli::credentials::SealingKey::mint())
    }
}

impl zaru_cli::credentials::KeyStore for StagedKey {
    fn key(
        &self,
    ) -> Result<zaru_cli::credentials::SealingKey, zaru_cli::credentials::SealingError> {
        Ok(self.0.clone())
    }
}

struct AlwaysConfirms;

impl zaru_cli::credentials::Confirm for AlwaysConfirms {
    fn confirm_apex(&self, _alias: &zaru_cli::credentials::Alias, _grants: &str) -> bool {
        true
    }
}

/// ADR-0007 D7's listing, printed by the binary over a store it can read.
///
/// **This is the clause moving, and only one of five surfaces.** That record's
/// clause 10 wants all five and says "no binary reaches the credential store";
/// one does now. `add`, `describe`, `rm` and `use` are not built: `add` needs a
/// server, a key and a confirmer; the store's public door has no `describe`
/// and no `rm`; and `use` could only ever refuse, because nothing can put a
/// token in the store for the role to move to.
///
/// The planted bearer value is asserted **absent** from every byte the binary
/// printed, and so is its ASCII core — the credential store's surviving
/// mutation of 2026-09-04, and Verification lessons §50: an absence assertion
/// is blind to whatever the renderer escapes.
#[test]
fn adr_0007_d7s_listing_shows_the_composer_role_and_marks_an_apex_token() {
    use zaru_cli::credentials::{
        Alias, CredentialStore, Description, Entry, Instance, Reach, Secret, ToolScope,
    };

    let home = Home::new("notes-tokens");
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(home.path().join(".zaru")).expect("a scratch store");

    // A nonce with a combining mark, so that an escaping renderer cannot make
    // the absence assertion pass by mangling it. The ASCII core is what no
    // escaping alters.
    let core = format!("csnonce{}", std::process::id());
    let planted = format!("nn_mcp_{core}\u{301}");

    store
        .add(
            Entry::notes(
                Alias::new("work").expect("a legal alias"),
                // Deliberately does not contain the word `composer`. The
                // first version of this check filtered the listing for that
                // word and the description supplied it, so dropping the role
                // column left the check green -- Verification lessons §51: a
                // fixture can be awkward on one axis and ordinary on the axis
                // the mutant moves.
                Description::new("search from the editor").expect("one line"),
                Secret::notes(planted.clone()).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::new(["pages.read", "search.global"]))
            .with_workspace("zaru"),
            &keys,
            None,
        )
        .expect("the token is stored");
    store
        .add(
            Entry::notes(
                Alias::new("everywhere").expect("a legal alias"),
                Description::new("an operator token").expect("one line"),
                Secret::notes(format!("nn_app_{core}-second")).expect("nn_app_ names a kind"),
                Reach::Apex,
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::new(["pages.read"])),
            &keys,
            Some(&AlwaysConfirms),
        )
        .expect("a confirmed apex token is stored");
    store
        .grant_composer_role(&Alias::new("work").expect("a legal alias"))
        .expect("ADR-0006 D4's scope holds the role");
    store.save().expect("the store is written");

    let ran = zaru(&home, &["notes", "tokens"]);
    assert_eq!(ran.code, 0);
    assert_eq!(
        ran.lines().len(),
        2,
        "one line per token: {:?}",
        ran.lines()
    );

    let composer: Vec<&str> = ran
        .lines()
        .into_iter()
        .filter(|line| line.trim_end().ends_with("composer"))
        .collect();
    assert_eq!(
        composer.len(),
        1,
        "D7: the listing shows the composer role explicitly, so a user can answer \"which token \
         is my search using\" without inspecting configuration: {:?}",
        ran.lines()
    );
    assert!(
        composer[0].starts_with("  work") && composer[0].contains("zaru"),
        "the composer's row carries its alias and its workspace: {:?}",
        composer[0]
    );
    assert!(
        ran.stdout.contains("2 tool(s)") && ran.stdout.contains("1 tool(s)"),
        "D7 wants a tool count per token: {}",
        ran.stdout
    );
    assert!(
        ran.stdout.contains("apex (no instance boundary)"),
        "D8 marks an apex token wherever it appears, and the listing is one of its three \
         places: {}",
        ran.stdout
    );
    assert!(
        ran.stdout.contains("100monkeys-ai.cortex.page"),
        "an instance-locked token shows its instance, so the marking distinguishes something: {}",
        ran.stdout
    );

    let printed = format!("{}{}", ran.stdout, ran.stderr);
    assert!(
        !printed.contains(&planted),
        "a bearer value reached the listing"
    );
    assert!(
        !printed.contains(&core),
        "the bearer value's ASCII core reached the listing in some escaped form, which the raw \
         assertion above cannot see"
    );
}

/// `notes tokens` on a machine with no store says so and creates nothing.
#[test]
fn listing_tokens_on_a_fresh_machine_says_so_and_creates_nothing() {
    let home = Home::new("notes-empty");

    let ran = zaru(&home, &["notes", "tokens"]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.stdout.starts_with("no tokens"),
        "an empty listing and a listing that failed look identical unless one says so: {}",
        ran.stdout
    );
    assert!(
        home.path().is_dir(),
        "the scratch home must survive, or this reports absence for everything"
    );
    assert!(
        !home.path().join(".zaru").exists(),
        "listing tokens created ~/.zaru, which is creating state in order to read state"
    );
}

/// A task is refused, and the two halves of what is missing are two classes.
///
/// **This pins the reading ruled on 2026-09-05 so that deciding it the other
/// way reddens.** An unresolved `model.default` is the user's — the remedy
/// names a key they can set, and setting it moves the refusal on. A resolved
/// one is not, and **both are now the user's**: with no model they have a key
/// to set, and with a model and no key they have a key to store. Until
/// 2026-09-05 the second was ADR-0016 D1's capability class at exit 4, because
/// what was missing then was the wiring; a composition wires it now, so the
/// only thing left between a configured model and a turn is a credential.
///
/// D1's capability class is still reachable and still does not fit -- a project
/// that declares validators gets it, in `turn_from_outside.rs` -- and D1's
/// missing row for "not built yet" is still an open question on that record.
#[test]
fn the_two_halves_of_a_missing_provider_are_different_classes() {
    let home = Home::new("task");

    let unconfigured = zaru(&home, &["fix", "the", "failing", "test"]);
    assert_eq!(
        unconfigured.code, 2,
        "with no model configured the refusal is the user's and the remedy is a key they can set"
    );
    assert!(
        unconfigured.stderr.contains("model.default")
            && unconfigured.stderr.contains("ZARU_MODEL_DEFAULT"),
        "the remedy names the key and the variable ADR-0014's own transform produces: {}",
        unconfigured.stderr
    );
    assert!(
        !unconfigured.stderr.contains("config set")
            && !unconfigured.stderr.contains("ZARU_ANTHROPIC_KEY"),
        "ADR-0016 D2's worked remedy names a command this harness does not have and a variable \
         the transform cannot produce, and neither is reused: {}",
        unconfigured.stderr
    );

    // **The second half changed class on 2026-09-05 and that is this check's
    // whole subject.** It asserted exit 4 and the sentence "nothing wires a
    // provider client to a loop", which was true from the day the `gemini`
    // client landed until the day a composition ran a turn. Something wires
    // one now, so what a machine with a model and no key is short of is a
    // **key** -- theirs, at exit 2, with a command they can run. A refusal
    // that still said "the wiring" would send a reader looking for the wrong
    // thing.
    let configured = zaru(
        &home,
        &["--model", "a-model-identifier", "do", "a", "thing"],
    );
    assert_eq!(
        configured.code, 2,
        "with a model configured and no key, what is missing is the user's and the remedy is a \
         command this binary runs"
    );
    assert!(
        configured.stderr.contains("providers keys add"),
        "the remedy must be the command that stores a key: {}",
        configured.stderr
    );
    assert!(
        !configured
            .stderr
            .contains("nothing wires a provider client to a loop"),
        "the refusal still claims nothing wires a client to a loop, which stopped being true when \
         the composition landed: {}",
        configured.stderr
    );
    assert!(
        configured.stderr.contains("gemini"),
        "the refusal must name the kind that does have a client, so a reader can tell which of \
         the five they are short of: {}",
        configured.stderr
    );
    // Nothing was written for a turn that never began.
    assert!(
        !home.path().join(".zaru/sessions").exists(),
        "a refusal reached before the session created one anyway"
    );
}

/// [ADR-0009] D6's `zaru init`, from the real artefact.
///
/// **The one thing this surface does that changes a file the user owns**, so it
/// is the one that most needs driving end to end rather than through the
/// crate's door. Three runs: the first writes, the second refuses at exit 2,
/// and what is on disk after both is what the first run wrote.
///
/// It then reads the file back through the binary itself — `zaru config explain
/// runtime.max_iterations` names `./zaru.toml` as the supplier — because a
/// template that writes and does not load is the failure this whole arc found
/// before writing any of it.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[test]
fn adr_0009_d6s_init_writes_once_refuses_twice_and_what_it_wrote_folds() {
    let home = Home::new("init");
    let manifest = home.project().join("zaru.toml");
    assert!(!manifest.exists(), "the scratch project has no manifest");

    let first = zaru(&home, &["init"]);
    assert_eq!(first.code, 0, "the first `zaru init` writes");
    assert!(
        first.stdout.contains("zaru.toml"),
        "it says which file it wrote: {}",
        first.stdout
    );
    let written = std::fs::read_to_string(&manifest).expect("the manifest is on disk");

    let second = zaru(&home, &["init"]);
    assert_eq!(
        second.code, 2,
        "a second `zaru init` is refused, and ADR-0016 D5 makes that user-correctable"
    );
    assert_eq!(
        std::fs::read_to_string(&manifest).expect("still on disk"),
        written,
        "ADR-0009 D6: the manifest is read, never written — a refused `init` changes nothing"
    );

    // The half that matters most: what it wrote is loadable by the thing that
    // wrote it.
    let explained = zaru(&home, &["config", "explain", "runtime.max_iterations"]);
    assert_eq!(
        explained.code, 0,
        "what `zaru init` wrote must fold, or it wrote a file this binary refuses"
    );
    let marked: Vec<&str> = explained
        .lines()
        .into_iter()
        .filter(|line| line.contains("← effective"))
        .collect();
    assert_eq!(marked.len(), 1, "one row is marked: {}", explained.stdout);
    assert!(
        marked[0].trim_start().starts_with("3 ") && marked[0].contains("zaru.toml"),
        "the value came from the manifest `init` wrote: {}",
        explained.stdout
    );
}

/// [ADR-0016] D5's `70`, from the real artefact, with no panic anywhere.
///
/// # Two records say this is not observable, and both are wrong about it
///
/// This record's Status tracking and [ADR-0015]'s both say "`1`, `3` and `70`
/// did not become observable and none was attempted", reasoning that there is
/// "no honest way to make the binary panic". The reasoning is right and the
/// conclusion does not follow: D5's `70` is D1's **defect** class, and a
/// *reported* defect reaches it without any panic at all.
///
/// One is reachable on a machine with a session whose transcript this harness
/// cannot read back. `ResumeFailure::Transcript` is one of the two variants
/// [ADR-0016]'s own Update lists as "unmapped, carried as a defect -- this
/// harness is the only writer of both files, so a complete line it cannot read
/// back is one it wrote wrongly".
///
/// # And the report names the session, which until 2026-09-05 it did not
///
/// `cli::run::resume` passed `SessionEvidence::NoSessionExists` on this path,
/// so the report said "there is no session and no transcript was written"
/// about a session directory that was right there and a transcript that had
/// been written. D3 asks for the session id and says the transcript is on
/// disk; the arm that makes claiming one a compile error was being handed the
/// wrong arm on the one path with a real session. Both halves are asserted
/// here, and the second is the one that would go quiet again.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn adr_0016_d5s_seventy_is_a_reported_defect_and_the_report_names_the_session() {
    let home = Home::new("defect");
    let id = "01HM2E5Y001440E1G50G1G4080";
    let session = home.path().join(".zaru/sessions").join(id);
    std::fs::create_dir_all(&session).expect("a scratch session directory");
    // A **complete** line -- it ends in a newline -- that is not a record. A
    // fragment would be the event in flight, which D2 permits and the reader
    // tolerates by design, so it would assert nothing.
    std::fs::write(session.join("transcript.jsonl"), "this is not a record\n")
        .expect("the transcript is written");

    let ran = zaru(&home, &["--resume", id]);
    assert_eq!(
        ran.code, 70,
        "a transcript this harness alone writes and cannot read back is ours, and D5 gives that 70"
    );
    assert!(
        ran.stderr.contains("this is a bug in Zaru"),
        "D3: a defect says it is a defect: {}",
        ran.stderr
    );
    assert!(
        ran.stderr.contains(id) && ran.stderr.contains("transcript.jsonl"),
        "D3's report carries the session id and says where the transcript is: {}",
        ran.stderr
    );
    assert!(
        !ran.stderr.contains("there is no session"),
        "the report claimed there is no session about a session that exists: {}",
        ran.stderr
    );
    // The defect boundary's own rule: the message the panic hook would have
    // printed is not in the report, because the report has no field for one.
    assert!(
        !ran.stderr.contains("this is not a record"),
        "the report quoted the file's contents, which D3's own shape has nowhere to put: {}",
        ran.stderr
    );
}

/// The awkward tail a planted value carries: a combining acute and an
/// astral-plane character, so a rendering that escaped rather than printed is
/// still caught by the value's ASCII core.
const AWKWARD_TAIL: &str = "-e\u{301}\u{1f701}";

/// The value a check plants and then looks for, shaped like a token and not
/// being one.
///
/// Deliberately awkward for the reason `zaru-cli`'s own fixtures are: it
/// carries a combining mark and an astral-plane character, so a rendering that
/// escaped rather than printed is still caught by its ASCII core.
fn planted_token() -> String {
    format!(
        "not_a_token_{}_{}{AWKWARD_TAIL}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos()
    )
}

/// The ASCII part of a planted value, which no escaping scheme alters.
fn ascii_core(value: &str) -> &str {
    value.strip_suffix(AWKWARD_TAIL).unwrap_or(value)
}

/// ADR-0007 D7's `add`, refusing before it reaches anything.
///
/// **The shape is checked before the network**, which is what makes this
/// runnable on a gate with none. A value whose prefix names no ADR-0161 kind is
/// refused by `Secret::notes`, and `notes tokens add` reaches
/// `credentials::tool_scope_at` only after that returns — so no socket is
/// opened, no instance is addressed, and the check needs neither.
#[test]
fn adr_0007_d7s_add_refuses_a_value_that_is_not_a_token_and_quotes_none_of_it() {
    let home = Home::new("notes-add-refused");
    let planted = planted_token();

    let ran = zaru_with_input(
        &home,
        &["notes", "tokens", "add", "work", "cortex.page"],
        &format!("{planted}\n"),
    );

    assert_eq!(
        ran.code, 2,
        "a value the user supplied and can supply again is ADR-0016 D5's user-correctable"
    );
    let rendered = format!("{}{}", ran.stdout, ran.stderr);
    assert!(
        !rendered.contains(&planted),
        "the refusal published the value verbatim: {rendered:?}"
    );
    assert!(
        !rendered.contains(ascii_core(&planted)),
        "the refusal published the value in an escaped form; its ASCII core {:?} is in \
         {rendered:?}",
        ascii_core(&planted)
    );
    assert!(
        rendered.contains("notes tokens add work cortex.page"),
        "the remedy does not name the command that would work: {rendered:?}"
    );
}

/// The sibling: refused earlier still, at the parser, with the pipe untouched.
///
/// It discriminates. Without it, a `notes tokens add` that refused every input
/// for any reason would pass the check above, and the two refusals here are
/// raised by different code at different times — one by the parser before
/// standard input is read at all, one by the secret's own shape check
/// afterwards.
#[test]
fn adr_0007_d7s_add_names_each_argument_it_was_not_given() {
    let home = Home::new("notes-add-arguments");

    let no_arguments = zaru(&home, &["notes", "tokens", "add"]);
    assert_eq!(no_arguments.code, 2);
    assert!(
        format!("{}{}", no_arguments.stdout, no_arguments.stderr)
            .contains("an alias and an instance host"),
        "the refusal does not say what was missing: {:?}",
        no_arguments.stderr
    );

    let no_host = zaru(&home, &["notes", "tokens", "add", "work"]);
    assert_eq!(no_host.code, 2);
    assert!(
        format!("{}{}", no_host.stdout, no_host.stderr).contains("an instance host"),
        "an alias with no host is a different refusal and must say so: {:?}",
        no_host.stderr
    );

    let too_many = zaru(
        &home,
        &["notes", "tokens", "add", "work", "cortex.page", "sideways"],
    );
    assert_eq!(too_many.code, 2);
    assert!(
        format!("{}{}", too_many.stdout, too_many.stderr).contains("sideways"),
        "the refusal does not name the word it would not take: {:?}",
        too_many.stderr
    );
}
