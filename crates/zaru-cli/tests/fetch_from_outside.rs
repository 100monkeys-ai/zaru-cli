// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside both crates drives `zaru-core`'s tool-call loop over
//! `zaru-cli`'s **real** `web.fetch`, and every refusal that surface makes.
//!
//! # Why this file holds the refusals and not the retrievals
//!
//! [`WebClient::new`] is the only constructor in the compiled library and it
//! refuses this machine, so a check outside the crate cannot be served by a
//! listener it started. The successful-retrieval path is therefore driven from
//! `crate::web`'s own in-crate checks, on the precedent of `crate::process`'s,
//! which drive real child processes the same way; **every refusal is driven
//! here, over the real client, because a refusal needs no server at all.**
//!
//! That division is the cost of the destination rule and it is stated rather
//! than hidden. What this file gets in exchange is the property the in-crate
//! checks cannot have: it holds only what `zaru-cli`'s public door offers, so
//! a permissive constructor leaking into that door would have to appear here
//! to be used.
//!
//! # Nothing here reaches a network, and that is structural rather than
//! careful
//!
//! Every URL in this file is refused before a socket is opened: by the parse,
//! for a scheme; or by the destination rule, for this machine and the
//! link-local range. **No check here names a routable host**, so there is no
//! DNS lookup to make and nothing for the CI runner's absent network to
//! attempt.
//!
//! **Evidence about the mechanism, and it must never be quoted as evidence
//! about the `zaru` binary**, which runs the commands the command surface
//! landed, none of which reaches the tool surface, and which refuses a task
//! because no provider client is wired into a loop.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [`WebClient::new`]: zaru_cli::web::WebClient::new

use core::time::Duration;
use std::sync::Mutex;
use zaru_cli::process::CommandLine;
use zaru_cli::redaction::HeldSecrets;
use zaru_cli::session::{SessionId, SessionStore, SystemWallClock, Transcript};
use zaru_cli::tools::port::Answer;
use zaru_cli::tools::{
    Captured, ConfirmFailure, Executor, Mode, NoMembrane, OutputBudget, Question, SessionOverflow,
    Subprocess, WorkingDirectory,
};
use zaru_cli::web::WebClient;
use zaru_core::iteration::{Clock, ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Ports, Start,
    TokenUsage, ToolCallCeiling, ToolCalling, ToolRequest, run,
};

// --- staging ---------------------------------------------------------------

fn nonce(label: &str) -> String {
    format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!("wf-{}", nonce(label)));
        std::fs::create_dir_all(base.join("project")).expect("staging: the project");
        std::fs::create_dir_all(base.join("sessions")).expect("staging: the session root");
        Self { base }
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

#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

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
        println!("  event: {event:?}");
    }
}

struct NoCommands;

