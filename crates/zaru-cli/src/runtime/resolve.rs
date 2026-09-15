// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one place `runtime.tier` is spelled, and the tier a session resolved.
//!
//! # One key, one declaration, one reason
//!
//! [ADR-0014]'s Neutral section leaves each record its own keys, and this is
//! ADR-0001's. Until 2026-09-05 it was declared **three times** — in
//! configuration's fixtures and in two outside-caller checks — carrying two
//! different reasons for why a project may not set it. A rule in three places
//! is a rule that diverges, and this one already had ([Verification lessons]
//! §27 and §30). [`key`] and [`field`] are what those three now ask.
//!
//! # The spelling is `runtime.tier`, decided rather than inferred
//!
//! [ADR-0001] D2 says "Config key `runtime` in `zaru.toml`", while
//! [ADR-0009] D1's own example manifest writes `[runtime] tier = "contained"`
//! and every declaration in this tree uses `runtime.tier`. Those are not the
//! same key: measured through [ADR-0014] D1's layer-4 transform, `runtime`
//! produces `ZARU_RUNTIME` and `runtime.tier` produces `ZARU_RUNTIME_TIER`.
//!
//! **`runtime.tier` is the spelling, under Jeshua's directive of 2026-09-05**,
//! and ADR-0001 D2 is corrected to match rather than the code being bent to
//! D2's shorter form: a bare `runtime` cannot hold the tier *and* the ceiling
//! `runtime.max_iterations` that D3 also puts under it.
//!
//! # D6, and what this module does not settle
//!
//! [ADR-0014] D6 says a project "may not ... move the runtime tier upward".
//! [`field`] declares the key
//! [`Refused`](crate::config::ProjectPolicy::Refused) to the project layer
//! outright, which is what the fold has enforced since that record landed and
//! is wider than D6 asks. That width is deliberate and is not this module's
//! to narrow: D6 presumes an ordering over tiers that **no record states**,
//! and `bare` has no membrane at all, so "upward" is a claim about reach
//! rather than about safety. The question is open on ADR-0014's own Update.
//!
//! # D2's immutability, and the bound this module will not exceed
//!
//! [`ResolvedTier`] has no setter, no method taking `&mut self`, no `reload`
//! and no interior mutability, and [`Meta`](crate::session::Meta) holds one
//! behind a getter rather than in a public field. So a session's tier has no
//! mutation surface at all, which is D2's "a membrane that can be dropped
//! mid-session is not a membrane" held by the type rather than by a rule
//! somebody remembers.
//!
//! **What that does not prove**, stated rather than glossed: a caller may
//! still call [`ResolvedTier::from_configuration`] twice and hold two values.
//! What is unrepresentable is a *session's* tier changing, which is the
//! property D2 and [ADR-0010] D2 actually state. This is the same bound
//! ADR-0014's own `Resolution` carries, and that record's Status tracking
//! calls it the invariant half.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::config::{Field, FieldKind, Key, Layer, Resolution};
use crate::failure::{THERE_ARE_EXACTLY, THIS_HARNESS};
use crate::providers::{Inference, Placement};
use crate::runtime::tier::Tier;
use core::fmt;
use zaru_core::iteration::Ceiling;

/// The configuration key ADR-0001 D2 puts the tier at.
///
/// Spelled here and nowhere else. See the module documentation for why it is
/// `runtime.tier` rather than D2's shorter `runtime`.
pub const KEY: &str = "runtime.tier";

/// Why a project may not set the tier, in the words the refusal carries.
///
/// One string rather than three. The two it replaces said "the runtime tier is
/// the membrane the user chose, and ADR-0001 D2 fixes it at session start" and
/// "the runtime tier is the user's to choose"; both halves are true and both
/// are kept, because [ADR-0016] D2 wants an error whose reader can act.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const PROJECT_REFUSAL: &str = "the runtime tier is the membrane the user chose, and it is \
                                   fixed at session start; set it in the user \
                                   configuration, the environment or a flag instead";

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("runtime.tier is a well-formed key")
}

/// What [`KEY`] holds, and what the project layer may do to it.
///
/// Handed to a caller's [`Schema`](crate::config::Schema). This module builds
/// no schema of its own: ADR-0014's Neutral section makes the schema an input
/// to the hierarchy, and a record that declared its own would be the sixth
/// record's implementation fixing a seventh record's spelling.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Text, PROJECT_REFUSAL)
}

