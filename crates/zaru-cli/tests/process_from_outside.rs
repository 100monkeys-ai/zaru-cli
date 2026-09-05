// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives both ports over **real child
//! processes**, on a real working directory and a real session.
//!
//! Two records meet here. [ADR-0009] D3's `ValidatorRunner` runs a command a
//! project declared; [ADR-0011] D1's `cmd.run` runs a command a model chose,
//! after that record's permission model has decided. One `Spawn` answers
//! both, and `zaru-core` supplies the dispatch and the loop while knowing
//! nothing about either.
//!
//! Every command here is a real program resolved through `PATH` — `printf`,
//! `false`, `true`, `touch`, `sleep` — and every one of them lives outside
//! the working directory, which is the point of one of the corpus cases
//! below.
//!
//! **Evidence about the mechanism, and it must never be quoted as evidence
//! about the `zaru` binary**, which takes no arguments, prints its version
//! and its composition, exits 0, and reaches none of this.
//!
//! Nothing here opens a socket. The only real effects are child processes, a
//! session directory and a project tree under the system temporary directory,
//! all removed when the check ends.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
};
use zaru_cli::process::{Environment, ProcessCeiling, Spawn};
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction, marker};
use zaru_cli::session::{Phase, Record, SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::{
    Captured, ELISION_PREFIX, Executor, Fetch, FileWrites, Invocation, Mode, NoMembrane,
    OutputBudget, SessionOverflow, ToolName, Verdict, Verdicts, WorkingDirectory,
};
use zaru_core::iteration::validator::{
    Declared, Dispatch, Expect, Name, Pattern, PatternMatch, Plan, Run, SchemaPath, SchemaValidate,
    ValidatorOutput, ValidatorRunner,
};
use zaru_core::iteration::{
    Clock, ContextPolicy, ContextRefusal, ExecutionOutcome, PortFailure, Prompt, Turn,
    ValidatorOutcome, Validators,
};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome, Ports,
    Start, TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

// ------------------------------------------------------------------ scratch

/// A directory the check owns: a project to run in and a session beside it.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!(
                "pr-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(base.join("project").join("src")).expect("staging: the project");
        std::fs::create_dir_all(base.join("sessions")).expect("staging: the session root");
        Self { base }
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }

    fn session(&self) -> zaru_cli::session::Session {
        let store = SessionStore::open(self.base.join("sessions")).expect("staging: the store");
        let id = SessionId::mint(&SystemWallClock).expect("staging: an id");
        store.start(id).expect("staging: the session")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A ceiling generous enough that reaching it means something is wrong.
fn generous() -> ProcessCeiling {
    ProcessCeiling::new(Duration::from_secs(30)).expect("thirty seconds is not zero")
}

/// The five names ADR-0011 D2 gives a child.
fn minimum() -> Environment {
    Environment::inherited_minimum().expect("the harness's own values pass on")
}

/// A value no other call produces, carrying text no implementation invents
/// and a tail no escaping leaves alone.
fn nonce(label: &str) -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(
        "{label}-{}-{nanos}-{seq}-e\u{301}\u{e9}\u{1f701}",
        std::process::id()
    )
}

/// The part of a nonce no formatter can alter.
///
/// [Verification lessons] §50: an absence assertion is blind to whatever the
/// renderer escapes, so both the raw value and this are asserted.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn ascii_core(value: &str) -> &str {
    value
        .strip_suffix("-e\u{301}\u{e9}\u{1f701}")
        .unwrap_or(value)
}

// --------------------------------------------------- the validator dispatch

/// The two `expect` kinds this file deliberately does not declare.
///
/// Staged to panic on purpose, and it stays that way now that both have real
/// implementations in `zaru_cli::validators`: reaching either from HERE would
/// mean the dispatch routed an `exit-zero` or an `exit-code` somewhere it
/// should not have, which is a routing failure rather than an answer.
/// `tests/validators_from_outside.rs` is where the real evaluators run.
struct NoEvaluator;

impl PatternMatch for NoEvaluator {
    async fn matches(&self, _pattern: &Pattern, _stdout: &str) -> Result<bool, PortFailure> {
        panic!(
            "no check in this file declares a `matches` validator, so reaching this is a \
                routing failure"
        )
    }
}

