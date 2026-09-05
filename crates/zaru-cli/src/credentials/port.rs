// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one port the store still calls out through, and it is not implemented
//! here.
//!
//! # There were two, and sealing stopped being one of them
//!
//! Until 2026-09-05 this module also declared a `SecretStore` port for
//! [ADR-0007] D3's at-rest half, with no product implementation, because
//! [ADR-0003] D2's table named neither an AEAD nor a keyring binding. Both are
//! rows in that table now, and the port went with the amendment rather than
//! surviving it.
//!
//! It went because it had the wrong shape once there was something real to put
//! behind it: its `seal` returned `()`, so a ciphertext had nowhere to go,
//! while D3 puts the ciphertext in the store's own file and only the *key* in
//! the keyring. What replaced it is [`crate::credentials::sealing`], where the
//! cipher is ordinary code — D3 names AES-256-GCM and there is nothing to vary
//! — and the seam is around the key, which is the thing that genuinely differs
//! between a laptop and a runner. The harness is pre-alpha, so the old shape is
//! removed rather than kept beside the new one.
//!
//! [`Confirm`] is untouched. **Nothing in this crate's product tree implements
//! it**, exactly as nothing in `zaru-core`'s implements one of the loop's five,
//! so the store still prompts nobody.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::credentials::alias::Alias;

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
