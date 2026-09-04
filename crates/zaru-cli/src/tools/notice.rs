// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0011 D2's once-per-session statement that `bare` is not a sandbox.
//!
//! D2: "**At bare tier the harness states plainly, once at session start,
//! that it is not a sandbox.** A permission prompt that reads like
//! containment while being a suggestion is worse than no prompt, because it
//! manufactures a confidence the user has not earned."
//!
//! # The sentence is a parameter, and that is deliberate
//!
//! What the user is told they are *not* getting is user-facing prose about a
//! security posture, and [Autonomous Development] puts authoring that on the
//! human side of the boundary: "one of its edits is user-facing prose about
//! what somebody is agreeing to... Draft the exact edits the name occupies,
//! so a person's decision is a yes or no rather than a design task, and leave
//! the decision."
//!
//! So this module builds the *mechanism* — a sentence stated once and never
//! again — and the sentence itself arrives from the caller. Exact proposed
//! wording is drafted on ADR-0011 as an Update for the record's author to
//! accept or replace; nothing here has a default, because a default would be
//! the wording, chosen by whoever typed it.
//!
//! # Bare only
//!
//! D2's table gives `contained` and `linked` a membrane, so the sentence
//! would be false at both. [`SessionNotice::for_tier`] returns nothing there,
//! which makes "the line is not emitted where it would be untrue" absence
//! rather than a branch.
//!
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::tools::mode::Tier;

/// A sentence the harness states once at session start and never again.
///
/// Taking it rather than holding a copy is what makes "once" a property of
/// the type: there is no second sentence to state, because the first call
/// moves it out.
#[derive(Debug)]
pub struct SessionNotice {
    sentence: Option<String>,
}

impl SessionNotice {
    /// The notice this tier owes the user, if it owes one.
    ///
    /// `None` at `contained` and `linked`, where a membrane exists and the
    /// sentence would be false.
    #[must_use]
    pub fn for_tier(tier: Tier, sentence: impl Into<String>) -> Option<Self> {
        if tier.has_membrane() {
            return None;
        }
        Some(Self {
            sentence: Some(sentence.into()),
        })
    }

    /// The sentence, the first time it is asked for, and never again.
    #[must_use]
    pub fn state_once(&mut self) -> Option<String> {
        self.sentence.take()
    }

    /// Whether the sentence is still owed.
    ///
    /// A reader rather than a second copy: a notice that could report itself
    /// unstated after stating would be two sources of truth for one fact.
    #[must_use]
    pub const fn is_owed(&self) -> bool {
        self.sentence.is_some()
    }
}
