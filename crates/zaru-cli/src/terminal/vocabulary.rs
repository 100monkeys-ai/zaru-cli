// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two read-only adapters: [ADR-0015] D2's namespaces and [ADR-0010] D2's
//! transcript.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::cli::namespace::Namespace;
use crate::session::{Phase, Record, SaidOnce, Voice};
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

/// [ADR-0016] D1's failure, as the lines a pane paints.
///
/// **The one function that turns a failure into pane lines, and every site
/// that shows one calls it.** The headline goes in [`Register::Failed`], which
/// is D1's error register, and everything under it goes in
/// [`Register::Plain`], which is the absence of a marker rather than a glyph
/// chosen for it — a second `✗` under the first would say a second failure
/// happened, and the two columns a reader sees under the headline are
/// [`zaru_tui::shell::port::Line::painted`]'s glyph column rather than an
/// indent authored here.
///
/// # Why it exists rather than being written at each site
///
/// Until 2026-09-14 it was written at each site, and **four of the five wrote
/// half of it**. `driver::lines_of`, the turn-failure path, pushed the
/// headline and then every line; the three refusals the pump shows and the
/// [`Record::Failure`] this module replays on `--resume` pushed the headline
/// and dropped the rest. So [ADR-0016] D2's remedy, D4's retry, D1's tier and
/// D3's report reached a reader through a pipe and reached nobody inside a
/// session — D2's own "a stack trace with better grammar", on the one surface
/// where a person cannot scroll up to `--help`. Measured on the release binary
/// at `8fda37f` over a pseudo-terminal: `/notes tokens rm nope` painted
/// `✗ nothing in the store answers to the alias "nope"` and nothing else,
/// while the same refusal out of session carried
/// `run `zaru notes tokens` or `zaru providers keys` to see what this machine
/// holds`.
///
/// A fifth site cannot render half of it now, and that is held rather than
/// asserted: `corpus_one_place_in_the_terminal_renders_a_classified_failure`
/// refuses a `Presentation::of` or a `.headline` anywhere in this module tree
/// but here, `tests.rs` excepted.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// # Why the headline's register is not always `Failed`
///
/// D1 puts `Expected` **outside** the error register: a run that found nothing
/// is not an error, and painting it `✗` tells the reader their command failed
/// when it did not. Every other class is inside it. The pane is the consumer
/// that `Class::is_the_error_register` exists for, and until 2026-09-14 it
/// ignored it and painted every class the same -- caught by
/// `one_failure_of_each_class_renders_whole_and_in_its_own_register`, which
/// walks `fixtures::one_of_each_class` onto a frame.
///
/// The lines under the headline stay `Plain` whatever the class: they are the
/// remedy, and the register is carried by the line the remedy is under.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
fn whole_lines(
    register: Register,
    headline: String,
    under: impl IntoIterator<Item = String>,
) -> Vec<Line> {
    let mut lines = vec![Line::new(register, headline)];
    lines.extend(
        under
            .into_iter()
            .map(|line| Line::new(Register::Plain, line)),
    );
    lines
}

/// The same, for a failure that has not been projected yet.
///
/// **The one place [`crate::failure::Presentation::of`] is called anywhere in
/// `terminal`**, so the projection reaches the pane by one route. Four of
/// [`whole_lines`]' five callers arrive here holding a `Classified`; the
/// fifth is the transcript's own [`Record::Failure`], which is already
/// projected and already flattened and so calls [`whole_lines`] directly.
pub(crate) fn refusal_lines(classified: &crate::failure::Classified) -> Vec<Line> {
    let presentation = crate::failure::Presentation::of(classified);
    let register = if presentation.is_the_error_register() {
        Register::Failed
    } else {
        Register::Plain
    };
    whole_lines(
        register,
        presentation.headline,
        presentation
            .lines
            .iter()
            .map(crate::failure::Line::flattened)
            .collect::<Vec<_>>(),
    )
}

