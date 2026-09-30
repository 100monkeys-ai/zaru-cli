// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The retry loop, the stall clock, and the figures both run under.
//!
//! # What these reach, and what they deliberately do not
//!
//! **No listener serves a provider's responses here**, which is the standing
//! ruling of 2026-09-05: a fake of a provider at the wire is the mock the
//! testing page refuses, and the seam is a staged port at the library level.
//! So the loop is driven through [`Exchanging`] with a staged attempt, the way
//! a turn drives it through [`crate::providers::ProviderClient`]; the stall
//! clock is driven over a staged body that arrives through a channel; and each
//! client's own use of both is held by reading its source, the instrument
//! `every_client_composes_its_transport_failures_the_same_way` already uses
//! for the three arms no check can drive.
//!
//! **Time is staged for the loop and real for the stall clock.** The loop's
//! waits are seconds long and are asserted by value, so its clock records a
//! pause instead of taking it; the stall clock is `tokio`'s own timer, and it
//! is measured in tens of milliseconds, which is what it would be measured in
//! by a person too if a person could see a millisecond.

use super::*;
use crate::cli::Overrides;
use crate::cli::layers::{self, Files};
use core::time::Duration;
use std::sync::Mutex;
use zaru_core::tool_call::{Capabilities, ModelRequest, ModelResponse, TokenUsage};

/// A clock that records each pause and moves itself on by it.
struct Staged {
    base: Instant,
    moved: Mutex<Duration>,
    paused: Mutex<Vec<Duration>>,
}

impl Staged {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            moved: Mutex::new(Duration::ZERO),
            paused: Mutex::new(Vec::new()),
        }
    }

    /// Time passing inside an attempt.
    fn spend(&self, spent: Duration) {
        *self.moved.lock().expect("not poisoned") += spent;
    }

    fn pauses(&self) -> Vec<Duration> {
        self.paused.lock().expect("not poisoned").clone()
    }
}

impl Clock for Staged {
    fn now(&self) -> Instant {
        self.base + *self.moved.lock().expect("not poisoned")
    }

    fn pause(&self, wait: Duration) -> impl Future<Output = ()> + Send {
        self.paused.lock().expect("not poisoned").push(wait);
        self.spend(wait);
        core::future::ready(())
    }
}

/// Jitter that is always the same, held to what the policy allows.
struct Fixed(Duration);

impl Jitter for Fixed {
    fn up_to(&self, most: Duration) -> Duration {
        self.0.min(most)
    }
}

/// A staged failure that says whether a retry could change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Failed(Option<Transient>);

impl Transience for Failed {
    fn transient(&self) -> Option<Transient> {
        self.0
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a staged failure: {:?}", self.0)
    }
}

const fn status(code: u16) -> Failed {
    Failed(Some(Transient {
        cause: Cause::Status(code),
        retry_after: None,
    }))
}

const fn stalled(silent: Duration) -> Failed {
    Failed(Some(Transient {
        cause: Cause::Stalled(silent),
        retry_after: None,
    }))
}

const REFUSED: Failed = Failed(None);

fn answer(text: &str) -> ModelResponse {
    ModelResponse::Text {
        echo: None,
        text: text.to_owned(),
        tokens: TokenUsage {
            prompt: 3,
            completion: 2,
        },
    }
}

/// A staged provider: each attempt takes the next staged outcome, spends the
/// staged time on the clock, and records what budget it was handed.
struct Script<'a> {
    clock: &'a Staged,
    outcomes: Mutex<Vec<(Duration, Result<ModelResponse, Failed>)>>,
    handed: Mutex<Vec<Attempt>>,
}

impl<'a> Script<'a> {
    fn of(clock: &'a Staged, outcomes: Vec<(Duration, Result<ModelResponse, Failed>)>) -> Self {
        Self {
            clock,
            outcomes: Mutex::new(outcomes.into_iter().rev().collect()),
            handed: Mutex::new(Vec::new()),
        }
    }

    fn attempts(&self) -> Vec<Attempt> {
        self.handed.lock().expect("not poisoned").clone()
    }
}

impl Exchanging for Script<'_> {
    type Failure = Failed;

    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    fn attempt(
        &self,
        _request: &ModelRequest<'_>,
        attempt: Attempt,
    ) -> impl Future<Output = Result<ModelResponse, Failed>> + Send {
        self.handed.lock().expect("not poisoned").push(attempt);
        let (spent, outcome) = self
            .outcomes
            .lock()
            .expect("not poisoned")
            .pop()
            .expect("staging: the provider was asked more often than it was staged");
        self.clock.spend(spent);
        core::future::ready(outcome)
    }
}

