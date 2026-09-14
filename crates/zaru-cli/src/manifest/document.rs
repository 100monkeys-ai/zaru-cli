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

use crate::config::{Contribution, Field, FieldKind, Key, Layer, Schema, Source, Table, Value};
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

/// The name [ADR-0009] D1 gives the manifest's array of validator tables.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const VALIDATOR_TABLE: &str = "validator";

/// The file [ADR-0009] D1 puts at the repository root.
///
/// One spelling, read by [`ManifestFile`](crate::manifest::ManifestFile) and
/// written by [ADR-0009] D6's `zaru init`, so the reader and the writer cannot
/// disagree about which file they are about.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const MANIFEST_FILE: &str = "zaru.toml";

/// The name [ADR-0009] D1's `[project]` table gives the project.
///
/// A constant for the reason [`PROJECT_TABLE`] is one: it is written into a
/// schema here and read back out of a document by the fold.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const NAME_KEY: &str = "project.name";

/// The name [ADR-0009] D1's `[project]` table gives the Nuclear Notes
/// workspace, per [ADR-0006] D5.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
pub const WORKSPACE_KEY: &str = "project.workspace";

/// The two configuration keys [ADR-0009] D1's `[project]` table sets.
///
/// Handed to whatever builds a [`Schema`], which is
/// [ADR-0014]'s own Neutral section — "Each record owns its own keys; this one
/// owns how they resolve" — followed the way
/// [`crate::providers::fields`] already follows it.
///
/// **Both are free at every layer.** [ADR-0014] D6 lists what a project may do
/// and "name its workspace" is on it in as many words; a project naming itself
/// is the same kind of statement.
///
/// # Why these had to be declared before a file could be read
///
/// Until 2026-09-05 the binary declared sixteen keys and none of them was one
/// this record's own worked manifest sets, so the moment layer 3 gained a
/// reader a real `zaru.toml` in D1's shape was refused by ADR-0014 D5 with
/// *"unknown key `project.name` in project config (layer 3)"* — and `zaru init`
/// would have written a file the binary then refused to fold.
///
/// # Panics
///
/// Never. The two spellings are this module's own and neither is empty, carries
/// a control character, has an empty segment or has a segment surrounded by
/// whitespace.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn fields() -> Vec<(Key, Field)> {
    [NAME_KEY, WORKSPACE_KEY]
        .into_iter()
        .map(|spelling| {
            (
                Key::new(spelling).expect("ADR-0009 D1's key spellings are well formed"),
                Field::free(FieldKind::Text),
            )
        })
        .collect()
}

/// The Nuclear Notes workspace a project pins, from a resolved configuration.
///
/// # [ADR-0009] D1's key finally has a reader, and it had none for nine days
///
/// [ADR-0006] D5's first sentence is "`zaru.toml` pins the workspace per
/// project. A repository's sessions therefore always search the cortex that
/// belongs to that work without the user reselecting." [`WORKSPACE_KEY`] has
/// existed since this module was written and [`declare`] has folded it into
/// the product schema all along — so a `zaru.toml` naming a workspace has
/// always been **validated** and then **discarded**, which is the worst of
/// the three possible states: a user could write the key, see no complaint,
/// and get nothing.
///
/// This is that key's first reader. What it feeds is
/// [`Meta::workspace`](crate::session::Meta), which
/// [`crate::terminal::open`] then hands the composer's fast tier as the
/// attached workspace.
///
/// **It reads the resolved value rather than the file**, so [ADR-0014]'s
/// layering applies unchanged: the key is `Field::free`, which D6 permits a
/// project to set, and a user's own `~/.zaru/config.toml` can still set it
/// for a directory that carries no manifest. Reading `zaru.toml` directly
/// here would have been a second reader of one file and would have skipped
/// the layer above it.
///
/// `None` where no layer set it, which is a real state rather than a
/// failure: a session in a directory with no pin has no attached workspace,
/// and [ADR-0006] D5's other half — falling back to the account's personal
/// workspace — is deliberately not built here. See
/// [`crate::terminal::open`], which records why.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn attached_workspace(resolution: &crate::config::Resolution) -> Option<String> {
    let key = Key::new(WORKSPACE_KEY).expect("ADR-0009 D1's key spellings are well formed");
    resolution
        .get(&key)
        .and_then(Value::as_text)
        // An empty pin is not a pin. `Field::free(FieldKind::Text)` accepts
        // `project.workspace = ""`, and an empty workspace slug would reach
        // the trie as a key nothing is grouped under -- indistinguishable
        // from the unpinned case, but arrived at by a value the user wrote.
        // Told apart here rather than at the three places that read it.
        .filter(|slug| !slug.trim().is_empty())
        .map(str::to_owned)
}

/// Declare ADR-0009's keys into a caller's schema.
///
/// Built on [`fields`] rather than repeating it, so there is one list.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    fields()
        .into_iter()
        .fold(schema, |schema, (key, field)| schema.with(key, field))
}

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
                 that was cloned, and one cannot configure its way to \
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
