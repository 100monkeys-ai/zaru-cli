// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `zaru <task>` driven as a person drives it, one turn at a time.
//!
//! # What this file is for that no unit check can be
//!
//! Every module the composition uses is checked inside its own crate, and
//! every one of those captures carries the sentence "evidence about the
//! mechanism, which must not be quoted as evidence about the binary". This is
//! the other kind: it runs the artefact Cargo built, with a scratch `$HOME`
//! and a cleared environment, and reads its standard output, its standard
//! error, its process status and the files it left behind.
//!
//! [Verification lessons] §25 is why it exists at all: "a mechanism whose only
//! callers are in the test suite is a mechanism nobody has been shown to
//! reach", and a composition is nothing *but* reachability.
//!
//! # No key is spent and no packet leaves the machine
//!
//! Two of these checks reach a socket, both to `127.0.0.1` on a port nothing
//! is listening on, and what they assert is the connection being refused.
//! Every key here is a nonce — a uniqueness device rather than a secret — and
//! the sealing key is `ZARU_CREDENTIAL_KEY`, which is [ADR-0007] D3's own
//! answer for a machine with no keyring and the ordinary case in CI.
//!
//! **A real provider is reached by exactly one check in this repository**, and
//! it is not here: `provider_from_outside.rs` holds it, gated on a variable
//! that says a key exists that may be spent.
//!
//! # Four of these are security-corpus cases
//!
//! [Testing]: "Every escape found at a security boundary … joins a permanent
//! hostile-input corpus as its reproduction", and the corpus only grows. The
//! four are the key's absence from everything a turn produces, a call that
//! needed a confirmation nobody could give, a held secret in a tool result
//! reaching a model, and — since 2026-09-05, when the checkpoint stopped being
//! empty — a stored key spoken back in a task reaching [ADR-0010] D3's
//! `context.json`, which is what the **next process** assembles a prompt from.
//! Each has an **accepting sibling** beside it, because an absence assertion is
//! satisfied by a harness that does nothing at all.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The sealing key [ADR-0007] D3 reads where there is no keyring.
///
/// A literal rather than a generated value: it is the *key* the store seals
/// with, not a credential, and a check that regenerated it could not read back
/// what a previous run wrote.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
const SEALING_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A port on the loopback interface with nothing listening on it.
///
/// Port 1 is `tcpmux` and is reserved; nothing binds it. A connection to it is
/// refused by the kernel on the same machine, so this reaches a socket and
/// **no packet leaves the host** — which is what lets [ADR-0016] D5's `3` be
/// observable on a runner that has no key and no business having one.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
const CLOSED_LOOPBACK: &str = "http://127.0.0.1:1";

