// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside this crate drives ADR-0011's tool surface through its own
//! public door.
//!
//! # What this establishes, and what it does not
//!
//! [Verification lessons] §25: "For any capability a user interacts with, one
//! check drives the interaction end to end and reads the outcome... Mutation
//! testing cannot find this — it operates on the assertions that exist, and
//! there is no mutant for a call that was never written." The unit checks
//! reach the surface's internals; this one reaches only what `zaru-cli`
//! exports, so a type or method that was never made public fails here and
//! nowhere else. All three of the permission model's ports are implemented
//! out here, which is itself part of what it establishes: a trait that could
//! only be implemented from inside would not be a seam.
//!
//! **It is not evidence about the `zaru` binary.** No binary reaches the tool
//! surface. ADR-0011's tools are called by the tool-call loop, which
//! [ADR-0008] D1 names and which is unbuilt; the prompt is rendered by
//! `zaru-tui`, which renders nothing yet; the not-a-sandbox line needs
//! [ADR-0010]'s session; and the mode comes from [ADR-0014]'s configuration
//! hierarchy, which is unbuilt too. What this check prints is evidence about
//! the mechanism and must not be quoted as evidence about the binary, which
//! still prints its version and its composition and reaches none of this.
//!
//! **Nothing here executes anything.** No subprocess, no network, no file is
//! read or written by any tool — the only writes are into a scratch tree this
//! check owns and removes.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zaru_cli::redaction::HeldSecrets;
use zaru_cli::tools::grants::SessionGrants;
use zaru_cli::tools::port::Answer;
use zaru_cli::tools::{
    Allowlist, Assessment, Captured, Confirm, ConfirmFailure, Decision, DestructiveMatch,
    Invocation, Layer, Mode, ModeRefused, Overflow, OverflowFailure, Permission, Question,
    RefusedBecause, Requirement, SessionNotice, ToolName, WorkingDirectory,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call produces. Written here rather than imported: the
/// crate's own fixtures are private, and a check about the public door that
/// borrowed the crate's internals would be reaching around the door.
fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}", std::process::id())
}

/// ADR-0011 D3's allowlist, implemented outside the crate that declares it.
///
/// It approves nothing, which is the state a user starts in.
struct ApprovesNothing;