impl SchemaValidate for NoEvaluator {
    async fn validates(&self, _schema: &SchemaPath, _stdout: &str) -> Result<bool, PortFailure> {
        panic!("no check here declares a `json_schema` validator")
    }
}

/// **ADR-0009 clause 1's third verb.** Three declared validators, one
/// depending on another, run as real child processes in declared dependency
/// order.
///
/// The declarations are deliberately **not** in dependency order and the
/// dependent is declared first, so `sort by declaration index` produces a
/// different answer ([Verification lessons] §55: a fixture copied from a
/// record's worked example is already in the answer's shape).
///
/// What each one exercises:
///
/// - `build` runs `true` and passes `exit-zero` — a real exit status, not a
///   staged number.
/// - `shape` runs `printf` and passes `exit-code = 0`, so both of D3's two
///   `std` kinds are decided over a process.
/// - `test` runs `false`, fails `exit-zero`, and its captured bytes are what
///   ADR-0009 D5 sends into refinement.
/// - `lint` comes after `test`, so it is `Skipped` and **its command is never
///   run** — asserted on the filesystem, because a runner that ran it and
///   threw the result away reports identically.
///
/// The resolved order is `build`, `test`, `lint`, `shape`: among validators
/// whose prerequisites are placed, D2's tie-break is the earliest *declared*,
/// and `lint` is declared before `shape`. That is not file-order dependence —
/// `test` is declared first and still runs after `build` — and the two are
/// what this ordering assertion separates.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn a_caller_outside_the_crate_runs_declared_validators_as_real_processes() {
    println!("== declared validators, as real child processes ==");
    let scratch = Scratch::new("validators");
    let working = WorkingDirectory::at(scratch.project()).expect("the project resolves");
    let spawn = Spawn::new(&working, minimum(), generous());

    // The effect a skipped validator must not have.
    let sentinel = scratch.project().join("lint-ran");
    assert!(!sentinel.exists(), "staging: the sentinel already exists");

    let declared = vec![
        // Declared first, runs third: `after` decides and file order does not.
        Declared::new(
            Name::new("test").expect("a name"),
            Run::new("false").expect("a command"),
            Expect::ExitZero,
        )
        .after([Name::new("build").expect("a name")]),
        Declared::new(
            Name::new("lint").expect("a name"),
            Run::new(format!("touch {}", sentinel.display())).expect("a command"),
            Expect::ExitZero,
        )
        .after([Name::new("test").expect("a name")]),
        Declared::new(
            Name::new("build").expect("a name"),
            Run::new("true").expect("a command"),
            Expect::ExitZero,
        ),
        Declared::new(
            Name::new("shape").expect("a name"),
            Run::new("printf 'shape ok'").expect("a command"),
            Expect::ExitCode(0),
        )
        .after([Name::new("build").expect("a name")]),
    ];
    let plan = Plan::from_declared(declared).expect("the prerequisites resolve");
    let order: Vec<&str> = plan.names().map(Name::as_str).collect();
    println!("  dependency order: {order:?}");
    assert_eq!(
        order,
        vec!["build", "test", "lint", "shape"],
        "ADR-0009 D2's order is derived from `after`, and the declarations were given in a \
         different one on purpose"
    );

    let dispatch = Dispatch::new(&plan, &spawn, &NoEvaluator, &NoEvaluator);
    let reports = dispatch
        .evaluate(&ExecutionOutcome {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        })
        .await
        .expect("no port failed");

    for report in &reports {
        println!(
            "  {} -> {:?}{}",
            report.name,
            report.outcome,
            if report.detail.is_empty() {
                String::new()
            } else {
                format!("  detail {:?}", report.detail)
            }
        );
    }

    let outcomes: Vec<(&str, ValidatorOutcome)> = reports
        .iter()
        .map(|report| (report.name.as_str(), report.outcome))
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("build", ValidatorOutcome::Passed),
            ("test", ValidatorOutcome::Failed),
            ("lint", ValidatorOutcome::Skipped),
            ("shape", ValidatorOutcome::Passed),
        ],
        "the four validators did not report what four real commands produced"
    );
    assert!(
        !sentinel.exists(),
        "the skipped validator's command ran anyway: ADR-0009 D2 says a validator whose \
         prerequisite failed does not run, and an implementation that ran it and discarded the \
         result reports identically to one that did not — only the filesystem separates them"
    );
}

