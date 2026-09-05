// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D4's branch, driven from the built binary.
//!
//! # A loopback server, and why one had to be built
//!
//! The iteration loop cannot be reached from `zaru` without a model that
//! answers, and the runner has no key and no business having one. Every
//! previous check that needed a provider used a **closed** loopback port, which
//! makes a socket fail and is enough for [ADR-0016] D5's `3` and for nothing
//! else: a refusal is not a loop.
//!
//! So this file binds `127.0.0.1:0` and serves the responses. It is a
//! stand-in for **Google**, not for anything this workspace wrote: the bytes
//! travel over a real socket, `reqwest` parses them, `providers::gemini::map`
//! maps them, and every seam between the socket and `ExecutionOutcome` is the
//! product's own. It is the same argument `zaru-notes` makes for
//! `tokio::io::duplex` — "real protocol bytes, nothing listens on a port" —
//! one notch weaker, because something does listen, on the loopback
//! interface, inside the test process. **No packet leaves the machine and no
//! credential exists**: the key in the store is a nonce and the server never
//! looks at it.
//!
//! **The response bodies are shaped from `providers/gemini/recorded/`**, which
//! are real responses the API returned, rather than from a shape somebody
//! imagined. Where a case needs a different function call, the call changes
//! and the envelope does not.
//!
//! The `composer-wiring` arc proposed this and did not build it; the coordinator
//! ruled the outside caller this arc's on 2026-09-05. It is flagged in that
//! arc's report as a decision open to veto, because a server in `tests/` is
//! the kind of thing [Testing]'s "build the seam, not a mock" is about — and
//! the reading taken is that a provider is not our seam.
//!
//! # What is deliberately not here
//!
//! **A validator killed at the process ceiling.** `cli::layers::PROCESS_CEILING`
//! is a compiled-in two minutes with no configuration key, so a check that
//! reached it would take two minutes. What is asserted instead is the property
//! that matters and is reachable — a validator whose command exits non-zero is
//! exhaustion and never success — and the kill itself is `process`'s own
//! check. Stated rather than skipped silently.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The sealing key, so the store opens without a keyring.
const SEALING_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A nonce standing where a provider key would be. It is never sent anywhere
/// that reads it: the server below ignores every header.
const NONCE_KEY: &str = "not-a-key-4173-iteration-wiring";

// --- The stand-in for Google -----------------------------------------------

/// A loopback HTTP server answering a fixed queue of bodies, once each.
struct Provider {
    origin: String,
    thread: Option<std::thread::JoinHandle<usize>>,
}

impl Provider {
    /// Serve these bodies, in order, one per request.
    ///
    /// The thread returns how many requests it served, which a check reads to
    /// assert how many exchanges the loop actually made — a number no other
    /// instrument here can see.
    fn serving(bodies: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let origin = format!(
            "http://{}",
            listener.local_addr().expect("the bound address")
        );
        let wanted = bodies.len();
        let thread = std::thread::spawn(move || {
            let mut queue = bodies.into_iter();
            let mut served = 0;
            // The client is `reqwest` and it pools connections, so the
            // responses are keep-alive and several requests arrive on one
            // socket. Closing after each one raced the pool: the second
            // request went out on a socket the server had already dropped and
            // came back as "error sending request", which is a flake rather
            // than a finding. If the peer does close, the outer loop accepts
            // another connection and carries on.
            'accepting: while served < wanted {
                let Ok((stream, _)) = listener.accept() else {
                    break;
                };
                let mut writer = stream.try_clone().expect("the stream clones for writing");
                let mut reader = BufReader::new(stream);
                while served < wanted {
                    let mut length = 0usize;
                    let mut saw_a_request = false;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            // The peer closed. Take the next connection.
                            if !saw_a_request {
                                continue 'accepting;
                            }
                            continue 'accepting;
                        }
                        saw_a_request = true;
                        if let Some(value) = line
                            .to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                        {
                            length = value;
                        }
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                    }
                    let mut sink = vec![0u8; length];
                    if reader.read_exact(&mut sink).is_err() {
                        continue 'accepting;
                    }
                    let body = queue.next().expect("served fewer than were queued");
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                         {}\r\n\r\n{body}",
                        body.len()
                    );
                    if writer.write_all(response.as_bytes()).is_err() {
                        continue 'accepting;
                    }
                    let _ = writer.flush();
                    served += 1;
                }
            }
            served
        });
        Self {
            origin,
            thread: Some(thread),
        }
    }

    fn origin(&self) -> &str {
        &self.origin
    }

    /// How many exchanges the loop made. Consumes the server.
    fn served(mut self) -> usize {
        // A connection of our own unblocks an `accept` still waiting for a
        // request that will never come, so the thread can finish.
        let _ = std::net::TcpStream::connect(self.origin.trim_start_matches("http://"));
        self.thread
            .take()
            .expect("the thread is taken exactly once")
            .join()
            .expect("the server thread did not panic")
    }
}

