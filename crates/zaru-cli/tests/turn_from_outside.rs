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
use std::process::{Output, Stdio};

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
    let mut command = owned::command(env!("CARGO_BIN_EXE_zaru"));
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

    let mut child = owned::command(env!("CARGO_BIN_EXE_zaru"))
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
        .stdin()
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

/// ADR-0007 D7's `describe` and `rm` print no part of what they act on.
///
/// # Why these four and not the store's own accessors
///
/// D3's rule is that a bearer value "appears in no prompt, no transcript, no
/// log, and no tool result", and the two surfaces this arc added are the first
/// that **write** the store from a person's words. `describe` echoes text back
/// and `rm` reports on a credential it has just destroyed, so both are places
/// a value could reach a terminal — and the `rm` outcome is composed *after*
/// the record is gone, which is exactly when a careless implementation reaches
/// for the record it still has in hand.
///
/// Every assertion goes through [`absent_everywhere`], which reads what the
/// run printed **and** every file under the scratch home with `std::fs`,
/// rather than asking the store whether it still holds anything.
///
/// # The control, so no absence here is vacuous
///
/// Each run is asserted to name the alias it acted on. A store that had
/// written nothing, or a command that printed nothing at all, would satisfy
/// every absence below and fail this — it is the same guard
/// `the_file_carries_ciphertext_and_a_reader_that_is_not_the_store_opens_it`
/// puts in front of its own absences.
#[test]
fn adr_0007_d7s_describe_and_rm_print_no_part_of_the_value_they_hold() {
    let home = Home::new("d7-surfaces-hold-nothing");
    let (planted, core) = nonce("d7-surface");
    store_a_key(&home, "gemini", &planted);

    // The listing, which names the credential this run is about.
    let listed = zaru(&home, &[], &["providers", "keys"]);
    assert!(
        listed.everything().contains("provider.gemini"),
        "the listing does not name the key that was just stored, so every absence asserted below \
         would pass over a store that had written nothing: {}",
        listed.everything()
    );
    absent_everywhere(&home, &listed, &planted, &core, "the stored key");

    // The two Nuclear Notes verbs, refusing a credential of the other family.
    // A refusal is composed from the alias that was asked for, and the alias
    // is the one thing here that is allowed to travel.
    for arguments in [
        vec![
            "notes",
            "tokens",
            "describe",
            "provider.gemini",
            "a",
            "description",
        ],
        vec!["notes", "tokens", "rm", "provider.gemini"],
    ] {
        let ran = zaru(&home, &[], &arguments);
        assert_eq!(
            ran.code,
            2,
            "`zaru {}` was expected to refuse: {}",
            arguments.join(" "),
            ran.everything()
        );
        assert!(
            ran.everything().contains("provider.gemini"),
            "`zaru {}` refused without naming the alias, so the absence below is vacuous: {}",
            arguments.join(" "),
            ran.everything()
        );
        absent_everywhere(&home, &ran, &planted, &core, "the stored key");
    }

    // A description a person typed is echoed back by the refusal that would
    // not take it, and that echo must carry nothing but their own words.
    let refused = zaru(
        &home,
        &[],
        &["notes", "tokens", "describe", "provider.gemini", &planted],
    );
    assert_eq!(refused.code, 2);
    absent_everywhere(&home, &refused, &planted, &core, "a description");

    // And `rm` itself, on the credential whose value is planted. The outcome
    // is composed after the record is gone.
    let removed = zaru(&home, &[], &["providers", "keys", "rm", "gemini"]);
    assert_eq!(
        removed.code,
        0,
        "`zaru providers keys rm gemini` did not remove a stored key: {}",
        removed.everything()
    );
    assert!(
        removed.everything().contains("gemini"),
        "`rm` reported nothing about what it removed: {}",
        removed.everything()
    );
    absent_everywhere(&home, &removed, &planted, &core, "the removed key");

    // The store is empty afterwards, and the listing says so rather than
    // printing an empty table -- which is what the absence scan above would
    // otherwise be reading.
    let after = zaru(&home, &[], &["providers", "keys"]);
    assert!(
        after.everything().contains("no provider key"),
        "the listing after `rm` does not say the store is empty: {}",
        after.everything()
    );
    absent_everywhere(&home, &after, &planted, &core, "the removed key");
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
/// A window too small to hold a turn refuses at exit 2, and a window that fits
/// reaches the socket for **the same exit code by a different sentence**.
///
/// # Why both arms carry exit 2, and why that is the point rather than a flaw
///
/// [ADR-0016] D5's `2` is "user-correctable", and both of these are: a window
/// the reader set too small, and a local model server the reader has not
/// started. So the exit code cannot discriminate and **the sentence must** —
/// which is exactly what this check asserts, because an arm that fired for
/// every failure would satisfy any assertion made on the code alone
/// ([Verification lessons] §13, the invariant that holds because both sides
/// are wrong together).
///
/// # No server, no key, no packet
///
/// The assembly refuses **before** the model is called — `ContextPolicy` is
/// consulted at the turn boundary and the socket only afterwards — so the
/// refusing arm needs nothing listening anywhere, which is measured here
/// rather than argued: with `provider.ollama.endpoint` pointed at
/// [`CLOSED_LOOPBACK`], the small window never reaches the port and the large
/// one is refused by the kernel. On a runner with no key and no Ollama, both
/// arms are the same two runs they are on a developer's machine.
///
/// # What this replaced
///
/// Before 2026-09-15 the refusing arm exited **70** with "a defect in Zaru
/// 0.0.0, at crates/zaru-cli/src/cli/classify.rs:1428:0 — this is a bug in
/// Zaru, not something you can configure", measured from the release binary
/// at `828a255` with the window at 1,000 and nothing listening. The assertion
/// on the absence of that sentence is what keeps the old behaviour from
/// coming back quietly.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_window_too_small_refuses_at_exit_2_and_one_that_fits_reaches_the_socket_for_the_same_code() {
    let home = Home::new("context-window");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");

    // The reader's own layer, because the project layer cannot carry a
    // `[provider]` table at all: `./zaru.toml` is ADR-0009 D1's manifest and
    // contributes only `[project]` and `[runtime]`.
    let configure = |tokens: u64| {
        std::fs::write(
            home.path().join(".zaru").join("config.toml"),
            format!(
                "[model]\ndefault = \"llama3.2:3b\"\n\n[provider.default]\nkind = \"ollama\"\n\n                 [provider.ollama]\nendpoint = \"{CLOSED_LOOPBACK}\"\ncontext_tokens = {tokens}\n"
            ),
        )
        .expect("a scratch user file");
    };

    // --- the window is smaller than the tool surface plus one question -----
    configure(1_000);
    let refused = zaru(&home, &[], &["say", "the", "word", "yes"]);

    assert_eq!(
        refused.code,
        2,
        "a window the reader configured is theirs to change, which is ADR-0016 D5's 2; this run \
         exited {} saying: {}",
        refused.code,
        refused.everything()
    );
    assert!(
        !refused.everything().contains("a defect in Zaru"),
        "the reader's own `provider.ollama.context_tokens` was reported as a bug in the product, \
         which is ADR-0016 D3 inverted: {}",
        refused.everything()
    );
    assert!(
        refused.everything().contains("the window allows 1000"),
        "the refusal must say what the window was, because a reader cannot act on \"the window \
         was exceeded\" without knowing by how much: {}",
        refused.everything()
    );
    assert!(
        refused
            .everything()
            .contains("provider.ollama.context_tokens"),
        "the refusal must name the key that sized the window, which is the one thing the defect \
         report it replaced never named: {}",
        refused.everything()
    );
    assert!(
        !refused.everything().contains("Connection refused"),
        "the turn reached the socket, so this arm is measuring the endpoint rather than the \
         window and its sibling below is not a sibling: {}",
        refused.everything()
    );

    // --- the accepting sibling: a window that fits, same exit, other words -
    configure(4_096);
    let reached = zaru(&home, &[], &["say", "the", "word", "yes"]);

    assert_eq!(
        reached.code,
        2,
        "an unreachable local server is also the reader's under ADR-0016 D1 row 2, so the two \
         arms share an exit code and only the sentence tells them apart: {}",
        reached.everything()
    );
    assert!(
        reached.everything().contains("Connection refused")
            || reached.everything().contains("nothing answered"),
        "a window that fits must get past the assembly and reach the endpoint; this run never \
         dialled it: {}",
        reached.everything()
    );
    assert!(
        !reached.everything().contains("the window allows"),
        "the window arm fired on a turn that fits, so it is firing on everything and the check \
         above asserts nothing: {}",
        reached.everything()
    );
}

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

