// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The walk over a [`Plan`] that implements the loop's
//! [`Validators`] port.
//!
//! # `skipped` is distinct, and the runner is not called for it
//!
//! [ADR-0009] D2: "A validator whose prerequisite failed does not run and
//! reports `skipped`, distinctly from `passed` and `failed`." That record's own
//! Positive consequence says why: "Dependency ordering makes a skipped
//! validator distinguishable from a passing one, **which is where a naive
//! implementation would silently report green**."
//!
//! Two halves, and the second is the one a check has to reach for. Reporting
//! [`ValidatorOutcome::Skipped`](crate::iteration::event::ValidatorOutcome)
//! is visible from outside. **Not calling the runner** is not — an
//! implementation that ran the command and threw the result away would report
//! identically, and would run a project's command after its prerequisite
//! failed. The staged runner records what it was asked for, and the check
//! asserts the skipped validator's command is absent from that list.
//!
//! # A skip propagates
//!
//! D2 says what happens to a validator whose prerequisite *failed*, and is
//! silent about one whose prerequisite was *skipped*. Under a delegated
//! coordinator ruling of 2026-09-04, **a skip propagates**: a prerequisite
//! that did not run cannot have passed, and running its dependent anyway would
//! be exactly the silent green D2 exists to prevent, arriving one hop further
//! down the chain. Recorded as a proposed Update on that record rather than
//! settled here, and pinned by
//! `a_skip_propagates_down_a_chain_of_prerequisites`.
//!
//! # The candidate's execution is not read, and that is a finding
//!
//! [`Validators::evaluate`]
//! takes the candidate's [`ExecutionOutcome`], and **all four of [ADR-0009]
//! D3's kinds are statements about the *validator command's* output** — its
//! exit code, its standard output. So the parameter is not read here. Either
//! declared validators genuinely do not need it, or ADR-0009 intends them to
//! see the candidate's output and does not say so; the finding is recorded on
//! ADR-0008 and ADR-0009 and **the port is ADR-0008's, so nothing about it is
//! changed**.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [`ExecutionOutcome`]: crate::iteration::port::ExecutionOutcome

use crate::iteration::event::ValidatorOutcome;
use crate::iteration::port::{ExecutionOutcome, PortFailure, ValidatorReport, Validators};
use crate::iteration::validator::expectation::Expect;
use crate::iteration::validator::name::Name;
use crate::iteration::validator::plan::Plan;
use crate::iteration::validator::port::{
    PatternMatch, SchemaValidate, ValidatorOutput, ValidatorRunner,
};
use std::collections::BTreeMap;

/// Runs a [`Plan`]'s validators in order and reports on each one.
///
/// Borrows everything it needs rather than owning it, for the reason
/// [`Ports`](crate::iteration::port::Ports) does: one run of the loop takes
/// these together and a caller that had to hand ownership over would have to
/// rebuild them per iteration.
#[derive(Debug)]
pub struct Dispatch<'a, R, M, S> {
    plan: &'a Plan,
    runner: &'a R,
    pattern: &'a M,
    schema: &'a S,
}

impl<'a, R, M, S> Dispatch<'a, R, M, S> {
    /// Dispatch this plan through these three ports.
    #[must_use]
    pub const fn new(plan: &'a Plan, runner: &'a R, pattern: &'a M, schema: &'a S) -> Self {
        Self {
            plan,
            runner,
            pattern,
            schema,
        }
    }

    /// The plan being dispatched.
    #[must_use]
    pub const fn plan(&self) -> &Plan {
        self.plan
    }
}

impl<R, M, S> Validators for Dispatch<'_, R, M, S>
where
    R: ValidatorRunner + Sync,
    M: PatternMatch + Sync,
    S: SchemaValidate + Sync,
{
    async fn evaluate(
        &self,
        _execution: &ExecutionOutcome,
    ) -> Result<Vec<ValidatorReport>, PortFailure> {
        let mut outcomes: BTreeMap<&Name, ValidatorOutcome> = BTreeMap::new();
        let mut reports = Vec::with_capacity(self.plan.len());

        for declared in self.plan.ordered() {
            // A prerequisite that did not pass blocks this validator, whether
            // it failed or was itself skipped. `Plan::from_declared` has
            // already refused an unknown prerequisite and a cycle, and the
            // plan is in dependency order, so every prerequisite has an
            // outcome by the time this reads for it.
            let blocked = declared
                .after
                .iter()
                .any(|prerequisite| outcomes.get(prerequisite) != Some(&ValidatorOutcome::Passed));
            if blocked {
                outcomes.insert(&declared.name, ValidatorOutcome::Skipped);
                reports.push(ValidatorReport {
                    name: declared.name.as_str().to_owned(),
                    outcome: ValidatorOutcome::Skipped,
                    detail: String::new(),
                });
                continue;
            }

            let output = self.runner.run(&declared.run).await?;
            let passed = self.decide(&declared.expect, &output).await?;
            let outcome = if passed {
                ValidatorOutcome::Passed
            } else {
                ValidatorOutcome::Failed
            };
            outcomes.insert(&declared.name, outcome);
            reports.push(ValidatorReport {
                name: declared.name.as_str().to_owned(),
                outcome,
                detail: if passed {
                    String::new()
                } else {
                    captured(&output)
                },
            });
        }

        Ok(reports)
    }
}

impl<R, M, S> Dispatch<'_, R, M, S>
where
    R: ValidatorRunner + Sync,
    M: PatternMatch + Sync,
    S: SchemaValidate + Sync,
{
    /// Whether this expectation held over what the command produced.
    ///
    /// **Exhaustive over [`Expect`] with no wildcard arm**, so a fifth kind
    /// cannot arrive without somebody deciding here what it means.
    async fn decide(&self, expect: &Expect, output: &ValidatorOutput) -> Result<bool, PortFailure> {
        match expect {
            Expect::ExitZero => Ok(output.exit_code == 0),
            Expect::ExitCode(code) => Ok(output.exit_code == *code),
            Expect::Matches(pattern) => self.pattern.matches(pattern, &output.stdout).await,
            Expect::JsonSchema(schema) => self.schema.validates(schema, &output.stdout).await,
        }
    }
}

/// What a failing validator carries into refinement.
///
/// [ADR-0009] D5: "The captured stdout **and** stderr of a failing validator
/// flow verbatim into refinement, per [ADR-0008] D4." So both streams are
/// carried, byte for byte, joined by a single newline when both have content
/// and by nothing at all when one is empty.
///
/// **No label, no heading, no prose.** [ADR-0008] D4 forbids paraphrase and
/// this module has nothing to add: the loop's own `failure_text` already
/// prefixes the validator's name, and `refinement::construct` already frames
/// the execution's two streams. Anything more here would be a second framing
/// of the same bytes, and the check that asserts these bytes reach the prompt
/// unaltered could then no longer be an exact one.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
fn captured(output: &ValidatorOutput) -> String {
    match (output.stdout.is_empty(), output.stderr.is_empty()) {
        (true, _) => output.stderr.clone(),
        (_, true) => output.stdout.clone(),
        _ => format!("{}\n{}", output.stdout, output.stderr),
    }
}
