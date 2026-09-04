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
            .filter(|(_, record)| record.role.as_deref() != Some(Role::Composer.as_str()))
            .map(|(alias, record)| Namespace {
                name: format!("{NAMESPACE_PREFIX}:{alias}"),
                description: agent_description(record),
                tools: record.tools.clone(),
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
    match &record.reach {
        StoredReach::InstanceLocked(_) => record.description.clone(),
        StoredReach::Apex => format!("{} [{}]", record.description, Reach::APEX_MARKING),
    }
}
