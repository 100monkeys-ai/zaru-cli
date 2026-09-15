// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which keys exist, what shape each holds, and what a project may do to it.
//!
//! # The schema is an input, and that is what ADR-0014 says
//!
//! The record's Neutral section: "**Nothing here specifies the schema. Each
//! record owns its own keys; this one owns how they resolve.**" D5 then
//! requires that "Unknown keys are an error at load, naming the nearest
//! match", which cannot be done without a known set.
//!
//! Both hold only if the known set arrives from outside. So [`Schema`] is
//! built by a caller and handed in, and this module enumerates no key at all.
//! Enumerating them here would transcribe [ADR-0001] D2's `runtime`,
//! [ADR-0009] D1's `[project]`, `[runtime]` and `[[validator]]`,
//! [ADR-0011] D3's permission mode and [ADR-0012] D1's aliases, fixing five
//! records' spellings in a sixth record's implementation — which is
//! [Agent lessons] §5 and [Verification lessons] §17 at once.
//!
//! # D6's ceiling, as three policies rather than a list of four keys
//!
//! D6: "A project may lower its own iteration ceiling, name its workspace,
//! and declare validators. It may **not** raise the permission mode, disable
//! SEAL, widen a token scope, or move the runtime tier upward."
//!
//! Those four escalations belong to four records, three of which are unbuilt
//! and one of which — [ADR-0011]'s permission mode — is being built by
//! another arc as this is written. So what is here is the *mechanism*: every
//! key declares its own [`ProjectPolicy`], and each of D6's four escalations
//! arrives as one declaration with the record that owns that key. The
//! escalation ceiling is checked over however many keys declare
//! [`ProjectPolicy::Refused`], not over four names typed here.
//!
//! # Nearest match, and what D5 leaves open
//!
//! D5 requires a suggestion and names no metric. [`Schema::nearest`] uses
//! Levenshtein edit distance over the schema's keys with ties broken
//! lexically, which is deterministic and reproducible. **D5 names no distance
//! threshold either**, so a key resembling nothing still receives the nearest
//! suggestion; whether a distant key should get none is on the record as a
//! question rather than answered here.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [Agent lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/agent-lessons
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::config::credential::CredentialRef;
use crate::config::key::Key;
use crate::config::value::Value;
use crate::credentials::{Alias, AliasRefused};
use std::collections::BTreeMap;

/// What shape a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// A boolean.
    Bool,
    /// A whole number.
    Integer,
    /// Free text.
    Text,
    /// A list, which ADR-0014 D2 replaces wholesale.
    Array,
    /// A nested table, which D2 merges by key.
    Table,
    /// The name of a credential the store holds, per ADR-0014 D4. **Never a
    /// bearer value** — see [`CredentialRef`].
    CredentialAlias,
}

impl FieldKind {
    /// What this kind is called in a refusal.
    #[must_use]
    pub const fn shape(self) -> &'static str {
        match self {
            Self::Bool => "a boolean",
            Self::Integer => "a whole number",
            Self::Text => "text",
            Self::Array => "a list",
            Self::Table => "a table",
            Self::CredentialAlias => "a credential reference",
        }
    }

    /// Bring an offered value to this kind.
    ///
    /// Layer 4 supplies text whatever a file supplies, so every kind that can
    /// be written as text accepts text as well as its own shape. **No kind
    /// parses text into a list**: no record says how an environment variable
    /// expresses one, and inventing a separator here would settle that.
    ///
    /// # Errors
    ///
    /// [`CoercionFailure`], which the caller wraps with the key and the layer
    /// it came from.
    pub fn coerce(self, offered: Value) -> Result<Value, CoercionFailure> {
        match (self, offered) {
            (Self::Bool, value @ Value::Bool(_))
            | (Self::Integer, value @ Value::Integer(_))
            | (Self::Text, value @ Value::Text(_))
            | (Self::Array, value @ Value::Array(_))
            | (Self::Table, value @ Value::Table(_))
            | (Self::CredentialAlias, value @ Value::Credential(_)) => Ok(value),

            (Self::Bool, Value::Text(text)) => text
                .parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| CoercionFailure::Unparsable),
            (Self::Integer, Value::Text(text)) => text
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| CoercionFailure::Unparsable),
            (Self::CredentialAlias, Value::Text(text)) => Alias::new(&text)
                .map(|alias| Value::Credential(CredentialRef::new(alias)))
                .map_err(CoercionFailure::Alias),

            (_, offered) => Err(CoercionFailure::WrongShape {
                found: offered.shape(),
            }),
        }
    }
}

