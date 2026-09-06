// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The session: one attached MCP connection to Nuclear Notes.
//!
//! # A session has no field a bearer value could occupy
//!
//! [`Session::attach`] takes a [`Bearer`], hands it to
//! [`Endpoint::open`], and drops it. **The struct has no
//! bearer field.** That is the strongest form of [ADR-0007] D3's "the token
//! string appears in no prompt, no transcript, no log, and no tool result": not
//! a redaction that could be forgotten, but the same argument [ADR-0014] D4
//! makes about configuration — "the design decision that prevents it is
//! refusing to have a field to put one in" — and the same one `zaru-cli`'s
//! stored record makes about the credential file.
//!
//! What that costs is real and is recorded rather than hidden: [ADR-0005] D8
//! wants an evicted session re-initialised transparently, and re-initialising
//! needs the bearer again. Whoever builds re-attachment supplies it again from
//! the store, or gives this type a reason to hold one. Nothing here pretends to
//! do it.
//!
//! # Every call names its workspace
//!
//! See the module documentation on [`super`]. [`Session::read_page`] takes a
//! [`WorkspaceId`] as an ordinary argument, and there is no overload without
//! one.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::session::address::{Instance, WorkspaceId, WorkspaceSlug};
use crate::session::bearer::Bearer;
use crate::session::endpoint::Endpoint;
use crate::session::error::{CallRefused, NotesError, TOOL_ERROR};
use crate::session::found::{self, Found};
use crate::session::invalidation::Invalidation;
use crate::session::listing::{self, Listed};
use core::fmt;
use rmcp::ClientHandler;
use rmcp::model::{CallToolRequestParams, ContentBlock, JsonObject};
use rmcp::serve_client;
use rmcp::service::{
    ClientInitializeError, NotificationContext, RoleClient, RunningService, ServiceError,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// The tool that turns a slug into an identifier, per [ADR-0006] D7.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub const RESOLVE_SLUG: &str = "workspaces.resolve_slug";

/// The tool that moves this session's workspace pointer.
///
/// Spelled as the substrate spells it. What this crate *calls* the thing it
/// moves is the attached workspace — see [`super`].
pub const SET_CURRENT_WORKSPACE: &str = "me.set_current_workspace";

/// The tool that reads a page.
pub const READ_PAGE: &str = "pages.read";

/// The tool that lists a workspace's pages.
///
/// In [ADR-0006] D4's `read_only_memory` set, spelled as that record spells it.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub const LIST_PAGES: &str = "pages.list";

/// The tool that lists a workspace's atoms.
///
/// In [ADR-0006] D4's `read_only_memory` set, spelled as that record spells it.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub const LIST_ATOMS: &str = "atoms.list";

/// The tool that searches a workspace.
///
/// In [ADR-0006] D4's `read_only_memory` set, spelled as that record spells it,
/// and the last of that set's six with a caller here.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub const SEARCH_GLOBAL: &str = "search.global";

/// The tool that answers with an instance's and a workspace's grounding.
///
/// **Not in [ADR-0006] D4's composer set, and that is the point.** D4 scopes
/// the composer's credential to `read_only_memory` plus
/// `me.set_current_workspace`, and this is neither; what wants a grounding is
/// the agent, whose access D3 leaves at "whatever scope the user granted".
/// [ADR-0013] D1 puts it at layer 2 of the context, "Grounding, session-start",
/// never discarded silently.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub const GROUND: &str = "cortex.ground";

/// What a session negotiated when it attached.
///
/// Deliberately this crate's own type rather than `rmcp`'s `InitializeResult`:
/// `zaru-cli` consumes this and has no reason to gain a dependency on the MCP
/// SDK's model in order to read a protocol version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    /// The protocol version both ends agreed on.
    pub protocol_version: String,
    /// What the server calls itself.
    pub server_name: String,
    /// The server's version.
    pub server_version: String,
    /// Whether the server declared a tools capability at all.
    pub serves_tools: bool,
}

/// Receives the notifications a server sends unsolicited.
///
/// Only one of them matters here: [ADR-0007] D6 invalidates a cached tool scope
/// on `notifications/tools/list_changed`, which the server delivers without
/// being asked.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Clone)]
struct Watcher {
    list_changed: mpsc::UnboundedSender<()>,
}

impl ClientHandler for Watcher {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        // A closed receiver means the session was dropped, which is not a
        // failure of anything: the notification has nowhere to go and nothing
        // is waiting for it.
        let _ = self.list_changed.send(());
    }
}

