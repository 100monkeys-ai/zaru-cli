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

use crate::iteration::limits::TruncationBudget;
use crate::iteration::port::ExecutionOutcome;

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefinementPrompt {
    text: String,
    failure_excerpt: String,
}

impl RefinementPrompt {
    /// The whole prompt.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The failure text as it went into the prompt, after truncation.
    ///
    /// This is what [`Event::RefinementConstructed`] carries, so a consumer
    /// renders the same excerpt the model was given rather than a second
    /// truncation of its own.
    ///
    /// [`Event::RefinementConstructed`]: crate::iteration::Event::RefinementConstructed
    #[must_use]
    pub fn failure_excerpt(&self) -> &str {
        &self.failure_excerpt
    }
}

/// The single point on the path from a validator's captured output to the
/// refinement prompt.
///
/// It is the identity and it does nothing. ADR-0008's trigger clause 6 — that
/// a decision must exist for secret redaction in failure text — is
/// deliberately open, and this function exists so that the decision, when it
/// is made, has exactly one place to attach. It is not a hook and takes no
/// policy: nothing may pass behaviour through it, because a configurable
/// redaction point would be the decision itself, settled in code.
///
/// ADR-0008's Consequences already name the obligation: verbatim failure text
/// can carry secrets from a failing command into a model prompt.
const fn failure_text_for_prompt(raw: &str) -> &str {
    raw
}

/// Build the next iteration's prompt.
///
/// Every variable-length part is bounded by the same caller-supplied budget,
/// independently: the previous candidate, the execution's two streams, and
/// the failure text. One number bounds each part rather than four numbers
/// nobody has chosen.
#[must_use]
pub fn construct(input: &RefinementInput<'_>, budget: TruncationBudget) -> RefinementPrompt {
    let failure_excerpt = truncate_marked(failure_text_for_prompt(input.failure_text), budget);
    let candidate = truncate_marked(input.previous_candidate, budget);
    let stdout = truncate_marked(&input.execution.stdout, budget);
    let stderr = truncate_marked(&input.execution.stderr, budget);

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
         {failure_excerpt}\n",
        input.iteration, input.execution.exit_code,
    );

    RefinementPrompt {
        text,
        failure_excerpt,
    }
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
        );
        assert_eq!(built.failure_excerpt(), "the failure");
        assert!(built.as_str().contains("ATTEMPT 2"));
    }

    fn execution() -> ExecutionOutcome {
        ExecutionOutcome {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}
