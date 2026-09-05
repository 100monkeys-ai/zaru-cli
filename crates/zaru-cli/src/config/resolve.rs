// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The fold: ADR-0014 D1's precedence, D2's merge, D5's refusal, D6's ceiling.
//!
//! # Order matters, and one ordering is a security property
//!
//! Each layer is validated before it is merged, and within a layer the checks
//! run in this order:
//!
//! 1. **Is the key declared?** D5 — an unknown key is an error at load.
//! 2. **Is the value credential-shaped?** D4 — refused before anything else
//!    touches it.
//! 3. **Bring it to the declared shape.**
//!
//! Step 2 sits before step 3 deliberately. A key declared as a credential
//! reference turns text into an [`Alias`](crate::credentials::Alias), and a
//! bearer value is a perfectly well-formed alias — nothing in ADR-0007's
//! alias rules refuses one. So a bearer value offered to a credential key
//! would become the *name* of a credential, and an alias is rendered into
//! `notes:<alias>` tool names, into D7's listing and into refusals that quote
//! it. Scanning first is what stops that, and
//! `a_bearer_value_offered_to_a_credential_key_is_refused_at_every_layer` is
//! the check that holds it.
//!
//! For the same reason the credential scan has **two** triggers rather than
//! one: the layer's own rule ([`Layer::refuses_credential_shaped_values`],
//! true for the two file layers D4 names) *or* the key being declared as a
//! credential reference, which holds at every layer. A key whose whole
//! meaning is "a name, not a value" refuses a value wherever the value came
//! from, and that is D4 rather than an extension of it.
//!
//! # What decides "credential-shaped"
//!
//! [`Secret::new`](crate::credentials::Secret) — ADR-0007 D2's own
//! discrimination by prefix, asked rather than retyped. There is no second
//! list of prefixes here to drift from the first, and no entropy heuristic:
//! deciding for oneself what a credential looks like is authoring a security
//! vocabulary, which [Autonomous development] puts on the human side of the
//! boundary.
//!
//! # D7's immutability, in the half that exists
//!
//! D7: "the tier is fixed for a session. Configuration participates in
//! resolving it and cannot change it afterwards. A membrane that can be
//! reconfigured mid-session is not one."
//!
//! [`Resolution`] has no method taking `&mut self`, no setter, no `reload`
//! and no interior mutability. Once resolved it can only be read. The session
//! that would hold it is [ADR-0010]'s and is unbuilt, so this is the
//! invariant half: whatever a session turns out to be, it cannot be handed a
//! configuration that changes underneath it.
//!
//! [Autonomous development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::config::explain::{Explanation, ExplanationRow};
use crate::config::key::Key;
use crate::config::layer::{Contribution, Layer, Source};
use crate::config::refusal::ConfigRefused;
use crate::config::schema::{CoercionFailure, FieldKind, ProjectPolicy, Schema};
use crate::config::value::{Table, Value};
use crate::credentials::Secret;

/// What one layer turned out to hold, after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedLayer {
    layer: Layer,
    source: Source,
    document: Table,
}

/// The effective configuration, and where every part of it came from.
///
/// **Read-only by construction** — see the module documentation on D7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    effective: Table,
    layers: Vec<ResolvedLayer>,
}

impl Resolution {
    /// Fold the layers, refusing anything ADR-0014 forbids.
    ///
    /// Contributions may arrive in any order and any layer may be absent; an
    /// absent layer is D3's `(not set)`. A layer offered twice is refused
    /// rather than resolved by taking one, because D1 does not say which of
    /// two documents is layer 3.
    ///
    /// # Errors
    ///
    /// [`ConfigRefused`], naming the layer the offending value arrived in.
    pub fn resolve(
        schema: &Schema,
        contributions: impl IntoIterator<Item = Contribution>,
    ) -> Result<Self, ConfigRefused> {
        let mut offered: Vec<Option<Contribution>> = vec![None; Layer::ALL.len()];
        for contribution in contributions {
            let slot = &mut offered[contribution.layer.index()];
            if slot.is_some() {
                return Err(ConfigRefused::DuplicateLayer {
                    layer: contribution.layer,
                });
            }
            *slot = Some(contribution);
        }

        let mut effective = Table::new();
        let mut layers = Vec::with_capacity(Layer::ALL.len());

        // Lowest first, so `effective` always holds what the layers below
        // this one granted -- which is what D6's ceiling compares against.
        for (index, layer) in Layer::ALL.into_iter().enumerate() {
            let (source, document) = match offered[index].take() {
                Some(contribution) => (contribution.source, contribution.document),
                None => (layer.default_source(), Table::new()),
            };

            let document = validate(schema, layer, document)?;
            if layer.bound_by_the_escalation_ceiling() {
                enforce_the_ceiling(schema, &effective, &document)?;
            }
            effective.merge_over(document.clone());
            layers.push(ResolvedLayer {
                layer,
                source,
                document,
            });
        }

        Ok(Self { effective, layers })
    }

    /// The whole effective configuration.
    #[must_use]
    pub fn effective(&self) -> &Table {
        &self.effective
    }

    /// What one key resolved to.
    #[must_use]
    pub fn get(&self, key: &Key) -> Option<&Value> {
        self.effective.get_path(key)
    }

    /// ADR-0014 D3's explanation for one key.
    ///
    /// Always five rows, highest layer first, including the layers that set
    /// nothing. The marked row is the highest layer that **set** the key,
    /// which is not the same as the highest layer.
    #[must_use]
    pub fn explain(&self, key: &Key) -> Explanation {
        let mut rows: Vec<ExplanationRow> = self
            .layers
            .iter()
            .map(|resolved| ExplanationRow {
                layer: resolved.layer,
                source: resolved.source.shown_for(key),
                value: resolved.document.get_path(key).cloned(),
                effective: false,
            })
            .collect();

        // Highest first, as D3's block prints them.
        rows.reverse();

        let effective_value = rows
            .iter_mut()
            .find(|row| row.value.is_some())
            .and_then(|row| {
                row.effective = true;
                row.value.clone()
            });

        Explanation {
            key: key.clone(),
            value: effective_value,
            rows,
        }
    }
}

