// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0013] D2's compaction, driven from outside both crates.
//!
//! # Why this file exists rather than more checks inside the crate
//!
//! A composition asserted from inside its own crate can reach private doors
//! nobody else can, so it can pass while the thing a caller would actually do
//! is impossible. Everything here goes through the public surface: a real
//! credential store on disk, a real `SessionContext`, the real
//! `ModelSummariser`, and a real transcript file.
//!
//! # What the security cases are about
//!
//! [ADR-0008] trigger clause 6 puts one `Redactor` on "every path from
//! captured bytes into a model prompt **or request**", and a compaction is a
//! request: it sends a span of layer 6 to a provider on its own. Layer 6 is
//! [ADR-0013] D1's "conversation **and tool results**", so whatever a tool
//! printed is in that span — and a tool that printed a bearer value has put
//! it there.
//!
//! Every absence arm is asserted **by value and by ASCII core**, which is
//! [Verification lessons] §63: `{:?}` escapes a combining mark, so a
//! value-only assertion is blind to a rendering that published every byte.
//! And every absence case has an **accepting sibling** that plants the same
//! value with nothing held and finds it, so the absence cannot pass
//! vacuously.
//!
//! The inverse is asserted too: [ADR-0010] D2's transcript keeps the raw span
//! **unredacted**, because that record's Negative section says the file
//! "contains whatever the session contained" and ADR-0008's decision says in
//! as many words that "the transcript is untouched". Redaction is on what a
//! model reads, not on the record, and the difference is checked rather than
//! assumed.
//!
//! # The one check that reaches a network, and how it is gated
//!
//! [Testing] forbids a check calling a provider, and the CI runner has no key.
//! So [`RUN_VARIABLE`] gates the real exchange exactly as
//! `provider_from_outside.rs` gates its own, and when it skips it **prints
//! why** — a gated check that passed silently would be indistinguishable from
//! one that ran.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-3

use std::sync::Mutex;
use zaru_cli::compose::{ModelSummariser, SessionContext, prefix_for};
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, HarnessKeys, Instance, KeyStore, OsKeyring, Reach,
    SealingError, SealingKey, Secret, ToolScope,
};
use zaru_cli::providers::ProviderKind;
use zaru_cli::providers::gemini::{Endpoint, GeminiClient};
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction};
use zaru_cli::session::{Record, Transcript};
use zaru_core::context::{ContextLimits, ContextWindow, PressureThreshold};
use zaru_core::conversation::Message;
use zaru_core::iteration::PortFailure;
use zaru_core::redaction::Redactor;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse, TokenUsage};

/// The variable that says a key is in a store on this machine.
const RUN_VARIABLE: &str = "ZARU_GEMINI_EXCHANGE";

/// The awkward tail every planted value carries, so the ASCII-core arm has
/// something to be a *different* arm about.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

