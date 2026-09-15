// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where the credential store meets the Nuclear Notes client.
//!
//! # This module is the whole seam, and that is checkable
//!
//! [ADR-0003] D8 gives `zaru-notes` no sibling dependency, so that crate
//! carries its own [`Bearer`] rather than reusing this crate's
//! [`Secret`] — measured by the `notes-client` arc rather than
//! assumed, because `scripts/check-crate-boundaries.py` counts a
//! *dev*-dependency as a sibling edge exactly as it counts a normal one.
//! `zaru-cli` is the composition root and is therefore where the two meet.
//!
//! **This module is the only place in `crates/zaru-cli/src` that names
//! `zaru_notes::session`**, and that is the invariant one search checks: it
//! means one search finds every crossing between the store and the client,
//! which is the same discipline [`Secret::expose_for_dispatch`] and
//! [`Bearer::expose_for_dispatch`] are named for on their own sides.
//!
//! **The sentence said `zaru_notes` rather than `zaru_notes::session` and was
//! already false when it was written** — `crate::terminal::trie` names
//! `zaru_notes::trie`, and `crate::lib` names the crate to print its version.
//! Narrowed here rather than left, because an invariant a search disproves is
//! worse than none. The narrower one is what the module is actually for: the
//! trie is a data structure with no credential in it, and a session is the
//! thing a bearer is handed to.
//!
//! It is what made [`tool_scope_at`] land here rather than beside its caller in
//! `cli::run`. That function opens a session, and putting it in the command
//! surface would have put `zaru_notes::session` in a second module.
//!
//! **[`corpus_at`] is here for exactly that reason and no other.** It belongs
//! to the composer's fast tier rather than to credentials, and it sits in this
//! module because it opens a session from a stored secret — the same shape and
//! the same precedent. What it hands the listings to is
//! [`Corpus`], a port of two methods, so the
//! widest value in this file is reachable from one function and the rest of
//! the wiring cannot name a `Session` at all.
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
//! The value that exists at dispatch time is a [`Secret`]
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
use crate::credentials::entry::{CachedTool, ToolScope, Ttl};
use crate::credentials::secret::Secret;
use crate::credentials::store::{CredentialStore, StoreError};
use core::fmt;
use core::time::Duration;
use zaru_core::iteration::Clock;
use zaru_notes::session::{
    Bearer, CallRefused, Corpus, HttpEndpoint, Instance as NotesInstance, Invalidation, Listed,
    NotesError, Persona, Session, WorkspaceId as NotesWorkspaceId,
};
use zaru_notes::trie::{CachedEntry, EntryKind as CachedKind};

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
        let declared = session
            .tool_declarations()
            .await
            .map_err(|source| ScopeError::Read {
                alias: alias.clone(),
                source,
            })?;
        let scope = ToolScope::new(declared.into_iter().map(cached));
        self.replace_tools(alias, &scope)
            .map_err(ScopeError::Store)?;
        Ok(Cached { scope, at })
    }
}

/// A Nuclear Notes instance would not complete a session.
///
/// # Why this carries a sentence rather than the client's error
///
/// The module note above says this is the only place in `crates/zaru-cli/src`
/// that names `zaru_notes`, and that is a property one search can check rather
/// than a habit. A failure type carrying [`NotesError`] would put that name in
/// the signature of every function that handled one, and the seam would stop
/// being one module. So the client's own sentence crosses, rendered once, here.
///
/// **Nothing is classified.** [ADR-0016] maps enums all or nothing and
/// `NotesError` is one of the four this crate deliberately leaves unmapped;
/// a caller reads the *command* it was running, not this value's shape.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachFailure {
    /// No HTTP client could be built at all.
    Endpoint(String),
    /// The session did not attach, or a call never came back.
    ///
    /// **Everything that is not an answer**: DNS, a refused connection, a
    /// dropped stream, a shape this client could not read. The instance has
    /// said nothing, so nothing here licenses a conclusion about what the
    /// token may reach.
    Session(String),
    /// The instance **answered**, and its answer was no.
    ///
    /// Added 2026-09-15 for [ADR-0005] D8's eviction rule, and the reason is a
    /// defect this split was found to have: `Endpoint` is only "no HTTP client
    /// could be built", so a DNS failure, a refused connection and a revoked
    /// token all arrived as [`ReachFailure::Session`] and were
    /// indistinguishable. A caller deciding whether to forget a cached corpus
    /// has to tell "the instance says you may not read this" from "the
    /// instance is not there", and only [`NotesError::Call`] says the first.
    ///
    /// The refusal crosses whole rather than as a sentence, which is the one
    /// place this type departs from the paragraph above: a caller needs the
    /// *kind* of failure and not only its words, and `CallRefused` is a
    /// three-field value with no `NotesError` in it, so the seam stays one
    /// module wide.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    Refused(CallRefused),
}

