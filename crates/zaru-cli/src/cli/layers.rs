// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0014] D1's layers 1 and 5, and the fold this binary runs.
//!
//! # Layer 5 has a reader, and it is this crate's first `LayerSource`
//!
//! That record's own Status tracking has said since 2026-09-04 that "layers 2,
//! 3 and 5 are read through a `LayerSource` port with **no implementation in
//! the product tree**", and [`crate::config::port`] carries the same sentence.
//! [`Flags`] is the first implementation of that port anywhere in this crate's
//! product tree. Layers 2 and 3 still have none: they need a TOML reader, and
//! `toml` is a row in [ADR-0003] D2's table that no arc has taken a caller for
//! yet.
//!
//! **`read` cannot fail**, and that is worth saying rather than hiding behind
//! the signature. The port returns a `Result` because a *file* source can fail
//! to be read; a flag was read from the process before this type was built, so
//! there is nothing left to go wrong. The arm exists because the trait's
//! signature has one.
//!
//! # Layer 1 holds exactly one key
//!
//! [`crate::runtime::BUILT_IN_TIER`], and nothing else. Every other key this
//! binary declares is deliberately without a default: [ADR-0012]'s Neutral
//! consequence is one sentence — "Nothing here selects a default model. That
//! is configuration and it changes as models do" — and [ADR-0010] D6's thirty
//! days has no key to be the default of, which that record says outright.
//!
//! # What layer 5 sets, and what a flag may not reach
//!
//! Two keys, because D1 names two: `runtime.tier` from `--runtime`, and
//! `model.default` from `--model`. A flag cannot reach any other key, which is
//! a property of [`Overrides`] having two fields rather than of a check.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::cli::invocation::Overrides;
use crate::config::{
    ConfigRefused, Contribution, Layer, LayerSource, Resolution, Schema, SizeCeiling, Source,
    SourceFailure, Table, TomlFile, Value, environment, gather,
};
use crate::manifest::{ManifestFile, ManifestSource};
use crate::providers::ModelAlias;
use crate::tools::WorkingDirectory;
use crate::validators::PatternCeiling;
use core::fmt;
use std::path::Path;

/// Everything the fold could refuse this binary.
#[derive(Debug)]
pub enum LoadFailure {
    /// A layer could not be read.
    ///
    /// **Unreachable from this binary today**: its two sources are a compiled
    /// constant and a parsed flag, and neither can fail. It is carried because
    /// [`LayerSource::read`] returns a `Result` and a layer that reads a file
    /// will need it.
    Source(SourceFailure),
    /// The fold refused something ADR-0014 forbids.
    Refused(ConfigRefused),
}

impl fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(failure) => write!(f, "{failure}"),
            Self::Refused(refusal) => write!(f, "{refusal}"),
        }
    }
}

impl std::error::Error for LoadFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(failure) => Some(failure),
            Self::Refused(refusal) => Some(refusal),
        }
    }
}

/// [ADR-0014] D1's layer 1: what this binary compiles in.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltIn {
    document: Table,
}

impl Default for BuiltIn {
    fn default() -> Self {
        Self::new()
    }
}

impl BuiltIn {
    /// The compiled-in layer.
    #[must_use]
    pub fn new() -> Self {
        let mut document = Table::new();
        document.insert_path(
            &crate::runtime::key(),
            Value::Text(crate::runtime::BUILT_IN_TIER.as_str().to_owned()),
        );
        Self { document }
    }
}

impl LayerSource for BuiltIn {
    fn layer(&self) -> Layer {
        Layer::BuiltIn
    }

    fn source(&self) -> Source {
        Layer::BuiltIn.default_source()
    }

    fn read(&self) -> Result<Table, SourceFailure> {
        Ok(self.document.clone())
    }
}

/// [ADR-0014] D1's layer 5: what the flags said.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flags {
    document: Table,
}

impl Flags {
    /// The layer a parsed command line contributes.
    ///
    /// Values arrive as [`Value::Text`], exactly as layer 4's do, and the
    /// schema coerces them during the fold — so `--runtime nonsense` is
    /// refused by [`crate::runtime`] naming layer 5, rather than by the parser
    /// with no layer to name.
    #[must_use]
    pub fn of(overrides: &Overrides) -> Self {
        let mut document = Table::new();
        if let Some(tier) = &overrides.tier {
            document.insert_path(&crate::runtime::key(), Value::Text(tier.clone()));
        }
        if let Some(model) = &overrides.model {
            document.insert_path(&ModelAlias::Default.key(), Value::Text(model.clone()));
        }
        Self { document }
    }
}

