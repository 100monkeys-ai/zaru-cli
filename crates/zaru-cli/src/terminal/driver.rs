// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The loop that pumps the shell, and the terminal it pumps into.
//!
//! # Everything the terminal can do is a port, so every path is checkable
//!
//! [`Surface`] is drawing, reading a key, and restoring. The product
//! implementation is [`Crossterm`], reached as `ratatui::crossterm` through
//! `ratatui`'s own feature; a check implements the same three methods over a
//! recorded script and a `TestBackend`. That is what lets
//! `the_terminal_is_restored_when_the_shell_panics` exist at all: a check
//! cannot put a real terminal into raw mode, and the property that matters is
//! not about crossterm.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::invocation::Request;
use crate::failure::Exit;
use crate::terminal::source::{Pace, Source, Taken};
use crate::tools::port::Question;
use core::time::Duration;
use zaru_tui::shell::port::{Confirmation, Line, Register};
use zaru_tui::shell::{Action, Command, Shell};

/// Giving the terminal back to the user.
///
/// A port of its own rather than a method on [`Surface`], because [`Guard`] is
/// generic over it and the only thing a guard must be able to do is this one.
pub trait Restore {
    /// Leave the alternate screen, leave raw mode, show the cursor.
    ///
    /// **Called at most once**, which is [`Guard`]'s doing rather than an
    /// obligation on the implementation.
    fn restore(&mut self);
}

/// Everything the pump needs from a terminal it paints on.
///
/// **Drawing only, since 2026-09-05.** Reading a keystroke used to be a method
/// here and blocked, which is precisely why nothing repainted while the model
/// was thinking: a turn polled to completion on one thread could not also be
/// inside it. The reader is [`Source`] now, on a thread of its own, so a turn
/// and the terminal are two things the pump can wait on at once.
pub trait Surface: Restore {
    /// Paint the shell.
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()>;
}

/// Holds a restorer and gives the terminal back on drop.
///
/// # Why a `Drop` and not a call at the end of the loop
///
/// A restore written at the end runs on the paths the author thought of. This
/// one runs on all of them, including an early return and an unwind — and the
/// unwind is the one that matters, because a panic that left the terminal in
/// raw mode would make [ADR-0016] D3's defect report unreadable at the moment
/// the user most needs to read it.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub struct Guard<R: Restore>(Option<R>);

impl<R: Restore> Guard<R> {
    /// Take ownership of a restorer.
    pub const fn new(restorer: R) -> Self {
        Self(Some(restorer))
    }

    /// The restorer, while the guard still holds it.
    pub const fn get_mut(&mut self) -> Option<&mut R> {
        self.0.as_mut()
    }

    /// Restore now rather than on drop.
    ///
    /// Idempotent by construction: the restorer is taken, so a later drop has
    /// nothing to restore and the terminal is never handed back twice.
    pub fn restore_now(&mut self) {
        if let Some(mut restorer) = self.0.take() {
            restorer.restore();
        }
    }
}

impl<R: Restore> Drop for Guard<R> {
    fn drop(&mut self) {
        self.restore_now();
    }
}

/// [ADR-0011] D3's question, as the shell renders it.
///
/// # Two renderings of one question, and one source for every word in it
///
/// [`crate::tools::prompt`] is the plain one, "for the surface that writes
/// plain lines to standard output"; this is the terminal's, which paints the
/// same question in a pane. **Both reach the same [`Confirm`] port with the
/// same [`Question`]**, which is what that module's own documentation
/// anticipates.
///
/// So nothing a user reads is spelled twice. The statement crosses unchanged —
/// D3's port says it "is composed once, by the decision, and handed here...
/// so that what the user was told and what the harness believes it asked
/// cannot drift apart", and a conversion that reworded it would be that drift.
/// **The answers cross too**, as [`prompt::SUFFIX`] trimmed of the padding the
/// plain line needs and the pane does not: the `y/N` a user reads is part of
/// what they were told, and it landed in `prompt` first, so `zaru-tui` holds
/// no constant for it.
///
/// # What is deliberately *not* shared, because the inputs differ
///
/// [`prompt::answer`] reads a typed line and this pane reads a keystroke.
/// There is no line to trim and no end-of-input to see, so the two rules
/// cannot be one function. What they agree on is the only case they share and
/// it is asserted rather than assumed:
/// `the_pane_and_the_plain_prompt_agree_on_what_a_yes_is` drives an explicit
/// `y` through both and a decline through both.
///
/// [`Confirm`]: crate::tools::port::Confirm
/// [`prompt::SUFFIX`]: crate::tools::prompt::SUFFIX
/// [`prompt::answer`]: crate::tools::prompt::answer
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn question_for_the_shell(question: &Question) -> Confirmation {
    Confirmation::new(
        question.statement.clone(),
        crate::tools::prompt::SUFFIX.trim(),
        question.prominent,
    )
}

/// What the fall-through in [`dispatch`] says.
///
/// Named once so a check can look for it rather than for a phrase somebody
/// retyped.
pub(crate) const UNAVAILABLE: &str = "needs something this harness does not have yet";

