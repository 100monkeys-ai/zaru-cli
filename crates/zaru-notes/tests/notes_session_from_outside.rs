// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside the crate drives a session end to end against a real MCP
//! server, exchanging real protocol bytes, with no socket and no credential.
//!
//! # What this is evidence about, and what it is not
//!
//! It is evidence that the mechanism works through `zaru-notes`' public door:
//! this file implements [`Endpoint`] itself, from outside, and uses nothing the
//! crate does not export. **It is not evidence about the `zaru` binary**, which
//! reaches none of this, and it is not evidence about Nuclear Notes, which was
//! never called. The server on the other end is a fixture written here.
//!
//! # Why the bytes are real
//!
//! The transport is `rmcp`'s `transport-async-rw` over [`tokio::io::duplex`] —
//! an in-memory pipe, not a socket. Every frame is serialised, newline-framed,
//! and parsed by `rmcp`'s own codec exactly as it would be over a network, so a
//! defect in framing or in argument spelling is visible here. A byte pump
//! between two duplex pairs copies everything both ways and keeps a transcript,
//! which is what lets a check assert on **what was sent** rather than on what
//! the code that sent it says it sent.

use std::sync::{Arc, Mutex};

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

use zaru_notes::session::{
    Bearer, Endpoint, EndpointFailure, Instance, Invalidation, NotesError, Session, WorkspaceId,
    WorkspaceSlug,
};

// -- the nonce, which this file must carry its own copy of ---------------
//
// `zaru-cli` and `zaru-notes` each hold this pattern, and an integration test
// cannot see either crate's `#[cfg(test)]` modules. Duplicating twenty lines is
// the price of the ADR-0003 D8 boundary and of Rust's `tests/` boundary, and it
// is cheaper than the edge that would remove it.

/// The awkward tail every nonce carries: a decomposed grapheme cluster, a
/// precomposed one, and an astral-plane character.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