impl LayerSource for Flags {
    fn layer(&self) -> Layer {
        Layer::Flag
    }

    /// D3's second column for a layer with no file: its own label.
    ///
    /// **The label rather than a spelling of the flags.** D3's block prints
    /// `5  flag` for exactly this row, and [`Layer::default_source`] is where
    /// that word already lives, so the fold's own fallback and this
    /// implementation cannot disagree.
    fn source(&self) -> Source {
        Layer::Flag.default_source()
    }

    /// See the module documentation: this cannot fail.
    fn read(&self) -> Result<Table, SourceFailure> {
        Ok(self.document.clone())
    }
}

/// How large a configuration file this binary will parse, in bytes.
///
/// **One mebibyte, and this is the one place the number is written.**
/// [`SizeCeiling`] takes it as a required argument with no default of its own,
/// because a default there would be a value chosen for a different caller
/// ([Verification lessons] §14); a caller has to choose, and this is that
/// choice.
///
/// A file above it is refused **before it is parsed**, with the sentence that
/// whatever it is, it is not a configuration file somebody wrote. No record
/// names a number, so this is a delegated coordinator ruling of 2026-09-05
/// recorded on [ADR-0014] and open to Jeshua's veto. Two properties made it the
/// number: ADR-0014 D1's files are hand-written, and D5 already refuses an
/// unknown key, so a legitimate file is bounded by the schema rather than by
/// its own length.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub const FILE_CEILING_BYTES: u64 = 1 << 20;

/// [`FILE_CEILING_BYTES`] as the reader takes it.
///
/// # Panics
///
/// Never. [`FILE_CEILING_BYTES`] is not zero.
#[must_use]
pub fn file_ceiling() -> SizeCeiling {
    SizeCeiling::new(FILE_CEILING_BYTES).expect("a mebibyte is not zero")
}

/// How large a compiled `matches` pattern this binary will run, in bytes.
///
/// **Ten mebibytes, and this is the one place the number is written.** It sits
/// beside [`FILE_CEILING_BYTES`] rather than in
/// [`crate::validators`] for that constant's own reason:
/// [`PatternCeiling`] takes it as a required argument with no default, because
/// a default there would be a value chosen for a different caller
/// ([Verification lessons] §14), and this module is where the numbers this
/// binary chooses live.
///
/// The number is `regex`'s own default `size_limit`, taken deliberately rather
/// than lowered. That crate's advice for untrusted patterns is to "configure
/// `RegexBuilder::size_limit` to something small and then expand it as
/// needed", and *needed* is the word this binary cannot yet evaluate: no
/// command declares a validator, so no real pattern has ever been compiled
/// here and a smaller number would be a bound chosen against no evidence. No
/// record names one either, so this is a delegated coordinator ruling of
/// 2026-09-05 recorded on [ADR-0009] and open to Jeshua's veto. **No
/// configuration key is declared for it**, for the reason
/// [`crate::process::ceiling`] gives: each record owns its own keys and
/// ADR-0009 names none.
///
/// **Nothing passes it yet.** Like the two numbers
/// [`crate::process::Spawn`] takes, it waits for the composition that wires a
/// task through the loop.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub const PATTERN_CEILING_BYTES: usize = 10 * (1 << 20);

/// [`PATTERN_CEILING_BYTES`] as the evaluator takes it.
///
/// # Panics
///
/// Never. [`PATTERN_CEILING_BYTES`] is not zero.
#[must_use]
pub fn pattern_ceiling() -> PatternCeiling {
    PatternCeiling::new(PATTERN_CEILING_BYTES).expect("ten mebibytes is not zero")
}

/// How large a file [ADR-0011] D1's `fs.search` will read the contents of.
///
/// **One mebibyte, and this is the one place the number is written** — the
/// same sentence [`FILE_CEILING_BYTES`] carries, and deliberately the same
/// number, because the two answer the same question: how large a file this
/// harness will read whole into memory to look at.
///
/// It is a second constant rather than a reuse of the first because the two
/// are answerable by different records. That one is [ADR-0014]'s, about a
/// hand-written configuration file; this one is ADR-0011 D5's, about what a
/// model-driven search may pull into a prompt. A single constant would make a
/// later change to one silently change the other.
///
/// A file above it is **named as skipped** rather than passed over, because a
/// search that quietly did not look somewhere is how a model concludes a
/// string is absent. No record names a number, so this is a delegated
/// coordinator ruling of 2026-09-05 recorded on ADR-0011 and open to Jeshua's
/// veto, and no configuration key is declared for it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const SEARCH_CEILING_BYTES: u64 = 1 << 20;

