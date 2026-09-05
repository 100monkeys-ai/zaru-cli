// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D3's four provider kinds.
//!
//! D3: "`anthropic`, `openai-compatible`, `ollama`, and `aegis`. Everything
//! OpenAI-shaped — vLLM, LM Studio, most gateways — uses the compatible kind
//! rather than earning its own."
//!
//! Closed, for the reason [`ModelAlias`](super::ModelAlias) is: D3 names four
//! and the record's own Negative consequence is that "four provider kinds will
//! not cover everything, and each addition is a maintenance surface with its
//! own streaming quirks and error taxonomy". A fifth is that record's to add.
//!
//! # A kind's name and a kind's key segment are two strings, and they have to be
//!
//! **`openai-compatible` carries a hyphen, and [ADR-0014]'s environment
//! transform turns a key into a variable name by upper-casing it and replacing
//! dots with underscores — it does not touch a hyphen.** So a key segment
//! spelled `openai-compatible` produces `ZARU_PROVIDER_OPENAI-COMPATIBLE_…`,
//! which is not a name a POSIX shell can set: a variable name is letters,
//! digits and underscores. The name a user reads stays D3's, and the key
//! segment is `openai_compatible`, and the two are derived from one
//! wildcard-free match so they cannot drift.
//!
//! **This is a defect in one of the two records and not a preference here**,
//! and it is raised rather than settled: either ADR-0014's transform should
//! map a hyphen as well as a dot, or a key segment may not contain one, or
//! endpoints are not keyed per kind. Recorded on ADR-0014 for its author.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::config::Key;
use core::fmt;

/// One of [ADR-0012] D3's four provider kinds.
///
/// **Closed.** This is the data that selects a provider implementation, and
/// there is no implementation of the provider trait anywhere in this
/// workspace's product tree.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderKind {
    /// D3's first — the Anthropic API.
    Anthropic,
    /// D3's second — "everything OpenAI-shaped — vLLM, LM Studio, most
    /// gateways".
    OpenAiCompatible,
    /// D3's third — a local Ollama server.
    Ollama,
    /// D3's fourth — the AEGIS orchestrator, reached across a process boundary
    /// per [ADR-0003] D5.
    ///
    /// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
    Aegis,
}

impl ProviderKind {
    /// Every kind D3 names, in the record's own order.
    ///
    /// The length is annotated, so a fifth variant fails to compile here as
    /// well as in every exhaustive match below.
    pub const ALL: [Self; 4] = [
        Self::Anthropic,
        Self::OpenAiCompatible,
        Self::Ollama,
        Self::Aegis,
    ];

    /// The first segment of every configuration key that names a provider.
    pub const TABLE: &'static str = "provider";

    /// The last segment of the key that names a provider's endpoint.
    pub const ENDPOINT_LEAF: &'static str = "endpoint";

    /// The kind's name as D3 spells it, which is what a user reads.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai-compatible",
            Self::Ollama => "ollama",
            Self::Aegis => "aegis",
        }
    }

    /// The kind's segment inside a configuration key.
    ///
    /// The same as [`ProviderKind::as_str`] for three of the four. See the
    /// module documentation for why the fourth differs and why that is a
    /// question for ADR-0014 rather than an answer given here.
    #[must_use]
    pub const fn key_segment(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai_compatible",
            Self::Ollama => "ollama",
            Self::Aegis => "aegis",
        }
    }

    // There is deliberately **no** `is_local` here, and the absence is
    // [ADR-0012] D5: "Ollama and OpenAI-compatible local servers configure
    // exactly like hosted ones … the sovereignty promise is not credible if
    // the local path is a second-class code path that breaks quietly." A
    // predicate nobody calls is the thing a later branch is written against,
    // so there is none to call, and D3 makes one impossible anyway --
    // `openai-compatible` covers both vLLM on a laptop and a hosted gateway,
    // so the kind alone does not say where its endpoint is.
    //
    // [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

    /// The [ADR-0014] configuration key that names this kind's endpoint.
    ///
    /// `provider.<kind>.endpoint`. **The project layer may not set it**; the
    /// declaration that refuses it arrives with the schema this module supplies.
    ///
    /// # Panics
    ///
    /// Never. The four segments are this module's own and none of them is
    /// refused by [`Key::new`].
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn endpoint_key(self) -> Key {
        Key::new(&format!(
            "{}.{}.{}",
            Self::TABLE,
            self.key_segment(),
            Self::ENDPOINT_LEAF
        ))
        .expect("ADR-0012 D3's kind segments are well-formed configuration keys")
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