/// One `generateContent` response carrying tool calls.
///
/// The envelope is `providers/gemini/recorded/calls.json`'s, which is a real
/// response; only the calls differ.
fn answers_with_calls(calls: &[(&str, serde_json::Value)]) -> String {
    let parts: Vec<serde_json::Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (name, arguments))| {
            serde_json::json!({
                "functionCall": {
                    "name": name,
                    "args": arguments,
                    "id": format!("call_{index}")
                }
            })
        })
        .collect();
    serde_json::json!({
        "candidates": [{
            "content": { "parts": parts, "role": "model" },
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {
            "promptTokenCount": 54,
            "candidatesTokenCount": 17,
            "totalTokenCount": 133,
            "thoughtsTokenCount": 62
        },
        "modelVersion": "gemini-3.6-flash"
    })
    .to_string()
}

// --- The scratch home, the project, and the binary --------------------------

struct Home {
    path: PathBuf,
}

impl Home {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-iteration-from-outside-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("project")).expect("a scratch home and project");
        std::fs::create_dir_all(path.join(".zaru")).expect("a scratch configuration directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn project(&self) -> PathBuf {
        self.path.join("project")
    }

    /// Write `~/.zaru/config.toml`, which is ADR-0014 D1's layer 2.
    fn user_config(&self, contents: &str) {
        std::fs::write(self.path.join(".zaru/config.toml"), contents)
            .expect("staging: the user's configuration");
    }

    /// Write `./zaru.toml`, which is ADR-0009 D1's manifest and layer 3.
    fn manifest(&self, contents: &str) {
        std::fs::write(self.project().join("zaru.toml"), contents)
            .expect("staging: the project's manifest");
    }

    /// Every `Record::Loop` line the session's transcript holds, in order.
    ///
    /// **Refuses rather than skipping** when there is no session: a check that
    /// silently found none would assert nothing about a loop and report it as
    /// a pass (library verification lessons §4).
    fn one_session(&self) -> PathBuf {
        let sessions = self.path.join(".zaru/sessions");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&sessions)
            .unwrap_or_else(|error| {
                panic!(
                    "no sessions directory at {}: {error}. A turn that ran creates one",
                    sessions.display()
                )
            })
            .map(|entry| entry.expect("a readable directory entry").path())
            .collect();
        found.sort();
        assert_eq!(found.len(), 1, "one invocation is one turn is one session");
        found.remove(0)
    }

    fn iteration_records(&self) -> Vec<serde_json::Value> {
        let sessions = self.path.join(".zaru/sessions");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&sessions)
            .unwrap_or_else(|error| {
                panic!(
                    "no sessions directory at {}: {error}. A turn that ran creates one",
                    sessions.display()
                )
            })
            .map(|entry| entry.expect("a readable directory entry").path())
            .collect();
        found.sort();
        assert_eq!(found.len(), 1, "one invocation is one turn is one session");
        let transcript = std::fs::read_to_string(found[0].join("transcript.jsonl"))
            .expect("a turn that ran wrote a transcript");
        // `Record` is externally tagged, so a line is one object whose single
        // key names the producer. What is returned is the `Event` inside the
        // `loop` key, which is itself externally tagged the same way.
        transcript
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|record| record.get("loop").cloned())
            .collect()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Ran {
    stdout: String,
    stderr: String,
    code: i32,
}

impl Ran {
    fn everything(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }
}

