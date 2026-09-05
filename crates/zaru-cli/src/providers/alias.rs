// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D2's fixed alias set.
//!
//! # Four, and a fifth is an ADR-level change
//!
//! D2: "**Adding an alias is an ADR-level change**, because the set is a
//! contract with AEGIS and adding one on the harness side alone reintroduces
//! exactly the drift this record exists to prevent."
//!
//! So [`ModelAlias`] is closed, [`ModelAlias::ALL`] is annotated with its
//! length, and every match on it below is exhaustive with no wildcard arm — a
//! fifth variant fails to compile here rather than travelling. That is the
//! same shape [`Class`](crate::failure::Class) and
//! [`Layer`](crate::config::Layer) already use for their own closed sets.
//!
//! # The contract this is one side of is already not shared, and that is on the record
//!
//! Measured on 2026-09-05: [ADR-009]'s "Standard Model Aliases" names
//! **five** — `default`, `fast`, `smart`, `cheap`, `local` — with `smart`
//! where D2 has `reasoning`, over a map that accepts whatever a node
//! configuration declares. D2's four are what is built here and **nothing
//! reconciles the two**; the difference is recorded as a proposed Update on
//! ADR-0012 for the author, because the set is a contract and a contract is
//! not a thing an implementation settles.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-009]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/009-byollm-provider-system

use crate::config::Key;
use core::fmt;

/// One of [ADR-0012] D2's four aliases.
///
/// **Closed.** A project asks for one of these; what it resolves to is
/// configuration, resolved in exactly one place — the resolution table, which
/// is the only module that can construct a model identifier.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ModelAlias {
    /// D2 row 1 — "General work. The one nearly everything uses."
    Default,
    /// D2 row 2 — "Cheap and quick — classification, routing, short edits".
    Fast,
    /// D2 row 3 — "Hard problems, worth the latency and cost".
    ///
    /// Spelled `reasoning` here and `smart` on the platform side. See the
    /// module documentation.
    Reasoning,
    /// D2 row 4 — "Whatever the user runs on their own hardware".
    Local,
}

impl ModelAlias {
    /// Every alias D2 names, in the record's own order.
    ///
    /// The length is annotated, so a fifth variant fails to compile here as
    /// well as in every exhaustive match below.
    pub const ALL: [Self; 4] = [Self::Default, Self::Fast, Self::Reasoning, Self::Local];

    /// The first segment of every configuration key this record owns.
    ///
    /// A constant rather than a literal repeated in [`ModelAlias::key`] and in
    /// whatever reads a key back, for the reason
    /// [`PROJECT_TABLE`](crate::manifest::PROJECT_TABLE) is one: two spellings
    /// of one path agree on the day they are written.
    pub const TABLE: &'static str = "model";

    /// The alias as D2's table spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Fast => "fast",
            Self::Reasoning => "reasoning",
            Self::Local => "local",
        }
    }

    /// What D2's second column says this alias is for.
    ///
    /// Carried so that a refusal or a listing can say what the user asked for
    /// rather than only which word they wrote.
    #[must_use]
    pub const fn intent(self) -> &'static str {
        match self {
            Self::Default => "general work, and the one nearly everything uses",
            Self::Fast => "cheap and quick work — classification, routing, short edits",
            Self::Reasoning => "hard problems, worth the latency and cost",
            Self::Local => "whatever the user runs on their own hardware",
        }
    }

    /// The [ADR-0014] configuration key that resolves this alias.
    ///
    /// `model.default`, `model.fast`, `model.reasoning`, `model.local`. **That
    /// spelling is load-bearing rather than arbitrary**: ADR-0012 D4's own
    /// worked environment variable is `ZARU_MODEL_DEFAULT`, and ADR-0014's
    /// transform produces exactly that from `model.default` and from no other
    /// spelling tried — `provider.model.default` gives
    /// `ZARU_PROVIDER_MODEL_DEFAULT` and `models.default` gives
    /// `ZARU_MODELS_DEFAULT`. Measured 2026-09-05 by running
    /// [`variable_name`](crate::config::environment::variable_name) rather
    /// than by reading it.
    ///
    /// # Panics
    ///
    /// Never. The four spellings are this module's own and none of them is
    /// empty, carries a control character, has an empty segment or has a
    /// segment surrounded by whitespace — the four shapes
    /// [`Key::new`] refuses.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn key(self) -> Key {
        Key::new(&format!("{}.{}", Self::TABLE, self.as_str()))
            .expect("ADR-0012 D2's alias spellings are well-formed configuration keys")
    }
}

impl fmt::Display for ModelAlias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
