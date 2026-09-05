// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D2's namespaces, closed, with both of a namespace's spellings.
//!
//! # One namespace, two entry points
//!
//! D2, as settled on 2026-09-05 under directive 20: "**A namespace has two
//! entry points, and they are one operation.** `/session <verb>` inside a
//! session and `zaru sessions <verb>` outside one are the same commands
//! reached from the two places a user can be, so this table governs both
//! spellings rather than only the slash one."
//!
//! So a [`Namespace`] carries both spellings and neither is derived from the
//! other. **The singular/plural difference is kept as each record wrote it** —
//! `zaru sessions` reads as a collection operated on from outside, `/session`
//! as the one you are in — which D2's own settling sentence says explicitly.
//! Deriving one spelling from the other would have to encode that difference
//! as a rule, and it is not a rule; it is two words.
//!
//! # The set is closed, and that is what D2's shadowing rule needs
//!
//! D2: "A user command may not shadow a built-in namespace... **This binds a
//! subcommand exactly as it binds a slash command**." Nothing in this harness
//! loads a user command yet — [ADR-0015] D3's discovery and D4's admission are
//! both unbuilt — so no collision can occur. What this type supplies is the
//! *vocabulary* that rule will be checked against, as one closed enum with no
//! wildcard match anywhere, so a tenth namespace cannot arrive without every
//! answer being given for it.
//!
//! # `models` is a namespace as of 2026-09-05
//!
//! [ADR-0012] D4 names the spelling `zaru models` in as many words, and D2's
//! table did not carry a row for it — so the shadowing rule did not reach it
//! and a user command called `models` would have been a collision the rule
//! could not see. D2 gains the row as an **accepted Update under directive
//! 20**, a delegated coordinator ruling of 2026-09-05 open to Jeshua's veto.
//! Its slash spelling is `/models`, by the same two-entry-point sentence.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use core::fmt;

/// One of [ADR-0015] D2's namespaces.
///
/// Declared in D2's own table order, with `Models` appended as the row added
/// on 2026-09-05.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Namespace {
    /// D2 row 1 — tier and membrane.
    Runtime,
    /// D2 row 2 — AEGIS component fetch and status.
    Stack,
    /// D2 row 3 — Nuclear Notes tokens, workspace, search.
    Notes,
    /// D2 row 4 — configuration and explanation.
    Config,
    /// D2 row 5 — relationship memory.
    Memory,
    /// D2 row 6 — what this session wrote to craft memory.
    Learned,
    /// D2 row 7 — deposits per [ADR-0002] D3.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    Inbox,
    /// D2 row 8 — resume, list, remove.
    Session,
    /// D2 row 9, added 2026-09-05 — alias resolution.
    Models,
}

impl Namespace {
    /// Every namespace, in D2's table order.
    ///
    /// The length is annotated, so a tenth fails to compile here as well as in
    /// every exhaustive match below.
    pub const ALL: [Self; 9] = [
        Self::Runtime,
        Self::Stack,
        Self::Notes,
        Self::Config,
        Self::Memory,
        Self::Learned,
        Self::Inbox,
        Self::Session,
        Self::Models,
    ];

    /// The spelling inside a session — D2's own first column.
    #[must_use]
    pub const fn slash(self) -> &'static str {
        match self {
            Self::Runtime => "/runtime",
            Self::Stack => "/stack",
            Self::Notes => "/notes",
            Self::Config => "/config",
            Self::Memory => "/memory",
            Self::Learned => "/learned",
            Self::Inbox => "/inbox",
            Self::Session => "/session",
            Self::Models => "/models",
        }
    }

    /// The spelling outside a session, which is the subcommand `zaru` takes.
    ///
    /// **Not derived from [`Namespace::slash`].** `/session` is `zaru
    /// sessions`, per [ADR-0010] D4 and D6 and D2's settling sentence.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub const fn subcommand(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Stack => "stack",
            Self::Notes => "notes",
            Self::Config => "config",
            Self::Memory => "memory",
            Self::Learned => "learned",
            Self::Inbox => "inbox",
            Self::Session => "sessions",
            Self::Models => "models",
        }
    }

    /// What D2's second column says this namespace governs.
    #[must_use]
    pub const fn governs(self) -> &'static str {
        match self {
            Self::Runtime => "tier and membrane",
            Self::Stack => "AEGIS component fetch and status",
            Self::Notes => "Nuclear Notes tokens, workspace, search",
            Self::Config => "configuration and explanation",
            Self::Memory => "relationship memory",
            Self::Learned => "what this session wrote to craft memory",
            Self::Inbox => "deposits",
            Self::Session => "resume, list, remove",
            Self::Models => "alias resolution",
        }
    }

    /// The namespace this subcommand spells, if it spells one.
    ///
    /// Walked from [`Namespace::ALL`] rather than matched against literals, so
    /// a tenth namespace is reachable the moment it is declared.
    #[must_use]
    pub fn from_subcommand(offered: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|namespace| namespace.subcommand() == offered)
    }

    /// Whether this harness implements the namespace's out-of-session half.
    ///
    /// **Four of the nine answer `false`, and that is a statement about this
    /// build rather than about D2.** `/stack` needs [ADR-0003] D7's component
    /// fetch, `/memory` needs [ADR-0031]'s relationship memory, `/learned` and
    /// `/inbox` need [ADR-0002]'s announcement channel — none of which exists
    /// anywhere in this workspace. A word that names one of them is refused
    /// saying so, rather than being placed against the nearest noun, because
    /// telling a user who typed `stack` that they may have meant `sessions` is
    /// a worse answer than telling them the truth.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
    /// [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
    #[must_use]
    pub const fn is_built(self) -> bool {
        match self {
            Self::Runtime | Self::Notes | Self::Config | Self::Session | Self::Models => true,
            Self::Stack | Self::Memory | Self::Learned | Self::Inbox => false,
        }
    }

    /// The verbs this namespace takes outside a session, in the order
    /// `--help` lists them.
    ///
    /// **Empty for a namespace whose subcommand takes no verb**, which is a
    /// different thing from a namespace that is not built: `zaru runtime` and
    /// `zaru models` are whole commands on their own.
    #[must_use]
    pub const fn verbs(self) -> &'static [&'static str] {
        match self {
            Self::Runtime | Self::Models => &[],
            Self::Config => &["explain"],
            Self::Session => &["list", "rm"],
            Self::Notes => &["tokens"],
            Self::Stack | Self::Memory | Self::Learned | Self::Inbox => &[],
        }
    }
}

impl fmt::Display for Namespace {
    /// The subcommand spelling, because this type is only ever rendered by
    /// the out-of-session surface.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.subcommand())
    }
}
