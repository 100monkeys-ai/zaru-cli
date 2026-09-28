// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a model is sent on the turns after one that called a tool, and after
//! a session is resumed.
//!
//! # The defect these checks were written against
//!
//! Until 2026-09-28 the next turn's request carried the earlier turns as one
//! `user` message holding what the pane had shown: "`fs.read … — permitted`",
//! "`cmd.run reported a failure · 1256 bytes`", the token line, the notice
//! about validators. The calls' arguments and results were not in it. So a
//! model could not see what it had read or run one turn earlier, and asked
//! what a test run printed, a real model searched for seven exchanges and made
//! part of the answer up. The first check here was watched red on that tree:
//! *"turn 2's request does not carry what turn 1's fs.read returned, so the
//! model has forgotten its own tool result one turn later"*.
//!
//! # How the turns are driven
//!
//! Through the product's own pieces: the tool-call loop, the real executor on
//! a scratch working directory, the session's own context and policy, the
//! transcript writer, and the turn boundary that rebuilds layer 6 from the
//! transcript. Only the model is a double, and it is a recording one: no check
//! may call a provider or stand a fake one up at the wire. What each provider
//! would put on the wire is its own `request_from`, called on the very request
//! the loop handed the model.

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::compose::{ContextShape, Facts, Records, SessionContext, boundary};
use zaru_cli::process::CommandLine;
use zaru_cli::redaction::HeldSecrets;
use zaru_cli::session::{Record, SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::{
    Captured, Executor, Fetch, Mode, NoMembrane, OutputBudget, SessionOverflow, Subprocess,
    WorkingDirectory,
};
use zaru_core::context::{ContextLimits, ContextWindow, PressureThreshold};
use zaru_core::conversation::Message;
use zaru_core::iteration::{Clock, PortFailure, Prompt};
use zaru_core::tool_call::{
    Capabilities, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome, Ports, Start,
    TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

/// What the file the model reads holds. Unique, so finding it is finding it.
const BODY: &str = "INVENTORY-FILE-BODY-7f3a: def remove(self, name, qty): pass\n";

/// Lines the pane shows for a call, which no request may carry.
const NARRATION: [&str; 6] = [
    "— permitted",
    " returned · ",
    "exchange 1, call 1",
    "tokens: ",
    "no validators are declared",
    "reported a failure",
];

// ------------------------------------------------------------ the scratch

struct Scratch {
    base: std::path::PathBuf,
    session: zaru_cli::session::Session,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("resolves")
            .join(format!(
                "{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(base.join("project/pkg")).expect("staging");
        std::fs::write(base.join("project/pkg/inventory.py"), BODY).expect("staging");
        std::fs::create_dir_all(base.join("sessions")).expect("staging");
        let store = SessionStore::open(base.join("sessions")).expect("staging: the store");
        let session = store
            .start(SessionId::mint(&SystemWallClock).expect("an id"))
            .expect("staging: the session");
        Self { base, session }
    }

    fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// ------------------------------------------------------------ the doubles

#[derive(Debug, Default)]
struct Still;
impl Clock for Still {
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

struct Unbuilt;
impl Subprocess for Unbuilt {
    async fn run(&self, _line: &CommandLine) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("cmd.run has no implementation here"))
    }
}
impl Fetch for Unbuilt {
    async fn retrieve(
        &self,
        _url: &zaru_cli::web::RequestedUrl,
        _followed: usize,
    ) -> Result<zaru_cli::tools::Retrieved, PortFailure> {
        Err(PortFailure::new("web.fetch has no implementation here"))
    }
}

struct Nothing;
impl zaru_cli::tools::Allowlist for Nothing {
    fn approves(&self, _invocation: &zaru_cli::tools::Invocation<'_>) -> bool {
        false
    }
}
impl zaru_cli::tools::DestructiveMatch for Nothing {
    fn is_destructive(&self, _invocation: &zaru_cli::tools::Invocation<'_>) -> bool {
        false
    }
}

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        Err(PortFailure::new("no inner loop here"))
    }
}

/// One request, as the model was handed it and as each provider would send it.
#[derive(Debug, Clone)]
struct Sent {
    system: Option<String>,
    history: Vec<Message>,
    task: String,
    turn: Vec<Message>,
    ollama: String,
    openai: String,
    gemini: String,
}

/// Answers from a script and keeps every request, in every provider's shape.
struct Scripted {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    sent: Mutex<Vec<Sent>>,
}

impl Scripted {
    fn answering(script: impl IntoIterator<Item = ModelResponse>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            sent: Mutex::new(Vec::new()),
        }
    }

    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().expect("poisoned").clone()
    }
}