impl Subprocess for NoCommands {
    async fn run(&self, _line: &CommandLine) -> Result<Captured, PortFailure> {
        Err(PortFailure::new("cmd.run is not this file's subject"))
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

/// A confirmer that records whether it was ever asked.
///
/// The subject of `a_retrieval_is_not_prompted_at_the_default_mode`: the
/// question is not what it answers but whether the surface consults it at all.
#[derive(Default)]
struct Counting(Mutex<Vec<String>>);

impl zaru_cli::tools::Confirm for Counting {
    fn confirm(&self, question: &Question) -> Result<Answer, ConfirmFailure> {
        self.0
            .lock()
            .expect("the counter is not poisoned")
            .push(question.statement.clone());
        Ok(Answer::Once)
    }
}

struct NeverIterates;
impl InnerLoop for NeverIterates {
    async fn iterate(&self, _task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        unreachable!("no validators are declared in these checks")
    }
}

struct Provider {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    seen: Mutex<Vec<(String, String)>>,
    offered: Mutex<Vec<(String, String)>>,
}

impl Provider {
    fn new(script: Vec<ModelResponse>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            seen: Mutex::new(Vec::new()),
            offered: Mutex::new(Vec::new()),
        }
    }
}

impl Model for Provider {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        let mut offered = self.offered.lock().expect("offered poisoned");
        if offered.is_empty() {
            for descriptor in request.tools {
                offered.push((descriptor.name.clone(), descriptor.parameters.clone()));
            }
        }
        drop(offered);
        let mut seen = self.seen.lock().expect("seen poisoned");
        for result in request.results {
            if seen.iter().any(|(id, _)| id == &result.id) {
                continue;
            }
            println!("  model was given: {:?}", result.content.as_str());
            seen.push((result.id.clone(), result.content.as_str().to_owned()));
        }
        drop(seen);
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

fn fetch_call(id: &str, url: &str) -> ModelResponse {
    ModelResponse::Calls {
        calls: vec![ToolRequest {
            id: id.to_owned(),
            name: String::from("web.fetch"),
            arguments: serde_json::json!({ "url": url }).to_string(),
        }],
        tokens: TokenUsage::default(),
    }
}

/// What one staged run produced.
struct Run {
    given_to_the_model: Vec<String>,
    transcript: String,
    prompts: Vec<String>,
    offered: Vec<(String, String)>,
}

/// Drive a script through the real loop over the real `web.fetch`.
///
/// The `fetch` port is [`WebClient::new`] — the product constructor, with the
/// reach every caller in the workspace has. Nothing is substituted for it.
async fn drive(scratch: &Scratch, mode: Mode, script: Vec<ModelResponse>) -> Run {
    let store = SessionStore::open(scratch.base.join("sessions")).expect("the session store opens");
    let id = SessionId::mint(&SystemWallClock).expect("an id");
    let session = store.start(id).expect("the session starts");
    let directory = session.directory().to_path_buf();

    let working = WorkingDirectory::at(scratch.project()).expect("the working directory resolves");
    let mut transcript =
        Transcript::append_to(session.transcript_path()).expect("the transcript opens");
    let mut overflow = SessionOverflow::in_session(session.directory());
    let nothing = Nothing;
    let commands = NoCommands;
    let membrane = NoMembrane;
    let confirmer = Counting::default();
    let clock = Ticking::default();
    let redactor = HeldSecrets::none();
    let mut sink = Printing;
    let calls = script.len();

    // The real client, built exactly as the binary builds it, with the
    // binary's own bounds.
    let web = WebClient::new(zaru_cli::cli::layers::fetch_bounds()).expect("an HTTP client builds");

    let mut script = script;
    script.push(ModelResponse::Text {
        text: String::from("done"),
        tokens: TokenUsage::default(),
    });
    let model = Provider::new(script);

    {
        let no_grants = zaru_cli::tools::grants::SessionGrants::none();
        let mut executor = Executor {
            working_directory: &working,
            mode,
            allowlist: &nothing,
            destructive: &nothing,
            session_grants: &no_grants,
            confirmer: Some(&confirmer),
            verdicts: &membrane,
            budget: OutputBudget::new(4096).expect("a usable budget"),
            preview_budget: OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: zaru_cli::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &redactor as &(dyn Redactor + Sync),
            subprocess: &commands,
            fetch: &web,
            projected: &zaru_cli::tools::NoProjection,
            declared: zaru_cli::tools::descriptor_set(),
        };
        run::<_, _, _, _, _, NeverIterates>(
            1,
            Start::Task("work on the project"),
            ToolCallCeiling::new(u32::try_from(calls + 1).expect("a small script"))
                .expect("a ceiling"),
            ToolCalling::required(&model, "staged").expect("the staged model can call tools"),
            Ports {
                model: &model,
                tools: &mut executor,
                context: &Policy,
                clock: &clock,
                redactor: &redactor,
            },
            None,
            &mut [&mut sink],
        )
        .await
        .expect("no port failed");
    }

    let text = std::fs::read_to_string(directory.join("transcript.jsonl"))
        .expect("the transcript is on disk");
    println!("  transcript:\n{text}");

    Run {
        given_to_the_model: model
            .seen
            .lock()
            .expect("seen poisoned")
            .iter()
            .map(|(_, content)| content.clone())
            .collect(),
        transcript: text,
        prompts: confirmer.0.lock().expect("the counter").clone(),
        offered: model.offered.lock().expect("offered").clone(),
    }
}

// --- the checks ------------------------------------------------------------

/// A scheme this surface does not retrieve is **not a call**, so nothing is
/// recorded.
///
/// The security-corpus case behind it is `file://`: it would read the
/// filesystem with none of [ADR-0011] D4's working-directory boundary, which
/// is `fs.read` without the one rule `fs.read` has. It is refused at the
/// parse, in the same place and for the same reason a shell construct is
/// refused before `cmd.run` is decided — and, like that one, it writes **no
/// transcript record**, because D4's "mode may remove the prompt; it never
/// removes the record" is about a call and nothing was attempted.
#[tokio::test]
async fn a_scheme_this_surface_does_not_retrieve_is_not_a_call_and_is_never_recorded() {
    println!("== five schemes, none of them a call ==");
    let scratch = Scratch::new("scheme");
    let run = drive(
        &scratch,
        Mode::Ask,
        vec![
            fetch_call("s1", "file:///etc/passwd"),
            fetch_call("s2", "data:text/plain,hello"),
            fetch_call("s3", "ftp://example.invalid/x"),
            fetch_call("s4", "gopher://example.invalid/x"),
            fetch_call("s5", "not a url at all"),
        ],
    )
    .await;

    assert_eq!(
        run.given_to_the_model.len(),
        5,
        "the model is told about each, and the turn carries on: {:?}",
        run.given_to_the_model
    );
    let mut wrong = Vec::new();
    for (index, told) in run.given_to_the_model.iter().enumerate() {
        if !told.contains("web.fetch retrieves") && !told.contains("is not a URL") {
            wrong.push(format!("call {index} was told {told:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "each refusal says what it refused: {wrong:#?}"
    );

    assert!(
        !run.transcript.contains("web.fetch"),
        "nothing that is not a call reaches the transcript, and it held:\n{}",
        run.transcript
    );
    assert!(
        run.prompts.is_empty(),
        "nothing that is not a call is put to the user, and these were: {:?}",
        run.prompts
    );
    assert!(
        run.given_to_the_model[0].contains("fs.read without the one rule fs.read has"),
        "the file scheme's refusal says why it is not merely an unsupported scheme, and said {:?}",
        run.given_to_the_model[0]
    );
}

/// This machine and the link-local range are refused **as calls**, so each one
/// is in the record.
///
/// The difference from the check above is the whole point of the two-place
/// design: a well-formed `GET` to `http://169.254.169.254/` *is* a call this
/// surface could make and declines to, so a user must be able to find it in
/// their transcript afterwards.
#[tokio::test]
async fn a_refused_destination_is_a_refused_call_and_is_in_the_record() {
    println!("== this machine and the metadata endpoint ==");
    let scratch = Scratch::new("destination");
    let run = drive(
        &scratch,
        Mode::Ask,
        vec![
            fetch_call("d1", "http://127.0.0.1:9/nothing"),
            fetch_call("d2", "http://localhost:9/nothing"),
            fetch_call("d3", "http://[::1]:9/nothing"),
            fetch_call("d4", "http://169.254.169.254/latest/meta-data/"),
            fetch_call("d5", "http://[fe80::1]/x"),
        ],
    )
    .await;

    let mut wrong = Vec::new();
    for (index, told) in run.given_to_the_model.iter().enumerate() {
        let expected = if index < 3 {
            "this machine"
        } else {
            "link-local"
        };
        if !told.contains(expected) {
            wrong.push(format!(
                "call {index} wanted {expected:?} and was told {told:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "each destination is refused for what it is, by name: {wrong:#?}"
    );
    assert!(
        run.given_to_the_model[3].contains("instance-metadata"),
        "the metadata endpoint is named as a credential source rather than as an address, and \
         was {:?}",
        run.given_to_the_model[3]
    );

    // The half the scheme check asserts the negative of: a call the surface
    // declined to make IS recorded, all five of them.
    assert_eq!(
        run.transcript.matches("web.fetch").count(),
        10,
        "every refused call writes its started and refused pair, and the transcript held:\n{}",
        run.transcript
    );
    for host in ["127.0.0.1", "localhost", "169.254.169.254"] {
        assert!(
            run.transcript.contains(host),
            "the record names the host that was asked for, and {host} was absent from:\n{}",
            run.transcript
        );
    }
}

/// [ADR-0011] D3's `ask` row does not prompt for a retrieval, read literally.
///
/// **This pins the open question rather than answering it.** D3's `ask` row is
/// "prompts before any write or command" and a retrieval is neither, so at the
/// default mode a model-chosen URL is fetched with no prompt — which is the
/// asymmetry the destination rule exists to answer, and which is recorded on
/// [ADR-0011] as open. If that question is ever answered the other way, this
/// check reddens, which is how whoever answers it finds every place that
/// depended on the literal reading.
#[tokio::test]
async fn a_retrieval_is_not_prompted_at_the_default_mode() {
    println!("== the default mode puts no question about a URL ==");
    let scratch = Scratch::new("prompt");
    let run = drive(
        &scratch,
        Mode::Ask,
        vec![fetch_call("p1", "http://127.0.0.1:9/nothing")],
    )
    .await;

    assert!(
        run.prompts.is_empty(),
        "ADR-0011 D3's `ask` prompts before a write or a command and a retrieval is neither, so \
         no question is put; these were: {:?}",
        run.prompts
    );
    // The accepting sibling, in the same file rather than in another: the
    // confirmer is real and IS consulted for a write, so an executor that
    // never asked anybody would not satisfy the assertion above.
    let written = drive(
        &scratch,
        Mode::Ask,
        vec![ModelResponse::Calls {
            calls: vec![ToolRequest {
                id: String::from("p2"),
                name: String::from("fs.write"),
                arguments: serde_json::json!({ "path": "note.txt", "contents": "x" }).to_string(),
            }],
            tokens: TokenUsage::default(),
        }],
    )
    .await;
    assert_eq!(
        written.prompts.len(),
        1,
        "the same confirmer is consulted for a write, so the absence above is about retrievals \
         rather than about a confirmer nobody wired: {:?}",
        written.prompts
    );
}

/// The descriptor a model is shown carries `url` and nothing else, and it is
/// JSON a provider can parse.
#[tokio::test]
async fn web_fetch_is_offered_with_the_one_field_adr_0011_d1_gives_it() {
    println!("== the wire contract for the seventh built-in ==");
    let scratch = Scratch::new("descriptor");
    let run = drive(&scratch, Mode::Ask, vec![fetch_call("w1", "file:///x")]).await;

    let (_, parameters) = run
        .offered
        .iter()
        .find(|(name, _)| name == "web.fetch")
        .expect("web.fetch is among the descriptors offered");
    let schema: serde_json::Value =
        serde_json::from_str(parameters).expect("a provider parses the descriptor's parameters");
    assert_eq!(
        schema["properties"],
        serde_json::json!({ "url": { "type": "string" } }),
        "ADR-0011 D1's argument contract gives web.fetch exactly one field, and the schema was \
         {schema}"
    );
    assert_eq!(
        schema["additionalProperties"],
        serde_json::json!(false),
        "there is no second field a header could arrive through, and the schema was {schema}"
    );
}

/// A request naming one built-in never performs another.
///
/// The structural case: the executor derives the invocation from the request
/// it was handed, so a `web.fetch` carrying an `fs.write`'s arguments is not a
/// call at all rather than a write.
#[tokio::test]
async fn a_retrieval_carrying_another_tool_s_arguments_is_not_that_tool() {
    println!("== a web.fetch cannot become a write ==");
    let scratch = Scratch::new("confusion");
    let target = scratch.project().join("planted.txt");
    let run = drive(
        &scratch,
        Mode::Ask,
        vec![ModelResponse::Calls {
            calls: vec![ToolRequest {
                id: String::from("x1"),
                name: String::from("web.fetch"),
                arguments: serde_json::json!({ "path": "planted.txt", "contents": "written" })
                    .to_string(),
            }],
            tokens: TokenUsage::default(),
        }],
    )
    .await;

    assert!(
        run.given_to_the_model[0].contains("\"url\""),
        "the refusal names the field web.fetch declares, and was {:?}",
        run.given_to_the_model[0]
    );
    assert!(
        !target.exists(),
        "no file was created, and {} exists",
        target.display()
    );
    assert!(
        !run.transcript.contains("fs.write"),
        "nothing became a write, and the transcript held:\n{}",
        run.transcript
    );
}

/// No refusal on this path renders the URL it refused.
///
/// A URL is where a token pasted into a query string lives, and a refusal is
/// exactly the text that gets copied into a bug report. Asserted by raw value
/// **and** by an ASCII core no escaping can alter — verification lessons §50.
#[tokio::test]
async fn no_refusal_renders_the_url_it_refused() {
    println!("== a refusal names the scheme and the host, never the URL ==");
    let scratch = Scratch::new("leak");
    let planted = format!("planted-{}\u{301}-\u{1f701}", nonce("token"));
    let core = {
        let end = planted
            .char_indices()
            .find(|(_, character)| !character.is_ascii())
            .map_or(planted.len(), |(at, _)| at);
        &planted[..end]
    };
    assert!(
        core.len() > 8,
        "staging: the core is discriminating: {core:?}"
    );

    let run = drive(
        &scratch,
        Mode::Ask,
        vec![
            fetch_call("l1", &format!("file:///tmp/{planted}")),
            fetch_call("l2", &format!("gopher://host.invalid/{planted}")),
        ],
    )
    .await;

    let mut leaked = Vec::new();
    for told in &run.given_to_the_model {
        if told.contains(&planted) || told.contains(core) {
            leaked.push(told.clone());
        }
    }
    assert!(
        leaked.is_empty(),
        "a refusal renders the scheme and never the URL's path or query: {leaked:#?}"
    );
    // The accepting sibling: a refused DESTINATION does name its host, which
    // is what a user needs in order to see what the model reached for. So the
    // rule above is about the path and the query rather than about rendering
    // nothing.
    let named = drive(
        &scratch,
        Mode::Ask,
        vec![fetch_call("l3", "http://169.254.169.254/latest/meta-data/")],
    )
    .await;
    assert!(
        named.given_to_the_model[0].contains("169.254.169.254"),
        "a refused destination names the host, and said {:?}",
        named.given_to_the_model[0]
    );
}