/// The configuration key [ADR-0001] D3's iteration defaults are overridden at.
///
/// Spelled here and nowhere else, for the reason [`KEY`] is: [ADR-0009] D1's
/// worked manifest sets `[runtime] max_iterations`, [ADR-0014] D3's worked
/// example explains `runtime.max_iterations`, and the record that owns the
/// numbers is this one — D3's table is the source of truth for what a tier's
/// default is, so the key that overrides it is ADR-0001's to spell.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const MAX_ITERATIONS_KEY: &str = "runtime.max_iterations";

/// [`MAX_ITERATIONS_KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`MAX_ITERATIONS_KEY`] is a literal this module owns and is well
/// formed.
#[must_use]
pub fn max_iterations_key() -> Key {
    Key::new(MAX_ITERATIONS_KEY).expect("runtime.max_iterations is a well-formed key")
}

/// What [`MAX_ITERATIONS_KEY`] holds, and what the project layer may do to it.
///
/// [`Field::ceiling`], which is [ADR-0014] D6's `LowerOnly` policy: "A project
/// may **lower** its own iteration ceiling". **This is that arm's first real
/// key** — until 2026-09-05 the only key carrying it was a fixture's, and a
/// policy exercised only by a fixture is a policy nothing in the product has
/// ever been measured against.
///
/// # What reads it, and what does not
///
/// **It has a consumer since 2026-09-05, and this sentence said it had none.**
/// [`ceiling_for`] is it: the value resolved here is the ceiling `zaru-core`
/// is handed where any layer set the key, and where none did, the cell
/// [`iterations`](crate::runtime::iterations) gives for the resolved tier,
/// inference axis and placement. The sentence this replaces read "**Nothing
/// consumes this value yet** … the arc that wires a provider client into the
/// loop is the one that connects this key to that number" — that arc ran and
/// could not, because the consumer is the *iteration* ceiling and there was no
/// iteration loop; the arc that built one is the one that connected it.
///
/// That is not [ADR-0014] D5's silent typo. D5's failure is "the user sees no
/// change and concludes the setting does not work"; here `zaru config explain
/// runtime.max_iterations` prints the value, the layer that supplied it, and
/// every layer that did not, so where the setting stops is exactly what the
/// harness shows.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn max_iterations_field() -> Field {
    Field::ceiling()
}

/// The tier [ADR-0014] D1's layer 1 supplies when nothing else does.
///
/// **`bare`, decided 2026-09-05 under directive 20**, as a delegated
/// coordinator ruling open to Jeshua's veto, and written as accepted Updates
/// on [ADR-0001] D2 and ADR-0014 D1. Until then this module refused an unset
/// tier outright, on the ground that ADR-0001 names no fallback and choosing
/// one decides which membrane a user gets when they said nothing.
///
/// What changed is that the binary can now be run, and a harness that refuses
/// to do anything on a machine with no configuration contradicts [ADR-0001]
/// D1's own sentence about this tier: "a plain agentic harness. No AEGIS, no
/// account, no cortex, full tool capability. **This is the on-ramp and it is
/// not a trial: it is complete, it is supported, and a user may stay here
/// permanently.**" [ADR-0003] D7 is the third voice — "the default install is
/// **zaru alone**... with `bare` fully functional".
///
/// It is a *layer* rather than a fallback inside this module, which is the
/// load-bearing half: it arrives as [ADR-0014] D1's layer 1 like any other
/// compiled-in value, so `zaru config explain runtime.tier` names `built-in`
/// as the supplier and every layer above it wins in the ordinary way. A
/// default hidden inside [`ResolvedTier::from_configuration`] would be a value
/// the trace could not show.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const BUILT_IN_TIER: Tier = Tier::Bare;

/// Why a tier could not be taken from a resolved configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TierRefused {
    /// No layer set the key.
    ///
    /// **Not a default, and this refusal stays.** ADR-0001 names no fallback
    /// tier, and choosing one *here* would decide which membrane a user gets
    /// when they said nothing, invisibly — which is a security posture, and on
    /// the human side of the boundary. Layer 1 is where a built-in belongs and
    /// it is a caller's to supply: since 2026-09-05 the `zaru` binary supplies
    /// [`BUILT_IN_TIER`] there, so this variant is what a caller that declares
    /// no layer-1 default gets, which is every caller that is not that binary.
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
    /// The value named no tier ADR-0001 D1 defines.
    NoSuchTier {
        /// The key.
        key: Key,
        /// The value offered.
        offered: String,
        /// Which layer offered it, so the reader knows which file to edit.
        layer: Layer,
    },
}