fn a_prompt() -> zaru_core::iteration::Prompt {
    zaru_core::iteration::Prompt::new(zaru_core::redaction::Redacted::by(
        &crate::redaction::HeldSecrets::none(),
        "say ok",
    ))
}

fn secs(count: u64) -> Duration {
    Duration::from_secs(count)
}

// --- The figures -------------------------------------------------------------

/// The ruled figures are the defaults, and every one of them is a key.
///
/// 60 s of silence is a stall; three retries; waits of 1, 2 and 4 seconds each
/// plus up to 1 s of jitter; a `Retry-After` honoured when it asks for longer;
/// no wait past 30 s; and the whole exchange inside the 600 s ceiling. Read
/// through layer 1 as the binary folds it, so a default that lived only in
/// this module and never reached `zaru config explain` would fail here.
#[test]
fn the_ruled_figures_are_layer_ones_and_each_is_a_key() {
    let resolution =
        layers::resolve(&Overrides::default(), [], &Files::none()).expect("layer 1 alone folds");
    let mut wrong = Vec::new();
    for (name, default, _) in KEYS {
        let explained = resolution.explain(&key(name));
        let from_layer_one = explained.rows.iter().any(|row| {
            row.layer == crate::config::Layer::BuiltIn
                && row.value == Some(Value::Integer(i64::from(default)))
        });
        if !from_layer_one {
            wrong.push(format!(
                "`{name}` is not {default} in layer 1, so `zaru config explain` cannot show it"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let policy = Policy::from_configuration(&resolution).expect("the defaults are counts");
    assert_eq!(policy, Policy::built_in());
    assert_eq!(policy.stall(), secs(60), "a stall is 60 s of silence");
    assert_eq!(policy.retries(), 3, "three retries at most");
    assert_eq!(
        policy.budget(),
        super::super::transport::EXCHANGE_TIMEOUT,
        "the whole exchange is inside the exchange ceiling"
    );
    let waits = |jitter: Duration| -> Vec<Duration> {
        (1..=3)
            .map(|retry| policy.wait_before(retry, None, jitter))
            .collect()
    };
    assert_eq!(waits(Duration::ZERO), [secs(1), secs(2), secs(4)]);
    assert_eq!(
        waits(secs(1)),
        [secs(2), secs(3), secs(5)],
        "up to a second of jitter on each"
    );
    assert_eq!(
        waits(secs(9)),
        [secs(2), secs(3), secs(5)],
        "jitter is held to the policy's, whatever the source drew"
    );
    assert_eq!(
        policy.wait_before(1, Some(secs(20)), Duration::ZERO),
        secs(20),
        "a Retry-After longer than the backoff is honoured"
    );
    assert_eq!(
        policy.wait_before(3, Some(secs(1)), Duration::ZERO),
        secs(4),
        "a Retry-After shorter than the backoff does not shorten it"
    );
    assert_eq!(
        policy.wait_before(1, Some(secs(90)), Duration::ZERO),
        secs(30),
        "no wait is longer than 30 s, a Retry-After's included"
    );
    assert_eq!(
        policy.wait_before(6, None, Duration::ZERO),
        secs(30),
        "the doubling is held to the ceiling"
    );
}

/// A layer above sets each figure, and a value that is not a count is refused
/// naming its key.
#[test]
fn a_layer_sets_each_figure_and_a_value_that_is_not_a_count_is_refused() {
    let resolved = |pairs: &[(&str, &str)]| {
        layers::resolve(
            &Overrides::default(),
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<Vec<_>>(),
            &Files::none(),
        )
        .expect("the layers fold")
    };
    let set = resolved(&[
        ("ZARU_PROVIDER_STALL_SECONDS", "120"),
        ("ZARU_PROVIDER_RETRIES", "5"),
        ("ZARU_PROVIDER_RETRY_BACKOFF_SECONDS", "2"),
        ("ZARU_PROVIDER_RETRY_JITTER_SECONDS", "0"),
        ("ZARU_PROVIDER_RETRY_WAIT_CEILING_SECONDS", "10"),
    ]);
    let policy = Policy::from_configuration(&set).expect("each is a count");
    assert_eq!(policy.stall(), secs(120));
    assert_eq!(policy.retries(), 5);
    assert_eq!(policy.jitter(), Duration::ZERO, "no jitter is allowed");
    assert_eq!(
        (1..=4)
            .map(|retry| policy.wait_before(retry, None, secs(1)))
            .collect::<Vec<_>>(),
        [secs(2), secs(4), secs(8), secs(10)]
    );

    for (variable, name) in [
        ("ZARU_PROVIDER_RETRIES", RETRIES_KEY),
        ("ZARU_PROVIDER_STALL_SECONDS", STALL_KEY),
        ("ZARU_PROVIDER_RETRY_BACKOFF_SECONDS", BACKOFF_KEY),
    ] {
        let refused = Policy::from_configuration(&resolved(&[(variable, "0")]))
            .expect_err("zero bounds nothing");
        assert_eq!(refused.key().as_str(), name);
        assert!(
            refused.to_string().contains(name),
            "the refusal names its key: {refused}"
        );
    }
}

/// A `Retry-After` is read in both of RFC 9110's forms.
#[test]
fn a_retry_after_is_read_as_seconds_and_as_a_date() {
    let now = UNIX_EPOCH + secs(784_111_747); // Sun, 06 Nov 1994 08:49:07 GMT
    assert_eq!(retry_after("120", now), Some(secs(120)));
    assert_eq!(retry_after(" 7 ", now), Some(secs(7)));
    assert_eq!(
        retry_after("Sun, 06 Nov 1994 08:49:37 GMT", now),
        Some(secs(30)),
        "RFC 9110's own example date, thirty seconds on"
    );
    assert_eq!(
        retry_after("Sun, 06 Nov 1994 08:48:37 GMT", now),
        Some(Duration::ZERO),
        "a date already past is no wait"
    );
    for nonsense in ["soon", "-3", "", "Sun, 06 Nov 1994 08:49:37 PST"] {
        assert_eq!(retry_after(nonsense, now), None, "{nonsense:?}");
    }
}

// --- The loop ------------------------------------------------------------------

/// A transient failure is retried until it answers, and each retry is told
/// before its wait, in words that say which, of how many, how long and why.
#[tokio::test]
async fn a_transient_failure_is_retried_until_it_answers_and_each_retry_is_told() {
    let clock = Staged::new();
    let script = Script::of(
        &clock,
        vec![
            (secs(1), Err(status(503))),
            (secs(60), Err(stalled(secs(60)))),
            (
                secs(1),
                Err(Failed(Some(Transient {
                    cause: Cause::Connection,
                    retry_after: None,
                }))),
            ),
            (secs(1), Ok(answer("ok"))),
        ],
    );
    let told = Mutex::new(Vec::new());
    let tell = |retrying: &Retrying| {
        told.lock()
            .expect("not poisoned")
            .push(retrying.to_string())
    };
    let mut policy = Policy::built_in();
    policy.retries = RetryCeiling::new(3).expect("three");
    let resilient = Resilient::timed(&script, policy, &clock, Fixed(Duration::ZERO), &tell);

    let answered = {
        let prompt = a_prompt();
        resilient
            .exchange(&ModelRequest {
                prompt: &prompt,
                tools: &[],
                turn: &[],
            })
            .await
    };

    assert_eq!(answered, Ok(answer("ok")), "the fourth attempt answered");
    assert_eq!(
        told.into_inner().expect("not poisoned"),
        [
            "retry 1 of 3 in 1 s after HTTP 503",
            "retry 2 of 3 in 2 s: the provider stalled, sending nothing for 60 s",
            "retry 3 of 3 in 4 s: the connection to the provider failed",
        ],
        "each retry is one line, labelled, counted, bounded, with its wait and its cause"
    );
    assert_eq!(clock.pauses(), [secs(1), secs(2), secs(4)]);
}

/// A failure a retry cannot change is returned at once, with nothing told.
#[tokio::test]
async fn a_failure_no_retry_can_change_is_returned_at_once() {
    let clock = Staged::new();
    let script = Script::of(&clock, vec![(secs(1), Err(REFUSED))]);
    let told = Mutex::new(Vec::<String>::new());
    let tell = |retrying: &Retrying| {
        told.lock()
            .expect("not poisoned")
            .push(retrying.to_string())
    };
    let resilient = Resilient::timed(
        &script,
        Policy::built_in(),
        &clock,
        Fixed(Duration::ZERO),
        &tell,
    );
    let failed = {
        let prompt = a_prompt();
        resilient
            .exchange(&ModelRequest {
                prompt: &prompt,
                tools: &[],
                turn: &[],
            })
            .await
    };
    assert_eq!(failed, Err(REFUSED));
    assert_eq!(script.attempts().len(), 1, "a refusal is asked once");
    assert!(told.into_inner().expect("not poisoned").is_empty());
    assert!(clock.pauses().is_empty());
}

/// Retries stop at the bound, and what is reported is the last failure.
#[tokio::test]
async fn retries_stop_at_the_bound_and_the_last_failure_is_reported() {
    let clock = Staged::new();
    let script = Script::of(
        &clock,
        vec![
            (secs(1), Err(status(429))),
            (secs(1), Err(status(429))),
            (secs(1), Err(status(502))),
            (secs(1), Err(status(503))),
        ],
    );
    let told = Mutex::new(Vec::<String>::new());
    let tell = |retrying: &Retrying| {
        told.lock()
            .expect("not poisoned")
            .push(retrying.to_string())
    };
    let resilient = Resilient::timed(
        &script,
        Policy::built_in(),
        &clock,
        Fixed(Duration::ZERO),
        &tell,
    );
    let failed = {
        let prompt = a_prompt();
        resilient
            .exchange(&ModelRequest {
                prompt: &prompt,
                tools: &[],
                turn: &[],
            })
            .await
    };
    assert_eq!(failed, Err(status(503)), "the last attempt's failure");
    assert_eq!(script.attempts().len(), 4, "one attempt and three retries");
    assert_eq!(told.into_inner().expect("not poisoned").len(), 3);
}

/// No retry extends the exchange's ceiling: each attempt is handed what is
/// left of it, and a wait that would end past it is not taken.
#[tokio::test]
async fn no_retry_extends_the_exchanges_ceiling() {
    let clock = Staged::new();
    let script = Script::of(
        &clock,
        vec![(secs(4), Err(status(500))), (secs(4), Err(status(500)))],
    );
    let told = Mutex::new(Vec::<String>::new());
    let tell = |retrying: &Retrying| {
        told.lock()
            .expect("not poisoned")
            .push(retrying.to_string())
    };
    let mut policy = Policy::built_in();
    policy.budget = secs(10);
    let resilient = Resilient::timed(&script, policy, &clock, Fixed(Duration::ZERO), &tell);
    let failed = {
        let prompt = a_prompt();
        resilient
            .exchange(&ModelRequest {
                prompt: &prompt,
                tools: &[],
                turn: &[],
            })
            .await
    };
    assert_eq!(failed, Err(status(500)));
    assert_eq!(
        script
            .attempts()
            .iter()
            .map(|attempt| attempt.budget)
            .collect::<Vec<_>>(),
        [secs(10), secs(5)],
        "the second attempt is handed 10 s less the 4 s spent and the 1 s waited"
    );
    assert_eq!(
        told.into_inner().expect("not poisoned").len(),
        1,
        "the second wait, 2 s from 9 s spent, would end past 10 s and is not taken"
    );
}

/// A retry is not an exchange: the loop counts what the model was asked.
///
/// A turn whose limit is one exchange answers after two failed attempts,
/// because the retries are below the model port. Driven through
/// `tool_call::run` itself, the thing that counts, with the resilient model as
/// its only model.
#[tokio::test]
async fn a_retry_is_not_an_exchange_against_the_turns_limit() {
    use zaru_core::iteration::{Clock as LoopClock, ContextPolicy, ContextRefusal, Prompt, Turn};
    use zaru_core::tool_call::{
        EventSink, InnerLoop, Outcome, Ports, Start, ToolCallCeiling, ToolCalling, ToolExecutor,
        run,
    };

    struct NeverIterates;
    impl InnerLoop for NeverIterates {
        async fn iterate(
            &self,
            _task: &str,
        ) -> Result<zaru_core::iteration::Outcome, zaru_core::iteration::PortFailure> {
            unreachable!("staging: no validators are declared")
        }
    }
    struct Unused;
    impl ToolExecutor for Unused {
        fn descriptors(&self) -> &[zaru_core::tool_call::ToolDescriptor] {
            &[]
        }

        async fn execute(
            &mut self,
            request: &zaru_core::tool_call::ToolRequest,
        ) -> Result<zaru_core::tool_call::ToolOutcome, zaru_core::iteration::PortFailure> {
            panic!("staging: the answer calls no tool, and {request:?} was asked for");
        }
    }
    struct Whole;
    impl ContextPolicy for Whole {
        async fn assemble(&self, turn: &Turn<'_>) -> Result<Prompt, ContextRefusal> {
            let said = match turn {
                Turn::Initial { task } => (*task).to_owned(),
                Turn::Refinement { refinement } => refinement.as_str().to_owned(),
            };
            Ok(Prompt::new(zaru_core::redaction::Redacted::by(
                &crate::redaction::HeldSecrets::none(),
                &said,
            )))
        }
    }
    struct Still;
    impl LoopClock for Still {
        fn now(&self) -> Duration {
            Duration::ZERO
        }
    }
    struct Nothing;
    impl EventSink for Nothing {
        fn emit(&mut self, _event: &zaru_core::tool_call::Event) {}
    }

    let clock = Staged::new();
    let script = Script::of(
        &clock,
        vec![
            (secs(1), Err(status(503))),
            (secs(1), Err(status(429))),
            (secs(1), Ok(answer("answered"))),
        ],
    );
    let told = Mutex::new(Vec::<String>::new());
    let tell = |retrying: &Retrying| {
        told.lock()
            .expect("not poisoned")
            .push(retrying.to_string())
    };
    let resilient = Resilient::timed(
        &script,
        Policy::built_in(),
        &clock,
        Fixed(Duration::ZERO),
        &tell,
    );
    let mut nothing = Nothing;
    let outcome = run::<_, _, _, _, _, NeverIterates>(
        1,
        Start::Task("say ok"),
        ToolCallCeiling::new(1).expect("a limit of one exchange"),
        ToolCalling::required(&resilient, "staged").expect("it calls tools"),
        Ports {
            model: &resilient,
            tools: &mut Unused,
            context: &Whole,
            clock: &Still,
            redactor: &crate::redaction::HeldSecrets::none(),
        },
        None,
        &mut [&mut nothing],
    )
    .await
    .expect("no port failed: the two failures were retried below the port");
    assert!(
        matches!(outcome, Outcome::Answered { .. }),
        "a turn limited to one exchange answered after two retries, or a retry was counted as an \
         exchange: {outcome:?}"
    );
    assert_eq!(script.attempts().len(), 3);
    assert_eq!(told.into_inner().expect("not poisoned").len(), 2);
}

/// Every line a retry is told in says `retry` and never `iteration`.
///
/// ADR-0016 D4: "labelled `retry`, never `iteration`". The two words name
/// different acts -- a retry repeats hoping for a different outcome, an
/// iteration changes the attempt because of the failure -- and a person reading
/// one where the other happened is being told something false.
#[test]
fn a_retry_is_labelled_a_retry_and_never_an_iteration() {
    for cause in [
        Cause::Status(429),
        Cause::Stalled(secs(60)),
        Cause::Connection,
    ] {
        let line = Retrying {
            cause,
            retry: 2,
            of: 3,
            wait: Duration::from_millis(2_400),
        }
        .to_string();
        assert!(line.starts_with("retry 2 of 3 in 2.4 s"), "{line}");
        assert!(!line.contains("iteration"), "{line}");
    }
}

// --- The stall clock -------------------------------------------------------------

/// A slow stream that keeps flowing is not a stall, and a silent one is.
///
/// A staged body: pieces arrive through a channel with a pause before each.
/// Twelve pieces 20 ms apart take 240 ms, three times the 80 ms stall bound,
/// and are read whole; the same body with one 250 ms silence is abandoned at
/// that silence, having read what came before it.
#[tokio::test]
async fn a_slow_stream_that_keeps_flowing_is_not_a_stall_and_a_silent_one_is() {
    async fn read(gaps: Vec<u64>, stall: Duration) -> (usize, Option<Stalled>) {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Vec<u8>>(1);
        let body = tokio::spawn(async move {
            for gap in gaps {
                tokio::time::sleep(Duration::from_millis(gap)).await;
                if sender.send(vec![b'x']).await.is_err() {
                    return;
                }
            }
        });
        let mut read = 0;
        let stopped = loop {
            match within_stall(stall, receiver.recv()).await {
                Ok(Some(piece)) => read += piece.len(),
                Ok(None) => break None,
                Err(stalled) => break Some(stalled),
            }
        };
        body.abort();
        (read, stopped)
    }

    let stall = Duration::from_millis(80);
    let flowing = read(vec![20; 12], stall).await;
    assert_eq!(
        flowing,
        (12, None),
        "a stream whose every gap is inside the bound is read whole, however long it takes"
    );

    let mut gaps = vec![20; 5];
    gaps.push(250);
    gaps.extend([20; 5]);
    let silent = read(gaps, stall).await;
    assert_eq!(
        silent,
        (5, Some(Stalled { silent_for: stall })),
        "a silence past the bound is a stall, after what arrived before it"
    );
}

/// Every client reads every piece of its stream under the stall clock, hands
/// each attempt's budget to the request, and keeps a `Retry-After`.
///
/// Read from the source, for the reason the transport check gives: the chunk
/// arm of a client is reachable only by a server that answers and then stops,
/// and no such server may exist in this suite.
#[test]
fn every_client_reads_under_the_stall_clock_and_the_attempts_budget() {
    const CLIENTS: [(&str, &str); 3] = [
        ("gemini", include_str!("../gemini.rs")),
        ("ollama", include_str!("../ollama.rs")),
        ("openai_compatible", include_str!("../openai_compatible.rs")),
    ];
    let mut wrong = Vec::new();
    for (kind, source) in CLIENTS {
        let chunks = source.matches(".chunk()").count();
        let stalled = source
            .matches("within_stall(attempt.stall, response.chunk())")
            .count();
        if chunks != 1 || stalled != 1 {
            wrong.push(format!(
                "the {kind} client reads {chunks} chunk(s), {stalled} of them under the stall \
                 clock; it must read its one stream under it"
            ));
        }
        if source.matches(".timeout(attempt.budget)").count() != 1 {
            wrong.push(format!(
                "the {kind} client does not bound its request by what is left of the exchange's \
                 ceiling, so a retry could extend it"
            ));
        }
        if source.matches("retry_after_of(response.headers())").count() != 1 {
            wrong.push(format!("the {kind} client does not keep a Retry-After"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// --- Which failures are transient ------------------------------------------------

/// Each kind says, of each of its failures, whether a retry could change it:
/// a connection, a stall, 408, 429 and 5xx could, and nothing else.
#[test]
fn every_kinds_failures_say_whether_a_retry_could_change_them() {
    use crate::providers::capacity::Refused;
    use crate::providers::{GeminiFailure as G, OllamaFailure as O, OpenAiCompatibleFailure as C};

    let endpoint = crate::providers::ProviderEndpoint::new("http://127.0.0.1:1")
        .expect("a well-formed origin");
    let asked = Some(secs(7));
    let connection = Some(Cause::Connection);
    // A case: its name, what the failure said, and the cause and wait owed.
    type Case = (
        &'static str,
        Option<Transient>,
        Option<Cause>,
        Option<Duration>,
    );
    let cases: Vec<Case> = vec![
        (
            "gemini unreachable",
            G::Unavailable {
                code: None,
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            connection,
            None,
        ),
        (
            "gemini 429 with Retry-After",
            G::Unavailable {
                code: Some(429),
                detail: String::new(),
                retry_after: asked,
            }
            .transient(),
            Some(Cause::Status(429)),
            asked,
        ),
        (
            "gemini 503",
            G::Unavailable {
                code: Some(503),
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            Some(Cause::Status(503)),
            None,
        ),
        (
            "gemini stream broken after 200",
            G::Unavailable {
                code: Some(200),
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            connection,
            None,
        ),
        (
            "gemini non-envelope 400",
            G::Unavailable {
                code: Some(400),
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            None,
            None,
        ),
        (
            "gemini stalled",
            G::Stalled {
                silent_for: secs(60),
            }
            .transient(),
            Some(Cause::Stalled(secs(60))),
            None,
        ),
        (
            "gemini malformed request",
            G::RequestRefused {
                code: 400,
                status: String::new(),
                detail: String::new(),
            }
            .transient(),
            None,
            None,
        ),
        (
            "gemini missing model",
            G::ModelNotFound {
                model: String::new(),
                detail: String::new(),
            }
            .transient(),
            None,
            None,
        ),
        (
            "gemini capacity",
            G::CapacityRefused(Refused {
                code: 400,
                status: None,
                detail: String::new(),
            })
            .transient(),
            None,
            None,
        ),
        (
            "ollama unreachable",
            O::Unreachable {
                endpoint: endpoint.clone(),
                detail: String::new(),
            }
            .transient(),
            connection,
            None,
        ),
        (
            "ollama 503 with Retry-After",
            O::Unavailable {
                code: 503,
                detail: String::new(),
                retry_after: asked,
            }
            .transient(),
            Some(Cause::Status(503)),
            asked,
        ),
        (
            "ollama 403",
            O::Unavailable {
                code: 403,
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            None,
            None,
        ),
        (
            "ollama stalled",
            O::Stalled {
                silent_for: secs(60),
            }
            .transient(),
            Some(Cause::Stalled(secs(60))),
            None,
        ),
        (
            "ollama bad request",
            O::RequestRefused {
                code: 400,
                detail: String::new(),
            }
            .transient(),
            None,
            None,
        ),
        (
            "openai-compatible unreachable",
            C::Unreachable {
                endpoint,
                detail: String::new(),
            }
            .transient(),
            connection,
            None,
        ),
        (
            "openai-compatible 408",
            C::Unavailable {
                code: Some(408),
                detail: String::new(),
                retry_after: None,
            }
            .transient(),
            Some(Cause::Status(408)),
            None,
        ),
        (
            "openai-compatible error frame 500",
            C::StreamFailed {
                code: Some(500),
                detail: String::new(),
            }
            .transient(),
            Some(Cause::Status(500)),
            None,
        ),
        (
            "openai-compatible error frame with no code",
            C::StreamFailed {
                code: None,
                detail: String::new(),
            }
            .transient(),
            None,
            None,
        ),
        (
            "openai-compatible stalled",
            C::Stalled {
                silent_for: secs(60),
            }
            .transient(),
            Some(Cause::Stalled(secs(60))),
            None,
        ),
        (
            "openai-compatible 422",
            C::RequestRefused {
                code: 422,
                detail: String::new(),
            }
            .transient(),
            None,
            None,
        ),
    ];
    let mut wrong = Vec::new();
    for (name, got, cause, retry_after) in cases {
        let expected = cause.map(|cause| Transient { cause, retry_after });
        if got != expected {
            wrong.push(format!("{name}: {got:?}, where {expected:?} was owed"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A stall is the provider's condition and shows how long it waited, and a
/// failure that was retried says so where the person reads the refusal.
#[test]
fn a_stall_is_environmental_and_a_retried_failure_says_it_was_retried() {
    use crate::cli::classify::Surface;
    use crate::failure::{Class, Presentation, SessionEvidence};
    use crate::providers::{
        GeminiFailure, OllamaFailure, OpenAiCompatibleFailure, ProviderFailure,
    };

    let surface = Surface::new("0.0.0", "https://example.invalid/report");
    let shown = |failure: ProviderFailure| {
        Presentation::of(&surface.provider_failure(&failure, SessionEvidence::NoSessionExists))
    };
    let mut wrong = Vec::new();
    for failure in [
        ProviderFailure::Gemini(GeminiFailure::Stalled {
            silent_for: secs(60),
        }),
        ProviderFailure::Ollama(OllamaFailure::Stalled {
            silent_for: secs(60),
        }),
        ProviderFailure::OpenAiCompatible(OpenAiCompatibleFailure::Stalled {
            silent_for: secs(60),
        }),
    ] {
        let presented = shown(failure);
        let said = presented.to_string();
        if presented.class != Class::Environmental {
            wrong.push(format!("a stall is {:?}: {said}", presented.class));
        }
        for needed in ["sent nothing for 60 s", "provider.retries"] {
            if !said.contains(needed) {
                wrong.push(format!("a stall does not say {needed:?}: {said}"));
            }
        }
        if said.contains("no retry policy") {
            wrong.push(format!(
                "a stall still says there is no retry policy: {said}"
            ));
        }
    }
    let busy = shown(ProviderFailure::Gemini(GeminiFailure::Unavailable {
        code: Some(503),
        detail: "the model is overloaded".to_owned(),
        retry_after: None,
    }))
    .to_string();
    if !busy.contains("provider.retries") || busy.contains("no retry policy") {
        wrong.push(format!("a retried 503 does not say it was retried: {busy}"));
    }
    let refused = shown(ProviderFailure::Ollama(OllamaFailure::Unavailable {
        code: 403,
        detail: "forbidden".to_owned(),
        retry_after: None,
    }))
    .to_string();
    if !refused.contains("a retry cannot change this answer") {
        wrong.push(format!(
            "a 403 does not say a retry cannot change it: {refused}"
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
