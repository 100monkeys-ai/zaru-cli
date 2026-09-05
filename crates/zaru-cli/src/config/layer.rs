// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0014 D1's five layers, and what a contribution says it came from.
//!
//! # Precedence is the type's, not a comparison somebody wrote
//!
//! [`Layer`]'s variants are declared lowest first and its [`Ord`] is derived,
//! so "higher wins" is a property of the enum rather than a rule the fold
//! restates. A fold that compared ranks by hand would be a second statement
//! of the precedence order, and a second statement is one that can disagree
//! with the first.
//!
//! # There is no pin, and that is a missing field rather than a missing check
//!
//! D1: "There is no layer above flags and no way for a lower layer to pin a
//! value against a higher one — a 'final' mechanism turns a precedence
//! question into a search." A [`Contribution`] therefore has **no field a pin
//! could occupy**, which is the same argument D4 makes about secrets in
//! configuration files, applied to precedence. Nothing has to check for a pin
//! because nothing can express one.
//!
//! # Layer and source are two different things and D3 prints both
//!
//! D3's explain block has a layer number, a source, a value and a marker:
//!
//! ```text
//! 3  ./zaru.toml       8          ← effective
//! ```
//!
//! The number is the layer; `./zaru.toml` is the *source*, which the layer
//! does not determine — layer 2 is whichever file the loader was pointed at,
//! and layer 4's shown name depends on the key being explained. So [`Source`]
//! is carried beside the layer rather than derived from it.

use crate::config::key::Key;
use crate::config::value::Table;
use core::fmt;

/// One of ADR-0014 D1's five layers, ordered lowest to highest.
///
/// The derived [`Ord`] follows declaration order, so `BuiltIn < User <
/// Project < Environment < Flag` and the highest layer that set a key wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// D1 layer 1 — compiled in.
    BuiltIn,
    /// D1 layer 2 — `~/.zaru/config.toml`.
    User,
    /// D1 layer 3 — `./zaru.toml`, which is also [ADR-0009]'s manifest.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    Project,
    /// D1 layer 4 — `ZARU_*`.
    Environment,
    /// D1 layer 5 — `--tier`, `--model`, and the rest.
    Flag,
}

impl Layer {
    /// Every layer, lowest first.
    ///
    /// The length is annotated, so a sixth variant fails to compile here as
    /// well as in every exhaustive match below.
    pub const ALL: [Self; 5] = [
        Self::BuiltIn,
        Self::User,
        Self::Project,
        Self::Environment,
        Self::Flag,
    ];

    /// D1's own number for this layer, 1 through 5.
    ///
    /// D3's explain block prints it, so it is the record's number rather than
    /// an index into [`Layer::ALL`] — an index would silently renumber every
    /// row if a layer were ever inserted rather than appended.
    #[must_use]
    pub const fn number(self) -> u8 {
        match self {
            Self::BuiltIn => 1,
            Self::User => 2,
            Self::Project => 3,
            Self::Environment => 4,
            Self::Flag => 5,
        }
    }

