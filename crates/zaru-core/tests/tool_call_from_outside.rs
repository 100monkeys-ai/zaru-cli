// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives the tool-call loop to an outcome.
//!
//! Library [Verification lessons] §25: a per-property check cannot see a
//! defect that lives in a seam, and every assertion about what a system
//! *contains* can be correct while the thing a user does first does not work.
//! Everything here reaches the loop through `zaru-core`'s public door and
//! implements every port from outside, so it is evidence that the loop is
//! reachable at all.
//!
//! **Evidence about the mechanism, never about the `zaru` binary**, which
//! prints its version and its composition and reaches none of this.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use core::time::Duration;
use std::sync::Mutex;
use zaru_core::iteration::{Clock, ContextPolicy, ContextRefusal, PortFailure, Prompt, Turn};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    Capabilities, Event, EventSink, InnerLoop, Model, ModelRequest, ModelResponse, Outcome, Ports,
    Start, TokenUsage, ToolCallCeiling, ToolCalling, ToolDecision, ToolDescriptor, ToolExecutor,
    ToolOutcome, ToolRequest, ToolResult, TurnEnding, run,
};

/// A clock the caller sets, so every elapsed time here is an exact value.
#[derive(Debug, Default)]
struct Ticking(Mutex<Duration>);

impl Ticking {
    fn advance(&self, by: Duration) {
        *self.0.lock().expect("clock poisoned") += by;
    }
}

impl Clock for Ticking {
    fn now(&self) -> Duration {
        *self.0.lock().expect("clock poisoned")
    }
}

/// A model that walks a script, implemented entirely outside the crate.
struct Provider<'a> {
    script: Mutex<std::collections::VecDeque<ModelResponse>>,
    clock: &'a Ticking,
    can_call_tools: bool,
}

impl Model for Provider<'_> {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            tool_calling: self.can_call_tools,
        }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // Print what the model was actually shown, so the capture is a record
        // of the exchange rather than of the harness's opinion of it.
        println!(
            "  model <- prompt {:?}, {} tool(s) offered, {} message(s) so far this turn",
            request.prompt.rendered(),
            request.tools.len(),
            request.turn.len()
        );
        for message in request.turn {
            println!("      {message:?}");
        }
        self.clock.advance(Duration::from_millis(10));
        self.script
            .lock()
            .expect("script poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("the script ran out"))
    }
}

/// A tool surface implemented outside the crate.
///
/// It reads from a map rather than a disk: this crate must be drivable with
/// no filesystem at all, and `zaru-cli`'s own outside-caller check is where a
/// real `std::fs` read is driven.
struct Surface<'a> {
    descriptors: Vec<ToolDescriptor>,
    answers: Mutex<std::collections::VecDeque<ToolOutcome>>,
    clock: &'a Ticking,
}

impl ToolExecutor for Surface<'_> {
    fn descriptors(&self) -> &[ToolDescriptor] {
        &self.descriptors
    }

    async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
        println!("  tool  -> {} {:?}", request.name, request.arguments);
        self.clock.advance(Duration::from_millis(4));
        self.answers
            .lock()
            .expect("answers poisoned")
            .pop_front()
            .ok_or_else(|| PortFailure::new("no answer staged"))
    }
}

/// A context policy that renders the turn it was handed, so a reader can see
/// which variant reached it.
#[derive(Default)]
struct Policy(Mutex<Vec<String>>);

impl ContextPolicy for Policy {
    async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
        let rendered = match turn {
            Turn::Initial { task } => format!("[initial] {task}"),
            Turn::Refinement { refinement } => format!("[refinement] {}", refinement.as_str()),
        };
        self.0
            .lock()
            .expect("turns poisoned")
            .push(rendered.clone());
        Ok(Prompt::new(Redacted::by(&NothingHeld, &rendered)))
    }
}

/// An iteration loop the outer loop can enter, implemented from outside.
struct Iterating(Mutex<Vec<String>>);

impl InnerLoop for Iterating {
    async fn iterate(&self, task: &str) -> Result<zaru_core::iteration::Outcome, PortFailure> {
        self.0.lock().expect("tasks poisoned").push(task.to_owned());
        Ok(zaru_core::iteration::Outcome::Succeeded {
            iterations: 2,
            total_elapsed: Duration::from_millis(30),
        })
    }
}

