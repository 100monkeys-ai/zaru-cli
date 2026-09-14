// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The projection a renderer is handed. **Nothing here knows about a
//! terminal.**
//!
//! [ADR-0016] D1: "the class determines the presentation". This module is the
//! half of that sentence `zaru-cli` owns — *what must be shown*, as data — and
//! it stops exactly where the other half begins. There is no colour, no glyph,
//! no width, no frame and no cursor anywhere in it.
//!
//! # Why the renderer cannot be here
//!
//! `zaru-tui` depends only on `zaru-core` ([ADR-0003] D8), so it cannot see
//! this type at all. A renderer reaches it the way ADR-0005's composer reaches
//! its two search tiers: through a port its own crate declares, with
//! `zaru-cli` implementing and injecting at the composition root. **Nothing is
//! added to `zaru-core` by this module and no D8 edge moves.**
//!
//! # What the trigger clauses get from this, and what they do not
//!
//! Clause 1 — "one failure of each class renders in its class's presentation"
//! — is half here: [`Presentation::of`] is a total function of a
//! [`Classified`] and the five results are distinguishable, each carrying what
//! its D1 row obliges. The appearance of each is `zaru-tui`'s.
//!
//! Clause 2 — "an iteration failure is asserted not to render in the error
//! register" — is half here and it is the strong half: the register is
//! *derived* from the class by [`Class::is_the_error_register`] rather than
//! being a field, so an expected failure in the error register is not a state
//! any code can construct.
//!
//! Clause 5's "asserted distinct from iterations in the rendered output" is
//! half here too: the label is [`RETRY_LABEL`]
//! and the word *iteration* appears in nothing this module renders, which a
//! check asserts over every class.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::class::Class;
use crate::failure::classified::Classified;
use crate::failure::defect::{DefectReport, SessionEvidence};
use crate::failure::remedy::Action;
use crate::failure::wait::{RETRY_LABEL, RetryRecord, Wait};
use core::fmt;

/// One line under a presentation's headline.
///
/// `lead` is D2's `set one:` / `or:` column, present when there is one, and
/// `text` is the line itself. A renderer aligns them; this type does not,
/// because column widths are a terminal's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The lead-in, where the line has one.
    pub lead: Option<String>,
    /// The line.
    pub text: String,
}

impl Line {
    /// A line with a lead-in.
    fn led(lead: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            lead: Some(lead.into()),
            text: text.into(),
        }
    }

    /// A line with none.
    fn plain(text: impl Into<String>) -> Self {
        Self {
            lead: None,
            text: text.into(),
        }
    }

    /// The line as one string: the lead-in, a space, and the text.
    ///
    /// **The one place the two fields become one string**, and that is the
    /// whole reason it exists rather than being written at each call site.
    /// Three consumers need exactly this — the out-of-session projection
    /// below, the transcript's [`crate::session::FailureLine`], and the
    /// terminal's pane adapter — and until 2026-09-14 the same three lines were
    /// typed in each of them. A remedy the pane paints and the same remedy
    /// repainted from the file on `--resume` must not be able to disagree
    /// about a space.
    ///
    /// **It carries no indent, no glyph and no width**, for the reason this
    /// module carries none: where the line sits is the renderer's. The
    /// out-of-session projection adds its own two spaces; the pane's indent is
    /// its register's glyph column.
    #[must_use]
    pub fn flattened(&self) -> String {
        match &self.lead {
            Some(lead) => format!("{lead} {}", self.text),
            None => self.text.clone(),
        }
    }
}

/// What a renderer must show for one failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    /// D1's class, which decides the register and the treatment.
    pub class: Class,
    /// The one line saying what happened.
    pub headline: String,
    /// Everything under it — D2's remedy, D4's retry, D1's tier, D3's report.
    pub lines: Vec<Line>,
}

impl Presentation {
    /// Project a classified failure into what must be shown.
    ///
    /// A **total function over the five classes with no wildcard arm**, so a
    /// sixth class cannot arrive without somebody deciding what it shows.
    #[must_use]
    pub fn of(classified: &Classified) -> Self {
        let class = classified.class();
        match classified {
            Classified::Expected(expected) => Self {
                class,
                headline: expected.statement().as_str().to_owned(),
                lines: Vec::new(),
            },
            Classified::UserCorrectable { statement, remedy } => Self {
                class,
                headline: statement.as_str().to_owned(),
                lines: remedy.actions().map(Self::action_line).collect(),
            },
            Classified::Environmental { statement, wait } => Self {
                class,
                headline: statement.as_str().to_owned(),
                lines: Self::wait_lines(wait),
            },
            Classified::Capability {
                statement,
                offered_by,
            } => Self {
                class,
                headline: statement.as_str().to_owned(),
                lines: vec![Line::led(
                    "available at:",
                    format!("{offered_by} tier and above"),
                )],
            },
            Classified::Defect(report) => Self {
                class,
                headline: format!(
                    "a defect in Zaru {}, at {}",
                    report.version(),
                    report.location()
                ),
                lines: Self::defect_lines(report),
            },
        }
    }

    /// Whether this renders in the error register. ADR-0016 D1.
    #[must_use]
    pub const fn is_the_error_register(&self) -> bool {
        self.class.is_the_error_register()
    }

    fn action_line(action: &Action) -> Line {
        match action.command() {
            Some(command) => Line::led(action.lead().as_str(), command),
            None => Line::plain(action.lead().as_str()),
        }
    }

    fn wait_lines(wait: &Wait) -> Vec<Line> {
        match wait {
            Wait::Retrying(record) => vec![Self::retry_line(record)],
            Wait::NoWaitWillHelp(statement) => {
                vec![Line::led("waiting will not help:", statement.as_str())]
            }
        }
    }

    /// ADR-0016 D4's line: labelled `retry`, counted, and bounded, all three
    /// visible in the one line a reader sees.
    fn retry_line(record: &RetryRecord) -> Line {
        Line::led(
            format!("{RETRY_LABEL}:"),
            format!(
                "{} of {} made, {} remaining, {:?} before the next",
                record.made(),
                record.policy().ceiling.get(),
                record.remaining(),
                record.backoff()
            ),
        )
    }

    fn defect_lines(report: &DefectReport) -> Vec<Line> {
        let mut lines = vec![Line::plain(report.there_is_nothing_to_configure())];
        match report.session() {
            SessionEvidence::Session { id, transcript } => {
                lines.push(Line::led("session:", id.as_str()));
                lines.push(Line::led("transcript:", transcript.display().to_string()));
            }
            SessionEvidence::NoSessionExists => {
                lines.push(Line::plain(
                    "there is no session and no transcript was written",
                ));
            }
        }
        lines.push(Line::led("report it at:", report.report_at()));
        lines
    }
}

impl fmt::Display for Presentation {
    /// The plain-text projection. Colour, glyph, width and frame are
    /// `zaru-tui`'s and are not decided here.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.headline)?;
        for line in &self.lines {
            write!(f, "\n  {}", line.flattened())?;
        }
        Ok(())
    }
}