/// Which register one record belongs in, and what it says.
///
/// **No wildcard arm anywhere**, so an eighth `Record` variant has to be given
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
        // **Every line, not only the headline.** `FailureLine` has carried
        // `lines` since the transcript did -- ADR-0016 D2's remedy, D4's
        // retry, D1's tier, D3's report, already flattened by
        // `FailureLine::of` -- and this arm read only the headline, so a
        // refusal replayed on `--resume` lost its remedy a second time, on a
        // file ADR-0010 D2 calls replayable.
        // **The same register the live refusal had**, recovered from the class
        // the transcript stored, so a failure does not change register between
        // the session that raised it and the `--resume` that replays it. An
        // unknown spelling -- a transcript written by a later version that
        // added a class -- stays in the error register, because a `Failure`
        // record is a failure whatever its class is called.
        Record::Failure(failure) => whole_lines(
            crate::failure::Class::named(&failure.class)
                .filter(|class| !class.is_the_error_register())
                .map_or(Register::Failed, |_| Register::Plain),
            failure.headline.clone(),
            failure.lines.clone(),
        ),
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
        // ADR-0010 D2's seventh producer, replayed. **`Plain`, and the
        // register is the record's rather than this adapter's in the
        // strongest sense available**: `Register::Plain`'s own documentation
        // reads "Ordinary narration: a user message, an iteration starting, a
        // candidate", so the type already names this exact use and nothing is
        // authored for it.
        Record::Conversation(said) => vec![spoken(said.voice, &said.text)],
    }
}

/// One half of a turn's conversation, as a line.
///
/// # One function, two callers, and that is what makes the replay honest
///
/// [`Transcript::of`] comes through here on `--resume` and
/// [`crate::terminal::driver::run_a_turn`] comes through here to echo the line
/// the moment the user presses Enter — the rule [`turn_line`] and [`loop_line`]
/// already follow, so what a person watches and what they read back cannot
/// disagree about a word.
///
/// **What they *can* disagree about is one thing, and it is stated rather than
/// hidden.** The echo paints the line as typed; the replay paints what the
/// file holds, which passed [ADR-0008] clause 6's redactor. So a session read
/// back shows the redaction marker where the live pane showed a credential the
/// person themselves typed. That asymmetry already existed for the answer —
/// the streamed deltas reach [`zaru_tui::shell::Shell::stream_delta`] with no
/// redactor anywhere on the path — and the echo follows that precedent under
/// an accepted Update of 2026-09-06 on [ADR-0010] D2, open to Jeshua's veto.
///
/// # It takes the two fields it renders, and not the record
///
/// [`Utterance`](crate::session::Utterance) also carries the turn's number,
/// which nothing here shows.
/// Taking the whole record would force the live caller to supply one, and the
/// caller that matters is [`crate::terminal::driver::run`]'s task arm, which
/// covers a session that resolved **no provider** and therefore has no turn
/// number to give. It would have had to invent one for the echo, which is
/// fabricating a datum in order to render a string that never shows it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub(crate) fn spoken(voice: Voice, text: &str) -> Line {
    Line::new(Register::Plain, format!("{}: {text}", voice.spoken_as()))
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

