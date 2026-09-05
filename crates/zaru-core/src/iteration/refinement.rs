// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Building the next iteration's prompt out of the last one's failure.
//!
//! ADR-0008 D1 makes `Refine` a distinct state rather than a branch back to
//! `Generate`, because the construction *is* the mechanism — a retry repeats
//! an operation hoping for a different outcome, and an iteration changes the
//! next attempt because of the failure it read. D1 also says the construction
//! is testable in isolation, which is why everything here is a pure function
//! of its inputs: no port, no clock, no loop state.
//!
//! ADR-0008 D4 governs the failure text. It is carried verbatim — truncated,
//! never paraphrased — because a prompt saying "the tests failed" produces a
//! model guess rather than a correction. Truncation keeps the head and the
//! tail and marks the elision, since a stack trace's first frames and its
//! final assertion are the load-bearing parts and the middle rarely is.
//!
//! # This is the first of the paths ADR-0008 clause 6's port covers
//!
//! Clause 6 was decided on 2026-09-05 and this module carries its first
//! application. **All four of the prompt's variable-length parts pass the
//! port**, not only the failure text: the two streams of
//! [`ExecutionOutcome`] are the executed candidate's captured output by any
//! reading, and the previous candidate is text a model wrote about them. The
//! identity seam this replaces sat on the failure text alone, which was
//! narrower than the path it was documented as sitting on — a finding of the
//! `redaction-seam` arc, recorded on ADR-0008 and ruled in on 2026-09-05.
//!
//! **Each part is redacted before it is truncated.** Truncating first would
//! cut a held value in half at the elision boundary and leave its head in the
//! prompt as a fragment the redactor no longer recognises. Redacting first
//! means the elision falls in text that has no secret left in it.

use crate::iteration::limits::TruncationBudget;
use crate::iteration::port::ExecutionOutcome;
use crate::redaction::{Redacted, Redactor};

/// What the refinement is built from: ADR-0008 D1's three named inputs, and
/// the iteration they came from.
#[derive(Debug)]
pub struct RefinementInput<'a> {
    /// The iteration that failed, counting from one.
    pub iteration: u32,
    /// The candidate that failed, as text.
    pub previous_candidate: &'a str,
    /// What executing it produced.
    pub execution: &'a ExecutionOutcome,
    /// The failing validators' own output, verbatim and untruncated.
    pub failure_text: &'a str,
}

/// The prompt the next iteration generates from.
///
/// Both fields are [`Redacted`], so this type cannot be built out of raw
/// captured bytes at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementPrompt {
    text: Redacted,
    failure_excerpt: Redacted,
}

impl RefinementPrompt {
    /// The whole prompt.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    /// The failure text as it went into the prompt, after truncation.
    ///
    /// This is what [`Event::RefinementConstructed`] carries, so a consumer
    /// renders the same excerpt the model was given rather than a second
    /// truncation of its own.
    ///
    /// It is **redacted**, and that is deliberate: this event's own contract
    /// is that a consumer renders what the model was given, so an excerpt
    /// that differed from the prompt's would be a second description of one
    /// thing. The raw failure survives on `Event::IterationFailed`, on
    /// `Event::ValidatorEvaluated` and on `Outcome::Exhausted`, which is what
    /// ADR-0010's transcript keeps.
    ///
    /// [`Event::RefinementConstructed`]: crate::iteration::Event::RefinementConstructed
    #[must_use]
    pub fn failure_excerpt(&self) -> &str {
        self.failure_excerpt.as_str()
    }
}

