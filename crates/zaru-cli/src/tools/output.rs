// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0011 D5: what a tool call produced, and what a caller is shown of it.
//!
//! D5: "Stdout and stderr are captured separately, both surfaced, and both fed
//! to the model. When output exceeds the budget it is truncated head-and-tail
//! with the elision marked and the full text written to the session directory,
//! with the path shown. **A truncation the user cannot notice is how a
//! diagnosis gets built on a fragment.**"
//!
//! # Nothing here runs anything
//!
//! A [`Captured`] is what a caller hands in. Producing one needs a subprocess
//! for `cmd.run` and a network for `web.fetch`, and neither exists in this
//! crate's product tree.
//!
//! # This type is deliberately not `zaru-core`'s
//!
//! `zaru_core::iteration::ExecutionOutcome` carries the same three fields, and
//! reusing it would make `zaru-core` a shared-types crate — which
//! [ADR-0016]'s Status tracking records as **deliberately avoided**, because a
//! crate with a sibling dependency cannot be published alone and ADR-0004 D5
//! wants `zaru-seal` publishable. The two loops are different loops
//! ([ADR-0008] D1) and this is the outer one, so the duplication is the
//! recorded cost of a boundary rather than an oversight. Whoever writes the
//! adapter between them writes it in this crate, which depends on both.
//!
//! # The budget is nobody's to invent
//!
//! D5 requires truncation and names no size. A budget invented by the thing
//! being budgeted is not a budget, so [`OutputBudget`] is a caller-passed
//! parameter refused at zero — the same shape `zaru-core`'s
//! `TruncationBudget` and [`credentials::Ttl`] already use in this workspace.
//!
//! # A truncation whose full text nobody can retrieve is refused
//!
//! D5's promise is that the whole output survives somewhere the user can
//! reach. The session directory that would hold it is [ADR-0010] D1's and is
//! unbuilt, so [`Overflow`] is a port with no implementation here — and
//! output that exceeds the budget with no sink to preserve it is **refused**
//! rather than quietly clipped.
//!
//! That is the same refusal, for the same reason, that this crate already
//! makes for ADR-0007 D8's apex confirmation: a promise nobody can keep is
//! the silent default the record exists to prevent. Clipping output and
//! showing a path that does not exist would be exactly the unnoticeable
//! truncation D5 names.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`credentials::Ttl`]: crate::credentials::Ttl

use core::fmt;
use std::path::PathBuf;

/// A budget the caller passed that cannot bound anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetIsZero;

impl fmt::Display for BudgetIsZero {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "an output budget of zero is refused; ADR-0011 D5 requires truncation to keep the \
             head and the tail of the output and mark the elision, and zero bytes can keep \
             neither, so what a caller would be shown carries nothing of what happened",
        )
    }
}

impl std::error::Error for BudgetIsZero {}

/// How many bytes of one stream a caller is shown before it is truncated.
///
/// Applied to standard output and standard error independently, so one number
/// bounds each stream rather than two numbers nobody has chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputBudget(usize);

impl OutputBudget {
    /// Take a budget from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`BudgetIsZero`] when `bytes` is zero.
    pub const fn new(bytes: usize) -> Result<Self, BudgetIsZero> {
        if bytes == 0 {
            return Err(BudgetIsZero);
        }
        Ok(Self(bytes))
    }

    /// The budget in bytes.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// What one tool call produced. ADR-0011 D5's three parts.
///
/// The two streams are separate fields and are never merged: D5 says
/// "captured separately, both surfaced", and a merged capture is one a reader
/// cannot take apart again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The exit code the call reported.
    pub exit_code: i32,
    /// Everything the call wrote to standard output.
    pub stdout: String,
    /// Everything the call wrote to standard error.
    pub stderr: String,
}

/// One stream as a caller is shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excerpt {
    text: String,
    elided: Option<usize>,
}

impl Excerpt {
    /// The text, head and tail kept and the elision marked where there was
    /// one.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// How many bytes were dropped, if any were.
    ///
    /// `None` and `Some(0)` are different answers and only the first is
    /// reachable: nothing is marked as elided unless something was.
    #[must_use]
    pub const fn elided_bytes(&self) -> Option<usize> {
        self.elided
    }

    /// Whether anything was dropped.
    #[must_use]
    pub const fn was_truncated(&self) -> bool {
        self.elided.is_some()
    }
}

/// Where the full text of an oversized capture goes.
///
/// ADR-0011 D5: "the full text written to the session directory, with the
/// path shown". The session directory is [ADR-0010] D1's
/// `~/.zaru/sessions/<ulid>/` and does not exist, so **nothing in this
/// crate's product tree implements this**.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub trait Overflow {
    /// Preserve the whole capture and say where it went.
    ///
    /// # Errors
    ///
    /// [`OverflowFailure`] when the implementation could not preserve it,
    /// carrying its own wording.
    fn preserve(&mut self, captured: &Captured) -> Result<PathBuf, OverflowFailure>;
}

/// The full text could not be preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverflowFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl OverflowFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for OverflowFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for OverflowFailure {}

