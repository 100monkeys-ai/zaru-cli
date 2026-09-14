// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One stored token: ADR-0007 D2's entry, and the values its fields take.
//!
//! D2's table has eight fields and this module carries all eight. `alias`,
//! `description` and `role` are the user's; `kind` is derived from the secret
//! and is therefore not a field at all but a method on
//! [`Secret`]; `instance` and `secret` come from the server;
//! `tools` and `workspace` are cached.
//!
//! # An entry is what the user supplies; a record is what the store keeps
//!
//! [`Entry`] carries a [`Secret`] and is *consumed* by
//! [`CredentialStore::add`](super::store::CredentialStore::add). What the
//! store then holds and lists is
//! [`Record`](super::store::Record), whose one field a secret is inside is
//! [`Record::sealed`](super::store::Record::sealed) — **and that field's type
//! has no constructor that takes a plaintext**. D2's eight fields are all
//! present across the pair: `secret` is the one that is sealed, and `kind` is
//! derived rather than stored.
//!
//! `role` is on the record and not here, because ADR-0007 D4 is an invariant
//! over the whole store — "Exactly one token is flagged `composer`" — and an
//! invariant over a collection cannot be held by a member of it.

use crate::credentials::alias::Alias;
use crate::credentials::secret::{Kind, Secret};
use crate::providers::ProviderKind;
use core::fmt;
use core::time::Duration;

/// The tools ADR-0006 D4 scopes the composer's credential to.
///
/// Transcribed from that record verbatim rather than assembled from a notion
/// of which tools are reads: D4 names "the `read_only_memory` set —
/// `pages.{list,read}`, `atoms.{list,read}`, `search.global`,
/// `kg.{get_related,list_cross_links}`, `discovery.list_entities` — plus
/// `me.set_current_workspace`, and nothing else", as amended 2026-09-14.
///
/// **Deciding for oneself which tool names are writes would be authoring a
/// security vocabulary**, which [Autonomous Development] puts on the human
/// side of the boundary. Copying a record's list is not.
///
/// **Two of the nine are spelled as the substrate spells them rather than as
/// D4 spelled them, corrected 2026-09-14, and the reason is that D4 is not
/// the authority on these two names.** D4 does not invent a set; it names the
/// members of the `read_only_memory` **preset**, and that preset is
/// [ADR-0135]'s — the server's. So "which spelling is right" is not a
/// judgement about which tools are safe, which would be authoring a security
/// vocabulary; it is a question about what the preset's members are called,
/// and the server answers it. The bytes were read twice, on two dates, by two
/// arcs: a live `tools/list` on 2026-09-06 and the attached tool surface
/// again on 2026-09-14, both spelling `kg.get_related` and
/// `discovery.list_entities`. Four of the six always agreed. **The record was
/// what had drifted and it is corrected there too**, as an amendment to D4
/// open to Jeshua's veto — a list that cannot match any token the substrate
/// can mint is a check holding a spelling instead of a rule.
///
/// **It unblocks nothing today.** No nine-tool token exists — every token
/// measured grants 94 — so the role is refused for a reason this correction
/// does not touch. It is here so that the day such a token is minted the
/// grant accepts it rather than failing on a name.
///
/// [ADR-0135]: https://cortex.page/adrs/p/0135-mcp-token-tool-scope-presets
/// [Autonomous Development]: https://100monkeys-ai.cortex.page/zaru/p/operations/autonomous-development
pub const COMPOSER_SCOPE: [&str; 9] = [
    "pages.list",
    "pages.read",
    "atoms.list",
    "atoms.read",
    "search.global",
    "kg.get_related",
    "kg.list_cross_links",
    "discovery.list_entities",
    "me.set_current_workspace",
];

/// The single role ADR-0007 D2 admits.
///
/// One variant, not a bool, because D2 calls the field `role` with the value
/// `composer` "or unset", and because a second role is an amendment to that
/// record rather than a new arm somebody adds in passing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The token ADR-0005's composer searches with, and ADR-0006 D2 says only
    /// the user may move.
    Composer,
}

impl Role {
    /// The role's name as ADR-0007 D2 spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Composer => "composer",
        }
    }
}

/// A Nuclear Notes instance a token authenticates against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance(String);

impl Instance {
    /// Take an instance host.
    #[must_use]
    pub fn new(host: impl Into<String>) -> Self {
        Self(host.into())
    }