/// Whether this shell can run a task, and what to say when it cannot.
///
/// # Two variants and no third, because a shell either has a turn or has a
/// reason
///
/// [`crate::compose::turn::prepare`] runs **once**, when the shell opens: a
/// session's tier, model, boundary, manifest, key, client and [ADR-0012]
/// clause 3 witness do not change between two of its turns. It either
/// resolves, and every turn is [`Turns`], or it refuses — and the refusal is
/// the real one: no key for a kind that has a client, a key only for kinds
/// that have none, a project that declared validators. Showing *that* is what
/// makes the pane's answer to a task **the same answer** `zaru "<task>"`
/// gives, rather than a second sentence about one fact.
///
/// The lines are carried rather than composed here, which is the rule
/// [ADR-0011] D3 states for its own statement: what the user was told and
/// what the harness believes it said cannot be allowed to drift apart.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[derive(Debug)]
pub enum Turnable<'a> {
    /// This session can run a turn, and this is what every one of them needs.
    ///
    /// Boxed because [`Turns`] carries a whole resolved session and this
    /// variant sits beside a `Vec`; `clippy::large_enum_variant` refuses the
    /// difference, and the value is constructed once per session.
    Ready(Box<Turns<'a>>),
    /// It cannot, and these are the lines that say why.
    Cannot(Vec<Line>),
}

/// What one turn of the pump produced.
#[derive(Debug)]
pub struct Pump {
    /// What the process should exit with.
    pub exit: Exit,
}

/// Everything a turn needs to run inside the session this shell is in.
///
/// # Resolved once, at the door, and then held
///
/// [`crate::compose::turn::prepare`] is the half of a turn that does not
/// change between two turns of one session — the tier, the model, the
/// boundary, the manifest, the key, the client, and [ADR-0012] clause 3's
/// witness, which that clause asks for "before the loop starts". The shell
/// resolves it when it opens and hands it to every turn.
///
/// [`Owed`](crate::compose::Owed) and the context are the session's for the
/// same reason and a stronger one: [ADR-0011] D2's notice is stated "once at
/// session start", [ADR-0002] D8's recommendation "fires at most once ever",
/// and [ADR-0013] D1's layer 6 is what a second turn assembles over. A turn
/// that rebuilt any of the three would restate two lines and forget the
/// conversation.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub struct Turns<'a> {
    /// The binary's own version, for [ADR-0016] D3's report.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    pub version: &'a str,
    /// Where a defect is reported.
    pub report_at: &'a str,
    /// [ADR-0014]'s five layers, folded once.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    pub resolution: &'a crate::config::Resolution,
    /// What this session resolved before it existed.
    pub prepared: &'a crate::compose::Prepared,
    /// [ADR-0010] D1's directory, already open.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub session: &'a crate::session::Session,
    /// The two lines this session owes once.
    pub owed: crate::compose::Owed,
    /// [ADR-0013]'s context, carried across turns.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    pub context: crate::compose::SessionContext,
    /// Which turn the next one is.
    ///
    /// [`zaru_core::tool_call::run`]'s `n` is "the caller's, because a session
    /// spans many calls to this function and a number invented here would
    /// restart at one every turn". This is the caller.
    pub next: u32,
}

impl core::fmt::Debug for Turns<'_> {
    /// Names what it holds and renders none of it.
    ///
    /// A `Debug` is what ends up in a panic message, and this value reaches a
    /// credential store's redactor, a resolved configuration, and the whole of
    /// what a model has been shown.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Turns")
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

/// The shell and the terminal it paints on, borrowed together for one turn.
///
/// # Why the two are one value behind one lock
///
/// A turn wants both of them from two places at once. [ADR-0008] clause 3's
/// second consumer is an [`EventSink`](zaru_core::tool_call::EventSink), whose
/// `emit` takes `&mut self`; [ADR-0011] D3's confirmation is a
/// [`Confirm`](crate::tools::port::Confirm), whose `confirm` takes `&self` and
/// which [`Executor`](crate::tools::Executor) requires to be `Sync`. Neither
/// can hold `&mut Shell` on its own and both must paint.
///
/// So the pair is one value behind a `Mutex`, which is exactly what
/// [`crate::tools::prompt::Prompt`] already does with its two handles and for
/// exactly the same reason: `Sync`. **The lock is never contended.** A turn is
/// polled to completion on one thread by [`crate::compose::turn`]'s
/// current-thread runtime, and the loop emits *around* a tool call rather than
/// inside one, so a sink and a confirmer are never live at the same instant.
/// `try_lock` rather than `lock` is what makes that a claim the code can
/// report on: contention here would be a defect, and a defect that reports is
/// better than one that hangs a user's terminal with no way out of it.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub struct Pane<'a, S: Surface + Send> {
    shell: &'a mut Shell,
    surface: &'a mut S,
    /// The first paint that failed, kept rather than lost — the rule
    /// [`crate::compose::Records`] follows for the same reason.
    first_failure: Option<std::io::Error>,
}

impl<S: Surface + Send> core::fmt::Debug for Pane<'_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pane")
            .field("painted_failure", &self.first_failure.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a, S: Surface + Send> Pane<'a, S> {
    /// Borrow a shell and its terminal for the length of one turn.
    pub fn of(shell: &'a mut Shell, surface: &'a mut S) -> Self {
        Self {
            shell,
            surface,
            first_failure: None,
        }
    }

    /// Add a line and paint.
    fn note(&mut self, line: Line) {
        self.shell.notice(line);
        self.paint();
    }

    /// Paint, keeping the first paint that failed.
    fn paint(&mut self) {
        if let Err(failure) = self.surface.draw(self.shell)
            && self.first_failure.is_none()
        {
            self.first_failure = Some(failure);
        }
    }
}

