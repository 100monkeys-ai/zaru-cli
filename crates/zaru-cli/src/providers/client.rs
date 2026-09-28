// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The clients this build carries, as one closed value.
//!
//! # Why an enum rather than a generic
//!
//! Until 2026-09-14 the composition held a `GeminiClient` and `cli::classify`
//! took a `GeminiFailure`, because there was one client and naming it was
//! honest. A second client makes that a choice rather than a description, and
//! there were two shapes available.
//!
//! **A generic over [`Model`] was the alternative and is not what was built.**
//! The loop only ever needs `Model`, so a generic composition would compile —
//! but [`crate::cli::classify`] maps a failure to an [ADR-0016] class **by
//! provenance**, and a generic erases exactly that. Recovering it would mean a
//! new trait with a `classify`-shaped method, which puts the error taxonomy's
//! vocabulary behind a trait object in a crate that deliberately keeps the
//! taxonomy in one place.
//!
//! **A closed enum is what the rest of this workspace already does with a
//! closed set**: [`ProviderKind`], [`ModelAlias`](super::ModelAlias),
//! [`Class`](crate::failure::Class) and [`Layer`](crate::config::Layer) are all
//! closed, length-annotated where they are arrays, and matched without a
//! wildcard arm. The property that buys is the one `cli::classify` already
//! relies on and documents: **a third client fails to compile until it is
//! placed**, rather than silently taking a neighbouring class. That is how
//! `GeminiFailure`'s sixth shape was caught, and it is why this is two enums
//! rather than two boxes.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [`Model`]: zaru_core::tool_call::Model

use super::capability::ProviderCapabilities;
use super::endpoint::ProviderEndpoint;
use super::gemini::{GeminiClient, GeminiFailure};
use super::kind::ProviderKind;
use super::ollama::{OllamaClient, OllamaFailure};
use super::openai_compatible::{OpenAiCompatibleClient, OpenAiCompatibleFailure};
use super::port::Provider;
use super::usage::TokenUsage;
use core::fmt;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

/// A provider client this build carries.
///
/// **Two of [ADR-0012] D3's five kinds.** `anthropic`, `openai-compatible` and
/// `aegis` have no client, so they have no variant here: a variant with
/// nothing behind it would be a value the composition could hold and never
/// use.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub enum ProviderClient {
    /// D3's `gemini`, which needs a key.
    Gemini(GeminiClient),
    /// D3's `ollama`, which needs none.
    Ollama(OllamaClient),
    /// D3's `openai-compatible`, whose key is optional -- see
    /// [`crate::providers::selection::KeyUse`].
    OpenAiCompatible(OpenAiCompatibleClient),
}

/// What a provider client could not do.
///
/// One variant per client, so [`crate::cli::classify`] keeps reading a typed
/// failure and keeps its wildcard-free match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderFailure {
    /// The `gemini` client's taxonomy.
    Gemini(GeminiFailure),
    /// The `ollama` client's taxonomy.
    Ollama(OllamaFailure),
    /// The `openai-compatible` client's taxonomy.
    OpenAiCompatible(OpenAiCompatibleFailure),
}

impl fmt::Display for ProviderFailure {
    /// The client's own sentence, unchanged.
    ///
    /// No prefix naming the kind is added. The sentences already name what a
    /// reader needs — an endpoint, a model, an alias — and a provider's own
    /// words are what the surface renders.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gemini(failure) => failure.fmt(f),
            Self::Ollama(failure) => failure.fmt(f),
            Self::OpenAiCompatible(failure) => failure.fmt(f),
        }
    }
}

impl std::error::Error for ProviderFailure {}

impl From<GeminiFailure> for ProviderFailure {
    fn from(failure: GeminiFailure) -> Self {
        Self::Gemini(failure)
    }
}

impl From<OllamaFailure> for ProviderFailure {
    fn from(failure: OllamaFailure) -> Self {
        Self::Ollama(failure)
    }
}

impl From<OpenAiCompatibleFailure> for ProviderFailure {
    fn from(failure: OpenAiCompatibleFailure) -> Self {
        Self::OpenAiCompatible(failure)
    }
}