    /// The host.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Instance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How far a token reaches: ADR-0007 D8's instance boundary, or its absence.
///
/// D8: "Every token the harness mints or stores is **instance-locked** unless
/// the user explicitly chooses otherwise", and an apex token "has no instance
/// boundary" — it "matches every instance the bearer can reach through
/// workspace membership".
///
/// Modelled as two variants rather than as an `Option<Instance>` plus a flag,
/// so that an apex entry cannot also be carrying a stale instance nobody
/// notices, and so that every consumer has to name the apex case to compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// The default. The token authenticates against exactly this instance.
    InstanceLocked(Instance),
    /// No instance boundary. Marked wherever the token appears, per D8.
    Apex,
}

impl Reach {
    /// How an apex token is marked, everywhere it is marked.
    ///
    /// One constant, read by all three of D8's surfaces, so that a token
    /// marked in a listing and unmarked in the description the agent reads
    /// is not a state this code can reach.
    pub const APEX_MARKING: &'static str = "apex (no instance boundary)";

    /// Whether this reach is the apex case D8 requires marking.
    #[must_use]
    pub const fn is_apex(&self) -> bool {
        matches!(self, Self::Apex)
    }

    /// How D8 requires this reach be shown, wherever the token appears.
    ///
    /// D8: "Apex entries are **marked wherever the token appears**:
    /// `/notes tokens`, the status line when the composer holds one, and the
    /// description the agent reads." One rendering, called from all three, so
    /// the three cannot drift apart.
    #[must_use]
    pub fn marking(&self) -> String {
        match self {
            Self::InstanceLocked(instance) => instance.to_string(),
            Self::Apex => Self::APEX_MARKING.to_owned(),
        }
    }
}

/// A time-to-live the caller chose.
///
/// ADR-0007 D6 names a TTL as "a backstop for a missed notification" and
/// gives no number. A number invented by the thing it bounds is not a choice
/// anybody made — the same reasoning `zaru-core`'s `Ceiling` and
/// `TruncationBudget` carry — so it arrives as a parameter and is validated
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ttl(Duration);

/// A time-to-live the store cannot work with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TtlRefused;

impl fmt::Display for TtlRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a cache time-to-live of zero is refused; the TTL is a backstop for a \
             missed list_changed notification, and a backstop that has already expired when it \
             is written is indistinguishable from having no cache at all",
        )
    }
}

impl std::error::Error for TtlRefused {}

impl Ttl {
    /// Take a time-to-live from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`TtlRefused`] when `window` is zero.
    pub const fn new(window: Duration) -> Result<Self, TtlRefused> {
        if window.is_zero() {
            return Err(TtlRefused);
        }
        Ok(Self(window))
    }

    /// The window.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// The tool names a token grants, as `tools/list` reported them.
///
/// ADR-0007 D6: "The harness calls `tools/list` once per token at attach and
/// caches the result in the entry. The three-gate enforcement in ADR-0135
/// means that response already reflects exactly what the token grants, so the
/// cache needs no interpretation."
///
/// **A scope reaches here from one `tools/list` and nothing interprets it on
/// the way.** [`CredentialStore::cache_tool_scope`] takes D6's reading at
/// attach and [`CredentialStore::refresh_tool_scope`] replaces it on each of
/// D6's three signals. The two halves of that contract live in two crates
/// that may not depend on each other: `zaru-notes` produces the signals and
/// this crate owns the cache, because ADR-0003 D8 permits that crate no
/// dependency on this one. See [`crate::credentials::notes`].
///
/// [`CredentialStore::cache_tool_scope`]: super::store::CredentialStore::cache_tool_scope
/// [`CredentialStore::refresh_tool_scope`]: super::store::CredentialStore::refresh_tool_scope
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolScope {
    names: Vec<String>,
}

impl ToolScope {
    /// Take a cached scope.
    #[must_use]
    pub fn new<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// The tool names, in the order the server reported them.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// How many tools this token grants. What ADR-0007 D7's listing shows.
    #[must_use]
    pub fn count(&self) -> usize {
        self.names.len()
    }

