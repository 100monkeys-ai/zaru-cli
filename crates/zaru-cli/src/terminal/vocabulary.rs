// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two read-only adapters: [ADR-0015] D2's namespaces and [ADR-0010] D2's
//! transcript.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::cli::namespace::Namespace;
use crate::session::{Phase, Record, SaidOnce};
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
            lines: records.iter().flat_map(lines_for).collect(),
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
/// **No wildcard arm anywhere**, so a seventh `Record` variant has to be given
/// a register rather than falling into the plain one — which is how a new
/// producer would otherwise render as narration and be read as narration.
///
/// # It returns many lines, because one record is not always one line
///
/// [ADR-0013] D2's compaction can announce more than once: a layer-6 summary
/// and then one line per attachment D4 had to drop, all from the single
/// `compact` call that caused them. A one-record-one-line signature would
/// have had to choose which of those to show, and the choice would have been
/// made silently at the moment the pane rendered — dropping exactly the lines
/// D4 exists to guarantee ("Removing their choice without telling them is
/// worse than running out"). Every other arm returns one line, and says so by
/// returning a one-element vector rather than by a comment.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
fn lines_for(record: &Record) -> Vec<Line> {
    match record {
        Record::Loop(event) => vec![loop_line(event)],
        Record::TurnLoop(event) => vec![turn_line(event)],
        // **A pair paints once.** `Phase::Started` and `Phase::Completed`
        // carry the *same* `line` -- ADR-0011 D4's rendered call, written
        // before the call and again after it so that a `Started` with no
        // closing record is the interruption ADR-0010 D4 needs. Rendering
        // both put the identical string on the pane twice: measured from
        // main's binary at `8179f8a` on 2026-09-05, one `fs.write` painted
        // `fs.write /tmp/.../note.txt` on two adjacent rows.
        //
        // The completion paints nothing rather than something, because there
        // is nothing on this record that the started line does not already
        // carry: `ToolCall` holds `line`, `out_of_tree`, `destructive` and
        // `phase`, and no outcome, no byte count and no elapsed time. Those
        // are on `Event::ToolCompleted`, which `turn_line` paints as
        // `fs.write returned · 174 bytes · 0.01s` on the very next row -- so
        // what completion adds is already narrated, by the producer that
        // has it.
        //
        // Nothing the file holds stops reaching the buffer, which is what
        // ADR-0010 D2's 2026-09-05 Update requires of this pane: the bytes
        // dropped are a byte-identical *second copy* of a line the reader
        // already has.
        Record::ToolCall(call) => match call.phase {
            Phase::Started => vec![Line::new(Register::Call, call.line.clone())],
            Phase::Completed => Vec::new(),
            // A refused call is not a failure -- ADR-0011 D6 gives the
            // harness no veto and the user's "no" is an answer -- but it
            // is not an ordinary call either, so it is announced. It closes
            // the pair exactly as a completion does and still paints, because
            // its register is the whole point: a reader must be able to see
            // that the call they declined did not act.
            Phase::Refused => vec![Line::new(Register::Announced, call.line.clone())],
        },
        Record::Failure(failure) => {
            vec![Line::new(Register::Failed, failure.headline.clone())]
        }
        // ADR-0002 D3's interrupt channel: a compaction "writes into the live
        // session", because it reports on a turn the user's own message
        // caused. The text is `crate::cli::render`'s and the glyph is the
        // register's, which is ADR-0008 D3's split.
        Record::Compacted(compaction) => compaction
            .announcements
            .iter()
            .map(|announced| {
                Line::new(
                    Register::Announced,
                    crate::cli::render::announcement(announced),
                )
            })
            .collect(),
        // The two lines a session says once, read back on `--resume`. **Two
        // registers, and each is the record's own.** [ADR-0002] D8 puts an
        // event-anchored recommendation "in the same visual register as a SEAL
        // verdict or a learning line", and a learning line is D4's and D5's,
        // which is `Announced`. [ADR-0011] D2 says the harness "states
        // plainly" that `bare` is not a sandbox, and `Plain` is the absence of
        // a marker rather than a glyph chosen for it — no record names one.
        // Deciding both by one register would be the same conflation the two
        // rules that produce them exist to avoid.
        Record::Said(said) => {
            let register = match said.line {
                SaidOnce::Notice => Register::Plain,
                SaidOnce::Recommendation => Register::Announced,
            };
            vec![Line::new(register, said.text.clone())]
        }
    }
}

