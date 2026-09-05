// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One guarded read, and the two parsers over it.
//!
//! # Why there is exactly one of these
//!
//! Four ports want the same bytes turned into the same shape: [ADR-0014] D1's
//! layer 2 and layer 3, [ADR-0009] D1's manifest — which *is* layer 3, one
//! file and two records — and [ADR-0010] D1's `meta.toml`. Two parsers would
//! be two readings that can disagree about what a file says, which is the
//! argument [`crate::manifest::port`] already makes for a manifest and layer 3
//! being one read.
//!
//! So [`TomlFile`] is the only thing in this workspace that calls a TOML
//! parser, and every port is an adapter over it.
//!
//! # And one guarded read under both parsers
//!
//! [ADR-0009] D3's `json_schema` kind needs a *JSON* file — a schema, which
//! [`Table`] cannot even hold, having neither a float nor a null while a schema
//! legitimately carries both (`multipleOf: 0.5`, `"default": null`). So
//! [`JsonFile`] sits beside [`TomlFile`] and produces a `serde_json::Value`.
//!
//! **What the two share is `bytes`, and that is the point of it being a
//! function rather than a method.** It is where the size ceiling is applied —
//! from the directory entry, *before* the file is brought into memory — and
//! where a file that is not UTF-8 is refused. Two readers with two of those
//! would be two ceilings that can disagree, which is the same argument this
//! module already makes for there being one TOML parse. Only the parse step
//! differs, and it is one arm each.
//!
//! # The JSON refusal carries the parser's rendering, and that was measured
//!
//! The opposite of the TOML one, for a measured reason rather than an
//! inconsistent one. `serde_json::Error`'s `Display` is a short description
//! plus a line and a column — `expected `,` or `}` at line 3 column 3` — and
//! **it does not render the offending source**. Fourteen malformed shapes were
//! probed on 2026-09-05 with a planted value on the offending line, including
//! a number out of range, an invalid escape, a control character in a string,
//! an unterminated string and trailing characters; **none of the fourteen
//! carried the value**. A schema is parsed to an untyped `Value`, so
//! `Category::Data` — the one class whose messages quote what they were given —
//! is unreachable from here. The position is therefore not carried separately
//! either: the parser's own message already has it, and a second rendering of
//! one position is one position in two places.
//!
//! # The refusal is built from the parser's `message`, never from its `Display`
//!
//! **`toml::de::Error`'s `Display` renders the offending source line
//! verbatim.** Measured 2026-09-05 by planting a bearer value on a malformed
//! line: `to_string()` contains it and `message()` does not. A refusal is the
//! text that gets pasted into a report, and [ADR-0014] D4 keeps credentials
//! out of configuration files precisely because those files get committed —
//! so a refusal that quoted the line it choked on would publish whatever was
//! on it.
//!
//! What is carried instead is the path, the line and column computed from the
//! error's own `span`, and its `message`. The computed pair was checked
//! against the parser's own rendering and agrees with it. Eight further
//! malformed shapes were probed for a string value inside `message` and none
//! carries one; the one shape that quotes its value is an out-of-range
//! **integer**, and an integer cannot be credential-shaped under [ADR-0007]
//! D2's discrimination by prefix — the same argument
//! [`ConfigRefused::ProjectMayNotRaise`] already carries for its two whole
//! numbers.
//!
//! `the_refusal_for_a_malformed_file_carries_neither_the_line_nor_a_value_on_it`
//! holds it, and the mutant is building the refusal from `Display`.
//!
//! # Two TOML kinds have no home in this value model, and are refused
//!
//! TOML has a float and a datetime; [`Value`] has neither, because no record
//! declares a key of either shape and adding a variant would settle a schema
//! question inside a reader. Both are refused **naming the key and the kind**
//! and never the value, which is the rule every refusal in
//! [`crate::config::refusal`] follows.
//!
//! # The size ceiling is the caller's and there is no number here
//!
//! A file larger than the ceiling is refused **before it is parsed**, so a
//! pathological document cannot be turned into a tree first. The number is a
//! required argument: a default would be a value chosen for a different caller
//! ([Verification lessons] §14), and the binary's own is declared once in
//! [`crate::cli`].
//!
//! # What this module does not decide
//!
//! It does not know about layers, schemas, working directories or sessions. An
//! absent file is `Ok(None)` rather than an error, because who is owed what by
//! an absent file differs per caller — [ADR-0014] D3 renders an absent layer
//! as `(not set)` and [ADR-0009] D4 owes a project without a manifest a line.
//! And containment is [ADR-0011] D4's, held by
//! [`WorkingDirectory`](crate::tools::WorkingDirectory); a reader that carried
//! a second notion of it would be the rule in two places.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [`ConfigRefused::ProjectMayNotRaise`]: crate::config::ConfigRefused::ProjectMayNotRaise
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

