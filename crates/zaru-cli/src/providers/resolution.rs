// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The resolution table: the one place a model identifier may exist, and the
//! only reading of which layer supplied it.
//!
//! # D1 is a private field, not a rule anybody remembers
//!
//! [ADR-0012] D1: "What each resolves to is configuration, and it is resolved
//! at one place. **A model identifier appearing anywhere except the resolution
//! table is a bug.**"
//!
//! [`ModelId`] is declared *in this module* with a private field and no public
//! constructor, so no other module in this crate — and nothing outside it —
//! can build one. That is not a lint and not a convention: a model identifier
//! constructed anywhere else does not compile. It is the same argument
//! [`CredentialRef`](crate::config::CredentialRef) makes for D4 of ADR-0014,
//! applied to a different kind of value.
//!
//! # D4 reads ADR-0014's explanation, and never writes a second one
//!
//! D4: "`zaru models` prints each alias, what it resolved to, and **which
//! layer supplied it**." That is [ADR-0014] D3's question, already answered by
//! [`Resolution::explain`], which returns a row per layer and marks the
//! highest one that *set* the key. So [`ModelTable::from_configuration`] makes
//! **one** call to `explain` per alias and takes both the value and the layer
//! off that one [`Explanation`]. There is no second traversal of layers
//! anywhere in this module, and no arithmetic over precedence: a table whose
//! layer disagreed with `config explain`'s block would be two explanations of
//! one fact, which is the shape [`Layer`]'s own de-duplication of 2026-09-04
//! removed from this workspace.
//!
//! # This record owns these keys, which is ADR-0014's instruction
//!
//! ADR-0014's Neutral section: "**Nothing here specifies the schema. Each
//! record owns its own keys; this one owns how they resolve.**" [`fields`] is
//! ADR-0012 owning its four alias keys and its four endpoint keys, handed to
//! whatever builds a [`Schema`]. This is the first module in the workspace to
//! declare one, and it is that instruction being followed rather than departed
//! from.
//!
//! # What the project layer may and may not set
//!
//! `model.<alias>` is **free at every layer**, because D4 lists project
//! configuration as one of the five that resolve an alias.
//!
//! `provider.<kind>.endpoint` is **refused to the project layer**, under a
//! delegated coordinator ruling of 2026-09-05 open to Jeshua's veto. ADR-0014
//! D6 names four escalations and an endpoint is not among them, but where a
//! user's prompts are sent is a security posture in exactly D6's sense — "a
//! repository the user cloned must not be able to configure its way to more
//! privilege than the user granted" — and a project silently redirecting a
//! user's traffic is the strongest form of that. **The refusal is a proposed
//! fifth escalation on ADR-0014 D6 and is settled by no record**; refusing is
//! the direction to be wrong in, and it costs [ADR-0012] D5 nothing because it
//! refuses identically for all four kinds.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::config::{Explanation, Field, FieldKind, Key, Layer, Resolution, Schema, Value};
use crate::providers::alias::ModelAlias;
use crate::providers::endpoint::{EndpointRefused, ProviderEndpoint};
use crate::providers::kind::ProviderKind;
use core::fmt;

/// Why a model identifier was not taken.
///
/// A model identifier is not a credential — the fold refuses a
/// credential-shaped value long before one could reach here — so a refusal
/// quotes it back, the way [`AliasRefused`](crate::credentials::AliasRefused)
/// quotes an alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelIdRefused {
    /// The identifier was empty.
    Empty,
    /// The identifier carried a control character.
    Control {
        /// The identifier as it was configured, escaped.
        offered: String,
    },
    /// The identifier began or ended with whitespace.
    SurroundingWhitespace {
        /// The identifier as it was configured.
        offered: String,
    },
}

impl fmt::Display for ModelIdRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "a model identifier is empty; ADR-0012 D4 has `zaru models` print what each alias \
                 resolved to, and an empty identifier resolves to nothing a provider could be \
                 asked for",
            ),
            Self::Control { offered } => write!(
                f,
                "the model identifier {offered:?} carries a control character; it is rendered into \
                 a terminal listing, where one can erase or overwrite a neighbouring row",
            ),
            Self::SurroundingWhitespace { offered } => write!(
                f,
                "the model identifier {offered:?} begins or ends with whitespace; two identifiers \
                 differing only there are one identifier to every reader of that listing",
            ),
        }
    }
}

impl std::error::Error for ModelIdRefused {}

/// What an alias resolved to.
///
/// **Constructible only inside this module.** The field is private and there
/// is no public constructor, which is [ADR-0012] D1 as a compile error rather
/// than as a rule — see the module documentation.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelId(String);

