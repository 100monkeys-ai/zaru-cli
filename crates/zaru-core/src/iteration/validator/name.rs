// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The four pieces of text a `[[validator]]` block carries, and the shapes
//! each one refuses.
//!
//! # Every refusal is derived from a surface, never from taste
//!
//! There is **no character allowlist and no length cap** anywhere here. A cap
//! nobody chose is a value chosen for a different caller
//! ([Verification lessons] §14), and an allowlist would refuse a non-Latin
//! validator name for no reason any record gives. What each type refuses is
//! what some surface it reaches cannot represent, and each refusal names that
//! surface.
//!
//! The four are separate types rather than one, because they refuse different
//! things for different reasons and a single `Text` would have to refuse the
//! union — which would refuse a multi-line shell command in order to protect a
//! rendered name.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use core::fmt;

/// Why a validator's name was not taken.
///
/// A name is not a value, so a refusal quotes it back: a reader has to be able
/// to see which declaration was rejected, and [ADR-0009] D1 puts the name in a
/// file the user wrote.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRefused {
    /// The name was empty.
    Empty,
    /// The name carried a control character.
    Control {
        /// The name as it was offered, escaped.
        offered: String,
    },
    /// The name began or ended with whitespace.
    SurroundingWhitespace {
        /// The name as it was offered.
        offered: String,
    },
}

impl fmt::Display for NameRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "a validator's name is empty; ADR-0009 D2's `after` names a prerequisite by name \
                 and the event stream renders one, and neither can name nothing",
            ),
            Self::Control { offered } => write!(
                f,
                "the validator name {offered:?} carries a control character; the name is rendered \
                 into the event stream and into the failure text a refinement prompt carries, \
                 where one can erase or overwrite a neighbouring line",
            ),
            Self::SurroundingWhitespace { offered } => write!(
                f,
                "the validator name {offered:?} begins or ends with whitespace; two names \
                 differing only there are one name to every reader of `after` and of the event \
                 stream",
            ),
        }
    }
}

impl std::error::Error for NameRefused {}

/// Why a piece of a validator's declaration was not taken.
///
/// Separate from [`NameRefused`] because these three carry no name to quote
/// back and because an empty one is the only shape any of them refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRefused {
    /// A validator's `run` command was empty or only whitespace.
    ///
    /// Refused because [ADR-0009] D3's every kind is a statement about what
    /// running the command produced, and there is nothing to run.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    EmptyRun,
    /// A `matches` pattern was empty.
    ///
    /// Refused because an empty pattern matches every possible standard
    /// output, so the validator passes whatever happens — which is exactly the
    /// silent green ADR-0009's dependency ordering exists to prevent, arriving
    /// through a different door.
    EmptyPattern,
    /// A `json_schema` path was empty or only whitespace.
    EmptySchemaPath,
}

impl fmt::Display for TextRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRun => f.write_str(
                "a validator's `run` command is empty; ADR-0009 D3's expectations are all \
                 statements about what running it produced",
            ),
            Self::EmptyPattern => f.write_str(
                "a `matches` pattern is empty; an empty pattern matches every standard output, \
                 so the validator would pass whatever happened",
            ),
            Self::EmptySchemaPath => f.write_str(
                "a `json_schema` path is empty; ADR-0009 D3 names a path to a schema file",
            ),
        }
    }
}

impl std::error::Error for TextRefused {}

/// A validator's declared name, per [ADR-0009] D1.
///
/// Constructed only through [`Name::new`], so a name that reached this type has
/// already been refused every shape [`NameRefused`] names.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Take a validator's name.
    ///
    /// # Errors
    ///
    /// One variant of [`NameRefused`] per shape; the first found is returned,
    /// and the order is fixed so a given input always names the same reason.
    pub fn new(offered: &str) -> Result<Self, NameRefused> {
        if offered.is_empty() {
            return Err(NameRefused::Empty);
        }
        if offered.chars().any(char::is_control) {
            return Err(NameRefused::Control {
                offered: offered.escape_debug().to_string(),
            });
        }
        if offered.trim() != offered {
            return Err(NameRefused::SurroundingWhitespace {
                offered: offered.to_owned(),
            });
        }
        Ok(Self(offered.to_owned()))
    }

    /// The name as it was declared.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A validator's `run` command, exactly as declared.
///
/// # Why this refuses less than [`Name`] does
///
/// It refuses an empty or whitespace-only command and **nothing else** — in
/// particular it does not refuse a control character, because TOML can express
/// a multi-line string and a multi-line shell command is an ordinary thing to
/// declare. Refusing one would be this module inventing a restriction no
/// record states, which is [Verification lessons] §14's default-argument
/// failure wearing a validator's clothes.
///
/// Nothing in this crate renders a command, so the argument that makes
/// [`Name`] refuse control characters does not reach here.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run(String);

impl Run {
    /// Take a `run` command.
    ///
    /// # Errors
    ///
    /// [`TextRefused::EmptyRun`] when the command is empty or only whitespace.
    pub fn new(offered: impl Into<String>) -> Result<Self, TextRefused> {
        let offered = offered.into();
        if offered.trim().is_empty() {
            return Err(TextRefused::EmptyRun);
        }
        Ok(Self(offered))
    }

    /// The command as it was declared.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The pattern of [ADR-0009] D3's `matches = "<regex>"`, as declared text.
///
/// **It is not compiled here and this crate carries no regular-expression
/// engine.** `regex` is a row in [ADR-0003] D2's table since 2026-09-05 and it
/// is `zaru-cli`'s, not this crate's, so what a pattern *means* is
/// [`PatternMatch`](super::port::PatternMatch)'s and this type is the
/// declaration travelling to it.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern(String);

impl Pattern {
    /// Take a `matches` pattern.
    ///
    /// # Errors
    ///
    /// [`TextRefused::EmptyPattern`] when the pattern is empty.
    pub fn new(offered: impl Into<String>) -> Result<Self, TextRefused> {
        let offered = offered.into();
        if offered.is_empty() {
            return Err(TextRefused::EmptyPattern);
        }
        Ok(Self(offered))
    }

    /// The pattern as it was declared.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The path of [ADR-0009] D3's `json_schema = "<path>"`, as declared text.
///
/// **Text rather than a `PathBuf`, and that is the boundary rather than an
/// oversight.** This crate resolves no path, knows no working directory and
/// opens no file; what a path is allowed to reach is [ADR-0011] D4's, whose
/// implementation lives in `zaru-cli` beside the working directory it is
/// measured against. So a schema path travels through here as the spelling
/// the manifest carried, and it is `zaru-cli` that refuses one resolving
/// outside the tree.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaPath(String);

impl SchemaPath {
    /// Take a `json_schema` path.
    ///
    /// # Errors
    ///
    /// [`TextRefused::EmptySchemaPath`] when the path is empty or only
    /// whitespace.
    pub fn new(offered: impl Into<String>) -> Result<Self, TextRefused> {
        let offered = offered.into();
        if offered.trim().is_empty() {
            return Err(TextRefused::EmptySchemaPath);
        }
        Ok(Self(offered))
    }

    /// The path as it was declared.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