#[cfg(test)]
mod tests;

use crate::config::value::{Table, Value};
use core::fmt;
use std::path::{Path, PathBuf};

/// A ceiling of zero was offered.
///
/// Refused rather than taken, because a zero ceiling accepts an empty file and
/// refuses every other one, which is not a policy anybody would choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CeilingRefused;

impl fmt::Display for CeilingRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a size ceiling of zero would accept an empty file and refuse every other one; a \
             ceiling is the largest file that may be parsed, so it is at least one byte",
        )
    }
}

impl std::error::Error for CeilingRefused {}

/// How large a file may be before it is refused unparsed.
///
/// **No default.** See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SizeCeiling(u64);

impl SizeCeiling {
    /// Take a ceiling from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`CeilingRefused`] when `bytes` is zero.
    pub const fn new(bytes: u64) -> Result<Self, CeilingRefused> {
        if bytes == 0 {
            return Err(CeilingRefused);
        }
        Ok(Self(bytes))
    }

    /// The ceiling, in bytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Where in a file something went wrong, one-based as an editor counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// The line, counting from one.
    pub line: usize,
    /// The column, in characters, counting from one.
    pub column: usize,
}

impl Position {
    /// The position `offset` bytes into `source`.
    ///
    /// The column counts characters rather than bytes, because it is rendered
    /// to a person reading the file in an editor. An offset past the end lands
    /// at the end, which is what a parser reporting an unexpected end of input
    /// wants.
    #[must_use]
    pub fn of(source: &str, offset: usize) -> Self {
        let mut offset = offset.min(source.len());
        // Back up to a character boundary rather than slicing at an arbitrary
        // byte: a span reported inside a multi-byte character would otherwise
        // panic, and a classifier that panics on a file a person wrote is a
        // defect report where a refusal belongs.
        while !source.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &source[..offset];
        Self {
            line: before.matches('\n').count() + 1,
            column: before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1,
        }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// Why a file did not become a document.
///
/// **No variant carries a value out of the file.** A key is quoted, because
/// [ADR-0014] D5 already quotes one back and a reader has to be able to see
/// which one; a *value* is never quoted, which is the rule
/// [`ConfigRefused`](crate::config::ConfigRefused) follows one layer up.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug)]
pub enum FileRefused {
    /// The file is there and could not be read.
    NotRead {
        /// The file.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The file is larger than the ceiling the caller passed.
    ///
    /// Refused **before** it is parsed, so nothing builds a tree out of it.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// How large it is.
        bytes: u64,
        /// The largest this caller will parse.
        ceiling: u64,
    },
    /// The file is not UTF-8.
    NotText {
        /// The file.
        path: PathBuf,
        /// How many bytes were valid before the one that is not.
        valid_up_to: usize,
    },
    /// The file is UTF-8 and is not TOML.
    NotToml {
        /// The file.
        path: PathBuf,
        /// Where, when the parser reported a span. See the module
        /// documentation for why the offending line itself is not here.
        at: Option<Position>,
        /// The parser's own `message`, which is a description rather than a
        /// rendering of the file.
        detail: String,
    },
    /// The file is UTF-8 and is not JSON.
    ///
    /// Unlike [`FileRefused::NotToml`] this carries the parser's own
    /// rendering, which already contains the line and the column. See the
    /// module documentation for the measurement that makes that safe.
    NotJson {
        /// The file.
        path: PathBuf,
        /// `serde_json`'s own message, which describes rather than quotes.
        detail: String,
    },
    /// A value's TOML kind has no counterpart in this value model.
    UnrepresentableKind {
        /// The file.
        path: PathBuf,
        /// The dotted key it sits under. A key, never a value.
        key: String,
        /// What TOML calls the kind.
        kind: &'static str,
    },
}

impl FileRefused {
    /// The file the refusal is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::NotRead { path, .. }
            | Self::TooLarge { path, .. }
            | Self::NotText { path, .. }
            | Self::NotToml { path, .. }
            | Self::NotJson { path, .. }
            | Self::UnrepresentableKind { path, .. } => path.as_path(),
        }
    }
}

