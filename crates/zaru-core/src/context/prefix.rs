// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Layers 1 to 4: the stable prefix, built once and never rewritten.
//!
//! ADR-0013 D1: "Layers 1 to 4 form the stable prefix and are never rewritten
//! mid-session. That is what makes prompt caching work, and the cost
//! difference is large enough that it is an architectural constraint rather
//! than an optimisation."
//!
//! # The clause is a property of this type, not a rule to remember
//!
//! Trigger clause 1 asks that layers 1 to 4 be byte-identical across every
//! turn of a long session. A check can assert that over a session it drove;
//! it cannot assert it over the session a user will have. What makes the
//! clause hold for every session is that **there is no method here that
//! changes anything**: the fields are private, no method takes `&mut self`,
//! nothing is public to assign to, and the rendered text is produced once, in
//! the constructor, and only ever borrowed afterwards. A caller that wants
//! different layers 1 to 4 has to build a different value, which is a new
//! session's prefix and not a rewrite of this one.
//!
//! That is [Verification lessons] §30 applied before the fact: the
//! alternative — a mutable prefix with a comment saying not to touch it
//! mid-session — is a hazard documented rather than a hazard prevented.
//!
//! # The order comes from `Layer`
//!
//! The four parts are rendered in [`Layer::ALL`] order rather than in the
//! order the constructor's argument happens to list them, so D1's precedence
//! has exactly one home.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::context::layer::Layer;
use serde::{Deserialize, Serialize};

/// What separates one layer from the next in a rendered context.
pub(crate) const SEPARATOR: &str = "\n\n";

/// The four texts the stable prefix is built from.
///
/// A plain struct rather than four positional arguments, because four strings
/// in a row is four things to get in the wrong order and the compiler would
/// not notice.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PrefixParts {
    /// Layer 1.
    pub system_prompt_and_persona: String,
    /// Layer 2.
    pub grounding: String,
    /// Layer 3. ADR-0031 D3 delivers this inside the served prompt; nothing
    /// here fetches it, because that record's Status tracking says a second
    /// fetch path is a second thing that can disagree.
    pub relationship_memory: String,
    /// Layer 4.
    pub project_manifest_summary: String,
}

/// Layers 1 to 4, assembled once at session start.
///
/// There is no way to change one. See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StablePrefix {
    parts: PrefixParts,
    rendered: String,
}

impl StablePrefix {
    /// Assemble the prefix for a session. The only constructor.
    ///
    /// An empty layer contributes nothing to the rendered text — not a blank
    /// section — because a separator around nothing spends tokens and says
    /// nothing. The part is still readable through [`Self::layer`], so an
    /// empty layer 4 and an absent one stay distinguishable to a caller.
    #[must_use]
    pub fn assembled_once(parts: PrefixParts) -> Self {
        let mut rendered = String::new();
        for layer in Layer::ALL {
            if !layer.in_stable_prefix() {
                continue;
            }
            let text = part_of(&parts, layer);
            if text.is_empty() {
                continue;
            }
            if !rendered.is_empty() {
                rendered.push_str(SEPARATOR);
            }
            rendered.push_str(text);
        }
        Self { parts, rendered }
    }

    /// The whole prefix, as it goes into every assembled context.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.rendered
    }

    /// One layer's own text, or `None` for a layer that is not in the prefix.
    #[must_use]
    pub fn layer(&self, layer: Layer) -> Option<&str> {
        layer
            .in_stable_prefix()
            .then(|| part_of(&self.parts, layer))
    }
}

/// Which part belongs to which layer.
///
/// Exhaustive over every layer rather than falling through on a wildcard, so
/// an eighth layer fails to compile here and has to be placed deliberately.
fn part_of(parts: &PrefixParts, layer: Layer) -> &str {
    match layer {
        Layer::SystemPromptAndPersona => &parts.system_prompt_and_persona,
        Layer::Grounding => &parts.grounding,
        Layer::RelationshipMemory => &parts.relationship_memory,
        Layer::ProjectManifestSummary => &parts.project_manifest_summary,
        Layer::UserAttachments | Layer::ConversationAndToolResults | Layer::IterationHistory => "",
    }
}