/// A scratch `$HOME` and working directory that remove themselves.
struct Home {
    path: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-turn-from-outside-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("project")).expect("a scratch home and project");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn project(&self) -> PathBuf {
        self.path.join("project")
    }

    /// The one session this home holds, or a refusal naming what it found.
    ///
    /// **Refuses rather than skipping**: a check that silently found no
    /// session would assert nothing about a turn and report it as a pass
    /// ([Verification lessons] §4).
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    fn one_session(&self) -> PathBuf {
        let sessions = self.path.join(".zaru/sessions");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&sessions)
            .unwrap_or_else(|error| {
                panic!(
                    "no sessions directory at {}: {error}. A turn that ran creates one, so this \
                     check has nothing to assert about",
                    sessions.display()
                )
            })
            .map(|entry| entry.expect("a readable directory entry").path())
            .collect();
        found.sort();
        assert_eq!(
            found.len(),
            1,
            "one invocation is one turn is one session, and {} were found: {found:?}",
            found.len()
        );
        found.remove(0)
    }

    /// Every session directory this home holds, which may be none.
    ///
    /// [`Self::one_session`]'s companion for the arm that asserts a turn
    /// **never began**: that one refuses when the directory is missing,
    /// which is right for a check about a session that ran and wrong for a
    /// check about one that did not.
    fn sessions(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.path.join(".zaru/sessions"))
            .map(|entries| {
                entries
                    .map(|entry| entry.expect("a readable directory entry").path())
                    .collect()
            })
            .unwrap_or_default()
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
    /// Everything the run put anywhere a person could read.
    fn everything(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

/// Run the built binary with a scratch home, a cleared environment, and
/// whatever `ZARU_*` the case needs.
fn zaru(home: &Home, variables: &[(&str, &str)], arguments: &[&str]) -> Ran {
    let mut command = Command::new(env!("CARGO_BIN_EXE_zaru"));
    command
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
        .current_dir(home.project())
        .stdin(Stdio::null());
    for (name, value) in variables {
        command.env(name, value);
    }
    let output: Output = command
        .output()
        .expect("failed to execute the built binary");

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

/// Put a nonce key in the sealed store the way a user puts a real one there.
///
/// Through `zaru providers keys add`, from standard input, which is the only
/// surface that writes one — rather than by writing the store's file, which
/// would be a check standing in for the product's own writer.
fn store_a_key(home: &Home, kind: &str, value: &str) {
    use std::io::Write as _;

    let mut child = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(["providers", "keys", "add", kind])
        .env_clear()
        .env("HOME", home.path())
        .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
        .current_dir(home.project())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute the built binary");
    child
        .stdin
        .as_mut()
        .expect("the child's standard input is a pipe")
        .write_all(format!("{value}\n").as_bytes())
        .expect("the key reaches the child");
    let output = child.wait_with_output().expect("the child exits");
    assert!(
        output.status.success(),
        "storing a `{kind}` key failed, so every check after it would be about a machine with no \
         key: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A nonce with a combining mark in it, and its ASCII core.
///
/// The mark is [Verification lessons] §50's instrument: a `{:?}` rendering
/// escapes it, so a value that has been published byte for byte is genuinely
/// absent from the output *as typed*. Every absence here is asserted twice,
/// against the value and against the core, and the core is what no escaping
/// scheme alters.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn nonce(seed: &str) -> (String, String) {
    let core = format!("nonce-{seed}-{}", std::process::id());
    (format!("{core}e\u{301}"), core)
}

/// Assert a value and its ASCII core are absent from everything a run
/// produced, and from every file it left behind.
fn absent_everywhere(home: &Home, ran: &Ran, value: &str, core: &str, what: &str) {
    let mut looked_at = 0_usize;
    let mut check = |where_: &str, text: &str| {
        looked_at += 1;
        assert!(
            !text.contains(value),
            "{what} reached {where_} by value: {text}"
        );
        assert!(
            !text.contains(core),
            "{what} reached {where_} by its ASCII core, which is what an escaping formatter \
             leaves intact: {text}"
        );
    };
    check("what the run printed", &ran.everything());

    // Every file under the scratch home, read with `std::fs` rather than
    // through any reader this workspace owns.
    let mut stack = vec![home.path().to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                check(&format!("the file {}", path.display()), &text);
            }
        }
    }
    assert!(
        looked_at > 1,
        "this walk read {looked_at} thing(s), which is too few to have asserted anything about \
         where {what} did not reach"
    );
    println!("   [absence] {what}: {looked_at} place(s) read, none carried it");
}

// --- The corpus -------------------------------------------------------------

/// A project cannot buy itself fewer prompts, and the user can set the mode.
///
/// **Security corpus, and the shape of its refusal is measured rather than
/// assumed.** ADR-0014 D6's *first* escalation is "raise the permission
/// mode", and until 2026-09-05 there was no key for it to name. There is now,
/// so this is the case a cloned repository would actually try.
///
/// **The refusal a user gets is ADR-0009's, not ADR-0014 D6's.** `tools.mode`
/// sits under `[tools]`, which ADR-0009 D1's manifest vocabulary does not
/// declare, so the manifest reader refuses the file one step before D6's
/// escalation ceiling sees a value. Nothing is weakened — the project still
/// cannot set it, and `tools::mode::field` declares it `Refused` besides —
/// but what the reader is told names a table rather than a privilege, so this
/// check asserts **that**, and asserts D6's sentence is *absent*, rather than
/// claiming a refusal the harness does not give. The shadowing itself is
/// pinned by `the_escalation_ceiling_is_shadowed_by_adr_0009s_manifest_vocabulary`
/// in `permission_from_outside.rs`, and D6's own arm by
/// `a_project_may_not_set_the_permission_mode_however_the_resolution_was_built`.
///
/// Its **accepting sibling** is the second half: the same value, in the
/// user's own file, is taken — read back out of `config explain` because that
/// is the only place a person can see it, and without it this check is
/// satisfied by a harness that refuses the key from every layer.
#[test]
fn corpus_a_project_may_not_set_the_permission_mode_and_the_user_may() {
    let home = Home::new("mode-project");

    std::fs::write(
        home.project().join("zaru.toml"),
        "[tools]\nmode = \"yolo\"\n",
    )
    .expect("a scratch project file");

    let refused = zaru(&home, &[], &["read", "the", "manifest"]);
    assert_eq!(
        refused.code, 2,
        "a project file the harness will not serve is the reader's to edit, which is exit 2"
    );
    assert!(
        refused.everything().contains("zaru.toml"),
        "the refusal must name the file the reader has to edit: {}",
        refused.everything()
    );
    assert!(
        !refused.everything().contains("yolo"),
        "a refusal that quoted the mode back would be teaching the reader the word that buys \
         fewer prompts: {}",
        refused.everything()
    );
    assert!(
        !refused
            .everything()
            .contains("more privilege than the user granted"),
        "ADR-0014 D6's own sentence reached the user, so the shadowing this check is written \
         against is over and it should now assert D6's refusal instead: {}",
        refused.everything()
    );

    // The accepting sibling: the same key, the same shape, in the user's own
    // file, seen where a person can see it.
    std::fs::remove_file(home.project().join("zaru.toml")).expect("the project file is removed");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
    std::fs::write(
        home.path().join(".zaru").join("config.toml"),
        "[tools]\nmode = \"allow\"\n",
    )
    .expect("a scratch user file");

    let explained = zaru(&home, &[], &["config", "explain", "tools.mode"]);
    assert_eq!(
        explained.code, 0,
        "the user's own layer is the one ADR-0011 D3 gives the mode to"
    );
    assert!(
        explained.stdout.contains("tools.mode = allow"),
        "the effective value must be the one the user wrote: {}",
        explained.stdout
    );
    assert!(
        explained.stdout.contains("\u{2190} effective"),
        "ADR-0014 D3 requires the effective layer be marked: {}",
        explained.stdout
    );
}

/// A `--mode` no record defines is refused naming every mode that exists.
///
/// **Corpus case for ADR-0014 D5's own argument** — "a typo that silently
/// does nothing is the worst outcome of any config system, because the user
/// sees no change and concludes the setting does not work" — applied to the
/// key with the most to lose from it.
///
/// **This is the check that decides where the mode is read.** With the read
/// at the `Executor`, as it was first written, `--mode fast` on a machine
/// with no model configured was discarded by the missing-model refusal above
/// it and the harness said nothing whatever about the word the user had just
/// typed. The mode is resolved beside the tier for that reason, and this
/// scratch home deliberately has **no model configured**, so a read that
/// moves back down reddens here.
///
/// Its **accepting sibling** is the third arm: a mode that *is* one of the
/// three gets past this refusal to the next one, so the check is not
/// satisfied by a harness that refuses every `--mode`.
#[test]
fn corpus_a_mode_no_record_defines_is_refused_naming_every_mode_that_does() {
    let home = Home::new("mode-flag");

    let refused = zaru(&home, &[], &["--mode", "fast", "write", "a", "haiku"]);
    assert_eq!(
        refused.code, 2,
        "a word the user just typed is the user's to correct, which is exit 2"
    );

    let everything = refused.everything();
    for named in ["ask", "allow", "yolo"] {
        assert!(
            everything.contains(named),
            "ADR-0011 D3 defines exactly three modes and the refusal leaves out {named:?}: \
             {everything}"
        );
    }
    assert!(
        everything.contains("fast"),
        "the refusal must quote back what the user wrote, or they cannot find it: {everything}"
    );

    // The accepting sibling: a real mode is not what stops this run.
    let taken = zaru(&home, &[], &["--mode", "allow", "write", "a", "haiku"]);
    assert!(
        !taken.everything().contains("names no permission mode"),
        "`allow` is one of D3's three and was refused as though it were not: {}",
        taken.everything()
    );

    // And the same value through layer 4, which the transform gives for free
    // the moment the key is declared -- so a mode reachable by a flag and not
    // by the environment would be a gap nobody chose.
    let through_the_environment = zaru(
        &home,
        &[("ZARU_TOOLS_MODE", "fast")],
        &["write", "a", "haiku"],
    );
    assert!(
        through_the_environment
            .everything()
            .contains("names no permission mode"),
        "ADR-0014 D1's transform makes ZARU_TOOLS_MODE this key's layer-4 spelling, and it did \
         not reach the same refusal: {}",
        through_the_environment.everything()
    );
}

/// A task with no key held is refused naming the alias and the command, and
/// never a key.
///
/// **Security corpus.** [ADR-0007] D3's whole argument is that a value the
/// harness holds must not travel where it is not needed, and a refusal is text
/// that gets pasted into a bug report.
///
/// Its **accepting sibling** is the second half: with a key stored, the same
/// invocation gets past the store and fails at the socket instead. Without it
/// this check is satisfied by a harness that refuses every task for any
/// reason.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[test]
fn corpus_a_task_with_no_key_is_refused_naming_the_alias_and_never_a_key() {
    let home = Home::new("no-key");
    let (value, core) = nonce("no-key");

    let refused = zaru(
        &home,
        &[],
        &["--model", "gemini-3.6-flash", "write", "a", "haiku"],
    );
    assert_eq!(
        refused.code, 2,
        "a machine with a model and no key is the user's half undone, and the remedy is a command"
    );
    assert!(
        refused.stderr.contains("providers keys add gemini"),
        "the remedy must name the command that stores one: {}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("the alias `default`")
            && refused.stderr.contains("gemini-3.6-flash"),
        "the refusal must name the alias and what it resolved to, so a reader knows which of the \
         five they are short of: {}",
        refused.stderr
    );
    // Nothing was created: a turn that never began is not a session.
    assert!(
        !home.path().join(".zaru/sessions").exists(),
        "a refusal before the session wrote a session directory"
    );

    // The accepting sibling. With a key, the same invocation gets past the
    // store and fails at the socket, which is a different code and a different
    // class -- so the refusal above is about the key rather than about tasks.
    store_a_key(&home, "gemini", &value);
    let reached = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "write", "a", "haiku"],
    );
    assert_eq!(
        reached.code, 3,
        "with a key held the turn reaches a socket, so the refusal above was about the key"
    );
    absent_everywhere(&home, &reached, &value, &core, "the stored provider key");
}