impl fmt::Display for FileRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRead { path, source } => {
                write!(f, "could not read {}: {source}", path.display())
            }
            Self::TooLarge {
                path,
                bytes,
                ceiling,
            } => write!(
                f,
                "{} is {bytes} bytes and this harness parses at most {ceiling}; a file that large \
                 is refused rather than parsed, because whatever it is it is not a configuration \
                 file somebody wrote",
                path.display(),
            ),
            Self::NotText { path, valid_up_to } => write!(
                f,
                "{} is not UTF-8: the first {valid_up_to} byte(s) are, and the one after them is \
                 not. A TOML file is UTF-8 by the format's own definition",
                path.display(),
            ),
            Self::NotToml { path, at, detail } => match at {
                Some(at) => write!(f, "{} is not TOML at {at}: {detail}", path.display()),
                None => write!(f, "{} is not TOML: {detail}", path.display()),
            },
            Self::NotJson { path, detail } => {
                write!(f, "{} is not JSON: {detail}", path.display())
            }
            Self::UnrepresentableKind { path, key, kind } => write!(
                f,
                "`{key}` in {} holds {kind}, which no configuration key can hold. The value is \
                 deliberately not quoted here",
                path.display(),
            ),
        }
    }
}

impl std::error::Error for FileRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotRead { source, .. } => Some(source),
            Self::TooLarge { .. }
            | Self::NotText { .. }
            | Self::NotToml { .. }
            | Self::NotJson { .. }
            | Self::UnrepresentableKind { .. } => None,
        }
    }
}

/// A TOML file on disk, read into this crate's own value model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlFile {
    path: PathBuf,
    ceiling: SizeCeiling,
}

impl TomlFile {
    /// The file at `path`, parsed only if it is at most `ceiling` bytes.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>, ceiling: SizeCeiling) -> Self {
        Self {
            path: path.into(),
            ceiling,
        }
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The ceiling this reader was given.
    #[must_use]
    pub const fn ceiling(&self) -> SizeCeiling {
        self.ceiling
    }

    /// The document, or `None` where there is no such file.
    ///
    /// **This never creates anything**, including the directory the file would
    /// be in — see [`crate::config::port`] on why a loader that created a
    /// directory in order to find nothing in it would be creating state to read
    /// state.
    ///
    /// # Errors
    ///
    /// [`FileRefused`], naming the file and, for a parse failure, the line and
    /// column.
    pub fn read(&self) -> Result<Option<Table>, FileRefused> {
        let Some(text) = text(&self.path, self.ceiling)? else {
            return Ok(None);
        };
        let parsed: toml::Table = text.parse().map_err(|error: toml::de::Error| {
            // `error.to_string()` renders the offending source line. It is
            // never reached from here; see the module documentation.
            FileRefused::NotToml {
                path: self.path.clone(),
                at: error.span().map(|span| Position::of(&text, span.start)),
                detail: error.message().to_owned(),
            }
        })?;
        self.document(parsed).map(Some)
    }

    /// This crate's value model, from the parser's.
    fn document(&self, parsed: toml::Table) -> Result<Table, FileRefused> {
        let mut here = Vec::new();
        self.table(parsed, &mut here)
    }

    fn table(&self, parsed: toml::Table, above: &mut Vec<String>) -> Result<Table, FileRefused> {
        let mut table = Table::new();
        for (name, value) in parsed {
            above.push(name.clone());
            let converted = self.value(value, above)?;
            above.pop();
            table.insert(name, converted);
        }
        Ok(table)
    }