/// **ADR-0009 D3's `exit-code` compares against the shell convention**, and a
/// command killed at its ceiling therefore reads as 137.
///
/// The number is not invented here: it is what `$?` gives, so a user running
/// the same command in their own shell sees what the harness reports. Raised
/// on that record because `ValidatorOutput` has one `i32` and no room for the
/// distinction `Ended` keeps.
#[tokio::test]
async fn a_validator_killed_at_its_ceiling_reports_the_shell_conventions_code() {
    let scratch = Scratch::new("ceiling-validator");
    let working = WorkingDirectory::at(scratch.project()).expect("the project resolves");
    let spawn = Spawn::new(
        &working,
        minimum(),
        ProcessCeiling::new(Duration::from_millis(200)).expect("not zero"),
    );

    // Named through the trait, because `Spawn` answers two ports and both
    // spell the method `run`.
    let output: ValidatorOutput =
        ValidatorRunner::run(&spawn, &Run::new("sleep 20").expect("a command"))
            .await
            .expect("the command ran");

    println!(
        "  a killed validator reports exit code {}",
        output.exit_code
    );
    assert_eq!(
        output.exit_code, 137,
        "a validator killed at the ceiling did not report 128 + SIGKILL, which is what a shell \
         would report for the same command"
    );
}

// ------------------------------------------------------------- the tool loop

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// The provider, implemented here because no product tree has one. It keeps
/// every result it was handed, which is what several checks below read.
struct Provider {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    seen: Mutex<Vec<String>>,
}

impl Provider {
    fn scripted(responses: impl IntoIterator<Item = ModelResponse>) -> Self {
        Self {
            script: Mutex::new(responses.into_iter().collect()),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn was_given(&self) -> Vec<String> {
        self.seen.lock().expect("poisoned").clone()
    }
}

impl Model for Provider {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        for result in request.results {
            let content = result.content.as_str().to_owned();
            println!("  the model is given: {content:?}");
            self.seen.lock().expect("poisoned").push(content);
        }
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

#[derive(Default)]
struct Policy;

impl ContextPolicy for Policy {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => format!("[initial] {task}"),
            Turn::Refinement { refinement } => format!("[refinement] {}", refinement.as_str()),
            Turn::Resumed { interrupted } => format!("[resumed] {}", interrupted.call()),
        };
        Ok(Prompt::new(Redacted::by(&HeldSecrets::none(), &rendered)))
    }
}

#[derive(Default)]
struct Printing;

impl EventSink for Printing {
    fn emit(&mut self, event: &Event) {
        println!("  event {event:?}");
    }
}

/// Every port this arc does not implement.
struct Unbuilt;

impl FileWrites for Unbuilt {
    async fn apply(&self, tool: ToolName, _target: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new(format!("{tool} has no implementation")))
    }
}
impl zaru_cli::tools::Search for Unbuilt {
    async fn find(&self, _query: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("fs.search has no implementation"))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(&self, _url: &str) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("web.fetch has no implementation"))
    }
}

struct Nothing;
impl zaru_cli::tools::Allowlist for Nothing {
    fn approves(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}
impl zaru_cli::tools::DestructiveMatch for Nothing {
    fn is_destructive(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}

/// A membrane that refuses, and records what it was asked about.
struct Denying(Mutex<Vec<String>>);

impl Verdicts for Denying {
    fn verdict(&self, invocation: &Invocation<'_>) -> Verdict {
        self.0
            .lock()
            .expect("poisoned")
            .push(invocation.subject_text());
        Verdict::Denied {
            code: String::from("SUBCOMMAND_DENIED"),
            reason: String::from("this check's membrane refuses every command"),
        }
    }
}

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        unreachable!("no check here declares validators for the tool-call loop")
    }
}

/// One turn of the tool-call loop over a real `Spawn`, returning what the
/// model was given and the session's own directory.
struct Ran {
    given: Vec<String>,
    outcome: Outcome,
    directory: std::path::PathBuf,
}