/// A loopback server that records the first request it is sent and answers
/// it with HTTP 500, so the turn ends there.
///
/// It is not a provider: it answers nothing a client could use. It exists so
/// a check can read which model a request named, on the wire, for each kind.
///
/// **It takes up to four requests**, one connection each, and answers every
/// one with 500. Since 2026-09-28 a session asks the provider for the
/// model's window before its first exchange, so the chat request is not the
/// first one to arrive; [`Recorder::request`] returns the first chat request.
struct Recorder {
    origin: String,
    seen: std::sync::mpsc::Receiver<String>,
}

impl Recorder {
    fn start() -> Self {
        use std::io::{Read as _, Write as _};
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
        let origin = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("an address").port()
        );
        let (tell, seen) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..4 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
                let mut request = Vec::new();
                let mut buffer = [0_u8; 8192];
                while let Ok(got) = stream.read(&mut buffer) {
                    if got == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..got]);
                    let text = String::from_utf8_lossy(&request).into_owned();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let _ = stream.write_all(
                b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
                let _ = tell.send(String::from_utf8_lossy(&request).into_owned());
            }
        });
        Self { origin, seen }
    }

    /// The first chat request: the one that carries the conversation, not
    /// the question about the model's window.
    fn request(&self) -> String {
        while let Ok(request) = self.seen.recv_timeout(std::time::Duration::from_secs(30)) {
            let line = request.lines().next().unwrap_or_default();
            if line.contains("/chat") || line.contains(":streamGenerateContent") {
                return request;
            }
        }
        String::new()
    }
}

/// **`--model` takes an alias or an identifier, for every kind.** A value
/// that names an alias is sent as the model that alias resolves to; any other
/// value is sent as given.
///
/// Ruled by the coordinator on 2026-09-28 under directive 58, open to
/// Jeshua's veto, after `zaru --model cheap` asked Gemini for a model called
/// "cheap". Watched red on `5748709`: "--model cheap sent the alias's name to
/// gemini rather than the model `cheap` resolves to".
#[test]
fn model_takes_an_alias_or_an_identifier_and_sends_the_model_for_every_kind() {
    for (kind, segment) in [
        ("gemini", "gemini"),
        ("ollama", "ollama"),
        ("openai-compatible", "openai_compatible"),
    ] {
        for (asked, expected, not_expected) in [
            ("cheap", "scripted-cheap-model", "\"cheap\""),
            ("not-an-alias-7", "not-an-alias-7", "scripted-cheap-model"),
        ] {
            let home = Home::new("model-flag");
            std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
            let recorder = Recorder::start();
            std::fs::write(
                home.path().join(".zaru").join("config.toml"),
                format!(
                    "[model]\ndefault = \"scripted-default-model\"\ncheap = \"scripted-cheap-model\"\n\n\
                     [provider.default]\nkind = \"{kind}\"\n\n\
                     [provider.{segment}]\nendpoint = \"{}\"\ncontext_tokens = 32768\n",
                    recorder.origin
                ),
            )
            .expect("a scratch user file");
            if kind == "gemini" {
                store_a_key(&home, "gemini", &nonce("model-flag").0);
            }

            let ran = zaru(&home, &[], &["--model", asked, "say", "hello"]);
            let request = recorder.request();
            let line = request.lines().next().unwrap_or_default().to_owned();
            let named = if kind == "gemini" {
                line.contains(&format!("/models/{expected}:"))
            } else {
                request.contains(&format!("\"model\":\"{expected}\""))
            };
            assert!(
                named,
                "--model {asked} sent the alias's name to {kind} rather than the model `{asked}` \
                 resolves to, or did not send the identifier as given: {line} {}",
                ran.everything()
            );
            assert!(
                !request.contains(not_expected) || kind == "gemini" && !line.contains(not_expected),
                "--model {asked} sent {not_expected} to {kind}: {line}"
            );
        }
    }
}

/// `zaru models` says where the context window comes from: the provider, the
/// reader's configuration, or this build's default.
///
/// The `ollama` server here is a port nothing listens on, so the provider
/// is asked and does not answer; the window is then the default, or the
/// reader's setting where there is one, and the line says which.
///
/// Red on `254e2b6`, where `zaru models` printed the aliases and no window:
/// "zaru models does not say where the window comes from".
#[test]
fn models_says_where_the_context_window_comes_from() {
    for (setting, expected) in [
        ("", "context window: 4096 tokens, this build's default"),
        (
            "context_tokens = 8000\n",
            "context window: 8000 tokens, from your configuration",
        ),
    ] {
        let home = Home::new("models-window");
        std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
        std::fs::write(
            home.path().join(".zaru").join("config.toml"),
            format!(
                "[model]\ndefault = \"scripted-model\"\n\n[provider.default]\nkind = \"ollama\"\n\n\
                 [provider.ollama]\nendpoint = \"{CLOSED_LOOPBACK}\"\n{setting}"
            ),
        )
        .expect("a scratch user file");
        let ran = zaru(&home, &[], &["models"]);
        assert!(
            ran.stdout.contains(expected)
                && ran
                    .stdout
                    .contains("(the provider was asked and did not say)"),
            "zaru models does not say where the window comes from; wanted {expected:?}: {}",
            ran.everything()
        );
    }
}

/// `zaru models` says when `--model` named an alias.
#[test]
fn models_says_when_the_model_flag_named_an_alias() {
    let home = Home::new("models-flag");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
    std::fs::write(
        home.path().join(".zaru").join("config.toml"),
        "[model]\ndefault = \"scripted-default-model\"\ncheap = \"scripted-cheap-model\"\n",
    )
    .expect("a scratch user file");
    let ran = zaru(&home, &[], &["--model", "cheap", "models"]);
    let default = ran
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("default"))
        .unwrap_or_default()
        .to_owned();
    assert!(
        default.contains("scripted-cheap-model") && default.contains("--model cheap"),
        "zaru models does not say that --model named the alias cheap: {}",
        ran.everything()
    );
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
/// **The seam is the redactor every message passes** and this is the case
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
    // its optional ceiling was -- an absent value means this turn is unlimited.
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
        first_of_the_loop.contains("\"turn_started\"") && first_of_the_loop.contains("\"of\":null"),
        "the turn loop's first record is not the turn starting unlimited: {first_of_the_loop}",
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
    // A project's validators run only once the person approves them.
    let (seen, status) = approve_in_a_terminal(&iterating_home, "y");
    assert_eq!(status, 0, "{seen}");

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

