// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0001] D3's iteration defaults: **the one source of truth in code.**
//!
//! # The record asked for this in as many words
//!
//! D3's Negative consequence: "Per-tier, per-provider iteration defaults are a
//! two-dimensional table that has to stay consistent across the CLI, the docs,
//! and the system prompt. **It will drift unless there is one source of truth
//! in code.**" [`iterations`] is that source, and D3's twelve cells are
//! transcribed into it and nowhere else in this workspace.
//!
//! D3's table:
//!
//! ```text
//! | Tier        | Local model         | BYO frontier key     |
//! | bare        | 1                   | 1                    |
//! | contained   | 3                   | 5                    |
//! | linked      | 3 local, 8 offloaded| 5 local, 12 offloaded|
//! ```
//!
//! That is three axes rather than two: the tier, D3's column, and — in the
//! `linked` row — whether the work runs locally or is offloaded.
//!
//! # Every cell is spelled, so nothing arrives by default
//!
//! [`iterations`] matches on the whole triple with **no wildcard arm**, so all
//! twelve cells are written out. A fourth [`Tier`], a third [`Inference`] or a
//! third [`Placement`] is a compile error rather than a value silently taking
//! a neighbour's number.
//!
//! # The unavailable cells are checked against D1, not derived from it
//!
//! Eight cells hold a number and four hold nothing: nothing offloads at `bare`
//! or at `contained`, which is [ADR-0001] D1's Loop column — "none", "local",
//! "local, offloadable". Those four arms return [`None`] and they are
//! **written out here** rather than computed from
//! [`Engagement::r#loop`](super::Engagement).
//!
//! That is deliberate. Deriving one table from the other would make them one
//! source, and a check comparing them would be a mirror with a verdict
//! attached ([Verification lessons] §11). Written twice from the record's two
//! tables, they are two transcriptions that a check can hold against each
//! other, and the mutant that changes either one reddens it.
//!
//! # `bare` is one by definition, and the number is not chosen here
//!
//! D3: "`bare` has no loop, so its iteration count is one by definition —
//! there is no validator to refine against." That is why both of `bare`'s
//! cells are 1, and it is the only cell in the table with a stated derivation.
//!
//! # The axis question, decided
//!
//! D3's columns are "Local model" and "BYO frontier key". [ADR-0012] D3's
//! provider kinds are `anthropic`, `openai-compatible`, `ollama` and `aegis`,
//! and **they are not the same axis**: `openai-compatible` covers a local vLLM
//! and a hosted gateway alike, so no mapping exists between the two without
//! inventing one.
//!
//! Under Jeshua's directive of 2026-09-05 the axis is decided rather than
//! inferred: [`Inference`] is **declared per alias in configuration**, at
//! `model.<alias>.inference`, and [`Inference::resolved_for`] reads it.
//! [`Placement`] is [`Placement::Local`] unless ADR-0012 D3's `aegis` kind is
//! the resolved provider kind. Neither is guessed from a model name and
//! neither has a default in this module — a default here would be this
//! record choosing another record's configuration.
//!
//! # Nothing here names a tier to `zaru-core`
//!
//! [`ceiling`] hands back `zaru-core`'s own
//! [`Ceiling`](zaru_core::iteration::Ceiling), which is a number and carries
//! no tier. [ADR-0008] D5 says the loop "takes one and never chooses one", and
//! its `limits` module says ADR-0001 D3 owns the numbers. This is the boundary
//! where that ownership is exercised.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::config::{Key, KeyRefused, Resolution};
use crate::runtime::tier::Tier;
use core::fmt;
use zaru_core::iteration::Ceiling;

/// [ADR-0001] D3's column axis: where the model runs.
///
/// The record's own two columns, named from its own words. **Not**
/// [ADR-0012] D3's provider kind — see the module documentation.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inference {
    /// D3's "Local model" column.
    Local,
    /// D3's "BYO frontier key" column.
    Frontier,
}

impl Inference {
    /// Both of D3's columns.
    pub const ALL: [Self; 2] = [Self::Local, Self::Frontier];

