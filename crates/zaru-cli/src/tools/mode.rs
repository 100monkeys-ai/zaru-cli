// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The runtime tier, the permission mode, and the layer a mode may not come
//! from.
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

/// How much of the platform is engaged, per ADR-0001 D1.
///
/// Transcribed from that record, which names the three and says they are
/// "effectively permanent once published". ADR-0011 D2's own table uses the
/// same three, and this type exists because D2's enforcement differs across
/// them.
///
/// **Fixed for the life of a session.** ADR-0001 D2: "Tier is resolved at
/// session start and is immutable for the life of a session... A membrane
/// that can be dropped mid-session is not a membrane." ADR-0014 D7 restates
/// it. Nothing here can change a tier, because there is nothing to change: a
/// tier is a value a caller holds, not a field on anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// No AEGIS, no membrane. ADR-0011 D2: harness-level prompts and an
    /// allowlist, advisory, and the harness says so.
    Bare,
    /// AEGIS locally. D2: SEAL security contexts, enforced by the
    /// orchestrator.
    Contained,
    /// The account is attached. D2: as contained; offloaded work carries its
    /// own context.
    Linked,
}

impl Tier {
    /// Every tier ADR-0001 D1 names.
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

    /// Whether a membrane exists at this tier.
    ///
    /// ADR-0011 D2's table gives `bare` no enforcement at all, which is why
    /// the not-a-sandbox line of D2 exists and why it is emitted at this tier
    /// and no other. See [`SessionNotice`](super::notice::SessionNotice).
    #[must_use]
    pub const fn has_membrane(self) -> bool {
        !matches!(self, Self::Bare)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which of ADR-0014 D1's five configuration layers a value came from.
///
/// Transcribed from D1's list, lowest to highest. Higher wins, and there is
/// no layer above flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// 1 — compiled in.
    BuiltIn,
    /// 2 — `~/.zaru/config.toml`.
    User,
    /// 3 — `./zaru.toml`. The layer a cloned repository writes.
    Project,
    /// 4 — `ZARU_*`.
    Environment,
    /// 5 — command-line flags.
    Flag,
}

impl Layer {
    /// Every layer ADR-0014 D1 names, lowest to highest.
    pub const ALL: [Self; 5] = [
        Self::BuiltIn,
        Self::User,
        Self::Project,
        Self::Environment,
        Self::Flag,
    ];

    /// How ADR-0014 D1's table names this layer.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in defaults",
            Self::User => "user config",
            Self::Project => "project config",
            Self::Environment => "environment",
            Self::Flag => "command-line flags",
        }
    }

    /// Whether a repository the user cloned writes this layer.
    ///
    /// One layer, today. It is a method rather than an equality test at every
    /// call site so that a second such layer — a served extension, an
    /// admitted skill — is added in one place.
    #[must_use]
    pub const fn is_written_by_a_cloned_repository(self) -> bool {
        matches!(self, Self::Project)
    }
}

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
                layer.as_str()
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
    /// [`ModeRefused::FromAClonedRepository`] when `layer` is one a cloned
    /// repository writes — ADR-0014 D6 — checked **before** the value is
    /// parsed, so that a project layer offering a misspelled mode is refused
    /// for the reason that matters rather than for the typo.
    ///
    /// [`ModeRefused::NoSuchMode`] when `value` names none of the three.
    pub fn from_layer(layer: Layer, key: &str, value: &str) -> Result<Self, ModeRefused> {
        if layer.is_written_by_a_cloned_repository() {
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
