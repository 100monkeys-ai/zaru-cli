// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D1's `./zaru.toml`, read.
//!
//! # One file, one reader, and this is it
//!
//! [`crate::manifest::port`] has said since 2026-09-04 that "this port returns
//! a whole [`Manifest`], and layer 3's contribution is derived from it by
//! [`Manifest::contribution`]. Whatever implements layer 3 later is an adapter
//! over this rather than a second reader." [`ManifestFile`] is that
//! implementation, and [`crate::cli::layers`]'s layer 3 is that adapter.
//!
//! The bytes come from [`TomlFile`], which is the only thing in this workspace
//! that calls a TOML parser.
//!
//! # The boundary is checked before the file is opened
//!
//! [ADR-0011] D4 makes the working directory the boundary, and `./zaru.toml` is
//! inside it by definition — *unless it is a symlink*. The path is classified
//! through [`WorkingDirectory`] **before** anything reads it, so a manifest
//! that resolves out of the tree is refused rather than parsed. Containment is
//! that type's and is not restated here.
//!
//! # An unknown table is refused rather than dropped
//!
//! [`Manifest`] holds `[project]`, `[runtime]` and `[[validator]]`, and
//! [`Manifest::contribution`] carries the first two into [ADR-0014] D1's layer
//! 3. A reader that ignored a fourth top-level table would drop it before the
//! fold could see it, so [ADR-0014] D5 would never fire on it — which is that
//! clause's own silent-typo failure, arriving one layer earlier than the clause
//! can reach. So a top-level name that is none of the three is refused here,
//! naming the nearest of the three through the same metric D5 uses.
//!
//! # How `expect` is spelled, which the record shows twice out of four times
//!
//! [ADR-0009] D1's worked manifest writes `expect = "exit-zero"` and
//! `expect = { json_schema = "schema/output.json" }`. D3's table gives the four
//! kinds as `exit-zero`, `exit-code = N`, `matches = "<regex>"` and
//! `json_schema = "<path>"`. The reading built, derived from those two rather
//! than invented: **a bare string names the one kind that takes no argument,
//! and a one-entry table names a kind and its argument.** The kind spellings
//! come from [`Expect::KINDS`] rather than being typed here, so a fifth kind
//! cannot arrive in this reader without arriving in that record's own list.
//! Recorded as a proposed reading on ADR-0009 rather than settled here.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::config::file::{FileRefused, SizeCeiling, TomlFile};
use crate::config::{Source, SourceFailure, Table, Value};
use crate::manifest::document::{
    MANIFEST_FILE, Manifest, ManifestRefused, PROJECT_TABLE, RUNTIME_TABLE, VALIDATOR_TABLE,
};
use crate::manifest::port::ManifestSource;
use crate::tools::WorkingDirectory;
use core::fmt;
use std::path::{Path, PathBuf};
use zaru_core::iteration::validator::{Declared, Expect, Name, Pattern, Run, SchemaPath};

/// Why a `zaru.toml` did not become a [`Manifest`].
///
/// **No variant carries a value out of the file.** Names are quoted — a
/// validator's name and a table's name are what a reader has to be able to
/// find in their own file — and values are named by shape, which is the rule
/// [`ConfigRefused`](crate::config::ConfigRefused) follows.
///
/// A validator is identified by its **position** rather than by its name,
/// because the name is one of the things that can be missing.
#[derive(Debug)]
pub enum ManifestNotRead {
    /// The file could not be read, or is not TOML.
    File(FileRefused),
    /// The manifest resolves outside the working directory.
    ///
    /// `./zaru.toml` is inside it by definition unless it is a symlink, so this
    /// is the symlink case and nothing else.
    OutsideTheWorkingDirectory {
        /// Where it actually resolved to, symlinks followed.
        resolved: PathBuf,
        /// The working directory it was measured against.
        working_directory: PathBuf,
    },
    /// A top-level name that is none of ADR-0009 D1's three.
    UnknownTable {
        /// The file.
        path: PathBuf,
        /// What was written.
        offered: String,
        /// The nearest of the three.
        nearest: &'static str,
    },
    /// `[project]` or `[runtime]` is not a table.
    NotATable {
        /// Which one.
        table: &'static str,
        /// What shape it had. **Never the value.**
        found: &'static str,
    },
    /// `validator` is not an array of tables.
    ValidatorsNotAList {
        /// What shape it had.
        found: &'static str,
    },
    /// One `[[validator]]` entry is not a table.
    ValidatorNotATable {
        /// Which entry, counting from one as the file reads.
        position: usize,
        /// What shape it had.
        found: &'static str,
    },
    /// A validator does not declare one of D1's three required fields.
    ValidatorMissing {
        /// Which entry, counting from one.
        position: usize,
        /// Which field.
        field: &'static str,
    },
    /// A validator's field is not the shape it has to be.
    ValidatorWrongShape {
        /// Which entry, counting from one.
        position: usize,
        /// Which field.
        field: &'static str,
        /// What shape it had.
        found: &'static str,
    },
    /// A validator's name, or one of its prerequisites, is not usable.
    UnusableName {
        /// Which entry, counting from one.
        position: usize,
        /// Which field it was in.
        field: &'static str,
        /// `zaru-core`'s own refusal.
        refusal: zaru_core::iteration::validator::NameRefused,
    },
    /// A validator's command, pattern or schema path is not usable.
    UnusableText {
        /// Which entry, counting from one.
        position: usize,
        /// Which field.
        field: &'static str,
        /// `zaru-core`'s own refusal.
        refusal: zaru_core::iteration::validator::TextRefused,
    },
    /// An `expect` naming no kind ADR-0009 D3 defines.
    NoSuchExpectKind {
        /// Which entry, counting from one.
        position: usize,
        /// What was written, escaped.
        offered: String,
        /// The nearest of D3's four.
        nearest: &'static str,
    },
    /// An `expect` table that does not name exactly one kind.
    ExpectNotOneKind {
        /// Which entry, counting from one.
        position: usize,
        /// How many entries it had.
        entries: usize,
    },
    /// The manifest is well shaped and [`Manifest::build`] refused it.
    Refused(ManifestRefused),
}