    /// The value this axis is written as in configuration.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Frontier => "frontier",
        }
    }

    /// D3's own column heading, for a reader rather than a config file.
    #[must_use]
    pub const fn column(self) -> &'static str {
        match self {
            Self::Local => "Local model",
            Self::Frontier => "BYO frontier key",
        }
    }

    /// The value named exactly as configuration spells it, if it names one.
    #[must_use]
    pub fn named(offered: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|inference| inference.as_str() == offered)
    }

    /// The key one alias declares this axis at.
    ///
    /// `model.<alias>.inference`, per Jeshua's directive of 2026-09-05. The
    /// `model.<alias>` half of that path is [ADR-0012]'s and is declared by
    /// whoever owns that record's keys; **this module declares no
    /// [`Field`](crate::config::Field) for it and no schema**, because the key
    /// belongs to ADR-0012 and only its *reading* belongs to ADR-0001 D3.
    ///
    /// # Errors
    ///
    /// [`KeyRefused`] when `alias` is a spelling no configuration key can
    /// carry — an empty segment, a control character, surrounding whitespace.
    /// An alias arrives from configuration, so it is a boundary and is checked
    /// rather than assumed.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub fn key_for(alias: &str) -> Result<Key, KeyRefused> {
        Key::new(&format!("model.{alias}.inference"))
    }

    /// Read this axis for one alias out of a resolved configuration.
    ///
    /// **Read rather than chosen**, which is the whole point: D3's column is a
    /// property of what the user configured, and a module that guessed it from
    /// a model name would be inventing the axis this record leaves to
    /// configuration.
    ///
    /// # Errors
    ///
    /// [`InferenceRefused`], naming the key. An unset key is refused rather
    /// than defaulted — a built-in default belongs in [ADR-0014] D1's layer 1
    /// with the record that owns the key, not here.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub fn resolved_for(resolution: &Resolution, alias: &str) -> Result<Self, InferenceRefused> {
        let key = Self::key_for(alias).map_err(|refusal| InferenceRefused::UnusableAlias {
            refusal: Box::new(refusal),
        })?;

        let Some(value) = resolution.get(&key) else {
            return Err(InferenceRefused::NotSet { key });
        };
        let Some(text) = value.as_text() else {
            return Err(InferenceRefused::WrongShape {
                key,
                found: value.shape(),
            });
        };
        Self::named(text).ok_or_else(|| InferenceRefused::NoSuchInference {
            key,
            offered: text.escape_debug().to_string(),
        })
    }
}

impl fmt::Display for Inference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why D3's column could not be read for an alias.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceRefused {
    /// The alias is a spelling no configuration key can carry.
    UnusableAlias {
        /// Why the key could not be built. Boxed so this enum stays small.
        refusal: Box<KeyRefused>,
    },
    /// No layer set the key.
    NotSet {
        /// The key that was looked for.
        key: Key,
    },
    /// The key held something other than text.
    WrongShape {
        /// The key.
        key: Key,
        /// What shape it held. **Never the value.**
        found: &'static str,
    },
    /// The value named neither of D3's columns.
    NoSuchInference {
        /// The key.
        key: Key,
        /// The value offered, escaped.
        offered: String,
    },
}

impl fmt::Display for InferenceRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnusableAlias { refusal } => write!(
                f,
                "a model alias does not spell a configuration key: {refusal}"
            ),
            Self::NotSet { key } => write!(
                f,
                "no configuration layer set {key}, so ADR-0001 D3's iteration default cannot be \
                 read for that alias. Set it to \"local\" or \"frontier\""
            ),
            Self::WrongShape { key, found } => write!(
                f,
                "{key} holds {found}, and ADR-0001 D3's column is the text \"local\" or \
                 \"frontier\""
            ),
            Self::NoSuchInference { key, offered } => write!(
                f,
                "the key {key} was set to {offered:?}, which names neither of ADR-0001 D3's \
                 columns: \"local\" is its Local model column and \"frontier\" its BYO frontier \
                 key column"
            ),
        }
    }
}

impl std::error::Error for InferenceRefused {}