/// [ADR-0008] clause 3's renderer half.
///
/// That clause asks for the stream to be "consumed by both the terminal
/// renderer and the transcript writer, **from one emission**". The transcript
/// writer is [`crate::compose::Records`] and has been on the loop's slice since
/// a provider client was wired to it; this is the other consumer, and putting
/// the two on one slice is what the clause is about — `run` "constructs each
/// event once and hands the same value to every registered sink in turn".
///
/// The wording is [`crate::terminal::vocabulary`]'s `turn_line`, which is the
/// same function the **resumed** pane renders a `Record::TurnLoop` through. So
/// what a user watches while a turn runs and what they read back on `--resume`
/// cannot disagree about a word.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
pub struct PaneSink<'m, 'a, S: Surface + Send> {
    pane: &'m std::sync::Mutex<Pane<'a, S>>,
    contended: usize,
}

impl<S: Surface + Send> core::fmt::Debug for PaneSink<'_, '_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaneSink")
            .field("contended", &self.contended)
            .finish_non_exhaustive()
    }
}

impl<'m, 'a, S: Surface + Send> PaneSink<'m, 'a, S> {
    /// A sink over a borrowed pane.
    pub const fn over(pane: &'m std::sync::Mutex<Pane<'a, S>>) -> Self {
        Self { pane, contended: 0 }
    }

    /// How many events the lock refused.
    ///
    /// Zero by construction — see [`Pane`] — and counted rather than assumed,
    /// so a check can assert the zero instead of the argument for it.
    #[must_use]
    pub const fn contended(&self) -> usize {
        self.contended
    }
}

impl<S: Surface + Send> zaru_core::tool_call::EventSink for PaneSink<'_, '_, S> {
    fn emit(&mut self, event: &zaru_core::tool_call::Event) {
        match self.pane.try_lock() {
            Ok(mut pane) => pane.note(crate::terminal::vocabulary::turn_line(event)),
            Err(_) => self.contended += 1,
        }
    }
}

/// [ADR-0011] D3's question, asked and answered in the pane.
///
/// # It is a `Confirm`, so the shell owns no prompt of its own
///
/// The port is `zaru-cli`'s and the plain implementation over a tty is
/// [`crate::tools::prompt::Prompt`]. This is a second implementation of the
/// same port taking the same [`Question`], which
/// [`question_for_the_shell`]'s own documentation anticipated: "a richer
/// prominence … is `zaru-tui`'s, and it reaches this same `Confirm` port with
/// the same `Question`". Nothing here reads a terminal's standard input and
/// nothing here composes a sentence.
///
/// **`confirm` is synchronous, and that is what makes this work with no
/// channel and no second thread.** The whole turn is polled on one thread by
/// [`crate::compose::turn`]'s current-thread runtime, so this paints the
/// question, pumps the terminal into [`Shell::key`] until the shell has an
/// answer, and returns it — inside the call the executor is waiting on.
///
/// # Running out of keys is not an answer
///
/// A surface whose events end yields a failure, never `false`. That is
/// [`crate::tools::prompt`]'s own rule stated for this surface: a default
/// answer would be the silent default D3 forbids, and answering `false` would
/// put "the user declined" in the transcript of a question nobody saw.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub struct PaneConfirm<'m, 'a, S: Surface + Send, P: Pace + Sync> {
    pane: &'m std::sync::Mutex<Pane<'a, S>>,
    source: &'m Source,
    pace: &'m P,
}

impl<S: Surface + Send, P: Pace + Sync> core::fmt::Debug for PaneConfirm<'_, '_, S, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaneConfirm").finish_non_exhaustive()
    }
}

impl<'m, 'a, S: Surface + Send, P: Pace + Sync> PaneConfirm<'m, 'a, S, P> {
    /// A confirmer over a borrowed pane, the terminal's keys, and a beat.
    pub const fn over(
        pane: &'m std::sync::Mutex<Pane<'a, S>>,
        source: &'m Source,
        pace: &'m P,
    ) -> Self {
        Self { pane, source, pace }
    }
}

impl<S: Surface + Send, P: Pace + Sync> crate::tools::port::Confirm for PaneConfirm<'_, '_, S, P> {
    fn confirm(&self, question: &Question) -> Result<bool, crate::tools::port::ConfirmFailure> {
        let mut pane = self.pane.try_lock().map_err(|_| {
            crate::tools::port::ConfirmFailure::new(
                "the pane was already in use when the question was raised".to_owned(),
            )
        })?;

        pane.shell.ask(question_for_the_shell(question));
        pane.paint();

        // A standing question takes every key: `Shell::key` gives the composer
        // nothing while one stands, so this cannot be typed past.
        let mut now = Duration::ZERO;
        loop {
            if let Some(answer) = pane.shell.answer() {
                return Ok(answer);
            }
            let input = match self.source.try_next() {
                Taken::Key(input) => input,
                // **The pane keeps painting while the question stands.**
                // Nothing on it changes on a bare beat -- see `TICK` -- but
                // the paint is what makes this loop a repaint rather than a
                // block, and it is the same call the pump's tick makes, so a
                // terminal that lost its screen recovers here as it does
                // there.
                Taken::Nothing => {
                    pane.paint();
                    self.pace.wait();
                    continue;
                }
                Taken::Ended => {
                    return Err(crate::tools::port::ConfirmFailure::new(
                        "the terminal stopped answering before the question was".to_owned(),
                    ));
                }
            };
            now += Duration::from_millis(1);
            let acted = pane.shell.key(input, now, &NoEntries, &NoVocabulary);
            pane.paint();
            // A question is not a prompt a user can leave past either: this
            // call is what a tool is waiting on and there is nowhere for a
            // `Leave` to be returned to. `Shell::key` absorbs everything but
            // the keys it acts on while a question stands, so this is
            // unreachable — and it is refused rather than ignored, because an
            // unreachable branch that silently continues is how a reachable
            // one arrives unnoticed.
            if !matches!(acted, Action::Idle) {
                return Err(crate::tools::port::ConfirmFailure::new(
                    "the shell acted on a keystroke while a question stood".to_owned(),
                ));
            }
        }
    }
}