/// Which register a validator's verdict is written in.
///
/// [ADR-0028] D2's heading — "Failure is shown, **in its own register**" —
/// applied to [ADR-0009] D2's three outcomes. Only the failing one is a
/// setback; a validator that passed is ordinary narration, and one that never
/// ran is ordinary narration too, because a skip is the *prerequisite's*
/// setback and that row carries it.
///
/// Exhaustive and with no wildcard arm, so a fourth outcome is a build error
/// naming this function rather than a silent assignment to whichever side the
/// match happened to fall through to.
///
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
const fn register_for(outcome: zaru_core::iteration::ValidatorOutcome) -> Register {
    use zaru_core::iteration::ValidatorOutcome as What;
    match outcome {
        What::Failed => Register::Setback,
        What::Passed | What::Skipped => Register::Plain,
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
        // spelling. The **failing** one is `Register::Setback` since
        // 2026-09-14 and the other two stay `Plain`: ADR-0028 D2 has an
        // iteration's failure render "as a plot point ... placed as part of
        // the work — the mechanism operating — and never in the register
        // reserved for defects", which is `Register::Failed`, and that
        // record's heading — "Failure is shown, **in its own register**" —
        // is what `Setback` answers. A skip is not a setback: ADR-0009 D2's
        // `skipped` is a validator whose prerequisite failed, so the setback
        // belongs to the prerequisite and was already painted on its own row.
        // The match is exhaustive with no wildcard, so a fourth outcome is a
        // build error here rather than a guess.
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
        } => Line::new(register_for(*outcome), {
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
        // ADR-0028 D2's own subject, in the register its heading names since
        // 2026-09-14. Still never `Register::Failed`, which is that clause's
        // second half and is what `corpus_an_iteration_failure_is_painted_in_
        // the_setback_register_and_never_the_error_registers_colour` holds
        // off the painted cell rather than off this arm.
        Event::IterationFailed { n, reason, elapsed } => Line::new(
            Register::Setback,
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
pub(crate) fn seconds(elapsed: core::time::Duration) -> String {
    format!("{:.2}s", elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::Vocabulary;
    use crate::cli::namespace::Namespace;
    use core::time::Duration;
    use zaru_tui::composer::{Composer, Entries, Entry};
    use zaru_tui::shell::{CommandVocabulary, Input, Key};

    /// The trie the composer is handed here: empty, because this check is
    /// about the second corpus and an entry from the first would be noise.
    struct NoNotes;

    impl Entries for NoNotes {
        fn matches(&self, _prefix: &str, _limit: usize) -> Vec<Entry> {
            Vec::new()
        }
    }

    /// Type `text` into `composer` against the product's own vocabulary.
    fn typing(composer: &mut Composer, text: &str) {
        for ch in text.chars() {
            composer.key(
                Input {
                    key: Key::Char(ch),
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                Duration::ZERO,
                &NoNotes,
                &Vocabulary,
            );
        }
    }

    /// The picker is driven by **this crate's** vocabulary rather than by
    /// `zaru-tui`'s staged one, and what a person meets at a bare `/` is what
    /// [ADR-0005]'s amendment of 2026-09-15 quotes.
    ///
    /// # Why this check is here and not beside the picker
    ///
    /// `zaru-tui`'s `StagedVocabulary` is a hand-written array in that crate's
    /// own fixtures — the crate cannot read [`Namespace::ALL`], because the
    /// dependency runs the other way — so every picker check over there is a
    /// check about that array. `/help` is the standing proof: [ADR-0015] D2's
    /// twelfth row, added 2026-09-14, and the staged array still carries
    /// eleven. Only a check in this crate can say what D2's table actually
    /// puts in front of a person.
    ///
    /// The overflow count is the arm that ties the two together: **seven** is
    /// twelve namespaces less the five rows that fit, and it is the number the
    /// record quotes, so a thirteenth namespace reddens here as well as in
    /// every exhaustive match on `Namespace`.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    #[test]
    fn a_bare_slash_offers_this_harnesss_own_twelve_namespaces() {
        assert_eq!(
            Vocabulary.namespaces().len(),
            Namespace::ALL.len(),
            "the port answers a different number of namespaces than D2's table holds"
        );

        let mut composer = Composer::new();
        typing(&mut composer, "/");
        let lines = composer.strip_lines();

        assert_eq!(
            lines,
            vec![
                "/runtime  tier and membrane".to_owned(),
                "/stack    AEGIS component fetch and status".to_owned(),
                "/notes    Nuclear Notes tokens, workspace, search".to_owned(),
                "/config   configuration and explanation".to_owned(),
                "/memory   relationship memory".to_owned(),
                "… 7 more · type to narrow".to_owned(),
            ],
            "a bare `/` should paint D2's first five namespaces and the line naming the seven \
             that do not fit; it painted {lines:?}"
        );
    }

    /// A namespace this build does not implement is **listed** by the picker
    /// and still refused on `Enter`, in the same words.
    ///
    /// The listing arm and the refusal arm are asserted apart, because they
    /// are two mechanisms: a picker that filtered the unbuilt namespaces out
    /// would satisfy the second on its own, and that is exactly the reading
    /// ADR-0015 D2's "a word naming one of them is refused saying so" does not
    /// support — a person cannot discover a namespace whose only appearance is
    /// in the refusal for typing it.
    #[test]
    fn an_unimplemented_namespace_is_listed_and_still_refuses() {
        let mut composer = Composer::new();
        typing(&mut composer, "/sta");
        assert_eq!(
            composer.strip_lines(),
            vec!["/stack  AEGIS component fetch and status".to_owned()],
            "`/stack` is one of D2's namespaces and the picker lists it whatever this build \
             implements; it painted {:?}",
            composer.strip_lines()
        );

        assert!(
            !Namespace::Stack.is_built(),
            "the premise of this check is that `/stack` has no in-session half"
        );
        let refusal = zaru_tui::shell::Refused::NotBuilt {
            slash: Namespace::Stack.slash(),
            governs: Namespace::Stack.governs(),
        }
        .to_string();
        assert_eq!(
            zaru_tui::shell::command::read("/stack", &Vocabulary),
            zaru_tui::shell::Typed::Refused(zaru_tui::shell::Refused::NotBuilt {
                slash: "/stack",
                governs: "AEGIS component fetch and status",
            }),
            "and typing it still refuses, in the words it refused in before this picker existed: \
             {refusal}"
        );
    }
}
