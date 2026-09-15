// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The product implementation of [ADR-0007] D5's projected servers: one
//! session per token, opened on first use.
//!
//! # Lazy, because D6 already paid for the declaration
//!
//! D6 caches `tools/list` "once per token at attach", and the declaration the
//! model is offered is built from that cache. So nothing needs a socket until
//! the model actually calls something — a session with three projected tokens
//! and no projected call opens no connection at all, which is what makes the
//! grant cheap to leave switched on.
//!
//! A session is opened at most once per alias per process and is kept for the
//! rest of it: the connection is the expensive part and a second call into the
//! same context should not pay for it again. They are dropped when this value
//! is, which is when the session ends.
//!
//! # The bearer does not cross the port
//!
//! [`Projected::call`](crate::tools::Projected) takes an alias, a tool and
//! arguments. The secret is resolved here, from the store, through the sealing
//! port that is its only path — ADR-0007 D3's "the harness holds the bearer
//! and attaches it when dispatching". Nothing on the port could carry one, and
//! this type has no field a caller could put one in.
//!
//! # The endpoint is a parameter, and that is what makes this checkable
//!
//! [`Projection::over`] takes an [`Endpoint`], exactly as
//! [`Session::attach`] does. The binary passes
//! [`HttpEndpoint`] through [`Projection::new`]; a check passes the in-process
//! `rmcp` server this workspace's other session checks already run against —
//! real protocol bytes over `tokio::io::duplex`, no socket.
//!
//! **That is not a convenience, it is the only shape the rules here permit.**
//! The gate has no network, and **the standing ruling of 2026-09-14 forbids a
//! loopback listener standing in for a server** — the reasoning is recorded in
//! three places in this workspace already, most plainly in
//! `tests/transport_from_outside.rs`: "a fake of a provider at the wire is the
//! mock that [Testing] refuses". A `Projection` that could only ever hold an
//! `HttpEndpoint` would therefore be a network path with nothing asserted
//! about it at all, which is what `zaru-notes`' own transport split exists to
//! avoid. What stays unexercised is the `reqwest` call itself, and that is the
//! same thing `zaru-notes` leaves unexercised for the same reason.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::credentials::alias::Alias;
use crate::credentials::notes::bearer_for_dispatch;
use crate::credentials::sealing::key::KeyStore;
use crate::credentials::store::CredentialStore;
use crate::tools::output::Captured;
use std::collections::BTreeMap;
use zaru_core::iteration::PortFailure;
use zaru_notes::session::{Endpoint, HttpEndpoint, Instance as NotesInstance, Session};

/// Open sessions for [ADR-0007] D5's projected tokens.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
pub struct Projection<'a, K, E> {
    store: &'a CredentialStore,
    keys: &'a K,
    endpoint: E,
    open: tokio::sync::Mutex<BTreeMap<Alias, Session>>,
}

impl<K, E> core::fmt::Debug for Projection<'_, K, E> {
    /// Names how many contexts are open and nothing about any of them.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Projection").finish_non_exhaustive()
    }
}

impl<'a, K: KeyStore, E: Endpoint> Projection<'a, K, E> {
    /// Take a projection over a store and an endpoint.
    #[must_use]
    pub fn over(store: &'a CredentialStore, keys: &'a K, endpoint: E) -> Self {
        Self {
            store,
            keys,
            endpoint,
            open: tokio::sync::Mutex::new(BTreeMap::new()),
        }
    }
}

impl<'a, K: KeyStore> Projection<'a, K, HttpEndpoint> {
    /// Take a projection over the transport the binary uses.
    ///
    /// # Errors
    ///
    /// When this workspace's one HTTP client cannot be built.
    pub fn new(store: &'a CredentialStore, keys: &'a K) -> Result<Self, PortFailure> {
        Ok(Self::over(
            store,
            keys,
            HttpEndpoint::new().map_err(|failure| PortFailure::new(failure.to_string()))?,
        ))
    }
}

impl<K: KeyStore + Sync, E: Endpoint + Sync> crate::tools::Projected for Projection<'_, K, E> {
    async fn call(
        &self,
        alias: &Alias,
        tool: &str,
        arguments: &str,
    ) -> Result<Captured, PortFailure> {
        let mut open = self.open.lock().await;
        if !open.contains_key(alias) {
            let record = self.store.record(alias).ok_or_else(|| {
                PortFailure::new(format!("no credential is stored under the alias `{alias}`"))
            })?;
            let host = match record.reach() {
                Some(crate::credentials::store::StoredReach::InstanceLocked(host)) => host.clone(),
                // An apex entry has no host to attach against, which is the
                // same absence `composer_token` answers `None` for. It is a
                // failure of this call rather than of the session: the token
                // exists and the harness cannot reach an instance with it.
                _ => {
                    return Err(PortFailure::new(format!(
                        "the token `{alias}` names no instance to reach, so a projected call \
                         cannot be addressed",
                    )));
                }
            };
            let secret = self
                .store
                .secret(alias, self.keys)
                .map_err(|failure| PortFailure::new(failure.to_string()))?;
            let session = Session::attach(
                &self.endpoint,
                NotesInstance::new(host),
                bearer_for_dispatch(&secret),
            )
            .await
            .map_err(|failure| PortFailure::new(failure.to_string()))?;
            open.insert(alias.clone(), session);
        }
        let session = open.get(alias).expect("the session was just opened");

        // **The server's refusal is a result rather than a failure**, and that
        // is ADR-0016 D1 row 1 read the way `ToolOutcome` already reads it: a
        // tool that ran and reported a problem produced a result, and the
        // model is the one who should see it. Only a transport failure --
        // nothing answered -- is a `PortFailure`. That is what makes the live
        // half of this work exercisable at all: the `play` token authenticates
        // and reads no workspace, so every real call it makes is an honest
        // refusal, and a refusal that arrived as a port failure would end the
        // turn instead of reaching the model as a tool result.
        match session.call_declared(tool, arguments).await {
            Ok(answered) => Ok(Captured {
                exit_code: 0,
                stdout: answered,
                stderr: String::new(),
            }),
            Err(failure @ zaru_notes::session::NotesError::Transport { .. }) => {
                Err(PortFailure::new(failure.to_string()))
            }
            Err(refused) => Ok(Captured {
                exit_code: 1,
                stdout: String::new(),
                stderr: refused.to_string(),
            }),
        }
    }
}
