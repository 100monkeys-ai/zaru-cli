// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D4's line, as data, produced once and never again.
//!
//! D4: "A project with no `zaru.toml` runs the tool-call loop only. The harness
//! says so once at session start — **a single line naming what is unavailable
//! and how to get it** — and never mentions it again."
//!
//! # Once is a property of the type
//!
//! [`MissingManifest::state_once`] **takes** the recommendation out. There is
//! no second one to state, because the first call moves it — the shape
//! [`SessionNotice`](crate::tools::SessionNotice) uses for ADR-0011 D2's
//! not-a-sandbox line, for the same reason: a counter is a rule somebody has
//! to remember to check, and a moved value is a rule nothing can forget
//! ([Verification lessons] §30).
//!
//! [`MissingManifest::for_manifest`] returns `None` when a manifest is
//! present, so **the line is not produced where it would be untrue** by
//! absence rather than by a branch.
//!
//! # The wording is a parameter and nothing here has a default
//!
//! What a user is told they are not getting, and what they should do about it,
//! is user-facing prose about a capability. [Autonomous development] puts
//! authoring that on the human side of the boundary, and a default would be
//! the wording, chosen by whoever typed it. So both halves arrive from the
//! caller and both are required: a recommendation naming what is unavailable
//! and not how to get it is half of what D4 asks for, and there is no
//! constructor that builds one.
//!
//! Exact proposed wording is drafted on ADR-0009 as an Update for the record's
//! author to accept or replace.
//!
//! # Nothing here renders
//!
//! [`Recommendation`] is data. Where it goes is `zaru-tui`'s, exactly as
//! [`Explanation`](crate::config::Explanation) leaves D3's block to its caller
//! and for the same reason: there is no command surface and no renderer.
//!
//! # It is not a failure, and that is deliberate
//!
//! [ADR-0016] D1's five classes are kinds of *failure*. A project with no
//! manifest has not failed at anything — D4 calls the line a **recommendation**
//! under [ADR-0002] D8. Making it a
//! [`Classified`](crate::failure::Classified) would be the misclassification
//! that record's Negative consequence warns about: "a misclassified error is
//! worse than an unclassified one because the presentation actively misleads".
//!
//! # Three things about D4 that the records do not agree on
//!
//! Recorded here because the code had to take a position on one of them, and
//! raised as an open question on ADR-0009 and ADR-0002 rather than settled.
//!
//! D4 says the line is stated "once at session start … and never again", and
//! then calls it "a recommendation under ADR-0002 D8: caused by an observed
//! condition, once per session, **suppressed after three showings**". ADR-0002
//! D8 has two kinds and D4's sentence matches neither. An **event-anchored
//! recommendation** is "appended to the end of the triggering turn" and "fires
//! at most once ever". A **standing tip** lives in the composer's hint strip
//! and is "suppressed after three displays without action", because an
//! ephemeral strip "may have been visible for 200ms and never read". D4 takes
//! the placement of neither, the count of the first and the suppression of the
//! second.
//!
//! So: **where** does the line appear — at session start, or appended to a
//! turn as D8's event-anchored kind is? **How many showings** — D4's own first
//! sentence says once and its second says three? And is "session start" an
//! observed condition in a completed turn at all, when D8 forbids a trigger
//! that "would still fire had the user done nothing" and its own worked
//! example for this case is "a task failed validation twice with no validators
//! declared for the project", which fires after work rather than before it?
//!
//! Under a delegated coordinator ruling of 2026-09-04 **D4's first sentence
//! takes the count**: once, and never again. That is the stricter of the two
//! readings and the one D4 states in its own voice. **All three were answered
//! on 2026-09-05 under directive 20 by choosing one of D8's two kinds**: this
//! line is the **event-anchored** one, appended to the end of the first turn,
//! fired at most once ever, spending the session's one recommendation, and
//! **not** subject to the three-display suppression, which is the standing
//! tip's rule. D4 and ADR-0002 D8 were rewritten in the same change and the
//! two records now describe one mechanism.
//!
//! # "At most once ever" outlives the process, since 2026-09-05
//!
//! This type is rebuilt when a process opens, so a session resumed a second
//! time stated the line again and "once ever" was true of a session and not
//! of a session reopened. [`MissingManifest::for_manifest_in_session`] is the
//! rule that closes it, reading
//! [`crate::session::AlreadySaid`] off the session's own
//! transcript — where ADR-0002's Status tracking rules the counter belongs —
//! and **there is no second store**: `crate::session::Record::Said` is the
//! sixth producer of ADR-0010 D2's stream and the only thing that remembers.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Autonomous development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::failure::Statement;
use crate::manifest::document::Manifest;
use crate::session::AlreadySaid;
use core::fmt;