impl fmt::Display for ReachFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint(detail) | Self::Session(detail) => f.write_str(detail),
            // `CallRefused`'s own sentence, which is what the strip has printed
            // for a refusal since `notes-hints-wiring` and is unchanged by the
            // variant arriving: "the server refused pages.list: You are not a
            // member of that workspace. (code -32002)".
            Self::Refused(refused) => write!(f, "{refused}"),
        }
    }
}

impl std::error::Error for ReachFailure {}

/// What [ADR-0007] D6 calls "one `tools/list` per token at attach", against a
/// real instance, for a token that is not in the store yet.
///
/// # The order is forced by two clauses at once
///
/// D6 caches the scope "once per token at attach". D8 requires an apex
/// credential's confirmation to state "what it grants", and
/// `CredentialStore::add` composes that sentence from the entry's own
/// [`ToolScope`]. An entry stored before its scope was read would therefore
/// tell the user it grants **zero** tools while asking them to accept it,
/// which is a confirmation that is worse than none. So the session is opened
/// and `tools/list` is read *before* an entry exists, and the scope is on the
/// entry the store is handed.
///
/// # The bearer is built here and dropped here
///
/// It is made from the secret by [`bearer_for_dispatch`], handed to
/// [`Session::attach`], and never held: neither this function nor
/// [`Session`] has a field one could sit in.
///
/// # Errors
///
/// [`ReachFailure`], carrying the client's own words and never the token.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
pub async fn tool_scope_at(host: &str, secret: &Secret) -> Result<ToolScope, ReachFailure> {
    let endpoint =
        HttpEndpoint::new().map_err(|failure| ReachFailure::Endpoint(failure.to_string()))?;
    let session = Session::attach(
        &endpoint,
        NotesInstance::new(host),
        bearer_for_dispatch(secret),
    )
    .await
    .map_err(|failure| ReachFailure::Session(failure.to_string()))?;
    let declared = session
        .tool_declarations()
        .await
        .map_err(|failure| ReachFailure::Session(failure.to_string()))?;
    Ok(ToolScope::new(declared.into_iter().map(cached)))
}

/// One `tools/list` declaration as [ADR-0007] D6's cache keeps it.
///
/// The one place this crate turns `zaru-notes`' reading into this crate's, so
/// the two halves of D6's cross-crate contract meet in a named function rather
/// than at three call sites — the shape
/// [`bearer_for_dispatch`] already has for
/// the other direction.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
fn cached(declared: zaru_notes::session::ToolDeclaration) -> CachedTool {
    CachedTool::declared(declared.name, declared.description, declared.input_schema)
}

/// Which shape of [`ReachFailure`] a client error is, and it turns on one
/// variant.
///
/// [`NotesError::Call`] is the instance answering: it carries the tool, the
/// code and the server's own sentence, and it is the only one of the six that
/// says anything about what this token may reach. Everything else — no
/// transport, no session, a dropped stream, an answer this client could not
/// read, a workspace that would not attach — is silence, and silence is not a
/// refusal. [ADR-0005] D8's cache turns on exactly that distinction.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
fn reach_failure(failure: NotesError) -> ReachFailure {
    match failure {
        NotesError::Call(refused) => ReachFailure::Refused(refused),
        other => ReachFailure::Session(other.to_string()),
    }
}

