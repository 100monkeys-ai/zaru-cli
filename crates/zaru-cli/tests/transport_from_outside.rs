// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What each provider client says when it cannot reach its endpoint, driven
//! from outside the crate against a port nothing is listening on.
//!
//! # These checks reach a socket and no server, which is the whole point
//!
//! [Testing] forbids a check that calls a provider, and the standing ruling of
//! 2026-09-14 forbids a loopback listener serving a provider's responses
//! anywhere, because a fake of a provider at the wire is the mock that page
//! refuses. **Nothing here serves anything.** [`a_port_nothing_is_listening_on`]
//! binds a socket only to learn a free port from the operating system and drops
//! it before any client is built, so the connection is refused by the kernel;
//! and [`UNRESOLVABLE`] is a name RFC 6761 reserves against ever resolving. No
//! byte of any provider's protocol is written or read by this file. That is
//! also why these run on a runner while
//! `tests/openai_compatible_from_outside.rs` skips: a refused connection needs
//! no network, no key and no model.
//!
//! # Why the assertion is `assert_ne!` and not an operating system's wording
//!
//! The defect these checks exist for is that a refused connection and a
//! hostname that does not resolve **read identically**: `reqwest::Error`'s
//! `Display` for a failed send is `error sending request for url (…)` and
//! nothing else, and the cause is three links down its `source` chain. So the
//! property is that the two sentences differ, and that survives any resolver's
//! choice of words. Asserting a literal like `Name or service not known` would
//! pin one libc's phrasing and redden on another machine for a reason that has
//! nothing to do with this harness. The refused arm additionally asserts the
//! word `refused` appears, which every system that implements `ECONNREFUSED`
//! renders and which also makes the check self-guarding: if something were
//! unexpectedly listening on the port, the assertion fails rather than passing
//! for the wrong reason.
//!
//! # Three of nine arms
//!
//! Each client maps a `reqwest::Error` at three points — the send, the body
//! read on a non-success status, and each chunk of the stream. **Only the send
//! arm is reachable from here**, because the other two need a server that
//! answers and then breaks, and no such server may exist in this suite. The
//! other six are covered by [`every_client_composes_its_transport_failures_the_same_way`],
//! which reads the source, and this limit is stated rather than implied.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing

use zaru_cli::config::{Contribution, Layer, Resolution, Schema, Source, Table, Value};
use zaru_cli::credentials::Secret;
use zaru_cli::providers::gemini::GeminiClient;
use zaru_cli::providers::ollama::OllamaClient;
use zaru_cli::providers::openai_compatible::OpenAiCompatibleClient;
use zaru_cli::providers::{ProviderEndpoint, ProviderKind};
use zaru_core::iteration::Prompt;
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{Model, ModelRequest};

/// A host name that cannot resolve, anywhere, by specification.
///
/// RFC 6761 reserves `.invalid` precisely so that a name under it is
/// guaranteed not to be in the public namespace. Using a plausible-looking
/// name instead would make this check depend on somebody else's DNS zone.
const UNRESOLVABLE: &str = "http://nonexistent.invalid";

/// A redactor holding nothing, so a [`Prompt`] can be built.
struct NothingHeld;

impl Redactor for NothingHeld {
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(text)
    }
}