/// One attached connection to Nuclear Notes.
pub struct Session {
    service: RunningService<RoleClient, Watcher>,
    instance: Instance,
    negotiated: Negotiated,
    attached: Option<WorkspaceId>,
    list_changed: mpsc::UnboundedReceiver<()>,
}

impl fmt::Debug for Session {
    /// Written by hand, and the reason is not redaction.
    ///
    /// There is nothing here to redact — see the module note. It is by hand
    /// because `rmcp`'s `RunningService` is not `Debug`, and a derived `Debug`
    /// would therefore not compile at all.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("instance", &self.instance)
            .field("negotiated", &self.negotiated)
            .field("attached", &self.attached)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Open a transport through `endpoint` and initialise a session over it.
    ///
    /// `bearer` is handed to the endpoint and dropped. This function is the
    /// only place in this crate that holds one.
    ///
    /// # Errors
    ///
    /// [`NotesError::Endpoint`] when the port refuses, and
    /// [`NotesError::Attach`] when the MCP handshake does not complete.
    pub async fn attach<E>(
        endpoint: &E,
        instance: Instance,
        bearer: Bearer,
    ) -> Result<Self, NotesError>
    where
        E: Endpoint,
    {
        let transport =
            endpoint
                .open(&instance, &bearer)
                .await
                .map_err(|failure| NotesError::Endpoint {
                    detail: failure.detail,
                })?;
        drop(bearer);

        let (tx, list_changed) = mpsc::unbounded_channel();
        let service = serve_client(Watcher { list_changed: tx }, transport)
            .await
            .map_err(|error| NotesError::Attach {
                detail: attach_detail(&error),
            })?;

        let peer = service.peer_info().ok_or_else(|| NotesError::Attach {
            detail: "the handshake completed without the server's own information".to_owned(),
        })?;
        let negotiated = Negotiated {
            protocol_version: peer.protocol_version.as_str().to_owned(),
            server_name: peer
                .server_info
                .as_ref()
                .map_or_else(String::new, |info| info.name.clone()),
            server_version: peer
                .server_info
                .as_ref()
                .map_or_else(String::new, |info| info.version.clone()),
            serves_tools: peer.capabilities.tools.is_some(),
        };

        Ok(Self {
            service,
            instance,
            negotiated,
            attached: None,
            list_changed,
        })
    }

    /// The instance this session authenticated against.
    #[must_use]
    pub const fn instance(&self) -> &Instance {
        &self.instance
    }

    /// What the handshake agreed on, read off the response rather than assumed.
    #[must_use]
    pub const fn negotiated(&self) -> &Negotiated {
        &self.negotiated
    }

    /// The workspace this session is pointed at, as far as this session knows.
    ///
    /// `None` until something moves it. [ADR-0007] D2 calls the stored copy of
    /// this "informational only — the token row is authoritative", and the same
    /// caution applies here: another session on the same token can move the
    /// pointer, which is the whole reason [`Self::read_page`] names its
    /// workspace rather than trusting this.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    #[must_use]
    pub const fn attached_workspace(&self) -> Option<&WorkspaceId> {
        self.attached.as_ref()
    }

