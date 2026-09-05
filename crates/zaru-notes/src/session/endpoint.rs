// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a session's bytes go, as a port with no implementation here.
//!
//! # Why this is a port and not code
//!
//! [ADR-0103] mounts Nuclear Notes' MCP surface over streamable HTTP, and
//! `rmcp` implements a client for it behind
//! `transport-streamable-http-client-reqwest`. Taking that feature needs no
//! amendment to [ADR-0003] D2 — `reqwest` is already a row in that table and
//! the transport reaches it through `rmcp`'s own optional dependency rather
//! than through a new one.
//!
//! It is not taken here for a different reason. **No check in this crate may
//! open a socket and no credential exists in this arc**, so every one of the
//! 113 further packages that feature resolves — `reqwest`, `hyper`, `rustls`,
//! `aws-lc-sys` under `cmake`, and the rest — would arrive unexercised.
//! [ADR-0003]'s own trigger clause 7 wants each dependency present "with a
//! caller that uses it", and a transport nothing constructs is not that. So
//! the shape that is honest is a declared seam with no implementation, exactly
//! as `zaru-core` declares ports it does not implement and `zaru-cli`
//! declares sealing as another.
//!
//! What that buys is not merely deferral. Because there is no transport in this
//! crate's product tree, **nothing here can reach a network**, and that is a
//! property of what exists rather than a claim about a code path.
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
