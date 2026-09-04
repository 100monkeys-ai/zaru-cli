// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where the credential store meets the Nuclear Notes client.
//!
//! # This module is the whole seam, and that is checkable
//!
//! [ADR-0003] D8 gives `zaru-notes` no sibling dependency, so that crate
//! carries its own [`Bearer`] rather than reusing this crate's
//! [`Secret`](super::Secret) — measured by the `notes-client` arc rather than
//! assumed, because `scripts/check-crate-boundaries.py` counts a
//! *dev*-dependency as a sibling edge exactly as it counts a normal one.
//! `zaru-cli` is the composition root and is therefore where the two meet.
//!
//! **This module is the only place in `crates/zaru-cli/src` that names
//! `zaru_notes`**, the binary's version print aside. That is not tidiness: it
//! means one search finds every crossing between the store and the client,
//! which is the same discipline [`Secret::expose_for_dispatch`] and
//! [`Bearer::expose_for_dispatch`] are named for on their own sides.
//!
//! # Why the conversion is a function and not `impl From`
//!
//! `impl From<&Secret> for Bearer` compiles — the orphan rule permits it,
//! measured rather than reasoned about. It is deliberately not written.
//! A `From` impl makes the conversion available implicitly through `.into()`
//! in argument position, so the number of crossings becomes unbounded and no
//! search finds them. [`bearer_for_dispatch`] is one named door, and a call to
//! it says what it is doing.
//!
//! # Why it lives beside the store rather than on `Entry`
//!
//! The value that exists at dispatch time is a [`Secret`](super::Secret)
//! handed back by [`CredentialStore::secret`](super::CredentialStore::secret).
//! An [`Entry`](super::Entry) is *consumed* by
//! [`CredentialStore::add`](super::CredentialStore::add) and is never seen
//! again, so a method there would be reachable only before the secret was
//! sealed — the wrong end of the lifecycle. It would also invite keeping an
//! entry alive after storing it purely to get a bearer out, which is a second
//! in-memory copy of the secret living outside the sealing port.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [`Secret::expose_for_dispatch`]: super::Secret::expose_for_dispatch

use crate::credentials::secret::Secret;
use zaru_notes::session::Bearer;

/// The bearer a Nuclear Notes session authenticates with, from a stored secret.
///
/// **This is the one place a stored secret becomes a value another crate
/// holds.** [ADR-0007] D3 puts the bearer on the harness's own dispatch path
/// and nowhere else: not in a prompt, not in a transcript, not in a log, not in
/// a tool result. Both types refuse to render the value, so what this function
/// converts is one un-showable value into another; what it must not become is a
/// place the value is copied for any other purpose, which is why it is named
/// for the purpose rather than for the types.
///
/// The value crosses intact and is asserted to, because a conversion that
/// dropped it would satisfy every absence assertion on both sides perfectly.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[must_use]
pub fn bearer_for_dispatch(secret: &Secret) -> Bearer {
    Bearer::new(secret.expose_for_dispatch())
}