/// Prints every event, and keeps them so the check can read them back.
#[derive(Default)]
struct Printing(Vec<Event>);

impl EventSink for Printing {
    fn emit(&mut self, event: &Event) {
        println!("  event {event:?}");
        self.0.push(event.clone());
    }
}

fn descriptor(name: &str, purpose: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_owned(),
        description: purpose.to_owned(),
        parameters: String::new(),
    }
}

fn tokens() -> TokenUsage {
    TokenUsage {
        prompt: 12,
        completion: 5,
    }
}

/// The whole cycle, driven from outside: a read is requested, executed,
/// returned, and the model answers.
#[tokio::test]
async fn an_outside_caller_drives_a_turn_through_a_tool_call_to_an_answer() {
    println!("== a turn that calls a tool and then answers ==");
    let clock = Ticking::default();
    let model = Provider {
        script: Mutex::new(
            [
                ModelResponse::Calls {
                    text: String::new(),
                    echo: None,
                    calls: vec![ToolRequest {
                        id: String::from("c1"),
                        name: String::from("fs.read"),
                        arguments: String::from("src/main.rs"),
                    }],
                    tokens: tokens(),
                },
                ModelResponse::Text {
                    echo: None,
                    text: String::from("it reads the composition and exits 0"),
                    tokens: tokens(),
                },
            ]
            .into(),
        ),
        clock: &clock,
        can_call_tools: true,
    };
    let mut surface = Surface {
        descriptors: vec![
            descriptor("fs.read", "Read a file"),
            descriptor("cmd.run", "Execute a shell command"),
        ],
        answers: Mutex::new(
            [ToolOutcome::Completed {
                decision: ToolDecision {
                    statement: String::from("fs.read src/main.rs"),
                    permitted: true,
                },
                result: ToolResult {
                    id: String::from("c1"),
                    content: Redacted::by(&NothingHeld, "fn main() { println!(\"zaru\") }"),
                    failed: false,
                },
            }]
            .into(),
        ),
        clock: &clock,
    };
    let policy = Policy::default();
    let mut sink = Printing::default();

    let outcome = run::<_, _, _, _, _, Iterating>(
        1,
        Start::Task("read the entry point"),
        ToolCallCeiling::new(4).expect("a usable ceiling"),
        ToolCalling::required(&model, "staged-provider").expect("it can call tools"),
        Ports {
            model: &model,
            tools: &mut surface,
            context: &policy,
            clock: &clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut sink],
    )
    .await
    .expect("no port failed");

    println!("  outcome {outcome:?}");
    match outcome {
        Outcome::Answered { text, rounds, .. } => {
            assert_eq!(text, "it reads the composition and exits 0");
            assert_eq!(rounds, 2);
        }
        other => panic!("the model answered: {other:?}"),
    }
    assert_eq!(
        policy.0.lock().expect("turns poisoned").len(),
        1,
        "ADR-0013 D7: assembly happens at the turn boundary, once"
    );
}

/// ADR-0016's ruling, driven from outside: the user declines, the model is
/// told, and nothing failed.
#[tokio::test]
async fn a_refusal_reaches_the_model_and_the_turn_carries_on() {
    println!("== a turn whose tool call the user declines ==");
    let clock = Ticking::default();
    let model = Provider {
        script: Mutex::new(
            [
                ModelResponse::Calls {
                    text: String::new(),
                    echo: None,
                    calls: vec![ToolRequest {
                        id: String::from("c1"),
                        name: String::from("cmd.run"),
                        arguments: String::from("rm -rf /"),
                    }],
                    tokens: tokens(),
                },
                ModelResponse::Text {
                    echo: None,
                    text: String::from("understood, I will not run that"),
                    tokens: tokens(),
                },
            ]
            .into(),
        ),
        clock: &clock,
        can_call_tools: true,
    };
    let mut surface = Surface {
        descriptors: vec![descriptor("cmd.run", "Execute a shell command")],
        answers: Mutex::new(
            [ToolOutcome::Refused {
                decision: ToolDecision {
                    statement: String::from(
                        "Allow cmd.run rm -rf /  [matches a destructive pattern]?",
                    ),
                    permitted: false,
                },
                id: String::from("c1"),
                because: String::from("the user was asked about the call and did not permit it"),
            }]
            .into(),
        ),
        clock: &clock,
    };
    let policy = Policy::default();
    let mut sink = Printing::default();

    let outcome = run::<_, _, _, _, _, Iterating>(
        1,
        Start::Task("clean the workspace"),
        ToolCallCeiling::new(4).expect("a usable ceiling"),
        ToolCalling::required(&model, "staged-provider").expect("it can call tools"),
        Ports {
            model: &model,
            tools: &mut surface,
            context: &policy,
            clock: &clock,
            redactor: &NothingHeld,
        },
        None,
        &mut [&mut sink],
    )
    .await
    .expect("a declined prompt is not a failure, so the turn must not error");

    println!("  outcome {outcome:?}");
    assert!(
        matches!(outcome, Outcome::Answered { .. }),
        "the turn carried on after the refusal: {outcome:?}"
    );
    assert!(
        sink.0
            .iter()
            .any(|event| matches!(event, Event::ToolRefused { .. })),
        "the stream should say the call was refused"
    );
}