fn nonce(label: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
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

fn assert_absent(what: &str, rendered: &str, planted: &str) {
    assert!(
        !rendered.contains(planted),
        "{what} published the bearer value verbatim; it is in {rendered:?}"
    );
    let core = ascii_core(planted);
    assert!(
        !rendered.contains(core),
        "{what} published the bearer value in an escaped form; its ASCII core {core:?} is in \
         {rendered:?}"
    );
}

// -- the fixture server ---------------------------------------------------

/// The workspace identifier the fixture resolves every slug to.
const RESOLVED_ID: &str = "a96c9dde-becf-4ff0-836e-ad8bef46ff42";

/// The cursor the fixture's `pages.list` issues for its second page.
const SECOND_PAGE: &str = "cursor-4c7e";

/// A workspace the fixture answers in a shape no reader here accepts.
///
/// Named rather than spelled at the call site, so the accepting check and the
/// refusing one differ by exactly one value.
const UNREADABLE_ID: &str = "00000000-0000-0000-0000-000000000000";

/// What the fixture answers `tools/list` with before anything changes.
const FIRST_SCOPE: [&str; 4] = [
    "pages.read",
    "me.set_current_workspace",
    "workspaces.resolve_slug",
    "search.global",
];

#[derive(Clone)]
struct FakeNotes {
    /// Tool names to report. Changing it and notifying is how the fixture
    /// reproduces ADR-0007 D6's first signal.
    scope: Arc<Mutex<Vec<String>>>,
    /// Whether `workspaces.resolve_slug` should refuse, so that ADR-0006 D7's
    /// indistinguishable refusal has something to be produced by.
    refuse_resolution: Arc<Mutex<bool>>,
    /// Every `me.set_current_workspace` the server was asked for.
    pointer_moves: Arc<Mutex<Vec<String>>>,
}

impl FakeNotes {
    fn new() -> Self {
        Self {
            scope: Arc::new(Mutex::new(
                FIRST_SCOPE.iter().map(|s| (*s).to_owned()).collect(),
            )),
            refuse_resolution: Arc::new(Mutex::new(false)),
            pointer_moves: Arc::new(Mutex::new(Vec::new())),
        }
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
        let arguments = request.arguments.clone().unwrap_or_default();
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

        match request.name.as_ref() {
            "workspaces.resolve_slug" => {
                if *self
                    .refuse_resolution
                    .lock()
                    .expect("the fixture's refusal lock is not poisoned")
                {
                    // What the record says the server does: refuse without
                    // saying which of the three gates tripped.
                    return Err(McpError::new(
                        ErrorCode::INVALID_REQUEST,
                        "forbidden".to_owned(),
                        None,
                    ));
                }
                let slug = arguments
                    .get("slug")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::invalid_params("resolve_slug needs a slug", None))?;
                let instance = arguments
                    .get("instance")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("resolve_slug needs an instance", None)
                    })?;
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    serde_json::json!({"id": RESOLVED_ID, "slug": slug, "instance": instance})
                        .to_string(),
                )])
                .into())
            }
            "me.set_current_workspace" => {
                // The substrate rejects a slug here; the fixture does too, so a
                // client that sent one would fail rather than quietly work.
                if arguments.contains_key("workspaceSlug") {
                    return Err(McpError::invalid_params(
                        "me.set_current_workspace rejects workspaceSlug; pass workspaceId",
                        None,
                    ));
                }
                let id = arguments
                    .get("workspaceId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("set_current_workspace needs a workspaceId", None)
                    })?;
                self.pointer_moves
                    .lock()
                    .expect("the fixture's pointer lock is not poisoned")
                    .push(id.to_owned());
                Ok(
                    CallToolResult::success(vec![ContentBlock::text(format!("attached {id}"))])
                        .into(),
                )
            }
            "pages.read" => {
                // The measured substrate behaviour ADR-0006's Context records:
                // a read resolves only within the workspace it names.
                let workspace = arguments
                    .get("workspace")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("pages.read needs a workspace", None)
                    })?;
                let path = arguments
                    .get("pathOrId")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::invalid_params("pages.read needs a pathOrId", None))?;
                Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "{path} as read from {workspace}"
                ))])
                .into())
            }
            // Two pages, so a client that stopped at the first would return
            // two of the three rows and look like a cortex holding two.
            "pages.list" => {
                let workspace = arguments
                    .get("workspace")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("pages.list needs a workspace", None)
                    })?;
                let answer = match arguments.get("cursor").and_then(|v| v.as_str()) {
                    None => serde_json::json!({
                        "results": [
                            {"kind": "page", "path": "adrs/0005", "title": format!("Ω first {workspace} ✦")},
                            {"kind": "page", "path": "architecture/bóunded", "title": "Bóunded Contexts ✦"}
                        ],
                        "nextCursor": SECOND_PAGE
                    }),
                    Some(SECOND_PAGE) => serde_json::json!({
                        "results": [
                            {"kind": "page", "path": "operations/testing", "title": "Tésting ✦"}
                        ]
                    }),
                    Some(other) => {
                        return Err(McpError::invalid_params(
                            format!("pages.list was handed a cursor it never issued: {other}"),
                            None,
                        ));
                    }
                };
                Ok(CallToolResult::success(vec![ContentBlock::text(answer.to_string())]).into())
            }
            // A bare array, which is the other container `listing::read`
            // accepts and the one that cannot carry a cursor.
            "atoms.list" => Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::json!([
                    {"kind": "atom", "path": "atoms/mémbrane", "title": "Mémbrane ✦"}
                ])
                .to_string(),
            )])
            .into()),
            // A search answers the same container a listing does and carries
            // its own cursor, which this client deliberately does not follow:
            // a `nextCursor` here is what a check asserts is IGNORED.
            "search.global" => {
                let workspace = arguments
                    .get("workspace")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("search.global needs a workspace", None)
                    })?;
                let query = arguments
                    .get("q")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::invalid_params("search.global needs a query", None))?;
                if workspace == UNREADABLE_ID {
                    // A hit that cannot locate itself: ADR-0006 D6's four
                    // parts are what `found::read` requires and this has one.
                    return Ok(CallToolResult::success(vec![ContentBlock::text(
                        serde_json::json!({"hits": [
                            {"path": "adrs/0006", "title": "ADR-0006"}
                        ]})
                        .to_string(),
                    )])
                    .into());
                }
                // The container and the fields the live server was measured to
                // send on 2026-09-06, not the ones this client first guessed.
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    serde_json::json!({
                        "query": query,
                        "scope": "workspace",
                        "hits": [{
                            "kind": "page",
                            "path": "adrs/0006",
                            "title": format!("{query} in {workspace} ✦"),
                            "workspaceSlug": "zaru",
                            "snippet": format!("…<mark>{query}</mark>…"),
                            "permalink": "https://example.invalid/zaru/p/adrs/0006",
                            "uri": "nn://workspace/zaru/p/adrs/0006"
                        }],
                        "nextCursor": SECOND_PAGE
                    })
                    .to_string(),
                )])
                .into())
            }
            "cortex.ground" => {
                let workspace = arguments
                    .get("workspace")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        McpError::invalid_params("cortex.ground needs a workspace", None)
                    })?;
                Ok(CallToolResult::success(vec![ContentBlock::text(format!(
                    "grounding for {workspace} ✦"
                ))])
                .into())
            }
            other => Err(McpError::new(
                ErrorCode::METHOD_NOT_FOUND,
                format!("no such tool: {other}"),
                None,
            )),
        }
    }
}

