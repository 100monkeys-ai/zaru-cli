// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0011 D4's boundary: what is below the working directory, and what is
//! not.
//!
//! D4: "Reads and writes below the working directory are ordinary. Anything
//! above it, anywhere in `$HOME` outside the project, or anywhere absolute is
//! a distinct class: it prompts in `ask` and `allow`, and it renders
//! differently in the transcript at every mode including `yolo`."
//!
//! # Three phrasings, one invariant
//!
//! D4 names three shapes — above the working directory, inside `$HOME` but
//! outside the project, absolute — and calls them **one** distinct class. The
//! invariant all three share, and the one this module holds, is: *the
//! resolved target is not below the working directory*. `$HOME` needs no
//! special case, because a path in `$HOME` outside the project is already not
//! below the working directory; "absolute" needs none either, because an
//! absolute path below the working directory is ordinary and an absolute path
//! anywhere else is already caught. Special-casing any of the three would be
//! three rules that can disagree with each other.
//!
//! This reading is a delegated coordinator ruling of 2026-09-04 and is
//! recorded as a proposed Update on ADR-0011, because "the harness's
//! definition of D4's three phrasings" is a security rule and not an
//! implementation detail.
//!
//! # How a path is resolved, and why not lexically
//!
//! Every containment defect in every harness lives in this function, so the
//! rule is stated once and in full:
//!
//! 1. The working directory is **canonicalised once, at construction**. A
//!    root that is itself a symlink would otherwise make every later
//!    comparison compare two different spellings of one directory.
//! 2. A relative candidate is joined onto that root; an absolute one is taken
//!    as it is.
//! 3. The candidate's **longest existing ancestor is canonicalised**, which
//!    follows every symlink on the part of the path that exists, and the
//!    remainder is appended **lexically normalised** — `.` dropped, `..`
//!    popping a component, and `..` at the root staying at the root.
//! 4. It is in-tree **if and only if the canonical working directory is a
//!    whole-component prefix** of the result.
//!
//! Step 3 is where the two interesting escapes die. A purely lexical
//! normalisation would let a symlink out of the tree through a name that
//! looks ordinary; a pure `canonicalize` would fail outright on a path that
//! does not exist yet, which is every `fs.write` that creates a file, and a
//! harness that cannot classify a path it is about to create classifies
//! nothing. Resolving through the longest *existing* ancestor gets both: the
//! symlink is followed, and `..` cannot escape through a segment that does
//! not exist.
//!
//! Step 4 is where the third one dies. See `is_within` below.
//!
//! # Unix
//!
//! Path semantics here are Unix's, as [`super::super::credentials::store`]'s
//! file modes are. The crate already refuses to build off Unix for that
//! reason and the ADR backlog's "Windows support strategy" row is where the
//! question belongs.

use core::fmt;
use std::path::{Component, Path, PathBuf};

/// The working directory could not be established.
#[derive(Debug)]
pub enum TreeError {
    /// The working directory could not be canonicalised.
    ///
    /// Refused rather than fallen back on, because a boundary whose root is a
    /// guess is not a boundary. ADR-0011 D4 is the only thing standing
    /// between a model-driven action and the rest of the disk at `bare` tier.
    NoSuchWorkingDirectory {
        /// The path that was offered as the working directory.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl TreeError {
    /// The process has no working directory this harness can name.
    ///
    /// A separate constructor rather than a second variant: what failed is the
    /// same thing — the directory every path is measured against could not be
    /// established — and the path is the one the operating system would have
    /// given.
    #[must_use]
    pub fn from_current_directory(source: std::io::Error) -> Self {
        Self::NoSuchWorkingDirectory {
            path: PathBuf::from("."),
            source,
        }
    }
}

impl fmt::Display for TreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSuchWorkingDirectory { path, source } => write!(
                f,
                "the working directory {} could not be resolved: {source}. ADR-0011 D4 makes it \
                 the boundary every tool call is classified against, and a boundary whose root \
                 is a guess is not one",
                path.display()
            ),
        }
    }
}

impl std::error::Error for TreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoSuchWorkingDirectory { source, .. } => Some(source),
        }
    }
}

/// Which of ADR-0011 D4's two classes a target falls in.
///
/// Two variants and not four. D4 names three shapes and calls them one class;
/// see the module documentation for why holding them as one rule rather than
/// three is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Below the working directory. D4: "ordinary".
    InTree,
    /// Not below the working directory. D4's "distinct class".
    OutOfTree,
}

impl Placement {
    /// Whether ADR-0011 D4 makes this placement prompt in `ask` and `allow`
    /// whatever the tool's effect is.
    #[must_use]
    pub const fn is_out_of_tree(self) -> bool {
        matches!(self, Self::OutOfTree)
    }

    /// How this placement is named wherever a target is shown.
    ///
    /// One rendering, so that the prompt and the transcript entry cannot
    /// disagree about whether a target left the tree — the same reason
    /// ADR-0007's apex marking is a single constant read by all three of its
    /// surfaces.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InTree => "in the working directory",
            Self::OutOfTree => "OUTSIDE the working directory",
        }
    }
}

