// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The product transport: [`Endpoint`] over streamable HTTP.
//!
//! # This is the half this crate did not have
//!
//! [`Endpoint`] was declared on 2026-09-04 with no
//! implementation anywhere, because no token existed that an arc could open a
//! session with. One was issued on 2026-09-05, and this module is what it is
//! for.
//!
//! # The three shapes that were measured, and why this one
//!
//! Measured 2026-09-05 in a scratch copy against `main` at `8179f8a`, whose
//! `Cargo.lock` is 314 packages and whose `zaru-notes` and `zaru-cli` closures
//! are 42 and 242 (`cargo tree -p <crate> -e normal --prefix none`,
//! deduplicated).
//!
//! **`rmcp`'s `transport-streamable-http-client-reqwest` alone: +4 packages,
//! and it cannot connect.** That feature is `transport-streamable-http-client`
//! plus `__reqwest`, and `__reqwest` is `dep:reqwest` with **no TLS feature at
//! all** — `rmcp` declares `reqwest = "0.13.2", default-features = false,
//! features = ["json", "stream"]`. Nothing over `https` would open. It also
//! puts `reqwest` 0.13.4 beside this workspace's 0.12.28, which
//! `cargo tree -e normal -d` prints as two `reqwest`: the duplication
//! [ADR-0003] D2's own comments pin `ratatui` to 0.29 and `aes-gcm` to 0.11 to
//! avoid.
//!
//! **The same, plus `rmcp`'s `reqwest` feature, which is what makes it
//! connect: +29 packages.** `reqwest?/rustls` on 0.13 is
//! `__rustls-aws-lc-rs` plus `rustls-platform-verifier`, so the lock goes 314
//! to 343 and gains `aws-lc-sys` under `cmake`, `aws-lc-rs`, `jni` and its
//! three siblings, `rustls-platform-verifier` and its Android half,
//! `rustls-native-certs`, `schannel` and `openssl-probe` — the chain
//! `[workspace.dependencies]`' `rmcp` comment already refuses by name, and the
//! chain [ADR-0003] D7's "a default install that is `zaru` alone" argument
//! rejects.
//!
//! **Bumping the workspace's `reqwest` row to 0.13 so the SDK's client can be
//! reused: does not resolve.** `cargo metadata` says *"package `zaru-cli`
//! depends on `reqwest` with feature `rustls-tls` but `reqwest` does not have
//! that feature"* — 0.13 renamed it to `rustls`, and 0.13's `rustls` is the
//! aws-lc chain above. It would also be a semver-major bump of a D2-named crate
//! and edits to two other arcs' landed code.
//!
//! **What is taken: the generic transport, with the client written here. +3
//! packages** — `base64 0.23.1` and `sse-stream 0.2.6` from `rmcp`'s
//! `client-side-sse`, and `wasm-streams 0.4.2`, a `wasm32` target dependency of
//! `reqwest`'s `stream` feature that no build on this platform compiles. Lock
//! 314 to 317; `zaru-notes`' closure 42 to 107 and `zaru-cli`'s 242 to 244,
//! because `reqwest`, `hyper` and `rustls` were already in the tree for a
//! different reason and only enter *this crate's* closure now.
//! `cargo tree -e normal -d` carries no `reqwest` line: one `reqwest` in the
//! tree.
//!
//! **Both features belong to rows [ADR-0003] D2 already names**, which is that
//! record's clause 7 caller-arrives case and the shape D2 blessed for `rmcp`'s
//! transport and for `ratatui`'s `crossterm`. `futures` and `sse-stream` are a
//! narrower reading and are recorded as a proposed amendment rather than made
//! silently — see `[workspace.dependencies]`.
//!
//! # What this changes about two sentences this crate used to carry
//!
//! `crate`'s own documentation said "**What is not here is a transport**" and
//! [`endpoint`](crate::session::endpoint)'s said that because there is none,
//! "**nothing here can reach a network**, and that is a property of what exists
//! rather than a claim about a code path". Both are now false and both are
//! corrected where they stand rather than left. What replaces the second is
//! narrower and still structural: a [`Session`](crate::session::Session)
//! reaches a network **only** through an [`Endpoint`]
//! it was handed, so a caller that hands it one built by
//! `crate::session::fixtures` over `tokio::io::duplex` has a session that
//! cannot open a socket, and every check in this crate is such a caller.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing

pub mod bounded;
pub mod http;
pub mod request;

use crate::session::address::Instance;
use crate::session::bearer::Bearer;
use crate::session::endpoint::{Endpoint, EndpointFailure};
use core::future::Future;
use http::ReqwestHttp;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransportConfig, StreamableHttpError,
};

pub use bounded::{Budget, EventTooLarge};
pub use http::ReqwestHttp as HttpClient;
pub use request::Outcome;

/// Where Nuclear Notes mounts its MCP surface on an instance.
///
/// [ADR-0103] mounts one endpoint per instance at this path, and it is a
/// constant rather than a parameter because an [`Instance`] is a host: the
/// product decides where on its own host it serves, and a harness that let a
/// caller choose would be inviting a bearer to be sent somewhere the product
/// does not serve. Measured against `cortex.page` on 2026-09-05.
///
/// [ADR-0103]: https://cortex.page/adrs/p/0103-mcp-server-transport-mount
pub const MCP_PATH: &str = "/api/mcp";