fn zaru(home: &Home, provider: &Provider, arguments: &[&str]) -> Ran {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(arguments)
        .env_clear()
        .env("HOME", home.path())
        .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
        .env("ZARU_PROVIDER_GEMINI_ENDPOINT", provider.origin())
        .env("ZARU_MODEL_DEFAULT", "gemini-3.6-flash")
        .current_dir(home.project())
        .stdin(Stdio::null())
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

/// Put the nonce in the sealed store through the surface a user uses.
fn store_a_key(home: &Home) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zaru"))
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
        .stdin
        .as_mut()
        .expect("the child's standard input is a pipe")
        .write_all(format!("{NONCE_KEY}\n").as_bytes())
        .expect("the key reaches the child");
    let output = child.wait_with_output().expect("the child exits");
    assert!(
        output.status.success(),
        "staging: the key was not stored: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A manifest declaring one validator that passes only once `marker` exists.
///
/// `test -f` rather than a shell line, because ADR-0009 D1's `run` is a
/// command line and the harness runs no shell.
fn one_validator_wanting(marker: &str) -> String {
    format!(
        "[project]\nname = \"scratch\"\n\n[[validator]]\nname = \"marker\"\nrun = \"test -f \
         {marker}\"\nexpect = \"exit-zero\"\n"
    )
}

/// The name of each event, in order, from its own external tag.
fn event_names(events: &[serde_json::Value]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            event
                .as_object()
                .and_then(|event| event.keys().next().cloned())
        })
        .collect()
}

// --- The checks -------------------------------------------------------------

/// ADR-0009 D4's branch runs, and ADR-0008 D5's exhaustion is exit 1.
///
/// Every previous run of this binary refused a validator-declaring project at
/// [ADR-0016] D5's `4`. It runs one now: two iterations, each generating a
/// candidate the model wrote, applying it through the tool surface, and
/// evaluating the declared validator against it.
///
/// The model writes the **wrong** file both times, so the validator never
/// passes and the loop reaches its ceiling. That is ADR-0008 D5's outcome, and
/// D5 says it "is not an error and is not a success" — so the code is `1`, the
/// expected register, and the line says what was tried.
///
/// **`RefinementConstructed` appears once, not twice.** ADR-0008 D1: the
/// ceiling is checked on the transition out of `Evaluate`, so a run of *n*
/// iterations at a ceiling of *n* emits *n* minus one. That is the assertion
/// that separates this loop from a retry wrapper with a counter.
///
/// Watched red by supplying the inner loop unconditionally, which ran the loop
/// over an empty plan and printed *"a project that declares validators must
/// run the iteration loop and exhaust at 1, and it exited 0"*.
#[test]
fn adr_0009_d4s_branch_runs_the_loop_and_exhaustion_is_exit_one() {
    let home = Home::new("exhausted");
    home.manifest(&one_validator_wanting("marker"));
    // ADR-0014 D1's layer 2 raising ADR-0001 D3's `bare` cell of one, which is
    // D3's own next sentence: "a user who wants more raises it explicitly".
    home.user_config("[runtime]\nmax_iterations = 2\n");
    store_a_key(&home);

    let wrong = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({ "path": "not-the-marker", "contents": "no" }),
    )]);
    let provider = Provider::serving(vec![wrong.clone(), wrong]);
    let ran = zaru(
        &home,
        &provider,
        &["--mode", "yolo", "make the marker exist"],
    );

    assert_eq!(
        ran.code,
        1,
        "a project that declares validators must run the iteration loop and exhaust at 1, and it \
         exited {}: {}",
        ran.code,
        ran.everything()
    );
    assert_eq!(
        provider.served(),
        2,
        "a ceiling of two is two generations, and the loop made a different number of exchanges"
    );

    let records = home.iteration_records();
    let names = event_names(&records);
    assert!(
        names.iter().any(|name| name == "loop_exhausted"),
        "the transcript must carry the loop's own exhaustion, and it carried {names:?}"
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| name.as_str() == "refinement_constructed")
            .count(),
        1,
        "a run of two iterations at a ceiling of two emits one refinement, not two: ADR-0008 D1 \
         checks the ceiling on the way out of Evaluate. The events were {names:?}"
    );
    assert!(
        ran.everything().contains("marker"),
        "ADR-0008 D5 has the harness present what was tried, and the validator's name is the \
         least of it: {}",
        ran.everything()
    );
}

