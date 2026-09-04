// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The manifest itself, and the one thing it refuses.
//!
//! # A project-supplied path meets ADR-0011 D4's boundary
//!
//! [ADR-0009] D3's fourth `expect` kind is `json_schema = "<path>"`, so
//! **filesystem paths are legal values in a file that arrives from a
//! repository the user cloned**. [ADR-0011] D4 makes anything not below the
//! working directory a distinct class, but that clause is about the *tool
//! surface* and a validator's schema is read by the harness rather than by a
//! tool; [ADR-0014] D6 forbids a project raising a security posture and says
//! nothing about a project naming a path outside itself. **No record covers
//! it**, and the gap was raised on 2026-09-04 by the `configuration-hierarchy`
//! arc, which built no path handling at all.
//!
//! Under a delegated coordinator ruling of 2026-09-04 the reading built here
//! is that **a schema path resolving outside the working directory is
//! refused**, using [`WorkingDirectory`] as it
//! stands rather than a second notion of containment. It is recorded as a
//! proposed reading on ADR-0009 and ADR-0011 and settled by neither. The case
//! joins the security corpus, which only grows.
//!
//! Refusing rather than prompting is the direction to be wrong in on a
//! boundary: the alternative reading is that a schema path prompts as D4's
//! distinct class does, and prompting needs a confirmer that ADR-0011 declares
//! and nothing implements. A refusal a user can act on is available today; a
//! prompt nobody can answer is the silent default that record forbids.
//!
//! # Every refusal names the path it resolved to
//!
//! `../../../etc/passwd` tells a reader less than the path it actually
//! reached, which is the same argument
//! [`Target::resolved`](crate::tools::Target::resolved) already makes for the
//! transcript. A path is not a credential — [`ConfigRefused`]'s rule against
//! carrying values is about the credential shapes ADR-0007 D2 names, and a
//! credential-shaped string is refused by the configuration fold long before
//! it could reach a validator's declaration.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [`ConfigRefused`]: crate::config::ConfigRefused

use crate::config::{Contribution, Layer, Source, Table, Value};
use crate::tools::WorkingDirectory;
use core::fmt;
use std::path::PathBuf;
use zaru_core::iteration::validator::{Declared, Expect, Name};

/// The name [ADR-0009] D1 gives the manifest's project table, which is also
/// the first segment of every key it contributes to [ADR-0014]'s layer 3.
///
/// A constant rather than two string literals: the name is written into a
/// contribution here and read back out of one by whatever declares the schema,
/// and two spellings of one path agree on the day they are written
/// ([Verification lessons] §47).
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2
pub const PROJECT_TABLE: &str = "project";

/// The name [ADR-0009] D1 gives the manifest's runtime table.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const RUNTIME_TABLE: &str = "runtime";

/// Why a manifest was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestRefused {
    /// A validator's `json_schema` path resolves outside the working
    /// directory. See the module documentation.
    SchemaPathLeavesTheWorkingDirectory {
        /// Which validator declared it.
        validator: Name,
        /// The path as the manifest spelled it.
        declared: String,
        /// Where it actually resolved to, symlinks followed.
        resolved: PathBuf,
        /// The working directory it was measured against.
        working_directory: PathBuf,
    },
}

impl fmt::Display for ManifestRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaPathLeavesTheWorkingDirectory {
                validator,
                declared,
                resolved,
                working_directory,
            } => write!(
                f,
                "the validator {validator:?} expects a schema at {declared:?}, which resolves to \
                 {} and is outside the working directory {}. A manifest arrives from a repository \
                 that was cloned, and ADR-0014 D6 exists so that one cannot configure its way to \
                 more than the user granted",
                resolved.display(),
                working_directory.display(),
            ),
        }
    }
}

impl std::error::Error for ManifestRefused {}

/// One project's `zaru.toml`, as [ADR-0009] D1 shapes it.
///
/// Built by a caller. **Nothing parses TOML anywhere in this workspace** — see
/// [`ManifestSource`](super::port::ManifestSource).
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    project: Table,
    runtime: Table,
    validators: Vec<Declared>,
}

impl Manifest {
    /// Take a manifest, measuring every declared schema path against the
    /// working directory.
    ///
    /// The working directory is a **required** argument with no default. That
    /// is the point of it being here: a default would be a value chosen for a
    /// different caller ([Verification lessons] §14), and the only reason this
    /// constructor is in `zaru-cli` rather than beside the declarations in
    /// `zaru-core` is that this is where the boundary lives.
    ///
    /// # Errors
    ///
    /// [`ManifestRefused`], naming the validator, the declared spelling and
    /// the path it resolved to. Every offending validator is checked, and the
    /// first in declaration order is reported.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    pub fn build(
        project: Table,
        runtime: Table,
        validators: Vec<Declared>,
        working_directory: &WorkingDirectory,
    ) -> Result<Self, ManifestRefused> {
        for declared in &validators {
            let Expect::JsonSchema(schema) = &declared.expect else {
                continue;
            };
            let target = working_directory.classify(schema.as_str());
            if target.placement().is_out_of_tree() {
                return Err(ManifestRefused::SchemaPathLeavesTheWorkingDirectory {
                    validator: declared.name.clone(),
                    declared: schema.as_str().to_owned(),
                    resolved: target.resolved().to_path_buf(),
                    working_directory: working_directory.root().to_path_buf(),
                });
            }
        }
        Ok(Self {
            project,
            runtime,
            validators,
        })
    }

    /// What `[project]` holds.
    #[must_use]
    pub const fn project(&self) -> &Table {
        &self.project
    }

    /// What `[runtime]` holds.
    #[must_use]
    pub const fn runtime(&self) -> &Table {
        &self.runtime
    }

    /// The declared validators, in the order the file declared them.
    ///
    /// **Not in dependency order** — that is
    /// [`Plan::from_declared`](zaru_core::iteration::validator::Plan::from_declared)'s
    /// to derive, and deriving it here as well would be two orderings that can
    /// disagree.
    #[must_use]
    pub fn validators(&self) -> &[Declared] {
        &self.validators
    }

    /// What this manifest contributes to [ADR-0014] D1's layer 3.
    ///
    /// `[project]` and `[runtime]` become nested tables under their own names,
    /// so a schema declaring `runtime.max_iterations` finds it at the dotted
    /// key ADR-0014 D3 explains it by. **`[[validator]]` is not here** — see
    /// the module documentation.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn contribution(&self, source: Source) -> Contribution {
        let mut document = Table::new();
        if !self.project.is_empty() {
            document.insert(PROJECT_TABLE, Value::Table(self.project.clone()));
        }
        if !self.runtime.is_empty() {
            document.insert(RUNTIME_TABLE, Value::Table(self.runtime.clone()));
        }
        Contribution::new(Layer::Project, source, document)
    }
}
