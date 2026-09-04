// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where the credential store meets the Nuclear Notes client.
//!
//! # This module is the whole seam, and that is checkable
//!
//! [ADR-0003] D8 gives `zaru-notes` no sibling dependency, so that crate
//! carries its own [`Bearer`] rather than reusing this crate's
//! [`Secret`](super::Secret) — measured by the `notes-client` arc rather than
//! assumed, because `scripts/check-crate-boundaries.py` counts a
//! *dev*-dependency as a sibling edge exactly as it counts a normal one.
//! `zaru-cli` is the composition root and is therefore where the two meet.
//!
//! **This module is the only place in `crates/zaru-cli/src` that names
//! `zaru_notes`**, the binary's version print aside. That is not tidiness: it
//! means one search finds every crossing between the store and the client,
//! which is the same discipline [`Secret::expose_for_dispatch`] and
//! [`Bearer::expose_for_dispatch`] are named for on their own sides.
//!
//! # Why the conversion is a function and not `impl From`
//!
//! `impl From<&Secret> for Bearer` compiles — the orphan rule permits it,
//! measured rather than reasoned about. It is deliberately not written.
//! A `From` impl makes the conversion available implicitly through `.into()`
//! in argument position, so the number of crossings becomes unbounded and no
//! search finds them. [`bearer_for_dispatch`] is one named door, and a call to
//! it says what it is doing.
//!
//! # Why it lives beside the store rather than on `Entry`
//!
//! The value that exists at dispatch time is a [`Secret`](super::Secret)
//! handed back by [`CredentialStore::secret`](super::CredentialStore::secret).
//! An [`Entry`](super::Entry) is *consumed* by
//! [`CredentialStore::add`](super::CredentialStore::add) and is never seen
//! again, so a method there would be reachable only before the secret was
//! sealed — the wrong end of the lifecycle. It would also invite keeping an
//! entry alive after storing it purely to get a bearer out, which is a second
//! in-memory copy of the secret living outside the sealing port.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [`Secret::expose_for_dispatch`]: super::Secret::expose_for_dispatch

use crate::credentials::alias::Alias;
use crate::credentials::entry::{ToolScope, Ttl};
use crate::credentials::secret::Secret;
use crate::credentials::store::{CredentialStore, StoreError};
use core::fmt;
use core::time::Duration;
use zaru_core::iteration::Clock;
use zaru_notes::session::{Bearer, Invalidation, NotesError, Session};

/// The bearer a Nuclear Notes session authenticates with, from a stored secret.
///
/// **This is the one place a stored secret becomes a value another crate
/// holds.** [ADR-0007] D3 puts the bearer on the harness's own dispatch path
/// and nowhere else: not in a prompt, not in a transcript, not in a log, not in
/// a tool result. Both types refuse to render the value, so what this function
/// converts is one un-showable value into another; what it must not become is a
/// place the value is copied for any other purpose, which is why it is named
/// for the purpose rather than for the types.
///
/// The value crosses intact and is asserted to, because a conversion that
/// dropped it would satisfy every absence assertion on both sides perfectly.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[must_use]
pub fn bearer_for_dispatch(secret: &Secret) -> Bearer {
    Bearer::new(secret.expose_for_dispatch())
}

/// One `tools/list`, and the caller's clock when it was taken.
///
/// # `at` is in memory and never reaches the file
///
/// It is a **monotonic offset** from wherever the caller's [`Clock`] started,
/// for the reason `zaru-core`'s own port gives: an instant cannot be
/// constructed at a chosen value, so a clock a check cannot set is a clock a
/// check cannot assert on. The consequence is that the number is meaningless
/// in any other process — an offset written to a file that outlives the run
/// that took it says nothing on the next run — so it lives here and
/// [`Record`](super::store::Record) has **no field it could go in**. That is
/// the same argument [ADR-0014] D4 makes about configuration, applied to a
/// clock reading rather than to a secret, and a check destructures `Record`
/// exhaustively so that adding such a field stops compiling.
///
/// What it therefore bounds is a **session's** staleness rather than a
/// laptop's, which is what [ADR-0007] D6's "backstop for a missed
/// notification" over a live stream actually means.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cached {
    /// The tool names the server reported, in its order and uninterpreted.
    pub scope: ToolScope,
    /// The caller's clock reading, taken **before** the call. See the type.
    pub at: Duration,
}

impl Cached {
    /// [ADR-0007] D6's TTL backstop, over the caller's clock.
    ///
    /// **This is the only place [`Ttl::get`] is unwrapped**, which is what
    /// makes the window the one the store validated rather than a number
    /// somebody wrote at a call site. `None` while the window still holds.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    #[must_use]
    pub fn expired(&self, now: Duration, ttl: Ttl) -> Option<Invalidation> {
        Invalidation::expired(self.at, now, ttl.get())
    }
}

