// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside this crate drives a stored credential through a real MCP
//! session and watches [ADR-0007] D6's cache be invalidated three ways.
//!
//! # What this establishes, and what it does not
//!
//! It is evidence that the mechanism works through **`zaru-cli`'s public
//! door**: the store is opened, a secret is sealed and taken back out,
//! converted to a bearer, handed to a session, and the entry's cached scope is
//! replaced by one `tools/list` on each of D6's three signals — using nothing
//! the crates do not export. Three crates are named because a composition root
//! is where three things meet: `zaru_cli` for the store, `zaru_notes` for the
//! session and the signals, and `zaru_core` for the clock port.
//!
//! **It is not evidence about the `zaru` binary**, which reaches none of this,
//! and it is not evidence about Nuclear Notes, which was never called. The
//! server on the other end is a fixture written in this file.
//!
//! # Why the bytes are real
//!
//! The transport is `rmcp`'s `transport-async-rw` over [`tokio::io::duplex`] —
//! an in-memory pipe, not a socket. Every frame is serialised, newline-framed
//! and parsed by `rmcp`'s own codec exactly as it would be over a network. A
//! byte pump between two duplex pairs keeps a transcript, which is what lets
//! the "exactly one `tools/list`" claim have **two independent readers**: the
//! fixture server counts entries into its own handler on the far side of the
//! protocol, and the transcript counts request frames. Neither travels back
//! through `refresh_tool_scope`.
//!
//! # Why this fixture is a second copy
//!
//! `zaru-notes` carries one in its own `tests/` tree. An integration test
//! cannot see another crate's test targets, and [ADR-0003] D8 forbids the
//! dependency that would share them. Whether a test-support crate should exist
//! is a question for that record and is deliberately not answered here.
//!
//! Everything here is a generated nonce. No real credential is held.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorCode,
    Implementation, JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer, RunningService};
use rmcp::transport::async_rw::TransportAdapterAsyncCombinedRW;
use rmcp::{ErrorData as McpError, serve_server};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::sync::mpsc;

use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance as TokenInstance, KeyStore, Reach,
    SealingError, SealingKey, Secret, ToolScope, Ttl, bearer_for_dispatch,
};
use zaru_core::iteration::Clock;
use zaru_notes::session::{
    Bearer, Endpoint, EndpointFailure, Instance as NotesInstance, Instance, Invalidation,
    NotesError, Session, WorkspaceId,
};

// -- the nonce, which this file must carry its own copy of -------------------

/// The awkward tail every nonce carries: a decomposed grapheme cluster, a
/// precomposed one, and an astral-plane character.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

fn nonce(label: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}{AWKWARD_TAIL}", std::process::id())
}

fn ascii_core(value: &str) -> &str {
    value.strip_suffix(AWKWARD_TAIL).unwrap_or(value)
}

/// Asserts a rendering carries neither the planted value nor its ASCII core.
///
/// The second arm is the one that catches an escaped leak: `{:?}` renders a
/// combining mark as `\u{301}`, so a rendering that published every byte of a
/// nonce does not `contain` it as typed.
fn assert_absent(what: &str, rendered: &str, planted: &str) {
    assert!(
        !rendered.contains(planted),
        "{what} published the bearer value verbatim; it is in {rendered:?}"
    );
    let core = ascii_core(planted);
    assert!(
        !core.is_empty(),
        "the fixture produced no ASCII core, so this check asserted nothing"
    );
    assert!(
        !rendered.contains(core),
        "{what} published the bearer value in an escaped form; its ASCII core {core:?} is in \
         {rendered:?}"
    );
}

// -- the clock, which is `zaru-core`'s port and this file's implementation ---

/// A clock that moves only when a check moves it.
///
/// The reason `zaru-core`'s port returns a monotonic offset rather than an
/// instant: an instant cannot be constructed at a chosen value, so a clock a
/// check cannot set is a clock a check cannot assert on.
#[derive(Debug, Default)]
struct ManualClock {
    elapsed: Mutex<Duration>,
}

impl ManualClock {
    fn advance(&self, by: Duration) {
        *self
            .elapsed
            .lock()
            .expect("the manual clock is not poisoned") += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self
            .elapsed
            .lock()
            .expect("the manual clock is not poisoned")
    }
}

// -- the sealing port, implemented outside the crate that declares it --------

/// The key port, implemented outside the crate that declares it.
///
/// That it can be implemented from out here is part of what this check
/// establishes: `KeyStore` is the seam a machine's own keyring sits behind, and
/// a trait that could only be implemented from inside would not be one. The key
/// is kept so this check can open what the store sealed **without going back
/// through the store**, which is the arm of the comparison that must not travel
/// through the code under test.
struct StagedKey(SealingKey);

impl StagedKey {
    fn minted() -> Self {
        Self(SealingKey::mint())
    }
}

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

// -- the fixture server ------------------------------------------------------

/// The workspace identifier the fixture answers reads for.
const WORKSPACE: &str = "a96c9dde-becf-4ff0-836e-ad8bef46ff42";