#[expect(
    clippy::too_many_arguments,
    reason = "every argument is a port or a caller-passed bound the executor needs, and \
              collapsing them into a struct here would be a second `Executor` beside the one \
              under test"
)]
async fn one_command_turn(
    scratch: &Scratch,
    command: &str,
    mode: Mode,
    budget: usize,
    redactor: &(dyn Redactor + Sync),
    verdicts: &(dyn Verdicts + Sync),
    ceiling: ProcessCeiling,
    confirmer: Option<&(dyn zaru_cli::tools::Confirm + Sync)>,
) -> Ran {
    let session = scratch.session();
    let directory = session.directory().to_path_buf();
    let working = WorkingDirectory::at(scratch.project()).expect("the project resolves");
    let spawn = Spawn::new(&working, minimum(), ceiling);
    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(&directory);
    let unbuilt = Unbuilt;
    let nothing = Nothing;
    let clock = Ticking::default();
    let policy = Policy;
    let mut sink = Printing;

    let model = Provider::scripted([
        ModelResponse::Calls {
            calls: vec![ToolRequest {
                id: String::from("c1"),
                name: String::from("cmd.run"),
                arguments: serde_json::json!({ "command": command }).to_string(),
            }],
            tokens: TokenUsage {
                prompt: 9,
                completion: 3,
            },
        },
        ModelResponse::Text {
            text: String::from("done"),
            tokens: TokenUsage {
                prompt: 9,
                completion: 2,
            },
        },
    ]);

    let outcome = {
        let mut executor = Executor {
            working_directory: &working,
            mode,
            allowlist: &nothing,
            destructive: &nothing,
            confirmer,
            verdicts,
            budget: OutputBudget::new(budget).expect("a usable budget"),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor,
            writes: &unbuilt,
            search: &unbuilt,
            subprocess: &spawn,
            fetch: &unbuilt,
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("run the command"),
            ToolCallCeiling::new(4).expect("a usable ceiling"),
            ToolCalling::required(&model, "outside-caller").expect("it can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &policy,
                clock: &clock,
                redactor,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("no port failed")
    };

    Ran {
        given: model.was_given(),
        outcome,
        directory,
    }
}

/// **ADR-0011 clause 1's third built-in, and corpus cases 1 and 2.**
///
/// A model asks for a command, the harness runs it as a child process at the
/// boundary's root, and the bytes come back — with the transcript written
/// around the act.
///
/// **Corpus case 1: a command line is never classified as a path.** The
/// command is rendered into the transcript exactly as it was written, and
/// carries no out-of-tree marking, because a command's boundary is the
/// working directory it starts in rather than where its text would resolve.
/// Until 2026-09-05 this string was measured against ADR-0011 D4 as though it
/// were a filename.
///
/// **Corpus case 2: the program lives outside the tree and runs.** `printf`
/// is `/usr/bin/printf` on every machine this has run on — outside every
/// project root — and ADR-0011 D1 names no allowlist of programs, so
/// `cmd.run` executes what `PATH` finds. Pinned here so that adding an
/// allowlist later is a visible act.
#[tokio::test]
async fn a_model_runs_a_command_and_its_output_comes_back() {
    println!("== a model runs a command ==");
    let scratch = Scratch::new("command");
    let marker_text = nonce("stdout-marker");

    let ran = one_command_turn(
        &scratch,
        &format!("printf '{marker_text}'"),
        Mode::Yolo,
        4096,
        &HeldSecrets::none(),
        &NoMembrane,
        generous(),
        None,
    )
    .await;

    assert!(
        matches!(ran.outcome, Outcome::Answered { .. }),
        "the turn did not end with the model answering: {:?}",
        ran.outcome
    );
    assert_eq!(ran.given.len(), 1, "the model was given {:?}", ran.given);
    let given = &ran.given[0];
    assert!(
        given.contains(&marker_text),
        "the child's standard output did not reach the model: {given:?}"
    );
    assert!(
        given.contains("exit code: 0"),
        "the work's own exit code did not reach the model: {given:?}"
    );

    let restored = zaru_cli::session::resume(&ran.directory, 32).expect("the session resumes");
    println!("  the transcript holds:");
    for record in &restored.tail {
        println!("      {record:?}");
    }
    let calls: Vec<&zaru_cli::session::ToolCall> = restored
        .tail
        .iter()
        .filter_map(|record| match record {
            Record::ToolCall(call) => Some(call),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls.iter().map(|call| call.phase).collect::<Vec<_>>(),
        vec![Phase::Started, Phase::Completed],
        "ADR-0010 D2 and ADR-0011 D4: the call owes a pair around the act"
    );
    let line = &calls[0].line;
    assert_eq!(
        line,
        &format!("cmd.run printf {marker_text}"),
        "the transcript does not render the command as it was written; ADR-0010's \"a rendered \
         `cmd.run` line **is** a command line\" is what this asserts"
    );
    assert!(
        !line.contains("OUTSIDE the working directory"),
        "a command line was marked as having left the working directory. A command has no \
         placement against D4's boundary at all — its boundary is the directory it starts in — \
         and marking one was the accident corrected on 2026-09-05: {line:?}"
    );
    assert!(
        restored.interrupted.is_none(),
        "the call completed, so nothing was in flight"
    );
}

/// **Corpus case 3 — a secret in a command line.**
///
/// A bearer the harness itself holds, passed as an *argument*, is absent from
/// what the model is given and **present** in the session's own files. That
/// is ADR-0008 clause 6's port on a path no file's contents travel: the value
/// is in the command line rather than in a capture, and it reaches the model
/// through the rendered output.
///
/// Asserted on the raw value **and** on an ASCII core no escaping can alter
/// ([Verification lessons] §50), with the discriminating arm below.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[tokio::test]
async fn a_secret_in_a_command_line_is_redacted_for_the_model_and_kept_in_the_session() {
    println!("== a planted bearer in a command line ==");
    let scratch = Scratch::new("secret");
    let value = format!("nn_mcp_{}", nonce("bearer"));
    let (store, keys, alias) = store_holding(&scratch, "planted", &value);
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "the store held nothing to redact");

    let command = format!("printf '%s' '{value}'");
    let ran = one_command_turn(
        &scratch,
        &command,
        Mode::Yolo,
        4096,
        &held,
        &NoMembrane,
        generous(),
        None,
    )
    .await;

    let given = ran.given.join("\n");
    assert!(
        !given.contains(&value) && !given.contains(ascii_core(&value)),
        "the planted bearer reached the model: {given:?}"
    );
    assert!(
        given.contains(&marker(&alias)),
        "the model was given no marker where the value was removed, so it cannot tell that \
         anything was: {given:?}"
    );

    // The record keeps what the record is for. Read with `std::fs` rather
    // than through `Transcript::read`, so neither arm of the comparison
    // travels through the product's own reader.
    let transcript = std::fs::read_to_string(ran.directory.join("transcript.jsonl"))
        .expect("the transcript is on disk");
    assert!(
        transcript.contains(&value),
        "the transcript does not carry the command as it was run. ADR-0010's record is \
         deliberately outside the port: it contains whatever the session contained"
    );

    // The discriminating arm: with nothing held, the same run carries the
    // value through byte for byte. Without this a redactor that erased
    // everything would pass every assertion above.
    let control = Scratch::new("secret-control");
    let carried = one_command_turn(
        &control,
        &command,
        Mode::Yolo,
        4096,
        &HeldSecrets::none(),
        &NoMembrane,
        generous(),
        None,
    )
    .await;
    assert!(
        carried.given.join("\n").contains(&value),
        "with nothing held, the value did not reach the model either — so the absence above is \
         about the check rather than about the redactor"
    );
}

/// **Corpus case 4 — a command's output over the budget.**
///
/// Truncated head and tail with the elision marked, the whole capture written
/// into the session directory, and the path shown to the model. The command
/// is a real one writing far more than the budget.
#[tokio::test]
async fn a_commands_output_over_the_budget_is_elided_and_the_whole_is_preserved() {
    println!("== a command that writes more than the budget ==");
    const WRITTEN: usize = 4_000;
    let scratch = Scratch::new("budget");

    let ran = one_command_turn(
        &scratch,
        &format!("printf %0{WRITTEN}d 7"),
        Mode::Yolo,
        256,
        &HeldSecrets::none(),
        &NoMembrane,
        generous(),
        None,
    )
    .await;

    let given = ran.given.join("\n");
    assert!(
        given.contains(ELISION_PREFIX),
        "output over the budget reached the model unmarked: {given:?}"
    );
    assert!(
        given.len() < WRITTEN,
        "nothing was elided: the model was given {} bytes of a {WRITTEN}-byte capture",
        given.len()
    );
    let preserved: Vec<std::path::PathBuf> = std::fs::read_dir(&ran.directory)
        .expect("the session directory reads")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("output-"))
        })
        .collect();
    assert_eq!(
        preserved.len(),
        1,
        "ADR-0011 D5 writes the full text to the session directory; found {preserved:?}"
    );
    let whole = std::fs::read_to_string(&preserved[0]).expect("the preserved capture reads");
    assert!(
        whole.contains(&"0".repeat(WRITTEN - 1)),
        "the preserved file does not hold the whole output, so the truncation the user cannot \
         notice is exactly what happened"
    );
    assert!(
        given.contains(
            preserved[0]
                .to_str()
                .expect("the path is representable as text")
        ),
        "D5 requires the path be shown, and it is not in what the model was given: {given:?}"
    );
}

