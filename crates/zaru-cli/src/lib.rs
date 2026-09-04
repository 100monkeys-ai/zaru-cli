// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The library half of the `zaru` binary's crate.
//!
//! # Why this crate has a library target at all
//!
//! `zaru-cli` began as a binary alone. A binary target cannot be named from
//! an integration test, so anything that lives only in `src/main.rs` can be
//! exercised only through the process — and the credential store has to be
//! drivable through its own public door by a caller outside this crate, which
//! is the reachability evidence [Verification lessons] §25 asks for. Adding a
//! library target changes no crate boundary: ADR-0003 D8 names six crates and
//! there are still six, `scripts/check-crate-boundaries.py` reads packages
//! rather than targets, and `publish = false` is unchanged.
//!
//! # Boundary
//!
//! This crate is the composition root. ADR-0014's configuration hierarchy,
//! ADR-0010's session lifecycle, ADR-0016's error taxonomy and the credential
//! store all belong here, because each is a property of the whole program
//! rather than of any one part — see [Bounded Contexts], which gives this
//! crate "Binary, configuration, session lifecycle, the credential store".
//!
//! Only the credential store is built. Nothing else named above exists yet.
//!
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

pub mod credentials;