/// [ADR-0010] D2's **eighth** producer: a turn that was refused writes what
/// refused it.
///
/// # The clause this closes, and what was actually wrong
///
/// D5: "The user can read every byte the harness stores about them with
/// `cat`." `Record::Failure` existed as a variant from the day the transcript
/// did and had **no product producer at all**, so every refusal became printed
/// lines and an [ADR-0016] D5 exit code and reached no file — for exactly the
/// turns a person most wants to read back.
///
/// **And it was worse than a gap.** The seventh producer's own amendment makes
/// "a `user` half with no `zaru` half after it" *the interruption*. A refused
/// turn left exactly that shape: a `conversation` user record, a
/// `turn_started`, and nothing after — measured on the release binary at
/// `1e09dfd` as three lines. So the file said a turn the harness had declined
/// was a process that had died. This is not coverage; it is a record that was
/// saying something false.
///
/// # Four properties
///
/// **The headline on the file is the headline the reader was shown**, compared
/// against the binary's own first line of standard error rather than against a
/// literal — so a producer that recorded some other classification's headline,
/// or a constant, fails here.
///
/// **It is the last record of the turn**, so `cat` reads in the order the turn
/// happened: the question, `turn_started`, the work, then what stopped it.
///
/// **A refusal is now distinguishable from an interruption.** The staging is a
/// turn that ran and was refused at the socket, and what discriminates is that
/// something follows `turn_started`.
///
/// **A refusal before the session writes nothing**, because there is nothing
/// to write to: `compose::turn::prepare` resolves everything ahead of the
/// session and its own documentation says "a turn that never began is not a
/// session". That arm is what reddens if the producer is pushed up into
/// `prepare`.
///
/// # What this check does not hold, said rather than implied
///
/// **The accepting sibling is a turn that succeeded and carries no `failure`
/// record, and it cannot be staged here.** `run_one` takes a `&Prepared`
/// holding a `GeminiClient`, and no stub substitutes for it — which
/// `terminal::driver`'s own comment records of the same function: "a mutation
/// deleting it reddens nothing, because that function needs a real provider
/// and no offline check can drive it". So that half is the arc's artefact,
/// run against the real provider under the key discipline and recorded on
/// [ADR-0010]'s Status tracking. Nothing here rounds it up.
///
/// The mutants: deleting the record call; recording on every path rather than
/// on `Exit::Failed`; and recording a constant headline.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn adr_0010_d2s_failure_record_puts_what_refused_the_turn_on_the_transcript() {
    let home = Home::new("conversation-failure");
    let (value, core) = nonce("conversation-failure");
    store_a_key(&home, "gemini", &value);

    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "tell me a joke"],
    );
    assert_eq!(
        ran.code, 3,
        "the staging is a turn that ran and could not reach a model"
    );
    let headline = ran
        .stderr
        .lines()
        .next()
        .expect("a refused turn says something on standard error")
        .to_owned();

    let transcript = transcript_of(&home);
    let last = transcript
        .lines()
        .next_back()
        .expect("a turn that ran wrote records");
    assert!(
        last.starts_with(r#"{"failure":"#),
        "the turn was refused and the last thing on the transcript is not what refused it, so a \
         reader with `cat` sees a question and a turn that simply stops — which is what an \
         interrupted turn looks like:\n{transcript}"
    );
    assert!(
        last.contains(&serde_json_escaped(&headline)),
        "the record does not carry the headline the reader was shown. Shown: {headline:?}. \
         Recorded: {last}"
    );

    // The turn started and something follows it, which is the whole of the
    // distinction between a refusal and an interruption on this file.
    let started = transcript
        .find(r#"{"turn_loop":{"turn_started""#)
        .unwrap_or_else(|| panic!("the turn never started, so this staged nothing:\n{transcript}"));
    assert!(
        transcript[started..].contains(r#"{"failure":"#),
        "nothing follows `turn_started`, so a refused turn is indistinguishable from an \
         interrupted one:\n{transcript}"
    );

    absent_everywhere(&home, &ran, &value, &core, "the stored provider key");

    // The arm that reddens if the producer moves up into `prepare`: a refusal
    // resolved before the session leaves no session to write to.
    let early = Home::new("conversation-failure-early");
    let refused = zaru(&early, &[], &["tell me a joke"]);
    assert_eq!(
        refused.code, 2,
        "a machine with no model configured refuses before the session"
    );
    assert!(
        !early.path().join(".zaru/sessions").exists(),
        "a refusal reached before the session wrote a session directory, so the failure producer \
         has been moved above the one thing that makes a session"
    );
}

/// The JSON spelling of a string, as `serde_json` writes it into a record.
///
/// The headline is compared as it lands on the file rather than as it was
/// printed, because a headline carrying a quote or a backslash would otherwise
/// never match and the check would silently be about nothing.
fn serde_json_escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
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
/// answer — the provider endpoint is a closed port. That absence is exactly
/// the shape an *interrupted* turn leaves, which is how ADR-0010 D4's
/// interruption is read from a `Phase::Started` with nothing closing it.
///
/// **Corrected 2026-09-14 by the `first-run` arc.** This paragraph went on to
/// say "and the turn stays distinguishable from an interrupted one because it
/// has a `turn_ended` record". It had none: the transcript this very check
/// stages was three lines — the notice, the `conversation` user half, and
/// `turn_started` — and nothing followed. So a refused turn and an interrupted
/// one were byte-identical in shape, which is what D2's eighth producer exists
/// to fix; `adr_0010_d2s_failure_record_puts_what_refused_the_turn_on_the_\
/// transcript` is where that is now asserted, and the sentence is true again
/// because a `failure` record follows `turn_started`.
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
        !ran.stdout.contains("--runtime contained"),
        "the warning recommends a tier that is not built and contains nothing: {}",
        ran.stdout
    );
}

