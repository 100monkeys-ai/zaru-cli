// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What one line of [ADR-0010] D2's transcript is.
//!
//! D2: "Every event from [ADR-0008] D3 is written as it occurs, alongside
//! user messages, tool calls, SEAL verdicts, attachments, and learning
//! announcements."
//!
//! # A producer is a variant, never a string
//!
//! Eight producers are named above and **three exist in this workspace**:
//! `zaru-core`'s [`Event`], [ADR-0011] D4's transcript entry, and
//! [ADR-0016] D1's five classes. Each is a variant of [`Record`], so a fourth
//! producer is a variant and every match over the enum fails to compile
//! rather than a `kind` string being invented at a call site — the same
//! closed-enum discipline [`Class`](crate::failure::Class),
//! [`Layer`](crate::config::Layer) and [`ToolName`](crate::tools::ToolName)
//! already carry in this crate.
//!
//! **The five producers that do not exist get no variant at all.** User
//! messages, SEAL verdicts, attachments and learning announcements belong to
//! records that are unbuilt, and a variant whose condition nothing can
//! satisfy is a permanent exemption dressed as a promise
//! ([Verification lessons] §7). Absence is what makes the gap findable.
//!
//! # The tool call is stored as its rendered line, and that is D2's own claim
//!
//! [`TranscriptEntry`] has private fields and
//! derives no `Serialize`, and **nothing in `tools/` is changed here**. What
//! is stored is what that type's own public door yields: `render()` — which
//! ADR-0011 D4 calls "the line a transcript shows", produced in exactly one
//! place so the prompt and the record cannot describe one call two ways —
//! together with D4's out-of-tree class and D6's destructive annotation.
//!
//! That is the better fit rather than the concession. D2's replayability
//! claim is that "re-rendering it reproduces what the user saw", and
//! `render()`'s output **is** what the user saw. A checkpoint would need the
//! struct; a transcript needs the line.
//!
//! # A tool call is two records, and that is what makes `Interrupted` possible
//!
//! [ADR-0010] D4 requires an interrupted call to be recorded as `Interrupted`.
//! **A process that has been killed writes nothing**, so that record cannot
//! be appended at the moment of interruption. What is appended is the pair: a
//! [`Phase::Started`] before the call and a [`Phase::Completed`] after it, and
//! on resume a `Started` with no matching `Completed` **is** the
//! interruption. D4 leaves the two framings open and they are the same
//! mechanism from two ends; the reader's end is the only one a killed process
//! can honour.
//!
//! The phase sits beside the entry's data rather than inside it, so D4's "the
//! record is the same at every mode" is undisturbed — a phase is not a mode.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::failure::{Classified, Presentation};
use crate::tools::TranscriptEntry;
use serde::{Deserialize, Serialize};
use zaru_core::iteration::Event;

/// Where a tool call had got to when this line was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The harness is about to act. Written **before** the call.
    Started,
    /// The call returned. Written after it.
    Completed,
}

/// [ADR-0011] D4's record of one tool call, as a transcript keeps it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// ADR-0011 D4's line, exactly as the prompt rendered it.
    pub line: String,
    /// D4's out-of-tree class, which renders differently at every mode.
    pub out_of_tree: bool,
    /// D6's destructive annotation.
    pub destructive: bool,
    /// Whether the call had returned when this line was written.
    pub phase: Phase,
}

impl ToolCall {
    /// The line written **before** the call acts.
    #[must_use]
    pub fn started(entry: &TranscriptEntry) -> Self {
        Self::at(entry, Phase::Started)
    }

    /// The line written after it returns.
    #[must_use]
    pub fn completed(entry: &TranscriptEntry) -> Self {
        Self::at(entry, Phase::Completed)
    }

    fn at(entry: &TranscriptEntry, phase: Phase) -> Self {
        Self {
            line: entry.render(),
            out_of_tree: entry.is_out_of_tree(),
            destructive: entry.is_destructive(),
            phase,
        }
    }
}

/// [ADR-0016] D1's classified failure, as a transcript keeps it.
///
/// Stored as the projection rather than as the `Classified` itself, for the
/// reason the tool call is stored as its line: a transcript is a record of
/// what the user was shown, and
/// [`Presentation`] is what a renderer shows.
/// It also means the transcript carries no `PathBuf` and no `Tier`, so the
/// stored shape does not move when those types do.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureLine {
    /// Which of D1's five this was, as that record spells it.
    pub class: String,
    /// The one line saying what happened.
    pub headline: String,
    /// Everything under it.
    pub lines: Vec<String>,
}

impl FailureLine {
    /// Project a classified failure into what a transcript keeps.
    #[must_use]
    pub fn of(classified: &Classified) -> Self {
        let presentation = Presentation::of(classified);
        Self {
            class: presentation.class.as_str().to_owned(),
            headline: presentation.headline,
            lines: presentation
                .lines
                .into_iter()
                .map(|line| match line.lead {
                    Some(lead) => format!("{lead} {}", line.text),
                    None => line.text,
                })
                .collect(),
        }
    }
}

/// One line of [ADR-0010] D2's transcript.
///
/// **Externally tagged**, so a line is one JSON object whose single key names
/// the producer it came from and a reader can tell them apart without a
/// convention.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// ADR-0008 D3's event stream, which D2 says the transcript is.
    Loop(Event),
    /// ADR-0011 D4's record, which no permission mode removes.
    ToolCall(ToolCall),
    /// ADR-0016 D1's classified failure, as it was presented.
    Failure(FailureLine),
}

impl Record {
    /// Which producer this line came from.
    ///
    /// A total function over the enum with **no wildcard arm**, so a fourth
    /// producer cannot arrive without a name being chosen for it here.
    #[must_use]
    pub const fn producer(&self) -> &'static str {
        match self {
            Self::Loop(_) => "loop",
            Self::ToolCall(_) => "tool_call",
            Self::Failure(_) => "failure",
        }
    }
}
