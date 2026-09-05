// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D3's `json_schema = "<path>"`, decided over `boon`.
//!
//! # The boundary is measured again, at the moment the file is opened
//!
//! [`Manifest::build`](crate::manifest::Manifest::build) already refuses a
//! schema path that resolves outside [ADR-0011] D4's working directory, when
//! the manifest is read. This evaluator measures it **again**, and that is one
//! rule rather than two: the rule is
//! [`WorkingDirectory::classify`](crate::tools::WorkingDirectory::classify) and
//! it lives in exactly one place; what there are two of is call sites, on
//! purpose.
//!
//! The reason is the window between them. A manifest is read once, at the top
//! of a run; a validator runs later, and can run again on the next iteration.
//! In between, `schema/output.json` can become a symlink to somewhere else —
//! by the project's own build, by a tool, by anything. The manifest check
//! cannot close that window because it happened before it opened. Checking at
//! the moment of use is what closes it, and the case is in the security
//! corpus, which only grows.
//!
//! # A `$ref` opens nothing and reaches nothing
//!
//! `boon`'s default loader registers the `file` scheme
//! (`loader.rs`, `DefaultUrlLoader::new`), so a schema containing
//! `$ref: "file:///etc/passwd"` would be **read** by the library, outside
//! whatever this module checked. Measured 2026-09-05: the default loader
//! returns `error loading file:///etc/… No such file or directory`, which is
//! the operating system answering, which means it tried.
//!
//! So the loader is replaced with [`NothingIsLoaded`], which refuses every URL
//! and names it. The whole document arrives through `Compiler::add_resource`,
//! from bytes this module read itself, inside the boundary, under the caller's
//! ceiling. A `$ref` that stays inside the document — `#/$defs/…` — resolves
//! as it always did, because that never reaches a loader at all.
//!
//! **This is what keeps `boon` off `scripts/check-crate-boundaries.py`'s
//! network list**: the crate has no HTTP of its own, and the one scheme it
//! could have opened is gone.
//!
//! # The draft is pinned, because ADR-0009 D3 names none
//!
//! D3's fourth row says only "Stdout parses as JSON and validates". A schema
//! carrying its own `$schema` still decides for itself. Where one does not,
//! `boon` "assumes latest draft" and its own documentation says of that:
//! *"The use of this option is HIGHLY encouraged to ensure continued correct
//! operation of your schema. The current default value will not stay the same
//! over time."* Pinning [`Draft::V2020_12`] is what stops a dependency upgrade
//! silently changing what a project's schema means. A **proposal** recorded on
//! ADR-0009 D3 under a delegated coordinator ruling of 2026-09-05, open to
//! Jeshua's veto, not a clause.
//!
//! # `Ok(false)` is a failing validator and `Err` is an unusable declaration
//!
//! Stated by [`zaru_core::iteration::validator::port`] and obeyed here.
//! Standard output that is not JSON is `Ok(false)` — D3 says the kind passes
//! when "Stdout parses as JSON and validates", so output that does not parse
//! is the command producing the wrong thing, which is the validator doing its
//! job. A schema file that is absent, too large, not UTF-8, not JSON, or not a
//! schema is `Err`: the manifest is wrong.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::config::file::{FileRefused, JsonFile, SizeCeiling};
use crate::tools::WorkingDirectory;
use boon::{Compiler, Draft, Schemas, UrlLoader};
use core::fmt;
use core::future::Future;
use std::error::Error;
use std::path::PathBuf;
use zaru_core::iteration::PortFailure;
use zaru_core::iteration::validator::{SchemaPath, SchemaValidate};

/// What this module calls the one document it hands the compiler.
///
/// A URL because `boon` addresses every resource by one, and a scheme of our
/// own because there is deliberately no loader that could resolve it: nothing
/// can be fetched relative to it either.
const SCHEMA_LOCATION: &str = "zaru:///validator-schema";

/// A loader that loads nothing, which is the whole point of it.
///
/// See the module documentation. It is not an empty implementation standing in
/// for a real one — it is the mechanism, and replacing it with `boon`'s default
/// would let a project's schema read a file this harness never classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NothingIsLoaded;

impl UrlLoader for NothingIsLoaded {
    fn load(&self, url: &str) -> Result<serde_json::Value, Box<dyn Error>> {
        Err(Box::new(RefFollowedOutOfTheDocument {
            url: url.to_owned(),
        }))
    }
}

/// A schema asked for something outside its own document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefFollowedOutOfTheDocument {
    /// The URL the schema named. A project's own text, quoted as a path is.
    pub url: String,
}

impl fmt::Display for RefFollowedOutOfTheDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the schema refers to `{}`, and this harness resolves no reference outside the schema \
             file it was given. A manifest arrives from a repository that was cloned, so a `$ref` \
             is a request for the harness to open something the project chose; a reference inside \
             the same document (`#/$defs/...`) is resolved as usual",
            self.url,
        )
    }
}

impl Error for RefFollowedOutOfTheDocument {}