impl ModelId {
    /// Take a model identifier out of a resolved configuration value.
    ///
    /// Private on purpose: this is the resolution table, and D1 says a model
    /// identifier existing anywhere else is a bug.
    fn new(offered: &str) -> Result<Self, ModelIdRefused> {
        if offered.is_empty() {
            return Err(ModelIdRefused::Empty);
        }
        if offered.chars().any(char::is_control) {
            return Err(ModelIdRefused::Control {
                offered: offered.escape_debug().to_string(),
            });
        }
        if offered.trim() != offered {
            return Err(ModelIdRefused::SurroundingWhitespace {
                offered: offered.to_owned(),
            });
        }
        Ok(Self(offered.to_owned()))
    }

    /// The identifier as configuration resolved it.
    ///
    /// Reading one is ordinary; *building* one is what D1 forbids outside this
    /// module, and the private field is what stops it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What one alias resolved to, and which of [ADR-0014] D1's layers said so.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedModel {
    /// Some layer set it, and this is which.
    Resolved {
        /// What the alias resolved to.
        model: ModelId,
        /// The highest layer that **set** the key, read straight off
        /// ADR-0014 D3's own explanation.
        supplied_by: Layer,
    },
    /// No layer set it.
    ///
    /// **A value rather than a refusal, and no default is invented.**
    /// [ADR-0012]'s Neutral consequence is exactly one sentence: "Nothing here
    /// selects a default model. That is configuration and it changes as models
    /// do." So an alias nobody configured resolves to nothing, `zaru models`
    /// would render it the way ADR-0014 D3's block renders `(not set)`, and
    /// what a *caller* does when it needs one is that caller's problem — which
    /// is where [ADR-0016] D2's "no credential for alias" line lives.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    Unresolved,
}

/// Why a resolution could not be read as ADR-0012's table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableRefused {
    /// A key resolved to something that is not text.
    ///
    /// **Ours rather than the user's.** [`fields`] declares every one of these
    /// keys as [`FieldKind::Text`], and the fold coerces to a key's declared
    /// kind, so a non-text value here means the caller built a [`Schema`]
    /// declaring this record's key as something else.
    NotText {
        /// Which alias.
        alias: ModelAlias,
        /// What shape the value had. **Never the value itself.**
        found: &'static str,
    },
    /// A key resolved to a value the listing could not render.
    UnusableModelId {
        /// Which alias.
        alias: ModelAlias,
        /// Why.
        refusal: ModelIdRefused,
    },
    /// A key resolved to an endpoint the listing could not render.
    UnusableEndpoint {
        /// Which kind.
        kind: ProviderKind,
        /// Why.
        refusal: EndpointRefused,
    },
    /// A key had a value and no layer that set it.
    ///
    /// **Ours, and unreachable through [`Resolution::explain`]**, which marks
    /// the first row carrying a value. It is a variant rather than a panic
    /// because a resolution table that cannot say where a value came from is
    /// exactly the thing D4 exists to prevent, and reporting it is cheaper
    /// than a process that stops.
    NoSupplyingLayer {
        /// Which key.
        key: Key,
    },
}

impl fmt::Display for TableRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotText { alias, found } => write!(
                f,
                "the alias `{alias}` resolved to {found}; ADR-0012's own schema declares \
                 `{key}` as text, so a schema declaring it as something else is this harness's \
                 defect rather than a user's",
                key = alias.key(),
            ),
            Self::UnusableModelId { alias, refusal } => {
                write!(f, "the alias `{alias}` resolved to a model, and {refusal}")
            }
            Self::UnusableEndpoint { kind, refusal } => {
                write!(f, "the provider `{kind}` names an endpoint, and {refusal}")
            }
            Self::NoSupplyingLayer { key } => write!(
                f,
                "`{key}` has a value and no layer that set it; ADR-0012 D4 requires that every \
                 resolution name the layer that supplied it, and this one cannot",
            ),
        }
    }
}

impl std::error::Error for TableRefused {}

/// The four keys ADR-0012 owns for its aliases and the four for its endpoints.
///
/// Handed to whatever builds a [`Schema`]; this record owns these keys, and
/// [ADR-0014]'s own Neutral section is why they arrive from here rather than
/// being written into the hierarchy.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn fields() -> Vec<(Key, Field)> {
    let mut declared: Vec<(Key, Field)> = ModelAlias::ALL
        .into_iter()
        // Free at every layer: ADR-0012 D4 names project configuration as one
        // of the five layers that resolve an alias, so a project asking for a
        // different model is the mechanism working.
        .map(|alias| (alias.key(), Field::free(FieldKind::Text)))
        .collect();

    declared.extend(ProviderKind::ALL.into_iter().map(|kind| {
        (
            kind.endpoint_key(),
            Field::refused_to_projects(
                FieldKind::Text,
                "where a user's prompts are sent is the user's choice, and a repository they \
                 cloned must not be able to redirect them. ADR-0014 D6 names four escalations and \
                 this is proposed as a fifth",
            ),
        )
    }));

    declared
}

