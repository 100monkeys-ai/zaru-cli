// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which provider kind serves a resolved alias.
//!
//! # This is a proposed reading and it is settled in code nowhere
//!
//! [ADR-0012] D3 names five provider kinds and D4 resolves an alias to a model
//! identifier, and **no clause says which kind answers for a given alias**.
//! The question is on [ADR Status — open questions] and both `provider-aliases`
//! and `composer-wiring` recorded that the code took the kind as a parameter
//! rather than inventing a key, because with exactly one client the question
//! did not arise.
//!
//! **It arises the moment a second client exists**, which is what
//! `KINDS_WITH_A_CLIENT`'s own documentation says: "the day a second client
//! lands this array has two entries and the question is real … the arc that
//! adds the second client cannot do so without meeting this."
//!
//! What this module implements is a **delegated coordinator ruling of
//! 2026-09-14, open to Jeshua's veto**, written as a proposed amendment on
//! ADR-0012 before this code existed. A person settles whether the key exists
//! at all; the checks here pin the reading so that deciding it the other way
//! reddens rather than passing unnoticed.
//!
//! # Why the previous answer could not simply be extended
//!
//! The composition did not lack an answer — it had one, and it was **credential
//! presence**: the first kind with a client that the store held a key for.
//! That answer cannot express this workspace's second client, because `ollama`
//! needs no credential at all. A keyless kind would be unreachable by
//! construction however the alias-to-kind key were spelled, so the rule below
//! has two parts rather than one.
//!
//! # The rule
//!
//! 1. **An explicit `provider.<alias>.kind` decides it**, resolved through
//!    ADR-0014's five layers like every other key.
//! 2. **Absent that key**, the kind is the first in [`KINDS_WITH_A_CLIENT`]'s
//!    declaration order whose **requirement** holds — a held key for a kind
//!    that needs one, a configured endpoint for a kind that does not.
//!
//! Part 2 makes the old behaviour the special case of a general rule rather
//! than replacing it: a machine holding a Gemini key and nothing else resolves
//! exactly as it did before this module existed, which is asserted rather than
//! assumed.
//!
//! # No reachability probe, deliberately
//!
//! Nothing here asks whether a local server is listening. A probe at selection
//! would make choosing a provider perform a side effect, and would make the
//! answer depend on the instant it was asked — so a task could select
//! differently from the task before it for reasons the user never sees. An
//! unreachable local endpoint is instead a **user-correctable refusal at the
//! first call**, which is where `providers::ollama::failure` puts it and which
//! [ADR-0016] D1 row 2 names in as many words.
//!
//! # The key is a sibling, not a child
//!
//! `provider.<alias>.kind` rather than `model.<alias>.kind`, for the reason
//! `providers::inference` measured for its own key: `model.<alias>` already
//! holds text, so a nested table would make one key a value and a table at
//! once, and at ADR-0014 D2's cross-layer merge the loser vanishes with
//! nothing reported.
//!
//! **It does not collide with `provider.<kind>.endpoint`**, and that is
//! checked rather than assumed: no alias is spelled like any kind, and the two
//! keys have different leaves in any case.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [ADR Status — open questions]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status-questions
//! [`KINDS_WITH_A_CLIENT`]: crate::compose::KINDS_WITH_A_CLIENT

use super::alias::ModelAlias;
use super::kind::ProviderKind;
use crate::config::Key;
use core::fmt;

/// The last segment of the key that names an alias's provider kind.
pub const KIND_LEAF: &str = "kind";

/// The [ADR-0014] configuration key that names which kind serves an alias.
///
/// `provider.<alias>.kind`. See the module documentation for the spelling.
///
/// # Panics
///
/// Never. The segments are this workspace's own and none is a shape
/// [`Key::new`] refuses.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn kind_key(alias: ModelAlias) -> Key {
    Key::new(&format!(
        "{}.{}.{}",
        ProviderKind::TABLE,
        alias.as_str(),
        KIND_LEAF
    ))
    .expect("an alias spelling is a well-formed configuration key segment")
}

