// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0013 D1's seven layers, and the precedence between them.
//!
//! ```text
//! 1  System prompt and persona          never discarded
//! 2  Grounding, session-start           never discarded silently
//! 3  Relationship memory                never discarded
//! 4  Project manifest summary           never discarded
//! 5  User-attached items                discarded last, and announced
//! 6  Conversation and tool results      compacted first
//! 7  Iteration history                  compacted first, summarised
//! ```
//!
//! **The order is derived, not written down twice.** [`Ord`] is derived, and
//! on a fieldless enum that is declaration order — so D1's table *is* this
//! enum, and there is no second list beside it to go stale. [Verification
//! lessons] §30: a comment telling the next person to keep two things in step
//! is a hazard documented rather than a hazard prevented.
//!
//! [`Layer::retention`] is the second derivation, and it is exhaustive, so a
//! new layer fails to compile here rather than silently inheriting its
//! neighbour's rule.
//!
//! # Layer 2's "silently"
//!
//! D1's table says layer 2 is "never discarded **silently**", which reads as
//! though it could be discarded with an announcement. D1's own prose is
//! stronger: "Layers 1 to 4 form the stable prefix and are never rewritten
//! mid-session." The stronger rule is the one built, and it is structural —
//! [`crate::context::StablePrefix`] has no method that changes it — so within
//! a session the weaker reading has nothing to describe. Recorded on the
//! record as a delegated coordinator ruling of 2026-09-04 rather than left as
//! a difference a reader has to notice.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

/// What ADR-0013 D1 says may happen to a layer under pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Never discarded, and never rewritten mid-session. Layers 1 to 4.
    NeverDiscarded,
    /// Discarded last of everything that can go, and announced when it is.
    DiscardedLast,
    /// The first thing compacted.
    CompactedFirst,
}

/// One layer of the context, in ADR-0013 D1's precedence order.
///
/// Closed: seven variants and no `#[non_exhaustive]`. An eighth layer is a
/// change to D1, which is an ADR-level act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// 1. The system prompt and the persona. ADR-0027 serves it.
    SystemPromptAndPersona,
    /// 2. Grounding, read at session start.
    Grounding,
    /// 3. Relationship memory. ADR-0031 owns it, and it arrives inside the
    ///    served prompt rather than being fetched here — that record's Status
    ///    tracking forbids a second fetch path.
    RelationshipMemory,
    /// 4. The project manifest summary. ADR-0009 owns the manifest.
    ProjectManifestSummary,
    /// 5. Items the user attached through the composer. ADR-0005 D4.
    UserAttachments,
    /// 6. The conversation and its tool results.
    ConversationAndToolResults,
    /// 7. Iteration history. ADR-0008's loop produces it.
    IterationHistory,
}

impl Layer {
    /// Every layer, in D1's order.
    ///
    /// A hand-written list, guarded by the exhaustive match in
    /// `every_layer_the_enum_declares_appears_once_in_all`: adding a variant
    /// fails to compile there, which is the signal to add it here.
    pub const ALL: [Self; 7] = [
        Self::SystemPromptAndPersona,
        Self::Grounding,
        Self::RelationshipMemory,
        Self::ProjectManifestSummary,
        Self::UserAttachments,
        Self::ConversationAndToolResults,
        Self::IterationHistory,
    ];

    /// What D1 says may happen to this layer under pressure.
    #[must_use]
    pub const fn retention(self) -> Retention {
        match self {
            Self::SystemPromptAndPersona
            | Self::Grounding
            | Self::RelationshipMemory
            | Self::ProjectManifestSummary => Retention::NeverDiscarded,
            Self::UserAttachments => Retention::DiscardedLast,
            Self::ConversationAndToolResults | Self::IterationHistory => Retention::CompactedFirst,
        }
    }

    /// Whether this layer is part of the stable prefix.
    ///
    /// Derived from [`Self::retention`] rather than listed again: D1's stable
    /// prefix is exactly the layers it never discards, so stating it twice
    /// would be two rules that can disagree.
    #[must_use]
    pub const fn in_stable_prefix(self) -> bool {
        matches!(self.retention(), Retention::NeverDiscarded)
    }
}
