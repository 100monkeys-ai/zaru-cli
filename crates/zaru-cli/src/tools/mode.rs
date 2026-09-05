// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The permission mode, and the layer a mode may not come from.
//!
//! [ADR-0001]'s runtime tier was declared here too until 2026-09-05 and is now
//! [`crate::runtime`]'s, re-exported below — see [`Tier`] for the ruling.
//!
//! # A project may not set the permission mode, and that is the whole point
//!
//! [ADR-0014] D6: "A project may lower its own iteration ceiling, name its
//! workspace, and declare validators. It may **not** raise the permission
//! mode, disable SEAL, widen a token scope, or move the runtime tier upward.
//! **A repository the user cloned must not be able to configure its way to
//! more privilege than the user granted.**"
//!
//! D6 forbids *raising*, and this module refuses the project layer **any**
//! mode at all. That is wider than D6 asks, and it is wider for a stated
//! reason: raising and lowering are only distinguishable if there is an
//! ordering over `ask`, `allow` and `yolo`, and **no record states one**.
//! `yolo` is less prompting and more privilege, which is one ordering; a list
//! written safest-first is another; and choosing between them is authoring a
//! security ordering, which [Autonomous Development] puts on the human side
//! of the boundary. The wider gate settles nothing and is recorded as a
//! proposed Update on ADR-0011 rather than left for a reader to infer.
//!
//! # No configuration key is invented here
//!
//! [ADR-0014]'s Neutral consequence says "Nothing here specifies the schema.
//! Each record owns its own keys", and ADR-0011 names no key for the
//! permission mode. So [`Mode::from_layer`] takes the key as a parameter and
//! quotes back whatever it was handed. A key invented by the thing that reads
//! it is a name nobody chose, and this module chooses none.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use core::fmt;

/// [ADR-0001] D1's three runtime tiers.
///
/// **Declared once, in [`crate::runtime`], and re-exported here.** This module
/// declared it until 2026-09-05, because the tool surface needed it first —
/// [ADR-0011] D2's enforcement differs per tier — which left one file holding
/// ADR-0001's tier beside ADR-0011's permission mode and [ADR-0014] D1's
/// layers. The declaration that stays is `runtime`'s, because ADR-0001 owns
/// the tier and its D1 table, its D2 resolution and its D3 ceilings all live
/// with it. This is the **delegated coordinator ruling of 2026-09-05**,
/// recorded on ADR-0001's Status tracking and open to Jeshua's veto, and it is
/// the same move the [`Layer`] re-export below records for 2026-09-04.
///
/// Nothing that imported `crate::tools::Tier` changed: this re-export is the
/// same path it always was.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub use crate::runtime::Tier;

/// Which of ADR-0014 D1's five configuration layers a value came from.
///
/// **Declared once, in [`crate::config::layer`], and re-exported here.** This
/// module transcribed D1's five layers a second time until 2026-09-04, and a
/// rule that exists in two places diverges — these two already had, on the
/// spellings of layers 1 and 5. The declaration that stays is
/// configuration's, because [Bounded Contexts] gives `zaru-cli`
/// configuration and D1's layers are configuration's own vocabulary. This is
/// a **delegated coordinator ruling of 2026-09-04**, recorded on ADR-0011's
/// Status tracking and open to Jeshua's veto.
///
/// The predicate this module used to spell `is_written_by_a_cloned_repository`
/// is [`Layer::bound_by_the_escalation_ceiling`], which is ADR-0014 D6's own
/// framing of the same rule over the same single layer. One predicate for one
/// clause has one name.
///
/// [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
pub use crate::config::layer::Layer;

/// Why a permission mode was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeRefused {
    /// The layer was one a cloned repository writes.
    ///
    /// ADR-0014 D6 requires the error name "the key and the reason", so both
    /// are carried.
    FromAClonedRepository {
        /// The configuration key, as the caller spelled it.
        key: String,
        /// The mode that layer tried to set.
        offered: String,
        /// Which layer it came from.
        layer: Layer,
    },
    /// The value named no mode ADR-0011 D3 defines.
    NoSuchMode {
        /// The configuration key, as the caller spelled it.
        key: String,
        /// The value offered, escaped.
        offered: String,
    },
}

impl fmt::Display for ModeRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromAClonedRepository {
                key,
                offered,
                layer,
            } => write!(
                f,
                "the key {key:?} set the permission mode to {offered:?} from {}, and the \
                 permission mode is not that layer's to set. ADR-0014 D6: \"A repository the user \
                 cloned must not be able to configure its way to more privilege than the user \
                 granted.\" Set it in the user configuration, the environment or a flag instead. \
                 This refusal is wider than D6's, which forbids only *raising* the mode: no \
                 record states an ordering over \"ask\", \"allow\" and \"yolo\", so there is no \
                 way to tell a raise from a lower, and that question is open on the record",
                layer.label()
            ),
            Self::NoSuchMode { key, offered } => write!(
                f,
                "the key {key:?} was set to {offered:?}, which names no permission mode. \
                 ADR-0011 D3 defines exactly three: \"ask\", \"allow\" and \"yolo\""
            ),
        }
    }
}

impl std::error::Error for ModeRefused {}

/// How much the harness prompts, per ADR-0011 D3.
///
/// Three modes, "chosen by the user, defaulting to the safe one". The third
/// is named honestly on purpose: D3 says "A user who chooses it has chosen
/// it, and the name is what makes that choice conscious rather than a setting
/// they clicked past."
///
/// At `contained` tier and above the mode governs **prompting only** — D3
/// again: "SEAL enforcement is not affected by it. A user in `yolo` inside a
/// membrane is still inside the membrane, and that is precisely the argument
/// for the membrane." Nothing in this crate lets a mode reach an enforcement
/// path, because no enforcement path exists here to reach: SEAL is
/// `zaru-seal`'s and is unbuilt. That is absence rather than refusal, and it
/// is what ADR-0011's trigger clause 5 can be shown today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Prompts before any write or command. **The default**, per D3.
    #[default]
    Ask,
    /// Runs the allowlist without prompting; prompts for anything outside it.
    Allow,
    /// No prompts.
    Yolo,
}

impl Mode {
    /// Every mode ADR-0011 D3 names.
    pub const ALL: [Self; 3] = [Self::Ask, Self::Allow, Self::Yolo];

    /// The mode's name as ADR-0011 D3 spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
            Self::Yolo => "yolo",
        }
    }

    /// Take a mode from a named key in a named configuration layer.
    ///
    /// `key` is the caller's spelling and is quoted back in any refusal;
    /// ADR-0011 names no configuration key for the mode and this function
    /// invents none.
    ///
    /// # Errors
    ///
    /// [`ModeRefused::FromAClonedRepository`] when `layer` is one ADR-0014
    /// D6's escalation ceiling binds — the project layer, which is the one a
    /// cloned repository writes — checked **before** the value is
    /// parsed, so that a project layer offering a misspelled mode is refused
    /// for the reason that matters rather than for the typo.
    ///
    /// [`ModeRefused::NoSuchMode`] when `value` names none of the three.
    pub fn from_layer(layer: Layer, key: &str, value: &str) -> Result<Self, ModeRefused> {
        if layer.bound_by_the_escalation_ceiling() {
            return Err(ModeRefused::FromAClonedRepository {
                key: key.to_owned(),
                offered: value.escape_debug().to_string(),
                layer,
            });
        }
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == value)
            .ok_or_else(|| ModeRefused::NoSuchMode {
                key: key.to_owned(),
                offered: value.escape_debug().to_string(),
            })
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