/// [ADR-0016] D5's `3`: a provider that could not be reached.
///
/// The endpoint is [ADR-0012] D5's own configuration key at a loopback port
/// nothing listens on, so this reaches a real socket, gets a real refusal from
/// the kernel, and sends nothing anywhere. That record's Status tracking has
/// said since 2026-09-05 that `3` "did not become observable and none was
/// attempted -- there is … no network to be unreachable". There is now.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn adr_0016_d5s_three_is_a_provider_that_could_not_be_reached() {
    let home = Home::new("unreachable");
    let (value, core) = nonce("unreachable");
    store_a_key(&home, "gemini", &value);

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );
    assert_eq!(ran.code, 3, "an unreachable provider is environmental");
    assert!(
        ran.stderr.contains("could not be reached"),
        "the refusal must say what happened: {}",
        ran.stderr
    );
    // ADR-0016 D2's other half: an environmental failure says whether waiting
    // helps, and this one says it does not, because no record states a retry
    // policy and inventing one would answer that silently.
    assert!(
        ran.stderr.contains("no retry policy"),
        "D4 requires a retry policy be stated, and its absence has to be stated too: {}",
        ran.stderr
    );
    absent_everywhere(&home, &ran, &value, &core, "the stored provider key");
}

/// [ADR-0010] D1's directory, its three files, and the first `meta.toml` a
/// product path ever wrote.
///
/// That record's clause 1 has been "whole for the mechanism and unreachable
/// for the caller" since 2026-09-05: "**the binary starts no session**, so no
/// `meta.toml` exists on any machine". It does now, and this reads it off the
/// filesystem rather than through the reader that wrote it.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn adr_0010_d1s_session_holds_three_files_and_meta_toml_records_six_things() {
    use std::os::unix::fs::PermissionsExt as _;

    let home = Home::new("session");
    let (value, _core) = nonce("session");
    store_a_key(&home, "gemini", &value);

    zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );

    let session = home.one_session();
    for (name, mode) in [
        ("meta.toml", 0o600),
        ("transcript.jsonl", 0o600),
        ("context.json", 0o600),
    ] {
        let path = session.join(name);
        assert!(
            path.is_file(),
            "D1's {name} is not there: {}",
            path.display()
        );
        let found = std::fs::metadata(&path)
            .expect("the file is readable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            found, mode,
            "D5 says filesystem permissions are the only protection a transcript has, and {name} \
             carries {found:o}"
        );
    }

    // The six things D1's accepted Update names, read as bytes.
    let meta = std::fs::read_to_string(session.join("meta.toml")).expect("meta.toml reads");
    for field in ["tier", "tier_from", "provider", "started"] {
        assert!(
            meta.contains(&format!("{field} =")),
            "meta.toml does not record {field}: {meta}"
        );
    }
    assert!(
        meta.contains("tier = \"bare\"") && meta.contains("tier_from = \"built-in\""),
        "the tier and the layer that supplied it are what D1's Update added: {meta}"
    );
    // `workspace` and `ended` are absent rather than empty, which is the
    // distinction `Meta`'s own documentation draws: a session that had no
    // workspace is not a session whose workspace was "".
    assert!(
        !meta.contains("workspace ="),
        "a session with no Nuclear Notes workspace records none: {meta}"
    );
    assert!(
        !meta.contains("ended ="),
        "D1 makes an absent `ended` mean still running: {meta}"
    );
}