/// ADR-0009 D4, driven from outside: declared validators make the turn an
/// iteration, and no manifest makes it a tool cycle.
#[tokio::test]
async fn declared_validators_make_the_turn_an_iteration() {
    println!("== a turn with declared validators ==");
    let clock = Ticking::default();
    let model = Provider {
        script: Mutex::new(std::collections::VecDeque::new()),
        clock: &clock,
        can_call_tools: true,
    };
    let mut surface = Surface {
        descriptors: Vec::new(),
        answers: Mutex::new(std::collections::VecDeque::new()),
        clock: &clock,
    };
    let policy = Policy::default();
    let inner = Iterating(Mutex::new(Vec::new()));
    let mut sink = Printing::default();

    let outcome = run(
        3,
        Start::Task("make the tests pass"),
        ToolCallCeiling::new(2).expect("a usable ceiling"),
        ToolCalling::required(&model, "staged-provider").expect("it can call tools"),
        Ports {
            model: &model,
            tools: &mut surface,
            context: &policy,
            clock: &clock,
            redactor: &NothingHeld,
        },
        Some(&inner),
        &mut [&mut sink],
    )
    .await
    .expect("no port failed");

    println!("  outcome {outcome:?}");
    assert_eq!(
        inner.0.lock().expect("tasks poisoned").as_slice(),
        ["make the tests pass"]
    );
    assert!(
        matches!(outcome, Outcome::Iterated(_)),
        "the iteration loop was the turn's body: {outcome:?}"
    );
    assert!(
        sink.0.iter().any(|event| matches!(
            event,
            Event::TurnEnded {
                ending: TurnEnding::Iterated { .. },
                ..
            }
        )),
        "the stream should say the turn was an iteration"
    );
}

/// ADR-0012 clause 3's structural half, from outside: a model that cannot
/// call tools cannot be used to start a turn.
#[test]
fn a_model_that_cannot_call_tools_cannot_start_a_turn() {
    println!("== a provider with no tool calling ==");
    let clock = Ticking::default();
    let model = Provider {
        script: Mutex::new(std::collections::VecDeque::new()),
        clock: &clock,
        can_call_tools: false,
    };
    let refused = ToolCalling::required(&model, "local/tiny")
        .expect_err("a provider that cannot call tools must be refused");
    println!("  {refused}");
    assert_eq!(refused.model, "local/tiny");
    // `run` cannot be reached at all from here: it takes a `ToolCalling` and
    // there is no other way to make one. That is the clause's "not mid-loop"
    // as a property of what compiles.
}

/// A redactor holding nothing, which is therefore the identity.
///
/// Every outside caller has to supply one, because a `Prompt` can only be
/// built from text that has passed [ADR-0008] clause 6's port — which is the
/// whole point of that type. It is declared here rather than shared because
/// an integration test cannot see another crate's test tree and [ADR-0003] D8
/// forbids the dependency that would let it, the same cost `zaru-cli`'s
/// Nuclear Notes fixture server already pays.
///
/// Holding nothing is also the **discriminating** arm: a check asserting that
/// a value is absent from a prompt is worthless unless the same run with
/// nothing held carries that value through byte for byte.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}