    /// What D1's table calls this layer.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::User => "user config",
            Self::Project => "project config",
            Self::Environment => "environment",
            Self::Flag => "flag",
        }
    }

    /// Whether a value arriving in this layer is scanned for the credential
    /// shapes [ADR-0007] D2 names.
    ///
    /// **Two of the five say yes, and which two is a decision rather than an
    /// oversight.** ADR-0014 Trigger clause 3 says "A *config file*
    /// containing a credential-shaped value is rejected at load", and D4's
    /// reasoning is about files specifically: "A config file gets committed
    /// to a repository. That is not a hypothetical; it is the single most
    /// common way a token leaks." Layers 2 and 3 are those files.
    ///
    /// Layer 4 and layer 5 are deliberately **not** scanned, and that is the
    /// open question rather than an answer to it. [ADR-0016] D2's worked
    /// remedy offers `ZARU_ANTHROPIC_KEY=<key>` — an environment variable
    /// whose whole purpose is to carry a bearer value — while ADR-0014 D4
    /// says configuration holds a reference and never a credential. Whether
    /// those can both hold is on [operations/adr-status] under open questions
    /// and refusing the environment here would answer it in code, which is
    /// exactly what [Agent lessons] §5 forbids. Layer 1 is compiled in and is
    /// ours, so a credential there would be a defect in this repository
    /// rather than in a user's file.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    /// [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status
    /// [Agent lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/agent-lessons
    #[must_use]
    pub const fn refuses_credential_shaped_values(self) -> bool {
        match self {
            Self::User | Self::Project => true,
            Self::BuiltIn | Self::Environment | Self::Flag => false,
        }
    }

    /// What D3's second column shows for this layer when nothing named a
    /// source for it.
    ///
    /// D3's block prints a source on rows that set nothing —
    /// `4  ZARU_MAX_ITER  (not set)` — so a layer's shown name cannot depend
    /// on there having been a contribution. The environment's name is derived
    /// from the key being explained; every other layer with no file falls
    /// back to its own label, which is what D3's `built-in` and `flag` rows
    /// already show.
    #[must_use]
    pub fn default_source(self) -> Source {
        match self {
            Self::Environment => Source::Environment,
            other => Source::named(other.label()),
        }
    }

    /// The layer whose [`Layer::label`] this is, if it is one.
    ///
    /// Walked from [`Layer::ALL`] rather than matched against literals, so a
    /// sixth layer is reachable the moment it is declared — the shape
    /// [`Tier::named`](crate::runtime::Tier::named) and
    /// [`Flag::named`](crate::cli::Flag::named) already use.
    ///
    /// **The label is the round trip, not a second spelling.** [ADR-0010] D1's
    /// `meta.toml` records which layer supplied a session's tier, and it
    /// records it by the same word [ADR-0014] D3's block prints in its supplier
    /// column, so a person reading the file and a person reading the block see
    /// one vocabulary.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn named(offered: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.label() == offered)
    }

    /// This layer's index into [`Layer::ALL`].
    ///
    /// Read out of `ALL` rather than from the variant's discriminant, so the
    /// two cannot disagree if a layer is ever inserted rather than appended.
    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|candidate| *candidate == self)
            .expect("Layer::ALL contains every layer")
    }

    /// Whether D6's escalation ceiling applies to values arriving here.
    ///
    /// D6 governs the *project* layer alone: "A repository the user cloned
    /// must not be able to configure its way to more privilege than the user
    /// granted." The user's own file, their environment and their flags are
    /// the grant, so none of them is constrained by it.
    #[must_use]
    pub const fn bound_by_the_escalation_ceiling(self) -> bool {
        match self {
            Self::Project => true,
            Self::BuiltIn | Self::User | Self::Environment | Self::Flag => false,
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where a layer's values came from, as D3's explain block renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A name that is the same whichever key is being explained — a file
    /// path, or the layer's own label where there is no file.
    Named(String),
    /// The process environment, whose shown name is the variable a key maps
    /// to and therefore differs per key.
    Environment,
}

impl Source {
    /// A source with a fixed name.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self::Named(name.into())
    }

    /// What D3's second column shows for this source and this key.
    #[must_use]
    pub fn shown_for(&self, key: &Key) -> String {
        match self {
            Self::Named(name) => name.clone(),
            Self::Environment => crate::config::environment::variable_name(key),
        }
    }
}

/// What one layer offers, before anything has been validated or merged.
///
/// **There is no field a pin could occupy.** See the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    /// Which of D1's five layers this is.
    pub layer: Layer,
    /// What D3's explain block calls it.
    pub source: Source,
    /// What it sets, whole. An empty table is a layer that set nothing, which
    /// is a different thing from a layer that is absent only in that D3
    /// renders both as `(not set)`.
    pub document: Table,
}

impl Contribution {
    /// Offer a document at a layer, from a named source.
    #[must_use]
    pub fn new(layer: Layer, source: Source, document: Table) -> Self {
        Self {
            layer,
            source,
            document,
        }
    }
}