/// What a kind needs before it can be chosen with nothing configured.
///
/// **Not a capability and not a probe.** It is the question "has the user
/// already given this machine what this kind needs in order to work at all",
/// answered from configuration and the credential store rather than from the
/// network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// The credential store holds this kind's key.
    ///
    /// Every kind reached with a secret. Before 2026-09-14 this was the whole
    /// of the selection rule, for the one kind that had a client.
    HeldKey,
    /// A layer other than the built-in default sets this kind's endpoint.
    ///
    /// A kind that needs no credential has nothing in the store to be found
    /// by, so what says "the user meant this one" is that they said where it
    /// is. **The built-in default does not count**, because a default every
    /// machine carries would make a keyless kind the answer on every machine
    /// and no user would have chosen anything.
    ConfiguredEndpoint,
}

impl Requirement {
    /// What this kind needs to be *selected*.
    ///
    /// **Derived from [`KeyUse`] rather than stated again**, which is the
    /// change of 2026-09-14. Until then the two questions — what selects a
    /// kind, and whether a kind sends a key — had the same answer for every
    /// kind, so one enum could carry both. `openai-compatible` is where they
    /// part: it is selected by a configured endpoint and it sends a key when
    /// one is held. Deriving is what keeps them one statement; writing this
    /// match a second time is how the two would come to disagree about a kind
    /// somebody added to only one of them.
    #[must_use]
    pub const fn of(kind: ProviderKind) -> Self {
        match KeyUse::of(kind) {
            // A kind that cannot work without a key is found by its key.
            KeyUse::Required => Self::HeldKey,
            // A kind that can work without one has nothing in the store to be
            // found by, so what says "the user meant this one" is the endpoint.
            KeyUse::Optional | KeyUse::Never => Self::ConfiguredEndpoint,
        }
    }
}

/// Whether a kind's client sends a key, which is **not** the same question as
/// [`Requirement`].
///
/// # The two questions had one answer until 2026-09-14
///
/// `gemini`, `anthropic` and `aegis` cannot be reached without a key, so a
/// held key both selects them and is sent. `ollama` takes none, so a
/// configured endpoint selects it and nothing is sent. For those four the two
/// questions coincide and one enum answered both.
///
/// **`openai-compatible` is the first kind where they differ**, and it is not
/// an awkward case — it is what [ADR-0012] D3 asks for. That kind is
/// "everything OpenAI-shaped — vLLM, LM Studio, most gateways", which spans a
/// server on the reader's laptop that wants no key and a hosted gateway that
/// demands one. Measured 2026-09-14 with live controls: Ollama's own
/// `/v1/chat/completions` answers 200 to a request carrying a bogus bearer,
/// and `llama-server --api-key` answers 401 to a request carrying none.
/// **Both are this kind**, so neither "needs a key" nor "takes none" is true
/// of it and the honest answer is a third value.
///
/// # Why this is the primary fact and `Requirement` the derived one
///
/// Because this one is a property of the *protocol* and that one is a policy
/// about selection. A client either has somewhere to put a key or it does not;
/// what makes a kind the one a machine reaches for is a rule this workspace
/// chose and could choose differently. Deriving the policy from the fact means
/// a kind added here cannot be forgotten there.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyUse {
    /// The client cannot reach this kind at all without a key.
    Required,
    /// The client sends a key when this machine holds one, and works without.
    ///
    /// The absence of a key is **not** a refusal. A reader pointing the harness
    /// at a local server has nothing to store and must not be asked for one.
    Optional,
    /// The client sends no key and there is nowhere to put one.
    Never,
}

impl KeyUse {
    /// What this kind does with a key.
    ///
    /// **Exhaustive, with no wildcard arm**, so a sixth kind fails to compile
    /// here rather than silently inheriting an answer nobody chose for it.
    #[must_use]
    pub const fn of(kind: ProviderKind) -> Self {
        match kind {
            // Reached only with a secret.
            ProviderKind::Anthropic | ProviderKind::Gemini | ProviderKind::Aegis => Self::Required,
            // A gateway wants one and a local server does not, and the kind
            // alone does not say which -- see this type's documentation.
            ProviderKind::OpenAiCompatible => Self::Optional,
            // `/api/chat` needs none, and a request carrying no authorization
            // header of any kind answers 200 -- measured with a live control.
            ProviderKind::Ollama => Self::Never,
        }
    }
}