/// [ADR-0005] D3's corpus for one workspace, over the narrow port.
///
/// # Why this takes the port and not a [`Session`]
///
/// A `Session` offers `read_page`, `search`, `ground` and
/// `attach_workspace`. [`Corpus`] offers two listings and there is no third —
/// see that module for why a type carries [ADR-0006] D4's "cannot write"
/// while no token the substrate can mint carries the scope. This function is
/// the whole consumer of that port, so it is the one place the narrowing has
/// to hold, and it holds by signature rather than by discipline.
///
/// # Both listings, and a refusal is a refusal of the whole corpus
///
/// A trie built from the pages of a workspace whose atoms were refused is a
/// trie that is silently missing half of what D3 promises, and
/// [`crate::terminal::trie::NotesTrie`] cannot tell the difference — an empty
/// atom listing and a refused one produce the same strip. So the first
/// refusal ends it. That is [ADR-0005] D8's "degrade honestly" read the only
/// way it can be read here: say nothing was reached rather than serve a
/// corpus whose shape is an accident.
///
/// **An empty answer is not a refusal.** A workspace holding no atoms
/// answers `[]`, measured against the live server on 2026-09-14, and that is
/// a cortex with no atoms rather than one that would not say.
///
/// # Errors
///
/// [`ReachFailure::Session`] carrying the client's own sentence and never the
/// token.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
pub async fn corpus_from(
    source: &impl Corpus,
    workspace: &str,
) -> Result<Vec<CachedEntry>, ReachFailure> {
    let id = NotesWorkspaceId::new(workspace);

    let pages = source.pages(&id).await.map_err(reach_failure)?;
    let atoms = source.atoms(&id).await.map_err(reach_failure)?;

    // The workspace on every entry is the slug the caller asked for, not one
    // read back off a row. A listing row carries `path` and `title` and no
    // workspace at all -- ADR-0006 D6's identity pair is completed by the
    // argument the call named, which is the same reasoning `session::listing`
    // gives for `Listed` carrying two fields and no identifier.
    let entry = |listed: Listed, kind: CachedKind| {
        CachedEntry::new(workspace, listed.path, listed.title, kind)
    };
    Ok(pages
        .into_iter()
        .map(|listed| entry(listed, CachedKind::Page))
        .chain(
            atoms
                .into_iter()
                .map(|listed| entry(listed, CachedKind::Atom)),
        )
        .collect())
}

/// The same, opening a session against a real instance first.
///
/// # The session is closed as soon as the corpus is built
///
/// [ADR-0005] D3's second tier is a live `search.global` on the composer's
/// hot path and is **not built**; holding this session open for it would buy
/// D8's thirty-minute idle eviction and the transparent re-initialisation
/// that clause asks for, neither of which exists. So the session is dropped
/// here, and the day tier two arrives it opens its own.
///
/// # Errors
///
/// [`ReachFailure::Endpoint`] when no HTTP client can be built, and
/// [`ReachFailure::Session`] when the instance will not complete a session or
/// a listing is refused.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
pub async fn corpus_at(
    host: &str,
    secret: &Secret,
    workspace: &str,
) -> Result<Vec<CachedEntry>, ReachFailure> {
    let endpoint =
        HttpEndpoint::new().map_err(|failure| ReachFailure::Endpoint(failure.to_string()))?;
    let session = Session::attach(
        &endpoint,
        NotesInstance::new(host),
        bearer_for_dispatch(secret),
    )
    .await
    .map_err(|failure| ReachFailure::Session(failure.to_string()))?;
    corpus_from(&session, workspace).await
}

