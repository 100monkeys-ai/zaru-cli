// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `~/.zaru/` and the one function that creates it.
//!
//! # Why one function, and why it is here
//!
//! Four records put a file in `~/.zaru/`: [ADR-0004] D3's `node.key`,
//! [ADR-0007] D3's `credentials.json`, [ADR-0010] D1's `sessions/<ulid>/`,
//! and [ADR-0014] D1's `config.toml`. The directory carries `0700` because
//! ADR-0004 D3 and ADR-0007 both need it to, and a directory whose mode
//! depends on which of four callers happened to run first is a rule holding
//! by circumstance — [Verification lessons] §26, which reads identically to
//! one holding by construction and is the harder failure to see.
//!
//! Until 2026-09-04 the rule was that the credential store was the sole
//! creator and every other module refused to create the directory. That
//! solved the two-creators problem and created a worse one for the session
//! lifecycle: a session refused because no credential had ever been added is
//! an ordering obligation on whoever composes the program, which is the
//! "for now" the harness forbids rather than a mechanism. So under a
//! **delegated coordinator ruling of 2026-09-04**, open to Jeshua's veto,
//! there is exactly one creator and it is a *function* rather than a module:
//! [`ensure`]. Every caller calls it; nobody else creates the directory.
//!
//! It lives in `config` because [Bounded Contexts] gives `zaru-cli`
//! configuration and ADR-0014 D1 is what makes `~/.zaru/` the harness's own
//! directory at all. **The configuration loader still never creates it** —
//! see [`crate::config::port`] — because a loader that created a directory in
//! order to find nothing in it would be creating state to read state.
//!
//! # The layout constant is not re-typed here
//!
//! [`HOME_DIRECTORY`] and
//! [`DIRECTORY_MODE`] stay where
//! the credential store declared them and are read from there. Moving them
//! would be more than the call-site substitution the ruling permits in that
//! module, and copying them would be the rule-in-two-places this function
//! exists to remove, one level down.
//!
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::credentials::store::{DIRECTORY_MODE, HOME_DIRECTORY};
use core::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// [ADR-0014] D1's layer 2, inside `~/.zaru/`.
///
/// One spelling, read by [`crate::cli::layers`] and named by D1, so the file
/// the loader opens and the file the record names cannot drift apart.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const CONFIG_FILE: &str = "config.toml";

/// `~/.zaru`, where this machine's home directory is one.
///
/// **The one place this join is written.** It was written twice until
/// 2026-09-05 — once in the credential store and once in the session store —
/// and layer 2 would have been a third. Both call this now, and each keeps its
/// own error for the absent case, because "no home directory" means different
/// things to a store that is about to write and to a loader that is about to
/// read.
///
/// `None` where no home directory can be resolved, which for a *loader* is not
/// a failure: a machine with no home has no `~/.zaru/config.toml`, so
/// [ADR-0014] D3 renders that layer as `(not set)` against its own label and
/// no file is claimed to have been opened.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn default_root() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join(HOME_DIRECTORY))
}

/// `~/.zaru/` could not be made ready.
///
/// Two variants rather than one, because the two failures want different
/// sentences: a directory that could not be created and one that exists and
/// whose mode could not be set are different problems for whoever reads them,
/// and a caller that wraps this in its own error type needs to say which.
#[derive(Debug)]
pub enum HomeFailure {
    /// The directory could not be created.
    NotCreated {
        /// The directory.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The directory exists and [`DIRECTORY_MODE`] could not be set on it.
    ///
    /// [`DIRECTORY_MODE`]: crate::credentials::store::DIRECTORY_MODE
    ModeNotSet {
        /// The directory.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl HomeFailure {
    /// The directory the failure is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::NotCreated { path, .. } | Self::ModeNotSet { path, .. } => path.as_path(),
        }
    }

    /// What was being attempted, in words a wrapping error can quote.
    #[must_use]
    pub const fn action(&self) -> &'static str {
        match self {
            Self::NotCreated { .. } => "create the directory",
            Self::ModeNotSet { .. } => "set 0700 on the directory",
        }
    }

    /// What the operating system said.
    #[must_use]
    pub const fn source_error(&self) -> &std::io::Error {
        match self {
            Self::NotCreated { source, .. } | Self::ModeNotSet { source, .. } => source,
        }
    }

    /// Whether the creation failed, rather than the mode.
    ///
    /// A caller wrapping this in its own error type needs to say which of the
    /// two happened in its own words, and a caller that matched on the string
    /// [`HomeFailure::action`] returns would be reading a sentence as a
    /// discriminant.
    #[must_use]
    pub const fn is_creation(&self) -> bool {
        matches!(self, Self::NotCreated { .. })
    }

    /// Take the failure apart, for a caller that wraps it.
    #[must_use]
    pub fn into_parts(self) -> (PathBuf, std::io::Error) {
        match self {
            Self::NotCreated { path, source } | Self::ModeNotSet { path, source } => (path, source),
        }
    }
}

impl fmt::Display for HomeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not {} {}: {}",
            self.action(),
            self.path().display(),
            self.source_error()
        )
    }
}

impl std::error::Error for HomeFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source_error())
    }
}

/// Make `root` exist and carry [`DIRECTORY_MODE`].
///
/// **This is the only place in the harness that creates the harness's own
/// directory.** Every module that keeps a file under it calls this rather
/// than creating it, so the mode is a property of one function instead of a
/// race between four.
///
/// The mode is set on **every** call rather than only on creation. A `0700`
/// directory that is group- or world-readable is a defect whoever created it,
/// and a call that noticed and did nothing would be a comment rather than a
/// mechanism ([Verification lessons] §30).
///
/// `root` is a parameter rather than always `~/.zaru` because the product
/// needs it to be — a check is then an ordinary caller writing to the paths
/// the product writes to, rather than a fake standing in for the filesystem.
///
/// # Errors
///
/// [`HomeFailure::NotCreated`] and [`HomeFailure::ModeNotSet`].
///
/// [`DIRECTORY_MODE`]: crate::credentials::store::DIRECTORY_MODE
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub fn ensure(root: &Path) -> Result<(), HomeFailure> {
    fs::create_dir_all(root).map_err(|source| HomeFailure::NotCreated {
        path: root.to_path_buf(),
        source,
    })?;
    fs::set_permissions(root, fs::Permissions::from_mode(DIRECTORY_MODE)).map_err(|source| {
        HomeFailure::ModeNotSet {
            path: root.to_path_buf(),
            source,
        }
    })
}