/// `contained` and `linked` can be selected and enforce nothing, so selecting
/// one says the same as `bare` and one sentence more: that the tier is not
/// built yet and changes nothing.
///
/// Measured on `970f60a`: a task at `--runtime contained` printed no warning
/// at all, and a tool call the model asked for ran on the machine as at bare.
#[test]
fn a_tier_that_is_not_built_warns_as_bare_does_and_says_it_is_not_built() {
    for tier in ["contained", "linked"] {
        let home = Home::new(&format!("notice-{tier}"));
        let (value, _core) = nonce(&format!("notice-{tier}"));
        store_a_key(&home, "gemini", &value);
        let ran = zaru(
            &home,
            &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
            &[
                "--runtime",
                tier,
                "--model",
                "gemini-3.6-flash",
                "say",
                "hello",
            ],
        );
        assert!(
            ran.stdout.contains("Zaru is not a sandbox"),
            "at `{tier}` the not-a-sandbox warning was not printed, and that tier contains \
             nothing: {}",
            ran.stdout
        );
        assert!(
            ran.stdout.contains(&format!(
                "The {tier} tier is not built yet and changes nothing about how tool calls run."
            )),
            "at `{tier}` the warning does not say the tier is not built: {}",
            ran.stdout
        );
    }
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

/// [ADR-0034] trigger clause 2: "A positive value in a project's `zaru.toml`
/// reaches the outer loop and exhausts exactly at that exchange count."
///
/// # Why this one check does not run a turn through the built binary
///
/// Every other check in this file runs the artefact, and this one runs it only
/// for the configuration half. The binary's turn reaches a model through
/// `ProviderClient`, a closed set of three real clients, so the only way to
/// put a model that keeps asking for tools behind it is a loopback listener
/// serving a provider's protocol -- the fake at the wire that the rulings of
/// 2026-09-05 and 2026-09-14 refuse anywhere in this suite
/// (`iteration_from_outside.rs` and `transport_from_outside.rs` record both).
/// So the clause is driven here in four steps, each through the product's own
/// public functions and the one after it reading what the one before produced:
///
/// 1. **The binary** reads the project's `./zaru.toml`, and `config explain`
///    marks layer 3 as the source of the effective limit.
/// 2. **The same file**, at the same path, is folded by `cli::layers::resolve`
///    over `Files::at` -- the fold `main` runs, over the files it reads -- and
///    turned into the loop's ceiling by `runtime::tool_call_ceiling_for`, which
///    is the function `compose::turn::prepare` calls for the turn.
/// 3. **The outer loop**, `zaru_core::tool_call::run`, takes that ceiling over
///    a model scripted to ask for a tool on every exchange but its last. Under
///    the project's limit the turn ends `Exhausted` at exactly that many
///    rounds, having asked the model exactly that many times; the same script
///    under a project limit one higher is answered on its final exchange. The
///    pair is what "exactly" is held to: one exchange short of the script
///    stops it, and one more lets it finish.
/// 4. **What a person reads**: the binary renders `Exhausted` as
///    `Surface::turn_exhausted` at [ADR-0016] D5's `1`, and that presentation
///    names the count in the expected register -- distinct from a success,
///    which is `0`, and from a failure, which is in the error register.
///
/// **Not exercised, and stated rather than implied:** the hand-off inside
/// `compose::turn` from `Prepared`'s ceiling to `tool_call::run`, which no
/// check can reach without a scripted model behind the binary. The tools are
/// staged too: the clause counts exchanges, and a tool's work is not its
/// subject.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [ADR-0034]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0034-tool-call-exchange-limits
#[test]
fn adr_0034_clause_2_a_projects_exchange_limit_reaches_the_loop_and_exhausts_at_that_count() {
    use zaru_cli::cli::Overrides;
    use zaru_cli::cli::classify::Surface;
    use zaru_cli::cli::layers::{Files, resolve};
    use zaru_cli::failure::{Class, Presentation, SUCCESS};
    use zaru_cli::redaction::HeldSecrets;
    use zaru_cli::tools::WorkingDirectory;
    use zaru_core::iteration::{Clock, ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
    use zaru_core::redaction::Redacted;
    use zaru_core::tool_call::{
        Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome,
        Ports, Start, TokenUsage, ToolCallCeiling, ToolCalling, ToolDecision, ToolDescriptor,
        ToolExecutor, ToolOutcome, ToolRequest, ToolResult, TurnEnding, run,
    };

    /// A model that asks for `fs.read` on every exchange its script holds a
    /// call for, then answers, and counts how often it was asked.
    struct Scripted {
        script: std::sync::Mutex<std::collections::VecDeque<ModelResponse>>,
        asked: std::sync::atomic::AtomicU32,
    }
    impl Scripted {
        fn asking_for_tools(times: u32) -> Self {
            let mut script: std::collections::VecDeque<ModelResponse> = (0..times)
                .map(|n| ModelResponse::Calls {
                    text: String::new(),
                    echo: None,
                    calls: vec![ToolRequest {
                        id: format!("call-{n}"),
                        name: String::from("fs.read"),
                        arguments: String::from(r#"{"path":"notes.txt"}"#),
                    }],
                    tokens: TokenUsage::default(),
                })
                .collect();
            script.push_back(ModelResponse::Text {
                echo: None,
                text: String::from("done"),
                tokens: TokenUsage::default(),
            });
            Self {
                script: std::sync::Mutex::new(script),
                asked: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }
    impl Model for Scripted {
        fn capabilities(&self) -> Capabilities {
            Capabilities { tool_calling: true }
        }
        async fn respond(&self, _request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.script
                .lock()
                .expect("the script is not poisoned")
                .pop_front()
                .ok_or_else(|| PortFailure::new("the script ran out"))
        }
    }

    /// Every call completes: the clause counts exchanges, not what a tool did.
    struct Answering;
    impl ToolExecutor for Answering {
        fn descriptors(&self) -> &[ToolDescriptor] {
            zaru_cli::tools::descriptor_set()
        }
        async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
            Ok(ToolOutcome::Completed {
                decision: ToolDecision {
                    statement: String::from("read notes.txt"),
                    permitted: true,
                },
                result: ToolResult {
                    id: request.id.clone(),
                    content: Redacted::by(&HeldSecrets::none(), "the notes"),
                    failed: false,
                },
            })
        }
    }

    struct Stopped;
    impl Clock for Stopped {
        fn now(&self) -> core::time::Duration {
            core::time::Duration::ZERO
        }
    }

    struct Task;
    impl ContextPolicy for Task {
        async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
            let text = match turn {
                Turn::Initial { task } => (*task).to_owned(),
                Turn::Refinement { refinement } => refinement.as_str().to_owned(),
            };
            Ok(Prompt::new(Redacted::by(&HeldSecrets::none(), &text)))
        }
    }

    struct NeverIterates;
    impl InnerLoop for NeverIterates {
        async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
            unreachable!("this project declares no validators")
        }
    }

    #[derive(Default)]
    struct Endings(Vec<(TurnEnding, u32)>);
    impl EventSink for Endings {
        fn emit(&mut self, event: &Event) {
            if let Event::TurnEnded { ending, rounds, .. } = event {
                self.0.push((*ending, *rounds));
            }
        }
    }

    const LIMIT: u32 = 3;
    let home = Home::new("exchange-limit-exhausts");
    std::fs::write(home.project().join("notes.txt"), "the notes\n").expect("a file to read");
    let manifest = |limit: u32| {
        std::fs::write(
            home.project().join("zaru.toml"),
            format!("[project]\nname = \"acme\"\n\n[runtime]\nmax_tool_exchanges = {limit}\n"),
        )
        .expect("the manifest is written");
    };

    // The project's limit as the product folds it and hands it to the loop:
    // the same files, the same fold, the same function `prepare` calls. The
    // harness's home is the `~/.zaru` the binary was handed through `HOME`.
    let harness_home = zaru_cli::config::Home::at(home.path().join(".zaru"));
    let ceiling_from_the_project = || -> ToolCallCeiling {
        let files = Files::at(
            harness_home.root(),
            Some(WorkingDirectory::at(home.project()).expect("the project resolves")),
        );
        let resolution = resolve(&Overrides::default(), Vec::new(), &files)
            .expect("a manifest with a positive limit loads");
        zaru_cli::runtime::tool_call_ceiling_for(&resolution)
            .expect("a positive whole number is a limit")
    };

    // The loop, over a script that needs `LIMIT + 1` exchanges to answer.
    let turn = |ceiling: ToolCallCeiling| -> (Outcome, u32, Vec<(TurnEnding, u32)>) {
        let model = Scripted::asking_for_tools(LIMIT);
        let mut tools = Answering;
        let mut endings = Endings::default();
        let outcome = zaru_cli::compose::turn::runtime()
            .expect("a runtime builds")
            .block_on(run::<_, _, _, _, _, NeverIterates>(
                1,
                Start::Task("read the notes until told to stop"),
                ceiling,
                ToolCalling::required(&model, "scripted").expect("the script can call tools"),
                Ports {
                    model: &model,
                    tools: &mut tools,
                    context: &Task,
                    clock: &Stopped,
                    redactor: &HeldSecrets::none(),
                },
                None,
                &mut [&mut endings],
            ))
            .expect("no port failed");
        println!("   under {ceiling:?}: {outcome:?}");
        (
            outcome,
            model.asked.load(std::sync::atomic::Ordering::SeqCst),
            endings.0,
        )
    };

    // --- 1. the binary reads the project's limit from `./zaru.toml` --------
    manifest(LIMIT);
    let explained = zaru(
        &home,
        &[],
        &["config", "explain", "runtime.max_tool_exchanges"],
    );
    assert_eq!(explained.code, 0, "{}", explained.everything());
    assert!(
        explained
            .stdout
            .contains(&format!("runtime.max_tool_exchanges = {LIMIT}")),
        "the binary must resolve the project's {LIMIT} as the effective limit: {}",
        explained.stdout
    );
    assert!(
        explained.stdout.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("3  ")
                && line.contains("zaru.toml")
                && line.trim_end().ends_with("\u{2190} effective")
        }),
        "`config explain` must name the project file, layer 3, as the limit's source: {}",
        explained.stdout
    );

    // --- 2 and 3. the same file reaches the loop, which stops at LIMIT ------
    //
    // Asserted on what the loop did rather than on the ceiling's value, so a
    // limit lost or shifted anywhere between the file and the loop is caught
    // by the behaviour the clause names.
    let (outcome, asked, endings) = turn(ceiling_from_the_project());
    let Outcome::Exhausted { rounds, calls, .. } = outcome else {
        panic!(
            "a project limit of {LIMIT} did not exhaust a turn whose model asked for a tool on \
             each of its first {LIMIT} exchanges; the turn ended {outcome:?}"
        )
    };
    assert_eq!(
        (rounds, asked),
        (LIMIT, LIMIT),
        "a project limit of {LIMIT} must exhaust at exactly {LIMIT} exchange(s); the turn made \
         {rounds} round(s) and asked the model {asked} time(s)"
    );
    assert_eq!(
        calls, LIMIT,
        "each exchange asked for one tool, so {LIMIT} should have run"
    );
    assert_eq!(
        endings,
        vec![(TurnEnding::CeilingReached, LIMIT)],
        "the turn must end once, at the ceiling, after {LIMIT} rounds"
    );

    // --- the accepting sibling: one more exchange lets the script answer ----
    manifest(LIMIT + 1);
    let (answered, asked, _) = turn(ceiling_from_the_project());
    assert!(
        matches!(answered, Outcome::Answered { rounds, .. } if rounds == LIMIT + 1),
        "a project limit of {} must let the script answer on its final exchange, or the \
         exhaustion above may be every limit's; the turn ended {answered:?}",
        LIMIT + 1
    );
    assert_eq!(asked, LIMIT + 1);

    // --- 4. what a person reads: the count, in neither success nor error ---
    let shown = Surface::turn_exhausted(rounds, calls);
    let presentation = Presentation::of(&shown);
    let said = presentation.to_string();
    println!("   a person reads: {said}");
    assert!(
        said.contains(&format!("ceiling of {LIMIT} exchange(s)")),
        "the exhaustion must name the count the project set: {said}"
    );
    assert_eq!(
        (shown.class(), shown.exit_code()),
        (Class::Expected, 1),
        "exhaustion is ADR-0016 D1's expected class at D5's 1: {said}"
    );
    assert_ne!(shown.exit_code(), SUCCESS, "exhaustion is not a success");
    assert!(
        !presentation.is_the_error_register(),
        "exhaustion is not a failure and must not render in the error register: {said}"
    );
}

