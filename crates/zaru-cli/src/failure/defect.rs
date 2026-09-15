// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D3: what a defect says about itself.
//!
//! D3: "Panics and internal invariant violations are caught at the session
//! boundary and presented as what they are, with the session id, the version,
//! and where to report it. The transcript is already on disk and the message
//! says so. **Never present a defect as a user error.**"
//!
//! # The session arrives as an input, and none of ADR-0010 is built
//!
//! [ADR-0010] is not started: there is no `~/.zaru/sessions/<ulid>/`, no ULID
//! is minted anywhere in this workspace, and no directory is created. So the
//! session's identity is a **seam** — [`SessionEvidence`], with no product
//! implementation supplying its first variant — exactly as ADR-0011's identity
//! seam, ADR-0007's sealing and `zaru-core`'s unimplemented ports are.
//!
//! **[`SessionEvidence::NoSessionExists`] is the point of the type.** D3 says
//! the message says the transcript is on disk. With no session there is no
//! transcript, so saying it would be a lie, and this type makes that lie
//! unrepresentable: a report can only claim a transcript when it was handed
//! one. The day ADR-0010 lands, one call site in the binary changes and
//! nothing here does.
//!
//! # The report does not carry the panic's own words
//!
//! Under a **delegated coordinator ruling of 2026-09-04**, open to Jeshua's
//! veto: a [`DefectReport`] carries the location, the version, where to report
//! and the session evidence, and it does **not** carry the panic's message.
//! D3's list is the session id, the version, where to report, and that the
//! transcript is on disk; the message belongs to the transcript, which is
//! ADR-0010 D2's. The boundary captures it beside the report rather than
//! inside it — see [`guard`](crate::failure::guard::guard) — so a
//! presentation cannot show it, because it is not in the value a presentation
//! is built from.
//!
//! That is a stronger guarantee than a field a renderer is trusted to omit,
//! and it matters because a panic's message is the one field on this path that
//! can carry arbitrary captured text — a failing command's output, a provider
//! response. **This module designs no redaction and adds no filter**, and
//! [ADR-0008]'s trigger clause 6 — decided on 2026-09-05 — does not ask it
//! to: that decision covers every path from captured bytes into a **model
//! prompt**, and a defect report is read by a person. The structural
//! guarantee here is stronger than a filter anyway, because the message is
//! not in the value a presentation is built from. Whether a defect report may
//! ever show the message is recorded as an open question on ADR-0016 with its
//! alternatives named.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use core::fmt;
use std::path::{Path, PathBuf};

/// Why a session id was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdRefused {
    /// The id was empty.
    Empty,
    /// The id carried a control character.
    ///
    /// A defect report is rendered into a terminal and pasted into a bug
    /// report. A control character in either is how the report stops being
    /// evidence — the same argument
    /// [`AliasRefused::Control`](crate::credentials::AliasRefused::Control)
    /// makes about ADR-0007 D7's listing.
    Control {
        /// The id as it was offered.
        offered: String,
    },
}

impl fmt::Display for SessionIdRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                // ADR-0016 D3 requires a defect report name the session.
                "a session id is empty, and a defect report that must name the session cannot \
                 name nothing",
            ),
            Self::Control { offered } => write!(
                f,
                "the session id {offered:?} carries a control character; a defect report is \
                 rendered into a terminal and pasted into a bug report, and one there can erase \
                 or overwrite a neighbouring row"
            ),
        }
    }
}

impl std::error::Error for SessionIdRefused {}

/// A session's identity, as ADR-0010 D1 will supply it.
///
/// **This crate mints none.** D1 chooses a ULID and this type takes whatever
/// its caller was handed, because choosing the shape here would settle
/// ADR-0010 D1 inside a sixth record's implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

impl SessionId {
    /// Take a session id from whoever owns the session.
    ///
    /// # Errors
    ///
    /// [`SessionIdRefused`] for an empty id or one carrying a control
    /// character.
    pub fn new(id: impl Into<String>) -> Result<Self, SessionIdRefused> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(SessionIdRefused::Empty);
        }
        if id.chars().any(char::is_control) {
            return Err(SessionIdRefused::Control {
                offered: id.to_string(),
            });
        }
        Ok(Self(id))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the defect boundary knows about the session it is inside.
