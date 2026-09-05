// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0007] D3's at-rest half: AES-256-GCM, and where the key comes from.
//!
//! D3: "encrypted at rest with AES-256-GCM, key from the OS keyring where
//! available, environment variable as the CI fallback", mirroring [ADR-093]'s
//! `~/.aegis/auth.json`.
//!
//! # What is a port here and what is not
//!
//! **The cipher is not a port.** D3 names AES-256-GCM; there is nothing to
//! vary, and a port with one implementation is a seam nobody crosses. What
//! genuinely varies by machine is *where the key lives*, so that is
//! [`Keyring`] with two implementations and [`KeyStore`] above it.
//!
//! This replaces the `SecretStore` port the `credential-store` arc declared on
//! 2026-09-04. That port had the wrong shape once there was a real
//! implementation to give it: its `seal` returned `()`, so the ciphertext had
//! nowhere to go, while D3 puts the ciphertext in the file and only the key in
//! the keyring. The harness is pre-alpha, so the old shape is removed rather
//! than kept beside the new one.
//!
//! # The two questions [ADR-0003] amendment 2 left open, and their answers
//!
//! That amendment accepted `aes-gcm` and `keyring` on 2026-09-05 under
//! directive 20 and deliberately left two things undecided, because both are
//! decisions rather than implementation details. Both are answered on
//! [ADR-0007] D3 in the same change that wrote this module, under the same
//! directive:
//!
//! **What the environment variable holds.** Raw key bytes, written as
//! [`SealingKey::HEX_CHARACTERS`] lower-case hexadecimal characters. **No
//! key-derivation function and therefore no third dependency.** The amendment
//! states the reasoning it was raised on: "a passphrase with no KDF is a key
//! with far less entropy than AES-256 assumes, and the difference is invisible
//! from outside". Taking the bytes directly removes the question rather than
//! answering it cheaply, and a value that is not exactly 256 bits of
//! hexadecimal is refused naming the shape and never the value.
//!
//! **What a machine with no keyring does.** It uses the variable, and that is
//! not a concession to CI. Measured 2026-09-05: neither this development
//! machine nor a GitHub runner has a D-Bus session bus, so the "CI fallback" is
//! the ordinary path for a headless box and for anyone running the harness over
//! SSH. With neither source the store refuses, naming both.
//!
//! # `keyring` cannot silently become a mock here, and that was the risk
//!
//! `keyring` 3 has no default features and falls back to an in-memory *mock*
//! credential store when no platform feature is named — `src/lib.rs`:
//! "fallback to mock if neither keyutils nor secret service is available",
//! `pub use mock as default`. Taking it that way gives a keyring that forgets
//! everything while every check passes: [Verification lessons] §26's
//! rule-holding-by-circumstance, on the one boundary where it costs every
//! credential in the store.
//!
//! **`keyring` 4 removes that failure by construction rather than by a check.**
//! It carries `compile_error!("At least one of the features 'v1' or 'cli' must
//! be enabled")`, and both admissible feature sets pull a real Secret Service
//! backend on Linux. Measured 2026-09-05: taking `keyring` 4 with
//! `default-features = false` does not compile. A check for a condition that
//! cannot compile would be [Verification lessons] §36's permanent exemption
//! dressed as a promise, so what is checked instead is the consequence a mock
//! would produce — see `a_keyring_that_is_absent_is_never_reported_as_empty`.
//!
//! `linux-native`, the kernel `keyutils` backend, is disqualified rather than
//! merely larger or smaller. Its own documentation: "The key management
//! facility provided by the kernel is completely in-memory and will not persist
//! across reboots." A sealing key wiped on reboot makes every credential
//! already stored permanently unopenable.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-093]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/093-aegis-cli-authentication-flow
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

pub mod blob;
pub mod failure;
mod hex;
pub mod key;

pub use blob::{Sealed, VERSION};
pub use failure::SealingError;
pub use key::{
    CREDENTIAL_KEY_VARIABLE, FromKeyring, HarnessKeys, KeyStore, Keyring, OsKeyring, SealingKey,
};

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
