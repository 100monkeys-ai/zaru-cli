// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the model is offered: ADR-0011 D1's seven, then [ADR-0007] D5's
//! projected servers.
//!
//! # One list, built once per session
//!
//! [`crate::tools::descriptor_set`] is a `OnceLock` because the seven are a
//! compile-time constant and both [`ToolExecutor`](zaru_core::tool_call::ToolExecutor)
//! implementations must hand out a slice that outlives a borrow of a lock
//! guard. A projected server's tools are neither constant nor process-wide:
//! they come from the store D6 cached and the grant a person wrote, both of
//! which are a session's. So this builds a `Vec` once where the session is
//! composed, and the `Executor` and `Shared` are both handed a slice of **that
//! one** — which keeps the property the `OnceLock` existed to hold: the set
//! the model is offered and the set the executor will accept are one set.
//!
//! # The registration order, and a refusal that cannot fire
//!
//! Built-ins first, from [`ToolName::ALL`]; then each projected server in the
//! store's own order, which is a `BTreeMap` keyed by alias — so what the model
//! is offered is a function of what is stored rather than of when it was
//! stored.
//!
//! The coordinator's default of 2026-09-15, on `operations/adr-status-questions`:
//! built-in and server tool names share **one** namespace, and a server whose
//! tool collides with a built-in is refused at registration naming the
//! built-in. [`Refused::Collides`] is that refusal, and
//! **it has no reachable input under the `notes:<alias>.<tool>` spelling** —
//! `Alias::new` refuses a colon, so every projected name carries one and no
//! built-in does. It is built as **stated defence in depth** rather than
//! claimed as a live gate, and the check that pins it asserts the property
//! that makes it unreachable (`Alias::new` refuses `:`) beside the refusal
//! itself, so a change to either is visible.
//!
//! # Nothing is declared without a grant
//!
//! A namespace whose grant is empty contributes no tools at all — not an empty
//! server, not a name with nothing under it. See
//! [`crate::credentials::grant`] for the measurement behind that: 94 tools per
//! token against a 4,096-token window.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::credentials::{Alias, CachedTool, Granted, NAMESPACE_PREFIX, Namespace};
use crate::tools::name::{Called, ToolName};
use core::fmt;
use zaru_core::tool_call::ToolDescriptor;

/// Why a projected server could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// A projected tool's declared name is one of ADR-0011 D1's seven.
    ///
    /// See the module documentation: unreachable under this spelling, built
    /// and kept as stated defence in depth.
    Collides {
        /// The alias whose server declared it.
        alias: String,
        /// The tool, as that instance spells it.
        tool: String,
        /// The built-in it collided with.
        builtin: ToolName,
    },
    /// A granted tool's cached entry carries no schema, so it cannot be
    /// declared.
    ///
    /// ADR-0007 D6's refresh rule owns the remedy, and
    /// [`ToolScope::is_declarable`](crate::credentials::ToolScope::is_declarable)
    /// is what a caller asks before it gets here — this is the arm for a
    /// caller that did not.
    NoSchema {
        /// The alias whose server declared it.
        alias: String,
        /// The tool with no schema cached.
        tool: String,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collides {
                alias,
                tool,
                builtin,
            } => write!(
                f,
                "the token `{alias}` declares a tool named `{tool}`, which is the built-in \
                 `{builtin}`; built-in and server tool names share one namespace, so this server \
                 is not registered",
            ),
            Self::NoSchema { alias, tool } => write!(
                f,
                "the token `{alias}` grants `{tool}`, and the cached scope carries no schema for \
                 it; the scope was cached before schemas were kept and is refreshed at the next \
                 session",
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// The whole surface one session offers a model.
///
/// # Errors
///
/// [`Refused`], naming the alias and the tool.
pub fn surface<'a, G>(
    namespaces: &'a [Namespace],
    granted: G,
) -> Result<Vec<ToolDescriptor>, Refused>
where
    G: Fn(&Alias) -> &'a Granted,
{
    let mut declared = crate::tools::execute::descriptors();
    for namespace in namespaces {
        declared.extend(projected(namespace, &granted)?);
    }
    Ok(declared)
}

/// One projected server's declarations, filtered to what was granted.
fn projected<'a, G>(namespace: &'a Namespace, granted: &G) -> Result<Vec<ToolDescriptor>, Refused>
where
    G: Fn(&Alias) -> &'a Granted,
{
    // `Namespace::name` is `notes:<alias>`, built by the projection from an
    // alias that passed `Alias::new`. Splitting it back is the one place this
    // module needs the alias as a value, and it cannot fail: the prefix is a
    // constant and the alias carries no colon.
    let Some(bare) = namespace.name.strip_prefix(&format!("{NAMESPACE_PREFIX}:")) else {
        return Ok(Vec::new());
    };
    let Ok(alias) = Alias::new(bare) else {
        return Ok(Vec::new());
    };
    let grant = granted(&alias);

    // **There is no `grant.is_empty()` fast path, and its absence is
    // deliberate.** One stood here until a mutant showed it was not
    // load-bearing: with the membership filter below, an empty grant declares
    // nothing anyway, so the guard was a second rule that could only ever
    // agree with the first — and two rules over one set are two rules that can
    // come to disagree. The behaviour it claimed is asserted by
    // `adr_0007_d5_a_namespace_with_no_grant_contributes_nothing_at_all`,
    // which now has exactly one implementation to be true of.
    let mut declared = Vec::new();
    for tool in &namespace.tools {
        if !grant.carries(tool.name()) {
            continue;
        }
        declared.push(declaration(&alias, namespace, tool)?);
    }
    Ok(declared)
}

/// One tool's declaration, as the model is told about it.
fn declaration(
    alias: &Alias,
    namespace: &Namespace,
    tool: &CachedTool,
) -> Result<ToolDescriptor, Refused> {
    let name = Called::Projected {
        alias: alias.clone(),
        tool: tool.name().to_owned(),
    }
    .rendered();

    // The coordinator's default of 2026-09-15. See the module documentation
    // for why this cannot fire under this spelling and is built regardless.
    if let Some(builtin) = ToolName::ALL
        .into_iter()
        .find(|builtin| builtin.as_str() == name)
    {
        return Err(Refused::Collides {
            alias: alias.as_str().to_owned(),
            tool: tool.name().to_owned(),
            builtin,
        });
    }

    let Some(schema) = tool.input_schema() else {
        return Err(Refused::NoSchema {
            alias: alias.as_str().to_owned(),
            tool: tool.name().to_owned(),
        });
    };

    Ok(ToolDescriptor {
        name,
        // **The server's own words, and the token's description behind them.**
        // ADR-0007 D2 says the token's description is "Shown to the human
        // **and** to the agent", and D3 adds that a description is rendered to
        // the agent "as **data, not instruction**" -- which is why it is a
        // suffix in parentheses rather than a sentence of its own that could
        // read as a directive. D8's apex marking rides in it, because
        // `agent_description` composes the marking into that field and the
        // three renderings are asserted not to drift.
        description: match tool.description() {
            Some(text) => format!("{text} (via {})", namespace.description),
            None => format!("via {}", namespace.description),
        },
        parameters: schema.to_owned(),
    })
}