/// The composer sees no keystroke while a question stands, so the entries it
/// would search are never asked for.
#[derive(Debug)]
struct NoEntries;

impl zaru_tui::composer::Entries for NoEntries {
    fn matches(&self, _prefix: &str, _limit: usize) -> Vec<zaru_tui::composer::Entry> {
        Vec::new()
    }
}

/// The same, for the vocabulary: no line is submitted while a question stands.
#[derive(Debug)]
struct NoVocabulary;

impl zaru_tui::shell::CommandVocabulary for NoVocabulary {
    fn namespaces(&self) -> Vec<zaru_tui::shell::Namespace> {
        Vec::new()
    }

    fn nearest(&self, _offered: &str) -> Option<&'static str> {
        None
    }

    fn nearest_verb(&self, _slash: &str, _offered: &str) -> Option<&'static str> {
        None
    }
}

/// Every tool line one turn produced, for [ADR-0013] D1's layer 6.
///
/// A third sink beside the pane's and the transcript's, and it exists because
/// layer 6 is "conversation **and tool results**" and the tool results are on
/// the event stream rather than in [`Ran`](crate::compose::Ran), which carries
/// what the turn *printed*. Reading them off the same emission the pane and
/// the transcript read is what keeps the three consistent — ADR-0008 clause
/// 3's "one emission" with a third consumer rather than a second reading.
///
/// It keeps the lines the shell's own vocabulary puts in the call register,
/// which is ADR-0011 D4's rendered line and the one this workspace already
/// produces in exactly one place.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[derive(Debug, Default)]
pub(crate) struct ToolLines {
    lines: Vec<String>,
}

impl ToolLines {
    /// Take what the turn produced, leaving the collector empty.
    pub(crate) fn taken(&mut self) -> Vec<String> {
        core::mem::take(&mut self.lines)
    }
}

impl zaru_core::tool_call::EventSink for ToolLines {
    fn emit(&mut self, event: &zaru_core::tool_call::Event) {
        let line = crate::terminal::vocabulary::turn_line(event);
        if line.register == zaru_tui::shell::port::Register::Call {
            self.lines.push(line.text);
        }
    }
}

/// What the sentence a task typed during a turn is refused with says.
///
/// # The ruling this obeys, and why the case only now exists
///
/// [ADR-0015]'s Status tracking, 2026-09-05, under directive 20: "**A task
/// typed while a turn is running is refused with a notice, not queued.**
/// Neither D1 nor D2 nor any clause of this record says what a second task
/// means while the first is still running … Refusing is the safe direction: a
/// queue is a promise about ordering that no record has made, and a user who
/// typed while waiting can type again." That ruling also says it is "barely
/// reachable in practice — the shell reads no keystroke during a turn".
///
/// **It is reachable now**, because that sentence stopped being true the
/// moment a source could be read beside the turn. So the notice exists, and
/// the composed line stays on the input row rather than being cleared: not
/// queued, because nothing will submit it, and not lost either.
///
/// [`Register::Announced`] rather than [`Register::Failed`] for the reason
/// `ToolRefused` carries — a refusal that is a decision rather than one of
/// [ADR-0016] D1's five classes is rendered "in whatever register it renders a
/// decision in, and never in the error one".
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const BUSY: &str = "a turn is already running · this line stays in the prompt until it ends";

/// What became of a turn the pump was running.
///
/// Three cases and no `Option`, for [`Taken`]'s own reason: a turn that ran, a
/// user who left in the middle of it, and a terminal that stopped answering
/// are three different things and a caller does three different things with
/// them.
#[derive(Debug)]
pub enum Turned {
    /// The turn finished. These are the lines to leave on the pane.
    Ran(Vec<Line>),
    /// The user left while it was running, and the turn's future was dropped.
    Interrupted(zaru_tui::shell::Leaving),
    /// The terminal stopped answering. A product terminal does not do this.
    SourceEnded,
}