impl Allowlist for ApprovesNothing {
    fn approves(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}

/// ADR-0011 D6's matcher, implemented outside the crate that declares it.
///
/// It matches nothing. **Writing patterns here would be authoring a security
/// vocabulary**, which is on the human side of the boundary; D6 names four
/// categories and this check does not turn one into a matcher.
struct MatchesNothing;

impl DestructiveMatch for MatchesNothing {
    fn is_destructive(&self, _invocation: &Invocation<'_>) -> bool {
        false
    }
}

/// A user who answers, and remembers what they were asked.
struct AnsweringUser {
    answer: Answer,
    asked: RefCell<Vec<String>>,
}

impl AnsweringUser {
    fn saying(answer: Answer) -> Self {
        Self {
            answer,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Confirm for AnsweringUser {
    fn confirm(&self, question: &Question) -> Result<Answer, ConfirmFailure> {
        self.asked.borrow_mut().push(question.statement.clone());
        Ok(self.answer)
    }
}

/// ADR-0011 D5's overflow sink, implemented outside the crate that declares
/// it. Not evidence about ADR-0010's session directory, which does not exist.
struct IntoScratch {
    directory: PathBuf,
}

impl Overflow for IntoScratch {
    fn preserve(&mut self, captured: &Captured) -> Result<PathBuf, OverflowFailure> {
        let path = self.directory.join(nonce("overflow"));
        std::fs::write(&path, &captured.stdout).map_err(|source| {
            OverflowFailure::new(format!("could not write {}: {source}", path.display()))
        })?;
        Ok(path)
    }
}

#[test]
fn a_caller_outside_this_crate_can_classify_decide_and_be_refused() {
    let base = std::fs::canonicalize(std::env::temp_dir())
        .expect("the temporary directory resolves")
        .join(nonce("ts-outside"));
    let project = base.join("project");
    std::fs::create_dir_all(project.join("src")).expect("staging: the project");
    std::fs::create_dir_all(base.join("elsewhere")).expect("staging: a directory outside it");
    std::fs::write(base.join("elsewhere").join("secret"), b"out").expect("staging: a file in it");

    let working = WorkingDirectory::at(&project).expect("the project directory resolves");
    let inside = working.classify("src/main.rs");
    let outside = working.classify("../elsewhere/secret");

    println!("--- ADR-0011 D4: what the working directory boundary says ---");
    println!("working directory: {}", working.root().display());
    for (spelling, target) in [("src/main.rs", &inside), ("../elsewhere/secret", &outside)] {
        println!(
            "  {spelling:<24} -> {:<48} {:?}",
            target.resolved().display(),
            target.placement()
        );
    }

    // ADR-0011 D2: the bare tier says once, at session start, that it is not a
    // sandbox. The sentence is the caller's; this one is a placeholder and is
    // not proposed wording.
    let mut notice = SessionNotice::new(nonce("a caller's sentence"));
    println!("--- ADR-0011 D2: stated once at session start ---");
    println!("  {:?}", notice.state_once());
    println!("  {:?}   (and never again)", notice.state_once());

    // ADR-0011 D3 and D4, at every mode, over one call outside the tree.
    // Nothing granted: a session starts with no grant and a resumed one does
    // too, because D3's third answer is never persisted.
    let no_grants = SessionGrants::none();
    let escaping =
        Invocation::on_path(ToolName::FsRead, &outside).expect("fs.read addresses a path");
    println!("--- ADR-0011 D3 and D4: one out-of-tree read, at every mode ---");
    let mut records = Vec::new();
    for mode in Mode::ALL {
        let decision = Decision::assess(
            mode,
            &escaping,
            &ApprovesNothing,
            &MatchesNothing,
            &no_grants,
        );
        println!(
            "  {mode:<6} requires {:<8?} record: {}",
            decision.requirement(),
            decision.entry().render()
        );
        records.push(decision.entry().render());
    }

    assert!(
        records.iter().all(|line| *line == records[0]),
        "ADR-0011 D4: \"Mode may remove the prompt; it never removes the record.\" The record \
         differed by mode: {records:?}"
    );
    assert!(
        records[0].contains("OUTSIDE the working directory"),
        "the record does not show that the call left the working directory: {:?}",
        records[0]
    );

    // The refusals, from outside.
    let at_ask = Decision::assess(
        Mode::Ask,
        &escaping,
        &ApprovesNothing,
        &MatchesNothing,
        &no_grants,
    );
    assert_eq!(at_ask.requirement(), Requirement::Ask);
    assert_eq!(
        at_ask.permit(None),
        Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
        "a call needing the user, with nobody to ask, must be refused rather than performed"
    );

    let declining = AnsweringUser::saying(Answer::No);
    assert_eq!(
        at_ask.permit(Some(&declining)),
        Permission::Refused(RefusedBecause::TheUserDeclined)
    );
    let accepting = AnsweringUser::saying(Answer::Once);
    assert_eq!(at_ask.permit(Some(&accepting)), Permission::Granted);

    println!("--- what the user was actually asked ---");
    for statement in declining.asked.borrow().iter() {
        println!("  {statement}");
    }
    assert_eq!(
        declining.asked.borrow().len(),
        1,
        "the user was not asked at all"
    );

    // ADR-0014 D6, from outside: a cloned repository cannot set the mode and
    // the user can.
    let key = nonce("permission-key");
    let refusal = Mode::from_layer(Layer::Project, &key, "yolo")
        .expect_err("a cloned repository must not set the permission mode");
    assert!(matches!(refusal, ModeRefused::FromAClonedRepository { .. }));
    println!("--- ADR-0014 D6, from outside the crate ---");
    println!("  {refusal}");
    assert_eq!(
        Mode::from_layer(Layer::User, &key, "yolo"),
        Ok(Mode::Yolo),
        "the user's own configuration must be able to set the mode, or the refusal above is \
         \"refuse everything\""
    );

    // ADR-0011 D5, from outside: truncation, the marker, and the path shown.
    let overflow_directory = base.join("overflow");
    std::fs::create_dir_all(&overflow_directory).expect("staging: the overflow directory");
    let mut sink = IntoScratch {
        directory: overflow_directory,
    };
    let captured = Captured {
        exit_code: 2,
        stdout: format!("HEAD{}TAIL", "x".repeat(500)),
        stderr: nonce("stderr"),
    };
    let budget = zaru_cli::tools::OutputBudget::new(24).expect("a non-zero budget");
    let shown = captured
        .present(budget, &HeldSecrets::none(), Some(&mut sink))
        .expect("a sink was supplied");
    println!("--- ADR-0011 D5: what a caller is shown of 508 bytes at a 24-byte budget ---");
    println!("  exit {}", shown.exit_code);
    println!("  stdout: {:?}", shown.stdout.as_str());
    println!("  stderr: {:?}", shown.stderr.as_str());
    println!("  full text at: {:?}", shown.full_text_at);
    assert!(shown.stdout.as_str().starts_with("HEAD"));
    assert!(shown.stdout.as_str().ends_with("TAIL"));
    assert!(shown.stdout.was_truncated());
    let preserved = shown
        .full_text_at
        .clone()
        .expect("D5 requires the path be shown when anything was elided");
    assert_eq!(
        std::fs::read_to_string(&preserved).expect("the sink wrote what it named"),
        captured.stdout,
        "the preserved file does not carry the whole output"
    );
    assert!(
        captured
            .present(budget, &HeldSecrets::none(), None)
            .is_err(),
        "output that overflows with nowhere to keep it must be refused, not clipped"
    );

    // The pure rule is reachable from outside with no port at all.
    let ordinary = Invocation::on_path(ToolName::FsRead, &inside).expect("addresses a path");
    assert_eq!(
        Decision::reach(Mode::Ask, &ordinary, Assessment::default()).requirement(),
        Requirement::Proceed,
        "an ordinary in-tree read at `ask` is neither a write nor a command and does not prompt"
    );

    // The scratch tree goes, and a control beside it stays.
    let control = base.join("control");
    std::fs::create_dir_all(&control).expect("the control is creatable");
    std::fs::remove_dir_all(&project).expect("the project is removable");
    assert!(!project.exists(), "the project survived removal");
    assert!(control.exists(), "the control was removed too");
    std::fs::remove_dir_all(&base).expect("the scratch tree is removable");
    assert!(!base.exists(), "the scratch tree survived removal");
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