/// Build the next iteration's prompt.
///
/// Every variable-length part is bounded by the same caller-supplied budget,
/// independently: the previous candidate, the execution's two streams, and
/// the failure text. One number bounds each part rather than four numbers
/// nobody has chosen.
///
/// Every one of those four also passes `redactor`, before it is truncated.
/// See the module documentation for why the order matters.
#[must_use]
pub fn construct<R: Redactor + ?Sized>(
    input: &RefinementInput<'_>,
    budget: TruncationBudget,
    redactor: &R,
) -> RefinementPrompt {
    let failure_excerpt = redacted_then_truncated(redactor, input.failure_text, budget);
    let candidate = redacted_then_truncated(redactor, input.previous_candidate, budget);
    let stdout = redacted_then_truncated(redactor, &input.execution.stdout, budget);
    let stderr = redacted_then_truncated(redactor, &input.execution.stderr, budget);
    let (candidate, stdout, stderr) = (candidate.as_str(), stdout.as_str(), stderr.as_str());
    let excerpt = failure_excerpt.as_str();

    let text = format!(
        "The previous attempt did not satisfy the declared validators.\n\
         \n\
         --- ATTEMPT {} ---\n\
         {candidate}\n\
         \n\
         --- EXECUTION ---\n\
         exit code: {}\n\
         stdout:\n\
         {stdout}\n\
         stderr:\n\
         {stderr}\n\
         \n\
         --- VALIDATOR FAILURES ---\n\
         {excerpt}\n",
        input.iteration, input.execution.exit_code,
    );

    RefinementPrompt {
        // The whole is passed again so that what this type carries was
        // produced by the port rather than assembled around it. Redaction is
        // idempotent -- a marker carries no held value -- so the second pass
        // over already-redacted parts changes nothing, which
        // `redaction_is_idempotent_because_a_marker_carries_no_value`
        // asserts.
        text: Redacted::by(redactor, &text),
        failure_excerpt,
    }
}

/// Redact `text`, then keep the head and the tail of what is left.
///
/// **This order is load-bearing.** Truncating first cuts a held value in half
/// at the elision boundary and leaves its head in the prompt as a fragment no
/// redactor recognises; redacting first means the cut falls in text with no
/// secret in it.
fn redacted_then_truncated<R: Redactor + ?Sized>(
    redactor: &R,
    text: &str,
    budget: TruncationBudget,
) -> Redacted {
    let redacted = Redacted::by(redactor, text);
    Redacted::by(redactor, &truncate_marked(redacted.as_str(), budget))
}