impl fmt::Display for TierRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSet { key } => write!(
                f,
                "no configuration layer set {key}, so there is no runtime tier for this session. \
                 {THIS_HARNESS} names no default tier and nothing here invents one: set it in \
                 the \
                 built-in layer, the user configuration, the environment or a flag"
            ),
            Self::WrongShape { key, found } => write!(
                f,
                "{key} holds {found}, and a runtime tier is text naming one of {}",
                spellings()
            ),
            Self::NoSuchTier {
                key,
                offered,
                layer,
            } => write!(
                f,
                "the key {key} was set to {offered:?} in {}, which names no runtime tier. \
                 {THERE_ARE_EXACTLY} three: {}",
                layer.label(),
                spellings()
            ),
        }
    }
}

impl std::error::Error for TierRefused {}

/// Every tier's name, listed as a refusal lists them.
///
/// Walked from [`Tier::ALL`] rather than typed out, so a fourth tier appears
/// in every refusal the moment it is declared ([Verification lessons] §17).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn spellings() -> String {
    let quoted: Vec<String> = Tier::ALL
        .iter()
        .map(|tier| format!("{:?}", tier.as_str()))
        .collect();
    quoted.join(", ")
}

/// The tier a session resolved, and the layer that supplied it.
///
/// **Read-only by construction.** See the module documentation for what that
/// does and does not prove.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedTier {
    tier: Tier,
    supplied_by: Layer,
}

impl ResolvedTier {
    /// Resolve the tier once, at session start, from a folded configuration.
    ///
    /// The value and the layer come from **one** call to
    /// [`Resolution::explain`], which is [ADR-0014] D3's own trace. There is
    /// no second traversal of the layers anywhere in this module, so the layer
    /// this reports and the layer `config explain` would print cannot disagree.
    ///
    /// # Errors
    ///
    /// [`TierRefused`], naming the key and — where a layer offered something —
    /// the layer, so the reader knows which file to edit.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub fn from_configuration(resolution: &Resolution) -> Result<Self, TierRefused> {
        let key = key();
        let explanation = resolution.explain(&key);

        let (Some(value), Some(supplied_by)) =
            (explanation.value.as_ref(), explanation.effective_layer())
        else {
            return Err(TierRefused::NotSet { key });
        };

        let Some(text) = value.as_text() else {
            return Err(TierRefused::WrongShape {
                key,
                found: value.shape(),
            });
        };

        let Some(tier) = Tier::named(text) else {
            return Err(TierRefused::NoSuchTier {
                key,
                offered: text.to_string(),
                layer: supplied_by,
            });
        };

        Ok(Self { tier, supplied_by })
    }

    /// Take a tier that did not come from configuration.
    ///
    /// The layer is required rather than defaulted, because [ADR-0014] D3
    /// prints where every value came from and a resolution that could not say
    /// would be a row `config explain` cannot render. A caller that compiled
    /// the tier in passes [`Layer::BuiltIn`].
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub const fn supplied(tier: Tier, supplied_by: Layer) -> Self {
        Self { tier, supplied_by }
    }

    /// The tier itself.
    #[must_use]
    pub const fn tier(self) -> Tier {
        self.tier
    }

    /// Which of ADR-0014 D1's five layers set it.
    #[must_use]
    pub const fn supplied_by(self) -> Layer {
        self.supplied_by
    }
}

impl fmt::Display for ResolvedTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (from {})", self.tier, self.supplied_by.label())
    }
}