/// [`SEARCH_CEILING_BYTES`] as the tool surface takes it.
///
/// # Panics
///
/// Never. [`SEARCH_CEILING_BYTES`] is not zero.
#[must_use]
pub fn search_ceiling() -> SizeCeiling {
    SizeCeiling::new(SEARCH_CEILING_BYTES).expect("a mebibyte is not zero")
}

/// [ADR-0014] D1's layer 2: `~/.zaru/config.toml`.
///
/// **The loader never creates `~/.zaru/`.** That directory has exactly one
/// creator, [`crate::config::home::ensure`], and a loader that created it in
/// order to find nothing in it would be creating state to read state — see
/// [`crate::config::port`].
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFile {
    file: TomlFile,
}

impl UserFile {
    /// Layer 2 under a `~/.zaru`-equivalent directory.
    #[must_use]
    pub fn under(home: &Path) -> Self {
        Self {
            file: TomlFile::at(home.join(crate::config::CONFIG_FILE), file_ceiling()),
        }
    }

    /// The file, whether or not it is there.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

impl LayerSource for UserFile {
    fn layer(&self) -> Layer {
        Layer::User
    }

    /// D3's second column: the file, because this one is actually opened.
    fn source(&self) -> Source {
        Source::named(self.path().display().to_string())
    }

    fn read(&self) -> Result<Table, SourceFailure> {
        self.file
            .read()
            .map(Option::unwrap_or_default)
            .map_err(|refusal| SourceFailure::new(refusal.to_string()))
    }
}

/// [ADR-0014] D1's layer 3, which is [ADR-0009] D1's manifest.
///
/// **One file, one reader.** This is an adapter over
/// [`ManifestFile`] rather than a second parse
/// of the same bytes, which is the shape [`crate::manifest::port`] named on
/// 2026-09-04: the manifest is read whole and the layer's document is
/// [`Manifest::contribution`](crate::manifest::Manifest::contribution)'s.
/// `[[validator]]` is therefore not in this layer at all — see that record's
/// own Update for the reading and the one it was chosen over.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    manifest: ManifestFile,
}

impl ProjectFile {
    /// Layer 3 at the root of a working directory.
    #[must_use]
    pub fn in_directory(working_directory: WorkingDirectory) -> Self {
        Self {
            manifest: ManifestFile::in_directory(working_directory, file_ceiling()),
        }
    }

    /// The manifest itself, for a caller that wants ADR-0009's whole value
    /// rather than ADR-0014's layer.
    #[must_use]
    pub const fn manifest(&self) -> &ManifestFile {
        &self.manifest
    }

    /// The file, whether or not it is there.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.manifest.path()
    }
}

impl LayerSource for ProjectFile {
    fn layer(&self) -> Layer {
        Layer::Project
    }

    fn source(&self) -> Source {
        self.manifest.source()
    }

    fn read(&self) -> Result<Table, SourceFailure> {
        let source = self.source();
        Ok(self
            .manifest
            .read()?
            .map(|manifest| manifest.contribution(source).document)
            .unwrap_or_default())
    }
}

/// The two files [ADR-0014] D1's layers 2 and 3 are read from.
///
/// A parameter rather than something [`resolve`] reads for itself, for the
/// reason the environment is one: a check owns its own scratch home and its own
/// working directory and is an ordinary caller writing to the paths the product
/// writes to, rather than a fake standing in for a filesystem.
///
/// **An absent half is absent rather than empty-and-named.** A layer this
/// binary did not open prints `(not set)` against its own label, because a
/// source column naming `~/.zaru/config.toml` would claim a reading that did
/// not happen — which is the one thing D3's block exists to prevent.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Files {
    user: Option<UserFile>,
    project: Option<ProjectFile>,
}

