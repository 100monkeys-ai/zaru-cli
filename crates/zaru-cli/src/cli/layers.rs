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
//! Three keys: `runtime.tier` from `--runtime`, `model.default` from
//! `--model`, and `tools.mode` from `--mode`, which arrived on 2026-09-05 with
//! [ADR-0011] D3's key. A flag cannot reach any other key, which is a property
//! of [`Overrides`] having three fields rather than of a check.
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
        // [ADR-0012] D3's window, for the two kinds that have a source for
        // one. **Here rather than as a fallback where the value is read**,
        // and the reason is [ADR-0014] D6: `ProjectPolicy::LowerOnly` refuses
        // a project that raises what the layers below granted, and
        // `config::resolve` says in as many words that "a ceiling the layers
        // below never granted is not raised by being set: there is nothing to
        // exceed". A default living only in a client is a default no project
        // layer can be measured against, so the ceiling would not bind and a
        // repository the reader cloned could widen the window their own
        // server was told to serve.
        //
        // It also puts both numbers in front of `zaru config explain`, which
        // is D4's whole point: a resolution the user cannot explain is one
        // they cannot fix.
        //
        // **`openai-compatible` has no row**, because that kind has no
        // default window — see its client. The consequence is stated rather
        // than hidden: with nothing granted, the ceiling has nothing to
        // compare a project's value against for that kind.
        document.insert_path(
            &crate::providers::ProviderKind::Gemini.context_tokens_key(),
            Value::Integer(built_in_window(
                crate::providers::gemini::CONTEXT_WINDOW_TOKENS,
            )),
        );
        document.insert_path(
            &crate::providers::ProviderKind::Ollama.context_tokens_key(),
            Value::Integer(built_in_window(
                crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS,
            )),
        );
        // Whether the shell holds the mouse. On by default, so the wheel
        // scrolls the pane; see `terminal::mouse` for the trade and its cost.
        document.insert_path(
            &crate::terminal::mouse::key(),
            Value::Bool(crate::terminal::mouse::BUILT_IN),
        );
        Self { document }
    }
}

