// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a session's bytes go, as a port with no implementation here.
//!
//! # Why this is a port, and what now implements it
//!
//! [ADR-0103] mounts Nuclear Notes' MCP surface over streamable HTTP, and this
//! port is what a session opens that surface through.
//!
//! **It had no implementation until 2026-09-05, and the paragraph that stood
//! here explained the absence with a number that is no longer the number.** It
//! said the transport would cost "113 further packages … `reqwest`, `hyper`,
//! `rustls`, `aws-lc-sys` under `cmake`, and the rest", measured 2026-09-04
//! against a 53-package lock with no `reqwest` in the tree at all. `reqwest`
//! landed for the Gemini client the following day, so the measurement stopped
//! describing this workspace: taken as [`transport`](super::transport) takes
//! it, the cost is **three packages**, 314 to 317, with no `aws-lc-sys` and no
//! `cmake`. That module carries the three shapes and their numbers.
//!
//! It also said that because there is no transport here, "**nothing here can
//! reach a network**, and that is a property of what exists rather than a claim
//! about a code path". That is now false as written and its argument survives
//! one level down: a [`Session`](super::Session) reaches a network only through
//! the endpoint it is **handed**, so a caller that hands it
//! `crate::session::fixtures`' in-process server over `tokio::io::duplex` has
//! a session with no socket to open. Every check in this crate is such a
//! caller, which is still a property of what a value holds rather than a claim
//! about a code path.
//!
//! The port stays a port. [`super::transport::HttpEndpoint`] is one
//! implementation and a check's fixture is another, which is what lets the
//! session be driven end to end with no server anywhere.
//!
//! # This is also the only thing a [`Bearer`] is handed to
//!
//! [`Endpoint::open`] is the one product call site of
//! [`Bearer::expose_for_dispatch`](super::Bearer::expose_for_dispatch) in this
//! crate. A session holds a bearer and gives it to nothing else, which is what
//! makes [ADR-0007] D3's "the token string appears in no prompt, no transcript,
//! no log, and no tool result" checkable by looking at one function rather than
//! at a program.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0103]: https://cortex.page/adrs/p/0103-mcp-server-transport-mount
//! [`Bearer`]: super::Bearer

use crate::session::address::Instance;
use crate::session::bearer::Bearer;
use core::fmt;
use core::future::Future;
use rmcp::service::RoleClient;
use rmcp::transport::IntoTransport;

/// An endpoint could not produce a transport.
///
/// Carries a detail string the implementation writes. **An implementation must
/// not put a bearer value in it** — the whole point of the type it is handed is
/// that the value does not travel — and this crate's checks assert that no
/// refusal it can raise carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl EndpointFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for EndpointFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for EndpointFailure {}

/// Opens the byte channel a session runs over.
///
/// The associated types are `rmcp`'s own `IntoTransport` parameters rather than
/// an abstraction invented here, so that any transport that crate offers — and
/// any a test writes — satisfies this port without an adapter.
pub trait Endpoint {
    /// The transport [`Endpoint::open`] produces.
    type Transport: IntoTransport<RoleClient, Self::TransportError, Self::Adapter>;
    /// How that transport reports failure once it is running.
    type TransportError: std::error::Error + Send + Sync + 'static;
    /// `rmcp`'s marker selecting which `IntoTransport` impl applies.
    type Adapter;

    /// Open a channel to `instance`, authenticated by `bearer`.
    ///
    /// # Errors
    ///
    /// [`EndpointFailure`] carrying the implementation's own wording, and never
    /// the bearer value.
    fn open(
        &self,
        instance: &Instance,
        bearer: &Bearer,
    ) -> impl Future<Output = Result<Self::Transport, EndpointFailure>> + Send;
}
