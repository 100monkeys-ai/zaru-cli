// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D6: what the two sides disagreed about, and why nothing here can
//! quietly settle it.
//!
//! D6: "Before offloading, the harness compares its resolution table with the
//! orchestrator's. A disagreement is shown, naming both sides, and the user
//! chooses. **It is never silently reconciled.** Silent reconciliation is how a
//! benchmark run produces two different models' output in one table."
//!
//! # "Never silently reconciled" is a compile error, not a check
//!
//! The two sides of a [`Disagreement`] are **different types**.
//! [`ModelId`] can be constructed only inside
//! [`resolution`](super::resolution), and [`RemoteModelId`] is what an
//! implementation of [`AliasNegotiation`] reports. So the reconciliation this
//! clause forbids — taking the orchestrator's answer and calling it ours —
//! cannot be written: there is no `From` between them in either direction, and
//! building a `ModelId` out of a remote one does not compile.
//!
//! Reading both is ordinary, and has to be: a disagreement that could not be
//! rendered would be one the user could not choose between. It is *assignment*
//! that is impossible, which is the same shape
//! [`CredentialRef`](crate::config::CredentialRef) uses — a value nothing can
//! put a secret into — applied to a different mistake.
//!
//! # This module surfaces and does not ask
//!
//! D6 ends "and the user chooses", and **no confirmer is declared here**. Two
//! already exist in this crate — [`Confirm`](crate::credentials::Confirm) for
//! ADR-0007 D8's apex prompt and [ADR-0011]'s for a tool call — and a third
//! would make "ask the user something" a vocabulary spread across three
//! records with no page owning it. Whether those are three ports or one is
//! recorded as an open question on [operations/adr-status] rather than
//! answered by adding the third. What is built is the value the prompt would
//! carry.
//!
//! # What D6 as written does not cover
//!
//! "Naming both sides" presupposes that both sides name something.
//! [`disagreements`] therefore reports the case D6 describes — both sides
//! resolved, and to different models — and **not** the asymmetric ones, where
//! one side has an alias the other does not. Those are real and this arc
//! invents no behaviour for them; the gap is raised on the record. It is
//! sharper than it looks now that the sets are known to differ in *shape*: the
//! platform's alias map takes whatever a node configuration declares, while
//! [`ModelAlias`] is closed.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status

use crate::providers::alias::ModelAlias;
use crate::providers::resolution::{ModelId, ModelIdRefused, ModelTable, ResolvedModel};
use core::fmt;

/// The orchestrator could not be asked what it resolves an alias to.
///
/// Carries the implementation's own wording and nothing else, exactly as
/// [`SourceFailure`](crate::config::SourceFailure) and
/// [`SealFailure`](crate::credentials::SealFailure) do. Its class belongs to
/// the implementation rather than to this value — an orchestrator that is down
/// is environmental and one that refuses a credential is the user's — and no
/// implementation exists to state it, which is why it is not mapped into
/// [ADR-0016]'s taxonomy.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiationFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl NegotiationFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for NegotiationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for NegotiationFailure {}

/// What the orchestrator says an alias resolves to.
///
/// **A different type from [`ModelId`] on purpose.** See the module
/// documentation: this is what makes D6's "never silently reconciled" something
/// the compiler enforces rather than something a reviewer notices.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteModelId(String);

impl RemoteModelId {
    /// Take what the orchestrator reported.
    ///
    /// Refuses the same shapes a local model identifier is refused, through
    /// [`ModelIdRefused`], because both are rendered into the same listing and
    /// a second set of rules would be the same rule twice.
    ///
    /// # Errors
    ///
    /// [`ModelIdRefused`], when the reported identifier cannot be rendered.
    pub fn reported(offered: &str) -> Result<Self, ModelIdRefused> {
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

    /// What the orchestrator said, for rendering and for comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One alias the two sides resolve differently.
///
/// Carries **both** sides, which is D6's own requirement, and carries them as
/// two types so that neither can become the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    /// The alias the two sides disagree about.
    pub alias: ModelAlias,
    /// What this harness resolved it to.
    pub local: ModelId,
    /// What the orchestrator resolved it to.
    pub remote: RemoteModelId,
}

impl fmt::Display for Disagreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` resolves to {} here and to {} on the orchestrator",
            self.alias, self.local, self.remote
        )
    }
}

/// Where the orchestrator's resolution table comes from.
///
/// **Nothing in this crate's product tree implements it.** Reaching an
/// orchestrator is `zaru-aegis`'s, over MCP and SEAL across a process boundary
/// per [ADR-0003] D5, and that crate is a skeleton.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
pub trait AliasNegotiation {
    /// What the orchestrator resolves each alias to.
    ///
    /// An alias the orchestrator does not carry is simply absent from the
    /// result rather than present with an empty value, so that "it has no
    /// answer" and "its answer is nothing" stay different facts.
    ///
    /// # Errors
    ///
    /// [`NegotiationFailure`], carrying the implementation's own wording.
    fn remote_table(&self) -> Result<Vec<(ModelAlias, RemoteModelId)>, NegotiationFailure>;
}

/// Every alias the two sides resolve differently.
///
/// Reports the case [ADR-0012] D6 describes — both sides resolved, to different
/// models — in [`ModelAlias::ALL`]'s order. See the module documentation for
/// what it deliberately does not report.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[must_use]
pub fn disagreements(
    local: &ModelTable,
    remote: &[(ModelAlias, RemoteModelId)],
) -> Vec<Disagreement> {
    let mut found = Vec::new();
    for alias in ModelAlias::ALL {
        let ResolvedModel::Resolved { model, .. } = local.row(alias) else {
            continue;
        };
        let Some((_, theirs)) = remote.iter().find(|(candidate, _)| *candidate == alias) else {
            continue;
        };
        if model.as_str() != theirs.as_str() {
            found.push(Disagreement {
                alias,
                local: model.clone(),
                remote: theirs.clone(),
            });
        }
    }
    found
}
