// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two read-only adapters: [ADR-0015] D2's namespaces and [ADR-0010] D2's
//! transcript.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::cli::namespace::Namespace;
use crate::session::{Phase, Record};
use zaru_tui::shell::port::{
    CommandVocabulary, Line, Namespace as Row, Register, TranscriptSource,
};

/// [ADR-0015] D2's table, answered from this crate's own closed enum.
///
/// **Nothing here is a list.** Every row is walked from [`Namespace::ALL`] and
/// every answer is one of that type's own methods, so the eleventh namespace
/// arrives on both surfaces at once or on neither. The nearest match is
/// `config::nearest`, which is where [ADR-0014] D5's rule already lives and
/// where the out-of-session parser reads it from — one place, two callers.
/// Named in prose rather than linked: it is `pub(crate)`, and rustdoc's
/// `private_intra_doc_links` is right to refuse a public page pointing at
/// something a reader of that page cannot open.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, Copy, Default)]
pub struct Vocabulary;

impl CommandVocabulary for Vocabulary {
    fn namespaces(&self) -> Vec<Row> {
        Namespace::ALL
            .into_iter()
            .map(|namespace| Row {
                slash: namespace.slash(),
                governs: namespace.governs(),
                built: namespace.is_built(),
                verbs: namespace.slash_verbs(),
            })
            .collect()
    }

    fn nearest(&self, offered: &str) -> Option<&'static str> {
        crate::config::nearest::nearest(
            Namespace::ALL.into_iter().map(Namespace::slash),
            &format!("/{offered}"),
        )
    }

    fn nearest_verb(&self, slash: &str, offered: &str) -> Option<&'static str> {
        let namespace = Namespace::ALL
            .into_iter()
            .find(|namespace| namespace.slash() == slash)?;
        crate::config::nearest::nearest(namespace.slash_verbs().iter().copied(), offered)
    }
}

/// [ADR-0010] D2's transcript, as lines the pane can paint.
///
/// # The wording is the producer's and the register is this adapter's
///
/// [ADR-0008] D3: "**Rendering never reads loop internals.** If the terminal
/// needs something to display, the loop emits it; the terminal does not reach
/// in." A [`Record`] already carries the words — a tool call keeps
/// [ADR-0011] D4's line "exactly as the prompt rendered it", a failure keeps
/// [ADR-0016]'s presentation — so this converts and never composes, except
/// where a `Record::Loop` event has to be turned into a sentence at all.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    lines: Vec<Line>,
}

impl Transcript {
    /// Convert what a session's transcript holds.
    #[must_use]
    pub fn of(records: &[Record]) -> Self {
        Self {
            lines: records.iter().map(line_for).collect(),
        }
    }

    /// Convert the lines a resume already rendered.
    ///
    /// [`crate::session::Resumed`] carries `tail_lines`, which is what
    /// [ADR-0010] D4's out-of-session half prints, so a shell built on those
    /// bytes shows the same thing the pipe does.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn of_records_and_lines(records: &[Record]) -> Self {
        Self::of(records)
    }

    /// Add a line the session produced that no record holds.
    pub fn push(&mut self, line: Line) {
        self.lines.push(line);
    }
}

impl TranscriptSource for Transcript {
    fn lines(&self) -> Vec<Line> {
        self.lines.clone()
    }
}

/// Which register one record belongs in, and what it says.
///
/// **No wildcard arm anywhere**, so a fifth `Record` variant has to be given a
/// register rather than falling into the plain one — which is how a new
/// producer would otherwise render as narration and be read as narration.
fn line_for(record: &Record) -> Line {
    match record {
        Record::Loop(event) => loop_line(event),
        Record::TurnLoop(event) => Line::new(Register::Plain, format!("{event:?}")),
        Record::ToolCall(call) => {
            let register = match call.phase {
                Phase::Started | Phase::Completed => Register::Call,
                // A refused call is not a failure -- ADR-0011 D6 gives the
                // harness no veto and the user's "no" is an answer -- but it
                // is not an ordinary call either, so it is announced.
                Phase::Refused => Register::Announced,
            };
            Line::new(register, call.line.clone())
        }
        Record::Failure(failure) => Line::new(Register::Failed, failure.headline.clone()),
    }
}

/// One of [ADR-0008] D3's eight events, as a sentence.
///
/// The elapsed times are D6's — "each iteration renders its own elapsed time
/// as it completes... The loop trades wall-clock for correctness and that
/// trade must be visible while it is being paid" — and they are read off the
/// fields that record's Update put there for exactly this.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
fn loop_line(event: &zaru_core::iteration::Event) -> Line {
    use zaru_core::iteration::Event;

    match event {
        Event::IterationStarted { n, of } => {
            Line::new(Register::Plain, format!("iteration {n} of {of}"))
        }
        Event::CandidateGenerated { tokens, elapsed } => Line::new(
            Register::Plain,
            format!("generated {tokens} tokens · {}", seconds(*elapsed)),
        ),
        Event::ExecutionCompleted {
            exit_code, elapsed, ..
        } => Line::new(
            Register::Call,
            format!("ran, exit {exit_code} · {}", seconds(*elapsed)),
        ),
        Event::ValidatorEvaluated {
            name,
            outcome,
            detail,
        } => Line::new(Register::Plain, format!("{name}: {outcome:?} — {detail}")),
        Event::IterationFailed { n, reason, elapsed } => Line::new(
            Register::Plain,
            format!("iteration {n} failed: {reason} · {}", seconds(*elapsed)),
        ),
        Event::RefinementConstructed { n, failure_excerpt } => Line::new(
            Register::Plain,
            format!("refining after {n}: {failure_excerpt}"),
        ),
        Event::LoopSucceeded {
            iterations,
            elapsed,
            total_elapsed,
        } => Line::new(
            Register::Succeeded,
            format!(
                "succeeded after {iterations} iteration(s) · {} · {} total",
                seconds(*elapsed),
                seconds(*total_elapsed)
            ),
        ),
        Event::LoopExhausted {
            iterations,
            reason,
            last_failure,
        } => Line::new(
            Register::Exhausted,
            match last_failure {
                Some(failure) => {
                    format!("exhausted after {iterations} iteration(s): {reason:?} — {failure}")
                }
                None => format!("exhausted after {iterations} iteration(s): {reason:?}"),
            },
        ),
    }
}

/// An elapsed time, to two decimal places, in the one place it is formatted.
fn seconds(elapsed: core::time::Duration) -> String {
    format!("{:.2}s", elapsed.as_secs_f64())
}
