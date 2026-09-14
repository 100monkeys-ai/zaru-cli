// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0012] D3's capability descriptor, and the refusal that has to happen
//! before a loop starts.
//!
//! # D3's own reason, which is a statement about *when*
//!
//! D3: "The trait carries streaming, tool calling, token accounting, and a
//! capability descriptor. **The capability descriptor is load-bearing**: a
//! provider that cannot do tool calling must say so, because discovering it
//! mid-loop produces a failure the user reads as the harness being broken."
//!
//! So the descriptor is *data*, consulted at configuration time by
//! [`ProviderCapabilities::require_tool_calling`], and the refusal it produces
//! is [ADR-0016]'s user-correctable class carrying a remedy — because the user
//! can act, and what they can do is point the alias at a provider that calls
//! tools.
//!
//! # Clause 3 has two halves and neither is counted twice
//!
//! Trigger clause 3 — "A provider declaring no tool-call capability fails at
//! configuration time with a clear message, not mid-loop" — is satisfied here
//! for the **configuration-time** half: the descriptor is consulted before
//! anything is built and the refusal names the alias, the provider and the
//! remedy.
//!
//! The **structural** half belongs to the tool-call loop in `zaru-core`, which
//! demands a witness that the check happened before its first event, so a model
//! that cannot call tools cannot reach a turn at all. That is a guarantee about
//! the loop rather than a second check of this rule.
//!
//! **That `From` landed on 2026-09-05**, when the first provider client
//! arrived and needed to answer both traits: `impl From<ProviderCapabilities>
//! for zaru_core::tool_call::Capabilities` below is what makes
//! [`GeminiClient`](crate::providers::GeminiClient)'s two `capabilities`
//! methods one statement read twice rather than two literals that can drift.
//! **One rule, one place**, which is the ruling
//! [`Layer`](crate::config::Layer) already carries in this workspace.
//!
//! # Streaming and token accounting are carried and not consulted
//!
//! D3 names four concerns and this module refuses on exactly one of them,
//! because D3 gives a reason for exactly one. Nothing here refuses a provider
//! that cannot stream: no record says a non-streaming provider may not be
//! used, and inventing that refusal would settle a question ADR-0012 does not
//! ask. `token_accounting` is the flag [ADR-0012] D7's accounting is paired
//! with, and it is paired with the provider trait's own usage method.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::providers::alias::ModelAlias;
use crate::providers::kind::ProviderKind;
use core::fmt;

/// A provider cannot do something a configuration asked of it.
///
/// **Raised at configuration time and never inside a loop**, which is the
/// whole of [ADR-0012] trigger clause 3's "not mid-loop".
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRefused {
    /// No layer says how large the window behind an alias is.
    ///
    /// Carries the alias **and** the kind for the reason the arm below does:
    /// the alias is what the reader wrote and the kind is what has no window.
    ContextSizeUnknown {
        /// The alias whose provider was asked.
        alias: ModelAlias,
        /// The kind that could not say.
        kind: ProviderKind,
    },
    /// The provider behind an alias declares it cannot call tools.
    ///
    /// Carries the alias **and** the kind, because both are what a user needs
    /// in order to act: the alias is the thing they wrote, and the kind is the
    /// thing that cannot do it.
    ToolCallingUnavailable {
        /// The alias whose provider was asked.
        alias: ModelAlias,
        /// The kind that declared it cannot.
        kind: ProviderKind,
    },
}

impl fmt::Display for CapabilityRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextSizeUnknown { alias, kind } => write!(
                f,
                "the alias `{alias}` resolves to a `{kind}` provider and nothing says how large \
                 its context window is. A harness that guessed would compact too late and let the \
                 provider silently drop the oldest of a conversation, which is the one failure a \
                 reader cannot diagnose",
            ),
            Self::ToolCallingUnavailable { alias, kind } => write!(
                f,
                "the alias `{alias}` resolves to a `{kind}` provider that declares it cannot call \
                 tools, and the work asked for needs them. A provider says so \
                 rather than have it be discovered mid-loop, where it reads as the harness being \
                 broken",
            ),
        }
    }
}

impl std::error::Error for CapabilityRefused {}

/// What a provider says it can do, per [ADR-0012] D3.
///
/// Supplied by whoever configures a provider.
///
/// **This sentence said "Nothing in this workspace implements a provider"
/// until 2026-09-14, and it was false from 2026-09-05.** Two clients declare
/// their own descriptor today — `gemini` and `ollama`, both
/// `declared(true, true, true)` — and each is measured against its own
/// endpoint rather than asserted. The correction matters because this type's
/// whole purpose is that a provider **says** what it can do, and a comment
/// claiming nobody says anything invites the next reader to treat the
/// descriptor as decorative.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilities {
    streaming: bool,
    tool_calling: bool,
    token_accounting: bool,
    context_tokens: Option<u64>,
}