/// Run one turn of this session for `task`, painting it as it happens.
///
/// # The turn, the terminal and a beat, waited on together
///
/// Until 2026-09-05 this awaited the turn and nothing else, so **during the
/// provider's own await nothing repainted and no keystroke was read** — the
/// gap [ADR-0005] and [ADR-0008] both carried. The turn is one branch of a
/// `select!` now and the terminal is another, on the runtime the session
/// already holds. No second runtime and no second thread beyond the source's
/// own reader.
///
/// **`biased`, so the polling order is a decision rather than a coin.** The
/// turn first — a finished turn is not made to wait behind a tick that is also
/// ready — then the terminal, then the beat. Tokio's default is a random
/// branch, and a check over a random instrument is not a check (library
/// verification-lessons §57).
///
/// # What a mid-turn `Ctrl-C` is
///
/// It **leaves**, exactly as one at the prompt does: [`zaru_tui::shell::leaves`]
/// is the one rule and [ADR-0015]'s ruling of 2026-09-05 gives this key one
/// meaning, at [ADR-0016] D5's `0`.
///
/// **And leaving is what makes it an interruption.** Breaking out of the loop
/// drops the turn's future. Every event a sink emitted is already on disk —
/// [`crate::compose::Records`] serialises, appends and syncs per event — so
/// the transcript holds exactly what happened up to the drop, which is
/// [ADR-0010] D2's "a crash loses at most the event in flight" without a
/// crash. A tool call in flight has left its `Phase::Started` and no
/// `Phase::Completed`, and that pair *is* the interruption D4 derives on the
/// next `--resume` and hands the model as `Turn::Resumed`. Nothing is authored
/// for it and no new state exists.
///
/// **An interrupted turn records no exchange.** [ADR-0013] D1's layer 6 is
/// what came back, and nothing came back.
///
/// # A child process is interruptible too, since 2026-09-05
///
/// [`crate::process::Spawn::execute`] was blocking — a `std::process::Command`,
/// `try_wait` and a sleep, with two reader threads joined at the end — and it
/// is called from the asynchronous `Subprocess` port, so **while `cmd.run` or
/// a declared validator's command was running the current-thread runtime was
/// blocked**: no beat fired, no keystroke was read, and an interrupt was not
/// seen until the child returned. It is `async` now — the wait, the two pipes
/// and the ceiling are futures on this same runtime — so the two branches
/// below are polled during a child exactly as they are during a provider's
/// await. **And leaving ends the child**: the child is a value the turn's
/// future owns and `kill_on_drop` is set, so the drop that makes this an
/// interruption sends it the same `SIGKILL` the ceiling does. The gap this
/// paragraph used to name is closed on ADR-0011 and ADR-0009, with the reason
/// each recorded for not closing it corrected: a current-thread runtime does
/// have a blocking pool, and what disqualifies `spawn_blocking` is that a
/// blocking task cannot be cancelled at all.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the same list `run` takes, minus the two it does not need. Every one is a \
    port or a value some record owns, and bundling them would be a second name \
    for the same list -- the argument `compose::turn::run_one` already makes"
)]
pub async fn run_a_turn<S: Surface + Send, P: Pace + Sync>(
    shell: &mut Shell,
    surface: &mut S,
    source: &Source,
    pace: &P,
    entries: &dyn zaru_tui::composer::Entries,
    now: &mut Duration,
    turns: &mut Turns<'_>,
    task: &str,
) -> Turned {
    let n = turns.next;
    turns.next += 1;

    let mut tools = ToolLines::default();
    let raced = {
        let pane = std::sync::Mutex::new(Pane::of(shell, surface));
        let confirm = PaneConfirm::over(&pane, source, pace);
        let mut sink = PaneSink::over(&pane);
        let mut extra: [&mut dyn zaru_core::tool_call::EventSink; 2] = [&mut sink, &mut tools];

        race(
            &pane,
            source,
            pace,
            entries,
            now,
            crate::compose::turn::run_one(
                turns.version,
                turns.report_at,
                turns.resolution,
                turns.prepared,
                turns.session,
                n,
                task,
                Some(&confirm as &(dyn crate::tools::Confirm + Sync)),
                &mut extra,
                &mut turns.owed,
                &mut turns.context,
            ),
        )
        .await
    };
    let tool_lines = tools.taken();

    let ran = match raced {
        Raced::Ran(ran) => ran,
        Raced::Interrupted(leaving) => return Turned::Interrupted(leaving),
        Raced::SourceEnded => return Turned::SourceEnded,
    };

    // ADR-0013 D1's layer 6, so the next turn assembles over this one. Every
    // part passes the one `Redactor` the session already holds — ADR-0008
    // clause 6's port, on every path from captured bytes into a model prompt —
    // and this is why this file is on that clause's enumeration.
    //
    // **All three parts, because D1's layer 6 is "conversation *and tool
    // results*".** This caller recorded the task and the answer until
    // 2026-09-05; `Exchange::of_turn` is `context-summariser`'s declared shape
    // for the type and it takes the middle as well, so what the tools returned
    // survives into the next turn instead of ending with the turn that ran
    // them. That matters most in exactly the case the composed rule was
    // written for: a coding session, where the file that was read and the
    // command that failed are the facts a later turn needs.
    let redactor = turns.prepared.redactor();
    let redacted = |text: &str| {
        zaru_core::redaction::Redacted::by(redactor, text)
            .as_str()
            .to_owned()
    };
    let results: Vec<String> = tool_lines.into_iter().map(|line| redacted(&line)).collect();
    turns.context.record(zaru_core::context::Exchange::of_turn(
        &redacted(&format!("user: {task}")),
        &results,
        &redacted(&format!("zaru: {}", ran.lines.join("\n"))),
    ));

    // ADR-0013 clause 5 and ADR-0012 clause 6, both on ADR-0001 D2's row.
    // **After the record above**, so the number the user reads is the context
    // the *next* turn will assemble over rather than the one this turn saw --
    // which is what D6's "a number that has been visible all along" means for
    // somebody about to type again. A compaction made at the next turn's
    // boundary shows here at the end of that turn, by the same rule.
    refresh_status(
        shell,
        &turns.context,
        turns.prepared.usage().as_ref(),
        redactor,
    );

    Turned::Ran(lines_of(&ran))
}

