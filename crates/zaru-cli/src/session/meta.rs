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
//! # The tier is transcribed from nowhere, and it cannot be changed here
//!
//! [`Tier`] is [ADR-0001] D1's three, declared in [`crate::runtime`] with the
//! record that owns it. **Nothing in this module resolves a tier**; ADR-0001
//! D2 resolves it once at session start and this value records what it was
//! handed — as a [`ResolvedTier`], which cannot be built without naming the
//! configuration layer it came from.
//!
//! The field is **private**, so D2's "immutable for the life of a session" is
//! a property of the type rather than a rule a caller remembers. See [`Meta`].
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::runtime::{ResolvedTier, Tier};
use crate::session::id::Millis;
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
    /// ADR-0001 D1's tier, resolved once at session start.
    ///
    /// **Private, and that is D2's immutability rather than a style.** There
    /// is no setter, no `&mut` accessor and no way to reach this field from
    /// outside this module, so a session's tier cannot be replaced after the
    /// session exists. It was `pub` until 2026-09-05, which made "immutable
    /// for the life of a session" a rule somebody had to remember rather than
    /// a property of the type.
    ///
    /// A [`ResolvedTier`] rather than a bare [`Tier`], so the tier a session
    /// records is by construction the tier that was *resolved* — that type
    /// has no constructor that does not name where the value came from.
    tier: ResolvedTier,
    /// The attached Nuclear Notes workspace, where there is one.
    pub workspace: Option<String>,
    /// The provider alias this session generated with, where there is one.
    pub provider: Option<String>,
    /// When the session started.
    pub started: Millis,
    /// When it ended, or `None` while it is still running.
    pub ended: Option<Millis>,
}

impl Meta {
    /// What a session records about itself when it starts.
    ///
    /// `ended` is `None`, because a session that is being started has not
    /// ended; D1 makes that field's absence mean "still running".
    #[must_use]
    pub const fn new(
        tier: ResolvedTier,
        workspace: Option<String>,
        provider: Option<String>,
        started: Millis,
    ) -> Self {
        Self {
            tier,
            workspace,
            provider,
            started,
            ended: None,
        }
    }

    /// ADR-0001 D1's tier for this session.
    #[must_use]
    pub const fn tier(&self) -> Tier {
        self.tier.tier()
    }

    /// The tier together with the configuration layer that supplied it.
    ///
    /// [ADR-0010] D1 asks `meta.toml` to record the tier and says nothing
    /// about where it came from, so a writer may record the tier alone. The
    /// layer is carried because [ADR-0014] D3's whole argument is that a user
    /// who cannot see where a value came from cannot fix it.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub const fn resolved_tier(&self) -> ResolvedTier {
        self.tier
    }
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