impl Model for Scripted {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        let ollama = zaru_cli::providers::ollama::map::request_from(request, "a-model", 4096)
            .expect("ollama maps the request");
        let openai = zaru_cli::providers::openai_compatible::map::request_from(request, "a-model")
            .expect("openai-compatible maps the request");
        let gemini =
            zaru_cli::providers::gemini::map::request_from(request).expect("gemini maps it");
        self.sent.lock().expect("poisoned").push(Sent {
            system: request.prompt.system().map(str::to_owned),
            history: request.prompt.history().to_vec(),
            task: request.prompt.task().to_owned(),
            turn: request.turn.to_vec(),
            ollama: serde_json::to_string(&ollama).expect("serialises"),
            openai: serde_json::to_string(&openai).expect("serialises"),
            gemini: serde_json::to_string(&gemini).expect("serialises"),
        });
        self.script
            .lock()
            .expect("poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

fn calls(id: &str, name: &str, arguments: &str) -> ModelResponse {
    ModelResponse::Calls {
        calls: vec![ToolRequest {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        }],
        text: String::new(),
        echo: None,
        tokens: TokenUsage::default(),
    }
}

fn answer(text: &str) -> ModelResponse {
    ModelResponse::Text {
        text: text.to_owned(),
        echo: None,
        tokens: TokenUsage::default(),
    }
}

// ------------------------------------------------------------ one turn

/// The facts a check's layer 1 is built from: fixed, so a prompt a check
/// compares is the same on every machine and every day.
fn facts() -> Facts {
    Facts {
        directory: Some("/work".to_owned()),
        system: "linux".to_owned(),
        date: "2026-09-28".to_owned(),
        tools: vec!["fs.read".to_owned()],
        mode: None,
    }
}

fn shape() -> ContextShape {
    ContextShape::of(
        ContextLimits::new(
            ContextWindow::new(1_000_000).expect("a window"),
            PressureThreshold::new(750_000).expect("a threshold"),
        )
        .expect("limits"),
        0,
        one_token_a_byte(),
    )
}

/// Run one turn of `session` the way `compose::turn` does: the person's
/// message recorded, the loop over the product's executor and the session's
/// own policy, then layer 6 rebuilt from the transcript and checkpointed.
async fn a_turn(
    scratch: &Scratch,
    context: &mut SessionContext,
    model: &Scripted,
    held: &HeldSecrets,
    budget: usize,
    n: u32,
    task: &str,
) -> Outcome {
    a_turn_within(
        scratch,
        context,
        model,
        held,
        budget,
        n,
        task,
        ToolCallCeiling::unlimited(),
    )
    .await
}

/// [`a_turn`], under the exchange limit `ceiling`.
#[allow(clippy::too_many_arguments)]
async fn a_turn_within(
    scratch: &Scratch,
    context: &mut SessionContext,
    model: &Scripted,
    held: &HeldSecrets,
    budget: usize,
    n: u32,
    task: &str,
    ceiling: ToolCallCeiling,
) -> Outcome {
    let working = WorkingDirectory::at(scratch.project()).expect("resolves");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    transcript
        .record(&boundary::spoken_by_the_user(held, n, task))
        .expect("the person's message is recorded");
    let mut overflow = SessionOverflow::in_session(scratch.session.directory());
    let no_grants = zaru_cli::tools::grants::SessionGrants::none();
    let mut records = Records::appending_to(scratch.session.transcript_path()).expect("opens");
    let outcome = {
        let mut executor = Executor {
            working_directory: &working,
            mode: Mode::Yolo,
            allowlist: &Nothing,
            destructive: &Nothing,
            session_grants: &no_grants,
            confirmer: None,
            verdicts: &NoMembrane,
            budget: OutputBudget::new(budget).expect("a budget"),
            preview_budget: OutputBudget::new(budget).expect("a budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: held,
            subprocess: &Unbuilt,
            fetch: &Unbuilt,
            projected: &zaru_cli::tools::NoProjection,
            declared: zaru_cli::tools::descriptor_set(),
        };
        let policy = context.policy(held, false);
        let mut sinks: [&mut dyn EventSink; 1] = [&mut records];
        run::<_, _, _, _, _, NeverIterates>(
            n,
            Start::Task(task),
            ceiling,
            ToolCalling::required(model, "scripted").expect("it can"),
            Ports {
                model,
                tools: &mut executor,
                context: &policy,
                clock: &Still,
                redactor: held,
            },
            None,
            &mut sinks,
        )
        .await
        .expect("no port failed")
    };
    assert!(
        records.first_failure().is_none(),
        "the transcript refused a record"
    );
    boundary::rebuilt_from_the_transcript(context, &scratch.session)
        .expect("the transcript reads back");
    boundary::checkpointed(context, &scratch.session).expect("the checkpoint is written");
    outcome
}

/// Every narration line any of a request's three wire bodies carries.
fn narration_in(sent: &Sent) -> Vec<String> {
    let mut found = Vec::new();
    for (provider, body) in [
        ("ollama", &sent.ollama),
        ("openai-compatible", &sent.openai),
        ("gemini", &sent.gemini),
    ] {
        for line in NARRATION {
            if body.contains(line) {
                found.push(format!("{provider}: {line:?}"));
            }
        }
    }
    found
}

// ------------------------------------------------------------ the checks

/// A model that asks for the same tool for ever is stopped at the default
/// exchange limit, and "continue" goes on with everything the stopped turn
/// did.
///
/// The harness survey of 2026-09-28 measured a scripted model repeating one
/// `fs.list` 123 times in about two seconds, stopped only by the context
/// guard. The limit is the product's own: layer 1 resolved through
/// `runtime::tool_call_ceiling_for`, as `compose::turn::prepare` resolves it.
///
/// Red on `254e2b6`, where that resolved to no limit: "a model that asked for
/// a tool on every exchange was not stopped at the default limit of 50; the
/// turn ended Answered".
#[tokio::test]
async fn a_looping_model_stops_at_the_default_limit_and_continue_goes_on() {
    use zaru_cli::config::{Contribution, Layer, LayerSource as _, Resolution};

    let resolution = Resolution::resolve(
        &zaru_cli::cli::layers::schema(),
        vec![Contribution::new(
            Layer::BuiltIn,
            Layer::BuiltIn.default_source(),
            zaru_cli::cli::layers::BuiltIn::new()
                .read()
                .expect("layer 1 reads"),
        )],
    )
    .expect("layer 1 resolves");
    let ceiling = zaru_cli::runtime::tool_call_ceiling_for(&resolution).expect("a limit");
    let limit = zaru_cli::runtime::DEFAULT_TOOL_EXCHANGES;

    let scratch = Scratch::new("loop-limit");
    let held = HeldSecrets::none();
    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    // One call more than the limit, then an answer: without a limit the loop
    // would use every call and answer in the same turn.
    let mut script: Vec<ModelResponse> = (1..=limit + 1)
        .map(|n| calls(&format!("c{n}"), "fs.list", r#"{"path":"."}"#))
        .collect();
    script.push(answer(
        "I listed the directory and stopped repeating myself.",
    ));
    let model = Scripted::answering(script);

    let first = a_turn_within(
        &scratch,
        &mut context,
        &model,
        &held,
        4_096,
        1,
        "list the files",
        ceiling,
    )
    .await;
    let Outcome::Exhausted { rounds, calls, .. } = first else {
        panic!(
            "a model that asked for a tool on every exchange was not stopped at the default \
             limit of {limit}; the turn ended {first:?}"
        );
    };
    assert_eq!((rounds, calls), (limit, limit));

    let second = a_turn_within(
        &scratch,
        &mut context,
        &model,
        &held,
        4_096,
        2,
        "continue",
        ceiling,
    )
    .await;
    let sent = model.sent();
    let continuing = sent
        .get(usize::try_from(limit).expect("fits"))
        .expect("turn 2 asked");
    assert_eq!(continuing.task, "continue");
    let last_result = continuing
        .history
        .iter()
        .any(|message| matches!(message, Message::Tool { id, .. } if *id == format!("c{limit}")));
    assert!(
        last_result,
        "continue was not sent the last result of the turn that stopped at its limit, so the \
         model cannot go on from where it stopped"
    );
    assert!(
        matches!(second, Outcome::Answered { rounds: 2, .. }),
        "continue did not go on to an answer: {second:?}"
    );
}

/// Turn 2 is sent turn 1's tool call with its arguments and its result with
/// its content, in each provider's own shape, and no line of what the pane
/// showed.
///
/// Watched red on `2b5b871`, as this module's documentation quotes. The
/// mutant since: not rebuilding layer 6 at the turn boundary, which reddens
/// the history arm.
#[tokio::test]
async fn turn_two_is_sent_turn_ones_call_and_result_in_every_providers_own_shape() {
    let scratch = Scratch::new("turn-memory-two");
    let held = HeldSecrets::none();
    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    let model = Scripted::answering([
        calls("call_1", "fs.read", r#"{"path":"pkg/inventory.py"}"#),
        answer("I read it."),
        answer("It defines remove."),
    ]);

    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        1,
        "read pkg/inventory.py",
    )
    .await;
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        2,
        "what did the file say",
    )
    .await;

    let sent = model.sent();
    assert_eq!(sent.len(), 3, "three requests were made");
    let two = &sent[2];

    // The model's view: every earlier message, in order, as it was.
    let shape: Vec<String> = two
        .history
        .iter()
        .map(|message| match message {
            Message::User { text } => format!("user {text}"),
            Message::Assistant { text, calls, .. } => format!(
                "assistant {text}{}",
                calls
                    .iter()
                    .map(|call| format!("[{} {} {}]", call.id, call.name, call.arguments))
                    .collect::<String>()
            ),
            Message::Tool {
                id, name, content, ..
            } => format!(
                "tool {id} {name} {}",
                content.contains("INVENTORY-FILE-BODY-7f3a")
            ),
        })
        .collect();
    assert_eq!(
        shape,
        [
            "user read pkg/inventory.py",
            r#"assistant [call_1 fs.read {"path":"pkg/inventory.py"}]"#,
            "tool call_1 fs.read true",
            "assistant I read it.",
        ],
        "turn 2's request does not carry turn 1 as the messages it was: the task, the call with \
         its arguments, the result with the file's content, and the answer"
    );
    assert_eq!(two.task, "what did the file say");
    assert!(
        two.turn.is_empty(),
        "turn 2's first request has no turn of its own yet"
    );

    // The system text is in the system role and in no message.
    let system = two
        .system
        .as_deref()
        .expect("the harness sends a system prompt");
    assert!(
        system.contains("Working directory: /work"),
        "the system text is not the harness's system prompt: {system}"
    );
    assert!(
        !two.history
            .iter()
            .any(|message| message.rendered().contains("Working directory")),
        "the system prompt reached a message as if someone had said it"
    );

    // Each provider's own shape.
    let ollama: serde_json::Value = serde_json::from_str(&two.ollama).expect("json");
    assert_eq!(ollama["messages"][0]["role"], "system");
    assert_eq!(
        ollama["messages"][2]["tool_calls"][0]["function"]["arguments"]["path"],
        "pkg/inventory.py"
    );
    assert_eq!(ollama["messages"][3]["role"], "tool");
    assert_eq!(ollama["messages"][3]["tool_name"], "fs.read");
    assert!(
        ollama["messages"][3]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("INVENTORY-FILE-BODY-7f3a"),
        "ollama: the result's content is not on the wire: {ollama}"
    );

    let openai: serde_json::Value = serde_json::from_str(&two.openai).expect("json");
    assert_eq!(openai["messages"][0]["role"], "system");
    assert_eq!(openai["messages"][2]["tool_calls"][0]["id"], "call_1");
    assert_eq!(
        openai["messages"][2]["tool_calls"][0]["function"]["arguments"],
        r#"{"path":"pkg/inventory.py"}"#
    );
    assert_eq!(openai["messages"][3]["role"], "tool");
    assert_eq!(openai["messages"][3]["tool_call_id"], "call_1");
    assert!(
        openai["messages"][3]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("INVENTORY-FILE-BODY-7f3a"),
        "openai-compatible: the result's content is not on the wire: {openai}"
    );

    let gemini: serde_json::Value = serde_json::from_str(&two.gemini).expect("json");
    assert!(
        gemini["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("Working directory: /work"),
        "gemini: the system prompt is not the system instruction: {gemini}"
    );
    assert_eq!(gemini["contents"][1]["role"], "model");
    assert_eq!(
        gemini["contents"][1]["parts"][0]["functionCall"]["args"]["path"],
        "pkg/inventory.py"
    );
    let response = &gemini["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(response["name"], "fs.read");
    assert!(
        response["response"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("INVENTORY-FILE-BODY-7f3a"),
        "gemini: the result's content is not on the wire: {gemini}"
    );

    let narrated = narration_in(two);
    assert!(
        narrated.is_empty(),
        "turn 2's request carries the harness's narration of turn 1 as if someone had said it: \
         {narrated:?}"
    );
}

/// A session resumed in a new process is sent the conversation the live
/// session had, rebuilt from its transcript alone.
///
/// `--continue` and `--resume <id>` both open a session through
/// `terminal::open::restored_context`, over what `session::resume` read off
/// the disk; they differ only in which session they pick. This builds nothing
/// in memory the live session built: a new context, from the files.
///
/// The mutant: restoring from `context.json` rather than the transcript,
/// which the overwritten checkpoint below makes disagree.
#[tokio::test]
async fn a_session_resumed_in_a_new_process_is_sent_the_same_conversation() {
    let scratch = Scratch::new("turn-memory-resume");
    let held = HeldSecrets::none();
    let mut live = SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    let model = Scripted::answering([
        calls("call_1", "fs.read", r#"{"path":"pkg/inventory.py"}"#),
        answer("I read it."),
        answer("It defines remove."),
        answer("It said remove."),
    ]);
    a_turn(
        &scratch,
        &mut live,
        &model,
        &held,
        32_768,
        1,
        "read pkg/inventory.py",
    )
    .await;
    a_turn(
        &scratch,
        &mut live,
        &model,
        &held,
        32_768,
        2,
        "what did the file say",
    )
    .await;

    // A checkpoint that disagrees with the transcript. Nothing reads it back,
    // so the rebuild below must not see it.
    zaru_cli::session::Checkpoint::at(scratch.session.checkpoint_path())
        .write(&serde_json::json!({ "exchanges": [] }))
        .expect("the checkpoint is overwritten");

    let resumed = zaru_cli::session::resume(scratch.session.directory(), usize::MAX)
        .expect("the session resumes");
    let (mut restored, rebuilt) =
        zaru_cli::terminal::open::restored_context(&resumed, shape(), None, &facts());
    assert_eq!(
        rebuilt.unrecorded_calls(),
        0,
        "every call of this session was recorded"
    );
    assert_eq!(
        restored.exchanges(),
        live.exchanges(),
        "a resumed session was not rebuilt into the conversation the live session had"
    );

    a_turn(
        &scratch,
        &mut restored,
        &model,
        &held,
        32_768,
        3,
        "and now?",
    )
    .await;
    let sent = model.sent();
    let three = &sent[3];
    assert_eq!(
        three.history.len(),
        6,
        "after a resume turn 3 is not sent both earlier turns: {:?}",
        three.history
    );
    assert!(
        matches!(&three.history[2], Message::Tool { content, .. }
            if content.contains("INVENTORY-FILE-BODY-7f3a")),
        "after a resume the model is not sent what its fs.read returned: {:?}",
        three.history
    );
    let narrated = narration_in(three);
    assert!(
        narrated.is_empty(),
        "narration after a resume: {narrated:?}"
    );
}

/// A result the output budget cut is recorded and sent again exactly as the
/// model first read it, cut marks included.
///
/// A listing of a folder with two thousand entries is what the budget cuts
/// here. Until 2026-09-28 this was an `fs.read` and then an `fs.search`, and
/// both now size their own answers, so neither is cut by the budget. The
/// ranged read is the next check.
///
/// The mutant: a result altered by a single character on its way back into a
/// later prompt (its trailing newline trimmed), which the byte-for-byte
/// comparison sees.
#[tokio::test]
async fn a_result_cut_by_the_budget_is_sent_again_exactly_as_the_model_first_read_it() {
    let scratch = Scratch::new("turn-memory-cut");
    let folder = scratch.project().join("pkg/many");
    std::fs::create_dir_all(&folder).expect("staging: a folder");
    let names: Vec<String> = (0..2_000).map(|n| format!("entry-{n:04}.txt")).collect();
    for name in &names {
        std::fs::write(folder.join(name), "").expect("staging: an entry");
    }
    let big = names.join("\n");
    let held = HeldSecrets::none();
    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    let model = Scripted::answering([
        calls("call_1", "fs.list", r#"{"path":"pkg/many"}"#),
        answer("It is long."),
        answer("It had two thousand entries."),
    ]);
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        4_096,
        1,
        "list pkg/many",
    )
    .await;
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        4_096,
        2,
        "how long was it",
    )
    .await;

    let sent = model.sent();
    let first_read = match &sent[1].turn[1] {
        Message::Tool { content, .. } => content.clone(),
        other => panic!("turn 1's second request does not end with the result: {other:?}"),
    };
    assert!(
        first_read.len() < big.len() && first_read.contains("elided"),
        "staging: the budget did not cut the result, so this check says nothing about a cut one \
         ({} of {} bytes)",
        first_read.len(),
        big.len()
    );
    let later = match &sent[2].history[2] {
        Message::Tool { content, .. } => content.clone(),
        other => panic!("turn 2's history does not hold the result where it was: {other:?}"),
    };
    assert_eq!(
        later, first_read,
        "a cut result was not sent again exactly as the model first read it"
    );
}

/// **A ranged `fs.read` is sent again on the next turn exactly as the model
/// first read it**: the same lines, the same numbers, the same note of what
/// was not shown.
///
/// Watched red on `6e94f43`, where `fs.read` took no range: the call was
/// refused for carrying a field it did not take, so there was no ranged
/// result to send again.
#[tokio::test]
async fn a_ranged_read_is_sent_again_exactly_as_the_model_first_read_it() {
    let scratch = Scratch::new("turn-memory-ranged");
    let big: String = (1..=5_000).map(|n| format!("record {n}\n")).collect();
    std::fs::write(scratch.project().join("pkg/big.txt"), &big).expect("staging");
    let held = HeldSecrets::none();
    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    let model = Scripted::answering([
        calls(
            "call_1",
            "fs.read",
            r#"{"path":"pkg/big.txt","start_line":2500,"line_count":3}"#,
        ),
        answer("I read three lines."),
        answer("Lines 2500 to 2502."),
    ]);
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        1,
        "read the middle of pkg/big.txt",
    )
    .await;
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        2,
        "which lines?",
    )
    .await;

    let sent = model.sent();
    let first_read = match &sent[1].turn[1] {
        Message::Tool { content, .. } => content.clone(),
        other => panic!("turn 1's second request does not end with the result: {other:?}"),
    };
    assert!(
        first_read.contains("2500\u{2502}record 2500\n")
            && first_read.contains("2502\u{2502}record 2502\n")
            && !first_read.contains("record 2503")
            && first_read.contains("start_line 2503"),
        "the ranged read did not return lines 2500 to 2502 and where to read on: {first_read:?}"
    );
    let later = match &sent[2].history[2] {
        Message::Tool { content, .. } => content.clone(),
        other => panic!("turn 2's history does not hold the result where it was: {other:?}"),
    };
    assert_eq!(
        later, first_read,
        "a ranged read was not sent again exactly as the model first read it"
    );
    for (provider, body) in [
        ("ollama", &sent[2].ollama),
        ("openai-compatible", &sent[2].openai),
        ("gemini", &sent[2].gemini),
    ] {
        assert!(
            body.contains("start_line 2503"),
            "{provider}: turn 2's request does not carry the ranged result"
        );
    }
}

/// A transcript from before calls and results were recorded resumes with what
/// it holds — what the person asked and what the model answered — and says
/// how many calls it cannot give back.
///
/// The accepting sibling is the recorded session of the resume check above,
/// whose count is zero.
#[test]
fn a_transcript_from_before_results_were_kept_resumes_with_what_it_holds() {
    use zaru_cli::session::{Phase, ToolCall, Utterance, Voice};
    use zaru_core::tool_call::{Event, TurnEnding};

    let scratch = Scratch::new("turn-memory-old");
    let mut transcript = Transcript::append_to(scratch.session.transcript_path()).expect("opens");
    let line = |text: &str, phase| {
        Record::ToolCall(ToolCall {
            line: text.to_owned(),
            out_of_tree: false,
            destructive: false,
            phase,
        })
    };
    for record in [
        Record::Conversation(Utterance {
            n: 1,
            voice: Voice::User,
            text: "fix the failing tests".to_owned(),
        }),
        Record::TurnLoop(Event::TurnStarted { n: 1, of: None }),
        Record::TurnLoop(Event::ToolRequested {
            round: 1,
            call: 1,
            name: "cmd.run".to_owned(),
        }),
        line("cmd.run python3 -m unittest -q", Phase::Started),
        line("cmd.run python3 -m unittest -q", Phase::Completed),
        Record::TurnLoop(Event::ToolCompleted {
            round: 1,
            call: 1,
            name: "cmd.run".to_owned(),
            failed: true,
            content_bytes: 1256,
            elapsed: Duration::ZERO,
        }),
        Record::Conversation(Utterance {
            n: 1,
            voice: Voice::Zaru,
            text: "Two tests fail.".to_owned(),
        }),
        Record::TurnLoop(Event::TurnEnded {
            n: 1,
            ending: TurnEnding::Answered,
            rounds: 2,
            elapsed: Duration::ZERO,
        }),
    ] {
        transcript.record(&record).expect("a record");
    }

    let resumed = zaru_cli::session::resume(scratch.session.directory(), usize::MAX)
        .expect("an old transcript resumes");
    let (context, rebuilt) =
        zaru_cli::terminal::open::restored_context(&resumed, shape(), None, &facts());
    assert_eq!(
        rebuilt.unrecorded_calls(),
        1,
        "the transcript names one call it never kept, and the rebuild does not say so"
    );
    assert_eq!(context.exchanges().len(), 1, "the old turn is not rebuilt");
    assert_eq!(
        context.exchanges()[0].messages(),
        [
            Message::User {
                text: "fix the failing tests".to_owned()
            },
            Message::Assistant {
                text: "Two tests fail.".to_owned(),
                calls: Vec::new(),
                echo: None,
            },
        ],
        "an old turn resumes as what the person asked and what the model answered"
    );
}

/// A call left without a result is closed in the next turn's conversation
/// with a result saying it did not complete, and it is not run again.
///
/// This is ADR-0010 D4's "the model is told it did not complete", in the
/// shape a provider defines for a result: every provider refuses a call with
/// no answer, and a model given one would not know whether the call ran.
///
/// The mutant: leaving the call open, which reddens the history arm.
#[tokio::test]
async fn an_interrupted_call_is_closed_in_the_next_turn_and_not_run_again() {
    use zaru_cli::session::{Utterance, Voice};
    use zaru_core::tool_call::Event;

    let scratch = Scratch::new("turn-memory-interrupted");
    let written = scratch.project().join("pkg/fresh.py");
    {
        let mut transcript =
            Transcript::append_to(scratch.session.transcript_path()).expect("opens");
        for record in [
            Record::Conversation(Utterance {
                n: 1,
                voice: Voice::User,
                text: "write pkg/fresh.py".to_owned(),
            }),
            Record::TurnLoop(Event::Message(Message::User {
                text: "write pkg/fresh.py".to_owned(),
            })),
            Record::TurnLoop(Event::Message(Message::Assistant {
                text: String::new(),
                calls: vec![ToolRequest {
                    id: "call_w".to_owned(),
                    name: "fs.write".to_owned(),
                    arguments: r#"{"path":"pkg/fresh.py","contents":"x = 1\n"}"#.to_owned(),
                }],
                echo: None,
            })),
        ] {
            transcript.record(&record).expect("a record");
        }
    }

    let resumed =
        zaru_cli::session::resume(scratch.session.directory(), usize::MAX).expect("resumes");
    let (mut context, _) =
        zaru_cli::terminal::open::restored_context(&resumed, shape(), None, &facts());
    let held = HeldSecrets::none();
    let model = Scripted::answering([answer("That write did not finish.")]);
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        2,
        "what happened?",
    )
    .await;

    let sent = model.sent();
    assert_eq!(
        sent[0].history.get(2),
        Some(&Message::Tool {
            id: "call_w".to_owned(),
            name: "fs.write".to_owned(),
            content: zaru_cli::compose::prose::CALL_DID_NOT_COMPLETE.to_owned(),
            failed: true,
        }),
        "the call that never finished is not closed with a result saying so: {:?}",
        sent[0].history
    );
    assert!(
        !written.exists(),
        "the interrupted write was run again on resume"
    );
}

/// Compaction takes whole turns, so a call is never parted from its result,
/// and the newest message — the turn's own task — is never taken.
///
/// **What compaction did before this change, pinned**: the span is the
/// shortest run of the oldest exchanges whose measured size covers the
/// overage, replaced by one summary. That is unchanged; what changed is that
/// an exchange is a whole turn's messages rather than a rendering of them. A
/// session rebuilt from its transcript after a compaction holds the same
/// summary the live one does, because the compaction's record carries it.
#[tokio::test]
async fn a_compaction_takes_whole_turns_and_a_rebuild_keeps_its_summary() {
    struct Summarising;
    impl zaru_core::context::Summariser for Summarising {
        async fn summarise(&self, _span: &zaru_core::context::Span) -> Result<String, PortFailure> {
            Ok("SUMMARY-OF-EARLIER-TURNS".to_owned())
        }
    }

    let scratch = Scratch::new("turn-memory-compaction");
    let held = HeldSecrets::none();
    // The threshold is set so that four turns of one small `fs.read` each
    // are over it and two are not. It was 700 until 2026-09-28, when an
    // `fs.read` answer gained its header and line numbers and each turn grew
    // by about 180 bytes.
    let tight = ContextShape::of(
        ContextLimits::new(
            ContextWindow::new(12_000).expect("a window"),
            PressureThreshold::new(1_200).expect("a threshold"),
        )
        .expect("limits"),
        0,
        one_token_a_byte(),
    );
    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), tight.clone());
    let mut script = Vec::new();
    for n in 0..4 {
        script.push(calls(
            &format!("call_{n}"),
            "fs.read",
            r#"{"path":"pkg/inventory.py"}"#,
        ));
        script.push(answer(&format!("answer {n}")));
    }
    script.push(answer("the last answer"));
    let model = Scripted::answering(script);
    for n in 0..4 {
        a_turn(
            &scratch,
            &mut context,
            &model,
            &held,
            32_768,
            n + 1,
            &format!("task {n}"),
        )
        .await;
    }

    println!(
        "before the compaction: {} of layer 6 across {} turns",
        context.usage(&held).used(),
        context.exchanges().len()
    );
    let compaction = context
        .at_turn_boundary(&Summarising, &held)
        .await
        .expect("the summariser answers");
    let taken = compaction
        .raw
        .as_ref()
        .map_or(0, zaru_core::context::Span::len);
    assert!(
        taken > 0 && taken < 4,
        "the staging must compact some turns and not all of them; it took {taken}"
    );
    Transcript::append_to(scratch.session.transcript_path())
        .expect("opens")
        .record(&Record::Compacted(compaction))
        .expect("the compaction is recorded, as the turn boundary records it");

    // Every exchange left holds each of its calls together with its result.
    for exchange in context.exchanges() {
        let called: Vec<&str> = exchange
            .messages()
            .iter()
            .flat_map(|message| match message {
                Message::Assistant { calls, .. } => {
                    calls.iter().map(|call| call.id.as_str()).collect()
                }
                Message::User { .. } | Message::Tool { .. } => Vec::new(),
            })
            .collect();
        let answered: Vec<&str> = exchange
            .messages()
            .iter()
            .filter_map(|message| match message {
                Message::Tool { id, .. } => Some(id.as_str()),
                Message::User { .. } | Message::Assistant { .. } => None,
            })
            .collect();
        assert_eq!(
            called, answered,
            "a compaction parted a call from its result: {exchange:?}"
        );
    }
    assert_eq!(
        context.exchanges()[0].messages(),
        [Message::User {
            text: "SUMMARY-OF-EARLIER-TURNS".to_owned()
        }],
        "the summary replaces the oldest turns at the front"
    );

    // The next turn: its own task is its last message, whatever was taken.
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        5,
        "the newest task",
    )
    .await;
    let sent = model.sent();
    assert_eq!(
        sent.last().expect("a request").task,
        "the newest task",
        "the newest message is not the turn's own task"
    );

    // The turn's own end rebuilt layer 6 from the transcript, and the summary
    // is still where the compaction put it: the record carried it. Asserted
    // on the rebuild itself, because comparing two rebuilds would agree with
    // a rebuild that dropped the summary from both.
    assert_eq!(
        context.exchanges()[0].messages(),
        [Message::User {
            text: "SUMMARY-OF-EARLIER-TURNS".to_owned()
        }],
        "layer 6 rebuilt from the transcript after a compaction does not begin with the summary \
         the model was sent, so the compacted turns are back in full"
    );
    assert_eq!(
        context.exchanges().len(),
        1 + (4 - taken) + 1,
        "layer 6 rebuilt after a compaction is not the summary, the turns it left and the new one"
    );

    // A session rebuilt from the transcript holds the same summary.
    let resumed =
        zaru_cli::session::resume(scratch.session.directory(), usize::MAX).expect("resumes");
    let (restored, _) = zaru_cli::terminal::open::restored_context(&resumed, tight, None, &facts());
    assert_eq!(
        restored.exchanges(),
        context.exchanges(),
        "a session rebuilt after a compaction is not the session the live one was"
    );
}

/// A stored key in a tool's result reaches neither a request nor the
/// transcript, in the turn that read it or any later one.
///
/// The redaction check `providers_from_outside` already had holds the request
/// body; this extends it to the transcript, which now records every result.
/// The accepting sibling is the marker, present where the key was.
#[tokio::test]
async fn a_stored_key_in_a_tool_result_reaches_neither_a_request_nor_the_transcript() {
    use zaru_cli::credentials::{
        Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
        SealingKey, Secret, ToolScope,
    };

    struct StagedKey(SealingKey);
    impl KeyStore for StagedKey {
        fn key(&self) -> Result<SealingKey, SealingError> {
            Ok(self.0.clone())
        }
    }

    let scratch = Scratch::new("turn-memory-held");
    let planted = format!("nn_mcp_turnmemory{}e\u{301}\u{1f701}", std::process::id());
    let core = planted
        .split(|c: char| !c.is_ascii())
        .next()
        .expect("an ASCII core")
        .to_owned();
    std::fs::write(
        scratch.project().join("pkg/secrets.txt"),
        format!("token = {planted}\n"),
    )
    .expect("staging");
    let keys = StagedKey(SealingKey::mint());
    let mut store = CredentialStore::open(scratch.base.join("zaru")).expect("the store opens");
    let alias = Alias::new("work").expect("an alias");
    store
        .add(
            Entry::notes(
                alias.clone(),
                Description::new("the bearer this check plants").expect("one line"),
                Secret::notes(planted.clone()).expect("an nn_ value"),
                Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
            )
            .expect("an entry")
            .with_tools(ToolScope::of_names(["pages.read"])),
            &keys,
            None,
        )
        .expect("the entry is stored");
    let held = zaru_cli::redaction::held_secrets_for_redaction(&store, &keys).expect("held");

    let mut context =
        SessionContext::opened(zaru_cli::compose::prefix_for(None, &facts()), shape());
    let model = Scripted::answering([
        calls("call_1", "fs.read", r#"{"path":"pkg/secrets.txt"}"#),
        answer("I read it."),
        answer("It held a token."),
    ]);
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        1,
        "read pkg/secrets.txt",
    )
    .await;
    a_turn(
        &scratch,
        &mut context,
        &model,
        &held,
        32_768,
        2,
        "what was in it",
    )
    .await;

    let transcript =
        std::fs::read_to_string(scratch.session.transcript_path()).expect("the transcript reads");
    let marker = zaru_cli::redaction::marker(&alias);
    let mut bodies: Vec<(String, String)> = vec![("the transcript".to_owned(), transcript)];
    for (n, sent) in model.sent().iter().enumerate() {
        for (provider, body) in [
            ("ollama", &sent.ollama),
            ("openai-compatible", &sent.openai),
            ("gemini", &sent.gemini),
        ] {
            bodies.push((format!("request {n} to {provider}"), body.clone()));
        }
    }
    let reached: Vec<&str> = bodies
        .iter()
        .filter(|(_, body)| body.contains(&planted) || body.contains(&core))
        .map(|(what, _)| what.as_str())
        .collect();
    assert!(
        reached.is_empty(),
        "a stored key reached {} of {} places: {reached:?}",
        reached.len(),
        bodies.len()
    );
    assert!(
        bodies[0].1.contains(&marker) && bodies.last().expect("a body").1.contains(&marker),
        "nothing was replaced in the transcript or in turn 2's request, so the absence above \
         could be of a result that was never there"
    );
}

/// Two messages of one role side by side are joined into one turn for
/// Gemini, whose protocol expects the roles to alternate, and sent as they
/// are to the two kinds that accept a repeated role.
///
/// They arise after an interrupted turn (the closing result, then the next
/// task) and after a turn that left no answer (an iterating turn, or an old
/// transcript): two `user` messages in a row. What each kind is sent is
/// pinned here, on the request each mapping builds.
///
/// The mutant: pushing every message as a turn of its own, which puts two
/// `user` turns side by side on the Gemini wire.
#[test]
fn messages_of_one_role_side_by_side_are_one_gemini_turn_and_are_pinned_for_the_other_two() {
    let nothing = HeldSecrets::none();
    let interrupted = [
        Message::User {
            text: "run the slow command".to_owned(),
        },
        Message::Assistant {
            text: String::new(),
            calls: vec![ToolRequest {
                id: "call_s".to_owned(),
                name: "cmd.run".to_owned(),
                arguments: r#"{"command":"sleep 20"}"#.to_owned(),
            }],
            echo: None,
        },
        Message::Tool {
            id: "call_s".to_owned(),
            name: "cmd.run".to_owned(),
            content: zaru_cli::compose::prose::CALL_DID_NOT_COMPLETE.to_owned(),
            failed: true,
        },
    ];
    let unanswered = [Message::User {
        text: "a task that left no answer".to_owned(),
    }];

    for (what, history, gemini_roles, gemini_last_parts, chat_roles) in [
        (
            "after an interrupted call",
            &interrupted[..],
            vec!["user", "model", "user"],
            vec!["functionResponse", "text"],
            vec!["system", "user", "assistant", "tool", "user"],
        ),
        (
            "after a turn with no answer",
            &unanswered[..],
            vec!["user"],
            vec!["text", "text"],
            vec!["system", "user", "user"],
        ),
    ] {
        let prompt = Prompt::assembled(&nothing, "SYSTEM", history, "the next task");
        let request = ModelRequest {
            prompt: &prompt,
            tools: &[],
            turn: &[],
        };

        let gemini = serde_json::to_value(
            zaru_cli::providers::gemini::map::request_from(&request).expect("gemini maps"),
        )
        .expect("json");
        let contents = gemini["contents"].as_array().expect("contents");
        let roles: Vec<&str> = contents
            .iter()
            .map(|content| content["role"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            roles, gemini_roles,
            "{what}: gemini was sent two turns of one role side by side: {gemini}"
        );
        let last: Vec<&str> = contents.last().expect("a turn")["parts"]
            .as_array()
            .expect("parts")
            .iter()
            .map(|part| {
                if part.get("functionResponse").is_some() {
                    "functionResponse"
                } else {
                    "text"
                }
            })
            .collect();
        assert_eq!(
            last, gemini_last_parts,
            "{what}: the joined turn's parts are not in the order the messages came: {gemini}"
        );
        assert_eq!(
            contents.last().expect("a turn")["parts"]
                .as_array()
                .expect("parts")
                .last()
                .expect("a part")["text"],
            "the next task",
            "{what}: the task is not the last part of the last turn: {gemini}"
        );

        for (kind, body) in [
            (
                "ollama",
                serde_json::to_value(
                    zaru_cli::providers::ollama::map::request_from(&request, "m", 4096)
                        .expect("maps"),
                )
                .expect("json"),
            ),
            (
                "openai-compatible",
                serde_json::to_value(
                    zaru_cli::providers::openai_compatible::map::request_from(&request, "m")
                        .expect("maps"),
                )
                .expect("json"),
            ),
        ] {
            let roles: Vec<&str> = body["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .map(|message| message["role"].as_str().unwrap_or_default())
                .collect();
            assert_eq!(
                roles, chat_roles,
                "{what}: {kind} was not sent the messages as they are: {body}"
            );
        }
    }
}

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

/// A calibration that has learned one token a byte, so this file's numbers,
/// written in bytes, are the counts the context is measured at.
fn one_token_a_byte() -> zaru_cli::providers::capacity::Calibration {
    let calibration = zaru_cli::providers::capacity::Calibration::starting();
    assert!(
        calibration.learn(1_000, 1_000),
        "one token a byte is a count"
    );
    calibration
}