/// Put [ADR-0013] D6's and [ADR-0012] D7's numbers on the status row.
///
/// # The one place the row's two segments are set
///
/// Two records want a number on [ADR-0001] D2's row and ADR-0012's own
/// proposed Update says whoever settles that should settle it in one change
/// rather than "leave two arcs writing to one line". This is that one place:
/// both segments are written here, together, from the two data the session
/// already holds, so neither can be updated without the other being
/// considered.
///
/// # When it is called, and why nothing changes mid-turn
///
/// At session open, and at the end of every turn. Those are the moments the
/// numbers can change, and each is a record's:
///
/// - **Context usage changes only at a turn boundary.** [ADR-0013] D7:
///   "Compaction happens at turn boundaries only". `ContextPolicy::assemble`
///   takes `&self` and `Context::compact` takes `&mut self`, so a context
///   *cannot* change while a turn is in flight — the number would be the same
///   number however often it were re-read.
/// - **Token usage is D7's "per turn in the status line".** The client
///   replaces its slot on every exchange, so a mid-turn read would show a
///   per-*exchange* number where the record says per-turn, which is a reading
///   an implementation would be making rather than a record.
///
/// So the beat repaints these values without recomputing them, and
/// [`crate::terminal::source::TICK`]'s "nothing on the pane changes on a bare
/// tick" stays true of the status row as well.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub fn refresh_status(
    shell: &mut Shell,
    context: &crate::compose::SessionContext,
    tokens: Option<&crate::providers::TokenUsage>,
    redactor: &(dyn zaru_core::redaction::Redactor + Sync),
) {
    shell.set_context_usage(Some(crate::cli::render::context_usage(
        context.usage(redactor),
    )));
    // `render::usage` and not a second spelling: this is the same function the
    // session prints on exit, so the row and that line cannot disagree about a
    // word -- the argument `terminal::vocabulary::turn_line` already makes for
    // the pane and the resumed transcript.
    shell.set_token_usage(tokens.map(crate::cli::render::usage));
}

/// What the race broke out with, before the borrow of the pane ends.
///
/// Generic over what the raced future produced, which is what lets [`race`]
/// be driven by a check that has no provider: the mechanism is the loop, and
/// the loop does not care what it is racing.
#[derive(Debug, PartialEq, Eq)]
pub enum Raced<T> {
    /// The future finished.
    Ran(T),
    /// The user left while it was running. The future was dropped.
    Interrupted(zaru_tui::shell::Leaving),
    /// The terminal stopped answering.
    SourceEnded,
}

/// Wait on `running`, on the terminal, and on a beat, until one of them wins.
///
/// # This is the whole of the asynchronous terminal source's mechanism
///
/// [`run_a_turn`] hands it a turn; a check hands it a future it staged, which
/// is what lets the loop be exercised without a provider, a key or a network.
/// The mechanism and what it happens to be racing are two things, and only one
/// of them can be got wrong.
///
/// **`biased`, so the polling order is a decision rather than a coin.** The
/// future first — one that is ready is not made to wait behind a beat that is
/// also ready — then the terminal, then the beat. Tokio's default is to pick a
/// random ready branch, and a check over a random instrument is not a check
/// (library verification-lessons §57).
pub async fn race<S: Surface + Send, P: Pace + Sync, T>(
    pane: &std::sync::Mutex<Pane<'_, S>>,
    source: &Source,
    pace: &P,
    entries: &dyn zaru_tui::composer::Entries,
    now: &mut Duration,
    running: impl Future<Output = T>,
) -> Raced<T> {
    let mut running = core::pin::pin!(running);
    loop {
        tokio::select! {
            biased;

            ran = &mut running => break Raced::Ran(ran),

            input = source.next() => {
                let Some(input) = input else { break Raced::SourceEnded };
                if let Some(leaving) = zaru_tui::shell::leaves(&input) {
                    break Raced::Interrupted(leaving);
                }
                *now += Duration::from_millis(1);
                read_while_busy(pane, input, *now, entries);
            }

            () = pace.elapse() => {
                // The beat. Nothing on the pane changes because of it -- see
                // `TICK` -- and it is what turns a suspended future into a
                // surface that is still alive rather than one that has
                // stopped.
                if let Ok(mut pane) = pane.try_lock() {
                    pane.paint();
                }
            }
        }
    }
}

/// One keystroke read while a turn is running.
///
/// # Neither lost nor executed as a task
///
/// The keystroke reaches [`zaru_tui::composer::Composer`] and the pane is
/// painted, so a user typing during a turn sees their text and
/// [ADR-0002] D8's standing tip yields on it — "Typing dismisses a tip
/// instantly — no fade, no delay", which is [ADR-0005] D1's strip being "a
/// pure function of composer state".
///
/// **`Enter` is intercepted before the composer**, for two reasons that point
/// the same way. `Composer::key` would insert a newline into the text area,
/// because [`Shell::key`] is what reads `Enter` as a submission and this is
/// not that call. And ADR-0015's ruling says a task typed while a turn is
/// running is refused with a notice, not queued — see [`BUSY`].
///
/// A pane the beat could not lock is a defect this counts rather than one it
/// hangs on, which is [`Pane`]'s own argument; the keystroke is dropped in
/// that case and the count is asserted zero.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
fn read_while_busy<S: Surface + Send>(
    pane: &std::sync::Mutex<Pane<'_, S>>,
    input: zaru_tui::shell::Input,
    now: Duration,
    entries: &dyn zaru_tui::composer::Entries,
) {
    let Ok(mut pane) = pane.try_lock() else {
        return;
    };
    if input.key == zaru_tui::shell::Key::Enter {
        pane.note(Line::new(Register::Announced, BUSY));
    } else {
        pane.shell.composer_mut().key(input, now, entries);
        pane.paint();
    }
}