/// Whether the work runs on this machine or is offloaded.
///
/// D3's `linked` row is the only one that names both — "3 local, 8 offloaded"
/// — and [ADR-0001] D1's Loop column is why: `linked` is the only tier whose
/// loop is "local, offloadable".
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// On the user's own machine.
    Local,
    /// Accepted by cloud Zaru or AEGIS.
    Offloaded,
}

impl Placement {
    /// Both placements.
    pub const ALL: [Self; 2] = [Self::Local, Self::Offloaded];

    /// [ADR-0012] D3's provider kind that means the work is offloaded.
    ///
    /// The one place that word is spelled in this module. It is that record's
    /// vocabulary, transcribed under Jeshua's directive of 2026-09-05, and a
    /// `ProviderKind` type will convert into this rather than this growing a
    /// second list of kinds.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub const OFFLOADING_PROVIDER_KIND: &'static str = "aegis";

    /// Where work runs, given the provider kind that resolved.
    ///
    /// **Local unless the `aegis` kind resolved**, which is the directive of
    /// 2026-09-05 stated exactly. Nothing here consults the tier: a tier that
    /// cannot offload makes the *ceiling* unavailable, which is [`iterations`]'
    /// answer rather than this one's, so the two questions stay separable and
    /// a caller asking for an impossible pair is told so instead of being
    /// quietly corrected.
    #[must_use]
    pub fn for_resolved_provider_kind(kind: &str) -> Self {
        if kind == Self::OFFLOADING_PROVIDER_KIND {
            Self::Offloaded
        } else {
            Self::Local
        }
    }

    /// The placement's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Offloaded => "offloaded",
        }
    }
}

impl fmt::Display for Placement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// [ADR-0001] D3's iteration default, as a plain count.
///
/// `None` where the tier cannot offload, which is four of the twelve cells.
/// See the module documentation for why those four are written out rather than
/// derived from D1's Loop column.
///
/// **Every cell is spelled and there is no wildcard arm.**
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[must_use]
pub const fn iterations(tier: Tier, inference: Inference, placement: Placement) -> Option<u32> {
    match (tier, inference, placement) {
        // `bare` — D3: "no loop, so its iteration count is one by definition".
        (Tier::Bare, Inference::Local, Placement::Local)
        | (Tier::Bare, Inference::Frontier, Placement::Local) => Some(1),

        // `contained` — 3 on a local model, 5 on a frontier key.
        (Tier::Contained, Inference::Local, Placement::Local) => Some(3),
        (Tier::Contained, Inference::Frontier, Placement::Local) => Some(5),

        // `linked` — "3 local, 8 offloaded" and "5 local, 12 offloaded".
        (Tier::Linked, Inference::Local, Placement::Local) => Some(3),
        (Tier::Linked, Inference::Local, Placement::Offloaded) => Some(8),
        (Tier::Linked, Inference::Frontier, Placement::Local) => Some(5),
        (Tier::Linked, Inference::Frontier, Placement::Offloaded) => Some(12),

        // Nothing offloads below `linked`. D1's Loop column gives `bare`
        // "none" and `contained` "local"; only `linked` is "local,
        // offloadable".
        (Tier::Bare, Inference::Local, Placement::Offloaded)
        | (Tier::Bare, Inference::Frontier, Placement::Offloaded)
        | (Tier::Contained, Inference::Local, Placement::Offloaded)
        | (Tier::Contained, Inference::Frontier, Placement::Offloaded) => None,
    }
}

/// The same cell, as the bound `zaru-core`'s loop takes.
///
/// This is the boundary [ADR-0008] D5 describes from the other side: the loop
/// "takes one and never chooses one", and the number comes from ADR-0001 D3.
/// **`zaru-core` is handed a count and never a tier.**
///
/// `None` for the four cells [`iterations`] has none for. Every number D3
/// prints is at least one, so no cell of this table can produce the zero
/// `Ceiling::new` refuses — which is asserted rather than assumed.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
#[must_use]
pub fn ceiling(tier: Tier, inference: Inference, placement: Placement) -> Option<Ceiling> {
    iterations(tier, inference, placement).map(|count| {
        Ceiling::new(count).expect("ADR-0001 D3 prints no ceiling of zero; see the check")
    })
}