/// Why a declared `json_schema` validator could not be decided.
#[derive(Debug)]
pub enum SchemaRefused {
    /// The path resolves outside ADR-0011 D4's working directory.
    ///
    /// Measured at the moment of use, so this is also what a path that
    /// *became* a link after the manifest was read looks like.
    OutsideTheWorkingDirectory {
        /// The path as the manifest spelled it.
        declared: String,
        /// Where it actually resolved to, symlinks followed.
        resolved: PathBuf,
        /// The working directory it was measured against.
        working_directory: PathBuf,
    },
    /// There is no such file.
    NoSuchFile {
        /// Where it resolved to.
        resolved: PathBuf,
    },
    /// The file could not be read, or is not JSON.
    File(FileRefused),
    /// The document is JSON and is not a schema this harness can compile.
    NotASchema {
        /// `boon`'s own message, which names the keyword and the location
        /// inside the schema. It quotes the project's own file, which is what
        /// a reader needs to find the fault, and never the command's output.
        detail: String,
    },
}

impl fmt::Display for SchemaRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideTheWorkingDirectory {
                declared,
                resolved,
                working_directory,
            } => write!(
                f,
                "the schema at {declared:?} resolves to {} and is outside the working directory \
                 {}. It is measured again here rather than only when the manifest was read, \
                 because a path can become a link between the two",
                resolved.display(),
                working_directory.display(),
            ),
            Self::NoSuchFile { resolved } => write!(
                f,
                "there is no schema at {}; ADR-0009 D3's `json_schema` names a file that has to \
                 be there when the validator runs",
                resolved.display(),
            ),
            Self::File(refusal) => write!(f, "{refusal}"),
            Self::NotASchema { detail } => {
                write!(f, "the schema cannot be compiled: {detail}")
            }
        }
    }
}

impl Error for SchemaRefused {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::File(refusal) => Some(refusal),
            _ => None,
        }
    }
}

impl From<SchemaRefused> for PortFailure {
    fn from(refusal: SchemaRefused) -> Self {
        Self::new(refusal.to_string())
    }
}

/// [ADR-0009] D3's `json_schema` kind, over `boon`.
///
/// Holds a shared reference to the boundary and a ceiling, so it is `Sync`,
/// which [`Dispatch`](zaru_core::iteration::validator::Dispatch) requires. The
/// working directory is held here because the port hands this method only a
/// path and a string — see [`crate::validators`].
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaFiles<'a> {
    working_directory: &'a WorkingDirectory,
    ceiling: SizeCeiling,
}

impl<'a> SchemaFiles<'a> {
    /// Decide `json_schema` validators inside this boundary, under this
    /// ceiling.
    #[must_use]
    pub const fn new(working_directory: &'a WorkingDirectory, ceiling: SizeCeiling) -> Self {
        Self {
            working_directory,
            ceiling,
        }
    }

    /// The boundary every schema path is measured against.
    #[must_use]
    pub const fn working_directory(&self) -> &WorkingDirectory {
        self.working_directory
    }

    /// The ceiling every schema file is read under.
    #[must_use]
    pub const fn ceiling(&self) -> SizeCeiling {
        self.ceiling
    }

    /// Whether the output validates, in this implementation's own refusals.
    ///
    /// # Errors
    ///
    /// [`SchemaRefused`].
    pub fn decide(&self, schema: &SchemaPath, stdout: &str) -> Result<bool, SchemaRefused> {
        let compiled = self.compile(schema)?;
        // Output that is not JSON is the validator FAILING, not the harness.
        // D3: the kind passes when "Stdout parses as JSON and validates".
        let Ok(instance) = serde_json::from_str::<serde_json::Value>(stdout) else {
            return Ok(false);
        };
        let (schemas, index) = compiled;
        Ok(schemas.validate(&instance, index).is_ok())
    }

    /// Read and compile the declared schema, measuring the boundary first.
    fn compile(&self, schema: &SchemaPath) -> Result<(Schemas, boon::SchemaIndex), SchemaRefused> {
        // Before the file is opened, not after.
        let target = self.working_directory.classify(schema.as_str());
        if target.placement().is_out_of_tree() {
            return Err(SchemaRefused::OutsideTheWorkingDirectory {
                declared: schema.as_str().to_owned(),
                resolved: target.resolved().to_path_buf(),
                working_directory: self.working_directory.root().to_path_buf(),
            });
        }

        let document = JsonFile::at(target.resolved(), self.ceiling)
            .read()
            .map_err(SchemaRefused::File)?
            .ok_or_else(|| SchemaRefused::NoSuchFile {
                resolved: target.resolved().to_path_buf(),
            })?;

        let mut compiler = Compiler::new();
        compiler.use_loader(Box::new(NothingIsLoaded));
        compiler.set_default_draft(Draft::V2020_12);
        compiler
            .add_resource(SCHEMA_LOCATION, document)
            .map_err(|error| SchemaRefused::NotASchema {
                // The alternate form carries the cause, which for a refused
                // `$ref` is the sentence naming the URL.
                detail: format!("{error:#}"),
            })?;
        let mut schemas = Schemas::new();
        let index = compiler
            .compile(SCHEMA_LOCATION, &mut schemas)
            .map_err(|error| SchemaRefused::NotASchema {
                detail: format!("{error:#}"),
            })?;
        Ok((schemas, index))
    }
}

impl SchemaValidate for SchemaFiles<'_> {
    fn validates(
        &self,
        schema: &SchemaPath,
        stdout: &str,
    ) -> impl Future<Output = Result<bool, PortFailure>> + Send {
        // Decided before the future exists. `boon::Compiler` owns a
        // `Box<dyn UrlLoader>` and is not `Send`, so it must never be held
        // across an await -- see the module documentation of
        // [`crate::validators`].
        let decided = self.decide(schema, stdout).map_err(PortFailure::from);
        async move { decided }
    }
}