/// Run the shell against a terminal until the user leaves.
///
/// # What a command does here is what the subcommand does outside
///
/// [ADR-0015] D2: "**A namespace has two entry points, and they are one
/// operation.**" So a slash command is mapped onto the same [`Request`] the
/// out-of-session parser produces and executed by the same [`crate::cli::Run`],
/// and its lines go onto the pane instead of to standard output. There is no
/// second implementation of any command, which is what makes the two spellings
/// one operation rather than two things that agree today.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the pump wants eight distinct capabilities and each is a port or a value \
    some record owns -- the shell, the surface it paints on, the terminal's \
    keys, the beat, the command runner, the composer's entries, the \
    vocabulary and the session's turns. Bundling them would be a second name \
    for the same list, which is the argument `compose::turn::run_one` already \
    makes for its own"
)]
pub async fn run<S: Surface + Send, P: Pace + Sync>(
    shell: &mut Shell,
    surface: &mut S,
    source: &Source,
    pace: &P,
    runner: &crate::cli::Run<'_>,
    entries: &dyn zaru_tui::composer::Entries,
    vocabulary: &dyn zaru_tui::shell::CommandVocabulary,
    turns: &mut Turnable<'_>,
) -> std::io::Result<Pump> {
    let mut now = Duration::ZERO;
    surface.draw(shell)?;

    while let Some(input) = source.next().await {
        // The shell holds no clock, so the pump supplies one. A keystroke is
        // one tick, which is enough for the composer's debounce to be ordered
        // and is not a wall clock -- ADR-0005's whole reason for taking `now`
        // as an argument.
        now += Duration::from_millis(1);

        match shell.key(input, now, entries, vocabulary) {
            Action::Idle => {}
            Action::Leave(leaving) => {
                surface.draw(shell)?;
                return Ok(Pump {
                    exit: exit_for(leaving),
                });
            }
            Action::Run(command) => {
                for line in dispatch(runner, &command) {
                    shell.notice(line);
                }
            }
            Action::Task(task) => {
                // ADR-0008 D1: turns are the outer loop's unit, so a second
                // task in the same session is the next turn. The session stays
                // open whatever the turn did -- a turn that failed is not a
                // reason to close the thing the user is inside. **A user who
                // left in the middle of one is a different thing**, and that
                // is the one way out of this arm.
                let lines = match turns {
                    Turnable::Ready(turns) => {
                        match run_a_turn(
                            shell, surface, source, pace, entries, &mut now, turns, &task,
                        )
                        .await
                        {
                            Turned::Ran(lines) => lines,
                            Turned::Interrupted(leaving) => {
                                surface.draw(shell)?;
                                return Ok(Pump {
                                    exit: exit_for(leaving),
                                });
                            }
                            // The terminal stopped answering mid-turn. A
                            // product terminal does not; a script does, and
                            // this is what stops a pump that never left from
                            // hanging a check.
                            Turned::SourceEnded => {
                                return Ok(Pump {
                                    exit: Exit::Succeeded,
                                });
                            }
                        }
                    }
                    Turnable::Cannot(lines) => lines.clone(),
                };
                for line in lines {
                    shell.notice(line);
                }
            }
        }
        surface.draw(shell)?;
    }

    // The event source ran out without the user leaving. A product terminal
    // does not do this -- crossterm blocks -- and a check does, which is what
    // stops a pump that never returns from hanging one.
    Ok(Pump {
        exit: Exit::Succeeded,
    })
}

/// [ADR-0016] D5's code for a user who asked to leave and left.
fn exit_for(leaving: zaru_tui::shell::Leaving) -> Exit {
    debug_assert_eq!(leaving.code(), 0);
    Exit::Succeeded
}

/// Map one slash command onto the request its subcommand spelling produces.
///
/// # What the fall-through covers, stated because it is a wildcard
///
/// **Not a namespace.** The shell has already refused every namespace this
/// build does not implement, saying so, before anything reaches here — so a
/// namespace with no arm below would be a built one, and the only way to
/// arrive at the fall-through is a verb-and-argument shape that names no
/// request. `zaru providers keys add <kind>` is the one that does today, and
/// deliberately: it reads the key from standard input, which a terminal in raw
/// mode has taken.
///
/// The rest is a match over the namespaces this build implements, in the same
/// discipline `cli::help` uses for its summaries. **It is not compiler-checked
/// and that is the honest reading**: an eleventh built namespace added without
/// an arm here would reach the fall-through and be reported as unavailable
/// rather than failing to compile, which `cli::namespace`'s own exhaustive
/// matches would have caught one layer up. A check walks the vocabulary and
/// asserts every built namespace's first verb reaches a request.
pub(crate) fn dispatch(runner: &crate::cli::Run<'_>, command: &Command) -> Vec<Line> {
    let Some(request) = request_for(command) else {
        return vec![Line::new(
            Register::Failed,
            format!(
                "`{}{}` {UNAVAILABLE}",
                command.slash,
                command
                    .verb
                    .map(|verb| format!(" {verb}"))
                    .unwrap_or_default()
            ),
        )];
    };

    let line = crate::cli::invocation::CommandLine {
        request,
        overrides: crate::cli::invocation::Overrides::default(),
    };
    let outcome = runner.execute(&line);
    let mut lines: Vec<Line> = outcome
        .lines
        .into_iter()
        .map(|text| Line::new(Register::Plain, text))
        .collect();
    if let Exit::Failed(classified) = &outcome.exit {
        let presentation = crate::failure::Presentation::of(classified);
        lines.push(Line::new(Register::Failed, presentation.headline));
    }
    lines
}