/// Keep the head and the tail of `text` and mark what was dropped.
///
/// Text that fits the budget is returned byte-for-byte unchanged, with no
/// marker: an elision marker on text that was not elided would be a lie a
/// reader cannot distinguish from a truncation.
///
/// When it does not fit, the kept head and the kept tail together are at most
/// `budget` bytes and the marker is additional, so the marker can always say
/// how much went. Both cuts fall on character boundaries.
fn truncate_marked(text: &str, budget: TruncationBudget) -> String {
    let budget = budget.get();
    if text.len() <= budget {
        return text.to_owned();
    }

    let head_end = floor_boundary(text, budget.div_ceil(2));
    let tail_start = ceil_boundary(text, text.len() - (budget - budget.div_ceil(2)));
    let elided = tail_start - head_end;

    format!(
        "{}\n[... {elided} bytes elided ...]\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

/// The largest index at or below `at` that is a character boundary.
fn floor_boundary(text: &str, at: usize) -> usize {
    let mut i = at.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The smallest index at or above `at` that is a character boundary.
fn ceil_boundary(text: &str, at: usize) -> usize {
    let mut i = at.min(text.len());
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::{RefinementInput, construct};
    use crate::iteration::limits::TruncationBudget;
    use crate::iteration::port::ExecutionOutcome;
    use crate::redaction::fixtures::{HoldingOne, NothingHeld, ascii_core, staged_secret};

    /// A budget large enough that nothing here is truncated.
    const ROOMY: usize = 4096;

    #[test]
    fn truncation_keeps_head_and_tail_and_marks_the_elision() {
        let failure = format!("HEADHEAD{}TAILTAIL", "MIDDLE".repeat(200));
        let built = construct(
            &RefinementInput {
                iteration: 1,
                previous_candidate: "candidate",
                execution: &execution(),
                failure_text: &failure,
            },
            TruncationBudget::new(32).expect("budget"),
            &NothingHeld,
        );
        let excerpt = built.failure_excerpt();

        assert!(
            excerpt.starts_with("HEADHEAD"),
            "the head of the failure was dropped: {excerpt:?}"
        );
        assert!(
            excerpt.ends_with("TAILTAIL"),
            "the tail of the failure was dropped, which is what a plain truncation does: {excerpt:?}"
        );
        assert!(
            excerpt.contains("bytes elided"),
            "the elision was not marked: {excerpt:?}"
        );
        assert!(
            !excerpt.contains("MIDDLEMIDDLE"),
            "nothing was actually elided: {excerpt:?}"
        );
        assert!(excerpt.len() < failure.len());
    }

    #[test]
    fn text_that_fits_the_budget_is_not_truncated_and_carries_no_marker() {
        for length in [31_usize, 32, 33] {
            let failure = "x".repeat(length);
            let built = construct(
                &RefinementInput {
                    iteration: 1,
                    previous_candidate: "candidate",
                    execution: &execution(),
                    failure_text: &failure,
                },
                TruncationBudget::new(32).expect("budget"),
                &NothingHeld,
            );
            let excerpt = built.failure_excerpt();
            if length <= 32 {
                assert_eq!(
                    excerpt, failure,
                    "{length} bytes fits a 32-byte budget and must be carried unchanged"
                );
            } else {
                assert!(
                    excerpt.contains("bytes elided"),
                    "{length} bytes exceeds a 32-byte budget and must be marked: {excerpt:?}"
                );
            }
        }
    }

    #[test]
    fn the_refinement_prompt_carries_the_candidate_the_output_and_the_failure() {
        let execution = ExecutionOutcome {
            exit_code: 3,
            stdout: "STDOUT-SENTINEL".to_owned(),
            stderr: "STDERR-SENTINEL".to_owned(),
        };
        let built = construct(
            &RefinementInput {
                iteration: 4,
                previous_candidate: "CANDIDATE-SENTINEL",
                execution: &execution,
                failure_text: "FAILURE-SENTINEL",
            },
            TruncationBudget::new(ROOMY).expect("budget"),
            &NothingHeld,
        );

        for sentinel in [
            "CANDIDATE-SENTINEL",
            "STDOUT-SENTINEL",
            "STDERR-SENTINEL",
            "FAILURE-SENTINEL",
        ] {
            assert!(
                built.as_str().contains(sentinel),
                "the refinement prompt is missing {sentinel}; ADR-0008 D1 names the previous \
                 candidate, the execution output and the failure text as its three inputs"
            );
        }
    }

    #[test]
    fn refinement_construction_needs_no_ports_and_no_loop() {
        // ADR-0008 D1: "that construction is the mechanism, so it is testable in
        // isolation". Nothing in this test constructs a port, a clock or a sink.
        let built = construct(
            &RefinementInput {
                iteration: 2,
                previous_candidate: "c",
                execution: &execution(),
                failure_text: "the failure",
            },
            TruncationBudget::new(ROOMY).expect("budget"),
            &NothingHeld,
        );
        assert_eq!(built.failure_excerpt(), "the failure");
        assert!(built.as_str().contains("ATTEMPT 2"));
    }

    // --- ADR-0008 trigger clause 6, decided 2026-09-05 ---------------------

    #[test]
    fn every_variable_length_part_of_the_refinement_prompt_passes_the_port() {
        // The identity seam this replaced sat on the failure text alone,
        // while `construct` also embeds the execution's two streams and the
        // previous candidate. Three of the four bypassed the one place
        // ADR-0008's Status tracking said a redaction decision would attach.
        // All four now pass the port, and this check is what says so.
        let secret = staged_secret();
        let core = ascii_core(&secret);
        assert!(
            !core.is_empty() && core != secret,
            "the staged secret must have an ASCII core distinct from itself"
        );
        let holding = HoldingOne::new(secret.clone(), "work");
        let marker = "<redacted: work>";

        let execution = ExecutionOutcome {
            exit_code: 3,
            stdout: format!("stdout carries {secret} here"),
            stderr: format!("stderr carries {secret} here"),
        };
        let input = RefinementInput {
            iteration: 2,
            previous_candidate: &format!("the candidate quoted {secret} back"),
            execution: &execution,
            failure_text: &format!("the validator printed {secret}"),
        };
        let budget = TruncationBudget::new(ROOMY).expect("budget");

        let redacted = construct(&input, budget, &holding);
        assert!(
            !redacted.as_str().contains(&secret),
            "a held value reached the refinement prompt: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(core),
            "a held value's ASCII core reached the refinement prompt, so an \
             escaping renderer would publish it: {:?}",
            redacted.as_str()
        );
        assert_eq!(
            redacted.as_str().matches(marker).count(),
            4,
            "all four variable-length parts must be redacted -- the failure \
             text, both execution streams, and the previous candidate -- and \
             this prompt carries a different number of markers: {:?}",
            redacted.as_str()
        );

        // The discriminating arm. Without it a `construct` that returned an
        // empty prompt would satisfy both absence assertions above, and the
        // four separate placements would prove nothing about which parts the
        // port actually reached.
        let carried = construct(&input, budget, &NothingHeld);
        assert_eq!(
            carried.as_str().matches(secret.as_str()).count(),
            4,
            "with nothing held, every one of the four parts must carry its \
             bytes through unaltered, or the check above is not about \
             redaction: {:?}",
            carried.as_str()
        );
        assert!(!carried.as_str().contains(marker));
    }

    #[test]
    fn a_held_value_is_redacted_before_it_is_truncated() {
        // Order matters and this is the only check that sees it. Truncating
        // first cuts a held value in half at the elision boundary and leaves
        // its head in the prompt as a fragment no redactor recognises, so the
        // value is published by a code path that ran the port.
        let secret = staged_secret();
        let core = ascii_core(&secret);
        // The budget is chosen so the kept head ends **inside** the ASCII
        // core. A first draft used a budget whose head boundary fell exactly
        // at the end of the core, so the truncate-first mutant left a
        // fragment the redactor still recognised as the core, replaced it,
        // and the check stayed green. The mutation was invisible because of
        // where the fixture's cut happened to land -- Verification lessons
        // §9 arriving from the direction that is easy to miss.
        let budget: usize = 40;
        let head_kept = budget.div_ceil(2);
        assert!(
            head_kept < core.len(),
            "the kept head must end inside the ASCII core, or a truncate-first \
             implementation leaves a fragment the redactor still matches and \
             this check sees nothing: {head_kept} vs {}",
            core.len()
        );
        let head_of_the_core = &core[..head_kept / 2];
        let holding = HoldingOne::new(secret.clone(), "work");

        // The secret leads the failure text, so with the port applied first
        // the marker is what the kept head contains; with truncation first
        // the kept head is the secret's own first bytes.
        let failure = format!("{secret}{}", "B".repeat(300));
        let built = construct(
            &RefinementInput {
                iteration: 1,
                previous_candidate: "candidate",
                execution: &execution(),
                failure_text: &failure,
            },
            TruncationBudget::new(budget).expect("budget"),
            &holding,
        );
        let excerpt = built.failure_excerpt();

        assert!(
            excerpt.contains("bytes elided"),
            "the staging must actually truncate, or this check is about \
             nothing: {excerpt:?}"
        );
        assert!(
            excerpt.contains("<redacted: work>"),
            "the kept head must be the marker, which is what redacting before \
             truncating produces: {excerpt:?}"
        );
        assert!(
            !excerpt.contains(head_of_the_core),
            "the first bytes of a held value survived into the excerpt as a \
             fragment, which is what truncating before redacting leaves \
             behind: {excerpt:?}"
        );
    }

    fn execution() -> ExecutionOutcome {
        ExecutionOutcome {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}