/// Why a capture could not be presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationRefused {
    /// The output exceeded the budget and there was nowhere to keep the rest.
    ///
    /// See the module documentation: clipping output while promising the full
    /// text is somewhere is the unnoticeable truncation D5 exists to prevent.
    ThereWasNowhereToKeepTheRest {
        /// How many bytes of the two streams together did not fit.
        elided_bytes: usize,
    },
    /// The overflow sink could not preserve it.
    NotPreserved(OverflowFailure),
}

impl fmt::Display for PresentationRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThereWasNowhereToKeepTheRest { elided_bytes } => write!(
                f,
                "{elided_bytes} byte(s) of output exceed the budget and no overflow sink was \
                 supplied, so the call was refused rather than clipped. ADR-0011 D5 requires the \
                 full text be written where the user can read it and the path shown, and \
                 \"a truncation the user cannot notice is how a diagnosis gets built on a \
                 fragment\". The session directory that would hold it is ADR-0010 D1's and is \
                 not built"
            ),
            Self::NotPreserved(failure) => write!(
                f,
                "the output exceeded the budget and the full text could not be preserved: \
                 {failure}"
            ),
        }
    }
}

impl std::error::Error for PresentationRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotPreserved(failure) => Some(failure),
            Self::ThereWasNowhereToKeepTheRest { .. } => None,
        }
    }
}

/// What a caller is shown of one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presented {
    /// The exit code, unaltered.
    pub exit_code: i32,
    /// Standard output, truncated if it had to be.
    pub stdout: Excerpt,
    /// Standard error, truncated if it had to be.
    pub stderr: Excerpt,
    /// Where the whole capture was preserved, when anything was elided.
    ///
    /// D5's "with the path shown"; rendering it is `zaru-tui`'s.
    pub full_text_at: Option<PathBuf>,
}

/// The single point on the path from a tool's captured output to its caller.
///
/// It is the identity and it does nothing. [ADR-0008]'s trigger clause 6 — a
/// decision must exist for secret redaction in failure text — is deliberately
/// open, and `zaru-core`'s refinement construction already carries the first
/// such point, on the path from a validator's output to the model's prompt.
/// **This is the second**, on the path from a tool's output to whatever
/// consumes it, and it exists so that a redaction decision, when it is made,
/// has two named places to attach rather than a scattering of filters.
///
/// It is not a hook and takes no policy: nothing may pass behaviour through
/// it, because a configurable redaction point would be the decision itself,
/// settled in code. ADR-0011 D5 sends both streams to the model, so the
/// obligation clause 6 names is exactly as live here as it is there.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
const fn tool_output_for_the_caller(raw: &str) -> &str {
    raw
}

impl Captured {
    /// Show this capture to a caller within the budget.
    ///
    /// # Errors
    ///
    /// [`PresentationRefused::ThereWasNowhereToKeepTheRest`] when either
    /// stream exceeds the budget and `overflow` is `None`.
    ///
    /// [`PresentationRefused::NotPreserved`] when the sink refused.
    pub fn present(
        &self,
        budget: OutputBudget,
        overflow: Option<&mut dyn Overflow>,
    ) -> Result<Presented, PresentationRefused> {
        let stdout = excerpt(tool_output_for_the_caller(&self.stdout), budget);
        let stderr = excerpt(tool_output_for_the_caller(&self.stderr), budget);

        let elided_bytes = stdout.elided.unwrap_or_default() + stderr.elided.unwrap_or_default();
        let full_text_at = if elided_bytes == 0 {
            None
        } else {
            let Some(overflow) = overflow else {
                return Err(PresentationRefused::ThereWasNowhereToKeepTheRest { elided_bytes });
            };
            Some(
                overflow
                    .preserve(self)
                    .map_err(PresentationRefused::NotPreserved)?,
            )
        };

        Ok(Presented {
            exit_code: self.exit_code,
            stdout,
            stderr,
            full_text_at,
        })
    }
}

/// How an elision is marked, wherever one is marked.
///
/// Named rather than written inline, because a check asserts its presence as
/// well as the absence of what was dropped — and a marker written twice is a
/// marker that will one day read two ways.
pub const ELISION_PREFIX: &str = "[... ";

/// Keep the head and the tail of `text` and mark what was dropped.
///
/// Text that fits is returned byte-for-byte with no marker: an elision marker
/// on text that was not elided is a lie a reader cannot tell from a
/// truncation. When it does not fit, the kept head and tail together are at
/// most `budget` bytes and the marker is additional, so the marker can always
/// say how much went. Both cuts fall on character boundaries.
fn excerpt(text: &str, budget: OutputBudget) -> Excerpt {
    let budget = budget.get();
    if text.len() <= budget {
        return Excerpt {
            text: text.to_owned(),
            elided: None,
        };
    }

    let head_end = floor_boundary(text, budget.div_ceil(2));
    let tail_start = ceil_boundary(text, text.len() - (budget - budget.div_ceil(2)));
    let elided = tail_start - head_end;

    Excerpt {
        text: format!(
            "{}\n{ELISION_PREFIX}{elided} bytes elided ...]\n{}",
            &text[..head_end],
            &text[tail_start..]
        ),
        elided: Some(elided),
    }
}

/// The largest index at or below `at` that is a character boundary.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut i = at.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The smallest index at or above `at` that is a character boundary.
fn ceil_boundary(text: &str, at: usize) -> usize {
    let mut i = at.min(text.len());
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}