impl Files {
    /// Neither layer, for a caller with no home and no working directory.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Layer 2 under `home` and layer 3 in `working_directory`, where each is
    /// given.
    #[must_use]
    pub fn at(home: Option<&Path>, working_directory: Option<WorkingDirectory>) -> Self {
        Self {
            user: home.map(UserFile::under),
            project: working_directory.map(ProjectFile::in_directory),
        }
    }

    /// The two files this process would read.
    ///
    /// A machine with no home directory has no layer 2, and a process whose
    /// working directory cannot be canonicalised has no layer 3. Neither is a
    /// failure: no file was opened, so the block says so.
    #[must_use]
    pub fn from_process() -> Self {
        Self::at(
            crate::config::home::default_root().as_deref(),
            std::env::current_dir()
                .ok()
                .and_then(|here| WorkingDirectory::at(here).ok()),
        )
    }

    /// Layer 2, if this caller has one.
    #[must_use]
    pub const fn user(&self) -> Option<&UserFile> {
        self.user.as_ref()
    }

    /// Layer 3, if this caller has one.
    #[must_use]
    pub const fn project(&self) -> Option<&ProjectFile> {
        self.project.as_ref()
    }

    /// Both, as the fold reads them.
    fn sources(&self) -> Vec<&dyn LayerSource> {
        let mut sources: Vec<&dyn LayerSource> = Vec::with_capacity(2);
        if let Some(user) = &self.user {
            sources.push(user);
        }
        if let Some(project) = &self.project {
            sources.push(project);
        }
        sources
    }
}

/// Every key this binary declares.
///
/// [ADR-0012]'s eleven and [ADR-0009]'s two, from those records' own `declare`,
/// plus [ADR-0001]'s `runtime.tier` and `runtime.max_iterations` from that
/// record's own `field`. **Nothing is spelled here**, which is [ADR-0014]'s
/// Neutral section: "Each record owns its own keys; this one owns how they
/// resolve."
///
/// **The three that arrived on 2026-09-05 are what makes a real `zaru.toml`
/// loadable at all.** Until then this binary declared sixteen keys and none of
/// them was one ADR-0009 D1's own worked manifest sets, so a file in that
/// record's shape was refused by ADR-0014 D5 as an unknown key the moment
/// layer 3 could be read.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn schema() -> Schema {
    let declared = crate::providers::declare(Schema::new());
    let declared = crate::manifest::declare(declared);
    declared
        .with(crate::runtime::key(), crate::runtime::field())
        .with(
            crate::runtime::max_iterations_key(),
            crate::runtime::max_iterations_field(),
        )
}

/// Fold the layers this binary can read, over a caller's environment.
///
/// Three of five: layer 1 compiled in, layer 4 from the `ZARU_*` pairs the
/// caller passes, layer 5 from the flags. **Layers 2 and 3 are absent rather
/// than empty-and-named**, so D3's block prints `(not set)` against their own
/// labels rather than against a file this harness never opened — a source
/// column naming `~/.zaru/config.toml` would claim a reading that did not
/// happen.
///
/// The environment is a parameter for the reason
/// [`crate::config::environment::read`] takes one: `std::env::set_var` is
/// `unsafe` in this edition and the workspace denies `unsafe_code`, so a check
/// that read the process's own environment would be a check whose answer
/// depends on whatever the runner was started with. [`resolve_from_process`]
/// is the product path.
///
/// # Errors
///
/// [`LoadFailure`].
pub fn resolve(
    overrides: &Overrides,
    variables: impl IntoIterator<Item = (String, String)>,
    files: &Files,
) -> Result<Resolution, LoadFailure> {
    let schema = schema();
    let built_in = BuiltIn::new();
    let flags = Flags::of(overrides);

    let mut sources: Vec<&dyn LayerSource> = vec![&built_in, &flags];
    sources.extend(files.sources());
    let mut contributions = gather(sources).map_err(LoadFailure::Source)?;
    contributions.push(Contribution::new(
        Layer::Environment,
        Layer::Environment.default_source(),
        environment::read(&schema, variables).map_err(LoadFailure::Refused)?,
    ));

    Resolution::resolve(&schema, contributions).map_err(LoadFailure::Refused)
}

/// Fold the layers over this process's own environment.
///
/// # Errors
///
/// [`LoadFailure`].
pub fn resolve_from_process(overrides: &Overrides) -> Result<Resolution, LoadFailure> {
    resolve(overrides, std::env::vars(), &Files::from_process())
}