    /// The first tool here that ADR-0006 D4 does not put in the composer's
    /// scope, if any.
    ///
    /// This is the local half of ADR-0005's trigger clause 9 — "The
    /// composer's own credential cannot write". The harness cannot verify
    /// what the server granted; ADR-0135's three gates do that, server-side.
    /// What it can do is refuse to *use* as the composer a token whose own
    /// cached scope reaches outside D4's set, and name the tool that put it
    /// there.
    #[must_use]
    pub fn outside_composer_scope(&self) -> Option<&str> {
        self.names
            .iter()
            .find(|name| !COMPOSER_SCOPE.contains(&name.as_str()))
            .map(String::as_str)
    }
}

/// A one-line description of what a token is for.
///
/// ADR-0007 D2: user-supplied, "Shown to the human **and** to the agent". D3
/// adds that descriptions "are human-authored and are rendered to the agent
/// as **data, not instruction**".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(String);

/// The store would not take a description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionRefused {
    /// The description as it was offered, escaped.
    pub offered: String,
}

impl fmt::Display for DescriptionRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            // ADR-0007 D2 calls it "One line on what this token is for"; D7 renders it.
            "the description {:?} carries a control character; it is one line on what this token \
             is for, and it is rendered into a terminal listing where a control character can \
             overwrite a neighbouring row",
            self.offered
        )
    }
}

impl std::error::Error for DescriptionRefused {}

impl Description {
    /// Take a description, refusing one that is not a single renderable line.
    ///
    /// # Errors
    ///
    /// [`DescriptionRefused`] when the text carries a control character,
    /// which includes the newline that would make it more than one line.
    pub fn new(offered: impl Into<String>) -> Result<Self, DescriptionRefused> {
        let offered = offered.into();
        if offered.chars().any(char::is_control) {
            return Err(DescriptionRefused {
                offered: offered.escape_debug().to_string(),
            });
        }
        Ok(Self(offered))
    }

    /// The description.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A credential's family and everything that belongs to only that family.
///
/// # This is where "a provider record cannot carry a reach" is held
///
/// [ADR-0007] D2's entry was Nuclear Notes' in every field — `instance`,
/// `tools`, `workspace`, a `kind` ADR-0161 discriminates by prefix — and the
/// store was closed to provider keys for exactly that reason. When the open
/// question closed in favour of one store on 2026-09-05, the alternative to
/// this enum was a flat entry with four fields that are meaningless for half
/// the credentials in it: an `Option<Reach>` nobody reads for a provider key,
/// an empty `ToolScope`, a `workspace` that is always `None`.
///
/// A field that is meaningless for a variant is a field something eventually
/// reads for that variant. Here a provider entry **has no reach to read** and
/// a Notes entry **has no provider kind to read**, so neither confusion is
/// expressible rather than merely being avoided — the argument
/// [`ProviderEndpoint`](crate::providers::ProviderEndpoint) makes about its
/// own missing variant, in the other direction.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[derive(Debug, Clone)]
pub enum Held {
    /// A Nuclear Notes token: [ADR-0007] D8's reach, D6's cached scope and
    /// D2's informational workspace pointer.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    Notes {
        /// D8's instance boundary, or its absence.
        reach: Reach,
        /// D6's cached tool names.
        tools: ToolScope,
        /// D2's `workspace`, "informational only".
        workspace: Option<String>,
    },
    /// A model provider's API key, for one of [ADR-0012] D3's kinds.
    ///
    /// One key per kind. The alias is `provider.<kind>` and the kind is here,
    /// so the two cannot disagree.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Provider {
        /// Which provider this key authenticates against.
        kind: ProviderKind,
    },
}

/// An entry was offered whose secret and whose family disagree.
///
/// **Carries the alias and two family names, and never a value.** The alias
/// is the user's own word and is what they need in order to act; the value is
/// the thing this whole module exists to keep out of a rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRefused {
    /// The alias the entry was offered under.
    pub alias: Alias,
    /// The family the constructor builds.
    pub wanted: &'static str,
    /// The family the secret actually belongs to.
    pub found: &'static str,
}

impl fmt::Display for EntryRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the entry `{}` was built as a {} credential from a {} secret. An entry holds two \
             families and the halves are not interchangeable: a \
             Nuclear Notes token has an instance and a tool scope, and a provider key has a \
             provider kind, and neither has the other's",
            self.alias, self.wanted, self.found,
        )
    }
}

impl std::error::Error for EntryRefused {}

/// One credential as the user supplies it, secret and all.
///
/// The fields are private and the accessors are read-only, so a caller
/// holding an entry cannot step around the store's invariants. An entry
/// carries no role: only the store grants one.
#[derive(Debug, Clone)]
pub struct Entry {
    alias: Alias,
    description: Description,
    secret: Secret,
    held: Held,
}