/// The loop refines: a validator that fails once passes after the model's fix.
///
/// This is the accepting sibling of the check above and it is the product
/// claim. The model's **first** candidate writes the wrong file and its second
/// writes the right one, so the validator fails, the failure reaches the
/// refinement, and the next iteration succeeds. Exit 0.
///
/// Without this arm the exhaustion check is satisfied by a loop that can never
/// succeed at all.
///
/// Watched red by making the second answer identical to the first, which
/// printed *"the loop must succeed once the model writes the file the
/// validator wants, and it exited 1"*.
#[test]
fn a_validator_that_fails_once_passes_after_the_models_fix() {
    let home = Home::new("refined");
    home.manifest(&one_validator_wanting("marker"));
    home.user_config("[runtime]\nmax_iterations = 3\n");
    store_a_key(&home);

    let wrong = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({ "path": "not-the-marker", "contents": "no" }),
    )]);
    let right = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({ "path": "marker", "contents": "yes" }),
    )]);
    let provider = Provider::serving(vec![wrong, right]);
    let ran = zaru(
        &home,
        &provider,
        &["--mode", "yolo", "make the marker exist"],
    );

    assert_eq!(
        ran.code,
        0,
        "the loop must succeed once the model writes the file the validator wants, and it exited \
         {}: {}",
        ran.code,
        ran.everything()
    );
    assert!(
        home.project().join("marker").exists(),
        "the succeeding candidate's write must have landed on disk"
    );
    assert_eq!(
        provider.served(),
        2,
        "one failing iteration and one passing one is two exchanges"
    );

    let records = home.iteration_records();
    let names = event_names(&records);
    assert!(
        names.iter().any(|name| name == "refinement_constructed"),
        "a run that failed once must have refined, and its events were {names:?}"
    );
    assert!(
        names.iter().any(|name| name == "loop_succeeded"),
        "a run that ended satisfied must say so on the stream: {names:?}"
    );
}

/// ADR-0008 clause 2 from the binary, and clause 6's port on the same path.
///
/// **Two assertions that must both hold, and they point opposite ways.** The
/// validator prints the harness's own stored key. `RefinementConstructed`'s
/// excerpt is what the model was given, so the key must be **absent** from it;
/// `IterationFailed`'s reason is what ADR-0010 D2's record holds, so the key
/// must be **present** there. One of those alone proves nothing: an
/// implementation that redacted everything passes the first, and one that
/// redacted nothing passes the second.
///
/// The failure text also has to reach the prompt verbatim otherwise, which is
/// clause 2 — so the check asserts a nonce the validator prints beside the key
/// survives into the excerpt.
///
/// Watched red by handing the refinement construction a redactor holding
/// nothing, which printed *"the harness's own key reached the refinement
/// prompt"*.
#[test]
fn a_held_secret_in_a_validators_output_is_redacted_in_the_refinement_and_kept_in_the_record() {
    let home = Home::new("redacted");
    let nonce = "rehearsal-4173";
    // `printf` rather than a shell line: ADR-0009 D1's `run` is a command line
    // and the harness runs no shell. It exits 0, so the validator has to fail
    // some other way -- `test -f` on a file nothing writes does that, and the
    // printing validator runs first and passes, so its output is not the
    // failure. So one validator does both: print, then fail.
    home.manifest(&format!(
        "[project]\nname = \"scratch\"\n\n[[validator]]\nname = \"leaky\"\nrun = \"cat \
         leaked\"\nexpect = {{ exit-code = 9 }}\n"
    ));
    std::fs::write(
        home.project().join("leaked"),
        format!("{nonce} {NONCE_KEY}\n"),
    )
    .expect("staging: the file the validator prints");
    home.user_config("[runtime]\nmax_iterations = 2\n");
    store_a_key(&home);

    let wrong = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({ "path": "not-the-marker", "contents": "no" }),
    )]);
    let provider = Provider::serving(vec![wrong.clone(), wrong]);
    let ran = zaru(&home, &provider, &["--mode", "yolo", "fix it"]);
    assert_eq!(
        ran.code,
        1,
        "the validator never passes: {}",
        ran.everything()
    );

    let records = home.iteration_records();
    let excerpts: String = records
        .iter()
        .filter(|event| event.get("refinement_constructed").is_some())
        .map(std::string::ToString::to_string)
        .collect();
    let reasons: String = records
        .iter()
        .filter(|event| event.get("iteration_failed").is_some())
        .map(std::string::ToString::to_string)
        .collect();

    assert!(
        !excerpts.is_empty() && !reasons.is_empty(),
        "both events must have been emitted, or neither assertion below reads anything"
    );
    assert!(
        excerpts.contains(nonce),
        "ADR-0008 clause 2: the validator's own output reaches the refinement prompt verbatim, \
         and the nonce it printed is not in {excerpts}"
    );
    assert!(
        !excerpts.contains(NONCE_KEY),
        "the harness's own key reached the refinement prompt: {excerpts}"
    );
    assert!(
        reasons.contains(NONCE_KEY),
        "ADR-0010 D2 keeps what the session contained, and the transcript's own record of the \
         failure lost the bytes the command printed: {reasons}"
    );
}

