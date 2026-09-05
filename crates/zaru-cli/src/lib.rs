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
//! crate "Binary, configuration, session lifecycle, the credential store, the
//! local tool surface and its permission model".
//!
//! Ten things are built here. Read off this crate's own module list rather
//! than off a commit log — `cli`, `config`, `credentials`, `failure`,
//! `manifest`, `providers`, `redaction`, `runtime`, `session` and `tools`.
//!
//! [`redaction`] is the **only** product implementation of [ADR-0008] trigger
//! clause 6's port, decided on 2026-09-05: the harness removes from what it
//! sends a model the bearer values it is itself holding in [ADR-0007]'s store,
//! by exact value and by ASCII core, and looks for nothing else. Unknown
//! secrets in command output are named as out of scope by that decision.
//! `zaru-core` declares the port and implements it nowhere.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//!
//! What that means for what acts. `session` writes a real directory, a real
//! append-only transcript and a real checkpoint. `tools` decides, and **three
//! of ADR-0011 D1's seven built-ins now act**: `fs.read` and `fs.list`,
//! through `std::fs`, inside D4's working-directory boundary, and `cmd.run`,
//! through [`process`] — which is the one place this workspace starts a child
//! process, and the first thing here that acts on the world outside a file.
//! `process` also implements ADR-0009 D3's `ValidatorRunner`, so a declared
//! validator's command runs. The other four built-ins sit behind ports with
//! no implementation in this product tree, as do the prompt, the allowlist,
//! the destructive matcher, the credential store's sealing, ADR-0014's file
//! layers **2 and 3** — layer 5 has a reader as of 2026-09-05,
//! [`cli::layers::Flags`], the first implementation of that port in this
//! product tree — `meta.toml`'s writer, ADR-0009's manifest reader,
//! ADR-0012's provider and its alias negotiation, and ADR-0004's membrane.
//! **Nothing in this workspace can reach a provider at all**, and nothing
//! contains a child process — ADR-0011 D2: "the harness is not a sandbox and
//! says so."
//!
//! **Six of the ten are reachable from the `zaru` binary as of 2026-09-05**,
//! and the sentence that said none of them was is gone with the argument
//! parser ADR-0003 D2 decided under directive 20. [`cli`] is [ADR-0015] D2's
//! out-of-session surface: it parses, folds three of ADR-0014 D1's five
//! layers, and prints `runtime`, `models`, `config explain <key>`, `sessions
//! list`, `sessions rm <id>` and `notes tokens`. The binary still **runs no
//! task**, because that needs a provider; `zaru --help` says so.
//!
//! `tools`, `manifest` and `redaction` are what nothing a user can type
//! reaches, and each waits on what it always did: a tool-call loop with a
//! model behind it, a TOML reader, and a prompt to redact.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//!
//! [Bounded Contexts] names no crate for the tool surface. It is here under a
//! delegated coordinator ruling of 2026-09-04, recorded on that page and on
//! ADR-0011, because every input the permission decision needs — the resolved
//! runtime tier, the permission mode's configuration layer, the allowlist, the
//! working directory, the session directory — is a property of the whole
//! program rather than of any one part, which is this crate's whole
//! responsibility.
//!
//! [Bounded Contexts] names no crate for ADR-0012's provider abstraction
//! either, and [`providers`] is here under a delegated coordinator ruling of
//! 2026-09-05, recorded on that page and on ADR-0012, for the same reason: an
//! alias resolves through ADR-0014's five layers, which are this crate's, and
//! nothing else in the workspace can see them. **No provider is called from
//! anywhere in this workspace** and the provider trait has no implementation
//! in any product tree.
//!
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

/// The crates this binary is composed of, each reporting its own name and
/// version rather than being described by a list kept here.
///
/// A list retyped beside the binary is a list that drifts. Every entry is read
/// out of the crate itself, so an entry can only be wrong if that crate's own
/// package metadata is wrong.
///
/// In the library rather than in `main.rs` since 2026-09-05, because
/// `--version` prints it and [`cli::run`] is where a request becomes lines.
#[must_use]
pub fn composition() -> [(&'static str, &'static str); 5] {
    [
        (zaru_core::NAME, zaru_core::VERSION),
        (zaru_tui::NAME, zaru_tui::VERSION),
        (zaru_notes::NAME, zaru_notes::VERSION),
        (zaru_seal::NAME, zaru_seal::VERSION),
        (zaru_aegis::NAME, zaru_aegis::VERSION),
    ]
}

pub mod cli;
pub mod config;
pub mod credentials;
pub mod failure;
pub mod manifest;
pub mod process;
pub mod providers;
pub mod redaction;
pub mod runtime;
pub mod session;
pub mod tools;