impl Entry {
    /// Take a Nuclear Notes token. Its role is unset; only the store grants
    /// one.
    ///
    /// # Errors
    ///
    /// [`EntryRefused`] when the secret is a provider key. The two
    /// constructors read the family off the secret rather than taking it as
    /// an argument, so an entry whose stored kind disagrees with its stored
    /// value cannot be built.
    pub fn notes(
        alias: Alias,
        description: Description,
        secret: Secret,
        reach: Reach,
    ) -> Result<Self, EntryRefused> {
        if !secret.kind().is_notes() {
            return Err(EntryRefused {
                alias,
                wanted: "Nuclear Notes",
                found: "provider",
            });
        }
        Ok(Self {
            alias,
            description,
            secret,
            held: Held::Notes {
                reach,
                tools: ToolScope::default(),
                workspace: None,
            },
        })
    }

    /// Take a model provider's API key.
    ///
    /// **The kind is not a parameter.** It is read off the secret, which was
    /// built under a kind the user named, so the entry's kind and the value's
    /// kind are one fact rather than two that can drift.
    ///
    /// # Errors
    ///
    /// [`EntryRefused`] when the secret is a Nuclear Notes token.
    pub fn provider(
        alias: Alias,
        description: Description,
        secret: Secret,
    ) -> Result<Self, EntryRefused> {
        let Kind::Provider(kind) = secret.kind() else {
            return Err(EntryRefused {
                alias,
                wanted: "provider",
                found: "Nuclear Notes",
            });
        };
        Ok(Self {
            alias,
            description,
            secret,
            held: Held::Provider { kind },
        })
    }

    /// Attach the cached tool scope D6 describes.
    ///
    /// # Panics
    ///
    /// Never through a reachable path: a tool scope belongs to the Notes half
    /// and [`Entry::notes`] is the only constructor that produces one, so a
    /// provider entry cannot be the receiver of a builder that only the Notes
    /// constructor's result exposes in practice. It is an `expect` rather
    /// than a silent no-op because a builder that quietly discarded a scope
    /// is how a token ends up in the store with less scope than the caller
    /// believed it had.
    #[must_use]
    pub fn with_tools(mut self, scope: ToolScope) -> Self {
        match &mut self.held {
            Held::Notes { tools, .. } => *tools = scope,
            Held::Provider { .. } => {
                panic!(
                    "a provider key has no cached tool scope; ADR-0007 D6's cache is a Notes \
                        token's `tools/list` and a provider serves no MCP tools"
                )
            }
        }
        self
    }

    /// Attach the last-known workspace pointer, which D2 calls informational.
    ///
    /// # Panics
    ///
    /// Never through a reachable path, for the reason [`Entry::with_tools`]
    /// gives.
    #[must_use]
    pub fn with_workspace(mut self, slug: impl Into<String>) -> Self {
        match &mut self.held {
            Held::Notes { workspace, .. } => *workspace = Some(slug.into()),
            Held::Provider { .. } => {
                panic!(
                    "a provider key has no workspace pointer; ADR-0007 D2's `workspace` is a \
                        Nuclear Notes token row's and a provider has no notion of one"
                )
            }
        }
        self
    }

    /// Which family this credential belongs to, and what belongs to it.
    #[must_use]
    pub const fn held(&self) -> &Held {
        &self.held
    }

    /// This token's local name.
    #[must_use]
    pub const fn alias(&self) -> &Alias {
        &self.alias
    }

    /// What this token is for.
    #[must_use]
    pub const fn description(&self) -> &Description {
        &self.description
    }

    /// The bearer value. Never rendered; see [`Secret`].
    #[must_use]
    pub const fn secret(&self) -> &Secret {
        &self.secret
    }

    /// Whether this token is instance-locked or apex, for a Notes token.
    ///
    /// `None` for a provider key, which has no instance boundary to have.
    #[must_use]
    pub const fn reach(&self) -> Option<&Reach> {
        match &self.held {
            Held::Notes { reach, .. } => Some(reach),
            Held::Provider { .. } => None,
        }
    }

    /// The cached tool scope, for a Notes token.
    #[must_use]
    pub const fn tools(&self) -> Option<&ToolScope> {
        match &self.held {
            Held::Notes { tools, .. } => Some(tools),
            Held::Provider { .. } => None,
        }
    }

    /// The last-known workspace pointer, which the token row overrides.
    #[must_use]
    pub fn workspace(&self) -> Option<&str> {
        match &self.held {
            Held::Notes { workspace, .. } => workspace.as_deref(),
            Held::Provider { .. } => None,
        }
    }
}
