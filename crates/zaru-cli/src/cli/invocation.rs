// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the user asked for, typed.
//!
//! # Why the name is `CommandLine`
//!
//! The obvious names are taken, both by records this one has to sit beside.
//! `Invocation` is [ADR-0011] D1's *tool call* — [`crate::tools::Invocation`]
//! — and `Command` is [ADR-0015] D1's *extension kind*, "a named prompt
//! template with arguments", which is a thing this harness will one day load
//! from a file and which is emphatically not this. A second type of either
//! name in one crate is the collision [Ubiquitous Language] exists to prevent,
//! and that page's own rule is that the newcomer is the one that qualifies.
//! Named 2026-09-05 under a delegated coordinator ruling, open to Jeshua's
//! veto, with the vocabulary rows written in the same change.
//!
//! # Nothing here is a string the rest of the program reparses
//!
//! A [`Request`] carries values that are already what they will be used as: a
//! [`Key`] the configuration hierarchy can be asked about, a [`SessionId`] the
//! session store can be asked for. Whatever the parser could validate, it did.
//!
//! **[`Overrides`] is the exception and it is deliberate.** `--runtime` and
//! `--model` carry raw text, because their destination is [ADR-0014] D1's
//! layer 5 and a layer's values arrive as [`Value::Text`](crate::config::Value)
//! and are coerced by the schema during the fold — which is exactly what
//! [`crate::config::environment`] does for layer 4. Validating a tier here
//! would produce a *second* refusal for a bad `--runtime`, beside
//! [`TierRefused::NoSuchTier`](crate::runtime::TierRefused), and the second
//! one would not be able to name the layer the value came from.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

use crate::config::Key;
use crate::session::SessionId;

/// What [ADR-0014] D1's layer 5 was told, before it becomes a layer.
///
/// **Two fields and no third.** D1 spells layer 5 "`--tier`, `--model`, and
/// the rest", and the rest does not exist: every other flag this surface takes
/// is a request rather than a setting. A flag that set a third key would add a
/// field here and would have to name the key it sets.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `--runtime <tier>`, destined for `runtime.tier`. [ADR-0001] D2.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    pub tier: Option<String>,
    /// `--model <identifier>`, destined for `model.default`.
    ///
    /// [ADR-0012] D4's own layered example ends `→ flag  --model`, and its D1
    /// says configuration names aliases rather than models — so the flag
    /// supplies the *identifier* for one alias, and the alias it supplies is
    /// `default`, which D2 calls "the one nearly everything uses". Recorded
    /// 2026-09-05 as a delegated coordinator ruling; a flag naming another
    /// alias would need a spelling no record gives.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub model: Option<String>,
}

impl Overrides {
    /// Whether any flag set anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tier.is_none() && self.model.is_none()
    }
}

/// What the user asked the binary to do.
///
/// A closed set with no wildcard match anywhere, so a tenth request cannot
/// arrive without `main`, `--help` and the renderer each answering for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `zaru --help`, and `zaru` with no arguments at all.
    Help,
    /// `zaru --version`.
    Version,
    /// `zaru runtime` — [ADR-0001] D2's datum.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    Runtime,
    /// `zaru config explain <key>` — [ADR-0014] D3's block, for one key.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    ConfigExplain {
        /// The key, already validated by [`Key::new`].
        key: Key,
    },
    /// `zaru models` — [ADR-0012] D4's listing.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Models,
    /// `zaru sessions list`.
    SessionsList,
    /// `zaru sessions rm <id>` — [ADR-0010] D6.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    SessionsRemove {
        /// The id, already validated by [`SessionId::parse`].
        id: SessionId,
    },
    /// `zaru notes tokens` — [ADR-0007] D7's listing.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesTokens,
    /// `zaru --resume <id>` — [ADR-0010] D4.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    Resume {
        /// The id, already validated.
        id: SessionId,
    },
    /// `zaru --continue` — D4's most recent session in this directory.
    Continue,
    /// Anything else: what the user wants done, which needs a provider.
    Task {
        /// The **task words**, as the user typed them, in order.
        ///
        /// Held rather than discarded so the refusal can quote what it could
        /// not run, and so that the day a provider exists this is the value
        /// the loop is handed. Never re-parsed as a command: whether a line is
        /// a command or a task was decided by the parser and is not decided
        /// twice.
        words: Vec<String>,
    },
}

/// One whole command line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    /// What layer 5 was told.
    pub overrides: Overrides,
    /// What to do.
    pub request: Request,
}
