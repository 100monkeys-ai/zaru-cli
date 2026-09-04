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
//! pair [`Phase::Started`](crate::session::Phase::Started) before the call and
//! [`Phase::Completed`](crate::session::Phase::Completed) after it, and a
//! `Started` with no matching `Completed` **is** the interruption. D4 leaves
//! the two framings open; the reader's end is the only one a killed process
//! can honour.
//!
//! # Telling the model is not this module's, and it is a stop
//!
//! D4's second half is "the model is told it did not complete". The model is
//! reached through `zaru_core::iteration::ContextPolicy::assemble`, which
//! takes a `Turn` with exactly two variants, `Initial` and `Refinement`.
//! **Neither can carry an interruption.** A third variant is a change to
//! `zaru-core`'s public contract and belongs to whoever builds the tool-call
//! loop. This module produces the datum and stops at the seam; it is recorded
//! as an open question on ADR-0008 and on `operations/adr-status`.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::session::checkpoint::{Checkpoint, CheckpointError};
use crate::session::record::{Phase, Record, ToolCall};
use crate::session::store::{CHECKPOINT_FILE, TRANSCRIPT_FILE};
use crate::session::transcript::{Transcript, TranscriptError};
use core::fmt;
use std::path::Path;

/// A tool call that was in flight when the process died.
///
/// **Not re-executed, and not re-executable from here**: this is a value, and
/// the function that produced it holds no port that could run anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interrupted {
    /// The call, as ADR-0011 D4 rendered it into the transcript.
    pub call: ToolCall,
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
        interrupted: unfinished_call(&reading.records),
        tail: reading.tail(tail).to_vec(),
        fragment: reading.fragment,
        checkpoint,
    })
}

/// The call that started and never completed, if there is one.
///
/// One in flight at a time, which is what ADR-0011's tool-call loop describes:
/// the model requests a tool, the harness executes it, the result returns. A
/// `Completed` clears whatever was pending, so a session that ran a hundred
/// calls and finished them all has nothing pending at the end.
fn unfinished_call(records: &[Record]) -> Option<Interrupted> {
    let mut pending: Option<&ToolCall> = None;
    for record in records {
        if let Record::ToolCall(call) = record {
            match call.phase {
                Phase::Started => pending = Some(call),
                Phase::Completed => pending = None,
            }
        }
    }
    pending.map(|call| Interrupted { call: call.clone() })
}
