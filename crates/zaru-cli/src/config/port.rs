// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a layer's document comes from, for the three layers that cannot be
//! read yet.
//!
//! **Nothing in this crate's product tree implements [`LayerSource`]**,
//! exactly as nothing implements the credential store's
//! [`SecretStore`](crate::credentials::SecretStore) or any of `zaru-core`'s
//! five loop ports. A check implements it; the product does not.
//!
//! # Which layers, and why each one stops here
//!
//! | Layer | Needs | Named in [ADR-0003] D2? |
//! | --- | --- | --- |
//! | 2 — `~/.zaru/config.toml` | a TOML parser | **No** |
//! | 3 — `./zaru.toml` | a TOML parser | **No** |
//! | 5 — `--tier`, `--model` | argument parsing | **No** |
//!
//! ADR-0003 D2's table names `rmcp`, `ratatui`, `tui-textarea`, `fastembed`,
//! `tokio`, `serde` and `reqwest`, and its Trigger clause 7 treats the table
//! as closed in the other direction too — a dependency the harness turns out
//! not to need is removed "by amendment rather than left standing unused". So
//! declaring a TOML crate or an argument parser is an amendment to that
//! record rather than an import, and a third proposed amendment is drafted
//! there naming both. Until one is accepted, the honest shape is a declared
//! seam with no implementation, which is the shape the `credential-store` arc
//! used for sealing on 2026-09-04.
//!
//! **Layer 4 is not here**, because it needs no dependency at all:
//! [`crate::config::environment`] reads it with `std` alone and is built.
//! Layer 1 is not here either — it is compiled in, so it is a
//! [`Contribution`] a caller constructs directly.
//!
//! # What this buys beyond deferral
//!
//! Every rule ADR-0014 states about *how* layers resolve — D1's precedence,
//! D2's merge, D3's explanation, D4's refusal, D5's unknown keys, D6's
//! ceiling — is checkable over contributions from any source. Only the
//! *reading* of three files and one command line waits. So the record's
//! semantics are exercised today and the parsers arrive as three
//! implementations of one trait rather than as a rewrite.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing

use crate::config::layer::{Contribution, Layer, Source};
use crate::config::value::Table;
use core::fmt;

/// A source could not be read.
///
/// Carries the implementation's own wording. **An implementation must not put
/// a configuration value in it**: the refusals in
/// [`ConfigRefused`](crate::config::refusal::ConfigRefused) carry none, and a
/// source failure that quoted a file's contents would be the hole those close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl SourceFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for SourceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for SourceFailure {}

/// Somewhere one of ADR-0014 D1's layers can be read from.
pub trait LayerSource {
    /// Which of D1's five layers this supplies.
    fn layer(&self) -> Layer;

    /// What D3's explain block calls it.
    fn source(&self) -> Source;

    /// What it holds.
    ///
    /// An absent file is `Ok` with an empty table rather than an error: D3
    /// renders a layer that set nothing as `(not set)`, and a user with no
    /// `~/.zaru/config.toml` has not made a mistake. **The configuration
    /// loader never creates that directory**, and a loader that created a
    /// directory in order to find nothing in it would be creating state to
    /// read state.
    ///
    /// `~/.zaru/` has exactly one creator and it is
    /// [`crate::config::home::ensure`] — one function, called by the
    /// credential store and by ADR-0010's session store alike. Until
    /// 2026-09-04 the rule was that the credential store was the sole creator
    /// and every other module refused; that solved the two-creators problem
    /// and left the session lifecycle with an ordering obligation on its
    /// caller, which is a "for now" rather than a mechanism. Two creators
    /// would still mean the mode held by whichever ran first, which is a rule
    /// holding by circumstance ([Verification lessons] §26) — one function is
    /// what makes it hold by construction instead.
    ///
    /// # Errors
    ///
    /// [`SourceFailure`] when the source exists and cannot be read or parsed.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    fn read(&self) -> Result<Table, SourceFailure>;
}

/// Read every source into the contributions [`Resolution::resolve`] folds.
///
/// [`Resolution::resolve`]: crate::config::resolve::Resolution::resolve
///
/// # Errors
///
/// The first [`SourceFailure`] any source reports.
pub fn gather<'a>(
    sources: impl IntoIterator<Item = &'a dyn LayerSource>,
) -> Result<Vec<Contribution>, SourceFailure> {
    sources
        .into_iter()
        .map(|source| {
            Ok(Contribution::new(
                source.layer(),
                source.source(),
                source.read()?,
            ))
        })
        .collect()
}