/// Where a tool call's target actually is, and which class it falls in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    resolved: PathBuf,
    placement: Placement,
}

impl Target {
    /// The absolute path the candidate resolved to, symlinks followed.
    ///
    /// This is what a transcript entry shows, rather than the spelling the
    /// caller passed: a target rendered as `../../etc/passwd` tells a reader
    /// less than one rendered as the path it actually reached.
    #[must_use]
    pub fn resolved(&self) -> &Path {
        &self.resolved
    }

    /// Which of D4's two classes it falls in.
    #[must_use]
    pub const fn placement(&self) -> Placement {
        self.placement
    }
}

/// The directory ADR-0011 D4 measures every path against.
///
/// Canonical from construction, so nothing downstream compares two spellings
/// of one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingDirectory {
    root: PathBuf,
}

impl WorkingDirectory {
    /// Take a working directory, canonicalising it once.
    ///
    /// # Errors
    ///
    /// [`TreeError::NoSuchWorkingDirectory`] when the path cannot be
    /// canonicalised — it does not exist, or is not reachable.
    pub fn at(path: impl AsRef<Path>) -> Result<Self, TreeError> {
        let path = path.as_ref();
        let root =
            std::fs::canonicalize(path).map_err(|source| TreeError::NoSuchWorkingDirectory {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self { root })
    }

    /// This process's own working directory, canonicalised once.
    ///
    /// # One function, so "the working directory" has one answer
    ///
    /// Four places want it: [ADR-0011] D4's boundary in
    /// `compose::turn::prepare`, [ADR-0009] D6's `zaru init`, and the two
    /// entry points of [ADR-0010] D4's `--continue`. Each spelled
    /// `std::env::current_dir()` followed by [`WorkingDirectory::at`], and
    /// four spellings of one rule are four chances for one of them to skip
    /// the canonicalisation — at which point a session started through a
    /// symbolic link records one path in `meta.toml` and is looked for under
    /// another, with nothing saying why. `corpus_one_thing_decides_a_working_directory`
    /// asserts that this is the only construction in the product tree.
    ///
    /// # Errors
    ///
    /// [`TreeError::NoSuchWorkingDirectory`] when the process has no working
    /// directory, or it cannot be canonicalised.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn of_this_process() -> Result<Self, TreeError> {
        std::env::current_dir()
            .map_err(TreeError::from_current_directory)
            .and_then(Self::at)
    }

    /// The canonical root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a candidate path and say which of D4's classes it falls in.
    ///
    /// Infallible on purpose. A path that does not exist is ordinary — every
    /// `fs.write` that creates a file passes one — and a classifier that
    /// errored on it would push the decision onto whichever caller was least
    /// equipped to make it.
    #[must_use]
    pub fn classify(&self, candidate: impl AsRef<Path>) -> Target {
        let candidate = candidate.as_ref();
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.root.join(candidate)
        };
        let resolved = resolve_through_longest_existing_ancestor(&absolute);
        let placement = if is_within(&self.root, &resolved) {
            Placement::InTree
        } else {
            Placement::OutOfTree
        };
        Target {
            resolved,
            placement,
        }
    }
}

/// Whether `candidate` is `root` or sits below it, by whole components.
///
/// **`Path::starts_with` compares components, not bytes, and that is the
/// whole of this function.** The defect it exists to name is the string
/// form — `candidate.to_string_lossy().starts_with(root)` — which reports a
/// sibling directory called `projectevil` as being inside `project`, because
/// one string is a prefix of the other. Writing it as a named function means
/// the mutation that reintroduces that bug is a one-line edit in a place a
/// check points at, rather than an inlined expression nobody re-reads.
fn is_within(root: &Path, candidate: &Path) -> bool {
    candidate.starts_with(root)
}

/// Resolve `absolute` by canonicalising as much of it as exists.
///
/// The remainder is appended with lexical normalisation, so `..` cannot walk
/// out of the tree through a segment that does not exist and therefore cannot
/// be canonicalised.
fn resolve_through_longest_existing_ancestor(absolute: &Path) -> PathBuf {
    let mut anchor = absolute;
    loop {
        if let Ok(real) = std::fs::canonicalize(anchor) {
            let remainder = absolute
                .strip_prefix(anchor)
                .expect("the anchor was derived from this path by taking parents");
            return append_normalised(real, remainder);
        }
        match anchor.parent() {
            Some(parent) => anchor = parent,
            // Unreachable on Unix, where `/` always canonicalises; handled
            // rather than panicked on, because a classifier that panics on a
            // path a model chose is a denial of service with extra steps.
            None => return append_normalised(PathBuf::new(), absolute),
        }
    }
}

/// Append `remainder` to `base`, dropping `.` and popping on `..`.
///
/// `..` at the root stays at the root: `PathBuf::pop` returns false there and
/// leaves the path alone, which is the same thing the kernel does.
fn append_normalised(base: PathBuf, remainder: &Path) -> PathBuf {
    let mut out = base;
    for component in remainder.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            // A path yielded by `strip_prefix` of an ancestor carries no root
            // or prefix component; the fallback arm above passes an absolute
            // path, whose root must be kept.
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
        }
    }
    out
}