impl fmt::Display for ManifestNotRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(refusal) => write!(f, "{refusal}"),
            Self::OutsideTheWorkingDirectory {
                resolved,
                working_directory,
            } => write!(
                f,
                "the project manifest resolves to {} and is outside the working directory {}. \
                 ADR-0011 D4 makes the working directory the boundary, and `./{MANIFEST_FILE}` is \
                 inside it by definition unless it is a link out of the tree",
                resolved.display(),
                working_directory.display(),
            ),
            Self::UnknownTable {
                path,
                offered,
                nearest,
            } => write!(
                f,
                "`{offered}` in {} is not something a manifest declares; ADR-0009 D1 gives it \
                 `[{PROJECT_TABLE}]`, `[{RUNTIME_TABLE}]` and `[[{VALIDATOR_TABLE}]]`. Did you \
                 mean `{nearest}`?",
                path.display(),
            ),
            Self::NotATable { table, found } => {
                write!(f, "`{table}` is {found}, and ADR-0009 D1 makes it a table",)
            }
            Self::ValidatorsNotAList { found } => write!(
                f,
                "`{VALIDATOR_TABLE}` is {found}; ADR-0009 D1 declares validators as \
                 `[[{VALIDATOR_TABLE}]]`, which is an array of tables",
            ),
            Self::ValidatorNotATable { position, found } => write!(
                f,
                "validator {position} is {found}, and every `[[{VALIDATOR_TABLE}]]` entry is a \
                 table",
            ),
            Self::ValidatorMissing { position, field } => write!(
                f,
                "validator {position} declares no `{field}`; ADR-0009 D1 gives every validator a \
                 `name`, a `run` and an `expect`",
            ),
            Self::ValidatorWrongShape {
                position,
                field,
                found,
            } => write!(f, "validator {position}'s `{field}` is {found}"),
            Self::UnusableName {
                position,
                field,
                refusal,
            } => write!(f, "validator {position}'s `{field}`: {refusal}"),
            Self::UnusableText {
                position,
                field,
                refusal,
            } => write!(f, "validator {position}'s `{field}`: {refusal}"),
            Self::NoSuchExpectKind {
                position,
                offered,
                nearest,
            } => write!(
                f,
                "validator {position} expects {offered}, which names no kind; ADR-0009 D3 defines \
                 exactly four and says the fifth is where a validator vocabulary becomes a build \
                 system. Did you mean `{nearest}`?",
            ),
            Self::ExpectNotOneKind { position, entries } => write!(
                f,
                "validator {position}'s `expect` names {entries} kinds and it names exactly one; \
                 write `expect = \"exit-zero\"`, or a table with one entry such as \
                 `expect = {{ json_schema = \"schema/output.json\" }}`",
            ),
            Self::Refused(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for ManifestNotRead {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File(refusal) => Some(refusal),
            Self::Refused(refusal) => Some(refusal),
            _ => None,
        }
    }
}

impl From<ManifestNotRead> for SourceFailure {
    /// The port carries the implementation's own wording, and this is it.
    fn from(refusal: ManifestNotRead) -> Self {
        Self::new(refusal.to_string())
    }
}