    /// The tool names this token grants, in the order the server reported them.
    ///
    /// [ADR-0007] D6: "The three-gate enforcement in ADR-0135 means that
    /// response already reflects exactly what the token grants, so the cache
    /// needs no interpretation." Nothing here interprets it.
    ///
    /// # Errors
    ///
    /// [`NotesError::Transport`] when the call cannot be made.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub async fn tools(&self) -> Result<Vec<String>, NotesError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|error| service_failure("tools/list", &error))?;
        Ok(tools
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect())
    }

    /// Resolve a slug to an identifier, per [ADR-0006] D7.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses, and
    /// [`NotesError::Unreadable`] when the answer carries no `id`.
    ///
    /// # The response shape is unmeasured
    ///
    /// No token exists in this arc, so what the live tool puts in its result
    /// has not been read. This expects a JSON object carrying a string `id` and
    /// fails loudly naming that expectation when it does not find one, rather
    /// than guessing a second shape — a client that silently accepted two
    /// shapes would hide the day the guess was wrong.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub async fn resolve_slug(&self, slug: &WorkspaceSlug) -> Result<WorkspaceId, NotesError> {
        let mut arguments = JsonObject::new();
        arguments.insert("slug".to_owned(), Value::String(slug.as_str().to_owned()));
        arguments.insert(
            "instance".to_owned(),
            Value::String(self.instance.as_str().to_owned()),
        );

        let answer = self.call(RESOLVE_SLUG, arguments).await?;
        let parsed: Value = serde_json::from_str(&answer).map_err(|_| NotesError::Unreadable {
            tool: RESOLVE_SLUG.to_owned(),
            expected: "a JSON object",
        })?;
        parsed
            .get("id")
            .and_then(Value::as_str)
            .map(WorkspaceId::new)
            .ok_or(NotesError::Unreadable {
                tool: RESOLVE_SLUG.to_owned(),
                expected: "a JSON object carrying a string `id`",
            })
    }

    /// Move this session's pointer, **by identifier only**.
    ///
    /// There is no counterpart taking a [`WorkspaceSlug`]. [ADR-0006] D7:
    /// `me.set_current_workspace` "rejects `workspaceSlug`, because slugs are
    /// unique per instance rather than globally. Apex callers must pass
    /// `workspaceId`." A caller holding a slug goes through
    /// [`Self::attach_workspace_by_slug`], which resolves first.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub async fn attach_workspace(&mut self, workspace: &WorkspaceId) -> Result<(), NotesError> {
        let mut arguments = JsonObject::new();
        arguments.insert(
            "workspaceId".to_owned(),
            Value::String(workspace.as_str().to_owned()),
        );
        self.call(SET_CURRENT_WORKSPACE, arguments).await?;
        self.attached = Some(workspace.clone());
        Ok(())
    }

    /// [ADR-0006] D7 end to end: resolve the slug, then switch by identifier.
    ///
    /// # Errors
    ///
    /// [`NotesError::WorkspaceUnattachable`] when either call is refused or
    /// answers unreadably, carrying **no cause**. D7: the server throws
    /// `forbidden` "without revealing which gate tripped — existence,
    /// membership, or the token's `scope.workspaceIds` filter — so the harness
    /// reports 'cannot attach that workspace' rather than inventing a more
    /// specific reason it cannot actually distinguish."
    ///
    /// A transport failure is **not** folded into that: it is not a gate, and
    /// reporting a dropped connection as an inaccessible workspace would send
    /// the user to look at their permissions.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub async fn attach_workspace_by_slug(
        &mut self,
        slug: &WorkspaceSlug,
    ) -> Result<WorkspaceId, NotesError> {
        let resolved = match self.resolve_slug(slug).await {
            Ok(id) => id,
            Err(error) => return Err(unattachable(slug, error)),
        };
        match self.attach_workspace(&resolved).await {
            Ok(()) => Ok(resolved),
            Err(error) => Err(unattachable(slug, error)),
        }
    }

    /// Read a page, naming the workspace it lives in.
    ///
    /// The workspace is a required argument. See [`super`] for why.
    ///
    /// The answer is returned exactly as the tool gave it, because this crate
    /// does not model a page and a client that paraphrased one would be a
    /// second, worse source of truth for its content.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses.
    pub async fn read_page(
        &self,
        path: &str,
        workspace: &WorkspaceId,
    ) -> Result<String, NotesError> {
        let mut arguments = JsonObject::new();
        arguments.insert("pathOrId".to_owned(), Value::String(path.to_owned()));
        arguments.insert(
            "workspace".to_owned(),
            Value::String(workspace.as_str().to_owned()),
        );
        self.call(READ_PAGE, arguments).await
    }

    /// Every page in a workspace, for [ADR-0005] D3's trie.
    ///
    /// D3 has the trie "built at session start and refreshed on write", over
    /// "page paths, titles … for every reachable workspace", so a caller builds
    /// one of these per workspace it can reach and folds the answers together.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses, and
    /// [`NotesError::Unreadable`] when the answer does not carry a listing.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    pub async fn pages(&self, workspace: &WorkspaceId) -> Result<Vec<Listed>, NotesError> {
        self.listing(LIST_PAGES, workspace).await
    }

    /// Every atom in a workspace, for [ADR-0005] D3's trie.
    ///
    /// # Errors
    ///
    /// As [`Self::pages`].
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    pub async fn atoms(&self, workspace: &WorkspaceId) -> Result<Vec<Listed>, NotesError> {
        self.listing(LIST_ATOMS, workspace).await
    }

    /// Search a workspace, for [ADR-0005] D3's server tier.
    ///
    /// # Why this does not follow its cursor and a listing does
    ///
    /// [`Self::pages`] follows its cursor because a listing that stopped early
    /// would under-populate the trie, and a trie missing entries is
    /// indistinguishable from a cortex that does not hold them. A search is the
    /// opposite: its answer is **ranked**, so the first page is the part that
    /// matters and following the cursor would fetch the tail nobody asked for
    /// — on the composer's hot path, against a rate-limit bucket
    /// [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces)
    /// counts.
    ///
    /// # The answer's shape was measured on 2026-09-06 and was not the guess
    ///
    /// It is a JSON object carrying an array **`hits`**, not the `results` a
    /// listing carries, and every row locates itself. See
    /// [`found`](super::found), which exists because the first live call
    /// refused rather than quietly returning nothing.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses, and
    /// [`NotesError::Unreadable`] when the answer does not carry `hits`.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    pub async fn search(
        &self,
        query: &str,
        workspace: &WorkspaceId,
    ) -> Result<Vec<Found>, NotesError> {
        let mut arguments = JsonObject::new();
        arguments.insert("q".to_owned(), Value::String(query.to_owned()));
        arguments.insert(
            "workspace".to_owned(),
            Value::String(workspace.as_str().to_owned()),
        );
        let answer = self.call(SEARCH_GLOBAL, arguments).await?;
        found::read(SEARCH_GLOBAL, &answer)
    }

    /// The grounding [ADR-0013] D1 puts at layer 2, for the workspace named.
    ///
    /// The answer is returned exactly as the tool gave it, for the reason
    /// [`Self::read_page`] gives: this crate does not model a grounding
    /// document and a client that paraphrased one would be a second, worse
    /// source of truth for it.
    ///
    /// # The handshake already carries this, and `rmcp` throws it away
    ///
    /// Measured against Nuclear Notes on 2026-09-05: the `initialize` result
    /// carries a fifth top-level key, `_grounding`, holding the instance
    /// grounding and the attached workspace's — 11,050 and 14,890 bytes on that
    /// connection — which is [ADR-0156]'s auto-ground arriving free on the
    /// handshake. **It cannot be read through this SDK.**
    /// `rmcp::model::InitializeResult` declares five fields and no catch-all,
    /// and `serde` drops an unknown key silently, so the bytes reach the
    /// process and are discarded before any code here could see them.
    ///
    /// That is why this is a tool call rather than a field on
    /// [`Negotiated`]: a second round trip for something the first already
    /// carried, and the alternative is a change to the SDK. Recorded rather
    /// than worked around.
    ///
    /// # Errors
    ///
    /// [`NotesError::Call`] when the server refuses.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    /// [ADR-0156]: https://cortex.page/adrs/p/0156-workspace-grounding-metadata-schema-and-auto-ground
    pub async fn ground(&self, workspace: &WorkspaceId) -> Result<String, NotesError> {
        let mut arguments = JsonObject::new();
        arguments.insert(
            "workspace".to_owned(),
            Value::String(workspace.as_str().to_owned()),
        );
        self.call(GROUND, arguments).await
    }

    /// Every page of one listing, following the cursor until it stops.
    ///
    /// # Why the loop ends on a condition and not on a count
    ///
    /// A listing that stopped at its first page would silently under-populate
    /// the trie, and a trie missing entries is indistinguishable from a cortex
    /// that does not hold them. So the cursor is followed. What bounds the loop
    /// is **the cursor advancing**: a server handing back the cursor it was
    /// given is refused naming that, which terminates without a retry count
    /// nobody chose — a count would be a number this record does not carry, and
    /// the condition is the thing actually going wrong.
    async fn listing(
        &self,
        tool: &str,
        workspace: &WorkspaceId,
    ) -> Result<Vec<Listed>, NotesError> {
        let mut collected = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut arguments = JsonObject::new();
            arguments.insert(
                "workspace".to_owned(),
                Value::String(workspace.as_str().to_owned()),
            );
            if let Some(from) = &cursor {
                arguments.insert("cursor".to_owned(), Value::String(from.clone()));
            }

            let answer = self.call(tool, arguments).await?;
            let page = listing::read(tool, &answer)?;
            collected.extend(page.listed);

            match page.next {
                None => return Ok(collected),
                Some(next) if Some(&next) == cursor.as_ref() => {
                    return Err(NotesError::Unreadable {
                        tool: tool.to_owned(),
                        expected: "a cursor that advances, rather than the one just sent",
                    });
                }
                Some(next) => cursor = Some(next),
            }
        }
    }

    /// [ADR-0007] D6's first signal, if one has arrived and not been taken.
    ///
    /// Non-blocking. Every notification is delivered once: a caller that takes
    /// one has consumed it, so two callers cannot both refresh on the same
    /// signal.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub fn take_list_changed(&mut self) -> Option<Invalidation> {
        self.list_changed
            .try_recv()
            .ok()
            .map(|()| Invalidation::ListChanged)
    }

    /// Wait for [ADR-0007] D6's first signal.
    ///
    /// Returns `None` when the session's connection is gone and no further
    /// notification can arrive. A caller that wants a bound puts one around
    /// this; waiting on the condition is what this offers, and waiting on a
    /// count is what it deliberately does not.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub async fn await_list_changed(&mut self) -> Option<Invalidation> {
        self.list_changed
            .recv()
            .await
            .map(|()| Invalidation::ListChanged)
    }

    /// One tool call, with the server's own refusal carried out unchanged.
    async fn call(&self, tool: &str, arguments: JsonObject) -> Result<String, NotesError> {
        let result = self
            .service
            .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
            .await
            .map_err(|error| service_failure(tool, &error))?;

        // A tool error arrives as a *successful* JSON-RPC response carrying
        // `isError`, not as an error response, so a client that only looked at
        // the transport's verdict would read a refusal as an answer.
        if result.is_error == Some(true) {
            return Err(NotesError::Call(CallRefused {
                tool: tool.to_owned(),
                code: TOOL_ERROR,
                detail: first_text(&result.content).unwrap_or_default(),
            }));
        }

        first_text(&result.content).ok_or(NotesError::Unreadable {
            tool: tool.to_owned(),
            expected: "at least one text content block",
        })
    }
}