/// A stored key spoken back in a task does not reach the checkpoint.
///
/// # Why this case exists only now
///
/// `context.json` held `{"exchanges":[]}` until 2026-09-05, so every absence
/// assertion over it was true of a file with nothing in it. It carries
/// [ADR-0013] D1's layer 6 now — the task, the tool lines and the answer — and
/// layer 6 is what the **next** turn, in the **next process**, is assembled
/// from. That makes the checkpoint a path from captured bytes into a model
/// prompt that outlives the process, which is [ADR-0008] clause 6's subject.
///
/// **The seam is `compose::boundary::exchange_of_turn`** and this is the case
/// that discriminates it: a task naming the stored key would put the key into
/// layer 6 verbatim, and from there into the prompt of every later turn of the
/// session, and onto disk in a file [ADR-0010] D5 says the user can read with
/// `cat`.
///
/// **The transcript is deliberately not asserted clean.** D2's Negative
/// section says it "contain\[s\] whatever the session contained, including
/// secrets that appeared in command output. Filesystem permissions are the
/// only protection" — so the raw file is the rule rather than a leak, and the
/// checkpoint is the thing that must be redacted because it is what a model
/// is shown.
///
/// The accepting sibling is a control value that is **not** a stored secret:
/// it must survive into the checkpoint, or this walk is one that finds nothing
/// anywhere and the absence above means nothing.
///
/// Asserted by value and by ASCII core (library verification-lessons §50).
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[test]
fn corpus_a_stored_key_spoken_in_a_task_does_not_reach_the_checkpoint() {
    let home = Home::new("checkpoint-redaction");
    let (value, core) = nonce("checkpoint-redaction");
    store_a_key(&home, "gemini", &value);

    // The control travels the same route and is not a secret, so whatever
    // reaches the checkpoint at all must carry it.
    let control = format!("control-{}", std::process::id());

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &[
            "--model",
            "gemini-3.6-flash",
            &format!("echo {value} and {control}"),
        ],
    );
    assert_eq!(ran.code, 3, "the staging is a turn that ran");

    let checkpoint = std::fs::read_to_string(home.one_session().join("context.json"))
        .expect("context.json reads");

    // The sibling first, so a checkpoint that held nothing could not satisfy
    // the two absences below by being empty.
    assert!(
        checkpoint.contains(&control),
        "the task did not reach layer 6 at all, so the absences below are about an empty file: \
         {checkpoint}"
    );
    for (what, needle) in [
        ("by value", value.as_str()),
        ("by its ASCII core", core.as_str()),
    ] {
        assert!(
            !checkpoint.contains(needle),
            "the stored provider key reached ADR-0010 D3's checkpoint {what}, and layer 6 is \
             what every later turn of this session is assembled from: {checkpoint}"
        );
    }
    absent_everywhere(&home, &ran, &value, &core, "the stored provider key");
}