/// What the fixture answers `tools/list` with before anything changes.
const FIRST_SCOPE: [&str; 3] = ["pages.read", "atoms.read", "search.global"];

/// What the entry's cache holds before any `tools/list` has been made.
///
/// Deliberately wrong. A store whose cache already held the server's answer
/// would satisfy every "the scope was replaced" assertion without a refresh.
const STALE_SCOPE: [&str; 1] = ["stale.tool"];

#[derive(Clone)]
struct FakeNotes {
    scope: Arc<Mutex<Vec<String>>>,
    /// Entries into this server's own `tools/list` handler. The far side of
    /// the protocol, which is the consequence rather than a proxy for it.
    tools_list_calls: Arc<AtomicUsize>,
}

impl FakeNotes {
    fn new() -> Self {
        Self {
            scope: Arc::new(Mutex::new(
                FIRST_SCOPE.iter().map(|s| (*s).to_owned()).collect(),
            )),
            tools_list_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn calls(&self) -> usize {
        self.tools_list_calls.load(Ordering::SeqCst)
    }

    fn grant(&self, tool: &str) {
        self.scope
            .lock()
            .expect("the fixture's scope lock is not poisoned")
            .push(tool.to_owned());
    }

    fn revoke(&self, tool: &str) {
        self.scope
            .lock()
            .expect("the fixture's scope lock is not poisoned")
            .retain(|name| name != tool);
    }
}

impl ServerHandler for FakeNotes {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("fixture-nuclear-notes", "0.0.0");
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        self.tools_list_calls.fetch_add(1, Ordering::SeqCst);
        let schema: JsonObject = serde_json::from_value(serde_json::json!({"type": "object"}))
            .expect("the fixture's schema is an object");
        let schema = Arc::new(schema);
        let tools = self
            .scope
            .lock()
            .expect("the fixture's scope lock is not poisoned")
            .iter()
            .map(|name| Tool::new(name.clone(), "a fixture tool", Arc::clone(&schema)))
            .collect();
        Ok(ListToolsResult {
            tools,
            ..ListToolsResult::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        // ADR-0135's three gates take a tool out of `tools/list`, so calling
        // one the token no longer grants is calling a method that is not there.
        let known = self
            .scope
            .lock()
            .expect("the fixture's scope lock is not poisoned")
            .contains(&request.name.to_string());
        if !known {
            return Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("no such tool: {}", request.name),
                None,
            ));
        }
        let arguments = request.arguments.clone().unwrap_or_default();
        let workspace = arguments
            .get("workspace")
            .and_then(|value| value.as_str())
            .ok_or_else(|| McpError::invalid_params("pages.read needs a workspace", None))?;
        let path = arguments
            .get("pathOrId")
            .and_then(|value| value.as_str())
            .ok_or_else(|| McpError::invalid_params("pages.read needs a pathOrId", None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "{path} as read from {workspace}"
        ))])
        .into())
    }
}

// -- the endpoint, implemented from outside both crates ----------------------

struct InProcess {
    server: FakeNotes,
    wire: Arc<Mutex<Vec<u8>>>,
    handed: Arc<Mutex<Vec<String>>>,
    started: Mutex<Option<mpsc::UnboundedSender<RunningService<RoleServer, FakeNotes>>>>,
}

impl Endpoint for InProcess {
    type Transport = DuplexStream;
    type TransportError = std::io::Error;
    type Adapter = TransportAdapterAsyncCombinedRW;

    async fn open(
        &self,
        _instance: &Instance,
        bearer: &Bearer,
    ) -> Result<DuplexStream, EndpointFailure> {
        // A real endpoint would put this in an Authorization header. This one
        // records that it was handed the value, which is the discriminating
        // arm of the redaction assertions: without it, a conversion that
        // dropped the credential would pass every absence check perfectly.
        self.handed
            .lock()
            .expect("the endpoint's record lock is not poisoned")
            .push(bearer.expose_for_dispatch().to_owned());

        let (client_end, mid_a) = tokio::io::duplex(64 * 1024);
        let (mid_b, server_end) = tokio::io::duplex(64 * 1024);
        let (read_a, write_a) = tokio::io::split(mid_a);
        let (read_b, write_b) = tokio::io::split(mid_b);

        pump(read_a, write_b, Arc::clone(&self.wire));
        pump(read_b, write_a, Arc::clone(&self.wire));

        let handler = self.server.clone();
        let started = self
            .started
            .lock()
            .expect("the endpoint's start lock is not poisoned")
            .clone();
        tokio::spawn(async move {
            // `serve_server` blocks until the client sends `initialize`, so it
            // has to run concurrently with `serve_client` or the two deadlock.
            match serve_server(handler, server_end).await {
                Ok(service) => match started {
                    Some(tx) => {
                        let _ = tx.send(service);
                    }
                    // **No channel means this task owns the handle**, and it
                    // must hold it: dropping a `RunningService` shuts the
                    // server down, so a caller that does not want to manage
                    // the handle would otherwise get one `initialize` and
                    // then `Transport closed` on its first real call. Waiting
                    // here keeps it until the client hangs up.
                    None => {
                        let _ = service.waiting().await;
                    }
                },
                Err(error) => panic!("the fixture server failed to start: {error}"),
            }
        });

        Ok(client_end)
    }
}

