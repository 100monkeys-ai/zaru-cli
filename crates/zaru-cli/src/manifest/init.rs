// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0009] D6's other half: `zaru init` writes the manifest, once.
//!
//! # This is the one exception to "the manifest is read, never written"
//!
//! D6: "**The manifest is read, never written.** Zaru does not edit
//! `zaru.toml` on the user's behalf, and `zaru init` writes it once, on an
//! explicit command, only when absent."
//!
//! [`crate::manifest::port`] holds the first sentence by absence — the trait
//! has one method and there is no `ManifestWriter` — and
//! `no_product_source_writes_a_manifest` holds it from the other side, by
//! walking both crates' product sources for a write verb. That check now
//! carries **exactly one named exception, and it is this module**. A second
//! writer anywhere reddens it, which is what makes adding one a visible act
//! rather than a diff nobody reads.
//!
//! # What it writes is the record's own worked example
//!
//! [`TEMPLATE`] is [ADR-0009] D1's manifest, as that record prints it after
//! directive 20 corrected it: no `tier`, and a `max_iterations` that lowers.
//! **Nothing here infers anything about the project** — not its name, not its
//! language, not its build command — because inference is that record's
//! Alternative 1 and it is rejected in as many words: "Inference means what
//! counts as success is invisible — the user cannot read the file and know what
//! the agent must satisfy."
//!
//! So a user gets a file they must edit, which is the point: what runs is
//! always visible in the file. The consequence is worth stating rather than
//! hiding — `zaru init` in a project that is not `acme-api` writes a manifest
//! naming `acme-api`, and the user renames it. The alternative was a template
//! this arc invented, which would be a capability's user-facing prose chosen by
//! whoever typed it.
//!
//! # It never overwrites, and that is a property of the system call
//!
//! A check-then-write has a window: the file can appear between the check and
//! the write, and the harness would then destroy a manifest a user had just
//! authored. [`write()`] closes it with the primitive that has no window —
//! the content is written to a sibling, synced, and then **linked** into place,
//! and `link` fails if the destination exists. So the name appears only when
//! the whole file is behind it, and it appears only if nothing was there.
//!
//! That is deliberately *not* [`crate::atomic::write`], which renames over
//! whatever is there and which the checkpoint, the credential store and
//! `meta.toml` all go through. The two are different operations: that one
//! replaces a file this harness owns, and this one claims a name it must not
//! take from anybody. The sibling's *name* is shared, because a leftover
//! temporary should be recognisable wherever it came from.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators

use crate::manifest::file::ManifestFile;
use crate::session::store::FILE_MODE;
use core::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// [ADR-0009] D1's worked manifest, as that record prints it.
///
/// Transcribed rather than composed, and transcribed **after** directive 20
/// corrected it on 2026-09-05: `[runtime]` no longer sets a `tier`, which
/// [ADR-0014] D6 forbids a project, and its `max_iterations` lowers rather than
/// raises. `adr_0009_d1s_worked_manifest_is_what_init_writes_and_it_folds`
/// reads it back through the reader and through the binary's own schema, so a
/// template that stopped being loadable reddens rather than shipping.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const TEMPLATE: &str = r#"[project]
name = "acme-api"
workspace = "acme-engineering"   # Nuclear Notes workspace, per ADR-0006 D5

[runtime]
max_iterations = 3

[[validator]]
name = "build"
run  = "cargo build --locked"
expect = "exit-zero"

[[validator]]
name = "test"
run  = "cargo test --all"
expect = "exit-zero"
after = ["build"]

[[validator]]
name = "shape"
run  = "cargo run -- --emit-schema"
expect = { json_schema = "schema/output.json" }
"#;

/// Why `zaru init` did not write a manifest.
#[derive(Debug)]
pub enum InitRefused {
    /// There is already one, and D6 says this command writes only when absent.
    ///
    /// **Not an overwrite that was declined; an overwrite that could not
    /// happen.** See the module documentation.
    AlreadyThere {
        /// The file that is there.
        path: PathBuf,
    },
    /// A step of the write failed.
    NotWritten {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl InitRefused {
    /// The file the refusal is about.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::AlreadyThere { path } | Self::NotWritten { path, .. } => path.as_path(),
        }
    }
}

impl fmt::Display for InitRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyThere { path } => write!(
                f,
                "{} already exists, and this command writes a manifest only when there is none. \
                 ADR-0009 D6: the manifest is read, never written — configuration a tool silently \
                 rewrites is configuration the user stops trusting, and this file governs what \
                 runs on their machine",
                path.display(),
            ),
            Self::NotWritten {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for InitRefused {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlreadyThere { .. } => None,
            Self::NotWritten { source, .. } => Some(source),
        }
    }
}

/// Write [`TEMPLATE`] to `file`'s path, once, only when it is absent.
///
/// Returns the path that was written, so a caller can say which file it made
/// without composing the path a second time.
///
/// # Errors
///
/// [`InitRefused::AlreadyThere`] when there is already a manifest, and
/// [`InitRefused::NotWritten`] for the write, the sync or the link.
pub fn write(file: &ManifestFile) -> Result<PathBuf, InitRefused> {
    let path = file.path().to_path_buf();
    let temporary = crate::atomic::temporary_path(&path);

    // The content is complete and on the disk before the name exists.
    let mut sibling = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(FILE_MODE)
        .open(&temporary)
        .map_err(|source| InitRefused::NotWritten {
            action: "open the sibling temporary for writing",
            path: temporary.clone(),
            source,
        })?;
    let written = sibling
        .write_all(TEMPLATE.as_bytes())
        .and_then(|()| sibling.sync_all());
    drop(sibling);
    if let Err(source) = written {
        let _ = fs::remove_file(&temporary);
        return Err(InitRefused::NotWritten {
            action: "write the sibling temporary",
            path: temporary,
            source,
        });
    }

    // `link` refuses an existing destination, so "only when absent" has no
    // window between a check and a write. A rename would take the name from
    // whoever holds it.
    let linked = fs::hard_link(&temporary, &path);
    let _ = fs::remove_file(&temporary);
    linked.map(|()| path.clone()).map_err(|source| {
        if source.kind() == std::io::ErrorKind::AlreadyExists {
            InitRefused::AlreadyThere { path }
        } else {
            InitRefused::NotWritten {
                action: "put the manifest in place",
                path,
                source,
            }
        }
    })
}