/// [ADR-0010] D3: "`context.json` … **is overwritten each turn.**"
///
/// # What was there before, and why it looked like a session with no history
///
/// The checkpoint was written **once**, before turn 1, from a layer 6 that was
/// empty because this path recorded no exchange at all — so `context.json`
/// stayed `{"exchanges":[]}` however much the session went on to say, and a
/// later `--resume` restored a session that had never spoken. Both documents
/// parse and both look like a session, which is why nothing else on this path
/// would have shown it.
///
/// # What discriminates, and what the sibling is
///
/// This path mints a session per invocation, so the assertion is that one
/// invocation's checkpoint holds **that invocation's** turn: one exchange,
/// carrying the words the task was. A checkpoint written before
/// `SessionContext::record` holds zero, and so does one written only at
/// session start — the two mutants this catches — and both leave a document
/// that parses.
///
/// The accepting sibling is a run that never begins a turn at all. It asserts
/// that no session is created, which is [ADR-0010]'s own inode consequence,
/// so this check cannot pass against a harness that wrote an exchange into
/// every checkpoint it ever made.
///
/// Read with `std::fs` and `serde_json` rather than through
/// `zaru_cli::session::Checkpoint`, so neither arm of the comparison travels
/// through the writer under test.
///
/// The mutant: writing the checkpoint before `SessionContext::record` instead
/// of after, or not writing it per turn at all.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn adr_0010_d3s_checkpoint_is_overwritten_each_turn_and_holds_that_turn() {
    let home = Home::new("checkpoint-per-turn");
    let (value, _core) = nonce("checkpoint-per-turn");
    store_a_key(&home, "gemini", &value);

    // A turn that ran: the provider is unreachable, so the turn refuses at
    // ADR-0016 D5's `3` — but it *began*, the loop emitted its events, and
    // ADR-0013 D1's layer 6 is what the turn was about either way.
    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "remember the word saffron"],
    );
    assert_eq!(
        ran.code, 3,
        "the staging is a turn that ran and could not reach a model"
    );

    let session = home.one_session();
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(session.join("context.json")).expect("context.json"))
            .expect("the checkpoint is a JSON document");
    let exchanges = document
        .get("exchanges")
        .and_then(serde_json::Value::as_array)
        .expect("D3's checkpoint holds ADR-0013's layer 6 under `exchanges`");
    assert_eq!(
        exchanges.len(),
        1,
        "a session that has had one turn has one exchange in its checkpoint, and this holds          {} — which is what a checkpoint written before the turn's own record, or written          only at session start, leaves behind",
        exchanges.len(),
    );
    let held = serde_json::to_string(exchanges).expect("the exchange renders");
    assert!(
        held.contains("remember the word saffron"),
        "the checkpoint holds some other turn's layer 6: {held}"
    );

    // The accepting sibling: a session whose turn never began still has D1's
    // third file, and it holds no exchange. Without this arm the assertion
    // above would pass against a writer that put an exchange in every
    // checkpoint it ever wrote.
    let untried = Home::new("checkpoint-no-turn");
    let ran = zaru(&untried, &[], &["remember the word saffron"]);
    assert_eq!(ran.code, 2, "a task with no key is refused before the turn");
    assert!(
        untried.sessions().is_empty(),
        "a turn that never began is not a session, which is ADR-0010's own inode consequence",
    );
}

/// [ADR-0008] clause 3's one emission reaching the transcript writer.
///
/// The turn's events are on disk as `turn_loop` records, in the order the loop
/// emitted them, read back as raw lines rather than through the reader that
/// wrote them.
///
/// **What this does not claim.** Clause 3 asks for *both* consumers from one
/// emission, and the second is the shell's. This is the transcript half.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[test]
fn the_turns_events_reach_the_transcript_as_they_occur() {
    let home = Home::new("transcript");
    let (value, core) = nonce("transcript");
    store_a_key(&home, "gemini", &value);

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );

    let session = home.one_session();
    let transcript =
        std::fs::read_to_string(session.join("transcript.jsonl")).expect("the transcript reads");
    let lines: Vec<&str> = transcript.lines().collect();
    assert!(
        !lines.is_empty(),
        "a turn that started emitted nothing, so the sink is not on the loop's slice"
    );
    // The turn began, and the record says which turn of the session and what
    // its ceiling was -- the two fields a consumer renders "1 of 8" from.
    //
    // **The turn loop's first record rather than the file's**, since
    // 2026-09-05: ADR-0011 D2's notice is stated at session *start*, before
    // the loop begins, and since it became ADR-0010 D2's sixth producer it is
    // on disk ahead of the turn it precedes. The subject here is clause 3's
    // emission order within the loop's own stream, and reading the file's
    // first line was a proxy for it that a second producer made false.
    let first_of_the_loop = lines
        .iter()
        .find(|line| line.starts_with("{\"turn_loop\":"))
        .expect("the turn started, so its stream is on disk");
    assert!(
        first_of_the_loop.contains("\"turn_started\"") && first_of_the_loop.contains("\"of\":8"),
        "the turn loop's first record is not the turn starting at this binary's ceiling:          {first_of_the_loop}",
    );
    // Every line is one JSON object naming its producer, which is what makes
    // the file readable by anything rather than only by this harness.
    for line in &lines {
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "a transcript line is one JSON object per ADR-0010 D2: {line}"
        );
    }
    absent_everywhere(&home, &ran, &value, &core, "the stored provider key");
}

