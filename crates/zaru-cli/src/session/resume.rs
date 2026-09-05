// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D4's resume: a pure function from a directory, and the one
//! thing it must never do.
//!
//! D4: "**Resume never re-runs a tool call.** A resumed session that
//! re-executes the last action because it was in flight when the process died
//! is a session that can delete a file twice. An interrupted tool call is
//! recorded as `Interrupted` and the model is told it did not complete."
//!
//! # "Never re-executes" is held by the signature, not by a check
//!
//! [`resume`] takes a path and a number. **It takes no ports at all**, so
//! there is nothing it could invoke: no `Executor`, no `Generator`, no
//! `Confirm`, no `Allowlist`. That is the same shape ADR-0014's `Resolution`
//! uses for its own immutability clause — "no method taking `&mut self`, no
//! setter, no `reload`" — and it is stronger than a check, because a check
//! asserting that a port was not called can only assert it about the ports
//! somebody remembered to pass in.
//!
//! A check that registered a counting `Executor` and asserted zero calls
//! would be theatre: this function has nowhere to put one. What is asserted
//! instead is the signature's consequence — that resuming a session whose
//! last record is an unfinished tool call produces a *marker* and changes
//! nothing on disk.
//!
//! # `Interrupted` is derived, and that is forced rather than chosen
//!
//! A process that has been killed writes nothing, so `Interrupted` cannot be
//! a record appended at the moment of interruption. What is appended is the
//! pair [`Phase::Started`] before the call and
//! [`Phase::Completed`] after it, and a
//! `Started` with no matching `Completed` **is** the interruption. D4 leaves
//! the two framings open; the reader's end is the only one a killed process
//! can honour.
//!
//! # Telling the model, which now has a carrier
//!
//! D4's second half is "the model is told it did not complete". The model is
//! reached through [`ContextPolicy::assemble`], which takes a
//! [`Turn`](zaru_core::iteration::Turn) — and when this module was written
//! that type had two variants, `Initial` and `Refinement`, neither of which
//! could carry an interruption. It has three now:
//! [`Turn::Resumed`](zaru_core::iteration::Turn::Resumed), added by the
//! tool-call loop, carrying a
//! [`Interruption`](zaru_core::iteration::Interruption) built from this
//! module's own [`Interrupted::call`]'s rendered line.
//!
//! **This module still tells nobody anything.** It produces the datum, as it
//! always did; the conversion and the turn are the caller's, and
//! `crates/zaru-cli/tests/tool_execution_from_outside.rs` drives it end to
//! end. The open question ADR-0008's Status tracking raised is answered.
//!
//! [`ContextPolicy::assemble`]: zaru_core::iteration::ContextPolicy::assemble
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::session::checkpoint::{Checkpoint, CheckpointError};
use crate::session::record::{Phase, Record, ToolCall};
use crate::session::store::{CHECKPOINT_FILE, TRANSCRIPT_FILE};
use crate::session::transcript::{Transcript, TranscriptError};
use core::fmt;
use std::path::Path;
use zaru_core::redaction::{Redacted, Redactor};

/// A tool call that was in flight when the process died.
///
/// **Not re-executed, and not re-executable from here**: this is a value, and
/// the function that produced it holds no port that could run anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interrupted {
    /// The call, as ADR-0011 D4 rendered it into the transcript.
    pub call: ToolCall,
}

impl Interrupted {
    /// The datum the model is told, as `zaru-core` carries it.
    ///
    /// ADR-0010 D4's second half. The line and nothing else, because
    /// ADR-0011 D4 calls `render()`'s output "the line a transcript shows"
    /// and D2's replayability claim is that re-rendering it reproduces what
    /// the user saw — so handing the model anything else would be a second
    /// description of one call.
    ///
    /// It passes ADR-0008 clause 6's port, which was a coordinator ruling of
    /// 2026-09-05 rather than one of the three paths that decision names: a
    /// rendered `cmd.run` line **is** a command line, and that is where a
    /// `--token=` argument lives. **`self.call.line` is untouched**, so the
    /// transcript this was read out of still carries the raw line, which is
    /// ADR-0010's rule and is asserted.
    #[must_use]
    pub fn for_the_model<R: Redactor + ?Sized>(
        &self,
        redactor: &R,
    ) -> zaru_core::iteration::Interruption {
        zaru_core::iteration::Interruption::of(Redacted::by(redactor, &self.call.line))
    }
}

