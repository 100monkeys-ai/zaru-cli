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
//! # Three of these are security-corpus cases
//!
//! [Testing]: "Every escape found at a security boundary … joins a permanent
//! hostile-input corpus as its reproduction", and the corpus only grows. The
//! three are the key's absence from everything a turn produces, a call that
//! needed a confirmation nobody could give, and a held secret in a tool result
//! reaching a model. Each has an **accepting sibling** beside it, because an
//! absence assertion is satisfied by a harness that does nothing at all.
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
    assert!(
        lines[0].contains("\"turn_started\"") && lines[0].contains("\"of\":8"),
        "the first record is not the turn starting at this binary's ceiling: {}",
        lines[0]
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

/// A project that declares validators is refused rather than run.
///
/// [ADR-0009] D4 branches on the manifest and this build has no iteration loop
/// to branch into. Running the tool-call loop instead would report work as
/// done that nothing checked, which is D2's silent green one layer up.
///
/// Its **accepting sibling** is the second half: the same project with its
/// validators removed runs a turn, so the refusal is about the validators
/// rather than about the manifest existing.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[test]
fn a_project_that_declares_validators_is_refused_and_one_without_runs() {
    let home = Home::new("validators");
    let (value, _core) = nonce("validators");
    store_a_key(&home, "gemini", &value);

    std::fs::write(
        home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n\n[[validator]]\nname = \"build\"\nrun = \"true\"\nexpect = \
         \"exit-zero\"\n",
    )
    .expect("the manifest is written");

    let refused = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        refused.code, 4,
        "a project that asked for validation and cannot have it is not the user's fault"
    );
    assert!(
        refused.stderr.contains("iteration loop"),
        "the refusal must name what is missing: {}",
        refused.stderr
    );
    assert!(
        !home.path().join(".zaru/sessions").exists(),
        "a project that could not be served still had a session written for it"
    );

    // The accepting sibling: the same manifest without validators runs.
    std::fs::write(
        home.project().join("zaru.toml"),
        "[project]\nname = \"acme\"\n",
    )
    .expect("the manifest is rewritten");
    let ran = zaru(
        &home,
        &[("ZARU_PROVIDER_GEMINI_ENDPOINT", CLOSED_LOOPBACK)],
        &["--model", "gemini-3.6-flash", "build", "it"],
    );
    assert_eq!(
        ran.code, 3,
        "a project with a manifest and no validators runs a turn, which then fails at the socket"
    );
    assert!(
        home.path().join(".zaru/sessions").exists(),
        "the turn that ran created no session"
    );
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