/// **A denied verdict stops a command before it runs**, and the effect is
/// asserted on the filesystem rather than on a return value.
///
/// ADR-0004 D2 and ADR-0011 D3: at `contained` and above the membrane
/// decides, and the mode governs prompting only. `Verdicts::verdict` takes no
/// `Mode` and the executor takes no `Tier`, so the verdict is asked for at
/// every mode and a denial refuses at every one — which is stronger than the
/// clause and is what can be shown today.
///
/// The accepting sibling runs the same command through `NoMembrane` and finds
/// the file, without which an assertion of absence is satisfied by a `Spawn`
/// that never runs anything.
#[tokio::test]
async fn a_denied_verdict_stops_a_command_before_the_child_is_started() {
    println!("== a membrane that refuses ==");
    let scratch = Scratch::new("denied");
    let sentinel = scratch.project().join("the-child-ran");
    let command = format!("touch {}", sentinel.display());
    let denying = Denying(Mutex::new(Vec::new()));

    let ran = one_command_turn(
        &scratch,
        &command,
        Mode::Yolo,
        4096,
        &HeldSecrets::none(),
        &denying,
        generous(),
        None,
    )
    .await;

    assert!(
        !sentinel.exists(),
        "a denied command started a child anyway: the file it would create exists"
    );
    assert_eq!(
        denying.0.lock().expect("poisoned").len(),
        1,
        "the membrane was not asked about the call at all"
    );
    assert_eq!(
        denying.0.lock().expect("poisoned")[0],
        format!("touch {}", sentinel.display()),
        "the membrane was asked about something other than the command line"
    );
    assert!(
        ran.given.join("\n").contains("SUBCOMMAND_DENIED"),
        "the denial's own reason did not become the model's next content: {:?}",
        ran.given
    );

    // The accepting sibling.
    let allowed = Scratch::new("denied-control");
    let control_sentinel = allowed.project().join("the-child-ran");
    let _ = one_command_turn(
        &allowed,
        &format!("touch {}", control_sentinel.display()),
        Mode::Yolo,
        4096,
        &HeldSecrets::none(),
        &NoMembrane,
        generous(),
        None,
    )
    .await;
    assert!(
        control_sentinel.exists(),
        "the same command ran with no membrane and still created nothing, so the absence above \
         says nothing about the verdict"
    );
}