/// What one refresh did: why it happened, and what the cache now holds.
///
/// `because` is why this type exists rather than the refresh returning a bare
/// [`Cached`]. [ADR-0007] D6 says "Refresh, then surface the failure — never
/// retry blind", and an `Invalidation::Claimed` **carries the server's own
/// refusal**. So the failure is in the caller's hand twice over after a
/// refresh: in its own copy, which
/// [`CredentialStore::refresh_tool_scope`] cannot consume because it takes the
/// signal by reference, and here beside the scope that replaced the stale one.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refreshed {
    /// The signal that caused this refresh, carried out unchanged.
    pub because: Invalidation,
    /// The cache as it now stands.
    pub cached: Cached,
}

/// A cached tool scope could not be read or could not be kept.
///
/// # This classifies nothing, deliberately
///
/// [ADR-0016] gives `zaru-cli` the mapping into its taxonomy, and this type
/// stays outside it. A `forbidden` from Nuclear Notes could be a revoked
/// token, a scope change, or a workspace the user was removed from —
/// user-correctable, environmental and neither — and [ADR-0006] D7 says in as
/// many words that the server does not reveal which gate tripped. A mapping
/// made here would invent a distinction the substrate refuses to make, so
/// [`NotesError`] is **carried** rather than read. This type joins
/// [`StoreError`] and `zaru_core::iteration::IterationError` in that record's
/// deliberately unmapped set.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub enum ScopeError {
    /// `tools/list` could not be read from the session.
    Read {
        /// The token whose scope was being read.
        alias: Alias,
        /// What the client said, in its own words and unclassified.
        source: NotesError,
    },
    /// The store would not take the scope that came back.
    Store(StoreError),
}

impl fmt::Display for ScopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { alias, source } => write!(
                f,
                "the tool scope for \"{alias}\" could not be read from Nuclear Notes: {source}"
            ),
            Self::Store(error) => write!(f, "the refreshed tool scope could not be kept: {error}"),
        }
    }
}

impl std::error::Error for ScopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Store(error) => Some(error),
        }
    }
}

impl CredentialStore {
    /// [ADR-0007] D6's "once per token at attach".
    ///
    /// D6: "The harness calls `tools/list` once per token at attach and caches
    /// the result in the entry. The three-gate enforcement in [ADR-0135] means
    /// that response already reflects exactly what the token grants, so the
    /// cache needs no interpretation." Nothing here interprets it.
    ///
    /// # Errors
    ///
    /// [`ScopeError::Read`] when the session cannot answer, and
    /// [`ScopeError::Store`] when the store will not keep the answer.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0135]: https://cortex.page/adrs/p/0135-mcp-token-tool-scope-presets
    pub async fn cache_tool_scope(
        &mut self,
        alias: &Alias,
        session: &Session,
        clock: &dyn Clock,
    ) -> Result<Cached, ScopeError> {
        self.read_scope_once(alias, session, clock).await
    }

    /// [ADR-0007] D6's invalidation half: one `tools/list`, on a signal.
    ///
    /// # A signal is the only key that opens this door
    ///
    /// There is no way to call this without holding an [`Invalidation`], and an
    /// `Invalidation` can only be obtained from one of D6's three causes:
    /// [`Session::take_list_changed`] and [`Session::await_list_changed`] for
    /// the notification, [`Cached::expired`] for the TTL backstop, and
    /// [`Invalidation::claimed`] for a refusal of a tool the cache claimed. So
    /// "the cache is never refreshed without a cause" is structural rather than
    /// a rule somebody follows.
    ///
    /// # Nothing retries, and that is structural too
    ///
    /// D6: "Refresh, then surface the failure — never retry blind." This body
    /// makes exactly one call and contains no loop, and **there is no retry
    /// method to call** — not on [`Invalidation`], not on [`Session`], nowhere
    /// in `zaru-notes`. The signal is taken **by reference**, so this cannot
    /// consume a failure it was handed: a caller holding
    /// `Invalidation::Claimed(refused)` still holds the refusal afterwards, and
    /// [`Refreshed::because`] carries a second copy beside the fresh scope.
    ///
    /// # Errors
    ///
    /// [`ScopeError::Read`] and [`ScopeError::Store`], as
    /// [`Self::cache_tool_scope`].
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub async fn refresh_tool_scope(
        &mut self,
        alias: &Alias,
        session: &Session,
        signal: &Invalidation,
        clock: &dyn Clock,
    ) -> Result<Refreshed, ScopeError> {
        let cached = self.read_scope_once(alias, session, clock).await?;
        Ok(Refreshed {
            because: signal.clone(),
            cached,
        })
    }

    /// One `tools/list`, written through to the entry the projection reads.
    ///
    /// The clock is read **before** the call rather than after, which is the
    /// conservative direction: the window then covers the round trip too, so a
    /// slow call cannot buy the cache extra life.
    async fn read_scope_once(
        &mut self,
        alias: &Alias,
        session: &Session,
        clock: &dyn Clock,
    ) -> Result<Cached, ScopeError> {
        let at = clock.now();
        let names = session.tools().await.map_err(|source| ScopeError::Read {
            alias: alias.clone(),
            source,
        })?;
        let scope = ToolScope::new(names);
        self.replace_tools(alias, &scope)
            .map_err(ScopeError::Store)?;
        Ok(Cached { scope, at })
    }
}