/// A candidate's write outside the tree is decided exactly as a turn's is.
///
/// **For the security corpus, which only grows.** ADR-0011 D4's boundary
/// belongs to the tool surface, and a candidate reaches the tool surface — so
/// nothing here classifies a candidate's path differently from a turn's. Both
/// halves are asserted:
///
/// - At D3's default mode with no terminal to ask at, the write is **refused**
///   and the file does not exist. That is D3's own rule — "a call that needed
///   asking is refused rather than performed" — reaching a candidate.
/// - The transcript marks the call `out_of_tree`, which is D4's record, and
///   it is the same field a turn's call is marked with.
///
/// **The accepting sibling is the same candidate at `yolo`**, where D3 says
/// there are no prompts and the write lands. Without it the refusal is
/// satisfied by a harness in which `fs.write` never works at all, which is the
/// shape that already let one mutant survive in this arc (library verification
/// lessons §13).
///
/// Watched red by giving the tool surface a permission mode of its own rather
/// than the user's — which is what a second executor built for the inner loop
/// would amount to — and it printed *"a candidate wrote outside the working
/// directory, at /tmp/…/outside-the-tree"*.
#[test]
fn a_candidates_write_outside_the_tree_is_decided_like_a_turns() {
    let home = Home::new("out-of-tree");
    home.manifest(&one_validator_wanting("marker"));
    home.user_config("[runtime]\nmax_iterations = 1\n");
    store_a_key(&home);

    let outside = home.path().join("outside-the-tree");
    let asking = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({
            "path": outside.to_string_lossy(),
            "contents": "this must not be written"
        }),
    )]);

    // --- No terminal, default mode: refused --------------------------------
    let provider = Provider::serving(vec![asking.clone()]);
    let refused = zaru(&home, &provider, &["write outside"]);
    assert!(
        !outside.exists(),
        "a candidate wrote outside the working directory, at {}",
        outside.display()
    );
    assert_eq!(
        refused.code,
        1,
        "the validator was never satisfied, so the run is exhaustion: {}",
        refused.everything()
    );
    let marked = std::fs::read_to_string(home.one_session().join("transcript.jsonl"))
        .expect("a turn that ran wrote a transcript");
    assert!(
        marked.contains("\"out_of_tree\":true"),
        "ADR-0011 D4 marks a call that left the tree, and a candidate's call is marked by the \
         same field a turn's is: {marked}"
    );
    assert!(
        marked.contains("\"phase\":\"refused\""),
        "ADR-0011 D4's record closes a refused call as refused, and a candidate's call closes the \
         same way a turn's does: {marked}"
    );

    // --- The accepting sibling, at `yolo` ----------------------------------
    let home = Home::new("out-of-tree-permitted");
    home.manifest(&one_validator_wanting("marker"));
    home.user_config("[runtime]\nmax_iterations = 1\n");
    store_a_key(&home);
    let outside = home.path().join("outside-the-tree");
    let asking = answers_with_calls(&[(
        "fs.write",
        serde_json::json!({
            "path": outside.to_string_lossy(),
            "contents": "permitted by the mode the user chose"
        }),
    )]);
    let provider = Provider::serving(vec![asking]);
    let permitted = zaru(&home, &provider, &["--mode", "yolo", "write outside"]);
    assert_eq!(
        permitted.code,
        1,
        "the validator is still never satisfied: {}",
        permitted.everything()
    );
    assert!(
        outside.exists(),
        "ADR-0011 D3's `yolo` has no prompts, so the same write must land -- without this the \
         refusal above says nothing about the permission model"
    );
}
