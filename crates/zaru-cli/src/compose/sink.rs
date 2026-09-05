// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The turn's event stream reaching [ADR-0010] D2's transcript, as it occurs.
//!
//! # This is the second consumer [ADR-0008] clause 3 has been waiting for
//!
//! Clause 3: "The event stream is consumed by both the terminal renderer and
//! the transcript writer, **from one emission**." Both consumers have existed
//! since the `tui-shell` arc, and that arc said exactly why the clause still
//! could not move: "**one emission needs a loop**, and no loop runs anywhere
//! in a product path. What the renderer is handed is a staged transcript … the
//! arc that wires a provider client into the loop is the one that can."
//!
//! `tool_call::run` takes `sinks: &mut [&mut dyn EventSink]`, constructs each
//! event once and hands the same value to every sink in turn — which is the
//! property the loop's own `emit` documents. So a slice holding this and the
//! shell's source is one emission reaching two consumers, and the clause is
//! about the caller rather than about either consumer.
//!
//! # Two handles on one file, and why that is one writer rather than two
//!
//! [`crate::tools::Executor`] holds `&mut Transcript` for the whole run,
//! because [ADR-0011] D4 has it write the started-and-completed pair around
//! every call. This sink needs the same file at the same time, and one
//! `&mut` cannot be two.
//!
//! So it opens its own [`Transcript`] on the same path. That is **one writer
//! type with two handles**, not two writers with two rules: every record still
//! leaves through `Transcript::record`, which serialises it, appends the line
//! and its newline in one `write_all` to a file opened `O_APPEND`, flushes and
//! syncs. The turn runs on one thread, so the two handles never write at the
//! same instant and the interleaving on disk is the real chronological
//! order — which is what D2's "written as it occurs" means, and what makes
//! re-rendering reproduce what the user saw.
//!
//! The alternative was buffering the events and writing them after the run,
//! and it is worse in the one way that matters: every turn-loop event would
//! land after every tool-call record, so the file would say the tools ran
//! before the model asked for them.
//!
//! # A sink cannot fail, so the failure is kept for the caller
//!
//! [`EventSink::emit`] returns `()`:
//! the loop's contract has nowhere to put a disk error and should not grow
//! one. A transcript write can fail. So the first failure is kept and the
//! caller reads it after the run — **the first rather than the last**, because
//! the first is the one that says where the record stopped being complete, and
//! every one after it is a consequence.
//!
//! Losing it silently would be the worst available outcome: [ADR-0010] D2
//! makes this file the replayable record, and a transcript that is missing
//! events without saying so is one nobody can trust afterwards.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::session::{Record, Transcript, TranscriptError};
use zaru_core::tool_call::{Event, EventSink};

/// Writes [ADR-0008] D1's outer loop's events into [ADR-0010] D2's transcript.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug)]
pub struct Records {
    transcript: Transcript,
    first_failure: Option<TranscriptError>,
    written: usize,
}

impl Records {
    /// Open a second handle on a session's transcript, for the turn's events.
    ///
    /// # Errors
    ///
    /// [`TranscriptError::Io`] when the file cannot be opened for appending.
    pub fn appending_to(path: impl Into<std::path::PathBuf>) -> Result<Self, TranscriptError> {
        Ok(Self {
            transcript: Transcript::append_to(path)?,
            first_failure: None,
            written: 0,
        })
    }

    /// How many events reached the file.
    ///
    /// A caller asserts this against what it staged rather than against a
    /// second reading taken through the loop.
    #[must_use]
    pub const fn written(&self) -> usize {
        self.written
    }

    /// The first write that failed, if one did.
    ///
    /// A caller reads this **after** the run and reports it: a turn whose
    /// transcript is incomplete has not recorded what happened, whatever the
    /// loop returned.
    #[must_use]
    pub const fn first_failure(&self) -> Option<&TranscriptError> {
        self.first_failure.as_ref()
    }
}

impl EventSink for Records {
    /// Append one event, and keep the first failure rather than losing it.
    ///
    /// A `Record::TurnLoop`, which is the fourth producer
    /// [`Record`] declares — a variant rather than a
    /// widening of `Record::Loop`, because ADR-0008 D1 makes the two loops
    /// different loops and D3's eight events are all iteration-shaped.
    fn emit(&mut self, event: &Event) {
        match self.transcript.record(&Record::TurnLoop(event.clone())) {
            Ok(()) => self.written += 1,
            Err(failure) => {
                if self.first_failure.is_none() {
                    self.first_failure = Some(failure);
                }
            }
        }
    }
}