/// A shell construct is told to the model, names itself, and writes no
/// transcript record.
///
/// It is a `NotACall`: a string carrying a pipe is not a call this surface
/// can make, so there is nothing to permit or deny, and nothing was
/// attempted. The record assertion is the load-bearing one — a bare
/// `Phase::Started` here would make a resumed session report a refusal as an
/// interruption.
#[tokio::test]
async fn a_shell_construct_is_told_to_the_model_and_writes_no_record() {
    println!("== a command carrying a pipe ==");
    let scratch = Scratch::new("shell");

    let ran = one_command_turn(
        &scratch,
        "printf hello | tee log",
        Mode::Yolo,
        4096,
        &HeldSecrets::none(),
        &NoMembrane,
        generous(),
        None,
    )
    .await;

    let given = ran.given.join("\n");
    assert!(
        given.contains("\"|\"") && given.contains("no shell"),
        "the model was not told which construct was refused or why: {given:?}"
    );

    let restored = zaru_cli::session::resume(&ran.directory, 32).expect("the session resumes");
    let calls = restored
        .tail
        .iter()
        .filter(|record| matches!(record, Record::ToolCall(_)))
        .count();
    assert_eq!(
        calls, 0,
        "a string that is not a call wrote a transcript record; nothing was attempted, and a \
         bare `Started` would make a resumed session report this as interrupted"
    );
    assert!(
        restored.interrupted.is_none(),
        "a refused command line was derived as an interrupted call"
    );
}

