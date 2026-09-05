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
    use zaru_cli::session::{Millis, Record, SessionId, SessionStore, Transcript};

    let store = SessionStore::open(home.path().join(".zaru")).expect("a scratch session store");
    // The minting time is named rather than read off the machine's clock, so
    // the order `--continue` picks from is this check's rather than the
    // scheduler's.
    let id = SessionId::from_parts(Millis::new(minted_at), [1, 2, 3, 4, 5, 6, 7, 8, 9, 0])
        .expect("a well-formed ULID");
    let session = store.start(id.clone()).expect("a session directory");

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

/// ADR-0010 D4's resume restores, prints the transcript, and does not continue.
///
/// The lines printed are the transcript's **own bytes**, read back off the file
/// by this check rather than through the binary, so the two sides of the
/// comparison do not travel through one code path.
#[test]
fn adr_0010_d4s_resume_prints_the_transcripts_own_bytes_and_then_refuses() {
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
        ran.code, 2,
        "a resume restores and then refuses to continue, because continuing needs a provider"
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
        ran.stderr.contains("no model is configured"),
        "and then says what it cannot do next: {}",
        ran.stderr
    );

    let continued = zaru(&home, &["--continue"]);
    assert_eq!(continued.code, 2);
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
}

/// `--continue` takes the most recent session and not merely the last listed.
///
/// Staged out of order on purpose: the older session is created *second*, so a
/// resume that took whatever `read_dir` happened to yield, or the one it
/// created last, answers differently from one that reads the ULID's own order.
#[test]
fn continue_takes_the_most_recent_session_by_the_ulids_own_order() {
    let home = Home::new("continue-order");
    let newer = stage_a_session(&home, 1_700_000_009_000);
    let older = stage_a_session(&home, 1_700_000_003_000);
    assert!(older < newer, "the ULIDs must sort by their minting time");

    let ran = zaru(&home, &["--continue"]);
    assert!(
        ran.stdout.contains(newer.as_str()),
        "`--continue` must take the most recent session: {}",
        ran.stdout
    );
    assert!(
        !ran.stdout.contains(older.as_str()),
        "and not the one created last: {}",
        ran.stdout
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
/// one is not: the user has done their half and this build carries no provider
/// client, which is presented as ADR-0016 D1's capability class at exit 4. That
/// class carries a `Tier` and no tier in this build offers a provider, so the
/// line names `bare` and is true of the design rather than of this binary. D1
/// has no row for "not built yet" and that is raised as an open question.
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

    let configured = zaru(
        &home,
        &["--model", "a-model-identifier", "do", "a", "thing"],
    );
    assert_eq!(
        configured.code, 4,
        "with a model configured what is missing is ours, and D5's capability code is what this \
         reading claims"
    );
    // The statement says what is missing rather than what the reader should
    // change -- and since 2026-09-05 what is missing is the **wiring**, not
    // the client. A `gemini` client exists; nothing connects one to a loop.
    // The sentence that stood here until then said "no provider client", and
    // it is corrected rather than loosened: a refusal that overstates what is
    // absent sends a reader looking for the wrong thing.
    assert!(
        configured
            .stderr
            .contains("nothing wires a provider client to a loop"),
        "the statement must say what is missing rather than what the reader should change: {}",
        configured.stderr
    );
    assert!(
        !configured.stderr.contains("no provider client"),
        "the refusal still claims this workspace has no provider client, which stopped being \
         true when `providers::gemini` landed: {}",
        configured.stderr
    );
    assert!(
        configured.stderr.contains("gemini"),
        "the refusal must name the kind that does have a client, so a reader can tell which of \
         the five they are short of: {}",
        configured.stderr
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
