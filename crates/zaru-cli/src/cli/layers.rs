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
    ConfigRefused, Contribution, Layer, LayerSource, Resolution, Schema, Source, SourceFailure,
    Table, Value, environment, gather,
};
use crate::providers::ModelAlias;
use core::fmt;

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
) -> Result<Resolution, LoadFailure> {
    let schema = schema();
    let built_in = BuiltIn::new();
    let flags = Flags::of(overrides);

    let mut contributions = gather([&built_in as &dyn LayerSource, &flags as &dyn LayerSource])
        .map_err(LoadFailure::Source)?;
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
    resolve(overrides, std::env::vars())
}
