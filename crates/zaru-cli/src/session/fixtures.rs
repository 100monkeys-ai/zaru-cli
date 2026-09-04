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