// ------------------------------------------------------- credential staging

/// The sealing key, supplied from outside.
///
/// **This says nothing about where a real key comes from.** ADR-0007 D3 reads
/// it from the OS keyring or from `ZARU_CREDENTIAL_KEY`, and a check that
/// reached either would be a check that changes state it does not own. What a
/// check may conclude from this is that the store seals under the key it is
/// given and opens under the same one; the encryption itself is real, and is
/// checked from outside in `tests/sealing_from_outside.rs`.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A store on the scratch root holding one bearer under one alias.
fn store_holding(
    scratch: &Scratch,
    alias: &str,
    value: &str,
) -> (CredentialStore, StagedKey, Alias) {
    let keys = StagedKey(SealingKey::mint());
    let mut store =
        CredentialStore::open(scratch.base.join("zaru")).expect("the credential store opens");
    let alias = Alias::new(alias).expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias.clone(),
        Description::new("the bearer this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::new(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    (store, keys, alias)
}

// ------------------------------------------------ a call interrupted for real

/// Where the parent tells the re-invoked child to write its session.
const CHILD_SESSION: &str = "PR_CHILD_SESSION";

/// Where the parent tells the re-invoked child to root its working directory.
const CHILD_PROJECT: &str = "PR_CHILD_PROJECT";

/// The command the child leaves in flight.
const IN_FLIGHT: &str = "sleep 30";

/// How long the parent will wait for the child to get a record onto disk.
const RECORD_DEADLINE: Duration = Duration::from_secs(20);

/// **ADR-0010 clause 3 — a tool call interrupted deliberately.**
///
/// The clause asks that resume restore context and re-render the tail
/// "without re-executing any tool call, asserted with a tool call interrupted
/// deliberately". Until a `cmd.run` could actually run, the only way to stage
/// that was to write the transcript a killed process would have left, which
/// asserts the derivation and not the interruption.
///
/// Here a real child of this crate's own test binary starts a real `cmd.run`
/// that will not finish, and is killed with `SIGKILL` while the command is
/// still running — so the `Started` with nothing closing it is one a killed
/// process genuinely left behind.
///
/// **The wait is on the condition, never on a clock** ([Verification lessons]
/// §20): the parent kills only once the record is on disk, and refuses rather
/// than proceeding if it never arrives, because killing a writer that has not
/// written says nothing about what a kill costs.
///
/// A newline is printed before the spawn so that this harness's own progress
/// framing cannot be glued to the child's first line (§60).
///
/// The `sleep` the child left running is orphaned by the kill and exits on its
/// own; nothing here contains a grandchild, which is what ADR-0011 D2 says
/// out loud about `bare`.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_command_in_flight_when_the_harness_dies_resumes_as_interrupted() {
    println!("== a tool call interrupted deliberately ==");
    let scratch = Scratch::new("interrupted");
    let session = scratch.session();
    let directory = session.directory().to_path_buf();
    let transcript = directory.join(zaru_cli::session::TRANSCRIPT_FILE);

    println!();
    let mut child = std::process::Command::new(
        std::env::current_exe().expect("the test binary knows where it is"),
    )
    .args([
        "--exact",
        "the_interruption_checks_child_leaves_a_command_in_flight",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(CHILD_SESSION, &directory)
    .env(CHILD_PROJECT, scratch.project())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .expect("could not spawn this crate's own test binary");

    let started = std::time::Instant::now();
    loop {
        let held = std::fs::read_to_string(&transcript).unwrap_or_default();
        if held.lines().any(|line| line.contains("cmd.run")) {
            break;
        }
        assert!(
            started.elapsed() < RECORD_DEADLINE,
            "the child never got a `cmd.run` record onto disk in {RECORD_DEADLINE:?}. This check \
             kills a harness with a command in flight, so a child that never started one cannot \
             say anything about what the kill leaves behind. The transcript held {} byte(s)",
            held.len(),
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    child.kill().expect("could not kill the child");
    let _ = child.wait();

    let restored = zaru_cli::session::resume(&directory, 32).expect("the session resumes");
    println!("  the transcript holds:");
    for record in &restored.tail {
        println!("      {record:?}");
    }

    let interrupted = restored
        .interrupted
        .expect("a `cmd.run` was in flight when the process was killed, so ADR-0010 D4 derives an interruption");
    assert_eq!(
        interrupted.call.phase,
        Phase::Started,
        "an interruption is derived from a `Started` with nothing closing it"
    );
    assert_eq!(
        interrupted.call.line,
        format!("cmd.run {IN_FLIGHT}"),
        "the interrupted call is not the command that was in flight"
    );
    assert_eq!(
        restored
            .tail
            .iter()
            .filter(|record| matches!(record, Record::ToolCall(_)))
            .count(),
        1,
        "the killed harness left more than the one record it had written"
    );

    // What the model is told, which is ADR-0010 D4's second half and passes
    // ADR-0008 clause 6's port because a rendered `cmd.run` line is a command
    // line.
    let told = interrupted.for_the_model(&HeldSecrets::none());
    println!("  the model would be told: {}", told.call());
    assert!(
        told.call().contains(IN_FLIGHT),
        "the model is not told which call did not complete: {}",
        told.call()
    );
}

/// The child half of the check above. Never run on its own.
///
/// It starts a command that will not finish and then waits to be killed. It
/// never returns, which is the point.
#[tokio::test]
#[ignore = "re-invoked by `a_command_in_flight_when_the_harness_dies_resumes_as_interrupted`"]
async fn the_interruption_checks_child_leaves_a_command_in_flight() {
    let directory = std::path::PathBuf::from(
        std::env::var(CHILD_SESSION)
            .unwrap_or_else(|_| panic!("{CHILD_SESSION} names the session this child writes")),
    );
    let project = std::path::PathBuf::from(
        std::env::var(CHILD_PROJECT)
            .unwrap_or_else(|_| panic!("{CHILD_PROJECT} names the working directory")),
    );

    let working = WorkingDirectory::at(&project).expect("the working directory resolves");
    // Longer than the parent will ever wait, so the command is still running
    // when the kill lands rather than having ended on a ceiling of its own.
    let spawn = Spawn::new(
        &working,
        minimum(),
        ProcessCeiling::new(Duration::from_secs(600)).expect("not zero"),
    );
    let mut transcript = Transcript::append_to(directory.join(zaru_cli::session::TRANSCRIPT_FILE))
        .expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(&directory);
    let unbuilt = Unbuilt;
    let nothing = Nothing;
    let clock = Ticking::default();
    let policy = Policy;
    let mut sink = Printing;
    let model = Provider::scripted([ModelResponse::Calls {
        calls: vec![ToolRequest {
            id: String::from("c1"),
            name: String::from("cmd.run"),
            arguments: serde_json::json!({ "command": IN_FLIGHT }).to_string(),
        }],
        tokens: TokenUsage {
            prompt: 1,
            completion: 1,
        },
    }]);

    let mut executor = Executor {
        working_directory: &working,
        mode: Mode::Yolo,
        allowlist: &nothing,
        destructive: &nothing,
        confirmer: None,
        verdicts: &NoMembrane,
        budget: OutputBudget::new(4096).expect("a usable budget"),
        overflow: &mut overflow,
        transcript: &mut transcript,
        redactor: &HeldSecrets::none(),
        writes: &unbuilt,
        search: &unbuilt,
        subprocess: &spawn,
        fetch: &unbuilt,
    };
    let _ = run::<_, _, _, _, _, NeverIterates>(
        1,
        Start::Task("leave a command in flight"),
        ToolCallCeiling::new(4).expect("a usable ceiling"),
        ToolCalling::required(&model, "interruption-child").expect("it can call tools"),
        Ports {
            model: &model,
            tools: &mut executor,
            context: &policy,
            clock: &clock,
            redactor: &HeldSecrets::none(),
        },
        None,
        &mut [&mut sink],
    )
    .await;
    unreachable!("the parent kills this child while the command is still running");
}