/// A project that declares validators takes [ADR-0009] D4's other branch.
///
/// **This check asserted the opposite until 2026-09-05**, when the iteration
/// loop was wired: it read `a_project_that_declares_validators_is_refused_and_one_without_runs`
/// and asserted exit `4` with a refusal naming "iteration loop". That refusal
/// is deleted, so the check is re-transcribed against the behaviour rather
/// than left to fail — and it is renamed with it, because a check whose name
/// states the old rule is a second statement of it that nothing keeps true.
///
/// What is asserted here is the **branch**, from the outside: a project with
/// validators and one without take different paths through the same binary
/// against the same closed socket, and the difference is visible in where each
/// one fails. The loop itself is
/// `tests/iteration_from_outside.rs`, which has a model that answers.
///
/// - **With validators**, the first thing that reaches the socket is the
///   *generator*, so the failure is the inner loop's — and its class is the
///   inner port's, read off the typed error the composition kept.
/// - **Without validators**, the tool-call loop's model port reaches it
///   instead. Both are ADR-0016 D5's `3`, and both create a session, which is
///   the half that changed: a validator-declaring project used to be refused
///   before one existed.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn a_project_that_declares_validators_takes_adr_0009_d4s_other_branch() {
    let iterating_home = Home::new("validators");
    let (value, _core) = nonce("validators");
    store_a_key(&iterating_home, "gemini", &value);

    std::fs::write(
        iterating_home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n\n[[validator]]\nname = \"build\"\nrun = \"true\"\nexpect = \
         \"exit-zero\"\n",
    )
    .expect("the manifest is written");

    let iterated = zaru(
        &iterating_home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        iterated.code,
        3,
        "a project that declares validators runs the iteration loop, whose generator then fails \
         at the socket: {}",
        iterated.everything()
    );
    assert!(
        !iterated.stderr.contains("iteration loop to run"),
        "the refusal that said this build has no iteration loop is deleted, and nothing should \
         still be saying it: {}",
        iterated.stderr
    );
    assert!(
        iterating_home.path().join(".zaru/sessions").exists(),
        "a project that declares validators is served now, and a served project gets a session"
    );

    // The accepting sibling: the same manifest without validators takes the
    // other branch, so the difference is the validators rather than the file.
    let plain_home = Home::new("validators-none");
    let (value, _core) = nonce("validators-none");
    store_a_key(&plain_home, "gemini", &value);
    std::fs::write(
        plain_home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n",
    )
    .expect("the manifest is written");
    let ran = zaru(
        &plain_home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        ran.code, 3,
        "a project with a manifest and no validators runs a turn, which then fails at the socket"
    );
    assert!(
        plain_home.path().join(".zaru/sessions").exists(),
        "the turn that ran created no session"
    );
    // The branch is visible in the transcript, which is the reading that
    // separates the two paths: both exit 3 at the same closed socket, and only
    // one of them started an iteration before it got there.
    let iterating = transcript_of(&iterating_home);
    assert!(
        iterating.contains("\"loop\":{\"iteration_started\""),
        "a project that declares validators must have started an iteration before it reached the \
         socket: {iterating}"
    );
    let plain = transcript_of(&plain_home);
    assert!(
        !plain.contains("\"loop\":"),
        "a project with no validators runs the tool-call loop only, and this one emitted the \
         iteration loop's own stream: {plain}"
    );
    assert!(
        plain.contains("\"turn_loop\":{\"turn_started\""),
        "the turn-only path must still have started a turn: {plain}"
    );
}