/// A window as [ADR-0014]'s integer value carries it.
///
/// # Panics
///
/// Never. Both callers pass a provider's own published window, and neither is
/// anywhere near `i64::MAX`; the saturation is here so that a third kind
/// declaring an absurd one is a clamped number rather than a panic in the
/// layer every invocation reads.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
fn built_in_window(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
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
        if let Some(mode) = &overrides.mode {
            document.insert_path(&crate::tools::mode::key(), Value::Text(mode.clone()));
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

/// The largest response body [ADR-0011] D1's `web.fetch` will accept.
///
/// **One mebibyte, the same magnitude as [`SEARCH_CEILING_BYTES`] and
/// deliberately not the same constant** — for the reason that one gives about
/// the configuration-file ceiling: the two are answerable by different records
/// and a single constant would make a later change to one silently change the
/// other. A page of markup is a fraction of this; what the number is really
/// bounding is how much of a model-chosen response the harness will hold in
/// memory and write into a session directory.
///
/// **Over it is a refusal rather than a truncation** — see
/// [`BodyCeiling`](crate::web::BodyCeiling) for why D5's own sentence decides
/// that, and note that this is *not* what a model is shown: D5's output budget
/// truncates the capture afterwards, and it is much smaller.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const FETCH_BODY_CEILING_BYTES: u64 = 1 << 20;

/// How long one `web.fetch` may take, start to finished body.
///
/// **Thirty seconds, and it is deliberately far shorter than the ten minutes
/// [`EXCHANGE_TIMEOUT`](crate::providers::transport::EXCHANGE_TIMEOUT) a model
/// completion gets.** A completion is slow on purpose — a large prompt on a
/// slow link is the case that number protects — whereas a page that has not
/// answered in thirty seconds will not be useful to the turn that asked for
/// it, and a tool call happens *inside* a turn that still has to reach the
/// model afterwards. A fetch that outlived the exchange around it would spend
/// the turn's budget on the cheaper half.
pub const FETCH_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(30);

/// How many redirects **within one host** a `web.fetch` follows.
///
/// **Three.** `reqwest`'s own default is ten, and that number is for chains
/// that may cross hosts, which this policy never does — a redirect that leaves
/// the host is refused whatever this says. Within one host the redirects that
/// actually occur are a scheme upgrade and a path canonicalisation, so three
/// is one more than the observed shapes need and small enough that a server
/// looping on itself is cut off quickly rather than after ten round trips
/// inside the turn's own timeout.
pub const FETCH_REDIRECT_LIMIT: usize = 3;

/// The three bounds `web.fetch` runs under in this binary.
///
/// # Panics
///
/// Never. [`FETCH_BODY_CEILING_BYTES`] and [`FETCH_TIMEOUT`] are not zero, and
/// [`FETCH_REDIRECT_LIMIT`] is allowed to be.
#[must_use]
pub fn fetch_bounds() -> crate::web::FetchBounds {
    crate::web::FetchBounds {
        body: crate::web::BodyCeiling::new(FETCH_BODY_CEILING_BYTES)
            .expect("a mebibyte is not zero"),
        timeout: crate::web::FetchTimeout::new(FETCH_TIMEOUT).expect("thirty seconds is not zero"),
        redirects: crate::web::RedirectLimit::new(FETCH_REDIRECT_LIMIT),
    }
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

    /// The two files this process would read, with layer 2 under `home`.
    ///
    /// **`home` is the caller's, since 2026-09-27**, and was
    /// `config::home::default_root()` here — a reading of `$HOME` inside the
    /// loader, so a caller that had named a home for the session store still
    /// had layer 2 read from the person's own. See [`crate::config::Home`].
    ///
    /// A machine with no home directory has no layer 2, and a process whose
    /// working directory cannot be canonicalised has no layer 3. Neither is a
    /// failure: no file was opened, so the block says so.
    ///
    /// **The working directory is [`WorkingDirectory::of_this_process`] and
    /// not a reading of its own**, since 2026-09-06. It was
    /// `std::env::current_dir()` here, one of five spellings of one rule, and
    /// this was the fifth — found by `corpus_one_thing_decides_a_working_directory`
    /// rather than by anybody remembering it, which is the whole reason that
    /// check is a source walk. [ADR-0014] D1's layer 3 and [ADR-0010] D4's
    /// `--continue` scope must be the same directory or a project's
    /// `zaru.toml` is read from one place and its sessions looked for in
    /// another.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn of_this_process(home: &crate::config::Home) -> Self {
        Self::at(home.root(), WorkingDirectory::of_this_process().ok())
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
/// [ADR-0012]'s fifteen and [ADR-0009]'s two, from those records' own
/// `declare`, plus [ADR-0011]'s `tools.allowlist` and `tools.mode` from its
/// own, plus [ADR-0002]'s `tips` from
/// [`crate::compose::tips::declare`], plus [ADR-0001]'s `runtime.tier` and
/// `runtime.max_iterations` from that record's own `field`. **Nothing is
/// spelled here**, which is
/// [ADR-0014]'s Neutral section: "Each record owns its own keys; this one owns
/// how they resolve."
///
/// **The three that arrived on 2026-09-05 are what makes a real `zaru.toml`
/// loadable at all.** Until then this binary declared sixteen keys and none of
/// them was one ADR-0009 D1's own worked manifest sets, so a file in that
/// record's shape was refused by ADR-0014 D5 as an unknown key the moment
/// layer 3 could be read.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn schema() -> Schema {
    let declared = crate::providers::declare(Schema::new());
    let declared = crate::manifest::declare(declared);
    let declared = crate::tools::allowlist::declare(declared);
    let declared = crate::tools::mode::declare(declared);
    let declared = crate::compose::tips::declare(declared);
    let declared = crate::credentials::grant::declare(declared);
    let declared = crate::compose::persona::declare(declared);
    let declared = crate::terminal::mouse::declare(declared);
    declared
        .with(crate::runtime::key(), crate::runtime::field())
        .with(
            crate::runtime::max_iterations_key(),
            crate::runtime::max_iterations_field(),
        )
        .with(
            crate::runtime::max_tool_exchanges_key(),
            crate::runtime::max_tool_exchanges_field(),
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

/// Fold the layers over this process's own environment, with layer 2 under
/// `home`.
///
/// # Errors
///
/// [`LoadFailure`].
pub fn resolve_from_process(
    home: &crate::config::Home,
    overrides: &Overrides,
) -> Result<Resolution, LoadFailure> {
    resolve(overrides, std::env::vars(), &Files::of_this_process(home))
}

/// How long a child process started by [ADR-0011] D1's `cmd.run` may run.
///
/// **Two minutes.** [`crate::process::ceiling`] says where the number has to
/// come from — "no record names a wall-clock bound for a command, so a ceiling
/// invented by the thing being bounded is not a ceiling … the numbers are the
/// composition's to supply" — and this is the composition supplying it.
///
/// **The provider's ten minutes is deliberately not reused.**
/// [`crate::providers::transport::EXCHANGE_TIMEOUT`] bounds a whole exchange
/// with a model that may think for minutes before it answers; a `cmd.run` is a
/// build, a test suite or a linter,
/// and [ADR-0009]'s own Negative consequence is that "a project whose test
/// suite takes minutes makes the loop impractical". Two minutes is short
/// enough that a hung command is a recognisable event rather than a session
/// nobody can leave, and long enough that an ordinary build is not cut off.
///
/// A command killed here reports `137`, per ADR-0009 D3's shell-convention
/// rule — and `ValidatorOutput` cannot say the harness was what killed it,
/// which that record raises and does not settle.
///
/// **No configuration key is declared for it**, because [ADR-0014]'s Neutral
/// consequence leaves each record its own keys and [ADR-0011] names none.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const PROCESS_CEILING: core::time::Duration = core::time::Duration::from_secs(120);

/// [`PROCESS_CEILING`] as [`crate::process::Spawn`] takes it.
///
/// # Panics
///
/// Never. [`PROCESS_CEILING`] is not zero.
#[must_use]
pub fn process_ceiling() -> crate::process::ProcessCeiling {
    crate::process::ProcessCeiling::new(PROCESS_CEILING).expect("two minutes is not zero")
}

/// How much of one tool's output the model is shown, in bytes.
///
/// **Thirty-two kibibytes.** [ADR-0011] D5 "requires truncation and names no
/// size", and [`crate::tools::OutputBudget`] refuses zero and takes the rest
/// from its caller.
///
/// It is far smaller than [`FILE_CEILING_BYTES`] and [`SEARCH_CEILING_BYTES`],
/// which are both a mebibyte, and the difference is the question each answers.
/// Those two bound how much this harness will read into memory. **This one
/// bounds how much of what it read goes into a context window**, and a
/// mebibyte of one tool result would leave no room for the conversation it is
/// part of — see [`context_limits`], against whose window a byte is counted.
///
/// A small budget is honest only because nothing is lost: D5 has the whole
/// output written to the session directory with the path shown, which
/// [`crate::tools::SessionOverflow`] does, and refuses to clip at all when
/// there is nowhere to preserve it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const OUTPUT_BUDGET_BYTES: usize = 32 * (1 << 10);

/// How much of a call's arguments [ADR-0011] D3's question formats.
///
/// **4 KiB, and the number's reason is the screen.** This is not
/// [`OUTPUT_BUDGET_BYTES`] and the two answer different questions: that one
/// bounds how much of a tool's *output* goes into a context window, and this
/// one bounds how much of an *argument* is formatted for a person to read
/// before they answer a question about it. What a person can read is bounded
/// by the terminal — thirty rows of a hundred columns is three thousand cells
/// and twenty-four of forty is nine hundred and sixty — so four kibibytes is
/// more than the widest terminal this workspace measures can show, and small
/// enough that a multi-megabyte write is never formatted at all.
///
/// Nothing is lost by the cut: D5's own elision marks what went, and the
/// bytes themselves are what the act writes to disk whether or not the
/// preview showed them.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const PREVIEW_BUDGET_BYTES: usize = 4 * (1 << 10);

/// [`PREVIEW_BUDGET_BYTES`] as the tool surface takes it.
///
/// # Panics
///
/// Never. [`PREVIEW_BUDGET_BYTES`] is not zero.
#[must_use]
pub fn preview_budget() -> crate::tools::OutputBudget {
    crate::tools::OutputBudget::new(PREVIEW_BUDGET_BYTES).expect("4 KiB is not zero")
}

/// [`OUTPUT_BUDGET_BYTES`] as the tool surface takes it.
///
/// # Panics
///
/// Never. [`OUTPUT_BUDGET_BYTES`] is not zero.
#[must_use]
pub fn output_budget() -> crate::tools::OutputBudget {
    crate::tools::OutputBudget::new(OUTPUT_BUDGET_BYTES).expect("32 KiB is not zero")
}

/// The window a session carries when no provider could be prepared.
///
/// **4,096, and it is the smallest window any kind in this binary states.**
/// A session whose `prepare` refused cannot run a turn — it opens, says what
/// is wrong, and its layer 6 never grows — so no threshold can be crossed
/// whatever this number is, and what it decides is only the second figure on
/// [ADR-0013] D6's row while the reader reads that refusal.
///
/// The smallest rather than the largest, because a row claiming more room
/// than any provider here offers is the same lie in miniature that the
/// composition's old single constant was. It is
/// [`crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS`] read
/// through this name rather than a second literal.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub const WINDOW_WHEN_NO_PROVIDER: u64 = crate::providers::ollama::endpoint::DEFAULT_CONTEXT_TOKENS;

/// One provider's window and the pressure threshold beneath it, as
/// [`zaru_core::context::Context`] takes them.
///
/// **`window` is a parameter and no longer a constant here, since
/// 2026-09-14.** What stood here was `CONTEXT_WINDOW_TOKENS`, 1,048,576, and
/// `PRESSURE_THRESHOLD_TOKENS` at three quarters of it — one model's number
/// cited from Google's page for `gemini-3.6-flash` and applied to every
/// provider this binary could reach, which its own documentation said was "a
/// real limit" and "wrong for it" the moment a second kind had a client.
/// Three kinds had clients when this changed. The window is now
/// [ADR-0012] D3's capability descriptor's, per kind, and reaches here
/// through [`crate::compose::Prepared::context_shape`]; the citation went
/// with it, to [`crate::providers::gemini::CONTEXT_WINDOW_TOKENS`].
///
/// **The three quarters stayed**, and it is still the one place that
/// fraction is written: [ADR-0013] D2 crosses "the window pressure
/// threshold" and D6 has the number visible continuously, and neither says
/// what the threshold is.
///
/// # Panics
///
/// When `window` is zero. Every product caller comes through
/// `Prepared::context_limits`, which cannot hold a zero: a descriptor with no
/// window is refused by `ProviderCapabilities::require_context_size` before a
/// `Prepared` exists, and a descriptor declaring zero is a client stating it
/// accepts nothing.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn context_limits(window: u64) -> zaru_core::context::ContextLimits {
    let threshold = window / 4 * 3;
    let window = zaru_core::context::ContextWindow::new(window).expect("the window is not zero");
    let threshold =
        zaru_core::context::PressureThreshold::new(threshold).expect("the threshold is not zero");
    zaru_core::context::ContextLimits::new(window, threshold)
        .expect("three quarters of a window is not above it")
}