/// The first text block of a tool result, if there is one.
fn first_text(content: &[ContentBlock]) -> Option<String> {
    content.iter().find_map(|block| match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    })
}

/// Turn `rmcp`'s failure into this crate's, keeping the server's own wording.
fn service_failure(tool: &str, error: &ServiceError) -> NotesError {
    match error {
        ServiceError::McpError(mcp) => NotesError::Call(CallRefused {
            tool: tool.to_owned(),
            code: mcp.code.0,
            detail: mcp.message.to_string(),
        }),
        other => NotesError::Transport {
            detail: innermost(other),
        },
    }
}

/// What a failed handshake says, with the SDK's type parameters left out.
///
/// [`ClientInitializeError::TransportError`] renders as
/// `"Send message error {error}, when {context}"`, and `{error}` is a
/// `DynamicTransportError` whose own `Display` prints the transport's Rust
/// type. **Its inner error is `#[source]`, but the enum variant above it does
/// not mark the field, so the chain is broken at exactly the link
/// [`innermost`] would need** — measured by running the binary and reading what
/// a real refusal printed. So this reaches through that one variant by name and
/// leaves every other to the chain.
fn attach_detail(error: &ClientInitializeError) -> String {
    match error {
        ClientInitializeError::TransportError { error, .. } => innermost(error.error.as_ref()),
        other => innermost(other),
    }
}

