// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The credential store: ADR-0007's named tokens, held locally.
//!
//! # What this module is, in the workspace's own vocabulary
//!
//! [Ubiquitous Language] fixes the words and this module uses no others. A
//! **credential store** is "the local store of named tokens". An **alias** is
//! "a token's local name — the handle in CLI, transcript, and tool
//! namespace". The **composer role** is "the single token flagged as the
//! composer's". An **apex token** is "a token with no instance boundary".
//!
//! **There is deliberately no `Context` type**, although ADR-0007's title is
//! "named tokens as contexts". A context is the *idea* that record names —
//! the approach to Nuclear Notes a token embodies, being its workspace
//! pointer, tool scope, rate-limit bucket and audit identity, all of which
//! live on the token row server-side and none of which this store owns. The
//! word is taken twice over in this codebase already: `architecture/bounded-contexts`
//! calls a crate a context, and `zaru-core` calls ADR-0013's layering a
//! `ContextPolicy`. A third meaning in a third place is how a word stops
//! carrying one.
//!
//! # Where the secrets are not
//!
//! ADR-0007 D3 has the store encrypted at rest with AES-256-GCM under a key
//! from the OS keyring. **No such thing is built here, and no secret this
//! module holds is written to disk.** The crates that would do it — an AEAD
//! implementation and a keyring binding — are not in ADR-0003 D2's dependency
//! table, and adding one is an amendment to that record rather than an
//! import. So sealing is a port declared beside this module with no implementation in
//! this crate's product tree, exactly as `zaru-core` declares five ports it
//! does not implement.
//!
//! The consequence is load-bearing rather than incidental: the type that is
//! written to disk has **no field a secret could go in**. That is the same argument ADR-0014 D4 makes about configuration —
//! "the design decision that prevents it is refusing to have a field to put
//! one in" — and it is why the on-disk half needed no encryption in order to
//! be honest. A store that wrote a secret in plaintext until the sealing arc
//! arrived would be the "for now" the harness forbids.
//!
//! # Where it sits, and what it is not
//!
//! The store lives beside `~/.zaru/config.toml`, **not inside it and not as a
//! layer of it**. ADR-0014 D1 makes `~/.zaru/config.toml` layer 2 of the
//! configuration hierarchy, and D4 says configuration holds a *reference* to
//! a credential and never a credential, because "a config file gets committed
//! to a repository". Configuration names an alias; this store holds the
//! value. Calling the store a configuration layer would reopen exactly the
//! hole D4 closes.
//!
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod alias;
pub mod entry;
pub mod notes;
pub mod port;
pub mod projection;
pub mod secret;
pub mod store;

pub use alias::{Alias, AliasRefused};
pub use entry::{
    COMPOSER_SCOPE, Description, DescriptionRefused, Entry, Instance, Reach, Role, ToolScope, Ttl,
    TtlRefused,
};
pub use notes::{Cached, Refreshed, ScopeError, bearer_for_dispatch};
pub use port::{Confirm, SealFailure, SecretStore};
pub use projection::{NAMESPACE_PREFIX, Namespace};
pub use secret::{Kind, REDACTED, Secret, SecretRefused};
pub use store::{CredentialStore, Record, StoreError, StoredReach};

// `pub(crate)` rather than private: `crate::config`'s checks plant the same
// awkward nonces and assert the same ASCII core, and the reason that core
// exists is a mutation that survived here on 2026-09-04. A second copy of
// that reasoning beside the configuration checks would be a rule living in
// two places, which is a rule that diverges.
#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