/// The scheme every instance is reached over.
///
/// **Not a parameter, and not derived from anything a caller supplies.** The
/// value this transport carries is a bearer token, and a scheme that could be
/// `http` is a scheme that can put it on the wire in the clear. There is no
/// constructor that takes one and no configuration key that sets one, which is
/// the same shape [ADR-0014] D4 uses for a secret: "the design decision that
/// prevents it is refusing to have a field to put one in".
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const SCHEME: &str = "https://";

/// Opens a streamable HTTP session against a Nuclear Notes instance.
///
/// # The bearer is handed on and never held
///
/// [`Self::open`] formats the credential into an `Authorization` value, puts it
/// in the transport's configuration, and returns. **This struct has no field a
/// bearer could occupy**, exactly as [`Session`](crate::session::Session) has
/// none, so the value's lifetime is the call rather than the program's.
#[derive(Debug, Clone)]
pub struct HttpEndpoint {
    client: ReqwestHttp,
}

impl HttpEndpoint {
    /// Build the endpoint, and with it the HTTP client it will use.
    ///
    /// One client for the endpoint's whole life rather than one per session:
    /// a `reqwest::Client` owns a connection pool and a TLS configuration, and
    /// building one per attach would re-do the TLS handshake every time.
    ///
    /// # Errors
    ///
    /// [`EndpointFailure`] carrying `reqwest`'s own sentence when the TLS
    /// backend cannot be initialised.
    pub fn new() -> Result<Self, EndpointFailure> {
        ReqwestHttp::new()
            .map(|client| Self { client })
            .map_err(|failure| {
                EndpointFailure::new(format!("could not build an HTTP client: {failure}"))
            })
    }

    /// The URL an instance's MCP surface is at.
    ///
    /// Separate from [`Self::open`] so that a check can assert what is
    /// addressed without opening anything.
    #[must_use]
    pub fn url(instance: &Instance) -> String {
        format!("{SCHEME}{}{MCP_PATH}", instance.as_str())
    }

    /// The `Authorization` value a bearer becomes.
    ///
    /// **This is the one place in this crate that formats a credential**, and
    /// it is a free function taking `&Bearer` rather than a method on `Bearer`
    /// so that the search for
    /// [`Bearer::expose_for_dispatch`](crate::session::Bearer::expose_for_dispatch)
    /// still finds every use in one place.
    fn authorization(bearer: &Bearer) -> String {
        format!("Bearer {}", bearer.expose_for_dispatch())
    }
}

impl Endpoint for HttpEndpoint {
    type Transport = StreamableHttpClientTransport<ReqwestHttp>;
    type TransportError = StreamableHttpError<reqwest::Error>;
    type Adapter = rmcp::transport::TransportAdapterIdentity;

    fn open(
        &self,
        instance: &Instance,
        bearer: &Bearer,
    ) -> impl Future<Output = Result<Self::Transport, EndpointFailure>> + Send {
        let config = StreamableHttpClientTransportConfig::with_uri(Self::url(instance))
            .auth_header(Self::authorization(bearer));
        let client = self.client.clone();
        // Building the transport spawns the worker and opens nothing; the
        // handshake is the session's, on the first message. There is therefore
        // no failure to report here, and an `async` block that returns `Ok` is
        // what satisfies a port whose other implementations can fail.
        async move { Ok(StreamableHttpClientTransport::with_client(client, config)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::fixtures::bearer_nonce;

    #[test]
    fn an_instance_is_addressed_over_tls_at_the_mounted_path() {
        let url = HttpEndpoint::url(&Instance::new("cortex.page"));
        assert_eq!(url, "https://cortex.page/api/mcp");
    }

    #[test]
    fn no_instance_is_addressed_in_the_clear() {
        // The mutant this is written against is one character: `https://`
        // becoming `http://` puts the bearer on the wire unencrypted, and every
        // other assertion in this crate would still pass.
        for host in ["cortex.page", "play.cortex.page", "localhost"] {
            let url = HttpEndpoint::url(&Instance::new(host));
            assert!(
                url.starts_with("https://"),
                "{host} would be reached in the clear: {url}"
            );
        }
    }

    #[test]
    fn the_authorization_value_is_the_bearer_scheme_and_the_value() {
        let planted = bearer_nonce();
        let header = HttpEndpoint::authorization(&Bearer::new(planted.clone()));
        assert_eq!(header, format!("Bearer {planted}"));
    }

    #[test]
    fn the_endpoints_own_rendering_carries_no_bearer() {
        let planted = bearer_nonce();
        let endpoint = HttpEndpoint::new().expect("a TLS backend");
        // The value is handed to `open` and to nothing that survives it, so
        // the endpoint's own `Debug` cannot carry it however often it is used.
        let _ = HttpEndpoint::authorization(&Bearer::new(planted.clone()));
        crate::session::fixtures::assert_absent(
            "the endpoint's Debug",
            &format!("{endpoint:?}"),
            &planted,
        );
    }
}