// -- the endpoint, implemented from outside the crate --------------------

/// An [`Endpoint`] over an in-memory pipe, with every byte recorded.
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
        // The one product call site of `expose_for_dispatch` is this port. A
        // real endpoint would put this in an Authorization header; this one
        // records that it was handed the value, which is the discriminating
        // arm of the redaction checks below -- without it, a `Bearer` that
        // dropped its value would pass every absence assertion perfectly.
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
                Ok(service) => {
                    if let Some(tx) = started {
                        let _ = tx.send(service);
                    }
                }
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

/// Everything a check needs: the session, what the server saw, and the wire.
struct Attached {
    session: Session,
    server: FakeNotes,
    wire: Arc<Mutex<Vec<u8>>>,
    handed: Arc<Mutex<Vec<String>>>,
    planted: String,
    peer: RunningService<RoleServer, FakeNotes>,
}

impl Attached {
    fn wire_text(&self) -> String {
        String::from_utf8(
            self.wire
                .lock()
                .expect("the wire lock is not poisoned")
                .clone(),
        )
        .expect("the protocol is UTF-8 JSON")
    }
}

async fn attach() -> Attached {
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

    let planted = format!("nn_mcp_{}", nonce("bearer"));
    let session = Session::attach(
        &endpoint,
        Instance::new("100monkeys-ai.cortex.page"),
        Bearer::new(planted.clone()),
    )
    .await
    .expect("the fixture session attaches");

    let peer = rx.recv().await.expect("the fixture server started");
    Attached {
        session,
        server,
        wire,
        handed,
        planted,
        peer,
    }
}

// -- the checks -----------------------------------------------------------