/// Copy bytes one way, keeping a transcript of everything that crossed.
fn pump(
    mut source: tokio::io::ReadHalf<DuplexStream>,
    mut sink: tokio::io::WriteHalf<DuplexStream>,
    wire: Arc<Mutex<Vec<u8>>>,
) {
    tokio::spawn(async move {
        let mut buffer = [0_u8; 8192];
        loop {
            match source.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    wire.lock()
                        .expect("the wire lock is not poisoned")
                        .extend_from_slice(&buffer[..read]);
                    if sink.write_all(&buffer[..read]).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
}

// -- the staging -------------------------------------------------------------

/// A store, a session over it, and everything a check needs to read both.
struct Wired {
    base: PathBuf,
    store: CredentialStore,
    alias: Alias,
    session: Session,
    server: FakeNotes,
    wire: Arc<Mutex<Vec<u8>>>,
    handed: Arc<Mutex<Vec<String>>>,
    planted: String,
    clock: ManualClock,
    peer: RunningService<RoleServer, FakeNotes>,
}

impl Wired {
    fn wire_text(&self) -> String {
        String::from_utf8(
            self.wire
                .lock()
                .expect("the wire lock is not poisoned")
                .clone(),
        )
        .expect("the protocol is UTF-8 JSON")
    }

    /// How many `tools/list` request frames actually crossed the wire.
    ///
    /// The second reader. A response carries no `method`, so this counts
    /// requests, and a paginated second round trip would show up here as well
    /// as in the server's own counter.
    fn tools_list_frames(&self) -> usize {
        self.wire_text().matches(r#""method":"tools/list""#).count()
    }

    fn cached_names(&self) -> Vec<String> {
        self.store
            .record(&self.alias)
            .expect("the token is stored")
            .tools()
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect()
    }

    fn projected_names(&self) -> Vec<String> {
        self.store
            .agent_namespaces()
            .into_iter()
            .find(|namespace| namespace.name.ends_with(self.alias.as_str()))
            .expect("the token projects a namespace")
            .tools
            .iter()
            .map(|tool| tool.name().to_owned())
            .collect()
    }
}

impl Drop for Wired {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Store a token, take its secret back out, convert it, and attach a session.
async fn wire() -> Wired {
    let base = std::env::temp_dir().join(nonce("ncw-outside"));
    let root = base.join("zaru");
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(&root).expect("a fresh root opens");

    let alias = Alias::new(&nonce("agent")).expect("a nonce is a legal alias");
    let planted = format!("nn_mcp_{}", nonce("secret"));
    store
        .add(
            Entry::notes(
                alias.clone(),
                Description::new("the agent's research").expect("one line"),
                Secret::notes(planted.clone()).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(TokenInstance::new("100monkeys-ai.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::of_names(STALE_SCOPE))
            .with_workspace("zaru"),
            &keys,
            None,
        )
        .expect("the token is stored");

    // A second Nuclear Notes token, which is staging rather than subject.
    // ADR-0007 D5's exclusion predicate became "the token the composer reads
    // with" on 2026-09-15, and with one stored token that is this one -- so a
    // store of one projects nothing and the projection arm below would read an
    // absence rather than a refreshed scope. Two puts `composer_token` in its
    // several-with-no-role case, where it serves nothing and both are the
    // agent's, which is the state this check is actually about.
    store
        .add(
            Entry::notes(
                Alias::new(&nonce("beside")).expect("a nonce is a legal alias"),
                Description::new("a second context").expect("one line"),
                Secret::notes(format!("nn_mcp_{}", nonce("second"))).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(TokenInstance::new("100monkeys-ai.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry"),
            &keys,
            None,
        )
        .expect("the second token is stored");

    // The bearer comes back through the sealing port, which is its only path,
    // and crosses into `zaru-notes` through the one named door.
    let secret = store.secret(&alias, &keys).expect("the port holds it");
    let bearer = bearer_for_dispatch(&secret);

    let server = FakeNotes::new();
    let wire = Arc::new(Mutex::new(Vec::new()));
    let handed = Arc::new(Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let endpoint = InProcess {
        server: server.clone(),
        wire: Arc::clone(&wire),
        handed: Arc::clone(&handed),
        started: Mutex::new(Some(tx)),
    };

    let session = Session::attach(
        &endpoint,
        Instance::new("100monkeys-ai.cortex.page"),
        bearer,
    )
    .await
    .expect("the fixture session attaches");
    let peer = rx.recv().await.expect("the fixture server started");

    Wired {
        base,
        store,
        alias,
        session,
        server,
        wire,
        handed,
        planted,
        clock: ManualClock::default(),
        peer,
    }
}

fn scope_of(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

// -- the checks --------------------------------------------------------------

#[tokio::test]
async fn a_stored_secret_reaches_the_endpoint_and_nothing_else() {
    let wired = wire().await;

    // The discriminating arm first: the value really did travel, exactly once,
    // to the one place ADR-0007 D3 puts it.
    let handed = wired
        .handed
        .lock()
        .expect("the endpoint's record lock is not poisoned")
        .clone();
    assert_eq!(
        handed,
        vec![wired.planted.clone()],
        "the endpoint is the dispatch path and must have been handed the bearer exactly once"
    );

    // And nowhere else: not in the session, not on the wire, and not in the
    // file the store wrote.
    assert_absent(
        "a session's Debug",
        &format!("{:?}", wired.session),
        &wired.planted,
    );
    assert_absent("the wire", &wired.wire_text(), &wired.planted);
    let on_disk = std::fs::read_to_string(wired.store.path()).expect("the store wrote a file");
    assert!(
        on_disk.contains(wired.alias.as_str()),
        "the file carries no entry at all, so the absence below asserts nothing"
    );
    assert_absent("the file on disk", &on_disk, &wired.planted);
}

#[tokio::test]
async fn adr_0007_d6_caches_the_scope_at_attach_with_exactly_one_tools_list() {
    let mut wired = wire().await;

    assert_eq!(
        wired.cached_names(),
        scope_of(&STALE_SCOPE),
        "the staging did not put a deliberately wrong scope in the cache"
    );
    assert_eq!(wired.server.calls(), 0, "something read tools/list already");

    wired.clock.advance(Duration::from_secs(7));
    let cached = wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the fixture answers tools/list");

    assert_eq!(
        cached.scope.names(),
        scope_of(&FIRST_SCOPE),
        "ADR-0007 D6: the response already reflects what the token grants, so the cache must not \
         reorder or interpret it"
    );
    assert_eq!(
        cached.at,
        Duration::from_secs(7),
        "the reading was not taken from the caller's clock"
    );
    assert_eq!(
        wired.cached_names(),
        scope_of(&FIRST_SCOPE),
        "the answer did not reach the entry, which is where D6 puts the cache"
    );
    assert_eq!(
        wired.server.calls(),
        1,
        "D6 calls tools/list once per token at attach"
    );
    assert_eq!(
        wired.tools_list_frames(),
        1,
        "the wire disagrees with the server's own count"
    );
}

#[tokio::test]
async fn adr_0007_d6s_first_signal_replaces_the_scope_with_exactly_one_tools_list() {
    let mut wired = wire().await;
    wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the scope caches at attach");

    assert!(
        wired.session.take_list_changed().is_none(),
        "nothing has happened yet, so there is no signal to take; a take that always answered \
         would make the assertion below meaningless"
    );

    wired.server.grant("kg.list_cross_links");
    wired
        .peer
        .peer()
        .notify_tool_list_changed()
        .await
        .expect("the fixture can notify");

    // Waiting on the condition, under a bound, rather than on a duration.
    let signal = tokio::time::timeout(Duration::from_secs(10), wired.session.await_list_changed())
        .await
        .expect("the notification arrived within the bound")
        .expect("the session was still connected");
    assert_eq!(signal, Invalidation::ListChanged);

    let before = wired.server.calls();
    let frames_before = wired.tools_list_frames();
    let refreshed = wired
        .store
        .refresh_tool_scope(&wired.alias, &wired.session, &signal, &wired.clock)
        .await
        .expect("the fixture answers tools/list");

    assert_eq!(refreshed.because, Invalidation::ListChanged);
    assert_eq!(
        wired.cached_names(),
        scope_of(&[
            "pages.read",
            "atoms.read",
            "search.global",
            "kg.list_cross_links"
        ]),
        "the refresh must pick up the change the signal announced"
    );
    assert_eq!(
        wired.server.calls() - before,
        1,
        "D6 refreshes with one tools/list per signal and never retries"
    );
    assert_eq!(
        wired.tools_list_frames() - frames_before,
        1,
        "the wire disagrees with the server's own count"
    );
    assert!(
        wired
            .wire_text()
            .contains("notifications/tools/list_changed"),
        "the notification did not cross the wire, so this check watched something else"
    );
}

#[tokio::test]
async fn adr_0007_d6s_ttl_backstop_replaces_the_scope_with_exactly_one_tools_list() {
    let mut wired = wire().await;
    let cached = wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the scope caches at attach");

    // The server changes without telling anybody, which is the missed
    // notification D6's backstop exists to bound.
    wired.server.grant("kg.list_cross_links");

    let ttl = Ttl::new(Duration::from_secs(300)).expect("a non-zero window");
    wired.clock.advance(Duration::from_secs(299));
    assert_eq!(
        cached.expired(wired.clock.now(), ttl),
        None,
        "the backstop fired one second inside its own window"
    );

    wired.clock.advance(Duration::from_secs(1));
    let signal = cached
        .expired(wired.clock.now(), ttl)
        .expect("the window has elapsed");
    assert_eq!(
        signal,
        Invalidation::Expired {
            window: Duration::from_secs(300),
            elapsed: Duration::from_secs(300),
        }
    );

    let before = wired.server.calls();
    let frames_before = wired.tools_list_frames();
    let refreshed = wired
        .store
        .refresh_tool_scope(&wired.alias, &wired.session, &signal, &wired.clock)
        .await
        .expect("the fixture answers tools/list");

    assert_eq!(refreshed.because, signal);
    assert_eq!(
        refreshed.cached.at,
        Duration::from_secs(300),
        "the fresh reading did not come from the caller's clock"
    );
    assert!(
        wired
            .cached_names()
            .contains(&"kg.list_cross_links".to_owned()),
        "the backstop refreshed nothing: {:?}",
        wired.cached_names()
    );
    assert_eq!(
        wired.server.calls() - before,
        1,
        "D6 refreshes with one tools/list per signal and never retries"
    );
    assert_eq!(wired.tools_list_frames() - frames_before, 1);
}

#[tokio::test]
async fn adr_0007_d6s_claimed_refusal_refreshes_and_the_original_failure_survives() {
    let mut wired = wire().await;
    wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the scope caches at attach");
    assert!(
        wired.cached_names().contains(&"pages.read".to_owned()),
        "the cache must claim the tool this check is about"
    );

    // ADR-0135's three gates take the tool out of the token's scope. A caller
    // whose cache still claims it calls it anyway, which is the situation D6's
    // third signal exists for.
    wired.server.revoke("pages.read");
    let error = wired
        .session
        .read_page("home", &WorkspaceId::new(WORKSPACE))
        .await
        .expect_err("a tool outside the scope is refused");
    let NotesError::Call(refused) = error else {
        panic!("a refused tool call must surface as NotesError::Call, not as {error:?}");
    };
    assert!(
        refused.is_method_not_found(),
        "the fixture sent code {}",
        refused.code
    );

    // The gate is over the entry's OWN cached scope, which is what D6 means by
    // "a tool the cache claimed".
    let claimed = wired.cached_names();
    let signal = Invalidation::claimed(refused.clone(), &claimed)
        .expect("the cache claimed the tool the server refused");

    let before = wired.server.calls();
    let frames_before = wired.tools_list_frames();
    let refreshed = wired
        .store
        .refresh_tool_scope(&wired.alias, &wired.session, &signal, &wired.clock)
        .await
        .expect("the fixture answers tools/list");

    // D6: "Refresh, then surface the failure -- never retry blind." The
    // refusal is in the caller's hand twice: its own copy, which the refresh
    // could not consume because it took the signal by reference, and the one
    // the signal carries.
    assert_eq!(
        refused.tool, "pages.read",
        "the caller's own copy of the failure did not survive the refresh"
    );
    assert_eq!(refreshed.because, Invalidation::Claimed(refused.clone()));

    assert!(
        !wired.cached_names().contains(&"pages.read".to_owned()),
        "the stale claim survived the refresh: {:?}",
        wired.cached_names()
    );
    assert_eq!(
        wired.server.calls() - before,
        1,
        "D6 refreshes once and never retries the call that failed"
    );
    assert_eq!(wired.tools_list_frames() - frames_before, 1);

    // The other direction, over the harness's own gate: a refusal for a tool
    // nothing claimed says nothing about a cache and comes back unchanged.
    let never_claimed: Vec<String> = Vec::new();
    assert_eq!(
        Invalidation::claimed(refused.clone(), &never_claimed),
        Err(refused),
        "a refusal the cache never claimed must not be swallowed by a staleness test"
    );
}

#[tokio::test]
async fn a_refreshed_scope_reaches_the_agents_namespace_in_the_same_read() {
    let mut wired = wire().await;
    wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the scope caches at attach");

    let before = wired.projected_names();
    assert_eq!(
        before,
        scope_of(&FIRST_SCOPE),
        "the projection did not start from the cached scope"
    );

    wired.server.grant("kg.list_cross_links");
    wired
        .peer
        .peer()
        .notify_tool_list_changed()
        .await
        .expect("the fixture can notify");
    let signal = tokio::time::timeout(Duration::from_secs(10), wired.session.await_list_changed())
        .await
        .expect("the notification arrived within the bound")
        .expect("the session was still connected");

    wired
        .store
        .refresh_tool_scope(&wired.alias, &wired.session, &signal, &wired.clock)
        .await
        .expect("the fixture answers tools/list");

    // ADR-0007 D6: "The cache is what the harness renders to the human and
    // projects to the agent. One read, two consumers."
    assert_eq!(
        wired.projected_names(),
        scope_of(&[
            "pages.read",
            "atoms.read",
            "search.global",
            "kg.list_cross_links"
        ]),
        "the refreshed scope did not reach D5's projection in the same read"
    );
}

#[tokio::test]
async fn what_the_wiring_offers_is_printed_for_a_reader() {
    // Not an assertion. The capture a report quotes, so a reader sees what the
    // wiring actually is rather than a list of check names.
    let mut wired = wire().await;
    println!("negotiated: {:?}", wired.session.negotiated());
    println!("cached at attach: {:?}", wired.cached_names());

    let cached = wired
        .store
        .cache_tool_scope(&wired.alias, &wired.session, &wired.clock)
        .await
        .expect("the fixture answers tools/list");
    println!("after one tools/list: {cached:?}");

    wired.server.grant("kg.list_cross_links");
    wired
        .peer
        .peer()
        .notify_tool_list_changed()
        .await
        .expect("the fixture can notify");
    let signal = tokio::time::timeout(Duration::from_secs(10), wired.session.await_list_changed())
        .await
        .expect("the notification arrived")
        .expect("the session was still connected");
    println!("signal: {signal} ({signal:?})");

    let refreshed = wired
        .store
        .refresh_tool_scope(&wired.alias, &wired.session, &signal, &wired.clock)
        .await
        .expect("the fixture answers tools/list");
    println!("refreshed: {refreshed:?}");
    println!(
        "the agent's namespaces: {:?}",
        wired.store.agent_namespaces()
    );
    println!("tools/list calls at the server: {}", wired.server.calls());
    println!("--- the file on disk ---");
    println!(
        "{}",
        std::fs::read_to_string(wired.store.path()).expect("the store wrote a file")
    );
    println!("--- the wire ---");
    for line in wired.wire_text().lines() {
        println!("    {line}");
    }
    println!("session: {:?}", wired.session);
}

// --- ADR-0007 D5's projection, end to end, with no socket -------------------
//
// # Why there is no loopback listener here
//
// The coordinator's ruling of 2026-09-15 asked for this half "against a
// loopback instance". **The standing ruling of 2026-09-14 forbids exactly
// that** -- a loopback listener standing in for a server -- and it is recorded
// in three places in this workspace, most plainly in
// `tests/transport_from_outside.rs`: "a fake of a provider at the wire is the
// mock that [Testing] refuses". A standing ruling is not a brief's to lift, so
// what is built is the shape this crate already uses and which loses nothing
// the ruling was protecting: `rmcp` over `tokio::io::duplex`, where every
// frame is serialised, framed and parsed by `rmcp`'s own codec exactly as it
// would be over a network, and no socket is opened.
//
// What that leaves unexercised is the `reqwest` call inside `HttpEndpoint`,
// which is the same thing `zaru-notes` leaves unexercised for the same reason
// and says so in `session::transport::request`'s own documentation.

/// A store holding one Notes token, a key, and a projection over the
/// in-process server.
struct Projected {
    base: PathBuf,
    store: zaru_cli::credentials::CredentialStore,
    keys: StagedKey,
    alias: Alias,
    server: FakeNotes,
    wire: Arc<Mutex<Vec<u8>>>,
    handed: Arc<Mutex<Vec<String>>>,
    planted: String,
}

impl Drop for Projected {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

impl Projected {
    fn wire_text(&self) -> String {
        String::from_utf8(
            self.wire
                .lock()
                .expect("the wire lock is not poisoned")
                .clone(),
        )
        .expect("the protocol is UTF-8 JSON")
    }

    fn endpoint(&self) -> InProcess {
        InProcess {
            server: self.server.clone(),
            wire: Arc::clone(&self.wire),
            handed: Arc::clone(&self.handed),
            started: Mutex::new(None),
        }
    }
}

/// Two Notes tokens, so `composer_token` serves neither and both project.
fn projected_store() -> Projected {
    let base = std::env::temp_dir().join(nonce("ncp-outside"));
    let root = base.join("zaru");
    let keys = StagedKey::minted();
    let mut store =
        zaru_cli::credentials::CredentialStore::open(&root).expect("a fresh root opens");

    let alias = Alias::new(&nonce("play")).expect("a nonce is a legal alias");
    let planted = format!("nn_mcp_{}", nonce("secret"));
    store
        .add(
            Entry::notes(
                alias.clone(),
                Description::new("the cortex I share with the team").expect("one line"),
                Secret::notes(planted.clone()).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(TokenInstance::new("play.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry"),
            &keys,
            None,
        )
        .expect("the token is stored");
    // The second token is staging: with one, `composer_token` answers this
    // alias and D5 never projects it. See `credentials::projection`.
    store
        .add(
            Entry::notes(
                Alias::new(&nonce("beside")).expect("a nonce is a legal alias"),
                Description::new("a second context").expect("one line"),
                Secret::notes(format!("nn_mcp_{}", nonce("second"))).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(TokenInstance::new("play.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry"),
            &keys,
            None,
        )
        .expect("the second token is stored");

    Projected {
        base,
        store,
        keys,
        alias,
        server: FakeNotes::new(),
        wire: Arc::new(Mutex::new(Vec::new())),
        handed: Arc::new(Mutex::new(Vec::new())),
        planted,
    }
}

/// **ADR-0007 D5, whole, over real protocol bytes.**
///
/// The cache is filled by one `tools/list`, the declaration the model is
/// offered is built from it, the model's call goes out over the wire, and the
/// answer comes back -- through `zaru-cli`'s public door, with the bearer
/// resolved from the store and never named by the caller.
///
/// The mutant: make `Projection::call` open a session per call rather than
/// once per alias, which the frame count catches.
#[tokio::test]
async fn adr_0007_d5_a_granted_tool_is_declared_called_and_answered_over_real_protocol_bytes() {
    let mut staged = projected_store();

    // D6's cache, filled by one `tools/list` through the product's own door.
    let endpoint = staged.endpoint();
    let session = Session::attach(
        &endpoint,
        NotesInstance::new("play.cortex.page"),
        bearer_for_dispatch(
            &staged
                .store
                .secret(&staged.alias, &staged.keys)
                .expect("the port holds it"),
        ),
    )
    .await
    .expect("the in-process server attaches");
    staged
        .store
        .cache_tool_scope(
            &staged.alias,
            &session,
            &ManualClock {
                elapsed: Mutex::new(Duration::from_secs(0)),
            },
        )
        .await
        .expect("one tools/list fills the cache");
    drop(session);

    // The scope now carries declarations rather than names, which is what D5
    // needs and what a store written before 2026-09-15 does not have.
    let scope = staged
        .store
        .record(&staged.alias)
        .expect("the token is stored")
        .tools_scope()
        .expect("a Notes token has a scope");
    assert!(
        scope.is_declarable(),
        "the cache came back without schemas, so nothing could be declared"
    );

    // The surface the model is offered, built from that cache and one grant.
    let namespaces = staged.store.agent_namespaces();
    let grant = granted_for(&staged.store, &staged.alias, &["pages.read"]);
    let declared = zaru_cli::tools::surface(&namespaces, |asked| {
        if asked == &staged.alias {
            &grant
        } else {
            &NOTHING
        }
    })
    .expect("nothing collides");
    let name = format!("notes:{}.pages.read", staged.alias);
    let offered = declared
        .iter()
        .find(|descriptor| descriptor.name == name)
        .unwrap_or_else(|| panic!("`{name}` was not declared: {declared:?}"));
    assert_eq!(
        offered.parameters, r#"{"type":"object"}"#,
        "the schema the model is shown is the server's own bytes"
    );
    assert!(
        offered
            .description
            .contains("the cortex I share with the team"),
        "ADR-0007 D2: the token's description reaches the agent: {}",
        offered.description
    );
    // And the tool the person did not grant is absent rather than refused.
    assert!(
        !declared
            .iter()
            .any(|descriptor| descriptor.name.ends_with(".atoms.read")),
        "an ungranted tool was offered: {declared:?}"
    );

    // The call, through the port, over the wire.
    let projection =
        zaru_cli::credentials::Projection::over(&staged.store, &staged.keys, staged.endpoint());
    let captured = zaru_cli::tools::Projected::call(
        &projection,
        &staged.alias,
        "pages.read",
        &format!(r#"{{"pathOrId":"home","workspace":"{WORKSPACE}"}}"#),
    )
    .await
    .expect("the in-process server answers");
    assert_eq!(captured.exit_code, 0, "{captured:?}");
    assert!(
        captured
            .stdout
            .contains(&format!("home as read from {WORKSPACE}")),
        "the server's own answer did not come back: {captured:?}"
    );

    // A second call into the same alias reuses the session: one `initialize`
    // on the wire for the projection's own connection, not two.
    let before = staged
        .wire_text()
        .matches(r#""method":"initialize""#)
        .count();
    zaru_cli::tools::Projected::call(
        &projection,
        &staged.alias,
        "pages.read",
        &format!(r#"{{"pathOrId":"second","workspace":"{WORKSPACE}"}}"#),
    )
    .await
    .expect("the second call answers");
    assert_eq!(
        staged
            .wire_text()
            .matches(r#""method":"initialize""#)
            .count(),
        before,
        "the projection opened a second session for the same alias"
    );

    // The bearer crossed to the endpoint and is not in anything rendered.
    assert!(
        staged
            .handed
            .lock()
            .expect("lock")
            .contains(&staged.planted),
        "the endpoint was never handed the bearer, so the absence checks below assert nothing"
    );
    let rendered = format!("{declared:?}{captured:?}{projection:?}");
    assert!(
        !rendered.contains(&staged.planted),
        "a bearer value reached the agent's surface"
    );
}

/// **The instance's refusal is a tool result, not a port failure.**
///
/// This is the half a test double could not establish: the executor check uses
/// one, so the mutant that turns a refusal into a `PortFailure` compiles and
/// reddens nothing there. Here the refusal comes from a real `rmcp` server
/// over real frames, through the product's own `Projection`.
///
/// The mutant: `Err(refused) => Err(PortFailure::new(refused.to_string()))`.
#[tokio::test]
async fn adr_0016_an_instances_refusal_comes_back_as_a_result_and_a_transport_failure_does_not() {
    let staged = projected_store();
    let projection =
        zaru_cli::credentials::Projection::over(&staged.store, &staged.keys, staged.endpoint());

    // The refusing arm: a tool the fixture's scope does not carry, which is
    // what ADR-0135's three gates produce -- `METHOD_NOT_FOUND`.
    let captured = zaru_cli::tools::Projected::call(
        &projection,
        &staged.alias,
        "pages.apply_patch",
        r#"{"pathOrId":"home"}"#,
    )
    .await
    .expect("an instance refusing is not a port failure, and that is the whole point");
    assert_eq!(captured.exit_code, 1, "a refusal is a failed result");
    assert!(
        captured.stderr.contains("pages.apply_patch"),
        "the server's own refusal did not come back: {captured:?}"
    );

    // The accepting sibling, so a refuse-everything implementation cannot
    // pass: a tool it does carry answers.
    let captured = zaru_cli::tools::Projected::call(
        &projection,
        &staged.alias,
        "pages.read",
        &format!(r#"{{"pathOrId":"home","workspace":"{WORKSPACE}"}}"#),
    )
    .await
    .expect("the in-process server answers");
    assert_eq!(captured.exit_code, 0, "{captured:?}");
}

/// A grant of `names` for `alias`, built through the product's own reader.
fn granted_for(
    store: &zaru_cli::credentials::CredentialStore,
    alias: &Alias,
    names: &[&str],
) -> zaru_cli::credentials::Granted {
    use zaru_cli::config::{
        Contribution, Field, FieldKind, Layer, Resolution, Schema, Source, Table, Value,
    };
    let schema = Schema::new().with_family(
        zaru_cli::credentials::grant::PREFIX,
        zaru_cli::credentials::grant::SUFFIX,
        Field::free(FieldKind::Array),
    );
    let mut document = Table::new();
    document.insert_path(
        &zaru_cli::credentials::grant::key(alias),
        Value::Array(
            names
                .iter()
                .map(|name| Value::Text((*name).to_owned()))
                .collect(),
        ),
    );
    let resolution = Resolution::resolve(
        &schema,
        [Contribution::new(
            Layer::User,
            Source::named("staged"),
            document,
        )],
    )
    .expect("a permissive schema takes an array at the user's layer");
    let cached: Vec<String> = store
        .record(alias)
        .expect("the token is stored")
        .tools()
        .iter()
        .map(|tool| tool.name().to_owned())
        .collect();
    let cached: Vec<&str> = cached.iter().map(String::as_str).collect();
    zaru_cli::credentials::Granted::from_configuration(&resolution, alias, &cached)
        .expect("every name is in the token's own scope")
}

static NOTHING: std::sync::LazyLock<zaru_cli::credentials::Granted> =
    std::sync::LazyLock::new(zaru_cli::credentials::Granted::nothing);

/// [ADR-0027] D1's persona, read over real protocol bytes through the narrow
/// port.
///
/// **The whole point of this check is the wire.** `persona_from` takes
/// `&impl Persona`, and a staged implementation of that trait proves the
/// signature and nothing about what a Nuclear Notes instance is asked. This
/// drives it over `rmcp` and `tokio::io::duplex` against the same fixture
/// every other check in this file uses, and reads the frames back: exactly one
/// `pages.read`, with the workspace named on the call, and **no `pages.list`,
/// no `atoms.list` and no `me.set_current_workspace`** — so the persona's port
/// is a second reader of [ADR-0006] D4's set rather than a wider one, and
/// [ADR-0006] D2's "the composer's pointer moves only by user action" is
/// untouched by it.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[tokio::test]
async fn adr_0027s_persona_is_one_pages_read_over_the_wire_and_moves_no_pointer() {
    let wired = wire().await;
    let before = wired.wire_text();

    let body = zaru_cli::credentials::persona_from(&wired.session, "zaru/persona", WORKSPACE)
        .await
        .expect("the fixture serves the page");

    assert_eq!(
        body,
        format!("zaru/persona as read from {WORKSPACE}"),
        "the body did not come back as the tool gave it, so something between the server and \
         layer 1 is paraphrasing a page this harness does not own"
    );

    let after = wired.wire_text();
    let sent = after
        .strip_prefix(&before)
        .expect("the transcript only grows")
        .to_owned();

    assert_eq!(
        sent.matches("\"pages.read\"").count(),
        1,
        "a persona is one read; the frames say otherwise: {sent}"
    );
    assert!(
        sent.contains(WORKSPACE),
        "the workspace was not named on the call, so the read would have resolved against the \
         token's pointer instead: {sent}"
    );
    for forbidden in [
        "pages.list",
        "atoms.list",
        "me.set_current_workspace",
        "apply_patch",
    ] {
        assert!(
            !sent.contains(forbidden),
            "the persona's read reached `{forbidden}`, which is not what its port is for: {sent}"
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