impl ProviderCapabilities {
    /// Declare what a provider can do.
    ///
    /// The four are positional and all four are required, so a provider
    /// cannot be described without saying something about each — which is
    /// what "must say so" means.
    ///
    /// `context_tokens` is `None` for a provider whose window nothing states,
    /// which is a real answer rather than a missing one: it is what
    /// [`Self::require_context_size`] refuses on, before a loop starts.
    #[must_use]
    pub const fn declared(
        streaming: bool,
        tool_calling: bool,
        token_accounting: bool,
        context_tokens: Option<u64>,
    ) -> Self {
        Self {
            streaming,
            tool_calling,
            token_accounting,
            context_tokens,
        }
    }

    /// Whether the provider streams, per D3's first concern.
    ///
    /// Carried and never refused on: see the module documentation.
    #[must_use]
    pub const fn streaming(self) -> bool {
        self.streaming
    }

    /// Whether the provider can call tools, per D3's second concern.
    #[must_use]
    pub const fn tool_calling(self) -> bool {
        self.tool_calling
    }

    /// Whether the provider reports what a request cost, per D3's third.
    ///
    /// Paired with the provider trait's own usage method: a provider that
    /// answers `false` here reports no usage, and one that answers `true`
    /// reports some.
    #[must_use]
    pub const fn token_accounting(self) -> bool {
        self.token_accounting
    }

    /// How large the provider's context window is, in tokens.
    ///
    /// **A fourth concern, added 2026-09-14, and the reason is that the third
    /// one is not it.** `token_accounting` says whether the provider reports
    /// what a request *cost*; this says what it will *accept*. Until this
    /// field existed the harness carried one model's number — Google's
    /// 1,048,576 for `gemini-3.6-flash` — as a constant in the composition,
    /// which was wrong for the second kind the day it had a client and was
    /// wrong for three kinds by the time this landed.
    ///
    /// Each kind answers from its own source, and no source is guessed:
    /// `gemini` from its own documentation, `ollama` from
    /// `provider.ollama.context_tokens` over a default that is the server's
    /// own, `openai-compatible` from that key with no default at all. See
    /// each client.
    ///
    /// `None` means nothing says, and
    /// [`Self::require_context_size`] refuses it.
    #[must_use]
    pub const fn context_tokens(self) -> Option<u64> {
        self.context_tokens
    }

    /// Refuse, at configuration time, a provider whose window nothing states.
    ///
    /// **Beside [`Self::require_tool_calling`] and for its reason**: D3's
    /// "discovering it mid-loop produces a failure the user reads as the
    /// harness being broken" is if anything stronger here, because a window
    /// nobody states is not discovered mid-loop at all — the provider accepts
    /// the request and truncates it, and what the reader sees is a model that
    /// forgot something they remember saying.
    ///
    /// # Errors
    ///
    /// [`CapabilityRefused::ContextSizeUnknown`], naming the alias and the
    /// kind.
    pub const fn require_context_size(
        self,
        alias: ModelAlias,
        kind: ProviderKind,
    ) -> Result<u64, CapabilityRefused> {
        match self.context_tokens {
            Some(tokens) => Ok(tokens),
            None => Err(CapabilityRefused::ContextSizeUnknown { alias, kind }),
        }
    }

    /// Refuse, at configuration time, a provider that cannot call tools.
    ///
    /// # Errors
    ///
    /// [`CapabilityRefused::ToolCallingUnavailable`], naming the alias and the
    /// kind.
    pub const fn require_tool_calling(
        self,
        alias: ModelAlias,
        kind: ProviderKind,
    ) -> Result<(), CapabilityRefused> {
        if self.tool_calling {
            return Ok(());
        }
        Err(CapabilityRefused::ToolCallingUnavailable { alias, kind })
    }
}

/// [ADR-0012] D3's descriptor as the tool-call loop needs it.
///
/// # One statement, not two
///
/// `zaru-core`'s [`Capabilities`] carries the one flag its loop refuses on,
/// and this type carries all three of D3's. A provider implementing both
/// traits answers each `capabilities` method through this conversion, so a
/// client that stops calling tools cannot say so in one place and not the
/// other. Declaring a second literal is what this exists to prevent, and it
/// is the shape this module's documentation promised when the loop's own type
/// landed.
///
/// Nothing is lost that the loop reads: streaming and token accounting have
/// no arm in `Capabilities`, because ADR-0012 D3 gives a reason for refusing
/// on exactly one concern and the loop refuses on exactly that one.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [`Capabilities`]: zaru_core::tool_call::Capabilities
impl From<ProviderCapabilities> for zaru_core::tool_call::Capabilities {
    fn from(declared: ProviderCapabilities) -> Self {
        Self {
            tool_calling: declared.tool_calling(),
        }
    }
}