/// An origin on loopback that nothing is listening on.
///
/// The listener exists for exactly as long as it takes the operating system to
/// assign a port, and is dropped before this function returns. **It never
/// accepts and never answers**, so it is not a stand-in for a provider; asking
/// the kernel for a free port is the only reliable way to name one, and a
/// hard-coded number is a number somebody else's process may hold.
fn a_port_nothing_is_listening_on() -> String {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
    let port = listener
        .local_addr()
        .expect("a bound listener has an address")
        .port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

/// A [`ModelId`] for `name`, built the way the binary builds one.
///
/// `ModelId` is constructible only inside the resolution table, so an outside
/// caller gets one the same way `zaru --model <identifier>` does. No test-only
/// door is opened in the product: a door that exists only for a check is a
/// door.
///
/// [`ModelId`]: zaru_cli::providers::ModelId
fn model_id(name: &str) -> zaru_cli::providers::ModelId {
    use zaru_cli::providers::{ModelAlias, ModelTable, ResolvedModel, declare};

    let mut document = Table::new();
    document.insert_path(&ModelAlias::Default.key(), Value::Text(name.to_owned()));
    let resolution = Resolution::resolve(
        &declare(Schema::new()),
        vec![Contribution::new(
            Layer::Flag,
            Source::named("transport_from_outside"),
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

/// The two sentences a client produced, for a refused port and for a name that
/// does not resolve.
///
/// Both arms go through the client's own `Model::respond`, so what is compared
/// is the rendered failure a person reads and not a value a check composed.
struct BothWays {
    refused: String,
    unresolvable: String,
}

/// Assert the shared property over one client's pair of sentences.
///
/// Named so that the four clauses report separately rather than as one
/// boolean, and so that each client's check body says only which client it is
/// about.
fn assert_the_two_failures_are_told_apart(kind: ProviderKind, said: &BothWays) {
    let BothWays {
        refused,
        unresolvable,
    } = said;
    // Printed rather than only asserted over: these are the sentences a person
    // reads, and a check over user-facing text that never shows the text makes
    // its reader take the assertion's word for what was compared. Visible with
    // `--nocapture`.
    println!("{kind:?} refused:      {refused}");
    println!("{kind:?} unresolvable: {unresolvable}");
    assert!(
        refused.to_lowercase().contains("refused"),
        "the {kind:?} client's refusal does not name the refused connection, so a reader is told \
         only that a request was sent: {refused}",
    );
    assert_ne!(
        refused, unresolvable,
        "the {kind:?} client tells a refused connection and a hostname that does not resolve in \
         the same words. They are different problems with different remedies, and a reader who \
         cannot tell which they have cannot act on either",
    );
    // The accepting half of the clause above: the two must differ, but not by
    // having lost the sentence that says which endpoint was tried.
    for said in [refused, unresolvable] {
        assert!(
            said.contains("error sending request for url"),
            "the {kind:?} client's refusal no longer carries the top-level sentence, so the walk \
             replaced what it was meant to extend: {said}",
        );
    }
}

/// Drive one `ollama` client at `origin` and render what it said.
async fn what_ollama_said(origin: &str) -> String {
    let client = OllamaClient::new(
        ProviderEndpoint::new(origin).expect("the origin is well-formed"),
        model_id("llama3.2:3b"),
        zaru_cli::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
    )
    .expect("an HTTP client builds");
    let prompt = Prompt::new(Redacted::by(&NothingHeld, "say hello"));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &[],
        results: &[],
    };
    match Model::respond(&client, &request).await {
        Ok(response) => panic!("something answered at {origin}: {response:?}"),
        Err(failure) => failure.to_string(),
    }
}

/// Drive one `openai-compatible` client at `origin` and render what it said.
async fn what_openai_compatible_said(origin: &str) -> String {
    let client = OpenAiCompatibleClient::new(
        ProviderEndpoint::new(origin).expect("the origin is well-formed"),
        model_id("llama3.2:3b"),
        ProviderKind::OpenAiCompatible.credential_alias(),
        None,
        Some(8_192),
    )
    .expect("an HTTP client builds");
    let prompt = Prompt::new(Redacted::by(&NothingHeld, "say hello"));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &[],
        results: &[],
    };
    match Model::respond(&client, &request).await {
        Ok(response) => panic!("something answered at {origin}: {response:?}"),
        Err(failure) => failure.to_string(),
    }
}

/// The `ollama` client tells a refused connection from a name that will not
/// resolve.
///
/// Measured from the release binary on 2026-09-14, before this arc: both read
/// `nothing answered at <origin>: error sending request for url (<url>). A
/// local model server is started by whoever runs it…`, identical outside the
/// URL, so a person whose DNS is wrong was told to start a server.
#[tokio::test]
async fn the_ollama_client_says_which_transport_failure_a_reader_has() {
    let said = BothWays {
        refused: what_ollama_said(&a_port_nothing_is_listening_on()).await,
        unresolvable: what_ollama_said(UNRESOLVABLE).await,
    };
    assert_the_two_failures_are_told_apart(ProviderKind::Ollama, &said);
}

/// The `openai-compatible` client does the same.
///
/// **This is the accepting sibling of the check above and of the `gemini` one.**
/// That kind has walked the chain since 2026-09-14, so this assertion passed
/// before either of the other two clients was touched — which is what says the
/// shared assertion can be satisfied at all, and what made the other two
/// checks' red a finding about those clients rather than about the check.
#[tokio::test]
async fn the_openai_compatible_client_says_which_transport_failure_a_reader_has() {
    let said = BothWays {
        refused: what_openai_compatible_said(&a_port_nothing_is_listening_on()).await,
        unresolvable: what_openai_compatible_said(&format!("{UNRESOLVABLE}/v1")).await,
    };
    assert_the_two_failures_are_told_apart(ProviderKind::OpenAiCompatible, &said);
}

/// Drive one `gemini` client at `origin` and render what it said.
///
/// The key is an invented string and never a real one: a refused connection and
/// a name that does not resolve both fail before a byte is sent, so nothing
/// this function builds ever leaves the machine and no credential is needed to
/// reach the arm under test.
async fn what_gemini_said(origin: &str) -> String {
    let client = GeminiClient::new(
        ProviderEndpoint::new(origin).expect("the origin is well-formed"),
        model_id("gemini-3.6-flash"),
        ProviderKind::Gemini.credential_alias(),
        Secret::provider(ProviderKind::Gemini, "not-a-key-and-never-sent")
            .expect("a non-empty value with no control character is a secret"),
        zaru_cli::providers::gemini::CONTEXT_WINDOW_TOKENS,
    )
    .expect("an HTTP client builds");
    let prompt = Prompt::new(Redacted::by(&NothingHeld, "say hello"));
    let request = ModelRequest {
        prompt: &prompt,
        tools: &[],
        results: &[],
    };
    match Model::respond(&client, &request).await {
        Ok(response) => panic!("something answered at {origin}: {response:?}"),
        Err(failure) => failure.to_string(),
    }
}

/// The `gemini` client tells a refused connection from a name that will not
/// resolve.
///
/// Measured from the release binary on 2026-09-14, before this arc: both read
/// `the provider could not be reached: error sending request for url (<url>)`,
/// identical outside the URL — and this kind's class is environmental, so the
/// remedy beside it said *"waiting will not help … Running the same command
/// again is the retry"*. A person whose endpoint was mistyped was told to run
/// it again. The class is unchanged here and is raised as a question of its
/// own; what changes is that the sentence now says which problem they have.
#[tokio::test]
async fn the_gemini_client_says_which_transport_failure_a_reader_has() {
    let said = BothWays {
        refused: what_gemini_said(&a_port_nothing_is_listening_on()).await,
        unresolvable: what_gemini_said(UNRESOLVABLE).await,
    };
    assert_the_two_failures_are_told_apart(ProviderKind::Gemini, &said);
}

/// Every client's transport arms go through the one walk, read from the source.
///
/// **This is the six arms no check in this file can reach.** Each client maps a
/// `reqwest::Error` at three points and only the send is reachable without a
/// server that answers and then breaks, which this suite may not have. So the
/// remaining six are held by reading the source rather than by running it, and
/// this check is that reading made mechanical: a fourth arm added to any client
/// with `to_string()` reddens here even though nothing can drive it.
///
/// **The needle became `transport_detail_within(` on 2026-09-15**, when the
/// arms began passing the exchange ceiling so a timed-out refusal could name
/// it. The property is unchanged and so is the count; what the arms call is
/// the wrapper rather than the walk.
///
/// The count is asserted in both directions on purpose. Zero occurrences of the
/// forbidden spelling is the property; three occurrences of the required one is
/// the control that says the file was found and the needle is findable at all,
/// so a path typo cannot make this pass by reading nothing.
#[test]
fn every_client_composes_its_transport_failures_the_same_way() {
    const CLIENTS: [(&str, &str); 3] = [
        ("gemini", include_str!("../src/providers/gemini.rs")),
        ("ollama", include_str!("../src/providers/ollama.rs")),
        (
            "openai_compatible",
            include_str!("../src/providers/openai_compatible.rs"),
        ),
    ];

    for (kind, source) in CLIENTS {
        let walked = source.matches("transport_detail_within(").count();
        assert_eq!(
            walked, 3,
            "the {kind} client has {walked} transport arms walking the source chain rather than \
             3. Either an arm stopped walking it, or one was added and did not start -- and the \
             arms this file can drive are only the sends, so a body-read or chunk arm that \
             regressed would otherwise reach nobody until a user met it",
        );
        let dropped = source.matches("detail: error.to_string()").count();
        assert_eq!(
            dropped, 0,
            "the {kind} client composes {dropped} transport failure(s) from `reqwest`'s \
             top-level message alone, which names no cause a reader can act on",
        );
    }
}

/// No client declares an exchange ceiling of its own, read from the source.
///
/// **A fourth kind picking a fourth number is what this check exists to stop,
/// and it cannot be caught by running anything.** A client's timeout is inside
/// the `reqwest::Client` it built; nothing on `Provider` reports it, and a
/// check that drove a client to its ceiling would have to wait ten minutes to
/// learn the figure. So the property is held by reading the source, the same
/// instrument and for the same reason as the walk check above.
///
/// **Until 2026-09-15 the three figures were 60, 600 and 600** — which bound a
/// turn ran under depended on which kind served the alias, and the sixty
/// killed reasoning turns that were inside the model's ordinary range. The
/// constant is now `providers::transport::EXCHANGE_TIMEOUT` and a client
/// chooses nothing.
///
/// The count is asserted in both directions, as the walk check's is: zero
/// declarations is the property, and four readings of the shared constant per
/// client is the control that says the files were found and the needle is
/// findable, so a path typo cannot make this pass by reading nothing. **Four
/// is the builder plus the three transport arms** -- the client passes the
/// ceiling to `web::client::build` and then to `transport_detail_within` at
/// each of the three points it maps a `reqwest::Error`, so the figure a
/// refusal names and the figure the request ran under are the same value by
/// construction rather than by two constants agreeing.
#[test]
fn no_client_declares_an_exchange_ceiling_of_its_own() {
    const CLIENTS: [(&str, &str); 3] = [
        ("gemini", include_str!("../src/providers/gemini.rs")),
        ("ollama", include_str!("../src/providers/ollama.rs")),
        (
            "openai_compatible",
            include_str!("../src/providers/openai_compatible.rs"),
        ),
    ];

    for (kind, source) in CLIENTS {
        let declared = source.matches("const EXCHANGE_TIMEOUT").count();
        assert_eq!(
            declared, 0,
            "the {kind} client declares {declared} exchange ceiling(s) of its own. One figure per \
             kind is what made a gemini turn die at sixty seconds while the same work against \
             ollama had ten minutes, and a fourth kind would pick a fourth number",
        );
        let read = source
            .matches("crate::providers::transport::EXCHANGE_TIMEOUT")
            .count();
        assert_eq!(
            read, 4,
            "the {kind} client reads the shared ceiling {read} time(s) rather than 4 -- the \
             builder and the three transport arms. Either it stopped reading it somewhere, or \
             this needle no longer names anything",
        );
    }
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
