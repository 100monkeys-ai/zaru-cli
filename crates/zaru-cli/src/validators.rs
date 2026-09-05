// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D3's two `expect` kinds that need a crate, decided.
//!
//! # Why these two are here and the other two are in `zaru-core`
//!
//! D3's `exit-zero` and `exit-code = N` are integer comparisons against what
//! the runner reported, so `zaru-core` decides them with `std` alone.
//! `matches` needs a regular-expression engine and `json_schema` needs a JSON
//! Schema validator, and until 2026-09-05 [ADR-0003] D2's table named neither
//! — so both were carried as data and evaluated through
//! [`PatternMatch`](zaru_core::iteration::validator::PatternMatch) and
//! [`SchemaValidate`](zaru_core::iteration::validator::SchemaValidate), which
//! nothing implemented. That table now carries `regex` and `boon` as
//! amendments 4 and 5, and **this module is their caller**.
//!
//! It is in `zaru-cli` rather than `zaru-core` for the reason
//! [`crate::process`] is: `zaru-core` resolves no path, knows no working
//! directory and opens no file, and a schema path is measured against
//! [ADR-0011] D4's boundary, which lives here. The three ports a validator
//! calls out through now all have their product implementation in this crate,
//! and `zaru-core` still implements none of them.
//!
//! # Both futures are already finished, and that is not a shortcut
//!
//! Each port method returns `impl Future + Send`, and each implementation here
//! does the whole thing synchronously and hands back a future that is already
//! resolved — the shape [`crate::process::ports`] uses. Here it is also a
//! requirement rather than a convenience: `boon::Compiler` owns a
//! `Box<dyn UrlLoader>` and is not `Send`, so it must never be held across an
//! await. Doing the work before the `async move` means the future carries a
//! `Result<bool, PortFailure>` and nothing else.
//!
//! # `Ok(false)` is a failing validator; `Err` is an unusable declaration
//!
//! Not decided here — [`zaru_core::iteration::validator::port`] states it and
//! this module obeys it. Output that is not JSON, or is JSON and does not
//! validate, is `Ok(false)`: the command produced the wrong thing, which is
//! the validator doing its job. A pattern that is not a regular expression, a
//! schema file that cannot be read, a schema that is not a schema — those are
//! the manifest being wrong, and they are `Err`.
//!
//! # What a refusal from here may say
//!
//! **A refusal never carries the pattern.** Measured 2026-09-05 against
//! `regex` 1.13.1: `regex::Error`'s `Display` renders the offending pattern
//! with a caret under the fault, and its `Debug` carries it too — four
//! invalid shapes were probed and all four leaked a planted value through both.
//! A manifest is a file that gets committed ([ADR-0014] D4's own reason for
//! keeping credentials out of configuration files) and a refusal is text that
//! gets pasted into a report, so [`PatternRefused`] carries the *kind* and
//! never the text. This is the same measurement, and the same conclusion,
//! [`crate::config::file`] made for `toml::de::Error`.
//!
//! A refusal **does** name a path and a URL. A path is not a value — the
//! argument [`crate::manifest::document`] already makes — and a `$ref` a
//! schema names is the project's own text, which a reader has to be able to
//! find in their own file.
//!
//! # This module declares no configuration key and invents no number
//!
//! [`PatternCeiling`] is the caller's, refused at zero, with no default of its
//! own — the shape [`ProcessCeiling`](crate::process::ProcessCeiling),
//! [`SizeCeiling`](crate::config::SizeCeiling) and
//! [`OutputBudget`](crate::tools::OutputBudget) already have here. Where the
//! number comes from when the binary is wired is the composition's, and the
//! binary's own is [`crate::cli::PATTERN_CEILING_BYTES`].
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

pub mod ceiling;
pub mod pattern;

pub use ceiling::{PatternCeiling, PatternCeilingIsZero};
pub use pattern::{PatternRefused, Patterns};

#[cfg(test)]
mod tests;
