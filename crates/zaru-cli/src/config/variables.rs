// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The process's environment for one invocation: read once, then handed to
//! every reader.
//!
//! # Why a value, and not a function every reader calls
//!
//! This is [`crate::config::Home`]'s argument made a second time, about the
//! thing `$HOME` itself is read out of. Until 2026-09-27 four readers asked the
//! process's environment for themselves, deep inside whatever called them:
//!
//! - [ADR-0014] D1's layer 4, folded from `std::env::vars()` inside
//!   `cli::layers`;
//! - [ADR-0007] D3's sealing key, read from [`CREDENTIAL_KEY_VARIABLE`] inside
//!   `HarnessKeys` at seven call sites;
//! - the five names [`crate::process::Environment::inherited_minimum`] hands
//!   a child;
//! - `NO_COLOR`, read by the product terminal to choose its palette.
//!
//! So a caller that had an environment to give could not give it. A check
//! above all: `set_var` is `unsafe` in this edition and the workspace denies
//! `unsafe_code`, so a check that handed `cli::layers::resolve` its own pairs
//! still had the sealing key, and every other layer-4 read on the same path,
//! come from whatever the developer's shell exported.
//!
//! **Measured on 2026-09-27 by the `test-env-isolation` arc.** With one
//! undeclared `ZARU_` variable exported, three in-process checks went red and
//! took both home guards with them:
//! `terminal::tests::a_slash_command_produces_what_its_subcommand_spelling_produces`,
//! `a_caller_outside_this_crate_opens_a_shell_over_a_session_and_leaves` and
//! `corpus_a_bare_zaru_at_a_terminal_opens_a_new_sessions_shell`. Each reached
//! layer 4 through a runner and was refused by [ADR-0014] D5 for a variable
//! nobody had handed it. The suite was green on a CI runner and on this
//! machine only because neither exported such a name.
//!
//! So the environment is a value. The binary's `main` reads it once, with
//! [`Variables::of_this_process`], and everything below takes a
//! `&Variables`: one place asks the operating system, and a caller that names
//! different variables is obeyed by every reader rather than by one.
//!
//! # What it holds, and what it never prints
//!
//! Every pair, as the operating system gave it. A name or a value need not be
//! Unicode, and each reader keeps the reading it had: the sealing key and a
//! child's five names are read as text and a non-Unicode value is absent, as
//! `std::env::var` made it; `NO_COLOR` is read as it stands, as
//! `std::env::var_os` did.
//!
//! **`Debug` names the variables and never their values**, because one of
//! them is a sealing key worth every credential in the store, and a value
//! type that prints its contents is a value type somebody will one day
//! format into a refusal.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::credentials::CREDENTIAL_KEY_VARIABLE;
use core::fmt;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

/// The process's environment, as one invocation read it.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Variables {
    held: BTreeMap<OsString, OsString>,
}

impl Variables {
    /// This process's environment, as the operating system gives it.
    ///
    /// **Called once, by the binary's `main`, and nowhere else in the
    /// product** — `corpus_one_thing_reads_the_environment` in
    /// `tests/files_from_outside.rs` walks the source for a second caller,
    /// and for any other reading of the environment outside `main`.
    #[must_use]
    pub fn of_this_process() -> Self {
        Self {
            held: std::env::vars_os().collect(),
        }
    }

    /// No variables at all: a process started with an empty environment.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Exactly these pairs, standing where the process's environment would.
    #[must_use]
    pub fn of<N, V>(pairs: impl IntoIterator<Item = (N, V)>) -> Self
    where
        N: Into<OsString>,
        V: Into<OsString>,
    {
        Self {
            held: pairs
                .into_iter()
                .map(|(name, value)| (name.into(), value.into()))
                .collect(),
        }
    }

    /// These variables and one more, replacing any of the same name.
    #[must_use]
    pub fn with(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.held.insert(name.into(), value.into());
        self
    }

    /// A variable as it stands, where it is set.
    #[must_use]
    pub fn get_os(&self, name: &str) -> Option<&OsStr> {
        self.held.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    /// A variable as text, where it is set and is Unicode.
    ///
    /// `std::env::var(name).ok()`, over this value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.get_os(name).and_then(OsStr::to_str)
    }

    /// [ADR-0007] D3's fallback: the sealing key a machine with no OS keyring
    /// keeps in [`CREDENTIAL_KEY_VARIABLE`].
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    #[must_use]
    pub fn credential_key(&self) -> Option<String> {
        self.get(CREDENTIAL_KEY_VARIABLE).map(str::to_owned)
    }

    /// Every pair whose name begins `prefix`, as text.
    ///
    /// [ADR-0014] D1's layer 4 reads the `ZARU_` names and nothing else. A name
    /// or value there that is not Unicode is carried with each unreadable
    /// sequence replaced rather than dropped: dropped would be D5's "typo that
    /// silently does nothing", and replaced is a name D5 refuses naming it, or
    /// a value the key's own shape refuses. Until 2026-09-27 such a variable —
    /// or one anywhere else in the environment — was a panic inside
    /// `std::env::vars`.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub fn prefixed<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (String, String)> + 'a {
        self.held.iter().filter_map(move |(name, value)| {
            let name = name.to_string_lossy();
            name.starts_with(prefix)
                .then(|| (name.into_owned(), value.to_string_lossy().into_owned()))
        })
    }

    /// Every name, in order. Never a value.
    pub fn names(&self) -> impl Iterator<Item = &OsStr> {
        self.held.keys().map(OsString::as_os_str)
    }
}

impl fmt::Debug for Variables {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Variables")
            .field("names", &self.held.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}