/// Why no provider kind could be chosen.
///
/// Carries what the machine *does* have, so the refusal can name both routes
/// out rather than only the one the reader did not take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoKindSelected {
    /// The alias that was being resolved.
    pub alias: ModelAlias,
    /// The kinds this build carries a client for, in declaration order.
    pub with_a_client: Vec<ProviderKind>,
}

/// How the keyless half of [`NoKindSelected`]'s sentence ends.
///
/// # Why this is a constant, and why it stopped saying "and start its server"
///
/// The sentence read "…or set `provider.<alias>.kind` to one that does not —
/// `ollama` — and start its server" while `ollama` was the only kind in that
/// list, and every word of it was true. **`openai-compatible` joins that list
/// on 2026-09-14 and makes the ending false for half the readers it reaches**:
/// that kind covers a hosted gateway as readily as a local server, a gateway
/// is not something the reader starts, and telling them to start it is a
/// remedy that cannot be followed — which is [ADR-0016] D2's "a stack trace
/// with better grammar" with a wrong suggestion attached.
///
/// So the ending names what the reader must supply rather than an act they may
/// not be able to perform: an endpoint, and behind it either a server they run
/// or a gateway they have. It is a constant because it is **Jeshua's to veto
/// as words** — recorded on ADR-0012's amendments page as a delegated ruling —
/// and a sentence somebody may want to rewrite should be in one place with a
/// name, not spliced into a `write!`.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const KEYLESS_ENDING: &str = "to where a running server or a gateway is listening for it";

impl fmt::Display for NoKindSelected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keyed: Vec<&str> = self
            .with_a_client
            .iter()
            .filter(|kind| Requirement::of(**kind) == Requirement::HeldKey)
            .map(|kind| kind.as_str())
            .collect();
        let keyless: Vec<&str> = self
            .with_a_client
            .iter()
            .filter(|kind| Requirement::of(**kind) == Requirement::ConfiguredEndpoint)
            .map(|kind| kind.as_str())
            .collect();
        write!(
            f,
            "nothing on this machine says which provider should answer for `{alias}`. Either \
             store a key for one that needs one — {keyed} — with `zaru providers keys add \
             <kind>`, or set `{key}` to one that does not — {keyless} — and set that kind's \
             `endpoint` {ending}",
            alias = self.alias,
            keyed = keyed.join(", "),
            keyless = keyless.join(", "),
            key = kind_key(self.alias),
            ending = KEYLESS_ENDING,
        )
    }
}

impl std::error::Error for NoKindSelected {}

/// Choose the kind that serves `alias`.
///
/// `configured` is what `provider.<alias>.kind` resolved to, if any layer set
/// it; `holds_key` answers whether the credential store holds a kind's key;
/// `has_endpoint` answers whether any layer set a kind's endpoint. They are
/// passed in rather than read here so that this function is a rule over facts
/// rather than a second place that knows how to read a store.
///
/// # Errors
///
/// [`NoKindSelected`] when no kind with a client has what it needs.
pub fn select(
    alias: ModelAlias,
    configured: Option<ProviderKind>,
    with_a_client: &[ProviderKind],
    holds_key: impl Fn(ProviderKind) -> bool,
    has_endpoint: impl Fn(ProviderKind) -> bool,
) -> Result<ProviderKind, NoKindSelected> {
    // Part 1: an explicit key decides it, and it decides it even for a kind
    // whose requirement does not hold -- because the user saying "use this
    // one" is exactly the case where the requirement was a guess standing in
    // for an answer. A kind named here that this build has no client for is
    // still refused below, since there would be nothing to build.
    if let Some(kind) = configured
        && with_a_client.contains(&kind)
    {
        return Ok(kind);
    }

    // Part 2: the first kind whose requirement holds, in declaration order.
    with_a_client
        .iter()
        .copied()
        .find(|kind| match Requirement::of(*kind) {
            Requirement::HeldKey => holds_key(*kind),
            Requirement::ConfiguredEndpoint => has_endpoint(*kind),
        })
        .ok_or_else(|| NoKindSelected {
            alias,
            with_a_client: with_a_client.to_vec(),
        })
}
