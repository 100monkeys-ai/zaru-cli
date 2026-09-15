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
//! **The composer's token is not here, and the predicate is not the role.**
//! D4: "The agent may use any token not flagged `composer`, and never the
//! composer's." The projection is built by skipping the composer's entry, so
//! the agent's namespace list is not a filtered view of everything — that
//! entry never enters it.
//!
//! # Why the role is the wrong question, measured
//!
//! D5's own words are "every non-composer token", and until 2026-09-15 that
//! was read as "every token not carrying the `composer` role". **On every
//! machine that exists, that reading projects the composer's own credential.**
//! `notes-hints-wiring` decided on 2026-09-14, under directives 20, 29 and 31,
//! that when no token carries the role and exactly one Nuclear Notes token is
//! stored, *that* token serves the composer's reads — because
//! [`CredentialStore::grant_composer_role`] refuses the role to any scope
//! outside [ADR-0006] D4's nine, and every token this project has ever held
//! reports **94 tools**. So the role is unheld on every real store and the
//! composer still reads with something.
//!
//! Those 94 include `me.set_current_workspace`. A projection excluding only
//! the role-holder would therefore declare, to the model, the pointer the
//! person is typing against — [ADR-0131]'s "the agent has yanked the human's
//! tab out from under them", reproduced inside one product, which
//! [ADR-0006] D1 exists to prevent. That record's own amendments page named
//! the moment: the one-token reading "expires the moment the agent's
//! projection ships, and whoever builds that inherits this paragraph".
//!
//! **So the predicate is [`composer_token`]'s answer**, which is the function
//! that decides what the composer actually reads with — one reading, two
//! consumers, the same argument D1 makes about one store. The role arm is kept
//! beside it rather than replaced, because the two do not answer the same
//! question in every state: `composer_token` answers `None` for a role held by
//! an *apex* token, whose entry has no host to map through, and D4 excludes
//! that token regardless. Either arm alone leaves a hole; both together are
//! "never the composer's".
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0131]: https://cortex.page/adrs/p/0131-mcp-per-token-current-workspace
//! [`composer_token`]: crate::credentials::composer_token
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

use crate::credentials::entry::{CachedTool, Reach, Role};
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
    /// The tools this token grants, as much of each as D6's cache holds.
    ///
    /// **Widened from `Vec<String>` on 2026-09-15 and the field count is
    /// unchanged, which is what keeps ADR-0007 clause 5 satisfied.** That
    /// clause's mechanism is the exhaustive destructure in
    /// `what_the_agent_sees_is_three_fields_and_a_fourth_would_not_compile`:
    /// it names three fields, so a *fourth* stops it compiling. Changing what
    /// the third field holds does not add one — a tool list carrying each
    /// tool's own declaration is still a tool list, and it is still the only
    /// three things D3 says the agent sees.
    pub tools: Vec<CachedTool>,
}

impl CredentialStore {
    /// Every token the agent may use, as its own MCP server.
    ///
    /// The composer's is absent. See the module documentation.
    #[must_use]
    pub fn agent_namespaces(&self) -> Vec<Namespace> {
        // Asked once, outside the walk, because it reads the whole store: its
        // one-token case is a property of how many Notes tokens there are
        // rather than of the record in hand.
        let reading_for_the_composer =
            crate::credentials::notes::composer_token(self).map(|(alias, _)| alias);
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
            // D4's own clause: a token flagged `composer` is never the
            // agent's, whatever else is true of the store.
            .filter(|(_, record)| record.role() != Some(Role::Composer.as_str()))
            // And the token the composer actually reads with, which on every
            // real store carries no role at all. See the module documentation.
            .filter(|(alias, _)| Some(*alias) != reading_for_the_composer.as_ref())
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