/// [ADR-0009] D4's single line, as its two named halves.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    unavailable: Statement,
    how_to_get_it: Statement,
}

impl Recommendation {
    /// What is unavailable without a manifest.
    #[must_use]
    pub const fn unavailable(&self) -> &Statement {
        &self.unavailable
    }

    /// How to get it.
    #[must_use]
    pub const fn how_to_get_it(&self) -> &Statement {
        &self.how_to_get_it
    }
}

impl fmt::Display for Recommendation {
    /// D4's "single line", as one line.
    ///
    /// The separator is the only thing this type adds to what its caller
    /// supplied, and a renderer that wants a different one has both halves.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} · {}", self.unavailable, self.how_to_get_it)
    }
}

/// The line a project with no manifest is owed, statable exactly once.
#[derive(Debug)]
pub struct MissingManifest {
    owed: Option<Recommendation>,
}

impl MissingManifest {
    /// What this project is owed, if it is owed anything.
    ///
    /// `None` when a manifest is present, so the line cannot be produced where
    /// it would be false.
    #[must_use]
    pub fn for_manifest(
        manifest: Option<&Manifest>,
        unavailable: Statement,
        how_to_get_it: Statement,
    ) -> Option<Self> {
        if manifest.is_some() {
            return None;
        }
        Some(Self {
            owed: Some(Recommendation {
                unavailable,
                how_to_get_it,
            }),
        })
    }

    /// What this project owes a session that has already said what it has.
    ///
    /// # This is D8's rule, and it is not [ADR-0011] D2's
    ///
    /// **The recommendation is a line in the pane.** [ADR-0002] D8 anchors it
    /// to a turn — "appended to the end of the triggering turn, in the
    /// transcript" — and says "the transcript is permanent and scrollable, so
    /// one showing is a real showing". So what "already in the transcript"
    /// means for this line is literally *this line was shown*, which is what
    /// [`crate::session::Record::Said`] records and what
    /// [`AlreadySaid::recommendation`] reports.
    ///
    /// **D4's own condition is re-read on every process, and it has to be.**
    /// [`Self::for_manifest`]'s refusal above still decides first: a project
    /// that gained a `zaru.toml` between two processes is **not owed the line
    /// at all** rather than owed it and suppressed, because D4 is about "a
    /// project with no `zaru.toml`" and that project runs the iteration loop
    /// instead. The converse is the case that makes "a turn has happened" the
    /// wrong derivation: a project that *had* a manifest on turn one and lost
    /// it before the resume was never owed the line, and is owed it now.
    ///
    /// [`SessionNotice::in_session`](crate::tools::SessionNotice) is
    /// the other line's rule and reads a different field of the same witness
    /// for a different reason; see there.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub fn for_manifest_in_session(
        manifest: Option<&Manifest>,
        unavailable: Statement,
        how_to_get_it: Statement,
        said: &AlreadySaid,
    ) -> Option<Self> {
        if said.recommendation() {
            return None;
        }
        Self::for_manifest(manifest, unavailable, how_to_get_it)
    }

    /// The recommendation, the first time it is asked for, and never again.
    #[must_use]
    pub fn state_once(&mut self) -> Option<Recommendation> {
        self.owed.take()
    }

    /// Whether the line is still owed.
    ///
    /// A reader rather than a second copy: a value that could report itself
    /// unstated after stating would be two sources of truth for one fact.
    #[must_use]
    pub const fn is_owed(&self) -> bool {
        self.owed.is_some()
    }
}
