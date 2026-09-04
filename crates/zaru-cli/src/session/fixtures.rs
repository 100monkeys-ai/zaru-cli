// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the session checks are built from. Compiled only under `cfg(test)`.
//!
//! # The scratch tree and the nonces come from the credential store's fixtures
//!
//! [`crate::credentials::fixtures`] already carries the awkward nonce, the
//! ASCII core, and a scratch root with the sibling control that discriminates
//! a real deletion from a checker that reports absence for everything. The
//! reason the ASCII core exists is a mutation that survived there on
//! 2026-09-04. Retyping any of it here would put one rule in two places, and
//! the configuration checks already re-use it for exactly this reason.

use crate::session::id::{Millis, SessionId, WallClock};
use crate::session::meta::{Meta, MetaFailure, MetaStore};
use std::cell::RefCell;

pub(crate) use crate::credentials::fixtures::{ScratchRoot, ascii_core, nonce};

/// A clock a check sets, so an assertion about ordering is about the encoding
/// rather than about how fast the machine ran.
#[derive(Debug)]
pub(crate) struct StagedClock {
    reading: RefCell<u64>,
}

impl StagedClock {
    /// A clock reading `millis`.
    pub(crate) const fn at(millis: u64) -> Self {
        Self {
            reading: RefCell::new(millis),
        }
    }

    /// Move it forward.
    pub(crate) fn advance(&self, by: u64) {
        *self.reading.borrow_mut() += by;
    }
}

impl WallClock for StagedClock {
    fn now(&self) -> Millis {
        Millis::new(*self.reading.borrow())
    }
}

/// Eighty bits a check chose, so that two ids differ for a reason the check
/// can name.
pub(crate) const fn entropy(seed: u8) -> [u8; 10] {
    [seed, 1, 2, 3, 4, 5, 6, 7, 8, 9]
}

/// An id at a named millisecond, for a check that knows its own inputs.
pub(crate) fn id_at(millis: u64, seed: u8) -> SessionId {
    SessionId::from_parts(Millis::new(millis), entropy(seed))
        .expect("a fixture millisecond fits in 48 bits")
}

/// A `meta.toml` that is held in memory rather than written.
///
/// **This is a test double and it is not evidence about `meta.toml`.**
/// [Verification lessons] §24: "A test double answering more simply than the
/// real thing is where a defect becomes invisible." What a check may conclude
/// from this is that the session hands the port a [`Meta`] carrying what
/// [ADR-0010] D1 says it records — nothing whatever about TOML, which is not
/// written by anything in this workspace.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[derive(Debug, Default)]
pub(crate) struct InMemoryMeta {
    held: RefCell<Option<Meta>>,
}

impl MetaStore for InMemoryMeta {
    fn write(&mut self, meta: &Meta) -> Result<(), MetaFailure> {
        *self.held.borrow_mut() = Some(meta.clone());
        Ok(())
    }

    fn read(&self) -> Result<Meta, MetaFailure> {
        self.held
            .borrow()
            .clone()
            .ok_or_else(|| MetaFailure::new("nothing has been recorded for this session"))
    }
}

/// A transcript entry, built through ADR-0011's own permission decision
/// rather than constructed here.
///
/// [`TranscriptEntry`](crate::tools::TranscriptEntry) has private fields and
/// no public constructor, deliberately — the record is what a `Decision`
/// produced. So this fixture drives the real decision, which is also what
/// makes the stored line the one ADR-0011 D4 renders rather than a second
/// spelling of it.
pub(crate) fn entry_for(
    working: &crate::tools::WorkingDirectory,
    candidate: &str,
    destructive: bool,
) -> crate::tools::TranscriptEntry {
    let target = working.classify(candidate);
    let invocation = crate::tools::Invocation::on_path(crate::tools::ToolName::FsWrite, &target)
        .expect("fs.write addresses a path");
    crate::tools::Decision::reach(
        crate::tools::Mode::Yolo,
        &invocation,
        crate::tools::Assessment {
            allowlisted: false,
            destructive,
        },
    )
    .entry()
    .clone()
}

/// An ADR-0008 D3 event carrying a sequence number and a payload of a chosen
/// size.
///
/// `ValidatorEvaluated` is used because it is the one event with both a field
/// a check can count on and a field a check can make long — and a long line
/// is what makes a torn write visible at all. Nothing is invented for the
/// test: this is the product's own event shape.
pub(crate) fn sequenced_event(seq: u64, payload_bytes: usize) -> zaru_core::iteration::Event {
    zaru_core::iteration::Event::ValidatorEvaluated {
        name: seq.to_string(),
        outcome: zaru_core::iteration::ValidatorOutcome::Failed,
        detail: "x".repeat(payload_bytes),
    }
}

/// The sequence number a [`sequenced_event`] record carries, if it is one.
pub(crate) fn sequence_of(record: &crate::session::Record) -> Option<u64> {
    match record {
        crate::session::Record::Loop(zaru_core::iteration::Event::ValidatorEvaluated {
            name,
            ..
        }) => name.parse().ok(),
        _ => None,
    }
}

/// The environment variable the kill check's child half reads.
pub(crate) const KILL_CHILD_TRANSCRIPT: &str = "ZARU_SESSION_KILL_CHILD_TRANSCRIPT";

/// How many bytes of payload each line the child appends carries.
///
/// Chosen so that a line does not divide a `BufWriter`'s default 8 KiB
/// buffer: with a buffer that flushes on its own boundary, the boundary then
/// falls inside a line and the tear is visible rather than lucky.
pub(crate) const KILL_LINE_PAYLOAD: usize = 200;