/// A value shaped like a personal bearer token and authenticating nothing.
fn planted_bearer(label: &str) -> String {
    format!(
        "nn_mcp_{label}-{}-{}{AWKWARD_TAIL}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

/// Everything before the first non-ASCII character: what survives `{:?}`.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// The key port, implemented out here because no product tree implements it.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A directory this check owns.
struct Scratch {
    base: std::path::PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(format!(
                "cs-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_nanos()
            ));
        std::fs::create_dir_all(&base).expect("staging: the scratch root");
        Self { base }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A store holding one secret, and the redactor built from what it holds.
fn holding(scratch: &Scratch, value: &str) -> HeldSecrets {
    let keys = StagedKey(SealingKey::mint());
    let mut store =
        CredentialStore::open(scratch.base.join("zaru")).expect("the credential store opens");
    let alias = Alias::new("planted").expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias,
        Description::new("the token this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    held_secrets_for_redaction(&store, &keys).expect("the store reopens what it sealed")
}

/// A provider that answers one sentence and keeps every prompt it was given.
struct Recording {
    seen: Mutex<Vec<String>>,
    tools: Mutex<Vec<usize>>,
}

impl Recording {
    fn new() -> Self {
        Self {
            seen: Mutex::new(Vec::new()),
            tools: Mutex::new(Vec::new()),
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.seen.lock().expect("no panic holds this").clone()
    }
}

impl Model for Recording {
    fn capabilities(&self) -> Capabilities {
        Capabilities { tool_calling: true }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        self.seen
            .lock()
            .expect("no panic holds this")
            .push(request.prompt.rendered());
        self.tools
            .lock()
            .expect("no panic holds this")
            .push(request.tools.len());
        Ok(ModelResponse::Text {
            echo: None,
            text: "they agreed on four spaces".to_owned(),
            tokens: TokenUsage {
                prompt: 900,
                completion: 20,
            },
        })
    }
}

/// Limits tight enough that a handful of staged exchanges crosses them.
fn tight() -> ContextLimits {
    ContextLimits::new(
        ContextWindow::new(8_000).expect("not zero"),
        PressureThreshold::new(900).expect("not zero"),
    )
    .expect("the threshold is below the window")
}

/// One turn's records, as the transcript holds them: what the person asked
/// and the loop's messages.
fn turn_records(n: u32, messages: Vec<Message>) -> Vec<Record> {
    let mut records = Vec::with_capacity(1 + messages.len());
    if let Some(Message::User { text }) = messages.first() {
        records.push(Record::Conversation(zaru_cli::session::Utterance {
            n,
            voice: zaru_cli::session::Voice::User,
            text: text.clone(),
        }));
    }
    for message in messages {
        records.push(Record::TurnLoop(zaru_core::tool_call::Event::Message(
            message,
        )));
    }
    records
}

/// A turn that read a file and answered.
fn a_read(n: u32, task: &str, result: &str, said: &str) -> Vec<Record> {
    turn_records(
        n,
        vec![
            Message::User {
                text: task.to_owned(),
            },
            Message::Assistant {
                text: String::new(),
                calls: vec![zaru_core::tool_call::ToolRequest {
                    id: format!("call_{n}"),
                    name: "fs.read".to_owned(),
                    arguments: r#"{"path":"notes.txt"}"#.to_owned(),
                }],
                echo: None,
            },
            // A tool result is where a captured secret actually arrives, which
            // is the whole reason layer 6 is a redaction path at all.
            Message::Tool {
                id: format!("call_{n}"),
                name: "fs.read".to_owned(),
                content: result.to_owned(),
                failed: false,
            },
            Message::Assistant {
                text: said.to_owned(),
                calls: Vec::new(),
                echo: None,
            },
        ],
    )
}

/// A turn that is one message from the person.
fn said(n: u32, text: String) -> Vec<Record> {
    turn_records(n, vec![Message::User { text }])
}

/// The records of enough layer 6 to cross the threshold, with `planted`
/// inside it.
fn carrying_records(planted: &str) -> Vec<Record> {
    let mut records = a_read(
        0,
        "read the deploy notes",
        &format!("the token is {planted}"),
        "the notes name a token",
    );
    for nth in 0..6 {
        records.extend(said(
            nth + 1,
            format!("exchange {nth}: {}", "detail ".repeat(30)),
        ));
    }
    records
}

/// Stage enough layer 6 to cross the threshold, with `planted` inside it,
/// rebuilt from records as a session's is.
fn session_carrying(planted: &str) -> SessionContext {
    let mut session = SessionContext::opened(
        prefix_for(None, &facts()),
        zaru_cli::compose::ContextShape::of(tight(), 0, one_token_a_byte()),
    );
    session.rebuild_from(&carrying_records(planted));
    session
}

/// Compact `session` through the product summariser and hand back what the
/// model was actually sent.
async fn compacted_through(
    session: &mut SessionContext,
    redactor: &(dyn Redactor + Sync),
) -> (Vec<String>, zaru_core::context::Compaction) {
    let model = Recording::new();
    let summariser = ModelSummariser::over(&model, redactor);
    let compaction = session
        .at_turn_boundary(&summariser, redactor)
        .await
        .expect("the recording provider answers");
    assert_eq!(
        model.tools.lock().expect("no panic holds this").clone(),
        vec![0],
        "a summarisation offers no tools"
    );
    (model.prompts(), compaction)
}

// ---------------------------------------------------------------------------
// The security corpus
// ---------------------------------------------------------------------------

/// **The seventh redaction path, asserted.** A value the harness holds, put
/// into layer 6 by a tool result, does not reach the summarisation request —
/// by value and by ASCII core.
///
/// The mutant: a redactor that returns its input unchanged, which is what the
/// accepting sibling below deliberately is.
#[tokio::test]
async fn a_held_secret_in_the_compacted_span_is_absent_from_the_summarisation_request() {
    let scratch = Scratch::new("absent");
    let planted = planted_bearer("absent");
    let held = holding(&scratch, &planted);
    let mut session = session_carrying(&planted);

    let (prompts, compaction) = compacted_through(&mut session, &held).await;

    assert!(
        compaction.raw.is_some(),
        "the staging must actually cross the threshold, or this check passes over a compaction \
         that never happened"
    );
    let sent = prompts.first().expect("the summariser sent one request");
    assert!(
        sent.contains(r#"fs.read {"path":"notes.txt"}"#),
        "the span really did reach the request, so the absence below is about redaction rather \
         than about the span being empty: {sent:?}"
    );
    assert!(
        !sent.contains(&planted),
        "ADR-0008 clause 6 puts the redactor on every path from captured bytes into a model \
         request, and a compaction is a request: the planted value reached it whole"
    );
    assert!(
        !sent.contains(ascii_core(&planted)),
        "the value's ASCII core reached the summarisation request, which is every byte of it up \
         to the first combining mark -- the arm that survives an escaping rendering"
    );
}

/// **The accepting sibling.** With nothing held, the same value reaches the
/// same request byte for byte — so the absence above is a statement about the
/// redactor rather than about a walk that looks in the wrong place.
#[tokio::test]
async fn the_absence_walk_finds_the_value_when_nothing_is_held() {
    let planted = planted_bearer("present");
    let mut session = session_carrying(&planted);
    let nothing = HeldSecrets::none();

    let (prompts, compaction) = compacted_through(&mut session, &nothing).await;

    assert!(compaction.raw.is_some(), "the threshold was crossed");
    let sent = prompts.first().expect("the summariser sent one request");
    assert!(
        sent.contains(&planted) && sent.contains(ascii_core(&planted)),
        "with nothing held the span must reach the model unaltered, or the check above is not \
         about redaction at all: {sent:?}"
    );
    assert!(
        !sent.contains("<redacted: "),
        "nothing was held and something was marked as redacted anyway: {sent:?}"
    );
}

/// **The record is not the model's view.** After a compaction, the raw span
/// on disk carries the planted value whole — ADR-0010 D2's transcript
/// "contains whatever the session contained", and ADR-0008's decision says
/// "the transcript is untouched".
///
/// The mutant: redacting on the way to the transcript, which reddens the
/// presence assertion.
#[tokio::test]
async fn the_transcript_keeps_the_span_the_model_was_not_given() {
    let scratch = Scratch::new("record");
    let planted = planted_bearer("record");
    let held = holding(&scratch, &planted);
    let mut session = session_carrying(&planted);

    let (prompts, compaction) = compacted_through(&mut session, &held).await;
    let sent = prompts.first().expect("the summariser sent one request");
    assert!(!sent.contains(&planted), "the model's view is redacted");

    let path = scratch.base.join("transcript.jsonl");
    let mut transcript = Transcript::append_to(path.clone()).expect("the file opens");
    transcript
        .record(&Record::Compacted(compaction))
        .expect("the compaction is written");
    drop(transcript);

    let bytes = std::fs::read_to_string(&path).expect("the file reads");
    assert!(
        bytes.contains(&planted),
        "redaction is on what a model reads and not on the record; the span on disk is missing \
         the value the session contained, in {} byte(s)",
        bytes.len()
    );

    // And it reads back, which is what makes ADR-0010 D2's replayability
    // claim true for this producer as well as the others.
    let read = Transcript::read(&path).expect("the file parses");
    assert_eq!(read.records.len(), 1);
}

/// A compaction announces itself exactly once, with counts that are
/// measurements of the span it replaced.
///
/// The mutant: announcing on every render rather than returning the
/// announcement from the operation that caused it.
#[tokio::test]
async fn a_compaction_announces_once_with_what_it_cost() {
    let planted = planted_bearer("announce");
    let mut session = session_carrying(&planted);
    let nothing = HeldSecrets::none();

    let (_, compaction) = compacted_through(&mut session, &nothing).await;

    let compacted: Vec<_> = compaction
        .announcements
        .iter()
        .filter(|announced| {
            matches!(
                announced,
                zaru_core::context::Announcement::Compacted { .. }
            )
        })
        .collect();
    assert_eq!(
        compacted.len(),
        1,
        "D3: \"Compaction is announced, once, with what it cost\"; got {:?}",
        compaction.announcements
    );

    let raw = compaction.raw.as_ref().expect("layer 6 was compacted");
    match compacted[0] {
        zaru_core::context::Announcement::Compacted {
            turns,
            before,
            after,
        } => {
            assert_eq!(*turns as usize, raw.len());
            let measured: u64 = raw
                .exchanges()
                .iter()
                .map(|exchange| exchange.as_str().len() as u64)
                .sum();
            assert_eq!(
                *before, measured,
                "`before` must be what the replaced exchanges actually cost rather than how many \
                 there were"
            );
            assert_eq!(*after, "they agreed on four spaces".len() as u64);
        }
        other => panic!("filtered for Compacted and got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// The one check that reaches a provider
// ---------------------------------------------------------------------------

/// One real summarisation, against a real model, through the real product
/// summariser.
///
/// **Skipped unless [`RUN_VARIABLE`] says a key is in a store at a scratch
/// home**, and it prints that it skipped rather than passing silently. What it
/// asserts is that a summary comes back with text in it, that it is shorter
/// than the span it replaced, that ADR-0012 D7's tokens are reported, and that
/// the key appears in nothing the run produced — by value and by ASCII core.
#[tokio::test]
async fn one_real_summarisation_and_the_key_is_in_none_of_it() {
    let Ok(_) = std::env::var(RUN_VARIABLE) else {
        println!(
            "SKIPPED: {RUN_VARIABLE} is not set, so no key is available and no provider is \
             called. This check reaches the network and must never run on a CI runner. To run \
             it: put a key in a store under a scratch HOME with `zaru providers keys add \
             gemini`, then set {RUN_VARIABLE}=1."
        );
        return;
    };

    // **The one check in this workspace that reads the process's own home, and
    // it does so because its operator set that home**: the skip line above
    // tells them to put a key under a scratch `HOME` and run it there. It
    // never runs on a runner and never under anybody's real `~/.zaru` by
    // accident, which is what `corpus_one_thing_decides_where_the_harness_lives`
    // exempts it for.
    let root =
        CredentialStore::root_in(&zaru_cli::config::Home::of_this_user()).expect("a HOME is set");
    let store =
        CredentialStore::reading(root.clone()).expect("the store at the scratch HOME opens");
    let keyring = OsKeyring::for_store(&root);
    // The sealing key, out of the environment its operator set, for the
    // reason the home above is: `corpus_one_thing_reads_the_environment`
    // exempts this check and names why.
    let keys = HarnessKeys::within(&keyring, &zaru_cli::config::Variables::of_this_process());

    let alias = ProviderKind::credential_alias(ProviderKind::Gemini);
    let secret = store
        .secret(&alias, &keys)
        .expect("the gemini key is in the store under its ordinary alias");
    let key = secret.expose_for_dispatch().to_owned();

    let model = model_id(&model_name());
    let client = GeminiClient::new(
        Endpoint::default_endpoint(),
        model,
        alias,
        secret,
        zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds");

    // The redactor is built from what the store holds, which is the provider
    // key itself -- so this run also asserts that the key the request is
    // authenticated with cannot appear in the request's body.
    let held = held_secrets_for_redaction(&store, &keys).expect("the store reopens what it sealed");

    let mut session = SessionContext::opened(
        prefix_for(None, &facts()),
        zaru_cli::compose::ContextShape::of(tight(), 0, one_token_a_byte()),
    );
    let staged = [
        "we agreed the indentation is four spaces and never tabs",
        "the deploy script is `just ship`, and it refuses on a dirty tree",
        "the rehearsal number is 4173",
    ];
    let mut records = Vec::new();
    for (n, line) in (0_u32..).zip(staged) {
        records.extend(said(n, format!("{line}. {}", "detail ".repeat(30))));
    }
    session.rebuild_from(&records);

    let summariser = ModelSummariser::over(&client, &held);
    let compaction = session
        .at_turn_boundary(&summariser, &held)
        .await
        .expect("the provider summarises the span");

    let raw = compaction
        .raw
        .as_ref()
        .expect("the staging crossed the threshold");
    let summary = session.exchanges()[0].rendered();
    assert!(
        !summary.trim().is_empty(),
        "the provider returned an empty summary"
    );
    let replaced: usize = raw
        .exchanges()
        .iter()
        .map(|exchange| exchange.as_str().len())
        .sum();
    assert!(
        summary.len() < replaced,
        "a summary that is longer than what it replaced has relieved no pressure: {} against {}",
        summary.len(),
        replaced
    );

    let spent = summariser
        .spent()
        .expect("ADR-0012 D7: every request records its tokens");
    assert!(spent.prompt > 0 && spent.completion > 0, "{spent:?}");

    // The key is in none of it, by value and by ASCII core.
    free_of_key(&summary, &key, "the summary the provider returned");
    free_of_key(&format!("{compaction:?}"), &key, "the compaction's Debug");
    free_of_key(&format!("{summariser:?}"), &key, "the summariser's Debug");
    for exchange in session.exchanges() {
        free_of_key(&exchange.rendered(), &key, "layer 6 after the compaction");
    }

    println!(
        "one real summarisation: {} exchange(s) totalling {replaced} bytes replaced by {} bytes, \
         costing {} prompt + {} completion tokens",
        raw.len(),
        summary.len(),
        spent.prompt,
        spent.completion,
    );
    println!("the summary: {summary:?}");
}

/// Assert that `haystack` carries no part of `key`, and say where it was.
fn free_of_key(haystack: &str, key: &str, what: &str) {
    assert!(
        !haystack.contains(key),
        "the key appears verbatim in {what}"
    );
    let core = ascii_core(key);
    assert!(
        !core.is_empty() && !haystack.contains(core),
        "the key's ASCII core appears in {what}, which is what an escaping formatter leaves intact"
    );
}

/// The model the issued test credential serves.
fn model_name() -> String {
    std::env::var("ZARU_GEMINI_MODEL").unwrap_or_else(|_| "gemini-3.6-flash".to_owned())
}

/// A `ModelId` for `name`, built the way the binary builds one.
///
/// `ModelId` is constructible only inside the resolution table, which is
/// ADR-0012 D1 as a compile error. An outside caller gets one the same way
/// `zaru --model <identifier>` does: stage a layer-5 contribution and read the
/// alias back out of the fold. No test-only door is opened in the product.
fn model_id(name: &str) -> zaru_cli::providers::ModelId {
    use zaru_cli::config::{Contribution, Layer, Resolution, Schema, Source, Table, Value};
    use zaru_cli::providers::{ModelAlias, ModelTable, ResolvedModel, declare};

    let mut document = Table::new();
    document.insert_path(&ModelAlias::Default.key(), Value::Text(name.to_owned()));
    let resolution = Resolution::resolve(
        &declare(Schema::new()),
        vec![Contribution::new(
            Layer::Flag,
            Source::named("summariser_from_outside"),
            document,
        )],
    )
    .expect("the staged contribution resolves");

    match ModelTable::from_configuration(&resolution)
        .expect("the value is text")
        .row(ModelAlias::Default)
    {
        ResolvedModel::Resolved { model, .. } => model.clone(),
        ResolvedModel::Unresolved => panic!("the alias was set above"),
    }
}

// ---------------------------------------------------------------------------
// ADR-0013 clause 5 — the number on the status row, from outside both crates
// ---------------------------------------------------------------------------

/// A resumed session's row carries what its rebuilt layer 6 holds.
///
/// A resumed session's layer 6 is rebuilt from its transcript's records by
/// `SessionContext::rebuilt`, the same rebuild every turn boundary does, and
/// this asserts the rebuilt context reports the count the live one did and
/// that the count reaches a painted row.
///
/// The mutant: `rebuilt` ignoring the records, which would look exactly like
/// a session that had said nothing.
#[test]
fn a_rebuilt_context_puts_the_count_the_live_one_had_on_the_row() {
    use zaru_cli::terminal::driver::refresh_status;
    use zaru_tui::shell::{Shell, Status};

    let held = HeldSecrets::none();
    let saved = session_carrying("nothing-here");
    let expected = saved.usage(&held).used();

    let (restored, _) = SessionContext::rebuilt(
        prefix_for(None, &facts()),
        zaru_cli::compose::ContextShape::of(tight(), 0, one_token_a_byte()),
        &carrying_records("nothing-here"),
    );

    assert_eq!(
        restored.usage(&held).used(),
        expected,
        "a rebuilt context must cost what the live one cost"
    );
    assert!(
        expected > 0,
        "the staging must actually carry exchanges, or this check compares two empty contexts"
    );

    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));
    refresh_status(&mut shell, &restored, None, None, &held);

    let segment = shell
        .status()
        .context
        .clone()
        .expect("a restored session has a context");
    let abbreviated = zaru_cli::cli::render::thousands(expected);
    // **Both spellings**, since 2026-09-06. A narrow row shows the same count
    // as a wide one -- the narrow form drops labelling and never a number --
    // and a check that read only the full one would pass while the figure a
    // 40-column terminal actually shows was wrong.
    assert!(
        segment.full.contains(&abbreviated) && segment.narrow.contains(&abbreviated),
        "the restored count must reach the row in both spellings; the segment was {segment:?}"
    );

    // The empty case, so the check above cannot be satisfied by a `rebuilt`
    // that returns whatever it likes: no records must report the prefix
    // alone, and that is a smaller number than the one above.
    let (empty, _) = SessionContext::rebuilt(
        prefix_for(None, &facts()),
        zaru_cli::compose::ContextShape::of(tight(), 0, one_token_a_byte()),
        &[],
    );
    assert!(
        empty.usage(&held).used() < expected,
        "no records must rebuild to less than seven turns"
    );
}

/// A held secret that reached layer 6 is measured by the row and printed by
/// nothing.
///
/// **The status row is a rendering of layer 6, so it is a boundary.** The
/// count comes from `Context::usage`, which measures the *redacted* text, and
/// the number is a number — but "the number is a number" is an argument, and
/// this record's own [ADR-0008] clause 6 obligations are checked rather than
/// argued. So a real credential store holds a real bearer, a tool result in
/// layer 6 carries it, and the painted row is asserted free of it **by value
/// and by ASCII core** ([Verification lessons] §63).
///
/// The **accepting sibling** is in the same body and is what makes the absence
/// mean anything: the same walk over the same row finds the count, so the
/// instrument demonstrably reads a populated row rather than an empty one
/// (§8, and `the_absence_walk_finds_the_value_when_nothing_is_held` above is
/// the file's own general form).
///
/// # The bearer is planted at BOTH ends of layer 6, and a red-watch is why
///
/// The first draft staged it through [`session_carrying`] alone, which puts
/// the value in the **oldest** exchange. A mutant that rendered layer 6's
/// **newest** exchange onto the row then survived — the row carried an
/// exchange, just not the one holding the secret — while the same mutant
/// reading the oldest reddened at once. That is a finding about the check
/// rather than about the product ([Verification lessons] §15): an absence
/// assertion over a rendering is only as wide as the part of the haystack the
/// staging can reach. So a final exchange carries the bearer too, and a
/// renderer showing *either* end of layer 6 is now visible.
///
/// The mutants: rendering `exchanges().first()` and `exchanges().last()`
/// beside the count. Both redden.
#[test]
fn a_held_secret_in_layer_six_is_absent_from_the_status_row_that_measures_it() {
    use zaru_cli::terminal::driver::refresh_status;
    use zaru_tui::shell::{Shell, Status};

    let scratch = Scratch::new("status-row");
    let planted = planted_bearer("row");
    let held = holding(&scratch, &planted);
    let mut context = session_carrying(&planted);
    let mut records = carrying_records(&planted);
    records.extend(a_read(
        7,
        "read it again",
        &format!("still {planted}"),
        "the notes still name it",
    ));
    context.rebuild_from(&records);
    let context = context;

    // The staging reaches both ends, which is what the two mutants above are
    // about. Asserted rather than assumed: an `Exchange` that dropped its tool
    // results would leave this check walking a haystack with no needle in it.
    let ends = [
        context.exchanges().first().expect("layer 6 is populated"),
        context.exchanges().last().expect("layer 6 is populated"),
    ];
    for end in ends {
        assert!(
            end.rendered().contains(&planted),
            "the staging must put the bearer at both ends of layer 6, or a renderer showing one              of them is invisible to this check; that end was {:?}",
            end.rendered()
        );
    }

    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));
    refresh_status(&mut shell, &context, None, None, &held);

    let row = shell.status().painted(200);
    let debugged = format!("{:?}", shell.status());

    for (what, haystack) in [("the painted row", &row), ("the row's Debug", &debugged)] {
        assert!(
            !haystack.contains(&planted),
            "{what} carries the planted bearer by value: {haystack}"
        );
        assert!(
            !haystack.contains(ascii_core(&planted)),
            "{what} carries the planted bearer's ASCII core, which no escaping scheme alters: \
             {haystack}"
        );
    }

    // The accepting sibling. Without it the two arms above pass on an empty
    // row, which is the shape [Verification lessons] §8 is about.
    let used = context.usage(&held).used();
    assert!(
        row.contains(&zaru_cli::cli::render::thousands(used)),
        "the walk must be over a row that actually carries the count, or its absences say \
         nothing; the row was {row:?}"
    );
    assert!(
        used > 0 && !held.is_empty(),
        "the staging must hold a secret and a populated context: {used} token(s), {} held",
        held.len()
    );
}

// --------------------------------- a home and an environment nobody handed

/// The facts a check's layer 1 is built from: fixed, so a prompt a check
/// compares is the same on every machine and every day.
fn facts() -> zaru_cli::compose::Facts {
    zaru_cli::compose::Facts {
        directory: Some("/work".to_owned()),
        system: "linux".to_owned(),
        date: "2026-09-28".to_owned(),
        tools: vec!["fs.read".to_owned()],
        mode: None,
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