/// The last sentence in an error's `source` chain.
///
/// # Why the innermost and not the outermost
///
/// **Found by running the binary rather than by reading it.** `rmcp`'s
/// outermost `Display` for a transport failure interpolates the transport's
/// own Rust type, so a real refusal reached a user as:
///
/// ```text
/// Send message error Transport [rmcp::transport::worker::WorkerTransport<
/// rmcp::transport::streamable_http_client::StreamableHttpClientWorker<
/// zaru_notes::session::transport::http::ReqwestHttp>>] error: Auth required,
/// when send initialize request
/// ```
///
/// The five words that matter are `Auth required`. [ADR-0016] D2 says an error
/// message whose reader cannot act "is a stack trace with better grammar", and
/// a sentence naming three generic parameters of this crate's own private
/// module is exactly that.
///
/// **This chooses between sentences the SDK already wrote; it does not
/// paraphrase one.** Walking to the end of the chain is what the standard
/// library's `source` is for, and the innermost link is the thing that actually
/// went wrong — `Auth required`, or
/// `error sending request for url (https://…)`. Rewriting either would be this
/// crate inventing a diagnosis, which is what
/// [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces)
/// D7 forbids about a workspace refusal and the same argument forbids here.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn innermost(error: &(dyn std::error::Error + 'static)) -> String {
    let mut deepest = error;
    while let Some(next) = deepest.source() {
        deepest = next;
    }
    deepest.to_string()
}

/// Fold a gate failure into D7's single indistinguishable refusal, and let
/// anything that is not a gate through unchanged.
fn unattachable(slug: &WorkspaceSlug, error: NotesError) -> NotesError {
    match error {
        NotesError::Call(_) | NotesError::Unreadable { .. } => {
            NotesError::WorkspaceUnattachable { slug: slug.clone() }
        }
        passthrough => passthrough,
    }
}
