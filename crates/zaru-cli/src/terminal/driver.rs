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

/// Everything the pump needs from a terminal.
pub trait Surface: Restore {
    /// Paint the shell.
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()>;

    /// The next keystroke, or `None` when the user is finished.
    ///
    /// A product implementation blocks; a check reads from a script and
    /// answers `None` when it runs out, so a pump that never left would hang
    /// a check rather than passing it.
    fn next(&mut self) -> std::io::Result<Option<zaru_tui::shell::Input>>;
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
    pub context: zaru_core::context::Context,
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
pub struct PaneConfirm<'m, 'a, S: Surface + Send> {
    pane: &'m std::sync::Mutex<Pane<'a, S>>,
}

impl<S: Surface + Send> core::fmt::Debug for PaneConfirm<'_, '_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaneConfirm").finish_non_exhaustive()
    }
}

impl<'m, 'a, S: Surface + Send> PaneConfirm<'m, 'a, S> {
    /// A confirmer over a borrowed pane.
    pub const fn over(pane: &'m std::sync::Mutex<Pane<'a, S>>) -> Self {
        Self { pane }
    }
}

impl<S: Surface + Send> crate::tools::port::Confirm for PaneConfirm<'_, '_, S> {
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
            let read = pane.surface.next().map_err(|failure| {
                crate::tools::port::ConfirmFailure::new(format!(
                    "the answer could not be read: {failure}"
                ))
            })?;
            let Some(input) = read else {
                return Err(crate::tools::port::ConfirmFailure::new(
                    "the terminal stopped answering before the question was".to_owned(),
                ));
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

/// Run one turn of this session for `task`, painting it as it happens.
///
/// Returns the lines to leave on the pane. The shell and the surface are
/// borrowed for the length of the turn and given back when it ends.
///
/// # What does not repaint, said rather than smoothed
///
/// The pane paints when the loop emits and when a question is answered, and at
/// no other moment. **During the provider's own await nothing repaints and no
/// keystroke is read**, because this crate has no asynchronous terminal source
/// — [`Surface::next`] blocks. A `Ctrl-C` pressed then is queued by raw mode,
/// which disables the interrupt signal, and is seen when the await returns. A
/// source that could be polled beside the turn is a real gap and is recorded
/// on ADR-0005 and ADR-0008 as one rather than worked around here.
pub fn run_a_turn<S: Surface + Send>(
    shell: &mut Shell,
    surface: &mut S,
    turns: &mut Turns<'_>,
    task: &str,
) -> Vec<Line> {
    let n = turns.next;
    turns.next += 1;

    let ran = {
        let pane = std::sync::Mutex::new(Pane::of(shell, surface));
        let confirm = PaneConfirm::over(&pane);
        let mut sink = PaneSink::over(&pane);
        let mut extra: [&mut dyn zaru_core::tool_call::EventSink; 1] = [&mut sink];

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
        )
    };

    // ADR-0013 D1's layer 6, so the next turn assembles over this one. **What
    // an exchange holds is `context-summariser`'s to declare**, together with
    // what `context.json` really carries; this is the caller that fills it,
    // and it fills it with what this turn was about and what came back, through
    // the one `Redactor` the session already holds — ADR-0008 clause 6's port,
    // on every path from captured bytes into a model prompt.
    let exchange = zaru_core::redaction::Redacted::by(
        turns.prepared.redactor(),
        &format!("user: {task}\nzaru: {}", ran.lines.join("\n")),
    );
    turns
        .context
        .record_exchange(zaru_core::context::Exchange::verbatim(
            exchange.as_str().to_owned(),
        ));

    lines_of(&ran)
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
pub fn run<S: Surface + Send>(
    shell: &mut Shell,
    surface: &mut S,
    runner: &crate::cli::Run<'_>,
    entries: &dyn zaru_tui::composer::Entries,
    vocabulary: &dyn zaru_tui::shell::CommandVocabulary,
    turns: &mut Turnable<'_>,
) -> std::io::Result<Pump> {
    let mut now = Duration::ZERO;
    surface.draw(shell)?;

    while let Some(input) = surface.next()? {
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
                // reason to close the thing the user is inside.
                let lines = match turns {
                    Turnable::Ready(turns) => run_a_turn(shell, surface, turns, &task),
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
/// So the seam is the same one [`request_for`] already uses against the same
/// class of accident: **a function with nothing to execute cannot execute
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

    fn next(&mut self) -> std::io::Result<Option<zaru_tui::shell::Input>> {
        use ratatui::crossterm::event::{Event, read};

        match read()? {
            Event::Key(key) => Ok(Some(translate(key))),
            // Everything else is redrawn around rather than acted on. A resize
            // changes the regions, which the next draw reads from the frame's
            // own area, so an empty input is the whole response.
            _ => Ok(Some(zaru_tui::shell::Input::default())),
        }
    }
}

/// One crossterm key event, as the backend-agnostic input the shell reads.
///
/// # This translation exists because `tui-textarea` is taken on `no-backend`
///
/// That feature is what keeps a terminal backend out of `zaru-tui`'s closure
/// and out of ADR-0005 D3's fast tier, and the cost of it is that the crate
/// ships no `From<KeyEvent>`. So the mapping is here, in the crate that has
/// crossterm, which is where the boundary puts it.
///
/// **Every key the shell reads has an arm and everything else is `Key::Null`.**
/// The shell's own reading is exhaustive over what it acts on -- `Enter`,
/// `Esc`, `y`, `n`, `Ctrl-C` -- and the composer's text area handles the rest;
/// a key with no arm reaches the composer as nothing rather than as something
/// else.
fn translate(key: ratatui::crossterm::event::KeyEvent) -> zaru_tui::shell::Input {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use zaru_tui::shell::Key;

    let code = match key.code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Enter => Key::Enter,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Tab => Key::Tab,
        KeyCode::Delete => Key::Delete,
        KeyCode::Esc => Key::Esc,
        KeyCode::F(n) => Key::F(n),
        _ => Key::Null,
    };
    zaru_tui::shell::Input {
        key: code,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    }
}