///
/// A seam with no product implementation of its first variant. See the module
/// documentation for why the second variant is the load-bearing one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvidence {
    /// ADR-0010 D1's session directory exists, and this is its identity and
    /// the transcript inside it.
    Session {
        /// D1's ULID, as its owner minted it.
        id: SessionId,
        /// D1's `transcript.jsonl`.
        transcript: PathBuf,
    },
    /// There is no session. **ADR-0010 is not started**, so this is what the
    /// `zaru` binary passes today.
    NoSessionExists,
}

impl SessionEvidence {
    /// The session's id, where there is a session.
    #[must_use]
    pub const fn id(&self) -> Option<&SessionId> {
        match self {
            Self::Session { id, .. } => Some(id),
            Self::NoSessionExists => None,
        }
    }

    /// The transcript on disk, where there is one.
    #[must_use]
    pub fn transcript(&self) -> Option<&Path> {
        match self {
            Self::Session { transcript, .. } => Some(transcript.as_path()),
            Self::NoSessionExists => None,
        }
    }
}

/// Where in this crate's own source a defect surfaced.
///
/// Not on D3's list, and carried anyway under the coordinator's ruling: it is
/// what turns a report into something a maintainer can act on, and it is the
/// one part of a panic that cannot carry captured text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The source file.
    pub file: String,
    /// The line.
    pub line: u32,
    /// The column.
    pub column: u32,
}

impl Location {
    /// Where the runtime gave no location.
    ///
    /// `PanicHookInfo::location` is documented as returning `None` in cases the
    /// standard library does not enumerate, so the boundary has to have an
    /// answer. Saying the location is unknown is D2's "admits there is not
    /// one" applied one level down: a report naming a made-up file is worse
    /// than one naming none.
    #[must_use]
    pub fn unknown() -> Self {
        Self {
            file: "an unknown location".to_owned(),
            line: 0,
            column: 0,
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

/// ADR-0016 D3's defect, with everything the record says it must say.
///
/// **Cannot be constructed without the version, where to report, and the
/// session evidence.** D2's "admits there is not one" is discharged by
/// [`DefectReport::there_is_nothing_to_configure`], which is part of what this
/// type renders rather than a sentence a caller is trusted to add: D3 says
/// "Never present a defect as a user error. Telling someone to check their
/// configuration when the harness has a bug costs them an hour and costs us
/// the report."
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefectReport {
    version: String,
    report_at: String,
    location: Location,
    session: SessionEvidence,
}

impl DefectReport {
    /// D2's admission, spelled once.
    ///
    /// A defect's remedy is that there is not one, and this is the sentence
    /// that says so. It is a constant rather than a caller's parameter,
    /// because unlike D2's *remedies* — which are the raising site's, since
    /// only it knows what to change — there is exactly one thing to say about
    /// a bug in the harness.
    pub const THERE_IS_NOTHING_TO_CONFIGURE: &'static str =
        "this is a bug in Zaru, not something you can configure";

    /// Report a defect.
    ///
    /// `version` and `report_at` are read out of the package's own metadata by
    /// the caller rather than retyped — the same argument the binary's
    /// `composition()` already makes: a list retyped beside the binary is a
    /// list that drifts.
    #[must_use]
    pub fn new(
        version: impl Into<String>,
        report_at: impl Into<String>,
        location: Location,
        session: SessionEvidence,
    ) -> Self {
        Self {
            version: version.into(),
            report_at: report_at.into(),
            location,
            session,
        }
    }

    /// The harness version this defect happened in.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where to report it.
    #[must_use]
    pub fn report_at(&self) -> &str {
        &self.report_at
    }

    /// Where in the source it surfaced.
    #[must_use]
    pub const fn location(&self) -> &Location {
        &self.location
    }

    /// What is known about the session it happened in.
    #[must_use]
    pub const fn session(&self) -> &SessionEvidence {
        &self.session
    }

    /// D2's admission for this class.
    #[must_use]
    pub const fn there_is_nothing_to_configure(&self) -> &'static str {
        Self::THERE_IS_NOTHING_TO_CONFIGURE
    }
}