/// [ADR-0027] D1's persona page, over the narrow port.
///
/// # Why this takes the port and not a [`Session`]
///
/// The same argument [`corpus_from`] makes, one method narrower.
/// [`Persona`] offers one read and there is no second — see
/// `zaru_notes::session::persona` for why a *second* port exists rather than a
/// third method on [`Corpus`], which would have deleted that port's landed
/// two-method guarantee. This function is the whole consumer of the persona
/// port, so it is the one place the narrowing has to hold, and it holds by
/// signature rather than by discipline.
///
/// # The body is returned unparsed, and that is [ADR-0031] riding this path
///
/// Nothing here reads a section out of the page. [ADR-0031] D3 appends the
/// relationship memory to the served prompt **before it is returned**, and
/// that record's Status tracking forbids the harness a second fetch — "a
/// second fetch path is a second thing that can disagree". Because this
/// function returns one whole body it did not inspect, the day the memory is
/// appended to that page it arrives with the persona and **the harness's fetch
/// count is still one**. Nothing is built for it and nothing needs to be.
///
/// # Errors
///
/// [`ReachFailure::Refused`] carrying the server's own sentence where the
/// instance answered and refused, and [`ReachFailure::Session`] where it said
/// nothing — never the token.
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
/// [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
pub async fn persona_from(
    source: &impl Persona,
    path: &str,
    workspace: &str,
) -> Result<String, ReachFailure> {
    let id = NotesWorkspaceId::new(workspace);
    source.read_page(path, &id).await.map_err(reach_failure)
}

/// The same, opening a session against a real instance first.
///
/// The session is closed as soon as the page is read, for the reason
/// [`corpus_at`]'s is: nothing in this harness holds a Nuclear Notes session
/// open between calls, and a persona is read once per session.
///
/// **There is no timeout here beyond `reqwest`'s own**, which is the absence
/// [`corpus_at`] already has. A number nobody chose would be a constant this
/// harness authored for a wait no record describes; ruled 2026-09-15 under
/// directive 20, open to Jeshua's veto.
///
/// # Errors
///
/// [`ReachFailure::Endpoint`] when no HTTP client can be built, and as
/// [`persona_from`] otherwise.
pub async fn persona_at(
    host: &str,
    secret: &Secret,
    path: &str,
    workspace: &str,
) -> Result<String, ReachFailure> {
    let endpoint =
        HttpEndpoint::new().map_err(|failure| ReachFailure::Endpoint(failure.to_string()))?;
    let session = Session::attach(
        &endpoint,
        NotesInstance::new(host),
        bearer_for_dispatch(secret),
    )
    .await
    .map_err(|failure| ReachFailure::Session(failure.to_string()))?;
    persona_from(&session, path, workspace).await
}

/// The composer's credential, opened: which token, which instance, and the
/// sealed value itself.
///
/// **One function, two readers**, in the shape `cached` already has for the
/// other direction. The composer's corpus and [ADR-0027]'s persona are read
/// with the same token against the same instance, and resolving that twice
/// would be two answers to one question — the failure `credential_store`'s own
/// comment names for opening the store twice.
///
/// `None` is a machine with no store entry the composer can read with, which
/// is every machine before the first `notes tokens add`. It is deliberately
/// not an error: [ADR-0005] D8's strip and [ADR-0027]'s absence are both
/// states this harness has a correct answer for.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[must_use]
pub fn composer_secret(store: &CredentialStore) -> Option<(Alias, String, Secret)> {
    let (alias, host) = composer_token(store)?;
    let keyring = crate::credentials::OsKeyring::for_store(store.root());
    let keys = crate::credentials::HarnessKeys::from_process(&keyring);
    let secret = store.secret(&alias, &keys).ok()?;
    Some((alias, host, secret))
}

