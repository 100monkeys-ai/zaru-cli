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
//! mode, disable SEAL, widen a token scope, move the runtime tier upward,
//! name a provider endpoint, or supply the tool-surface allowlist. **A
//! repository the user cloned must not be able to configure its way to more
//! privilege than the user granted.**"
//!
//! *That quotation carried D6's original four until 2026-09-05 and is now the
//! clause's six; the fifth and sixth were added that day and the permission
//! mode has been the **first** since the record was written. Declaring a key
//! for it adds no escalation — it gives D6's first one the name ADR-0014's
//! own Status tracking says it has been waiting for.*
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
//! # The key, and why it is spelled `tools.mode`
//!
//! This module said "no configuration key is invented here" until 2026-09-05,
//! and it was right to: [ADR-0014]'s Neutral consequence leaves each record
//! its own keys, ADR-0011 named none, and a key chosen by the thing that reads
//! it is a name nobody decided. **ADR-0011 D3 now names one**, by an accepted
//! Update of 2026-09-05 under Jeshua's directive of that day and open to his
//! veto, so this module spells it — once, in [`KEY`] — rather than inventing
//! it. [`Mode::from_layer`] still takes the key as a parameter, because a
//! caller holding a resolution built some other way names its own.
//!
//! `tools.mode`, and the spelling was settled a day before the key existed:
//! [`crate::tools::allowlist`] chose `tools.allowlist` over `tools.allow`
//! precisely so that "a future `tools.mode = \"allow\"` sitting beside a
//! `tools.allow` list" would not be two things a reader has one word for.
//! This is that future.
//!
//! # What a user can now say, and where
//!
//! Layers 1, 2, 4 and 5 of [ADR-0014] D1. **Layer 3 is refused** — see above,
//! and [`field`] for the arm the fold runs. **Layer 1 declares no default**:
//! `Mode::default()` is `Ask` because D3's table says "Default", and a
//! compiled-in layer-1 value would be a second statement of D3 that could
//! disagree with the first. `runtime.tier` is the one key with a layer-1
//! default, and it has one because [ADR-0001] D2 gained it by an Update for a
//! reason that does not apply here: an unset tier was *refused*, where an
//! unset mode has always had D3's own answer.
//!
//! Layer 4 arrives free. [ADR-0014] D1's transform is mechanical — `ZARU_`
//! plus the dotted key upper-cased with dots turned into underscores — so
//! declaring the key is what makes `ZARU_TOOLS_MODE` work, and no alias table
//! is written for it.
//!
//! # A project's refusal is shadowed, and that is measured rather than assumed
//!
//! `tools.mode` sits under `[tools]`, which [ADR-0009] D1's manifest
//! vocabulary does not declare — its top level is closed to `[project]`,
//! `[runtime]` and `[[validator]]`. So a real `./zaru.toml` setting this key
//! is refused by the **manifest reader**, one step before [`field`]'s
//! declaration is reached, with a message about a table name rather than
//! about privilege. Nothing is weakened — the project still cannot set it,
//! twice over — but the reason the user is told is ADR-0009's rather than
//! ADR-0014 D6's. That was already true of `tools.allowlist` and
//! `provider.<kind>.endpoint`; this key makes it **three**, and
//! `the_escalation_ceiling_is_shadowed_by_adr_0009s_manifest_vocabulary`
//! holds all of them so that widening ADR-0009's vocabulary reddens.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::config::{Field, FieldKind, Key, Resolution, Schema};
use core::fmt;

/// The configuration key [ADR-0011] D3's permission mode is read from.
///
/// Spelled here and nowhere else. See the module documentation for why it is
/// `tools.mode`, and for why it did not exist until 2026-09-05.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const KEY: &str = "tools.mode";

/// Why a project may not set [`KEY`], in the words the refusal carries.
///
/// One string, read by [`field`] and by [`ModeRefused`], so the fold's
/// refusal and this module's cannot give a user two different reasons for one
/// rule — the shape [`crate::tools::allowlist`] already uses. [ADR-0016] D2
/// wants an error whose reader can act, so it names where the mode *does*
/// belong.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const PROJECT_REFUSAL: &str = "how much the harness prompts is the user's own choice, and a \
                                   repository they cloned must not be able to grant itself fewer \
                                   prompts; set it in ~/.zaru/config.toml, ZARU_TOOLS_MODE or \
                                   --mode instead";

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("tools.mode is a well-formed key")
}

/// What [`KEY`] holds, and what the project layer may do to it.
///
/// Text, because a mode is one of three words — and
/// [`ProjectPolicy::Refused`](crate::config::ProjectPolicy::Refused) rather
/// than `LowerOnly`, which is the whole of the module documentation's first
/// section in one call: `LowerOnly` is defined on whole numbers and would need
/// an ordering over `ask`, `allow` and `yolo` that no record states.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Text, PROJECT_REFUSAL)
}

/// Declare [ADR-0011] D3's configuration key into a caller's schema.
///
/// The shape [`crate::tools::allowlist::declare`] already uses, so a caller
/// building a schema asks each record for its own keys rather than
/// transcribing them.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

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

    /// The mode a folded configuration resolves to.
    ///
    /// The shape [`Allowed::from_configuration`](crate::tools::Allowed) uses,
    /// and for the same reason: the value and the layer that supplied it come
    /// from **one** [`Resolution::explain`] call, so a caller cannot read the
    /// value from one place and the layer from another and have them disagree.
    ///
    /// An unset key is [`Mode::Ask`] — [ADR-0011] D3's own default, taken from
    /// [`Mode::default`] rather than spelled again here, because layer 1
    /// declares none and D3's table is where "Default" is written.
    ///
    /// **What this adds to [ADR-0014] D6 is the layer**, not the test.
    /// [`Mode::from_layer`] holds the ceiling and has since the mode landed;
    /// what a resolution contributes is *which layer to ask it about*, and
    /// getting that wrong is how a project's value gets judged as though the
    /// user had written it. So the effective layer is read from the same
    /// `explain` call the value comes from and handed straight down. The
    /// fold's own arm — [`field`]'s declaration, which refuses a project
    /// before any value exists — is independent of both, and each reddens on
    /// its own.
    ///
    /// # Errors
    ///
    /// [`ModeRefused::FromAClonedRepository`] when the effective layer is one
    /// [`Layer::bound_by_the_escalation_ceiling`] binds;
    /// [`ModeRefused::NoSuchMode`] when the value names none of the three.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub fn from_configuration(resolution: &Resolution) -> Result<Self, ModeRefused> {
        let key = key();
        let explanation = resolution.explain(&key);

        let (Some(value), Some(layer)) =
            (explanation.value.as_ref(), explanation.effective_layer())
        else {
            return Ok(Self::default());
        };

        // The shape a layer carries is the schema's business, and the schema
        // declares this key as text -- so anything else here is a coercion
        // that already happened or a caller who built the resolution by hand.
        // Either way the value is quoted back rather than described, which is
        // `NoSuchMode`'s own contract.
        let offered = value
            .as_text()
            .map_or_else(|| value.shape().to_owned(), std::borrow::ToOwned::to_owned);
        Self::from_layer(layer, KEY, &offered)
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