/// A session could not be resumed.
#[derive(Debug)]
pub enum ResumeFailure {
    /// The directory is not there.
    NoSuchDirectory {
        /// Which directory.
        path: std::path::PathBuf,
    },
    /// The transcript could not be read.
    Transcript(TranscriptError),
    /// The checkpoint could not be read.
    Checkpoint(CheckpointError),
}

impl fmt::Display for ResumeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchDirectory { path } => write!(
                f,
                "there is no session directory at {}, so there is nothing to restore",
                path.display()
            ),
            Self::Transcript(failure) => write!(f, "{failure}"),
            Self::Checkpoint(failure) => write!(f, "{failure}"),
        }
    }
}

impl std::error::Error for ResumeFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transcript(failure) => Some(failure),
            Self::Checkpoint(failure) => Some(failure),
            Self::NoSuchDirectory { .. } => None,
        }
    }
}

/// What a resumed session restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumed {
    /// D3's checkpoint, or `None` for a session that never wrote one.
    pub checkpoint: Option<serde_json::Value>,
    /// The tail of the transcript, which D4 says a resume re-renders.
    ///
    /// **This module renders nothing.** `zaru-tui` is the renderer and it
    /// depends only on `zaru-core`, so it reaches this through a port its own
    /// crate declares — the dependency inversion ADR-0005's composer uses.
    pub tail: Vec<Record>,
    /// The same lines as the bytes they are on disk.
    ///
    /// What the out-of-session `--resume` prints, because D1's promise is that
    /// the user can read every byte the harness stored with `cat` and the
    /// honest way to show a transcript is to show the file. D4's *re-render*
    /// is the terminal's and is a different act.
    pub tail_lines: Vec<String>,
    /// How many turns this session has already had.
    ///
    /// **The greatest `n` any `turn_started` record carries, and zero when
    /// there is none** — so the next turn is this plus one. See
    /// [`turns_so_far`] for why it is the greatest rather than the count, and
    /// [ADR-0010] D4's accepted Update of 2026-09-05 for the decision.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub turns: u32,
    /// A call that was in flight when the process died, if one was.
    pub interrupted: Option<Interrupted>,
    /// How many bytes of a partial line the transcript ends with, if any.
    ///
    /// D2's "at most the event in flight", surfaced rather than hidden: a
    /// resumed session that silently dropped it would be the invisible
    /// truncation ADR-0011 D5 forbids one layer up.
    pub fragment: Option<usize>,
}

/// Restore a session from its directory. **Executes nothing.**
///
/// `tail` is how many records D4's re-render wants. It is a parameter because
/// no record names a number, and a number invented by the thing it bounds is
/// not a number anybody chose — the shape `zaru-core`'s `Ceiling`, ADR-0007's
/// `Ttl` and ADR-0011's truncation budget already use. Zero is meaningful
/// here and is not refused: a caller that wants the checkpoint and no
/// re-render asks for no records.
///
/// # Errors
///
/// [`ResumeFailure::NoSuchDirectory`], [`ResumeFailure::Transcript`] and
/// [`ResumeFailure::Checkpoint`].
pub fn resume(directory: &Path, tail: usize) -> Result<Resumed, ResumeFailure> {
    if !directory.is_dir() {
        return Err(ResumeFailure::NoSuchDirectory {
            path: directory.to_path_buf(),
        });
    }

    let reading =
        Transcript::read(&directory.join(TRANSCRIPT_FILE)).map_err(ResumeFailure::Transcript)?;
    let checkpoint = Checkpoint::at(directory.join(CHECKPOINT_FILE))
        .read()
        .map_err(ResumeFailure::Checkpoint)?;

    Ok(Resumed {
        // Over every record rather than over the tail: `tail` is how many the
        // caller wants re-rendered, and a session's turn count is not a
        // property of how much of it somebody asked to see.
        turns: turns_so_far(&reading.records),
        interrupted: unfinished_call(&reading.records),
        tail: reading.tail(tail).to_vec(),
        tail_lines: reading.tail_lines(tail).to_vec(),
        fragment: reading.fragment,
        checkpoint,
    })
}