/// Which stored token the composer reads with, and the host it reaches.
///
/// # [ADR-0007] D4 says which token wins; it does not say what happens when
/// none does
///
/// D4 is "exactly one token is flagged `composer`", and that clause is
/// unchanged: a token carrying the role wins whenever one exists. What D4 does
/// not cover is a store where **none** carries it, which is the state of every
/// machine — `CredentialStore::grant_composer_role` refuses the role to any
/// token whose cached scope leaves [ADR-0006] D4's set, and every Nuclear
/// Notes token measured on 2026-09-14 grants 94 tools.
///
/// So the rule, accepted 2026-09-14 under a delegated coordinator ruling and
/// open to Jeshua's veto, in three cases:
///
/// 1. A token carrying the role wins.
/// 2. Otherwise, **exactly one** stored Nuclear Notes token serves. One token
///    is unambiguous; refusing to read with the only credential the user has
///    would be the harness declining to use what it was given.
/// 3. Otherwise nothing serves. Several tokens with no role is a **choice**,
///    and a harness that picked one would be guessing which cortex the person
///    meant — the failure [ADR-0007] D5's namespace design exists to prevent
///    one level up, arriving in the one place D5 does not reach. The remedy is
///    to grant the role, which `zaru notes use <alias>` offers.
///
/// What makes case 2 safe is not this function. It is
/// [`Corpus`]: the builder is handed a port of
/// two listings, so there is no write to make whatever the token's scope is.
///
/// # An apex token names no host, so it cannot be the one
///
/// [ADR-0007] D8's apex entry has "no instance boundary" and
/// `notes tokens add <alias> <host> apex` stores an apex reach **without the
/// host** — correctly, because an apex token is not bound to one. But
/// `HttpEndpoint` reaches an instance by host, so there is no address to open.
/// An apex entry is therefore skipped here rather than opened against a host
/// invented at this call site, and a store holding only apex tokens serves
/// nothing. Named rather than silently falling into case 3.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[must_use]
pub fn composer_token(store: &CredentialStore) -> Option<(Alias, String)> {
    let host = |record: &crate::credentials::store::Record| match record.reach() {
        Some(crate::credentials::store::StoredReach::InstanceLocked(host)) => Some(host.clone()),
        Some(crate::credentials::store::StoredReach::Apex) | None => None,
    };

    // Case 1. D4's own clause, and it is consulted first so that granting the
    // role always changes which token is used.
    if let Some((alias, record)) = store.composer() {
        return host(record).map(|host| (alias.clone(), host));
    }

    // Cases 2 and 3. `is_notes` is what keeps a provider key out: it carries
    // no reach, no tools and no role, and it speaks no MCP at all.
    let mut notes = store.records().filter(|(_, record)| record.is_notes());
    let only = notes.next()?;
    if notes.next().is_some() {
        return None;
    }
    host(only.1).map(|host| (only.0.clone(), host))
}

/// [ADR-0007] D8's marking for the status line, when the composer's credential
/// is apex.
///
/// D8: "Apex entries are **marked wherever the token appears**: `/notes
/// tokens`, the status line when the composer holds one, and the description
/// the agent reads." This is the reader for the second of those three; the
/// listing reads [`Reach::APEX_MARKING`](crate::credentials::Reach) at
/// `cli::render` and the agent's description reads it at
/// `credentials::projection`, and all three now answer with the one constant.
///
/// # Why this is not read off [`composer_token`]
///
/// Because that function answers `None` in exactly the case this one exists
/// for. Its first case consults [`CredentialStore::composer`] and then maps
/// the record through a host — and an apex entry has none, so **a composer
/// role held by an apex token makes `composer_token` answer nothing at all**.
/// The session therefore opens with no Nuclear Notes client, the hint strip
/// shows its absence line, and this marking is the one thing the row can
/// truthfully say about a credential the session holds and cannot use. Reading
/// the marking off a function that has already discarded the entry would have
/// made the row silent in the one state D8 names.
///
/// It is also the narrower read. It touches [`CredentialStore::composer`] and
/// [`Record::reach`](crate::credentials::Record) and nothing else: **no
/// secret, no keyring and no sealing key**, so a session on a machine whose
/// key is unreachable still marks an apex composer.
///
/// # What answers `None`, and why every machine that exists does
///
/// A store with no composer, a composer that is instance-locked, and a store
/// that will not open at all. The first of those is the ordinary case:
/// [`CredentialStore::grant_composer_role`] refuses the role to any token
/// whose cached scope leaves [ADR-0006] D4's set, and every Nuclear Notes
/// token measured on 2026-09-06 and 2026-09-14 grants 94 tools against that
/// set's nine. So **no real credential can hold the composer role on any
/// machine today, apex or not**, and this function's `Some` arm is reachable
/// only from a store built by hand. That is a fact about the substrate's token
/// scoping rather than about this code, and it is the same standing limit
/// `notes use` and `notes tokens rm` each report.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[must_use]
pub fn composer_apex_marking(store: &CredentialStore) -> Option<&'static str> {
    let (_, record) = store.composer()?;
    matches!(
        record.reach(),
        Some(crate::credentials::store::StoredReach::Apex)
    )
    .then_some(crate::credentials::Reach::APEX_MARKING)
}
