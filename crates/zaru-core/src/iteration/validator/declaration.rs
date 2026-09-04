// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One `[[validator]]` block, as data.
//!
//! [ADR-0009] D1's example gives the four fields this carries:
//!
//! ```toml
//! [[validator]]
//! name = "test"
//! run  = "cargo test --all"
//! expect = "exit-zero"
//! after = ["build"]
//! ```
//!
//! A [`Declared`] is built by a caller and never parsed here. The reader of
//! the file is `zaru-cli`'s, because [ADR-0009] D1's `zaru.toml` is also
//! [ADR-0014] D1's layer 3 and configuration is that crate's — one file, one
//! reader.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::iteration::validator::expectation::Expect;
use crate::iteration::validator::name::{Name, Run};

/// One declared validator.
///
/// `after` is a list rather than a single name because [ADR-0009] D1 writes it
/// as one — `after = ["build"]` — and a validator with two prerequisites is
/// the ordinary case the moment a third validator exists.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    /// The validator's declared name, which `after` refers to it by.
    pub name: Name,
    /// The command that is run to decide it.
    pub run: Run,
    /// What [ADR-0009] D3 kind decides whether it passed.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    pub expect: Expect,
    /// The validators this one declares as prerequisites.
    pub after: Vec<Name>,
}

impl Declared {
    /// Declare a validator with no prerequisites.
    #[must_use]
    pub fn new(name: Name, run: Run, expect: Expect) -> Self {
        Self {
            name,
            run,
            expect,
            after: Vec::new(),
        }
    }

    /// Declare the prerequisites this validator comes after.
    #[must_use]
    pub fn after(mut self, prerequisites: impl IntoIterator<Item = Name>) -> Self {
        self.after = prerequisites.into_iter().collect();
        self
    }
}
