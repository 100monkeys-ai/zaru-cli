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
/// has had a product implementation since 2026-09-05 —
/// [`Allowed`](crate::tools::Allowed) — and is checked as itself.
/// [Verification lessons] §24: "A test double answering more simply than the
/// real thing is where a defect becomes invisible." What a check may conclude
/// from this is what the *rule* does with an answer, and nothing about how an
/// answer would be arrived at; a check about the answer drives `Allowed`.
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
            invocation.called(),
            invocation.subject_text()
        ));
        self.answer
    }
}

/// A destructive-pattern matcher that answers as it was built to.
///
/// **Not evidence about ADR-0011 D6's categories**, which have had a product
/// matcher since 2026-09-05 — [`Shapes`](crate::tools::Shapes) — and are
/// checked as themselves. What a check may conclude from this double is what
/// the rule does with a match, and nothing about which commands match; two of
/// D6's four categories still match nothing, because naming a program for
/// them is authoring a security vocabulary.
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
///
/// **Not evidence about [`Prompt`](crate::tools::prompt::Prompt)**, which
/// asks over a terminal and cannot be driven by a check at all. What a check
/// concludes from this is what [`Decision::permit`](crate::tools::Decision)
/// does with each of the three answers a confirmer can give.
pub(crate) struct RecordedConfirmer {
    answer: crate::tools::port::Answer,
    asked: std::cell::RefCell<Vec<crate::tools::port::Question>>,
}

impl RecordedConfirmer {
    /// A user who says yes to this call and nothing about any other.
    pub(crate) fn accepting() -> Self {
        Self::saying(crate::tools::port::Answer::Once)
    }

    /// A user who says no.
    pub(crate) fn declining() -> Self {
        Self::saying(crate::tools::port::Answer::No)
    }

    /// A user who allows this exact line for the rest of the session.
    pub(crate) fn allowing_for_the_session() -> Self {
        Self::saying(crate::tools::port::Answer::ForThisSession)
    }

    /// A user who answers whatever the check says.
    pub(crate) fn saying(answer: crate::tools::port::Answer) -> Self {
        Self {
            answer,
            asked: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Every question this confirmer was asked.
    pub(crate) fn asked(&self) -> Vec<crate::tools::port::Question> {
        self.asked.borrow().clone()
    }
}

impl crate::tools::port::Confirm for RecordedConfirmer {
    fn confirm(
        &self,
        question: &crate::tools::port::Question,
    ) -> Result<crate::tools::port::Answer, crate::tools::port::ConfirmFailure> {
        self.asked.borrow_mut().push(question.clone());
        Ok(self.answer)
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

/// A confirmer whose ask fails, for the arm that is not a decline.
///
/// **Not a user who said no.** ADR-0011 D3's prompt reaches a person or it
/// does not, and this stands for the second — a terminal that closed between
/// the statement and the answer. It records that it *was* asked, so a check
/// can tell a refusal that skipped the question from one that put it and
/// could not hear back.
pub(crate) struct FailingConfirmer {
    asked: std::cell::Cell<usize>,
}

impl FailingConfirmer {
    /// A confirmer that cannot reach anybody.
    pub(crate) const fn new() -> Self {
        Self {
            asked: std::cell::Cell::new(0),
        }
    }

    /// How many times it was asked.
    pub(crate) fn asked(&self) -> usize {
        self.asked.get()
    }
}

impl crate::tools::port::Confirm for FailingConfirmer {
    fn confirm(
        &self,
        _question: &crate::tools::port::Question,
    ) -> Result<crate::tools::port::Answer, crate::tools::port::ConfirmFailure> {
        self.asked.set(self.asked.get() + 1);
        Err(crate::tools::port::ConfirmFailure::new(
            "the terminal closed between the statement and the answer",
        ))
    }
}

/// A [`Projected`](crate::tools::Projected) that answers without a network.
///
/// **Not a stand-in for Nuclear Notes.** It records what it was asked for and
/// hands back a fixed capture, so a check can assert that a permitted
/// projected call reached the port with the alias, the tool and the arguments
/// the decision was reached about — and nothing about what a real instance
/// would say. The offline half of that lives against a loopback instance; see
/// `tests/notes_projection_from_outside.rs`.
pub(crate) struct StagedProjection {
    asked: std::sync::Mutex<Vec<String>>,
    answer: crate::tools::output::Captured,
}

impl StagedProjection {
    /// A projection that answers `answer` to everything.
    pub(crate) fn answering(answer: &str) -> Self {
        Self {
            asked: std::sync::Mutex::new(Vec::new()),
            answer: crate::tools::output::Captured {
                exit_code: 0,
                stdout: answer.to_owned(),
                stderr: String::new(),
            },
        }
    }

    /// A projection that answers as an instance refusing does.
    pub(crate) fn refusing(detail: &str) -> Self {
        Self {
            asked: std::sync::Mutex::new(Vec::new()),
            answer: crate::tools::output::Captured {
                exit_code: 1,
                stdout: String::new(),
                stderr: detail.to_owned(),
            },
        }
    }

    /// Every call it was asked to make, in order.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .expect("the staged projection's lock is not poisoned")
            .clone()
    }
}

impl crate::tools::Projected for StagedProjection {
    async fn call(
        &self,
        alias: &crate::credentials::Alias,
        tool: &str,
        arguments: &str,
    ) -> Result<crate::tools::output::Captured, zaru_core::iteration::PortFailure> {
        self.asked
            .lock()
            .expect("the staged projection's lock is not poisoned")
            .push(format!("{alias} {tool} {arguments}"));
        Ok(self.answer.clone())
    }
}

/// A [`Projected`](crate::tools::Projected) nothing may call.
///
/// Distinct from the product's [`NoProjection`](crate::tools::NoProjection),
/// which *refuses*: being told no is an answer a model may legitimately get,
/// so a check whose subject is a built-in wants something that says the
/// harness reached a server it had no business reaching.
///
/// The shape `NoMembrane` already has: a check whose subject is a built-in
/// should not be able to reach a projected server at all, and a port that
/// panics says so louder than one that returns an empty capture.
pub(crate) struct UnreachableProjection;

impl crate::tools::Projected for UnreachableProjection {
    async fn call(
        &self,
        alias: &crate::credentials::Alias,
        tool: &str,
        _arguments: &str,
    ) -> Result<crate::tools::output::Captured, zaru_core::iteration::PortFailure> {
        panic!("nothing in this check should reach a projected server, and `{alias}` `{tool}` did")
    }
}
