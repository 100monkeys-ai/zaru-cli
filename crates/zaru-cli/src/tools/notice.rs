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
//! # Every tier, since 2026-09-28
//!
//! D2's table gives `contained` and `linked` a membrane, and until 2026-09-28
//! this notice was not said at either. But neither tier is built: a tool call
//! at `contained` runs on the machine exactly as at `bare`. So the notice is
//! said at every tier, and the caller adds that the tier is not built. Ruled
//! by the coordinator on 2026-09-28, open to Jeshua's veto: the program never
//! claims protection it does not give.
//!
//! # Once per session, and a session outlives the process it was opened in
//!
//! "Once at session start" and "once per call" were the same sentence while
//! one invocation was one session. They are not once a session holds a
//! conversation across `--resume`, and this type is rebuilt when a process
//! opens — so until 2026-09-05 a resumed session stated the notice again.
//! [`SessionNotice::in_session`] is the rule that closes it, reading
//! [`AlreadySaid`] off the session's own transcript, which is where
//! [ADR-0002]'s Status tracking rules the counter belongs. **There is no
//! second store**: [`crate::session::Record::Said`] is the sixth producer of
//! ADR-0010 D2's stream and the only thing that remembers.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//!
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::session::AlreadySaid;

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
    /// A notice owed once, carrying `sentence`.
    ///
    /// Every tier owes it, because no tier contains anything yet: the caller
    /// composes the sentence for the tier, with
    /// [`not_a_sandbox_at`](crate::compose::prose::not_a_sandbox_at).
    #[must_use]
    pub fn new(sentence: impl Into<String>) -> Self {
        Self {
            sentence: Some(sentence.into()),
        }
    }

    /// The notice a session owes, given what it has already said.
    ///
    /// # This is D2's rule, and it is not [ADR-0002] D8's
    ///
    /// A session that has already stated it does not state it again, which is
    /// what makes "once at session start" the *session's* rather than the
    /// process's. The witness is the session's own transcript.
    ///
    /// It was also a property of the tier until 2026-09-28, when a session
    /// at `contained` or `linked` was told nothing. Those tiers are not built
    /// and contain nothing, so the notice is owed at every tier now.
    ///
    /// [`MissingManifest::for_manifest_in_session`](crate::manifest::MissingManifest)
    /// is the other line's rule and reads a different field of the same
    /// witness for a different reason; see there.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    #[must_use]
    pub fn in_session(sentence: impl Into<String>, said: &AlreadySaid) -> Option<Self> {
        if said.notice() {
            return None;
        }
        Some(Self::new(sentence))
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
