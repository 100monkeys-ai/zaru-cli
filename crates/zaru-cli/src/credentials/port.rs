// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two ports the store calls out through, neither implemented here.
//!
//! **Nothing in this crate's product tree implements either**, exactly as
//! nothing in `zaru-core`'s implements one of the loop's five. A test
//! implements them; the product does not, and that is why this arc writes no
//! secret to disk and prompts nobody.

use crate::credentials::alias::Alias;
use crate::credentials::secret::Secret;
use core::fmt;

/// Sealing or unsealing failed.
///
/// Carries a detail string the implementation writes. **An implementation
/// must not put a bearer value in it** — the whole point of the type it
/// handles is that the value does not travel — and the store's own checks
/// assert that no refusal it can raise carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealFailure {
    /// What the implementation said went wrong, in its own words.
    pub detail: String,
}

impl SealFailure {
    /// Report a failure with the implementation's own wording.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for SealFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for SealFailure {}

/// Where a bearer value rests when the harness is not running.
///
/// ADR-0007 D3: "encrypted at rest with AES-256-GCM, key from the OS keyring
/// where available, environment variable as the CI fallback", mirroring
/// [ADR-093]'s `~/.aegis/auth.json`.
///
/// # Why this is a port and not code
///
/// An AEAD implementation and an OS keyring binding are two dependencies, and
/// [ADR-0003] D2's table names neither. Adding one is an amendment to that
/// record rather than an import — its own Trigger clause 7 treats the table
/// as closed in the other direction too, removing an unneeded entry "by
/// amendment rather than left standing unused". Two proposed amendments are
/// drafted on that record; until one is accepted, the shape that is honest is
/// a declared seam with no implementation.
///
/// What that buys is not merely deferral. Because the sealed half does not
/// exist, [`Record`](super::store::Record) has no field a secret
/// could occupy, so "the file on disk carries no secret" is a property of the
/// type rather than a claim about a code path.
///
/// [ADR-093]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/093-aegis-cli-authentication-flow
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
pub trait SecretStore {
    /// Put a bearer value at rest under an alias.
    ///
    /// # Errors
    ///
    /// [`SealFailure`] when the implementation cannot seal, carrying its own
    /// wording and never the value.
    fn seal(&mut self, alias: &Alias, secret: &Secret) -> Result<(), SealFailure>;

    /// Take a bearer value back out.
    ///
    /// # Errors
    ///
    /// [`SealFailure`] when there is nothing under that alias or the
    /// implementation cannot unseal.
    fn unseal(&self, alias: &Alias) -> Result<Secret, SealFailure>;
}

/// How the user answers ADR-0007 D8's confirmation.
///
/// D8: apex combined with a full tool scope "requires explicit confirmation
/// at add time, stating what it grants. Never silent, never a default."
///
/// # This gate is deliberately wider than D8's, and that is a finding
///
/// D8 fires on apex **plus full scope**. The harness cannot tell a full scope
/// from a wide one: "full" is one of [ADR-0135]'s presets, a property of the
/// token row, and none of ADR-0007 D2's eight fields carries it — `tools` is
/// a list of names, and a list of names cannot say whether it is all of them.
/// So this gate fires on **any** apex entry, which is a superset of what D8
/// asks for and settles nothing about what "full" means. The narrower gate
/// waits on whatever field carries the preset, and the widening is recorded
/// on the record rather than left for a reader to discover from behaviour.
///
/// D8 also says apex "is not refused". With a confirmer supplied that holds:
/// the user is asked and may say yes. It is refused only when the caller
/// supplied no way to ask, because a confirmation nobody can answer is the
/// silent default D8 forbids.
///
/// [ADR-0135]: https://cortex.page/adrs/p/0135-mcp-token-tool-scope-presets
pub trait Confirm {
    /// Ask whether to store an apex token, having been told what it grants.
    ///
    /// `grants` is the sentence D8 requires the prompt to state. It is passed
    /// in rather than composed by the implementation so that what the user is
    /// told and what the store believes it asked cannot drift apart.
    fn confirm_apex(&self, alias: &Alias, grants: &str) -> bool;
}