/// [ADR-0010] D2's seventh producer, on the file, ahead of the turn it opens.
///
/// D5 says "The user can read every byte the harness stores about them with
/// `cat`", and until 2026-09-06 `cat transcript.jsonl` showed a person loop
/// bookkeeping and not one word of what they had asked. This is the
/// out-of-session half of that, read with `std::fs` from the binary's own
/// output rather than through any reader this workspace owns.
///
/// # Three properties, and the third is what an interruption looks like
///
/// **The words are the task's.** Not a count, not a hash: the line the person
/// typed is on the file.
///
/// **It precedes `turn_started`.** The record is written before
/// `tool_call::run`, so `cat` reads in the order the turn happened. Asserted
/// as a byte offset rather than as presence, because both records present in
/// either order is a file a reader cannot follow.
///
/// **And this turn writes no `zaru` half**, because it never reached an
/// answer — the provider endpoint is a closed port. That absence is not a
/// gap: it is exactly the shape an *interrupted* turn leaves, which is how
/// ADR-0010 D4's interruption is read from a `Phase::Started` with nothing
/// closing it, and the turn stays distinguishable from an interrupted one
/// because it has a `turn_ended` record.
///
/// **What this check does not hold** is that an answering turn writes the
/// other half. `run_one` needs a real provider — `Prepared` holds a
/// `GeminiClient` and no stub substitutes for it — so that half is held by
/// `compose::tests::adr_0010_d2s_conversation_records_the_answer_and_not_the_turns_output`
/// over every outcome, by
/// `redaction::tests::adr_0010_d2s_conversation_records_are_built_through_the_port`
/// over the constructor, and by this arc's artefact over two real turns. Said
/// rather than implied.
///
/// The mutant: writing the `user` half after `tool_call::run` rather than
/// before it, which leaves both records on the file and reverses them.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn adr_0010_d2s_conversation_puts_the_task_on_the_transcript_before_the_turn() {
    let home = Home::new("conversation-task");
    let (value, _core) = nonce("conversation-task");
    store_a_key(&home, "gemini", &value);

    let task = "remember the word saffron";
    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", task],
    );
    assert_eq!(
        ran.code, 3,
        "the staging is a turn that ran and could not reach a model"
    );

    let transcript = transcript_of(&home);
    let spoken = transcript
        .find(r#"{"conversation":{"n":1,"voice":"user""#)
        .unwrap_or_else(|| {
            panic!(
                "ADR-0010 D2's seventh producer wrote nothing, so `cat` still shows this reader \
                 none of what they asked:\n{transcript}"
            )
        });
    assert!(
        transcript[spoken..].starts_with(&format!(
            r#"{{"conversation":{{"n":1,"voice":"user","text":"{task}"}}}}"#
        )),
        "the record does not carry the task in the words it was typed in, and it carries them \
         bare: the `user:` a reader sees is `vocabulary::spoken`'s, so that the record holds \
         the person's words and `voice` holds who said them:\n{transcript}"
    );

    let started = transcript
        .find(r#"{"turn_loop":{"turn_started""#)
        .expect("a turn that ran started");
    assert!(
        spoken < started,
        "the task is on the file after the turn it opened, so `cat` reads the work before the \
         question that caused it:\n{transcript}"
    );

    assert!(
        !transcript.contains(r#""voice":"zaru""#),
        "a turn that never reached an answer recorded one, which is the harness claiming it \
         spoke -- and it is the absence ADR-0010 D4's interruption is read from:\n{transcript}"
    );
}

/// A stored provider key spoken in a task does not reach the **transcript**
/// either, and this is the check that forced the record to be redacted at all.
///
/// # Why this is not in tension with an unredacted transcript
///
/// [ADR-0010]'s Negative section says the transcript "contains whatever the
/// session contained, **including secrets that appeared in command output**",
/// and every other record on that file is verbatim. [ADR-0008] clause 6's
/// port is a different obligation — it is over values the harness itself
/// **holds** — and `absent_everywhere` has walked every file under the scratch
/// home since before this producer existed. So a raw conversation record would
/// have been the first path in this harness that writes a value it holds into
/// a file, and would have reddened a standing check rather than raising a
/// question. The reading is on both records.
///
/// # What discriminates
///
/// The **control** travels in the same task, so a record that carried nothing
/// could not satisfy the absence by being empty; and the record must be found
/// at all, so a harness that stopped writing the producer could not satisfy it
/// by writing no file. The mutant is `compose::boundary::utterance` building
/// its text from the argument rather than through `Redacted::by`.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn corpus_a_stored_key_spoken_in_a_task_does_not_reach_the_transcript_either() {
    let home = Home::new("conversation-redaction");
    let (value, core) = nonce("conversation-redaction");
    store_a_key(&home, "gemini", &value);

    let control = format!("control-{}", std::process::id());
    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &[
            "--model",
            "gemini-3.6-flash",
            &format!("echo {value} and {control}"),
        ],
    );
    assert_eq!(ran.code, 3, "the staging is a turn that ran");

    let transcript = transcript_of(&home);
    assert!(
        transcript.contains(r#""voice":"user""#),
        "no conversation record was written, so the absences below are about a file that holds \
         nothing rather than about redaction:\n{transcript}"
    );
    assert!(
        transcript.contains(&control),
        "the task's own words did not reach the record, so the absences below are satisfied by \
         an empty record:\n{transcript}"
    );
    for (what, needle) in [
        ("by value", value.as_str()),
        ("by its ASCII core", core.as_str()),
    ] {
        assert!(
            !transcript.contains(needle),
            "the stored provider key reached ADR-0010 D2's transcript {what}:\n{transcript}"
        );
    }
    absent_everywhere(&home, &ran, &value, &core, "the stored provider key");
}

/// The one session's transcript, as bytes.
fn transcript_of(home: &Home) -> String {
    std::fs::read_to_string(home.one_session().join("transcript.jsonl"))
        .expect("a turn that ran wrote a transcript")
}

/// [ADR-0011] D2's not-a-sandbox line, once, at the tier where it is true.
///
/// That record's clause 2 asks for it "once per session", and until a session
/// existed there was nothing for it to be once per. It is on standard output
/// rather than standard error, because it is not a failure.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn adr_0011_d2s_not_a_sandbox_line_is_stated_once_at_bare_tier() {
    let home = Home::new("notice");
    let (value, _core) = nonce("notice");
    store_a_key(&home, "gemini", &value);

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );
    let occurrences = ran.stdout.matches("not a sandbox").count();
    assert_eq!(
        occurrences, 1,
        "D2 says once at session start, and it was said {occurrences} time(s): {}",
        ran.stdout
    );
    assert!(
        !ran.stderr.contains("not a sandbox"),
        "the line is not a failure and must not be on the failure stream: {}",
        ran.stderr
    );
    assert!(
        ran.stdout.contains("--runtime contained"),
        "the sentence must name what a reader can do about it: {}",
        ran.stdout
    );
}

/// [ADR-0009] D4's missing-manifest line is **not** stated at session start.
///
/// # What this can hold, and what only a key can
///
/// D4's line is [ADR-0002] D8's event-anchored kind, settled under directive
/// 20: appended to the end of the **triggering turn**, not stated at session
/// start, because "a manifest is absent before the user does anything" and a
/// line at session start "would still fire had the user done nothing", which
/// is D8's own test for a timer wearing a costume.
///
/// A turn that never completes therefore owes nothing, and that is the half
/// this check holds — over a turn that genuinely starts, creates its session
/// and fails at its provider. **The positive half needs a model that answers**,
/// so it is observable only behind `ZARU_GEMINI_EXCHANGE` and is quoted from
/// the real-artefact run rather than asserted here.
///
/// That distinction was found by running: a first version of this check
/// asserted the line absent in both arms and the mutation "the line is owed
/// even when a manifest is present" **survived**, because neither arm ever
/// reached the code that states it. The check now asserts the placement rule
/// instead, which is a property this staging can actually separate.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[test]
fn adr_0009_d4s_line_is_not_stated_at_session_start() {
    let home = Home::new("recommendation");
    let (value, _core) = nonce("recommendation");
    store_a_key(&home, "gemini", &value);

    // No manifest, so the line is owed by whichever turn completes -- and this
    // turn does not, because its provider is unreachable. What it must not do
    // is say it anyway.
    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );
    // The staging is asserted rather than assumed: a run that never started a
    // turn could not have stated the line either, and would satisfy this
    // vacuously.
    assert!(
        home.path().join(".zaru/sessions").exists(),
        "no turn started, so this check could not have seen the line whatever the code did"
    );
    assert!(
        ran.stdout.contains("not a sandbox"),
        "the session did start -- its own line is here -- so the absence below is about D4's line"
    );
    assert!(
        !ran.everything().contains("no validators are declared"),
        "D8 anchors the line to the end of a turn that happened, and this turn did not end: {}",
        ran.everything()
    );
}

