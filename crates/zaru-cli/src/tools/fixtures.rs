// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Values the tool-surface checks are built from. Compiled only under
//! `cfg(test)`.
//!
//! # Why the nonces are deliberately awkward
//!
//! [Verification lessons] §9: a fixture can be too well-behaved. A check that
//! asserts a refusal quoted back the key it was handed is only as good as the
//! chance that key would have shown up by accident, so every nonce here is
//! unique per call and carries text no implementation would produce on its
//! own.
//!
//! Nothing here is a credential. These are uniqueness devices, which is why
//! `std` alone makes one and no random-number crate is needed.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Distinguishes two nonces taken inside one clock tick.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call to this function will produce.
///
/// `label` is there so a failure names which fixture it came from.
pub(crate) fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock is before the unix epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}", std::process::id())
}

/// A directory tree a check owns, with the shapes ADR-0011 D4 has to
/// separate, removed when the check ends.
///
/// [Testing]'s rule: "Each test owns its own state... its own configuration
/// directory, its own session store, and its own working directory, writing
/// to the paths the product actually writes to inside that root."
/// [`WorkingDirectory`](crate::tools::tree::WorkingDirectory) takes its root
/// as a parameter because the product needs it to, so a check here is an
/// ordinary caller rather than a fake.
///
/// # Why the shapes are these shapes
///
/// [Verification lessons] §9: a fixture can be too well-behaved. A tree whose
/// only out-of-tree case is `/etc/passwd` is satisfied by an implementation
/// that checks for a leading slash, and a tree with no sibling whose name
/// extends the root's is satisfied by a string prefix test. So the tree
/// carries, deliberately:
///
/// - `project/`, the working directory, reached through a **symlink** so that
///   a root that is not canonicalised at construction is visible;
/// - `projectevil/`, a sibling whose name extends the root's — the case a
///   byte-wise prefix test gets wrong and a component-wise one gets right;
/// - `elsewhere/`, an ordinary out-of-tree directory;
/// - `project/escape`, a **symlink out of the tree** wearing an ordinary
///   name;
/// - `project/inside/`, so that every hostile case has an in-tree sibling the
///   same rule must accept. A rule that refuses everything is not a boundary.
///
/// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub(crate) struct ScratchTree {
    base: std::path::PathBuf,
    sentinel: String,
}

impl ScratchTree {
    /// Build the tree. Panics rather than returning: a check whose staging
    /// failed must refuse rather than skip ([Verification lessons] §4).
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    pub(crate) fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("ts-tree"));
        let tree = Self {
            base,
            sentinel: nonce("only-outside-the-tree"),
        };
        std::fs::create_dir_all(tree.project().join("inside")).expect("staging: project/inside");
        std::fs::create_dir_all(tree.base.join("projectevil")).expect("staging: projectevil");
        std::fs::create_dir_all(tree.base.join("elsewhere")).expect("staging: elsewhere");
        std::fs::write(tree.project().join("inside").join("file"), b"in")
            .expect("staging: project/inside/file");
        // The out-of-tree files carry a value that exists nowhere else, so a
        // check can assert that nothing which escaped the boundary reached a
        // caller -- an assertion about the bytes rather than about the path,
        // which is the only kind that survives a classifier that is right and
        // an executor that reads the wrong thing anyway.
        std::fs::write(
            tree.base.join("elsewhere").join("secret"),
            tree.sentinel.as_bytes(),
        )
        .expect("staging: elsewhere/secret");
        std::fs::write(
            tree.base.join("projectevil").join("loot"),
            tree.sentinel.as_bytes(),
        )
        .expect("staging: projectevil/loot");
        std::os::unix::fs::symlink(tree.base.join("elsewhere"), tree.project().join("escape"))
            .expect("staging: the escaping symlink");
        std::os::unix::fs::symlink(tree.project(), tree.base.join("by-link"))
            .expect("staging: the symlinked route to the root");
        tree
    }

    /// The working directory itself.
    pub(crate) fn project(&self) -> std::path::PathBuf {
        self.base.join("project")
    }

    /// A symlink whose target is the working directory.
    pub(crate) fn project_by_link(&self) -> std::path::PathBuf {
        self.base.join("by-link")
    }

    /// The directory everything else sits in.
    pub(crate) fn base(&self) -> &std::path::Path {
        &self.base
    }

    /// The value written into every out-of-tree file and nowhere else.
    ///
    /// A check that reads it back has read something the boundary should
    /// have stopped, whatever the classification said.
    pub(crate) fn sentinel(&self) -> &str {
        &self.sentinel
    }
}