/// Declare ADR-0012's keys into a caller's schema.
///
/// Built on [`fields`] rather than repeating it, so there is one list.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    fields()
        .into_iter()
        .fold(schema, |schema, (key, field)| schema.with(key, field))
}

/// [ADR-0012] D4's answer, for all four aliases at once.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTable {
    rows: Vec<(ModelAlias, ResolvedModel)>,
}

impl ModelTable {
    /// Read every alias out of a resolved configuration.
    ///
    /// One [`Resolution::explain`] call per alias, and both the value and the
    /// supplying layer come off that one call — see the module documentation
    /// for why a second reading would be a second explanation.
    ///
    /// # Errors
    ///
    /// [`TableRefused`], naming the alias.
    pub fn from_configuration(resolution: &Resolution) -> Result<Self, TableRefused> {
        let mut rows = Vec::with_capacity(ModelAlias::ALL.len());
        for alias in ModelAlias::ALL {
            let key = alias.key();
            let row = match supplied(resolution, alias, &key)? {
                None => ResolvedModel::Unresolved,
                Some((text, supplied_by)) => ResolvedModel::Resolved {
                    model: ModelId::new(&text)
                        .map_err(|refusal| TableRefused::UnusableModelId { alias, refusal })?,
                    supplied_by,
                },
            };
            rows.push((alias, row));
        }
        Ok(Self { rows })
    }

    /// What one alias resolved to.
    #[must_use]
    pub fn row(&self, alias: ModelAlias) -> &ResolvedModel {
        &self
            .rows
            .iter()
            .find(|(candidate, _)| *candidate == alias)
            .expect("every alias in ModelAlias::ALL has a row")
            .1
    }

    /// Every alias, in [ADR-0012] D2's own order.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub fn rows(&self) -> impl Iterator<Item = (ModelAlias, &ResolvedModel)> {
        self.rows.iter().map(|(alias, row)| (*alias, row))
    }
}

/// The endpoint one provider kind was configured with, if any layer set it.
///
/// Read through the same one function [`ModelTable::from_configuration`] uses,
/// so there is one path from a resolved configuration to a value here. The
/// supplying layer is discarded rather than carried, because [ADR-0012] D4's
/// "which layer supplied it" is about **alias** resolution and inventing a
/// second explanation for endpoints would be answering a question no record
/// asks.
///
/// # Errors
///
/// [`TableRefused`], naming the kind.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub fn endpoint_of(
    resolution: &Resolution,
    kind: ProviderKind,
) -> Result<Option<ProviderEndpoint>, TableRefused> {
    let key = kind.endpoint_key();
    match supplied_text(resolution, &key) {
        None => Ok(None),
        Some(text) => ProviderEndpoint::new(&text)
            .map(Some)
            .map_err(|refusal| TableRefused::UnusableEndpoint { kind, refusal }),
    }
}

/// The value and the layer that set it, off **one** explanation.
///
/// Both halves come from a single [`Resolution::explain`] call, so the layer
/// this module reports and the layer `config explain` would print are the same
/// reading rather than two. Returns `None` where no layer set the key, which
/// is D3's `(not set)`.
fn supplied(
    resolution: &Resolution,
    alias: ModelAlias,
    key: &Key,
) -> Result<Option<(String, Layer)>, TableRefused> {
    let explanation: Explanation = resolution.explain(key);
    let Some(value) = explanation.value.clone() else {
        return Ok(None);
    };
    let Value::Text(text) = value else {
        return Err(TableRefused::NotText {
            alias,
            found: explanation.value.as_ref().map_or("nothing", Value::shape),
        });
    };
    let Some(supplied_by) = explanation.effective_layer() else {
        return Err(TableRefused::NoSupplyingLayer { key: key.clone() });
    };
    Ok(Some((text, supplied_by)))
}

/// The value alone, for a key whose supplying layer no record asks about.
fn supplied_text(resolution: &Resolution, key: &Key) -> Option<String> {
    match resolution.explain(key).value {
        Some(Value::Text(text)) => Some(text),
        _ => None,
    }
}