/// How many turns this session has had, read off the transcript.
///
/// # The disk does say, and until 2026-09-05 nothing read it
///
/// [ADR-0010]'s amendments page carried this from the `shell-task-turns` arc:
/// "**nothing on disk says how many turns a session has already had** — the
/// transcript holds the events but no arc has decided that counting them is
/// the answer. So a session resumed a second time numbers its turns from one
/// again, and the transcript shows two `turn_started {"n": 1}` records for one
/// session."
///
/// **Half of that is false and the other half is why this function exists.**
/// [ADR-0008] D1's outer loop emits `TurnStarted { n, of }`, D2 puts every
/// event of it in this file, and `n` is the turn's own number — so the disk
/// says it in as many words. What was true is that no record had decided that
/// *reading* it is the answer. That is now decided, as an accepted Update to
/// D4 under directive 20 of 2026-09-05, open to Jeshua's veto: **the next turn
/// is one more than the greatest `n` any `turn_started` record carries, and
/// one when there is none.**
///
/// # The greatest rather than the count, the last, or one
///
/// The four rules give four different answers and only on a file this harness
/// wrote do three of them agree. The **count** re-derives a number the file
/// already states, which is a proxy for it rather than the thing; on a
/// transcript truncated by a crash, or trimmed by the user D1 promises can
/// read and edit these files, it is short by exactly the records that went
/// missing and the next turn reuses a number the file still holds. The
/// **last** is whatever record happens to be last, which on an out-of-order
/// or interleaved file is not the highest. And **one** is the behaviour this
/// replaces. `a_resumed_session_continues_the_turn_count_from_the_transcript`
/// stages `n` of 1, 5 and 2 in that order so that all four disagree.
///
/// **Not the restored exchange count either**, which is the fifth rule and the
/// one that looks cheapest from `compose::boundary`: `crate::terminal::driver`
/// records no exchange for an interrupted turn, so one interrupt makes the
/// next turn reuse a number the transcript already holds — the exact oddity
/// this closes.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn turns_so_far(records: &[Record]) -> u32 {
    records
        .iter()
        .filter_map(|record| match record {
            Record::TurnLoop(zaru_core::tool_call::Event::TurnStarted { n, .. }) => Some(*n),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// The call that started and never finished, if there is one.
///
/// One in flight at a time, which is what ADR-0011's tool-call loop describes:
/// the model requests a tool, the harness executes it, the result returns.
///
/// **A `Started` with no matching `Completed` *or* `Refused`** is the
/// interruption. Both of the latter close the pair, and for the same reason:
/// the process was alive to write them. A refused call is one the user
/// consciously declined, and reporting it to the model as an action that did
/// not complete would tell the model the opposite of what happened —
/// ADR-0016's ruling of 2026-09-04 is that a refusal is not a failure, and it
/// is not an interruption either.
fn unfinished_call(records: &[Record]) -> Option<Interrupted> {
    let mut pending: Option<&ToolCall> = None;
    for record in records {
        if let Record::ToolCall(call) = record {
            match call.phase {
                Phase::Started => pending = Some(call),
                Phase::Completed | Phase::Refused => pending = None,
            }
        }
    }
    pending.map(|call| Interrupted { call: call.clone() })
}