/// [ADR-0034] trigger clause 3: "Zero, negative, and non-integer values are
/// refused with the key and actionable reason named."
///
/// # Met where a person meets it
///
/// A `./zaru.toml` carrying `[runtime] max_tool_exchanges`, and a task run
/// through the built binary. The provider is `ollama` at [`CLOSED_LOOPBACK`],
/// so nothing needs a key and the one way a turn can get past the refusal is
/// visibly: by dialling a port nothing listens on.
///
/// Two refusal sites are reached, and both are the product's own. A zero or a
/// negative number is an integer, so the configuration loads and the turn's
/// composition refuses it before any session exists; a fraction, a word and a
/// boolean are not integers, so the file is refused at load -- the fraction by
/// the reader, which admits no float to any key, and the other two by
/// [ADR-0014]'s schema, which declares this key an integer.
///
/// **A quoted `"32"` is not among them, deliberately.** The schema coerces
/// text that parses as a whole number at every layer -- the environment's
/// values are all text -- so `"32"` is the integer 32 by the time anything
/// reads it, and it is a limit rather than a non-integer.
///
/// # What "actionable" is held to
///
/// D2's own sentences: "Its absence is the explicit unlimited state; zero and
/// negative values are refused rather than given a second meaning." A reader
/// who wrote `0` meaning "no limit" -- the reading Alternative 2 rejects -- can
/// act only if the refusal says how to ask for no limit. And Alternative 3
/// rejects coupling this key to `runtime.max_iterations`, so a refusal
/// calling it an *iteration ceiling* names a setting the reader did not set.
///
/// # The accepting sibling
///
/// `4`, the same file and the same task, gets past the composition and is
/// refused by the kernel at the closed port. Without it, a harness refusing
/// every value of the key would satisfy every assertion above it.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [ADR-0034]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0034-tool-call-exchange-limits
#[test]
fn adr_0034_clause_3_a_zero_negative_or_non_integer_exchange_limit_is_refused_naming_the_key() {
    let home = Home::new("exchange-limit-refused");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
    std::fs::write(
        home.path().join(".zaru").join("config.toml"),
        format!(
            "[model]\ndefault = \"llama3.2:3b\"\n\n[provider.default]\nkind = \"ollama\"\n\n\
             [provider.ollama]\nendpoint = \"{CLOSED_LOOPBACK}\"\n"
        ),
    )
    .expect("a scratch user file");
    let manifest = |value: &str| {
        std::fs::write(
            home.project().join("zaru.toml"),
            format!("[project]\nname = \"acme\"\n\n[runtime]\nmax_tool_exchanges = {value}\n"),
        )
        .expect("the manifest is written");
    };
    let reached_the_socket = |ran: &Ran| {
        ran.everything().contains("Connection refused")
            || ran.everything().contains("nothing answered")
    };

    // --- zero and a negative: integers, refused by the turn's composition --
    for value in ["0", "-3"] {
        manifest(value);
        let ran = zaru(&home, &[], &["say", "the", "word", "yes"]);
        let said = ran.stderr.as_str();

        assert_eq!(
            ran.code,
            2,
            "`max_tool_exchanges = {value}` is the reader's to change, which is ADR-0016 D5's 2; \
             this run exited {} saying: {}",
            ran.code,
            ran.everything()
        );
        assert!(
            !reached_the_socket(&ran),
            "`max_tool_exchanges = {value}` reached the provider, so it was not refused: {}",
            ran.everything()
        );
        assert!(
            home.sessions().is_empty(),
            "a turn refused before it began is not a session, and `{value}` left one behind"
        );
        assert!(
            said.contains("runtime.max_tool_exchanges") && said.contains(&format!("is {value}")),
            "the refusal of `{value}` must name the key and the value the reader wrote: {said}"
        );
        assert!(
            !said.contains("iteration ceiling"),
            "the refusal of `{value}` calls the exchange limit an iteration ceiling, which is \
             `runtime.max_iterations` -- a setting ADR-0034's Alternative 3 keeps apart from this \
             one and the reader did not set: {said}"
        );
        assert!(
            said.contains("unlimited"),
            "the refusal of `{value}` must say how to ask for no limit, because ADR-0034 D2 makes \
             absence the unlimited state and a reader who wrote {value} for it cannot otherwise \
             act: {said}"
        );
        assert!(
            said.contains("config explain runtime.max_tool_exchanges"),
            "the remedy must name the command that shows which layer set it: {said}"
        );
    }

    // --- a fraction and a quoted number: refused by the schema at load ----
    let file = home.project().join("zaru.toml").display().to_string();
    for (value, why) in [
        ("2.5", "holds float"),
        ("\"many\"", "the text given does not read as one"),
        ("true", "was given a boolean"),
    ] {
        manifest(value);
        let ran = zaru(&home, &[], &["say", "the", "word", "yes"]);
        let said = ran.stderr.as_str();

        assert_eq!(
            ran.code,
            2,
            "`max_tool_exchanges = {value}` is the reader's to change; this run exited {} saying: \
             {}",
            ran.code,
            ran.everything()
        );
        assert!(
            !reached_the_socket(&ran),
            "`max_tool_exchanges = {value}` reached the provider, so it was not refused: {}",
            ran.everything()
        );
        assert!(
            said.contains("runtime.max_tool_exchanges")
                && (said.contains(&file) || said.contains("project config (layer 3)")),
            "the refusal of `{value}` must name the key and where it was set: {said}"
        );
        assert!(
            said.contains(why),
            "the refusal of `{value}` must say why the value is not one the key takes: {said}"
        );
    }

    // --- the accepting sibling: a positive whole number reaches the port ---
    manifest("4");
    let accepted = zaru(&home, &[], &["say", "the", "word", "yes"]);
    assert!(
        reached_the_socket(&accepted),
        "`max_tool_exchanges = 4` never reached the provider, so the refusals above may be \
         refusing every value of the key: {}",
        accepted.everything()
    );
    assert!(
        !accepted.everything().contains("max_tool_exchanges"),
        "a positive limit was refused: {}",
        accepted.everything()
    );
}