/// Why a value could not be brought to a field's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoercionFailure {
    /// The value's shape is not this kind's and no text conversion applies.
    WrongShape {
        /// What shape the value had. **Never the value itself.**
        found: &'static str,
    },
    /// The value was text but does not read as this kind.
    Unparsable,
    /// The value was text but is not a usable alias.
    Alias(AliasRefused),
}

/// What ADR-0014 D6 lets the project layer do to a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectPolicy {
    /// D6's permitted list — "name its workspace, and declare validators".
    Free,
    /// D6's "may lower its own iteration ceiling".
    ///
    /// Defined on whole numbers, because the only key D6 permits with a
    /// direction is a ceiling. The tiers and permission modes D6 names are
    /// [`ProjectPolicy::Refused`] outright, so no ordering has to be invented
    /// for them — and inventing one would settle D6's unstated presumption
    /// that a runtime tier can move "upward", which is a statement about
    /// reach rather than about safety, since [ADR-0001]'s `bare` has no
    /// membrane at all.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    LowerOnly,
    /// D6's four escalations — "may not raise the permission mode, disable
    /// SEAL, widen a token scope, or move the runtime tier upward".
    Refused {
        /// Why, in words a user can act on. D6 says the error names "the key
        /// and the reason", and [ADR-0016] D2 says an error whose reader
        /// cannot act "is a stack trace with better grammar".
        ///
        /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
        reason: String,
    },
}

/// One key's declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// What shape it holds.
    pub kind: FieldKind,
    /// What the project layer may do to it.
    pub project: ProjectPolicy,
}

impl Field {
    /// A key the project layer may set freely.
    #[must_use]
    pub fn free(kind: FieldKind) -> Self {
        Self {
            kind,
            project: ProjectPolicy::Free,
        }
    }

    /// A whole number the project layer may only lower.
    #[must_use]
    pub fn ceiling() -> Self {
        Self {
            kind: FieldKind::Integer,
            project: ProjectPolicy::LowerOnly,
        }
    }

    /// One of D6's escalations: a key the project layer may not set at all.
    #[must_use]
    pub fn refused_to_projects(kind: FieldKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            project: ProjectPolicy::Refused {
                reason: reason.into(),
            },
        }
    }
}

/// The known keys and what each holds.
///
/// Built by a caller. This module declares none of them — see the module
/// documentation for why that is the record's own instruction rather than an
/// omission.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Schema {
    fields: BTreeMap<Key, Field>,
    families: Vec<KeyFamily>,
}

/// A family of keys sharing one shape, distinguished by a middle segment.
///
/// # Why the schema needs one at all
///
/// Every key declared here is a name somebody typed into this binary. That
/// works while the set is closed — [ADR-0001] D2's tier, [ADR-0011] D3's mode,
/// [ADR-0012] D3's five kinds — and it cannot express a key whose middle
/// segment is **a name the user chose**. [ADR-0007](https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store)
/// D5's per-token grant is the first of those: an alias is local metadata that
/// Nuclear Notes has no notion of, so the set of `notes.<alias>.agent_tools`
/// keys is whatever the store happens to hold, and it changes when a person
/// runs `notes tokens add`.
///
/// The alternative was to read the credential store while building the schema,
/// which would make configuration resolution depend on a store that can fail
/// to open, on a keyring, and on a sealing key — for a schema whose whole job
/// is to say which *names* exist. A family says that without knowing any
/// aliases.
///
/// **A family matches exactly three segments** and never fewer or more, so
/// `notes.agent_tools` and `notes.a.b.agent_tools` are both unknown keys
/// rather than near misses. The middle segment is not validated as an alias
/// here: an alias nothing is stored under resolves to a value nothing reads,
/// which is a setting with no effect rather than an error, and refusing it
/// would make configuration refuse to load because a token was removed.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyFamily {
    prefix: String,
    suffix: String,
    field: Field,
}