/// [ADR-0009] D1's manifest, at the root of a working directory.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    working_directory: WorkingDirectory,
    file: TomlFile,
}

impl ManifestFile {
    /// The manifest a project rooted at `working_directory` would have.
    #[must_use]
    pub fn in_directory(working_directory: WorkingDirectory, ceiling: SizeCeiling) -> Self {
        let path = working_directory.root().join(MANIFEST_FILE);
        Self {
            working_directory,
            file: TomlFile::at(path, ceiling),
        }
    }

    /// The file, whether or not it is there.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// The working directory it is measured against.
    #[must_use]
    pub const fn working_directory(&self) -> &WorkingDirectory {
        &self.working_directory
    }

    /// The manifest, in this implementation's own refusals.
    ///
    /// [`ManifestSource::read`] is this, with the refusal flattened into the
    /// port's type. A caller that wants to know *which* thing was wrong calls
    /// here.
    ///
    /// # Errors
    ///
    /// [`ManifestNotRead`].
    pub fn parse(&self) -> Result<Option<Manifest>, ManifestNotRead> {
        // Before the file is opened, not after: a manifest that links out of
        // the tree is refused rather than read.
        let target = self.working_directory.classify(MANIFEST_FILE);
        if target.placement().is_out_of_tree() {
            return Err(ManifestNotRead::OutsideTheWorkingDirectory {
                resolved: target.resolved().to_path_buf(),
                working_directory: self.working_directory.root().to_path_buf(),
            });
        }

        let Some(document) = self.file.read().map_err(ManifestNotRead::File)? else {
            return Ok(None);
        };

        let mut project = Table::new();
        let mut runtime = Table::new();
        let mut validators = Vec::new();
        for (name, value) in document.iter() {
            match name.as_str() {
                PROJECT_TABLE => project = self.table(PROJECT_TABLE, value)?,
                RUNTIME_TABLE => runtime = self.table(RUNTIME_TABLE, value)?,
                VALIDATOR_TABLE => validators = Self::validators(value)?,
                other => {
                    return Err(ManifestNotRead::UnknownTable {
                        path: self.file.path().to_path_buf(),
                        offered: other.to_owned(),
                        nearest: crate::config::nearest::nearest(
                            [PROJECT_TABLE, RUNTIME_TABLE, VALIDATOR_TABLE],
                            other,
                        )
                        .expect("the three names are never empty"),
                    });
                }
            }
        }

        Manifest::build(project, runtime, validators, &self.working_directory)
            .map(Some)
            .map_err(ManifestNotRead::Refused)
    }

    fn table(&self, name: &'static str, value: &Value) -> Result<Table, ManifestNotRead> {
        match value {
            Value::Table(table) => Ok(table.clone()),
            other => Err(ManifestNotRead::NotATable {
                table: name,
                found: other.shape(),
            }),
        }
    }

    /// ADR-0009 D1's `[[validator]]`, in the order the file declared them.
    ///
    /// **Not in dependency order** — that is
    /// [`Plan::from_declared`](zaru_core::iteration::validator::Plan::from_declared)'s
    /// to derive, and deriving it here as well would be two orderings that can
    /// disagree.
    fn validators(value: &Value) -> Result<Vec<Declared>, ManifestNotRead> {
        let Value::Array(entries) = value else {
            return Err(ManifestNotRead::ValidatorsNotAList {
                found: value.shape(),
            });
        };
        entries
            .iter()
            .enumerate()
            .map(|(index, entry)| Self::validator(index + 1, entry))
            .collect()
    }

    fn validator(position: usize, entry: &Value) -> Result<Declared, ManifestNotRead> {
        let Value::Table(fields) = entry else {
            return Err(ManifestNotRead::ValidatorNotATable {
                position,
                found: entry.shape(),
            });
        };

        let name = Name::new(Self::text(position, fields, "name")?).map_err(|refusal| {
            ManifestNotRead::UnusableName {
                position,
                field: "name",
                refusal,
            }
        })?;
        let run = Run::new(Self::text(position, fields, "run")?).map_err(|refusal| {
            ManifestNotRead::UnusableText {
                position,
                field: "run",
                refusal,
            }
        })?;
        let expect = Self::expect(position, fields)?;

        let mut declared = Declared::new(name, run, expect);
        if let Some(after) = fields.get("after") {
            let Value::Array(prerequisites) = after else {
                return Err(ManifestNotRead::ValidatorWrongShape {
                    position,
                    field: "after",
                    found: after.shape(),
                });
            };
            let mut names = Vec::with_capacity(prerequisites.len());
            for prerequisite in prerequisites {
                let Some(text) = prerequisite.as_text() else {
                    return Err(ManifestNotRead::ValidatorWrongShape {
                        position,
                        field: "after",
                        found: prerequisite.shape(),
                    });
                };
                names.push(
                    Name::new(text).map_err(|refusal| ManifestNotRead::UnusableName {
                        position,
                        field: "after",
                        refusal,
                    })?,
                );
            }
            declared = declared.after(names);
        }
        Ok(declared)
    }

