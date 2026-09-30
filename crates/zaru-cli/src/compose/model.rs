// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The provider as the loop sees it, and the typed failure the surface keeps.
//!
//! # The class of a provider failure is lost at the port, on purpose
//!
//! [ADR-0016]'s Update of 2026-09-04 names the shape: "**A port failure's
//! class belongs to the port's implementation, not to the value it hands
//! back.**" `zaru-core`'s `PortFailure` carries a sentence and no
//! discriminant, deliberately — that crate's loop has no taxonomy and must not
//! grow one — so a `ToolCallError::Port` cannot be classified by anything
//! downstream of it.
//!
//! Each provider client already says what to do about that, in its own
//! `Model::respond`:
//!
//! > The one place a typed failure becomes `PortFailure`. The port carries a
//! > sentence and nothing else, so the class ADR-0016 puts this failure in is
//! > lost here — which is right for `zaru-core`, whose loop has no taxonomy,
//! > and is why `exchange` is public: **the command surface classifies the
//! > typed failure**, and only the loop sees the flattened one.
//!
//! This is the command surface doing that. [`Classifying`] wraps the client,
//! calls [`ProviderClient::exchange`] — **the same inherent function the
//! client's own `respond` calls**, not a second copy of the mapping — keeps the
//! typed [`ProviderFailure`] where the surface can read it, and hands the loop
//! the identical `PortFailure` it would have had.
//!
//! **Since 2026-09-14 it wraps a [`ProviderClient`] rather than one client's
//! concrete type**, because a second client landed and naming one of them here
//! would have made this module work for exactly half the providers this build
//! carries. The enum is closed, so `taken()` still hands the surface a typed
//! value and `cli::classify` still matches it without a wildcard.
//!
//! **Since 2026-09-30 it calls that function through
//! [`crate::providers::resilience::Resilient`]**, which retries a transient
//! failure under the configured policy and tells the turn of each retry. The
//! failure kept here is the last attempt's, so what the surface classifies is
//! what finally stopped the exchange.
//!
//! **Nothing on any port changes.** `PortFailure` gains no field, `Model`
//! gains no method, `Provider` gains no method, and the client is not edited.
//! The clause ADR-0016 D1 needs — "a port's implementation states the class of
//! its own failures" — is answered at the composition root, which is the one
//! place that can see both the port and the taxonomy.
//!
//! # Why the failure is kept in a lock
//!
//! The same reason each client's own `last` slot is:
//! [`Model::respond`] returns
//! `impl Future + Send` over `&self`, so the client is reachable from more
//! than one task and anything it mutates has to be safe to read from all of
//! them. The guard is taken after the await and dropped before the return, so
//! the future stays `Send`.
//!
//! # It records the last failure, not every one
//!
//! One turn fails once: `tool_call::run` returns on the first
//! `ToolCallError::Port`, so there is never a second model failure in a turn
//! to lose. Keeping a list would be storing something no caller can ask about.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`ProviderClient::exchange`]: crate::providers::ProviderClient::exchange

use crate::providers::resilience::{Policy, Resilient, Retrying};
use crate::providers::{ProviderClient, ProviderFailure};
use core::fmt;
use std::sync::Mutex;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// A provider the loop can call, whose failures the surface can still class.
///
/// See the module documentation. It borrows the client rather than owning it,
/// because the composition also asks the client what it cost — ADR-0012 D7's
/// token accounting reaches [`crate::providers::Provider::usage`], which is a
/// different trait on the same value.
pub struct Classifying<'a> {
    client: &'a ProviderClient,
    resilient: Resilient<'a, ProviderClient>,
    last: Mutex<Option<ProviderFailure>>,
}

impl fmt::Debug for Classifying<'_> {
    /// Names what it wraps and renders neither the client nor the failure.
    ///
    /// The client's own `Debug` is safe by construction and this one still
    /// does not call it: a `Debug` is what ends up in a panic message, and a
    /// provider's failure detail is the provider's own sentence about a
    /// request this harness sent.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Classifying")
            .field("model", &self.client.model().as_str())
            .field("has_failure", &self.taken().is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Classifying<'a> {
    /// Wrap a client for one turn, retrying its transient failures under
    /// `policy` and telling `told` of each retry as it is made.
    ///
    /// **Every exchange a turn makes comes through here** -- the tool-call
    /// loop's, the inner loop's generator and the turn boundary's summariser
    /// -- so every one is retried under the same policy and every retry is
    /// seen. See `providers::resilience`.
    #[must_use]
    pub const fn over(
        client: &'a ProviderClient,
        policy: Policy,
        told: &'a (dyn Fn(&Retrying) + Sync),
    ) -> Self {
        Self {
            client,
            resilient: Resilient::over(client, policy, told),
            last: Mutex::new(None),
        }
    }

    /// The client, for the questions the loop does not ask it.
    #[must_use]
    pub const fn client(&self) -> &'a ProviderClient {
        self.client
    }

    /// The typed failure of the last exchange that failed, if one did.
    ///
    /// `None` before any exchange and after every successful one. A caller
    /// that has a `ToolCallError::Port` naming
    /// [`PortKind::Model`](zaru_core::tool_call::PortKind) reads it here and
    /// classifies it under ADR-0016 D1; a caller that has any other error must
    /// not, and the port kind is what tells them apart.
    #[must_use]
    pub fn taken(&self) -> Option<ProviderFailure> {
        match self.last.lock() {
            Ok(slot) => slot.clone(),
            // A poisoned lock means a panic happened while it was held, which
            // this module never does -- it holds the guard across no await and
            // across no call. Reading through the poison rather than
            // propagating it keeps a defect report from losing the failure
            // that a defect boundary is about to report.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Model for Classifying<'_> {
    /// The client's own answer, unchanged.
    fn capabilities(&self) -> Capabilities {
        Model::capabilities(self.client)
    }

    /// One exchange, keeping the typed failure and handing on the flat one.
    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // One exchange, however many attempts it took: the retries are below
        // this port, so the loop counts what the model was asked and not how
        // often the network had to be tried.
        match self.resilient.exchange(request).await {
            Ok(response) => Ok(response),
            Err(failure) => {
                // The sentence is taken from the typed value before it is
                // stored, so what the loop is given and what the surface will
                // classify are one failure rather than two readings of it.
                let flattened = PortFailure::new(failure.to_string());
                match self.last.lock() {
                    Ok(mut slot) => *slot = Some(failure),
                    Err(poisoned) => *poisoned.into_inner() = Some(failure),
                }
                Err(flattened)
            }
        }
    }
}