impl KeyFamily {
    /// The first segment every member shares.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The last segment every member shares.
    #[must_use]
    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// What every member holds.
    #[must_use]
    pub const fn field(&self) -> &Field {
        &self.field
    }

    /// The middle segment of a key in this family, if it is one.
    #[must_use]
    pub fn member<'k>(&self, key: &'k Key) -> Option<&'k str> {
        let mut segments = key.segments();
        let first = segments.next()?;
        let middle = segments.next()?;
        let last = segments.next()?;
        if segments.next().is_some() || first != self.prefix || last != self.suffix {
            return None;
        }
        Some(middle)
    }

    /// How a member of this family is spelled, for a person to read.
    ///
    /// The placeholder is angle-bracketed the way this workspace's records
    /// spell one, so a listing shows the shape rather than an example that
    /// could be mistaken for a stored alias.
    #[must_use]
    pub fn shape(&self) -> String {
        format!("{}.<alias>.{}", self.prefix, self.suffix)
    }
}

impl Schema {
    /// A schema with no keys, which refuses everything.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a key.
    #[must_use]
    pub fn with(mut self, key: Key, field: Field) -> Self {
        self.fields.insert(key, field);
        self
    }

    /// Whether anything is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// How many keys are declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Declare a family of keys distinguished by a middle segment.
    ///
    /// See [`KeyFamily`] for why the schema has one.
    #[must_use]
    pub fn with_family(
        mut self,
        prefix: impl Into<String>,
        suffix: impl Into<String>,
        field: Field,
    ) -> Self {
        self.families.push(KeyFamily {
            prefix: prefix.into(),
            suffix: suffix.into(),
            field,
        });
        self
    }

    /// Every declared family.
    pub fn families(&self) -> impl Iterator<Item = &KeyFamily> {
        self.families.iter()
    }

    /// What a key holds, if it is declared.
    ///
    /// An exact declaration wins over a family, so a key named outright is
    /// never shadowed by one whose shape it happens to match.
    #[must_use]
    pub fn field(&self, key: &Key) -> Option<&Field> {
        self.fields.get(key).or_else(|| {
            self.families
                .iter()
                .find(|family| family.member(key).is_some())
                .map(KeyFamily::field)
        })
    }

    /// Every declared key, in lexical order.
    pub fn keys(&self) -> impl Iterator<Item = &Key> {
        self.fields.keys()
    }

    /// The declared key nearest to one that is not declared.
    ///
    /// The metric is `config::nearest::nearest`, which this crate's three
    /// vocabularies share — see that module for why it is one function.
    /// **Ties are broken lexically**: [`Schema::keys`] iterates a `BTreeMap`
    /// in lexical order and that function takes the first of several equally
    /// near candidates, so the answer does not depend on insertion order.
    ///
    /// Returns `None` only for an empty schema. D5 names no distance
    /// threshold, so a key resembling nothing still gets the nearest one.
    #[must_use]
    pub fn nearest(&self, offered: &str) -> Option<&Key> {
        let found = crate::config::nearest::nearest(self.keys().map(Key::as_str), offered)?;
        self.fields.keys().find(|key| key.as_str() == found)
    }

    /// The nearest declared spelling to one that is not declared, families
    /// included.
    ///
    /// A family cannot be enumerated — its members are whatever a person has
    /// stored — so what a suggestion can offer is its *shape*. That is the
    /// honest answer to a mistyped `notes.work.agenttools`: the nearest thing
    /// that exists is `notes.<alias>.agent_tools`, and naming a stored alias
    /// instead would suggest a key the schema does not actually declare.
    #[must_use]
    pub fn nearest_spelling(&self, offered: &str) -> Option<String> {
        let shapes: Vec<String> = self.families.iter().map(KeyFamily::shape).collect();
        let candidates = self
            .keys()
            .map(|key| key.as_str().to_owned())
            .chain(shapes.iter().cloned())
            .collect::<Vec<_>>();
        crate::config::nearest::nearest(candidates.iter().map(String::as_str), offered)
            .map(str::to_owned)
    }
}
