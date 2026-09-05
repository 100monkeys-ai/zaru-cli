// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D1's `meta.toml`, as a typed value and a port with no
//! implementation.
//!
//! D1's directory holds `meta.toml` recording "tier, workspace, provider,
//! started, ended", and trigger clause 1 asks that it record the first three.
//! The value is here; the writer and the reader are not, and the reason is a
//! dependency rather than a preference — see [`crate::session`].
//!
//! # The tier is transcribed from nowhere
//!
//! [`Tier`] is [ADR-0001] D1's three, already declared in
//! this crate for [ADR-0011] D2's enforcement table and re-used here.
//! **Nothing in this module resolves a tier**; ADR-0001 D2 resolves it once
//! at session start and this value records whatever it was handed.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::session::id::Millis;
use crate::tools::Tier;
use core::fmt;

/// What [ADR-0010] D1 says `meta.toml` records.
///
/// `workspace` and `provider` are `Option` because a session may have
/// neither: [ADR-0001] D1 gives `bare` no cortex at all, and no provider is
/// resolved until [ADR-0012] has an implementation. An absent value is
/// recorded as absent rather than as an empty string, so a reader can tell a
/// session that had no workspace from one whose workspace was `""`.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// ADR-0001 D1's tier, resolved once at session start by its owner.
    pub tier: Tier,
    /// The attached Nuclear Notes workspace, where there is one.
    pub workspace: Option<String>,
    /// The provider alias this session generated with, where there is one.
    pub provider: Option<String>,
    /// When the session started.
    pub started: Millis,
    /// When it ended, or `None` while it is still running.
    pub ended: Option<Millis>,
}

/// Reading or writing `meta.toml` failed.
///
/// Carries the implementation's own wording, exactly as
/// [`SealFailure`](crate::credentials::SealFailure) and
/// [`SourceFailure`](crate::config::SourceFailure) do. **An implementation
/// must not put a session's workspace or provider in it**: a refusal is the
/// text that gets pasted into a bug report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl MetaFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for MetaFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for MetaFailure {}

/// Where [`Meta`] rests between sessions.
///
/// **Nothing in this crate's product tree implements this**, exactly as
/// nothing implements the credential store's
/// [`SecretStore`](crate::credentials::SecretStore), configuration's
/// [`LayerSource`](crate::config::LayerSource), or any of `zaru-core`'s five
/// loop ports. A check implements it; the product does not, which is why no
/// `meta.toml` is written anywhere and a session directory holds two files
/// rather than three.
///
/// # Why this is a port and not code
///
/// [ADR-0003] D2's table names no TOML crate. Its first proposed amendment
/// names this exact file and its third proposes the crate; **neither is
/// accepted**, and the same amendment is what holds [ADR-0007] clause 4 and
/// [ADR-0014]'s file layers. Until one is accepted the honest shape is a
/// declared seam — and see [`crate::session`] for why a `std`-only emitter
/// for five flat keys is not the honest alternative it looks like.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub trait MetaStore {
    /// Record what this session is.
    ///
    /// # Errors
    ///
    /// [`MetaFailure`], carrying the implementation's own wording and never
    /// a value out of the [`Meta`].
    fn write(&mut self, meta: &Meta) -> Result<(), MetaFailure>;

    /// Read back what a session was.
    ///
    /// # Errors
    ///
    /// [`MetaFailure`] when there is nothing to read or it does not parse.
    fn read(&self) -> Result<Meta, MetaFailure>;
}