/// One of [ADR-0008] D1's **outer** loop's seven events, as a sentence.
///
/// # Why this exists at all, and what was here before
///
/// Until 2026-09-05 [`line_for`]'s `TurnLoop` arm was
/// `Line::new(Register::Plain, format!("{event:?}"))` — a Rust `Debug` dump
/// in the narration register, for every one of the seven. That arm was
/// written when nothing emitted the stream; the arc that wired a provider
/// client into the loop made a real turn write it, so `zaru --resume <id>` at
/// a terminal has been painting whole turns as `Debug` since. It also
/// collapsed three of the registers this shell defines and put
/// [`Event::ToolRefused`] in the same one as everything else, which that
/// event's own contract forbids in as many words: "**Not a failure.** … A
/// consumer renders it in whatever register it renders a decision in, and
/// never in the error one."
///
/// [`line_for`]'s own comment already said why that shape was wrong — "how a
/// new producer would otherwise render as narration and be read as
/// narration". This is that producer, given its registers.
///
/// # One function, two callers
///
/// The shell's live sink and the resumed pane both come through here, so what
/// a user watches while a turn runs and what they read back on `--resume`
/// cannot disagree about a single word. [ADR-0008] D3 is why the wording is
/// composed rather than reached for: "**Rendering never reads loop
/// internals.** If the terminal needs something to display, the loop emits
/// it" — every field below is on the event.
///
/// **No register is new.** All six are the ones this shell already defined,
/// and the mapping follows the precedents already in this file:
/// [`Record::ToolCall`]'s `Phase::Started` is `Call` and its `Phase::Refused`
/// is `Announced`, for the same reason.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
pub(crate) fn turn_line(event: &zaru_core::tool_call::Event) -> Line {
    use zaru_core::tool_call::{Event, TurnEnding};

    match event {
        Event::TurnStarted { n, of } => {
            Line::new(Register::Plain, format!("turn {n}, up to {of} exchange(s)"))
        }
        Event::ModelResponded {
            round,
            tokens,
            calls,
            elapsed,
        } => Line::new(
            Register::Plain,
            format!(
                "exchange {round}: {tokens} tokens, {calls} tool call(s) · {}",
                seconds(*elapsed)
            ),
        ),
        Event::ToolRequested { round, call, name } => Line::new(
            Register::Call,
            format!("exchange {round}, call {call}: {name}"),
        ),
        // The statement is carried, never re-composed: ADR-0011 D3 has it
        // "composed once, by the decision, and handed here", which is the
        // same rule the pane's confirmation crosses under.
        Event::ToolPermissionDecided {
            statement,
            permitted,
            ..
        } => Line::new(
            Register::Call,
            format!(
                "{statement} — {}",
                if *permitted {
                    "permitted"
                } else {
                    "not permitted"
                }
            ),
        ),
        Event::ToolCompleted {
            name,
            failed,
            content_bytes,
            elapsed,
            ..
        } => Line::new(
            Register::Call,
            format!(
                "{name} {} · {content_bytes} bytes · {}",
                if *failed {
                    "reported a failure"
                } else {
                    "returned"
                },
                seconds(*elapsed)
            ),
        ),
        // Announced rather than Failed. See the event's own contract, quoted
        // above, and `Phase::Refused` two functions down, which is the same
        // decision about the same thing.
        Event::ToolRefused {
            name,
            because,
            elapsed,
            ..
        } => Line::new(
            Register::Announced,
            format!("{name} did not act: {because} · {}", seconds(*elapsed)),
        ),
        Event::TurnEnded {
            n,
            ending,
            rounds,
            elapsed,
        } => {
            let took = format!("{rounds} exchange(s) · {}", seconds(*elapsed));
            match ending {
                TurnEnding::Answered => {
                    Line::new(Register::Succeeded, format!("turn {n} answered · {took}"))
                }
                // ADR-0016 D5's `1`: the work failed. The provider's own word
                // is not on this event, so the line says what happened and
                // the answer's absence is the rest of it.
                TurnEnding::Stopped => Line::new(
                    Register::Failed,
                    format!("turn {n} stopped without an answer · {took}"),
                ),
                // ADR-0008 D5: exhaustion "is not an error and is not a
                // success", so it gets neither of the two above.
                TurnEnding::CeilingReached => Line::new(
                    Register::Exhausted,
                    format!("turn {n} reached its ceiling · {took}"),
                ),
                TurnEnding::Iterated {
                    iterations,
                    succeeded,
                } => Line::new(
                    if *succeeded {
                        Register::Succeeded
                    } else {
                        Register::Exhausted
                    },
                    format!(
                        "turn {n} ran {iterations} iteration(s), {} · {took}",
                        if *succeeded {
                            "all validators passed"
                        } else {
                            "not every validator passed"
                        }
                    ),
                ),
            }
        }
    }
}