    /// One required text field of a validator.
    fn text<'a>(
        position: usize,
        fields: &'a Table,
        field: &'static str,
    ) -> Result<&'a str, ManifestNotRead> {
        let value = fields
            .get(field)
            .ok_or(ManifestNotRead::ValidatorMissing { position, field })?;
        value
            .as_text()
            .ok_or_else(|| ManifestNotRead::ValidatorWrongShape {
                position,
                field,
                found: value.shape(),
            })
    }

    /// ADR-0009 D3's four kinds, from D1's two spellings.
    ///
    /// The kind names come from [`Expect::KINDS`], so a fifth kind cannot be
    /// spelled here without being declared there.
    fn expect(position: usize, fields: &Table) -> Result<Expect, ManifestNotRead> {
        let value = fields
            .get("expect")
            .ok_or(ManifestNotRead::ValidatorMissing {
                position,
                field: "expect",
            })?;

        match value {
            // A bare string names the one kind that takes no argument.
            Value::Text(kind) => Self::without_an_argument(position, kind),
            // A one-entry table names a kind and its argument.
            Value::Table(named) => {
                if named.len() != 1 {
                    return Err(ManifestNotRead::ExpectNotOneKind {
                        position,
                        entries: named.len(),
                    });
                }
                let (kind, argument) = named.iter().next().expect("exactly one entry");
                Self::with_an_argument(position, kind, argument)
            }
            other => Err(ManifestNotRead::ValidatorWrongShape {
                position,
                field: "expect",
                found: other.shape(),
            }),
        }
    }

    fn without_an_argument(position: usize, kind: &str) -> Result<Expect, ManifestNotRead> {
        if kind == Expect::ExitZero.kind() {
            return Ok(Expect::ExitZero);
        }
        Err(ManifestNotRead::NoSuchExpectKind {
            position,
            offered: format!("{:?}", kind.escape_debug().to_string()),
            nearest: Self::nearest_kind(kind),
        })
    }

    fn with_an_argument(
        position: usize,
        kind: &str,
        argument: &Value,
    ) -> Result<Expect, ManifestNotRead> {
        let wrong_shape = || ManifestNotRead::ValidatorWrongShape {
            position,
            field: "expect",
            found: argument.shape(),
        };

        // One arm per kind that takes an argument, matched against that
        // record's own spellings rather than against literals typed here.
        if kind == Expect::ExitCode(0).kind() {
            let code = argument.as_integer().ok_or_else(wrong_shape)?;
            let code = i32::try_from(code).map_err(|_| wrong_shape())?;
            return Ok(Expect::ExitCode(code));
        }
        if kind == Expect::Matches(Pattern::new("x").expect("a literal pattern")).kind() {
            let text = argument.as_text().ok_or_else(wrong_shape)?;
            return Pattern::new(text).map(Expect::Matches).map_err(|refusal| {
                ManifestNotRead::UnusableText {
                    position,
                    field: "expect",
                    refusal,
                }
            });
        }
        if kind == Expect::JsonSchema(SchemaPath::new("x").expect("a literal path")).kind() {
            let text = argument.as_text().ok_or_else(wrong_shape)?;
            return SchemaPath::new(text)
                .map(Expect::JsonSchema)
                .map_err(|refusal| ManifestNotRead::UnusableText {
                    position,
                    field: "expect",
                    refusal,
                });
        }
        Err(ManifestNotRead::NoSuchExpectKind {
            position,
            offered: format!("`{}`", kind.escape_debug()),
            nearest: Self::nearest_kind(kind),
        })
    }

    fn nearest_kind(offered: &str) -> &'static str {
        crate::config::nearest::nearest(Expect::KINDS, offered)
            .expect("ADR-0009 D3 names four kinds")
    }
}

impl ManifestSource for ManifestFile {
    /// What [ADR-0014] D3's explain block calls this file.
    ///
    /// The path as it will be read, so layer 3's source column names the file
    /// that was actually opened.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    fn source(&self) -> Source {
        Source::named(self.path().display().to_string())
    }

    fn read(&self) -> Result<Option<Manifest>, SourceFailure> {
        self.parse().map_err(SourceFailure::from)
    }
}