/// [ADR-0034] trigger clause 4: "Configuration resolution preserves a
/// higher-layer finite limit against a project attempt to raise or remove it,
/// and `zaru config explain runtime.max_tool_exchanges` identifies its
/// effective source."
///
/// # Met where a person meets it
///
/// The user's own `~/.zaru/config.toml` grants 9, and `./zaru.toml` is
/// rewritten three ways, each read back through the built binary's `config
/// explain`, which is the only place a person can see where a value came
/// from. [ADR-0014] D3's block prints one row per layer, highest first, and
/// marks the highest layer that set the key `← effective`.
///
/// - **Lowered to 3**: the project row is marked and carries 3; the user row
///   carries 9 and is not marked. This is the accepting sibling -- without
///   it, a harness refusing every project value of the key would pass the
///   raise arm.
/// - **Raised to 12**: refused at exit 2 naming the key and both numbers, and
///   no block is printed that could read as the project's value having won.
/// - **Removed** -- the project stops setting the key: the user's 9 is still
///   the effective value and the user row is the one marked. A project has no
///   spelling for "unlimited" but absence, and absence cannot remove what a
///   layer below it set; the one other spelling a reader might try, `0`, is
///   clause 3's refusal.
///
/// Not the environment layer: layer 4 is *above* the project's layer 3, so a
/// project "lowering" an environment value is decided by precedence rather
/// than by D6 -- the trap `a_project_may_lower_the_iteration_ceiling_and_may_not_raise_it`
/// records falling into once.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [ADR-0034]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0034-tool-call-exchange-limits
#[test]
fn adr_0034_clause_4_a_users_exchange_limit_survives_the_project_and_explain_names_its_source() {
    let home = Home::new("exchange-limit-explain");
    std::fs::create_dir_all(home.path().join(".zaru")).expect("a scratch ~/.zaru");
    std::fs::write(
        home.path().join(".zaru/config.toml"),
        "[runtime]\nmax_tool_exchanges = 9\n",
    )
    .expect("the user's configuration is written");
    let manifest = |runtime: &str| {
        std::fs::write(
            home.project().join("zaru.toml"),
            format!("[project]\nname = \"acme\"\n{runtime}"),
        )
        .expect("the manifest is written");
    };
    let explain = || {
        zaru(
            &home,
            &[],
            &["config", "explain", "runtime.max_tool_exchanges"],
        )
    };
    // D3's row for one layer: `  <n>  <source>  <value>[  ← effective]`.
    let row = |ran: &Ran, layer: char| -> String {
        ran.stdout
            .lines()
            .find(|line| {
                let line = line.trim_start();
                line.starts_with(layer) && line[layer.len_utf8()..].starts_with("  ")
            })
            .unwrap_or_else(|| panic!("no row for layer {layer} in: {}", ran.stdout))
            .to_owned()
    };
    let effective = "\u{2190} effective";

    // --- lowered: the project's 3 wins, and explain says it was the project --
    manifest("\n[runtime]\nmax_tool_exchanges = 3\n");
    let lowered = explain();
    assert_eq!(
        lowered.code,
        0,
        "D6 permits a project lowering a limit a layer below granted: {}",
        lowered.everything()
    );
    assert!(
        lowered.stdout.contains("runtime.max_tool_exchanges = 3"),
        "the project's lower limit is the effective one: {}",
        lowered.stdout
    );
    let (project, user) = (row(&lowered, '3'), row(&lowered, '2'));
    assert!(
        project.contains("zaru.toml") && project.trim_end().ends_with(effective),
        "explain must mark the project file as the effective source of a lowered limit: {}",
        lowered.stdout
    );
    assert!(
        user.contains(".zaru/config.toml") && user.trim_end().ends_with(" 9"),
        "the user's 9 is shown against its own file and is not the effective one: {}",
        lowered.stdout
    );

    // --- raised: refused, naming the key and both numbers -------------------
    manifest("\n[runtime]\nmax_tool_exchanges = 12\n");
    let raised = explain();
    assert_eq!(
        raised.code,
        2,
        "D6 forbids a project raising a user's finite limit; this run exited {} saying: {}",
        raised.code,
        raised.everything()
    );
    assert!(
        raised.stderr.contains("runtime.max_tool_exchanges")
            && raised.stderr.contains("12")
            && raised.stderr.contains('9'),
        "the refusal names the key, what the project asked and what the user granted: {}",
        raised.stderr
    );
    assert!(
        !raised.stdout.contains("runtime.max_tool_exchanges = 12"),
        "a refused raise must not be explained as the effective value: {}",
        raised.stdout
    );

    // --- removed: the user's 9 stands, and explain says it was the user -----
    manifest("");
    let removed = explain();
    assert_eq!(removed.code, 0, "{}", removed.everything());
    assert!(
        removed.stdout.contains("runtime.max_tool_exchanges = 9"),
        "a project that stops setting the key does not remove the user's limit: {}",
        removed.stdout
    );
    let user = row(&removed, '2');
    assert!(
        user.contains(".zaru/config.toml") && user.trim_end().ends_with(effective),
        "explain must mark the user's file as the effective source once the project sets \
         nothing: {}",
        removed.stdout
    );
    assert!(
        !row(&removed, '3').contains(effective),
        "the project sets nothing and must not be marked: {}",
        removed.stdout
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

/// The out-of-session contract of `zaru providers keys add <kind>` is exactly
/// what it was when one function did the reading and the storing.
///
/// # Why this exists, and what the mutant is
///
/// On 2026-09-14 that function was split so [ADR-0015] D2's in-session
/// spelling could reach the storing half with bytes read at a masked question
/// instead of from standard input. **A refactor that changes what a user's
/// `printf … | zaru …` does is not a refactor**, and the property it could
/// have broken is the trim rule: `printf '%s\n'`, `printf '%s\r\n'` and
/// `printf '%s'` all reach this surface and all three must store the same
/// bytes, because the line ending is the shell's rather than the user's.
///
/// **The three arms are compared against each other rather than against a
/// constant**, so the check states "these are one contract" rather than
/// restating today's answer. What is asserted absolutely is the pair of lines
/// a user reads, because those *are* the contract's visible half.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[test]
fn the_out_of_session_key_add_contract_is_unchanged_by_the_split() {
    use std::io::Write as _;

    let mut printed = Vec::new();
    for (label, ending) in [("bare", ""), ("newline", "\n"), ("crlf", "\r\n")] {
        let home = Home::new(&format!("key-add-{label}"));
        let (value, _core) = nonce(&format!("key-add-{label}"));

        let mut child = owned::command(env!("CARGO_BIN_EXE_zaru"))
            .args(["providers", "keys", "add", "gemini"])
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
            .stdin()
            .write_all(format!("{value}{ending}").as_bytes())
            .expect("the key reaches the child");
        let output = child.wait_with_output().expect("the child exits");
        let stdout = String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8");
        assert!(
            output.status.success(),
            "storing a key offered with a {label} ending failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout.lines().collect::<Vec<_>>(),
            vec![
                "stored a `gemini` key under the alias `provider.gemini`.",
                "  the value is sealed and is not printed by any command.",
            ],
            "the two lines a user reads changed, for the {label} ending"
        );

        // What was stored, read back through the listing, which prints the
        // alias and the kind and never the value.
        let listed = zaru(&home, &[], &["providers", "keys"]);
        assert_eq!(listed.code, 0, "the listing failed after a {label} ending");
        assert!(
            !listed.stdout.contains(&value),
            "the listing printed the key itself:\n{}",
            listed.stdout
        );
        printed.push((label, listed.stdout));
    }

    let (first_label, first) = &printed[0];
    for (label, listing) in &printed[1..] {
        assert_eq!(
            first, listing,
            "a {first_label} ending and a {label} ending did not store the same shape"
        );
    }
}

/// A *second* line ending is the user's, and is refused naming nothing of it.
///
/// The sibling of the check above, and the arm that pins "**exactly one**":
/// the helper removes the one ending a `printf` or a `return` adds, and what
/// is left is the user's. A trimming helper written one character wider —
/// `trim_end()` rather than one `strip_suffix` — would pass the check above
/// and fail this one.
///
/// **Measured rather than assumed.** A first attempt asserted that a key with
/// a space in the *middle* of it is refused, on the strength of a sentence
/// that stood in `cli::run`'s own documentation. It is not:
/// `Secret::provider` refuses an empty value, a control character and
/// *surrounding* whitespace, and an interior space is stored. The sentence was
/// false and is corrected at the helper rather than the rule widened to match
/// it — what a credential may contain is ADR-0007's to say.
#[test]
fn a_second_line_ending_is_the_users_and_is_refused_without_being_quoted() {
    use std::io::Write as _;

    let home = Home::new("key-add-whitespace");
    let (value, _core) = nonce("key-add-whitespace");
    let offered = format!("{value}\n");

    let mut child = owned::command(env!("CARGO_BIN_EXE_zaru"))
        .args(["providers", "keys", "add", "gemini"])
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
        .stdin()
        .write_all(format!("{offered}\n").as_bytes())
        .expect("the key reaches the child");
    let output = child.wait_with_output().expect("the child exits");
    let stderr = String::from_utf8(output.stderr).expect("zaru printed invalid UTF-8 on stderr");

    assert!(
        !output.status.success(),
        "a key offered with two line endings was stored, so the helper trimmed more than one"
    );
    assert!(
        !stderr.contains(&value),
        "the refusal quoted part of the offered key:\n{stderr}"
    );
}

// ------------------------------------ a stored key and a keyless provider

/// Run the built binary as [`zaru`] does, but with no sealing key: the
/// machine has no keyring and `ZARU_CREDENTIAL_KEY` is not set.
fn zaru_without_the_sealing_key(
    home: &Home,
    variables: &[(&str, &str)],
    arguments: &[&str],
) -> Ran {
    let mut command = owned::command(env!("CARGO_BIN_EXE_zaru"));
    command
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
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
    println!("-- zaru {} (no sealing key) --", arguments.join(" "));
    for line in ran.stdout.lines() {
        println!("   {line}");
    }
    for line in ran.stderr.lines() {
        println!(" ! {line}");
    }
    println!("   exit {}", ran.code);
    ran
}

/// A task that needs nothing from the credential store does not need the
/// sealing key, even when a key is stored.
///
/// Measured on `970f60a`: with a `gemini` key stored and `ollama` chosen, a
/// task on a machine with no keyring and no `ZARU_CREDENTIAL_KEY` was refused
/// at exit 2, "there is no sealing key", though `ollama` needs no key. Now it
/// goes on to the provider (a closed port here, so it then says nothing
/// answered), and says in one sentence that the stored keys could not be
/// read and so cannot be removed from what is sent. A provider that needs its
/// key is still refused.
#[test]
fn a_keyless_provider_does_not_need_the_sealing_key_when_a_key_is_stored() {
    let home = Home::new("keyless-with-a-stored-key");
    let (value, _core) = nonce("keyless-with-a-stored-key");
    store_a_key(&home, "gemini", &value);

    let ran = zaru_without_the_sealing_key(
        &home,
        &[
            ("ZARU_PROVIDER_DEFAULT_KIND", "ollama"),
            ("ZARU_PROVIDER_OLLAMA_ENDPOINT", CLOSED_LOOPBACK),
        ],
        &["--model", "llama3.2", "say", "hello"],
    );
    assert!(
        !ran.stderr.contains("sealing key") && ran.stderr.contains("nothing answered at"),
        "a task for a provider that needs no key was refused for want of the key that seals the \
         stored ones (exit {}); it should have gone on to the provider, a closed port here: {}",
        ran.code,
        ran.everything()
    );
    assert!(
        ran.stdout
            .contains(zaru_cli::compose::prose::STORED_KEYS_UNREAD),
        "the task went on without saying the stored keys could not be read: {}",
        ran.stdout
    );

    // The accepting sibling: a provider that needs its key is still refused.
    let refused = zaru_without_the_sealing_key(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "say", "hello"],
    );
    assert_eq!(refused.code, 2, "{}", refused.everything());
    assert!(refused.stderr.contains("sealing key"), "{}", refused.stderr);
}

// ------------------------------------------- a project's validators, approved

/// A manifest whose one validator leaves a file behind when it runs.
const PLANTING_MANIFEST: &str =
    "[[validator]]\nname = \"plant\"\nrun = \"touch VALIDATOR-RAN\"\nexpect = \"exit-zero\"\n";

/// A project's validators are commands, and none runs until the person has
/// approved them.
///
/// Measured on `970f60a` before this check existed: with this manifest, a
/// task given with `--mode ask` and standard input on `/dev/null` ran the
/// validator and left `VALIDATOR-RAN` in the project. Here the provider is a
/// closed port, so an unfixed build goes on to the provider and exits 3; a
/// fixed one refuses at exit 2, before anything runs, in every mode.
#[test]
fn a_projects_validators_do_not_run_until_the_person_approves_them() {
    let home = Home::new("validators-unapproved");
    let (value, _core) = nonce("validators-unapproved");
    store_a_key(&home, "gemini", &value);
    std::fs::write(home.project().join("zaru.toml"), PLANTING_MANIFEST)
        .expect("the manifest is written");

    for mode in ["ask", "allow", "yolo"] {
        let ran = zaru(
            &home,
            &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
            &["--mode", mode, "--model", "gemini-3.6-flash", "build", "it"],
        );
        assert_eq!(
            ran.code,
            2,
            "in `{mode}` mode a project whose validators nobody approved went on with the task \
             (exit {}), so its commands would have run as soon as the model answered: {}",
            ran.code,
            ran.everything()
        );
        assert!(
            ran.stderr.contains("zaru validators approve"),
            "the refusal must say how to approve the validators: {}",
            ran.stderr
        );
        assert!(
            !home.project().join("VALIDATOR-RAN").exists(),
            "in `{mode}` mode the validator ran"
        );
    }
    assert!(
        !ran_anything(&home),
        "a task refused before anything ran still reached the model or a validator"
    );
}

/// Run `zaru validators approve` in a pseudo-terminal and type `answer`.
///
/// `script` from util-linux puts the command on a terminal and relays what is
/// written to its standard input, the way `terminal_from_outside.rs` drives a
/// session. Returns everything the terminal was sent and the exit status.
fn approve_in_a_terminal(home: &Home, answer: &str) -> (String, i32) {
    use std::io::Write as _;
    let zaru = env!("CARGO_BIN_EXE_zaru");
    let inner = format!("'{zaru}' validators approve; echo ZARU-STATUS=$?");
    let mut child = owned::command("script")
        .args(["-q", "-e", "-c", &inner, "/dev/null"])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .current_dir(home.project())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("`script` from util-linux allocates the terminal this check answers on");
    child
        .stdin()
        .write_all(format!("{answer}\n").as_bytes())
        .expect("the answer reaches the terminal");
    let output = child.wait_with_output().expect("script exits");
    let seen = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    let status = seen
        .split("ZARU-STATUS=")
        .nth(1)
        .and_then(|rest| rest.trim().lines().next())
        .and_then(|code| code.trim().parse().ok())
        .unwrap_or_else(|| panic!("the terminal never showed zaru's exit status: {seen}"));
    println!("-- zaru validators approve, answered {answer:?} --\n{seen}");
    (seen, status)
}

/// Approving shows every validator's name and exact command and asks once; a
/// yes is remembered for this directory and this exact set, a changed set is
/// asked about again, and `zaru validators list` shows what is approved.
#[test]
fn approving_a_projects_validators_shows_them_asks_once_and_remembers_the_exact_set() {
    let home = Home::new("validators-approve");
    let (value, _core) = nonce("validators-approve");
    store_a_key(&home, "gemini", &value);
    std::fs::write(home.project().join("zaru.toml"), PLANTING_MANIFEST)
        .expect("the manifest is written");

    // A pipe is refused: nothing typed it, so nothing is approved.
    let piped = zaru(&home, &[], &["validators", "approve"]);
    assert_eq!(piped.code, 2, "{}", piped.everything());
    assert!(
        piped.stdout.contains("  plant: touch VALIDATOR-RAN"),
        "the refusal still shows the command it would have asked about: {}",
        piped.stdout
    );

    // A no approves nothing.
    let (seen, status) = approve_in_a_terminal(&home, "n");
    assert_eq!(status, 0, "{seen}");
    assert!(
        seen.contains(zaru_cli::validators::approval::ASK_FIRST),
        "{seen}"
    );
    assert!(seen.contains("  plant: touch VALIDATOR-RAN"), "{seen}");
    assert!(seen.contains("Nothing was approved."), "{seen}");
    let refused = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        refused.code,
        2,
        "a no approved something: {}",
        refused.everything()
    );

    // A yes approves this set in this directory.
    let (seen, status) = approve_in_a_terminal(&home, "y");
    assert_eq!(status, 0, "{seen}");
    assert!(seen.contains("Approved."), "{seen}");
    let listed = zaru(&home, &[], &["validators", "list"]);
    assert_eq!(listed.code, 0, "{}", listed.everything());
    assert!(
        listed.stdout.contains("  plant: touch VALIDATOR-RAN"),
        "the listing must show the approved command: {}",
        listed.stdout
    );
    let approved = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        approved.code,
        3,
        "an approved project goes on to the provider, which is a closed port here: {}",
        approved.everything()
    );

    // Changed validators are asked about again, and the question says what
    // changed.
    std::fs::write(
        home.project().join("zaru.toml"),
        PLANTING_MANIFEST.replace("touch VALIDATOR-RAN", "touch SOMETHING-ELSE"),
    )
    .expect("the manifest is rewritten");
    let changed = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(changed.code, 2, "{}", changed.everything());
    assert!(
        changed.stderr.contains("changed since you approved them"),
        "{}",
        changed.stderr
    );
    let (seen, _) = approve_in_a_terminal(&home, "n");
    assert!(
        seen.contains(zaru_cli::validators::approval::ASK_AGAIN),
        "{seen}"
    );
    assert!(
        seen.contains(
            "changed plant: it ran touch VALIDATOR-RAN and now runs touch SOMETHING-ELSE"
        ),
        "{seen}"
    );
    assert!(!home.project().join("VALIDATOR-RAN").exists());
    assert!(!home.project().join("SOMETHING-ELSE").exists());
}

/// Whether any session under this home recorded an iteration or a model call.
fn ran_anything(home: &Home) -> bool {
    home.sessions().iter().any(|session| {
        std::fs::read_to_string(session.join("transcript.jsonl"))
            .is_ok_and(|text| text.contains("iteration_started") || text.contains("exchange"))
    })
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
