// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0001] D1's three tiers, and the four columns its table gives each one.
//!
//! # The table is transcribed once, and every cell of it is a closed type
//!
//! D1's table:
//!
//! ```text
//! | Tier        | Membrane         | Loop               | Cortex        | Network                    |
//! | bare        | none             | none               | none          | model provider only        |
//! | contained   | local containers | local              | local         | model provider only        |
//! | linked      | local containers | local, offloadable | Nuclear Notes | model provider + platform  |
//! ```
//!
//! Each column is an enum whose variants are that column's distinct cells and
//! whose `as_str` is D1's own wording, and the whole row arrives from one
//! wildcard-free match in [`Tier::engagement`]. A fourth tier therefore fails
//! to compile in two places — that match, and [`Tier::ALL`]'s annotated length
//! — rather than silently taking a default row.
//!
//! [`Tier::has_membrane`] is **derived from the Membrane column** rather than
//! being a second match over the same three tiers. It was a second match until
//! 2026-09-05, which is the one-rule-in-two-places shape this module exists to
//! remove; [ADR-0011] D2's not-a-sandbox line is emitted on the answer, so the
//! two could not be allowed to disagree.
//!
//! # The names are not this module's to choose
//!
//! D1's Neutral consequence: the tier names "appear in the open-source
//! repository from the first commit and are therefore effectively permanent
//! once published". They were confirmed as written by Jeshua's directive of
//! 2026-09-05. Nothing here renames one.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use core::fmt;

/// How much of the platform is engaged, per [ADR-0001] D1.
///
/// Transcribed from that record, which names the three and says they are
/// "effectively permanent once published". [ADR-0011] D2's own table uses the
/// same three, and its enforcement differs across them.
///
/// **Fixed for the life of a session.** ADR-0001 D2: "Tier is resolved at
/// session start and is immutable for the life of a session... A membrane that
/// can be dropped mid-session is not a membrane." [ADR-0014] D7 restates it.
/// Nothing here can change a tier, because there is nothing to change: a tier
/// is a value a caller holds, not a field on anything. The value a *session*
/// holds is [`ResolvedTier`](super::ResolvedTier), which has no mutation
/// surface either.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// D1: "a plain agentic harness. No AEGIS, no account, no cortex, full
    /// tool capability. This is the on-ramp and it is not a trial."
    Bare,
    /// D1: "AEGIS runs locally. The membrane is containers; the 100monkeys
    /// loop runs on the user's machine."
    Contained,
    /// D1: "the account is attached. Nuclear Notes provides knowledge; cloud
    /// Zaru and AEGIS accept offloaded work."
    Linked,
}

impl Tier {
    /// Every tier ADR-0001 D1 names.
    ///
    /// The length is annotated, so a fourth variant fails to compile here as
    /// well as in [`Tier::engagement`]'s match.
    pub const ALL: [Self; 3] = [Self::Bare, Self::Contained, Self::Linked];

    /// The tier's name as ADR-0001 D1 spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bare => "bare",
            Self::Contained => "contained",
            Self::Linked => "linked",
        }
    }

    /// The tier named exactly as D1 spells it, if it names one.
    ///
    /// Walks [`Tier::ALL`] against [`Tier::as_str`] rather than carrying a
    /// second table of spellings, so a renamed tier cannot be accepted under
    /// its old name by a parser nobody updated.
    #[must_use]
    pub fn named(offered: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.as_str() == offered)
    }

    /// D1's whole row for this tier.
    ///
    /// **One match, no wildcard arm**, so a fourth tier is a compile error
    /// here and every column is answered for it deliberately.
    #[must_use]
    pub const fn engagement(self) -> Engagement {
        match self {
            Self::Bare => Engagement {
                membrane: Membrane::None,
                r#loop: Loop::None,
                cortex: Cortex::None,
                network: Network::ModelProviderOnly,
            },
            Self::Contained => Engagement {
                membrane: Membrane::LocalContainers,
                r#loop: Loop::Local,
                cortex: Cortex::Local,
                network: Network::ModelProviderOnly,
            },
            Self::Linked => Engagement {
                membrane: Membrane::LocalContainers,
                r#loop: Loop::LocalOffloadable,
                cortex: Cortex::NuclearNotes,
                network: Network::ModelProviderAndPlatform,
            },
        }
    }

    /// Whether D1's table plans a membrane at this tier.
    ///
    /// **Derived from D1's Membrane column**, not a second match. It says what
    /// the record plans and nothing about what is built: no tier encloses a
    /// tool call yet, which is why ADR-0011 D2's not-a-sandbox line is said at
    /// every tier. See [`SessionNotice`](crate::tools::SessionNotice).
    #[must_use]
    pub const fn has_membrane(self) -> bool {
        !matches!(self.engagement().membrane, Membrane::None)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One row of [ADR-0001] D1's table: what a tier engages.
///
/// Four fields and no fifth. A check destructures it exhaustively, so a column
/// added to D1 stops that check compiling rather than travelling unasserted.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engagement {
    /// D1's Membrane column.
    pub membrane: Membrane,
    /// D1's Loop column.
    ///
    /// Spelled `r#loop` because `loop` is a keyword and the column's name is
    /// the record's. Renaming it here would be one more place the table is
    /// spelled differently from the record.
    pub r#loop: Loop,
    /// D1's Cortex column.
    pub cortex: Cortex,
    /// D1's Network column.
    pub network: Network,
}

/// D1's Membrane column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Membrane {
    /// `bare`. There is no isolation boundary at all.
    None,
    /// `contained` and `linked`. AEGIS's containers.
    LocalContainers,
}

/// D1's Loop column.
///
/// [`Loop::LocalOffloadable`] is what makes
/// [`Placement::Offloaded`](super::Placement) available at a tier, and D3's
/// table is checked against this column rather than deriving from it — see
/// [`super::defaults`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    /// `bare`. D3: "`bare` has no loop, so its iteration count is one by
    /// definition — there is no validator to refine against."
    None,
    /// `contained`. The 100monkeys loop runs on the user's machine.
    Local,
    /// `linked`. Local, and cloud Zaru and AEGIS accept offloaded work.
    LocalOffloadable,
}

/// D1's Cortex column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cortex {
    /// `bare`. No knowledge substrate.
    None,
    /// `contained`. A local one.
    Local,
    /// `linked`. The product, named as
    /// [Ubiquitous Language] requires: Cortex is a concept, Nuclear Notes is
    /// the product.
    ///
    /// [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language
    NuclearNotes,
}

/// D1's Network column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// `bare` and `contained`. Nothing leaves the machine except
    /// model-provider traffic.
    ModelProviderOnly,
    /// `linked`. Model provider and platform.
    ModelProviderAndPlatform,
}

macro_rules! column {
    ($name:ident, $($variant:ident => $cell:literal),+ $(,)?) => {
        impl $name {
            /// Every cell D1's column holds.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// This cell, worded as ADR-0001 D1 words it.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $cell),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

column!(Membrane, None => "none", LocalContainers => "local containers");
column!(Loop, None => "none", Local => "local", LocalOffloadable => "local, offloadable");
column!(Cortex, None => "none", Local => "local", NuclearNotes => "Nuclear Notes");
column!(
    Network,
    ModelProviderOnly => "model provider only",
    ModelProviderAndPlatform => "model provider + platform",
);
