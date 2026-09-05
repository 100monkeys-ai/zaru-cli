// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D3's four `expect` kinds, and no fifth.
//!
//! # The set is closed, and D3 says why
//!
//! "**No scripting, no expressions, no conditionals.** A validator that needs
//! logic is a script the project already knows how to write, invoked by `run`.
//! The moment this vocabulary grows a conditional it has become a build
//! system, and **the fifth kind is where that starts**."
//!
//! So [`Expect`] is an enum with four variants and no way to build a fifth.
//! [`Expect::KINDS`] is a hand-written list guarded by an exhaustive match in
//! [`Expect::kind`] and by a length annotation, so adding a variant fails to
//! compile in two places rather than arriving as a silent extension — the same
//! shape `zaru-cli`'s tool names and error classes already use, and
//! [Verification lessons] §30's rule that a comment is not a mechanism applied
//! to a sentence D3 clearly means to be load-bearing.
//!
//! # Two kinds are decided here and two are not, and the split is a boundary
//!
//! `exit-zero` and `exit-code = N` are integer comparisons against what the
//! runner reported, so they are decided in this crate with `std` alone.
//!
//! `matches` and `json_schema` need a regular-expression engine and a JSON
//! Schema validator. Until 2026-09-05 **[ADR-0003] D2's table named neither**,
//! so both were carried as data and evaluated through ports nothing
//! implemented; that table now carries `regex` and `boon`, and both are
//! implemented in `zaru-cli`. They are still carried as data here, and
//! evaluated through [`PatternMatch`](super::port::PatternMatch) and
//! [`SchemaValidate`](super::port::SchemaValidate), because this crate opens no
//! file and knows no working directory — see that module. All four meanings now
//! exist; two of them live one crate over.
//!
//! # There is no score
//!
//! D3 specifies binary pass and fail, and that record's own Status tracking
//! sends gradient scoring to the backlog "rather than being pre-empted here".
//! [`ValidatorOutcome`](crate::iteration::event::ValidatorOutcome) carries no
//! number and nothing here produces one.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::iteration::validator::name::{Pattern, SchemaPath};

/// What [ADR-0009] D3 says an `expect` clause may be.
///
/// Four variants and no fifth. See the module documentation.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// D3 row 1 — "The command exits zero".
    ExitZero,
    /// D3 row 2 — "The command exits N".
    ExitCode(i32),
    /// D3 row 3 — "Stdout matches".
    Matches(Pattern),
    /// D3 row 4 — "Stdout parses as JSON and validates".
    JsonSchema(SchemaPath),
}

impl Expect {
    /// D3's own spellings for its four kinds, in the record's table order.
    ///
    /// The length is annotated, so a fifth variant fails to compile here as
    /// well as in [`Expect::kind`]'s exhaustive match.
    pub const KINDS: [&'static str; 4] = ["exit-zero", "exit-code", "matches", "json_schema"];

    /// How D3's table spells this kind.
    ///
    /// Transcribed from the record rather than derived from the variant, so a
    /// renamed variant cannot silently rename what a reader is shown.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::ExitZero => "exit-zero",
            Self::ExitCode(_) => "exit-code",
            Self::Matches(_) => "matches",
            Self::JsonSchema(_) => "json_schema",
        }
    }

    /// Whether deciding this kind needs a port rather than `std`.
    ///
    /// Not a convenience: it is the statement of which half of D3 exists in
    /// the product today, in a form a check can read, so that "two of the four
    /// kinds are decided here" is asserted rather than described in a comment.
    #[must_use]
    pub const fn needs_an_evaluator_port(&self) -> bool {
        match self {
            Self::ExitZero | Self::ExitCode(_) => false,
            Self::Matches(_) | Self::JsonSchema(_) => true,
        }
    }
}