/// Which request a slash command names, deciding nothing and doing nothing.
///
/// **Separate from [`dispatch`] because a check has to be able to ask this
/// question without answering it.** The first form of the coverage check below
/// walked the vocabulary through `dispatch`, which *executes* — and `/init` is
/// [ADR-0009](https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators)
/// D6's writer, the one command on this surface that changes a file the user
/// owns. It wrote a `zaru.toml` into this repository the first time the suite
/// ran, and it was found by reading `git status` rather than by any verdict.
/// A pure mapping is the seam that makes the question answerable without the
/// side effect, and it is a better shape besides: what a slash spelling
/// *means* and what running it *does* are two things.
pub(crate) fn request_for(command: &Command) -> Option<Request> {
    match (command.slash, command.verb) {
        ("/runtime", None) => Some(Request::Runtime),
        ("/models", None) => Some(Request::Models),
        ("/init", None) => Some(Request::Init),
        ("/notes", Some("tokens")) => Some(Request::NotesTokens),
        // `providers keys` lists; `providers keys add <kind>` reads the key
        // from standard input, which a shell has taken. So the listing is
        // reachable inside a session and the write is not, and that is a
        // property of the surface rather than an omission: ADR-0007's own
        // reason for reading a key from stdin is that an argument is in the
        // shell history and in `ps`, and a terminal in raw mode has no stdin
        // to hand it.
        ("/providers", Some("keys")) if command.words.is_empty() => Some(Request::ProviderKeys),
        ("/session", Some("list")) => Some(Request::SessionsList),
        ("/session", Some("rm")) => command
            .words
            .first()
            .and_then(|word| crate::session::SessionId::parse(word).ok())
            .map(|id| Request::SessionsRemove { id }),
        ("/config", Some("explain")) => command
            .words
            .first()
            .and_then(|word| crate::config::Key::new(word).ok())
            .map(|key| Request::ConfigExplain { key }),
        // ADR-0010 D4's two in-session spellings. Resuming from inside a
        // session is a different operation from resuming into one, and no
        // record says what it does to the session you are already in, so it
        // is refused rather than answered.
        ("/session", Some("resume" | "continue")) => None,
        _ => None,
    }
}

/// The lines a [`crate::compose::Ran`] leaves on the pane.
///
/// Both halves come through here: the refusal a session that could not
/// resolve a provider shows a task, and what a turn that ran produced.
/// [ADR-0016] D1's classification is rendered through
/// [`crate::failure::Presentation`], the same function the out-of-session
/// surface writes to standard error, so the sentence a user reads in the pane
/// is the sentence they would have read in a pipe.
///
/// # It takes no runner, and that signature is the fix rather than a style
///
/// Until 2026-09-05 the pane's task arm was `refuse_a_task(runner)`, which built a
/// `Request::Task { words: Vec::new() }` and **executed it** through
/// [`crate::cli::Run`]. That was written by the arc that opened this shell,
/// when `Request::Task` was itself a refusal and executing it was how the
/// pane got the refusal's own sentence. The arc that wired a provider client
/// into [ADR-0008] D1's loop made `Request::Task` **run a turn**, and this
/// call site was not revisited: on a machine holding a provider key, typing
/// anything at this prompt started a real session, asked a model an **empty**
/// prompt, and constructed [`crate::tools::prompt::Prompt`] over a standard
/// input the terminal was already holding in raw mode.
///
/// So the seam is the same one `request_for`, two functions down, already uses
/// against the same class of accident — named in prose rather than linked,
/// because it is `pub(crate)` and rustdoc's `private_intra_doc_links` is right
/// to refuse a public page pointing at something its reader cannot open: **a function with nothing to execute cannot execute
/// anything**. What a typed line *means* and what running it *does* are two
/// things, and only the second belongs anywhere near a `Run`. What runs a turn
/// now is [`run_a_turn`], over a session that already exists.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
pub fn lines_of(ran: &crate::compose::Ran) -> Vec<Line> {
    let mut lines: Vec<Line> = ran
        .lines
        .iter()
        .map(|text| Line::new(Register::Plain, text.clone()))
        .collect();
    if let Exit::Failed(classified) = &ran.exit {
        let presentation = crate::failure::Presentation::of(classified);
        lines.push(Line::new(Register::Failed, presentation.headline));
        lines.extend(presentation.lines.into_iter().map(|line| {
            Line::new(
                Register::Plain,
                match line.lead {
                    Some(lead) => format!("{lead} {}", line.text),
                    None => line.text,
                },
            )
        }));
    }
    lines
}

/// The product terminal: `ratatui` over crossterm, reached through `ratatui`'s
/// own re-export so no manifest names crossterm.
///
/// **Nothing in this workspace's checks constructs one**, because a check has
/// no terminal to put into raw mode. What the checks hold is the pump, the
/// guard and every adapter; what this type adds is the three system calls, and
/// that is stated rather than implied.
pub struct Crossterm {
    terminal: ratatui::DefaultTerminal,
}

impl Crossterm {
    /// Take the terminal: raw mode, the alternate screen, and the panic hook
    /// that gives both back.
    ///
    /// `ratatui::try_init` installs that hook itself, which is why this is the
    /// call rather than a hand-rolled sequence: a hook written here would be a
    /// second answer to a question the library already answers, and the two
    /// would have to be kept agreeing.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be put into raw mode or the alternate screen
    /// cannot be entered.
    pub fn take() -> std::io::Result<Self> {
        Ok(Self {
            terminal: ratatui::try_init()?,
        })
    }
}

impl Restore for Crossterm {
    fn restore(&mut self) {
        ratatui::restore();
    }
}

impl Surface for Crossterm {
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()> {
        self.terminal
            .draw(|frame| shell.render(frame, frame.area()))?;
        Ok(())
    }
}