impl Drop for ScratchTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// An allowlist that answers as it was built to, and records what it was
/// asked.
///
/// **A test double, and not evidence about ADR-0011 D3's allowlist**, which
/// has no format and no implementation — [Verification lessons] §24: "A test
/// double answering more simply than the real thing is where a defect becomes
/// invisible." What a check may conclude from this is what the *rule* does
/// with an answer, and nothing about how an answer would be arrived at.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub(crate) struct StagedAllowlist {
    answer: bool,
    asked: std::cell::RefCell<Vec<String>>,
}

impl StagedAllowlist {
    /// An allowlist that approves everything it is asked about.
    pub(crate) fn approving() -> Self {
        Self {
            answer: true,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// An allowlist that approves nothing.
    pub(crate) fn empty() -> Self {
        Self {
            answer: false,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Every call this allowlist was asked about, as the rule described it.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked.borrow().clone()
    }
}

impl crate::tools::port::Allowlist for StagedAllowlist {
    fn approves(&self, invocation: &crate::tools::decision::Invocation<'_>) -> bool {
        self.asked.borrow_mut().push(format!(
            "{} {}",
            invocation.tool(),
            invocation.subject_text()
        ));
        self.answer
    }
}

/// A destructive-pattern matcher that answers as it was built to.
///
/// **Not evidence about ADR-0011 D6's pattern list**, which is deliberately
/// unwritten: naming the patterns is authoring a security vocabulary. What a
/// check may conclude is what the rule does with a match.
pub(crate) struct StagedDestructive {
    answer: bool,
}

impl StagedDestructive {
    /// Matches everything.
    pub(crate) const fn matching() -> Self {
        Self { answer: true }
    }

    /// Matches nothing.
    pub(crate) const fn quiet() -> Self {
        Self { answer: false }
    }
}

impl crate::tools::port::DestructiveMatch for StagedDestructive {
    fn is_destructive(&self, _invocation: &crate::tools::decision::Invocation<'_>) -> bool {
        self.answer
    }
}

/// A confirmer that answers as it was built to, and records every question.
pub(crate) struct RecordedConfirmer {
    answer: bool,
    asked: std::cell::RefCell<Vec<crate::tools::port::Question>>,
}

impl RecordedConfirmer {
    /// A user who says yes.
    pub(crate) fn accepting() -> Self {
        Self {
            answer: true,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// A user who says no.
    pub(crate) fn declining() -> Self {
        Self {
            answer: false,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Every question this confirmer was asked.
    pub(crate) fn asked(&self) -> Vec<crate::tools::port::Question> {
        self.asked.borrow().clone()
    }
}

impl crate::tools::port::Confirm for RecordedConfirmer {
    fn confirm(&self, question: &crate::tools::port::Question) -> bool {
        self.asked.borrow_mut().push(question.clone());
        self.answer
    }
}

/// Where an oversized capture goes when there is no session directory.
///
/// **A test double, and not evidence about ADR-0011 D5's session directory**,
/// which is ADR-0010 D1's and is unbuilt. It writes into a scratch tree the
/// check owns, using the paths the product would use, so a check here is an
/// ordinary caller of the port rather than a fake of the mechanism.
pub(crate) struct ScratchOverflow {
    directory: std::path::PathBuf,
    written: std::cell::RefCell<Vec<std::path::PathBuf>>,
}

impl ScratchOverflow {
    /// A sink writing into a directory the check owns.
    pub(crate) fn in_directory(directory: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(&directory).expect("staging: the overflow directory");
        Self {
            directory,
            written: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Every path this sink reported.
    pub(crate) fn written(&self) -> Vec<std::path::PathBuf> {
        self.written.borrow().clone()
    }
}

impl crate::tools::output::Overflow for ScratchOverflow {
    fn preserve(
        &mut self,
        captured: &crate::tools::output::Captured,
    ) -> Result<std::path::PathBuf, crate::tools::output::OverflowFailure> {
        let path = self.directory.join(nonce("overflow"));
        let body = format!(
            "exit {}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
            captured.exit_code, captured.stdout, captured.stderr
        );
        std::fs::write(&path, body).map_err(|source| {
            crate::tools::output::OverflowFailure::new(format!(
                "could not write {}: {source}",
                path.display()
            ))
        })?;
        self.written.borrow_mut().push(path.clone());
        Ok(path)
    }
}

/// A sink that always refuses, so the failing arm has something to fail on.
pub(crate) struct RefusingOverflow;

impl crate::tools::output::Overflow for RefusingOverflow {
    fn preserve(
        &mut self,
        _captured: &crate::tools::output::Captured,
    ) -> Result<std::path::PathBuf, crate::tools::output::OverflowFailure> {
        Err(crate::tools::output::OverflowFailure::new(
            "this sink preserves nothing",
        ))
    }
}