/// Why an iteration ceiling could not be resolved.
///
/// Both variants name what the reader can change, which is [ADR-0016] D1
/// row 2's requirement and the reason neither carries a bare number.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeilingRefused {
    /// A layer set the key to something that is not a usable count.
    NotACount {
        /// The key.
        key: Key,
        /// What was set. An integer a user typed, never a secret.
        found: i64,
    },
    /// A layer set the key to something that is not an integer at all.
    WrongShape {
        /// The key.
        key: Key,
        /// What shape it held. **Never the value.**
        found: &'static str,
    },
    /// [ADR-0001] D3's table has no cell for this combination.
    ///
    /// Four of its twelve: nothing offloads below `linked`. It is a capability
    /// rather than a mistake — the reader configured something this tier does
    /// not offer.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    NoCell {
        /// The tier that was resolved.
        tier: Tier,
        /// The inference axis, per ADR-0001 D3 as directive 20 declared it.
        inference: Inference,
        /// Where inference runs.
        placement: Placement,
    },
}

impl fmt::Display for CeilingRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACount { key, found } => write!(
                f,
                "`{key}` is {found}, and an iteration ceiling is a count of at least one: a loop \
                 that runs no iterations still has to report something, and the only honest thing \
                 it could report is indistinguishable from a ceiling that was reached"
            ),
            Self::WrongShape { key, found } => write!(
                f,
                "`{key}` holds {found} and an iteration ceiling is a whole number"
            ),
            Self::NoCell {
                tier,
                inference,
                placement,
            } => write!(
                f,
                "there is no iteration ceiling for {tier} with {inference} \
                 inference placed {placement}: nothing offloads below `linked`. Set \
                 `{MAX_ITERATIONS_KEY}` to choose one, or run a tier whose row covers it"
            ),
        }
    }
}

impl std::error::Error for CeilingRefused {}

/// The iteration ceiling this run is bounded by.
///
/// # The configured value where there is one, and D3's cell where there is not
///
/// [ADR-0014] D3's own worked example explains `runtime.max_iterations`
/// through five layers, and [ADR-0014] D6's `LowerOnly` binds a project to
/// what the layers below granted. [ADR-0001] D3's twelve cells are the
/// **default**, applied after the fold rather than as layer 1 — and that is a
/// decision rather than an omission.
///
/// **A layer-1 row is not possible and would not be wanted.** D3's cell is a
/// function of the resolved tier, the inference axis and the placement, all
/// three of which the *same* fold produces, so a built-in row would need the
/// fold's answer before the fold ran. And it would refuse
/// [`crate::manifest::init`]'s own template: that file writes
/// `max_iterations = 3` while `bare`'s cell is `1`, so a project would be
/// raising a ceiling it was told to write. `config/resolve.rs` says "in
/// practice layer 1 always carries a default, per ADR-0001 D3"; it does not,
/// and that sentence is corrected there.
///
/// What a project may do is therefore unchanged and is exactly D6: it may
/// lower what layers 1, 2, 4 and 5 granted, and where none granted anything
/// there is nothing to exceed. **Decided 2026-09-05 under directive 20** as an
/// accepted Update on ADR-0001 D3, open to Jeshua's veto.
///
/// # `bare` is one, and ADR-0009 D4 is why that is not the end of it
///
/// D3 gives `bare` a ceiling of one "by definition — there is no validator to
/// refine against". [ADR-0009] D4 says a project that declares validators runs
/// the iteration loop, at every tier, so at `bare` there *is* one and D3's
/// reason stops holding. The loop still runs: one iteration, and any failure
/// is exhaustion with no refinement, which is honest. A user who wants more
/// raises `{MAX_ITERATIONS_KEY}` at their own layer, which is D3's own next
/// sentence — "a user who wants more raises it explicitly and accepts the
/// wait". Recorded on both records under directive 20.
///
/// # Errors
///
/// [`CeilingRefused`].
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub fn ceiling_for(
    resolution: &Resolution,
    tier: Tier,
    inference: Inference,
    placement: Placement,
) -> Result<Ceiling, CeilingRefused> {
    let key = max_iterations_key();
    match resolution.get(&key) {
        None => crate::runtime::defaults::ceiling(tier, inference, placement).ok_or(
            CeilingRefused::NoCell {
                tier,
                inference,
                placement,
            },
        ),
        Some(value) => {
            let Some(count) = value.as_integer() else {
                return Err(CeilingRefused::WrongShape {
                    key,
                    found: value.shape(),
                });
            };
            u32::try_from(count)
                .ok()
                .and_then(|count| Ceiling::new(count).ok())
                .ok_or(CeilingRefused::NotACount { key, found: count })
        }
    }
}