/// Check and coerce one layer's document against the schema.
fn validate(schema: &Schema, layer: Layer, document: Table) -> Result<Table, ConfigRefused> {
    let mut checked = Table::new();
    for (key, value) in walk(schema, layer, &document)? {
        let Some(field) = schema.field(&key) else {
            return Err(ConfigRefused::UnknownKey {
                layer,
                offered: key.as_str().to_owned(),
                suggestion: schema
                    .nearest(key.as_str())
                    .map(|nearest| nearest.as_str().to_owned()),
            });
        };

        let declared_as_a_reference = matches!(field.kind, FieldKind::CredentialAlias);
        if (layer.refuses_credential_shaped_values() || declared_as_a_reference)
            && let Some(text) = value.as_text()
            // Notes-shaped only, and deliberately. `Secret::provider` admits
            // any text that is not empty, has no control character and does
            // not begin or end with whitespace -- which is nearly every
            // configuration value there is -- so asking it here would refuse
            // the whole file. ADR-0014 D4's protection against a provider key
            // reaching configuration is that no field holds one: the store is
            // where a provider key goes, and `provider.<kind>.endpoint` is an
            // endpoint. Raised on ADR-0014 rather than approximated here.
            && Secret::notes(text).is_ok()
        {
            return Err(ConfigRefused::CredentialShaped {
                layer,
                key,
                declared_as_a_reference,
            });
        }

        match field.kind.coerce(value.clone()) {
            Ok(value) => checked.insert_path(&key, value),
            Err(CoercionFailure::WrongShape { found }) => {
                return Err(ConfigRefused::WrongShape {
                    layer,
                    key,
                    expected: field.kind.shape(),
                    found,
                });
            }
            Err(CoercionFailure::Unparsable) => {
                return Err(ConfigRefused::UnparsableText {
                    layer,
                    key,
                    expected: field.kind.shape(),
                });
            }
            Err(CoercionFailure::Alias(refusal)) => {
                return Err(ConfigRefused::UnusableAlias {
                    layer,
                    key,
                    refusal,
                });
            }
        }
    }
    Ok(checked)
}

/// ADR-0014 D6, over however many keys declare a policy.
///
/// Both arms are here and both are load-bearing. `Refused` is D6's four
/// escalations; `LowerOnly` is its "may lower its own iteration ceiling",
/// **and the permitted direction is what separates a correct implementation
/// from one that simply refuses everything the project layer offers**. A
/// refuse-always implementation passes every refusal check perfectly, which
/// is [Verification lessons] §13 — an invariant holding because both sides
/// are wrong together.
///
/// A ceiling the layers below never granted is not raised by being set: there
/// is nothing to exceed. In practice layer 1 always carries a default, per
/// [ADR-0001] D3.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
fn enforce_the_ceiling(
    schema: &Schema,
    granted: &Table,
    project: &Table,
) -> Result<(), ConfigRefused> {
    for (key, value) in walk(schema, Layer::Project, project)? {
        let Some(field) = schema.field(&key) else {
            continue;
        };
        match &field.project {
            ProjectPolicy::Free => {}
            ProjectPolicy::Refused { reason } => {
                return Err(ConfigRefused::ProjectMayNotSet {
                    key,
                    reason: reason.clone(),
                });
            }
            ProjectPolicy::LowerOnly => {
                let Some(asked) = value.as_integer() else {
                    continue;
                };
                if let Some(granted) = granted.get_path(&key).and_then(Value::as_integer)
                    && asked > granted
                {
                    return Err(ConfigRefused::ProjectMayNotRaise {
                        key,
                        granted,
                        asked,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Every value in a document that a key addresses.
///
/// Descends through tables until it reaches something the schema declares or
/// something that is not a populated table. **A declared key stops the
/// descent**, so a key declared as holding a table takes its whole subtree
/// rather than having every branch of it reported as unknown. An *undeclared*
/// empty table is yielded as a value rather than skipped, so that it is
/// refused by D5 rather than passing unmentioned — an absence the instrument
/// could not have found is not evidence ([Verification lessons] §8).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn walk<'a>(
    schema: &Schema,
    layer: Layer,
    document: &'a Table,
) -> Result<Vec<(Key, &'a Value)>, ConfigRefused> {
    let mut found = Vec::new();
    descend(schema, layer, document, &mut Vec::new(), &mut found)?;
    Ok(found)
}

fn descend<'a>(
    schema: &Schema,
    layer: Layer,
    table: &'a Table,
    above: &mut Vec<String>,
    found: &mut Vec<(Key, &'a Value)>,
) -> Result<(), ConfigRefused> {
    for (name, value) in table.iter() {
        above.push(name.clone());
        let path = above.join(".");
        // A name no key can be built from is refused rather than skipped. A
        // skip would be a silent drop, and D5's whole argument is that a
        // setting which quietly does nothing is the worst outcome available.
        let key = Key::new(&path).map_err(|refusal| ConfigRefused::UnusableKey {
            layer,
            offered: path.clone(),
            refusal,
        })?;
        let declared = schema.field(&key).is_some();
        match value {
            Value::Table(inner) if !declared && !inner.is_empty() => {
                descend(schema, layer, inner, above, found)?;
            }
            _ => found.push((key, value)),
        }
        above.pop();
    }
    Ok(())
}
