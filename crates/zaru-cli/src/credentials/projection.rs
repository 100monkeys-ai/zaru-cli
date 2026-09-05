// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the agent is shown: ADR-0007 D5's namespaces, and nothing else.
//!
//! D5: "The harness presents every non-composer token to the agent as its own
//! MCP server, named `notes:<alias>`, carrying that token's tools and that
//! token's `description` as the server description."
//!
//! Two exclusions are the point of this module, and both are structural.
//!
//! **The composer's token is not here.** D4: "The agent may use any token not
//! flagged `composer`, and never the composer's." The projection is built by
//! skipping the record that carries the role, so the agent's namespace list
//! is not a filtered view of everything — the composer's entry never enters
//! it.
//!
//! **A secret is not here.** [`Namespace`] has no field one could go in. D3:
//! "The agent sees aliases, descriptions, and tool lists. It never sees a
//! secret value... a model that can read its own bearer token can exfiltrate
//! it through any tool that takes a string."
//!
//! There is also deliberately **no tool that selects a token**. D5 again:
//! "There is deliberately no 'use token X' tool. A stateful selector would
//! re-create the flapping-pointer failure ADR-0131 was written to prevent."
//! The absence is the mechanism; a check searches for one and expects to find
//! nothing.

use crate::credentials::entry::{Reach, Role};
use crate::credentials::store::{CredentialStore, Record, StoredReach};

/// The prefix ADR-0007 D5 gives every projected server.
pub const NAMESPACE_PREFIX: &str = "notes";

/// One MCP server as the agent sees it.
///
/// Three fields, and adding a fourth is a deliberate act: the check that
/// asserts what the agent can see destructures this exhaustively, so a new
/// field stops that check compiling rather than quietly travelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// `notes:<alias>`.
    pub name: String,
    /// What the token is for, plus D8's apex marking when it applies.
    pub description: String,
    /// The tool names this token grants.
    pub tools: Vec<String>,
}

impl CredentialStore {
    /// Every token the agent may use, as its own MCP server.
    ///
    /// The composer's is absent. See the module documentation.
    #[must_use]
    pub fn agent_namespaces(&self) -> Vec<Namespace> {
        self.records()
            // A provider key is filtered out before the composer is, and the
            // order does not matter because the two predicates are
            // independent -- but the reason does. D5 projects *Nuclear Notes*
            // tokens as MCP servers, and a provider key serves no MCP tools
            // at all, so projecting one would offer the agent a namespace
            // with nothing in it whose name claims otherwise. ADR-0007 D3 is
            // the sharper reason: the agent must never see a provider key,
            // and a namespace is the surface the agent reaches through.
            .filter(|(_, record)| record.is_notes())
            .filter(|(_, record)| record.role() != Some(Role::Composer.as_str()))
            .map(|(alias, record)| Namespace {
                name: format!("{NAMESPACE_PREFIX}:{alias}"),
                description: agent_description(record),
                tools: record.tools().to_vec(),
            })
            .collect()
    }
}

/// The description the agent reads, carrying D8's marking where it applies.
///
/// D8 requires an apex token be marked in three places, one of which is "the
/// description the agent reads". The marking is composed by
/// [`StoredMarking`] so that the three renderings cannot drift apart.
fn agent_description(record: &Record) -> String {
    match record.reach() {
        Some(StoredReach::InstanceLocked(_)) | None => record.description.clone(),
        Some(StoredReach::Apex) => format!("{} [{}]", record.description, Reach::APEX_MARKING),
    }
}

/// Which family a listing is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// [ADR-0007](https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store)
    /// D7's `notes tokens`.
    Notes,
    /// `providers keys`.
    Providers,
}

/// One stored credential as **either** listing renders it.
///
/// # Three fields, and the fourth is the point
///
/// A stored record has four things a listing could want: the alias, the kind,
/// the description, and the sealed bearer value. This type has the first
/// three and **no field the fourth could occupy**, so neither listing can
/// render a value however it is written — the same argument
/// [`Namespace`] makes about what the agent sees, one surface along.
///
/// Both listings are built from this rather than from [`Record`] directly,
/// which is what makes "a listing never prints a value" one rule in one place
/// instead of two rules in two renderers that have to agree. `notes tokens`
/// adds its own three Nuclear Notes columns on top; `providers keys` prints
/// these three and stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The credential's local name.
    pub alias: String,
    /// `personal`, `app`, or a provider kind.
    pub kind: String,
    /// What it is for, in the user's own words.
    pub description: String,
}

impl CredentialStore {
    /// Every credential of one family, as a listing may see it.
    ///
    /// Sorted by alias, because the store is a `BTreeMap` keyed by one — so
    /// the order a user reads is a function of what is stored rather than of
    /// when it was stored.
    #[must_use]
    pub fn listed(&self, listing: Listing) -> Vec<Listed> {
        self.records()
            .filter(|(_, record)| match listing {
                Listing::Notes => record.is_notes(),
                Listing::Providers => !record.is_notes(),
            })
            .map(|(alias, record)| Listed {
                alias: alias.to_string(),
                kind: record.kind().to_owned(),
                description: record.description.clone(),
            })
            .collect()
    }
}
