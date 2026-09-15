// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which kind of model an alias runs on, and where the work happens.
//!
//! # Two axes, because [ADR-0001] D3's table has two
//!
//! D3's iteration defaults are a grid: its columns are "Local model" and "BYO
//! frontier key", and its `linked` row reads "3 local, 8 offloaded" and "5
//! local, 12 offloaded". Those are **two different questions** — what kind of
//! model is answering, and whether the work happens on this machine — and a
//! single word cannot carry both. So [`Inference`] is the column and
//! [`Placement`] is the split inside a cell.
//!
//! This module builds neither of D3's numbers. **ADR-0001's own arc owns that
//! table**; what is here is the pair of axes a caller has to know before the
//! table can be indexed at all, declared under directive 20 of 2026-09-05 so
//! that arc has something to read.
//!
//! # The key is `inference.<alias>` and not `model.<alias>.inference`, and that
//! was measured rather than preferred
//!
//! Directive 20 spelled the key `model.<alias>.inference`. **It cannot be
//! that**, and the reason is a property of [ADR-0014]'s own value model rather
//! than a taste: `model.<alias>` holds the model identifier, so a document
//! carrying both would need `model.default` to be text and a table at once.
//! Measured on 2026-09-05 by resolving exactly that schema:
//!
//! - writing the nested key second gives ``"`model.default` in user config
//!   (layer 2) is declared as text and was given a table"``;
//! - writing it **first** is worse — the later write replaces the table
//!   wholesale and the document comes back `{"default": Text("a-model")}` with
//!   the inference setting **silently gone**, which is ADR-0014 D5's "a typo
//!   that silently does nothing is the worst outcome of any config system"
//!   arriving without even a typo.
//!
//! The sibling spelling keeps every property the directive wanted and loses
//! none: the axis is configuration, it resolves through all five layers, it is
//! declared beside `model.<alias>` in the same [`fields`](super::fields) call,
//! and `ZARU_INFERENCE_DEFAULT` is settable — whereas the nested spelling would
//! also have cost [ADR-0012] D4's own worked `ZARU_MODEL_DEFAULT`, which this
//! arc landed a check to keep producible.
//! `an_inference_key_and_a_model_key_cannot_be_nested_inside_one_another` pins
//! the measurement so the nested spelling cannot be re-proposed without a red.
//!
//! # No URL is inspected
//!
//! Neither axis is derived from an endpoint. A `localhost` address is not a
//! promise that inference is local and a public one is not a promise that it is
//! not, and parsing a URL to guess would be both a dependency and a security
//! claim resting on string-matching a host. The axis is configured, and where
//! no layer sets it the **provider kind** supplies the default.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::config::Key;
use crate::providers::alias::ModelAlias;
use crate::providers::kind::ProviderKind;
use core::fmt;

/// Why an inference axis was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceRefused {
    /// The value named neither axis.
    NoSuchAxis {
        /// The key, as this record spells it.
        key: Key,
        /// The value offered.
        offered: String,
    },
}

impl fmt::Display for InferenceRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchAxis { key, offered } => write!(
                f,
                "`{key}` was set to {offered:?}, which names no inference axis; there are exactly \
                 two, \"local\" and \"frontier\", and they are the tier table's own two columns",
            ),
        }
    }
}

impl std::error::Error for InferenceRefused {}

/// What kind of model answers for an alias — [ADR-0001] D3's two columns.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Inference {
    /// D3's "Local model" column — a model on the user's own hardware.
    Local,
    /// D3's "BYO frontier key" column — a frontier model behind the user's own
    /// credential.
    Frontier,
}

impl Inference {
    /// Both axes, in ADR-0001 D3's own column order.
    pub const ALL: [Self; 2] = [Self::Local, Self::Frontier];

    /// The first segment of every key that names an inference axis.
    pub const TABLE: &'static str = "inference";

    /// The axis's name, as it is written in configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Frontier => "frontier",
        }
    }

    /// What a provider kind implies when no layer sets the axis.
    ///
    /// `ollama` is local, and every other kind is frontier. That is a
    /// **default** rather than a constraint: a user running an
    /// OpenAI-compatible server on their own machine says so by setting the
    /// key, and nothing here inspects an endpoint to guess — see the module
    /// documentation.
    #[must_use]
    pub const fn of(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Ollama => Self::Local,
            ProviderKind::Anthropic
            | ProviderKind::OpenAiCompatible
            | ProviderKind::Gemini
            | ProviderKind::Aegis => Self::Frontier,
        }
    }

    /// Take an axis from a configured value.
    ///
    /// # Errors
    ///
    /// [`InferenceRefused::NoSuchAxis`] when the value names neither.
    pub fn parse(key: &Key, offered: &str) -> Result<Self, InferenceRefused> {
        Self::ALL
            .into_iter()
            .find(|axis| axis.as_str() == offered)
            .ok_or_else(|| InferenceRefused::NoSuchAxis {
                key: key.clone(),
                offered: offered.to_string(),
            })
    }

    /// The configuration key that sets this axis for one alias.
    ///
    /// `inference.default`, `inference.fast`, and so on — a **sibling** of
    /// `model.<alias>` rather than a child of it. See the module documentation
    /// for the measurement that decided it.
    ///
    /// # Panics
    ///
    /// Never; the spellings are this module's own and none is a shape
    /// [`Key::new`](crate::config::Key::new) refuses.
    #[must_use]
    pub fn key(alias: ModelAlias) -> Key {
        Key::new(&format!("{}.{}", Self::TABLE, alias.as_str()))
            .expect("an alias spelling is a well-formed configuration key")
    }
}

impl fmt::Display for Inference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where the work happens — the other half of [ADR-0001] D3's `linked` row.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Placement {
    /// The work runs on this machine.
    Local,
    /// The work is handed to the orchestrator, which is D3's "offloaded".
    Offloaded,
}

impl Placement {
    /// Both placements.
    pub const ALL: [Self; 2] = [Self::Local, Self::Offloaded];

    /// The placement's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Offloaded => "offloaded",
        }
    }

    /// Where work runs, given the kind an alias resolved to.
    ///
    /// **Local unless the `aegis` kind is resolved**, under directive 20 of
    /// 2026-09-05. That is the only kind that hands work to something else:
    /// every other kind is the harness calling a model API from this machine,
    /// which is a network request rather than an offload. ADR-0001 D3's
    /// "offloaded" column is about *where the loop runs*, not about where the
    /// weights are.
    ///
    /// **Not configurable, and deliberately.** ADR-0012 D6 has the harness
    /// negotiate with the orchestrator before offloading, so a key that let a
    /// configuration declare "this is offloaded" would be a second answer to a
    /// question that record settles by asking.
    #[must_use]
    pub const fn of(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::Aegis => Self::Offloaded,
            ProviderKind::Anthropic
            | ProviderKind::OpenAiCompatible
            | ProviderKind::Ollama
            | ProviderKind::Gemini => Self::Local,
        }
    }
}

impl fmt::Display for Placement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