    fn value(&self, parsed: toml::Value, above: &mut Vec<String>) -> Result<Value, FileRefused> {
        // Wildcard-free, so a seventh TOML kind is a compile error here rather
        // than a value silently taking a neighbour's shape.
        match parsed {
            toml::Value::Boolean(flag) => Ok(Value::Bool(flag)),
            toml::Value::Integer(number) => Ok(Value::Integer(number)),
            toml::Value::String(text) => Ok(Value::Text(text)),
            toml::Value::Array(items) => {
                let mut converted = Vec::with_capacity(items.len());
                for (index, item) in items.into_iter().enumerate() {
                    above.push(index.to_string());
                    converted.push(self.value(item, above)?);
                    above.pop();
                }
                Ok(Value::Array(converted))
            }
            toml::Value::Table(inner) => self.table(inner, above).map(Value::Table),
            kind @ (toml::Value::Float(_) | toml::Value::Datetime(_)) => {
                Err(FileRefused::UnrepresentableKind {
                    path: self.path.clone(),
                    key: above.join("."),
                    kind: kind.type_str(),
                })
            }
        }
    }
}

/// One file's bytes, refusing one that is too large before reading it.
///
/// The size is taken from the directory entry rather than from the read, so an
/// oversized file is never brought into memory at all. An absent file is
/// `Ok(None)` rather than an error, because who is owed what by an absent file
/// differs per caller.
///
/// **A function rather than a method on either reader**, so that the ceiling
/// and the refusals are applied in one place whichever parser runs next. See
/// the module documentation.
///
/// # Errors
///
/// [`FileRefused::NotRead`] and [`FileRefused::TooLarge`].
pub(crate) fn bytes(path: &Path, ceiling: SizeCeiling) -> Result<Option<Vec<u8>>, FileRefused> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(FileRefused::NotRead {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.len() > ceiling.get() {
        return Err(FileRefused::TooLarge {
            path: path.to_path_buf(),
            bytes: metadata.len(),
            ceiling: ceiling.get(),
        });
    }
    std::fs::read(path)
        .map(Some)
        .map_err(|source| FileRefused::NotRead {
            path: path.to_path_buf(),
            source,
        })
}

/// The UTF-8 text of a file, under the same ceiling.
///
/// The step both parsers take before they differ: TOML and JSON are both UTF-8
/// by their formats' own definitions.
fn text(path: &Path, ceiling: SizeCeiling) -> Result<Option<String>, FileRefused> {
    let Some(raw) = bytes(path, ceiling)? else {
        return Ok(None);
    };
    String::from_utf8(raw)
        .map(Some)
        .map_err(|error| FileRefused::NotText {
            path: path.to_path_buf(),
            valid_up_to: error.utf8_error().valid_up_to(),
        })
}

/// A JSON file on disk, read through the same guarded read as [`TomlFile`].
///
/// **It produces a `serde_json::Value` and not a [`Table`]**, because the only
/// JSON file this harness reads is [ADR-0009] D3's schema and a schema carries
/// shapes this crate's configuration value model deliberately has no room for.
/// See the module documentation.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFile {
    path: PathBuf,
    ceiling: SizeCeiling,
}

impl JsonFile {
    /// The file at `path`, parsed only if it is at most `ceiling` bytes.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>, ceiling: SizeCeiling) -> Self {
        Self {
            path: path.into(),
            ceiling,
        }
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The ceiling this reader was given.
    #[must_use]
    pub const fn ceiling(&self) -> SizeCeiling {
        self.ceiling
    }

    /// The document, or `None` where there is no such file.
    ///
    /// **This never creates anything**, for [`TomlFile::read`]'s reason.
    ///
    /// # Errors
    ///
    /// [`FileRefused`], naming the file and, for a parse failure, the parser's
    /// own description with its line and column.
    pub fn read(&self) -> Result<Option<serde_json::Value>, FileRefused> {
        let Some(source) = text(&self.path, self.ceiling)? else {
            return Ok(None);
        };
        serde_json::from_str(&source)
            .map(Some)
            .map_err(|error| FileRefused::NotJson {
                path: self.path.clone(),
                detail: error.to_string(),
            })
    }
}