impl ProviderClient {
    /// One exchange, as the failure taxonomy sees it.
    ///
    /// # Errors
    ///
    /// [`ProviderFailure`], carrying whichever client's typed failure arose.
    pub async fn exchange(
        &self,
        request: &ModelRequest<'_>,
    ) -> Result<ModelResponse, ProviderFailure> {
        match self {
            Self::Gemini(client) => client
                .exchange(request)
                .await
                .map_err(ProviderFailure::from),
            Self::Ollama(client) => client
                .exchange(request)
                .await
                .map_err(ProviderFailure::from),
            Self::OpenAiCompatible(client) => client
                .exchange(request)
                .await
                .map_err(ProviderFailure::from),
        }
    }

    /// What this client's tool surface costs, in bytes as it is sent.
    ///
    /// Dispatched to the kind that answers, because the wire shape is the
    /// kind's: `gemini` narrows each schema to Google's subset and packs
    /// every declaration into one entry, while the other two offer the schema
    /// whole in one entry each. See each client's own method for the
    /// measurement this closes.
    ///
    /// # Errors
    ///
    /// [`ProviderFailure`] for a schema this kind cannot map.
    pub fn tool_surface_bytes(
        &self,
        descriptors: &[zaru_core::tool_call::ToolDescriptor],
    ) -> Result<u64, ProviderFailure> {
        match self {
            Self::Gemini(client) => client
                .tool_surface_bytes(descriptors)
                .map_err(ProviderFailure::from),
            Self::Ollama(client) => client
                .tool_surface_bytes(descriptors)
                .map_err(ProviderFailure::from),
            Self::OpenAiCompatible(client) => client
                .tool_surface_bytes(descriptors)
                .map_err(ProviderFailure::from),
        }
    }

    /// The ratio this client estimates requests at and learns into, shared
    /// with whatever measures the session's context. See
    /// [`crate::providers::capacity::Calibration`].
    #[must_use]
    pub fn calibration(&self) -> super::capacity::Calibration {
        match self {
            Self::Gemini(client) => client.calibration(),
            Self::Ollama(client) => client.calibration(),
            Self::OpenAiCompatible(client) => client.calibration(),
        }
    }

    /// Send this client's answer text to `sender` as each frame arrives.
    pub fn stream_deltas_to(&self, sender: tokio::sync::mpsc::UnboundedSender<String>) {
        match self {
            Self::Gemini(client) => client.stream_deltas_to(sender),
            Self::Ollama(client) => client.stream_deltas_to(sender),
            Self::OpenAiCompatible(client) => client.stream_deltas_to(sender),
        }
    }

    /// The model this client asks for, as the resolution table resolved it.
    #[must_use]
    pub const fn model(&self) -> &super::resolution::ModelId {
        match self {
            Self::Gemini(client) => client.model(),
            Self::Ollama(client) => client.model(),
            Self::OpenAiCompatible(client) => client.model(),
        }
    }
}

impl Provider for ProviderClient {
    fn kind(&self) -> ProviderKind {
        match self {
            Self::Gemini(client) => client.kind(),
            Self::Ollama(client) => client.kind(),
            Self::OpenAiCompatible(client) => client.kind(),
        }
    }

    fn endpoint(&self) -> &ProviderEndpoint {
        match self {
            Self::Gemini(client) => client.endpoint(),
            Self::Ollama(client) => client.endpoint(),
            Self::OpenAiCompatible(client) => client.endpoint(),
        }
    }

    fn capabilities(&self) -> ProviderCapabilities {
        match self {
            Self::Gemini(client) => Provider::capabilities(client),
            Self::Ollama(client) => Provider::capabilities(client),
            Self::OpenAiCompatible(client) => Provider::capabilities(client),
        }
    }

    fn usage(&self) -> Option<TokenUsage> {
        match self {
            Self::Gemini(client) => client.usage(),
            Self::Ollama(client) => client.usage(),
            Self::OpenAiCompatible(client) => client.usage(),
        }
    }
}

impl Model for ProviderClient {
    fn capabilities(&self) -> Capabilities {
        // Through each client's own `Model::capabilities`, which is its
        // `Provider::capabilities` converted -- so the one-statement-read-twice
        // property survives the dispatch rather than being restated here.
        match self {
            Self::Gemini(client) => Model::capabilities(client),
            Self::Ollama(client) => Model::capabilities(client),
            Self::OpenAiCompatible(client) => Model::capabilities(client),
        }
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        // The one place `ProviderFailure` becomes `PortFailure`, mirroring
        // each client's own. The class is lost here, which is right for
        // `zaru-core`; `exchange` above is what the command surface uses when
        // it needs the typed value.
        self.exchange(request)
            .await
            .map_err(|failure| PortFailure::new(failure.to_string()))
    }
}