/// One of [ADR-0008] D3's eight events, as a sentence.
///
/// # One function, two callers
///
/// [`crate::terminal::driver::PaneNarrator`] paints a run as it happens and this
/// module's [`Transcript`] paints a `Record::Loop` back on `--resume`, and
/// both come through here — so what a user watches while a run is being paid
/// and what they read afterwards cannot disagree about a word. That is the
/// rule [`turn_line`] already follows for the outer loop.
///
/// The elapsed times are D6's — "each iteration renders its own elapsed time
/// as it completes... The loop trades wall-clock for correctness and that
/// trade must be visible while it is being paid" — and they are read off the
/// fields that record's Update put there for exactly this.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
pub(crate) fn loop_line(event: &zaru_core::iteration::Event) -> Line {
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
        // The outcome in ADR-0009 D2's words rather than the enum's Rust
        // spelling. `Plain` for all three, including the failing one: ADR-0028
        // D2 has an iteration's failure render "as a plot point ... placed as
        // part of the work — the mechanism operating — and never in the
        // register reserved for defects", which is `Register::Failed`.
        // A silent validator says so rather than trailing an em dash with
        // nothing after it. The phrase is `zaru-core`'s `PRODUCED_NO_OUTPUT`,
        // read rather than retyped, because the refinement prompt composes
        // the same phrase for the same condition and a person and a model
        // must not be told it in two different words.
        //
        // **A skip is the one outcome that says nothing at all**, and the
        // difference is not tidiness. `produced no output` is a statement
        // about a command that ran; ADR-0009 D2's `skipped` is a validator
        // whose prerequisite failed, so it was *never run* -- that record's
        // own amendment of 2026-09-04 says the runner "is not called at all"
        // for one. Telling a reader it produced no output would be the
        // harness asserting something about an execution that did not
        // happen, which is the same class of untruth the empty detail was.
        // `cli::render::validator_outcome` spells that outcome "did not
        // run", so the old rendering read `lint: did not run — produced no
        // output`: a sentence that contradicts itself in six words. A skip
        // carries no clause at all, and the separator goes with the thing it
        // was there to introduce.
        Event::ValidatorEvaluated {
            name,
            outcome,
            detail,
        } => Line::new(Register::Plain, {
            let word = crate::cli::render::validator_outcome(*outcome);
            match (outcome, detail.trim().is_empty()) {
                (_, false) => format!("{name}: {word} — {detail}"),
                (zaru_core::iteration::ValidatorOutcome::Skipped, true) => {
                    format!("{name}: {word}")
                }
                (_, true) => {
                    format!(
                        "{name}: {word} — {}",
                        zaru_core::iteration::PRODUCED_NO_OUTPUT
                    )
                }
            }
        }),
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
        } => {
            // The reason in ADR-0008 D5's "`ExhaustionReason`'s own words",
            // through the function the binary's own exit already prints, so a
            // run that stopped does not have one explanation on the screen and
            // another at the exit code.
            let why = crate::cli::render::exhaustion(*iterations, *reason);
            Line::new(
                Register::Exhausted,
                match last_failure {
                    Some(failure) => {
                        format!("exhausted after {iterations} iteration(s): {why} — {failure}")
                    }
                    None => format!("exhausted after {iterations} iteration(s): {why}"),
                },
            )
        }
    }
}

/// An elapsed time, to two decimal places, in the one place it is formatted.
fn seconds(elapsed: core::time::Duration) -> String {
    format!("{:.2}s", elapsed.as_secs_f64())
}