/// A `zaru.toml` raising the iteration ceiling is refused; lowering it wins.
///
/// [ADR-0014] D6's `LowerOnly` policy on `runtime.max_iterations`, met by a
/// real project file through the real binary. ADR-0001's Status tracking says
/// "**nothing consumes it**" and that "the arc that wires a provider client
/// into the loop is the one that connects the declared key to that number".
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn a_project_may_lower_the_iteration_ceiling_and_may_not_raise_it() {
    let home = Home::new("ceiling");

    // The user's own layer 2, which is what a project may lower. **Not the
    // environment**: layer 4 is *above* layer 3, so a project "lowering" a
    // value the environment set is decided by precedence rather than by D6,
    // and a check staged that way passes against an implementation with no
    // `LowerOnly` policy at all. Found by running it the wrong way round
    // first, which reported the environment's 5 as effective over the
    // project's 3 -- correctly, and about nothing this check is named for.
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
    std::fs::write(
        home.path().join(".zaru/config.toml"),
        "[runtime]\nmax_iterations = 5\n",
    )
    .expect("the user's configuration is written");

    std::fs::write(
        home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n\n[runtime]\nmax_iterations = 3\n",
    )
    .expect("the manifest is written");
    let lowered = zaru(&home, &[], &["config", "explain", "runtime.max_iterations"]);
    assert_eq!(
        lowered.code, 0,
        "D6 permits a project lowering its own ceiling in as many words"
    );
    assert!(
        lowered.stdout.contains("runtime.max_iterations = 3"),
        "the project's lower ceiling is the effective one: {}",
        lowered.stdout
    );

    std::fs::write(
        home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n\n[runtime]\nmax_iterations = 8\n",
    )
    .expect("the manifest is rewritten");
    let raised = zaru(&home, &[], &["config", "explain", "runtime.max_iterations"]);
    assert_eq!(raised.code, 2, "D6 forbids a project raising one");
    assert!(
        raised.stderr.contains('8') && raised.stderr.contains('5'),
        "the refusal names both numbers so a reader can see which the user's layer had: {}",
        raised.stderr
    );
}

/// [ADR-0011] D2's line is a record in the transcript, so a later process
/// knows it was said.
///
/// D2's "once at session start" and [ADR-0002] D8's "at most once ever" were
/// once-per-process until 2026-09-05, because the carriers are rebuilt when a
/// process opens and nothing on disk said either had been said. What spans a
/// process is [ADR-0010] D2's own record stream: `Record::Said` is its sixth
/// producer.
///
/// **The mutant this catches is the record not being written at all**, which
/// leaves every unit check green — they drive the two rules over a witness a
/// check staged — and leaves the artefact stating the line on every process,
/// which is exactly what `session-restore` measured on `8553a6e`.
///
/// The turn here fails at a closed socket, which is the point: the notice is
/// owed by a session that started, and the record has to be on disk by then.
/// The text on the record must be the text the reader was shown, so a
/// re-rendering on `--resume` reproduces it, which is D2's replayability claim.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn the_not_a_sandbox_line_is_recorded_in_the_transcript_that_said_it() {
    let home = Home::new("said-notice");
    let (value, _core) = nonce("said-notice");
    store_a_key(&home, "gemini", &value);

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );
    // The staging is asserted: a run that never said the line could not have
    // recorded it either, and would satisfy an absence vacuously.
    let shown = ran
        .stdout
        .lines()
        .find(|line| line.contains("not a sandbox"))
        .expect("the session started, so its own line is on standard output");

    let transcript = transcript_of(&home);
    let records: Vec<&str> = transcript
        .lines()
        .filter(|line| line.starts_with("{\"said\":"))
        .collect();
    assert_eq!(
        records.len(),
        1,
        "a session that said D2's line once must hold one record of having said it, so a later \
         process of the same session does not say it again: {transcript}",
    );
    assert!(
        records[0].contains("\"line\":\"notice\""),
        "the record has to name which of the two once-ever lines it was, because the two are \
         decided by two rules: {}",
        records[0],
    );
    let stored: serde_json::Value =
        serde_json::from_str(records[0]).expect("the record is one JSON object");
    assert_eq!(
        stored["said"]["text"].as_str(),
        Some(shown),
        "the record must carry the sentence the reader was shown; a transcript that held a \
         different one could not reproduce what the user saw",
    );
    // ADR-0002 D8's line is not owed by a turn that never ended, so this
    // session must hold no record of it either.
    assert!(
        !transcript.contains("\"line\":\"recommendation\""),
        "this turn failed at its provider and never reached the end of a turn, so D8's \
         event-anchored line was neither shown nor recorded: {transcript}",
    );
}
