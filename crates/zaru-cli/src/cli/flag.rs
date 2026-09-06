// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The flags `zaru` takes, closed.
//!
//! # The small decisions [ADR-0003] D2's acceptance handed to [ADR-0015]
//!
//! That acceptance says in as many words that "whether `--tier contained` and
//! `--tier=contained` both work, how `--` is handled, whether short flags
//! cluster, what `--help` prints" are **ADR-0015's to make and to write
//! down**, "rather than defaults a crate would have chosen silently". They are
//! made here and written on that record, as delegated coordinator rulings of
//! 2026-09-05 open to Jeshua's veto:
//!
//! - **`--runtime contained` and `--runtime=contained` both work.** A user who
//!   types either has said the same thing, and a parser that takes one and
//!   refuses the other is a parser whose failure mode is a refusal with a
//!   remedy that reads as pedantry. One split at the first `=`.
//! - **`--` ends flag parsing.** Every word after it is a positional, however
//!   it is spelled, which is how a task whose first word begins with a dash is
//!   said at all.
//! - **There are no short flags, so clustering is not a question.** No record
//!   names one. A surface that has none can never have to decide whether `-ab`
//!   is two flags or one.
//! - **`--help` lists only what runs.** See [`crate::cli::help`].
//!
//! # The spelling is `--runtime`, not `--tier`
//!
//! Three records write layer 5's tier flag and two of them abbreviate.
//! [ADR-0001] D2 owns the tier and says "CLI flag `--runtime <tier>`";
//! ADR-0014 D1 illustrates layer 5 as "`--tier`, `--model`, ..." and
//! ADR-0003's third amendment repeats that illustration. The record that owns
//! the thing wins, so the flag is `--runtime` and the two illustrations want
//! correcting — raised on ADR-0014 and ADR-0003, not decided here.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

/// A flag `zaru` takes.
///
/// **Six, closed, with no wildcard match anywhere**, so a seventh cannot
/// arrive without a spelling, a value name, a help line and a place in the
/// parser being decided for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flag {
    /// [ADR-0001] D2's `--runtime <tier>`.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    Runtime,
    /// [ADR-0014] D1's and [ADR-0012] D4's `--model`.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    Model,
    /// [ADR-0011] D3's `--mode <mode>`.
    ///
    /// Named by ADR-0011 D3 on 2026-09-05, in the shape [ADR-0001] D2 names
    /// `--runtime` and [ADR-0012] D4 names `--model`: **the record that owns
    /// the key names the flag**. [ADR-0015] D2 states the flag surface's
    /// parsing contract and lists no flags, so it needed no row for this one.
    ///
    /// It exists because declaring `tools.mode` gives layer 4 for free —
    /// ADR-0014 D1's transform is mechanical, so `ZARU_TOOLS_MODE` works the
    /// moment the key does — and layer 5 being the one layer that could not
    /// say what the environment can would be a gap over exactly the key layer
    /// 5 exists for, a one-off override. `crate::cli::classify`'s refusal for
    /// a project-set mode had also promised one since the taxonomy landed:
    /// "set the permission mode in your own configuration, the environment or
    /// a flag instead".
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Mode,
    /// [ADR-0010] D4's `--resume <id>`.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    Resume,
    /// D4's `--continue`, which is scoped to the directory it is run in.
    ///
    /// The summary below said "the same, for the most recent session" until
    /// 2026-09-06, which agreed with the code and disagreed with the record:
    /// [ADR-0010] D4 is "the most recent session **in this directory**". The
    /// code is what changed; this line is corrected to it.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    Continue,
    /// `--help`.
    Help,
    /// `--version`.
    Version,
}

impl Flag {
    /// Every flag, in the order `--help` lists them.
    ///
    /// The length is annotated, so an eighth fails to compile here as well as
    /// in every exhaustive match below.
    pub const ALL: [Self; 7] = [
        Self::Runtime,
        Self::Model,
        Self::Mode,
        Self::Resume,
        Self::Continue,
        Self::Help,
        Self::Version,
    ];

    /// How the flag is written, with its leading dashes.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Runtime => "--runtime",
            Self::Model => "--model",
            Self::Mode => "--mode",
            Self::Resume => "--resume",
            Self::Continue => "--continue",
            Self::Help => "--help",
            Self::Version => "--version",
        }
    }

    /// What the flag's value is called, or `None` for a flag that takes none.
    #[must_use]
    pub const fn value_name(self) -> Option<&'static str> {
        match self {
            Self::Runtime => Some("<tier>"),
            Self::Model => Some("<identifier>"),
            Self::Mode => Some("<mode>"),
            Self::Resume => Some("<id>"),
            Self::Continue | Self::Help | Self::Version => None,
        }
    }

    /// The one line `--help` prints for it.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        match self {
            Self::Runtime => "set runtime.tier for this run, at ADR-0014 D1's layer 5",
            Self::Model => "set model.default for this run, at the same layer",
            Self::Mode => "set tools.mode for this run, at the same layer: ask, allow or yolo",
            Self::Resume => "restore a session and print its transcript",
            Self::Continue => "the same, for the most recent session started in this directory",
            Self::Help => "print this",
            Self::Version => "print the version and what this binary is composed of",
        }
    }

    /// Whether the flag is a request in its own right rather than a setting.
    ///
    /// The three settings are what layer 5 carries; the four requests each say
    /// what to do instead of what to configure, which is why a request flag
    /// beside a subcommand is refused rather than merged. [ADR-0014] D1
    /// illustrates layer 5 as "`--tier`, `--model`, …" and names no closed
    /// list, so the settings are the keys whose own records give them a flag
    /// rather than a list this enum holds against that illustration.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub const fn is_a_request(self) -> bool {
        match self {
            Self::Runtime | Self::Model | Self::Mode => false,
            Self::Resume | Self::Continue | Self::Help | Self::Version => true,
        }
    }

    /// The flag with this spelling, if there is one.
    ///
    /// Walked from [`Flag::ALL`] rather than matched against literals, so an
    /// eighth is reachable the moment it is declared.
    #[must_use]
    pub fn named(offered: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|flag| flag.spelling() == offered)
    }
}