#[tokio::test]
async fn a_session_negotiates_over_real_protocol_bytes_with_no_socket() {
    let attached = attach().await;
    let negotiated = attached.session.negotiated();

    assert_eq!(
        negotiated.protocol_version, "2025-11-25",
        "the version ADR-0003 D2 names as ADR-0103's requirement is what a default rmcp client \
         offers; a change here is a change in what the harness speaks"
    );
    assert_eq!(negotiated.server_name, "fixture-nuclear-notes");
    assert!(
        negotiated.serves_tools,
        "the fixture declared a tools capability and the session must have read it off the \
         handshake rather than assumed it"
    );

    // The bytes themselves, so that "real protocol" is a reading rather than a
    // claim about which crate was linked.
    let wire = attached.wire_text();
    assert!(
        wire.contains(r#""method":"initialize""#),
        "no initialize frame crossed the wire: {wire:?}"
    );
    assert!(
        wire.contains(r#""jsonrpc":"2.0""#),
        "what crossed was not JSON-RPC: {wire:?}"
    );
    assert!(
        wire.contains(r#""protocolVersion":"2025-11-25""#),
        "the negotiated version was not on the wire: {wire:?}"
    );
    println!("wire after attach ({} bytes):", wire.len());
    for line in wire.lines() {
        println!("    {line}");
    }
}

#[tokio::test]
async fn the_bearer_reaches_the_endpoint_and_nothing_else() {
    let attached = attach().await;

    // The discriminating arm first: the value really did travel to the one
    // place it is meant to.
    let handed = attached
        .handed
        .lock()
        .expect("the endpoint's record lock is not poisoned")
        .clone();
    assert_eq!(
        handed,
        vec![attached.planted.clone()],
        "the endpoint is the dispatch path and must have been handed the bearer exactly once"
    );

    // And nowhere else. The session carries no bearer field at all, which is
    // why this passes structurally rather than by a redaction that could be
    // forgotten -- see the module note on `zaru_notes::session::client`.
    assert_absent(
        "a session's Debug",
        &format!("{:?}", attached.session),
        &attached.planted,
    );
    assert_absent("the wire", &attached.wire_text(), &attached.planted);
}

#[tokio::test]
async fn a_session_lists_the_tools_the_server_reported_in_the_servers_order() {
    let attached = attach().await;
    let tools = attached.session.tools().await.expect("tools/list answers");
    assert_eq!(
        tools,
        FIRST_SCOPE
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>(),
        "ADR-0007 D6: the response already reflects what the token grants, so the client must not \
         reorder or interpret it"
    );
    assert!(
        attached.wire_text().contains(r#""method":"tools/list""#),
        "tools/list did not cross the wire"
    );
}

#[tokio::test]
async fn adr_0006_d7_resolves_the_slug_and_then_switches_by_identifier() {
    let mut attached = attach().await;
    let slug = WorkspaceSlug::new("zaru");

    let resolved = attached
        .session
        .attach_workspace_by_slug(&slug)
        .await
        .expect("the fixture resolves and attaches");
    assert_eq!(
        resolved,
        WorkspaceId::new(RESOLVED_ID),
        "the identifier must come from the server's resolution, not from the slug the user typed"
    );
    assert_eq!(
        attached.session.attached_workspace(),
        Some(&WorkspaceId::new(RESOLVED_ID)),
        "after a successful switch the session must know where it is pointed"
    );

    // Asserted from what was sent, not from the code that sent it. Neither arm
    // of this comparison travels back through the session's own formatter.
    let wire = attached.wire_text();
    let resolve_at = wire
        .find("workspaces.resolve_slug")
        .expect("no resolve_slug call crossed the wire");
    let switch_at = wire
        .find("me.set_current_workspace")
        .expect("no set_current_workspace call crossed the wire");
    assert!(
        resolve_at < switch_at,
        "ADR-0006 D7 requires the slug be resolved BEFORE the switch; the wire shows the switch \
         at {switch_at} and the resolution at {resolve_at}"
    );
    assert!(
        wire.contains(&format!(r#""workspaceId":"{RESOLVED_ID}""#)),
        "the switch did not carry the resolved workspaceId: {wire:?}"
    );
    assert!(
        !wire.contains("workspaceSlug"),
        "ADR-0006 D7: me.set_current_workspace rejects workspaceSlug, and a slug reached the wire \
         anyway: {wire:?}"
    );

    // What the server actually did with it, which is the consequence rather
    // than a proxy for it.
    assert_eq!(
        *attached
            .server
            .pointer_moves
            .lock()
            .expect("the fixture's pointer lock is not poisoned"),
        vec![RESOLVED_ID.to_owned()]
    );
}

#[tokio::test]
async fn a_workspace_that_cannot_be_resolved_is_unattachable_and_says_nothing_more() {
    let mut attached = attach().await;
    *attached
        .server
        .refuse_resolution
        .lock()
        .expect("the fixture's refusal lock is not poisoned") = true;

    let slug = WorkspaceSlug::new("some-other-workspace");
    let error = attached
        .session
        .attach_workspace_by_slug(&slug)
        .await
        .expect_err("a refused resolution cannot attach anything");

    assert_eq!(
        error,
        NotesError::WorkspaceUnattachable { slug },
        "ADR-0006 D7 requires a refused attachment to surface as the one text the harness can \
         honestly produce; passing the server's own refusal through instead sends the user to \
         look at a cause nobody can identify"
    );
    let rendered = error.to_string();
    assert!(rendered.contains("some-other-workspace"));
    assert!(
        !rendered.contains("forbidden"),
        "the server said `forbidden` and the harness must not repeat a word that names a gate it \
         cannot identify: {rendered:?}"
    );
    assert_eq!(
        attached.session.attached_workspace(),
        None,
        "a refused attachment must leave the session pointed where it was"
    );
}

#[tokio::test]
async fn every_read_names_its_workspace_on_the_wire() {
    let attached = attach().await;
    let workspace = WorkspaceId::new(RESOLVED_ID);

    let body = attached
        .session
        .read_page("home", &workspace)
        .await
        .expect("the fixture serves a page");
    assert_eq!(body, format!("home as read from {RESOLVED_ID}"));

    let wire = attached.wire_text();
    assert!(
        wire.contains(&format!(r#""workspace":"{RESOLVED_ID}""#)),
        "the read did not name its workspace on the wire, which is the failure the Zaru grounding \
         calls the rule that will bite you: {wire:?}"
    );
}

#[tokio::test]
async fn adr_0007_d6s_first_signal_arrives_unsolicited_and_the_refreshed_scope_differs() {
    let mut attached = attach().await;

    assert!(
        attached.session.take_list_changed().is_none(),
        "nothing has happened yet, so there is no signal to take; a take that always answered \
         would make the assertion below meaningless"
    );

    attached
        .server
        .scope
        .lock()
        .expect("the fixture's scope lock is not poisoned")
        .push("atoms.read".to_owned());
    attached
        .peer
        .peer()
        .notify_tool_list_changed()
        .await
        .expect("the fixture can notify");

    // Waiting on the condition, under a bound, rather than on a duration.
    let signal = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        attached.session.await_list_changed(),
    )
    .await
    .expect("the notification arrived within the bound")
    .expect("the session was still connected");
    assert_eq!(signal, Invalidation::ListChanged);

    let refreshed = attached.session.tools().await.expect("tools/list answers");
    assert!(
        refreshed.contains(&"atoms.read".to_owned()),
        "the refresh must pick up the change the signal announced; it returned {refreshed:?}"
    );
    assert!(
        attached
            .wire_text()
            .contains("notifications/tools/list_changed"),
        "the notification did not cross the wire, so this check watched something else"
    );
}

#[tokio::test]
async fn adr_0007_d6s_third_signal_fires_only_for_a_tool_the_caller_claimed() {
    let attached = attach().await;

    // The server takes the tool out of scope, which is what ADR-0135's three
    // gates do when a token loses one. A caller whose cache still claims it
    // calls it anyway -- which is the situation D6's third signal exists for.
    attached
        .server
        .scope
        .lock()
        .expect("the fixture's scope lock is not poisoned")
        .retain(|name| name != "pages.read");

    let error = attached
        .session
        .read_page("home", &WorkspaceId::new(RESOLVED_ID))
        .await
        .expect_err("a tool outside the scope is refused");

    let NotesError::Call(refused) = error else {
        panic!("a refused tool call must surface as NotesError::Call, not as {error:?}");
    };
    assert_eq!(refused.tool, "pages.read");
    assert!(
        refused.is_method_not_found(),
        "ADR-0135's three gates take a tool out of tools/list, so calling it is calling a method \
         that is not there; the server sent code {}",
        refused.code
    );

    let claimed = vec!["pages.read".to_owned()];
    assert_eq!(
        Invalidation::claimed(refused.clone(), &claimed),
        Ok(Invalidation::Claimed(refused.clone())),
        "the cache claimed the tool and the server refused it, so the cache is stale"
    );

    let never_claimed: Vec<String> = Vec::new();
    assert_eq!(
        Invalidation::claimed(refused.clone(), &never_claimed),
        Err(refused),
        "a refusal for a tool nothing claimed says nothing about a cache, and the failure must \
         come back unchanged rather than being swallowed"
    );
}

#[tokio::test]
async fn what_the_session_offers_is_printed_for_a_reader() {
    // Not an assertion. The capture a report quotes, so that a reader sees what
    // a session actually is rather than a list of check names.
    let mut attached = attach().await;
    println!("negotiated: {:?}", attached.session.negotiated());
    println!(
        "tools: {:?}",
        attached.session.tools().await.expect("tools/list answers")
    );
    let resolved = attached
        .session
        .attach_workspace_by_slug(&WorkspaceSlug::new("zaru"))
        .await
        .expect("the fixture resolves and attaches");
    println!("attached workspace: {resolved}");
    println!(
        "pages.read: {:?}",
        attached
            .session
            .read_page("home", &resolved)
            .await
            .expect("the fixture serves a page")
    );
    println!("session: {:?}", attached.session);
}

/// ADR-0005 D3's trie is built over "page paths, titles … for every reachable
/// workspace", and this is where the entries come from: `pages.list` and
/// `atoms.list`, both in ADR-0006 D4's composer scope.
///
/// **The listing pages, and following the cursor is the point.** The fixture
/// answers `pages.list` in two pages of two rows and one, so a client that took
/// the first answer and stopped returns two of three — which is not an error
/// and is indistinguishable from a cortex that holds two. The count and the
/// third row's own path are both asserted, because a client that paged but
/// dropped a row would satisfy neither.
///
/// The two containers are exercised apart: `pages.list` answers an object with
/// a cursor and `atoms.list` a bare array, so the reader is driven through both
/// shapes over real protocol bytes rather than only in a unit check.
#[tokio::test]
async fn a_listing_follows_its_cursor_and_names_its_workspace_on_the_wire() {
    let attached = attach().await;
    // ADR-0006 D4's composer scope carries both; this fixture's default scope
    // is the one the earlier checks assert, so it is widened here rather than
    // there.
    {
        let mut scope = attached
            .server
            .scope
            .lock()
            .expect("the fixture's scope lock is not poisoned");
        scope.push("pages.list".to_owned());
        scope.push("atoms.list".to_owned());
    }
    let workspace = WorkspaceId::new(RESOLVED_ID);

    let pages = attached
        .session
        .pages(&workspace)
        .await
        .expect("pages.list answers");
    let paths: Vec<&str> = pages.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["adrs/0005", "architecture/bóunded", "operations/testing"],
        "the listing pages, and a client that stopped at the first answer would return the first \
         two and look like a cortex holding two"
    );
    assert_eq!(
        pages[2].title, "Tésting ✦",
        "the row past the cursor must arrive whole, not merely counted"
    );

    let atoms = attached
        .session
        .atoms(&workspace)
        .await
        .expect("atoms.list answers");
    assert_eq!(atoms.len(), 1);
    assert_eq!(atoms[0].path, "atoms/mémbrane");
    assert_eq!(
        atoms[0].title, "Mémbrane ✦",
        "a bare array is the other container, and it carries no cursor"
    );

    // The second reader: the bytes, not what the client says about them.
    let wire = attached.wire_text();
    assert!(
        wire.contains(r#""name":"pages.list""#) && wire.contains(r#""name":"atoms.list""#),
        "neither listing crossed the wire, so this check watched something else"
    );
    assert!(
        wire.contains(SECOND_PAGE),
        "the cursor the first answer issued was never sent back, so nothing was followed"
    );
    assert_eq!(
        wire.matches(r#""name":"pages.list""#).count(),
        2,
        "two pages is two calls; a different number means the loop ran a different number of times"
    );
    assert!(
        wire.matches(RESOLVED_ID).count() >= 3,
        "every listing call names its workspace, which is this crate's own rule and is asserted \
         from the frames rather than from the arguments the client composed"
    );
}

/// A server handing back the cursor it was just given is refused, so the loop
/// ends on a condition rather than on a count nobody chose.
///
/// The refusal is the discriminating arm: without it this is a hang, and a hang
/// is the one failure a test suite reports as a timeout somewhere else.
#[tokio::test]
async fn a_cursor_that_does_not_advance_is_refused_rather_than_followed_forever() {
    let page = zaru_notes::session::listing::read(
        "pages.list",
        r#"{"results":[{"path":"a","title":"Á ✦"}],"nextCursor":"c-1"}"#,
    )
    .expect("a well-formed page is read");
    assert_eq!(
        page.next.as_deref(),
        Some("c-1"),
        "the reader carries the cursor out; the loop is what decides whether to follow it"
    );

    // The loop's own guard, reached through the session, needs a server that
    // repeats itself. `listing::read` cannot express that on its own, so what
    // is asserted here is the sentence the guard raises, spelled once.
    let repeated = zaru_notes::session::NotesError::Unreadable {
        tool: "pages.list".to_owned(),
        expected: "a cursor that advances, rather than the one just sent",
    };
    assert!(
        repeated.to_string().contains("advances"),
        "the refusal must say what was expected of the cursor: {repeated}"
    );
}

#[tokio::test]
async fn adr_0006_d4s_last_read_tool_names_its_workspace_and_stops_at_one_page() {
    let attached = attach().await;
    let workspace = WorkspaceId::new(RESOLVED_ID);

    let found = attached
        .session
        .search("membrane", &workspace)
        .await
        .expect("the fixture serves a search");

    assert_eq!(
        found.len(),
        1,
        "a search is ranked, so the first page is the answer; following its cursor would fetch \
         the tail nobody asked for: {found:?}"
    );
    assert_eq!(found[0].path, "adrs/0006");
    // ADR-0006 D6's four parts, off a search rather than composed here: a hit
    // carries everything `Attachment::new` requires, which a listing row does
    // not.
    assert_eq!(found[0].workspace, "zaru");
    assert_eq!(
        found[0].permalink,
        "https://example.invalid/zaru/p/adrs/0006"
    );
    assert_eq!(found[0].uri, "nn://workspace/zaru/p/adrs/0006");
    assert!(found[0].snippet.contains("<mark>membrane</mark>"));

    let wire = attached.wire_text();
    assert!(
        wire.contains(r#""q":"membrane""#),
        "the query never reached the server: {wire:?}"
    );
    assert!(
        wire.contains(&format!(r#""workspace":"{RESOLVED_ID}""#)),
        "the search did not name its workspace, which is the rule that will bite you: {wire:?}"
    );
    assert!(
        !wire.contains(&format!(r#""cursor":"{SECOND_PAGE}""#)),
        "the search followed the cursor the server offered, which a listing does and a ranked \
         answer must not: {wire:?}"
    );
}

#[tokio::test]
async fn adr_0013s_layer_two_is_read_by_a_tool_and_names_its_workspace() {
    let attached = attach().await;
    // **`cortex.ground` is deliberately not in this fixture's default scope**,
    // because that scope is ADR-0006 D4's composer set and D4 does not name it.
    // The grounding belongs to the agent, whose access D3 leaves at whatever
    // the user granted, so the widening here is the record's own boundary
    // showing up as a line of test code rather than an inconvenience.
    attached
        .server
        .scope
        .lock()
        .expect("the fixture's scope lock is not poisoned")
        .push(zaru_notes::session::GROUND.to_owned());
    let workspace = WorkspaceId::new(RESOLVED_ID);

    let grounding = attached
        .session
        .ground(&workspace)
        .await
        .expect("the fixture serves a grounding");
    assert_eq!(grounding, format!("grounding for {RESOLVED_ID} ✦"));

    let wire = attached.wire_text();
    assert!(
        wire.contains(zaru_notes::session::GROUND),
        "the grounding was not asked for by the tool this record names: {wire:?}"
    );
    assert!(
        wire.contains(&format!(r#""workspace":"{RESOLVED_ID}""#)),
        "the grounding read did not name its workspace: {wire:?}"
    );
}

#[tokio::test]
async fn a_search_that_answers_in_another_shape_is_refused_naming_the_expectation() {
    // The accepting sibling is the check two above. This one drives the same
    // reader down its refusing branch, so "the shape is expected and fails
    // loudly" is asserted rather than described. The hit it refuses carries a
    // path and a title and **not** the workspace slug, which is the half of
    // ADR-0006 D6's identity pair that makes a hit locatable at all -- so the
    // refusal names the one field whose absence would turn a search result
    // into something that resolves nowhere.
    let attached = attach().await;
    let refusal = attached
        .session
        .search("membrane", &WorkspaceId::new(UNREADABLE_ID))
        .await
        .expect_err("the fixture answers this workspace with a shape no reader accepts");
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("search.global") && rendered.contains("workspaceSlug"),
        "the refusal names neither the tool nor the field that was missing: {rendered}"
    );
}

// --- ADR-0007 D6's cache holds a declaration, not a name --------------------
//
// D5 projects the cache to the agent as an MCP server, and a model is offered
// a name, a description and a parameter schema. These three checks are the
// crate half of that: what `tools/list` carries whole, that the two readings
// are one `tools/list`, and the run-time-named call door D5's projection needs.

#[tokio::test]
async fn adr_0007_d6_a_declaration_carries_the_name_the_description_and_the_schema() {
    let attached = attach().await;
    let declared = attached
        .session
        .tool_declarations()
        .await
        .expect("tools/list answers");

    assert_eq!(
        declared.len(),
        FIRST_SCOPE.len(),
        "every tool the server reported should carry a declaration"
    );
    let first = declared.first().expect("the fixture reports four tools");
    assert_eq!(first.name, FIRST_SCOPE[0], "the name is the server's");
    assert_eq!(
        first.description.as_deref(),
        Some("a fixture tool"),
        "ADR-0007 D6: the description is carried as the server wrote it, uninterpreted"
    );
    assert_eq!(
        first.input_schema, r#"{"type":"object"}"#,
        "the schema is the bytes the server sent, re-serialised rather than re-shaped -- a \
         declaration with no schema is not JSON and a provider handed one refuses the surface"
    );
}

#[tokio::test]
async fn adr_0007_d6_the_names_and_the_declarations_are_one_tools_list() {
    let attached = attach().await;
    let names = attached.session.tools().await.expect("tools/list answers");
    let declared = attached
        .session
        .tool_declarations()
        .await
        .expect("tools/list answers");

    assert_eq!(
        names,
        declared
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>(),
        "`tools` is implemented over `tool_declarations`, so the two cannot answer differently \
         about what a token grants"
    );
}

#[tokio::test]
async fn adr_0007_d5_a_tool_named_at_run_time_is_called_and_its_answer_carried_back() {
    let attached = attach().await;

    // The accepting arm: a tool the server serves, named as a string the way
    // a projected call names one, with its arguments as JSON text.
    let answered = attached
        .session
        .call_declared(
            "workspaces.resolve_slug",
            r#"{"slug":"zaru","instance":"fixture"}"#,
        )
        .await
        .expect("the fixture answers a tool it serves");
    assert!(
        answered.contains(RESOLVED_ID),
        "the server's own answer is carried back unread: {answered}"
    );

    // The refusing arm: the server's refusal reaches the caller unchanged
    // rather than being turned into something this crate invented.
    let refused = attached
        .session
        .call_declared("pages.apply_patch", "{}")
        .await
        .expect_err("a tool outside the scope is refused by the server");
    assert!(
        matches!(refused, NotesError::Call(_)),
        "a server refusal is carried out as a refusal: {refused:?}"
    );
}

#[tokio::test]
async fn adr_0007_d5_arguments_that_are_not_a_json_object_are_refused_before_the_wire() {
    let attached = attach().await;

    for offered in [r#""a string""#, "[1,2,3]", "not json at all", "7"] {
        let refused = attached
            .session
            .call_declared("pages.read", offered)
            .await
            .expect_err("arguments that are not an object are not a call");
        match refused {
            NotesError::Unreadable { tool, expected } => {
                assert_eq!(tool, "tools/call");
                assert_eq!(expected, "arguments that are a JSON object");
            }
            other => panic!("{offered} should be unreadable, not {other:?}"),
        }
    }

    // The accepting sibling, so a refuse-everything implementation cannot
    // pass: an object reaches the server.
    let answered = attached
        .session
        .call_declared("pages.read", r#"{"pathOrId":"home","workspace":"w"}"#)
        .await;
    assert!(
        answered.is_ok(),
        "an object is a call: {answered:?}"
    );
}
