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
use crate::providers::ProviderKind;
use crate::session::Resumed;
use crate::terminal::source::{Pace, Source, Taken};
use crate::tools::port::Question;
use core::time::Duration;
use zaru_core::iteration::Interruption;
use zaru_core::redaction::Redactor;
use zaru_core::tool_call::Start;
use zaru_tui::shell::port::{Confirmation, Line, Register};
use zaru_tui::shell::{Action, Command, Palette, Queued, Shell, Struck};

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
    /// Whether the person left, or asked to be somewhere else.
    pub outcome: Pumped,
}

/// The two ways a pump ends.
///
/// # Why there is a second one
///
/// [ADR-0010] D4: "**Inside a session the same operation is `/session resume
/// <id>` and `/session continue`** — one operation with two entry points."
/// Outside a session that operation *puts the person inside the named
/// session*, so inside one it does the same thing, which is a switch. Both
/// verbs refused with a full sentence until 2026-09-06, and they were the
/// in-session half the record had already settled.
///
/// Two states and no `Option`, so a caller cannot ignore the second: a pump
/// whose switch was dropped would leave the user in the session they asked to
/// leave, saying nothing.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug)]
pub enum Pumped {
    /// The person left. The process exits with this.
    Left(Exit),
    /// The person asked to be in another session, already resolved.
    ///
    /// Resolved rather than named, because the lookup can fail and its
    /// refusal has to reach the pane: see the `Action::Run` arm of [`run`].
    ///
    /// # `saying` exists because a switch drops this session's notices
    ///
    /// A shell's notices are this session's own lines — a refusal, a command's
    /// output — and they are deliberately not the transcript, so the shell the
    /// switch opens is built without them. That is right for
    /// `/session resume` and `/session continue`, where the person asked to be
    /// somewhere else and the lines they are leaving belong where they left.
    ///
    /// It is wrong for the one switch nobody asked for. A key stored inside a
    /// session re-opens it so the next turn can use the key — see
    /// [`KEY_IS_STORED`] — and a re-open that dropped the two lines saying the
    /// key was stored would leave a person who had just typed a credential
    /// looking at a pane that said nothing at all about it. So the lines cross
    /// the switch, and `/session resume` and `/session continue` carry none.
    Switch {
        /// The session to open.
        to: crate::session::SessionId,
        /// Lines to put on the new shell as it opens. Empty for a switch the
        /// person asked for.
        saying: Vec<Line>,
    },
}

/// [ADR-0010] D4's interruption, held by a resumed session and told once.
///
/// # What this holds and what it deliberately does not
///
/// D4: "An interrupted tool call is recorded as `Interrupted` **and the model
/// is told it did not complete**." The first half is
/// [`crate::session::resume()`]'s and has been built since `session-lifecycle`:
/// a `Phase::Started` with no `Completed` or `Refused` closing it **is** the
/// interruption, because a killed process writes nothing. The second half is
/// the turn's, and until 2026-09-05 nothing in any product tree started one —
/// [`Start::Resumed`] appeared twice in this repository and both were in test
/// trees, which is [ADR-0010]'s own standing gap, written there by
/// `session-restore`: "reachable from an outside caller and from no door a
/// person can open".
///
/// **This is the carrier and not a second derivation.** [`Self::of`] reads
/// [`Resumed::interrupted`], which `session::resume` already produced, and
/// converts it through [`crate::session::Interrupted::for_the_model`], which
/// is the one redaction seam [ADR-0008] clause 6 already enumerates. Nothing
/// here scans a transcript, and nothing here writes one.
///
/// # Once, and the mechanism is that taking it empties it
///
/// [`Self::tell_once`] moves the value out, so a second call answers `None`
/// whatever the caller does — the shape [`crate::tools::SessionNotice`]'s
/// `state_once` already uses for [ADR-0011] D2's line, and for the same
/// reason: a rule enforced by a type is not a rule anybody has to keep.
///
/// **It is once per resumed process rather than once ever, and that is
/// forced.** The interruption is derived from the transcript's shape and
/// nothing closes the pair — writing a `Completed` or an `Interrupted` record
/// for a call that never finished would author an event that did not happen,
/// which is exactly what [ADR-0010] D2's replayability claim forbids. So a
/// session resumed twice whose resumed turn called no tool is told twice.
/// Where that turn *did* call a tool, `session::resume`'s "one in flight at a
/// time" clears the older pending call and the second resume tells nothing.
///
/// # It is not one of [ADR-0002] D8's once-ever lines
///
/// D8 governs a **recommendation**: output the harness volunteers to the
/// *user*, "appended to the end of the triggering turn", budgeted at one per
/// session. This is none of those. It is addressed to the **model**, it is a
/// turn's *start* rather than a line appended to a turn, and it is caused by
/// the user's own act of resuming rather than volunteered — so it needs no
/// `Record::Said` and takes no row in `once-ever-counter`'s witness.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Default)]
pub struct Pending(Option<Interruption>);

impl core::fmt::Debug for Pending {
    /// Says whether one is owed and never what it is.
    ///
    /// The line is redacted by the time it is here, so this is not a secrets
    /// argument — it is [`Turns`]'s own: a `Debug` is what ends up in a panic
    /// message, and a session's command line is the session's rather than the
    /// panic's. A check asserts that a planted value reaches no `Debug` on
    /// this path, and that check has to be able to fail.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pending")
            .field("owed", &self.is_owed())
            .finish()
    }
}

impl Pending {
    /// What a resumed session owes the model, if it owes anything.
    ///
    /// `None` for a session whose every call closed — including one the user
    /// **refused**, which closes the pair exactly as a completion does:
    /// [ADR-0016]'s ruling of 2026-09-04 is that a refusal is not a failure,
    /// and `session::resume` records that it is not an interruption either.
    /// Telling the model that a call the user consciously declined did not
    /// complete would say the opposite of what happened.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn of<R: Redactor + ?Sized>(resumed: &Resumed, redactor: &R) -> Self {
        Self(
            resumed
                .interrupted
                .as_ref()
                .map(|interrupted| interrupted.for_the_model(redactor)),
        )
    }

    /// A session that owes nothing.
    ///
    /// What [`crate::compose::turn::task`] has, and it is a **fact rather than
    /// a default**: that path mints a session directory and runs turn one in
    /// it, so nothing was in flight because nothing has happened yet. The same
    /// argument [`crate::session::AlreadySaid::none`] makes for its own.
    #[must_use]
    pub const fn none() -> Self {
        Self(None)
    }

    /// Whether a turn still owes the model this. A reader for a check, not a
    /// second copy of the state.
    #[must_use]
    pub const fn is_owed(&self) -> bool {
        self.0.is_some()
    }

    /// The interruption, once. `None` on every call after the first.
    pub fn tell_once(&mut self) -> Option<Interruption> {
        self.0.take()
    }
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
    /// What this session owes the model about a call that never completed.
    ///
    /// [ADR-0010] D4, and the same source as `next` above: `session::resume`
    /// read the transcript once when the shell opened. See [`Pending`].
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub interrupted: Pending,
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
            .field("interrupted", &self.interrupted)
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

    /// Add to the answer arriving and paint, so a reader watches it grow.
    ///
    /// The provisional line is the shell's; this is the beat that makes it
    /// visible. It is taken away by [`Shell::clear_streaming`] when the turn
    /// ends and the turn's own rendered lines arrive — see that method for
    /// why the answer is painted once, from one place.
    fn stream(&mut self, text: &str, meter: Option<&Meter<'_>>) {
        self.shell.stream_delta(text);
        self.tick(meter);
    }

    /// Read [ADR-0028] D5's meter onto the status row, then paint.
    ///
    /// **The read is here rather than at the two call sites** so that a branch
    /// which paints cannot forget it: every repaint during a turn goes through
    /// one function, which is the shape [`Drop`] below already uses for the
    /// streamed line and for the same reason.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    fn tick(&mut self, meter: Option<&Meter<'_>>) {
        if let Some(meter) = meter {
            meter.refresh(self.shell);
        }
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

/// Ending the turn's borrow takes the provisional streamed line away.
///
/// # Why this is a `Drop` and not a line in [`run_a_turn`]
///
/// It was a line in `run_a_turn` first, and a mutation deleting it reddened
/// **nothing**: `run_a_turn` needs a real provider to reach, so no offline
/// check can drive it, and the shell-level check could only prove that
/// `clear_streaming` works — never that anything called it. A property whose
/// only guarantee is that somebody remembered to write one line is the shape
/// this workspace keeps replacing with a shape that cannot be forgotten.
///
/// A [`Pane`] borrows the shell for exactly the length of one turn, so the
/// end of that borrow *is* the end of the turn. Clearing here means the
/// provisional line cannot outlive the turn that produced it, and the
/// ordering the answer depends on — cleared **before** the turn's rendered
/// lines are added — is the borrow checker's rather than a comment's, because
/// those lines cannot be added while the pane still holds the shell.
impl<S: Surface + Send> Drop for Pane<'_, S> {
    fn drop(&mut self) {
        self.shell.clear_streaming();
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

/// [ADR-0028] D3's subscriber: the **inner** loop's narrative, in the pane.
///
/// # What this is for, and why it is not [`PaneSink`]
///
/// [ADR-0028] D1 has iteration state changes "surface as plain-English events
/// inline in the conversation: what was tried, what failed, what changed, what
/// succeeded", and D3 has the harness render "the loop's typed events per
/// [ADR-0008] D3". Those are the **eight** events of
/// [`zaru_core::iteration`], and until 2026-09-05 nothing subscribed to them:
/// [`crate::compose::Inner`] handed `iteration::run` the transcript writer
/// alone, so the whole narrative was reachable only by `zaru --resume <id>`
/// after the fact. What a person watched while a run was being paid was its
/// first line and its last.
///
/// It is a second type rather than a second `impl` on [`PaneSink`] because
/// the two are borrowed differently and at the same time. `PaneSink` rides
/// `compose::turn::run_one`'s `extra` slice as `&mut`, for the whole turn;
/// this rides [`crate::compose::Narrator`] as `&`, inside that same turn.
/// One value cannot be both. They are two handles on one
/// [`std::sync::Mutex`], which is the shape [`crate::compose::sink`] already
/// documents for the transcript's two writers, and the pane they paint into
/// is the same pane.
///
/// # The wording is `vocabulary::loop_line`'s
///
/// Named in prose rather than linked, because it is `pub(crate)` and
/// rustdoc's `private_intra_doc_links` is right to refuse a public page
/// pointing at something a reader of that page cannot open — the same reason
/// `Vocabulary` names `config::nearest` in prose.
///
/// It is the same function the **resumed** pane renders a `Record::Loop` through, so
/// what a user watches while a run happens and what they read back on
/// `--resume` cannot disagree about a word. That is the rule [`PaneSink`]
/// already follows for the outer loop, and the reason is [ADR-0008] D3's:
/// "**Rendering never reads loop internals.** If the terminal needs something
/// to display, the loop emits it."
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
pub struct PaneNarrator<'m, 'a, S: Surface + Send> {
    pane: &'m std::sync::Mutex<Pane<'a, S>>,
    contended: std::sync::atomic::AtomicUsize,
}

impl<S: Surface + Send> core::fmt::Debug for PaneNarrator<'_, '_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaneNarrator")
            .field("contended", &self.contended())
            .finish_non_exhaustive()
    }
}

impl<'m, 'a, S: Surface + Send> PaneNarrator<'m, 'a, S> {
    /// A narrator over a borrowed pane.
    pub const fn over(pane: &'m std::sync::Mutex<Pane<'a, S>>) -> Self {
        Self {
            pane,
            contended: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many events the lock refused.
    ///
    /// Zero by construction — the whole turn is polled on one thread by
    /// [`crate::compose::turn`]'s current-thread runtime, so nothing else can
    /// hold the pane while an event arrives — and counted rather than assumed,
    /// so a check can assert the zero instead of the argument for it. Atomic
    /// because [`crate::compose::Narrator`] takes `&self`; see that trait for
    /// why it must.
    #[must_use]
    pub fn contended(&self) -> usize {
        self.contended.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<S: Surface + Send> crate::compose::Narrator for PaneNarrator<'_, '_, S> {
    fn narrate(&self, event: &zaru_core::iteration::Event) {
        match self.pane.try_lock() {
            Ok(mut pane) => pane.note(crate::terminal::vocabulary::loop_line(event)),
            Err(_) => {
                self.contended
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// The one line an interrupt-and-stay paints.
    ///
    /// [`Register::Announced`], and not [`Register::Failed`], for the reason
    /// `ToolRefused` already carries — named in prose rather than linked,
    /// because the constant that carried it here, `BUSY`, was deleted when a
    /// mid-turn `Enter` stopped being refused: an interruption is the user's
    /// own decision rather than one of [ADR-0016] D1's five classes, and "a
    /// refusal that is
    /// a decision … is rendered in whatever register it renders a decision in,
    /// and never in the error one". [ADR-0016]'s own reading is that an
    /// interruption is not a failure.
    ///
    /// The wording is [`crate::compose::prose::INTERRUPTED`], where the other
    /// lines a person reads live and where the account of it being authored
    /// is. Nothing is composed here.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    fn announce_interrupted(&self) {
        match self.pane.try_lock() {
            Ok(mut pane) => pane.note(Line::new(
                Register::Announced,
                crate::compose::prose::INTERRUPTED,
            )),
            Err(_) => {
                self.contended
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
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
                Taken::Struck(Struck::Key(input)) => input,
                // A paste is absorbed for the reason a key is: ADR-0011 D3's
                // `ask` "prompts before any write or command", and a prompt a
                // user can paste past is no more a prompt than one they can
                // type past. `Shell::pasted` refuses one too, so this arm is
                // the same rule reached by the other door -- and it is an arm
                // rather than a wildcard so that a third kind of event cannot
                // arrive here already decided.
                Taken::Struck(Struck::Pasted(_)) => {
                    pane.paint();
                    self.pace.wait();
                    continue;
                }
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
    /// The user interrupted it, and the turn's future was dropped. The
    /// session stays open and the next typed line is the next turn.
    ///
    /// **It carries [`crate::compose::Narrated`], which cannot be built
    /// outside [`crate::compose::iterate`]**, so this variant cannot be
    /// produced without the pane having been told — see
    /// [`crate::compose::Narrator::interrupted`] for the mutation that
    /// survived until it did.
    Interrupted(crate::compose::Narrated),
    /// The terminal stopped answering. A product terminal does not do this.
    SourceEnded,
}

/// What the pump does with a turn that ended.
///
/// **Separate from [`run`] for the reason `request_for` is separate from
/// `dispatch`: a check has to be able to ask this question without answering
/// it.** Those two are `pub(crate)` and named in prose rather than linked,
/// because rustdoc is right to refuse a public page pointing at something a
/// reader of that page cannot open — the rule [`PaneNarrator`] already
/// records. Reaching the arm through `run` needs a [`Turns`], which needs a
/// [`Prepared`](crate::compose::Prepared), which needs a provider client and a
/// key — so the one decision that says whether a session survives its own
/// interruption would be reachable only from a machine holding a credential.
/// It is a total function over three variants instead, and
/// `an_interrupted_turn_is_the_one_ending_the_pump_carries_on_from` walks all
/// three.
#[derive(Debug)]
pub enum AfterTurn {
    /// The session stays open. These lines go on the pane.
    Carries(Vec<Line>),
    /// The session is over and the process exits with this.
    Stops(Exit),
}

/// Which of [`AfterTurn`]'s two a finished turn is, and what it leaves owed.
///
/// **Exhaustive with no wildcard arm**, so a fourth [`Turned`] cannot arrive
/// without somebody deciding whether it ends the session.
///
/// The interrupted arm is the 2026-09-06 ruling: `Ctrl-C` mid-turn stops the
/// turn and the session stays, where until then it returned a [`Pump`] and the
/// process left. It carries no lines because the narrator has already painted
/// the one there is — see [`crate::compose::prose::INTERRUPTED`] — and because
/// everything the turn itself painted is already on the pane.
///
/// # The re-derivation is here rather than in [`run_a_turn`], and that is a
/// mutation's doing
///
/// [ADR-0010] D4's carrier is `resumed-turn`'s [`Pending`], built by
/// [`crate::terminal::open`] from `session::resume` when the shell opens.
/// Before 2026-09-06 that was enough, because an interrupt ended the process;
/// an interrupt that keeps the session has to derive it again, in this
/// process, from the transcript the drop just left.
///
/// It was written inside `run_a_turn` first, and **a mutation deleting it
/// reddened nothing**: that function needs a
/// [`Prepared`](crate::compose::Prepared), which needs a provider client and a
/// key, so no offline check can drive it. This function needs a
/// [`Session`](crate::session::Session), a [`Pending`] and a redactor, all of
/// which a check can build — so the rule lives where it can be falsified. It
/// is the same finding [`Pane`]'s `Drop` records, with the same answer.
///
/// `session::resume` is the same reader `terminal::open` used, so there is one
/// derivation rather than two, and what it finds is whatever the drop left: a
/// `Phase::Started` with no `Phase::Completed`, if a call was in flight.
///
/// **A read that fails leaves nothing owed rather than ending the session.**
/// The interruption is on disk either way, so the next `--resume` derives it
/// again — `resumed-turn`'s own once-per-process argument — and closing a
/// session the user did not ask to leave, in order to report a transcript that
/// will be read again in a moment, is the worse answer.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub fn after<R: Redactor + ?Sized>(
    turned: Turned,
    owed: &mut Pending,
    session: &crate::session::Session,
    redactor: &R,
    queued: &mut Option<Queued>,
) -> AfterTurn {
    match turned {
        Turned::Ran(lines) => AfterTurn::Carries(lines),
        Turned::Interrupted(_) => {
            // **A queued task is discarded**, and that follows from what this
            // key already means rather than being a second rule for it.
            // `Ctrl-C` stops the turn, and a queued task is the *next* turn of
            // the turn being stopped. Discarding costs nothing anybody can
            // lose, because it was never a record: nothing was written for it
            // and [ADR-0010] D2's producers are untouched.
            *queued = None;
            *owed = crate::session::resume(session.directory(), 0)
                .map(|resumed| Pending::of(&resumed, redactor))
                .unwrap_or_default();
            AfterTurn::Carries(Vec::new())
        }
        // The terminal stopped answering mid-turn. A product terminal does
        // not; a script does, and this is what stops a pump that never left
        // from hanging a check.
        Turned::SourceEnded => AfterTurn::Stops(Exit::Succeeded),
    }
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
    start: Start<'_>,
) -> Turned {
    let n = turns.next;
    turns.next += 1;

    // What [ADR-0013] D1's layer 6 records as this turn's own side of the
    // exchange, taken off the start rather than from a second argument: the
    // task the user typed, or the interrupted call's own rendered line, which
    // is the same bytes `compose::context` assembled the prompt over. A second
    // string here would be a second description of one turn.
    let said = match &start {
        Start::Task(task) => (*task).to_owned(),
        Start::Resumed(interrupted) => interrupted.call().to_owned(),
    };

    let mut tools = crate::compose::ToolLines::default();

    // The answer's text on its way to the pane. Handed to the client here
    // rather than at `prepare`, because this is the first moment there is
    // somewhere to paint: `zaru "<task>"` builds the same client, never calls
    // this, and streams nothing.
    let (sender, mut deltas) = tokio::sync::mpsc::unbounded_channel();
    turns.prepared.client().stream_deltas_to(sender);

    // ADR-0028 D5's meter, started with the turn. `prepared` is copied out of
    // `turns` here because `run_one` takes `&mut turns.owed` and
    // `&mut turns.context` below, and the reader borrows the provider client
    // for as long as the race runs. The clock is this turn's own -- see
    // `Meter` for why it is not `run_one`'s.
    let prepared = turns.prepared;
    let clock = zaru_core::iteration::SystemClock::started_now();
    let reported = move || prepared.usage();
    let meter = Meter::started(&clock, &reported);

    let outcome: Result<crate::compose::Ran, Turned> = {
        let pane = std::sync::Mutex::new(Pane::of(shell, surface));
        let confirm = PaneConfirm::over(&pane, source, pace);
        let mut sink = PaneSink::over(&pane);
        let mut extra: [&mut dyn zaru_core::tool_call::EventSink; 2] = [&mut sink, &mut tools];
        // ADR-0028 D3's subscriber for the inner loop. A second handle on the
        // same `Mutex`, because `extra` above holds `sink` as `&mut` for the
        // whole turn and this is reached by shared reference inside it -- see
        // `PaneNarrator`.
        let narrator = PaneNarrator::over(&pane);

        let raced = race(
            &pane,
            source,
            pace,
            entries,
            now,
            Some(&mut deltas),
            Some(&meter),
            crate::compose::turn::run_one(
                turns.version,
                turns.report_at,
                turns.resolution,
                turns.prepared,
                turns.session,
                n,
                start,
                Some(&confirm as &(dyn crate::tools::Confirm + Sync)),
                &mut extra,
                Some(&narrator as &dyn crate::compose::Narrator),
                &mut turns.owed,
                &mut turns.context,
            ),
        )
        .await;
        // ADR-0028 D3's subscriber says the narrative stopped, and it says it
        // **here** because this is where it is still alive: the pane's borrow
        // ends with this block and the narrator holds the lock it needs.
        //
        // The witness travels out of the block *inside* the value, so there is
        // no `Option` and no `expect` between the announcement and the arm
        // that reports the interruption. A version that carried it in an
        // `Option` compiled with the announcement deleted and panicked at run
        // time instead, which is a rule the type system was not holding.
        match raced {
            Raced::Ran(ran) => Ok(ran),
            Raced::Interrupted => Err(Turned::Interrupted(crate::compose::Narrator::interrupted(
                &narrator,
            ))),
            Raced::SourceEnded => Err(Turned::SourceEnded),
        }
    };
    // The provisional streamed line is already gone: `Pane`'s `Drop` took it
    // when the block above ended the turn's borrow of the shell. See that
    // impl for why it is there rather than here, and `Shell::stream_delta`
    // for why the line is cleared rather than promoted.
    // ADR-0028 D5's meter is a turn's, so it comes off the row with the turn.
    // **On every path**, including the interrupted one below: a stopped clock
    // left on the row would go on saying how long something took that is no
    // longer happening. What a finished turn took is already on the pane in
    // the narrative's own line, which is why this is cleared rather than
    // promoted -- `Shell::stream_delta`'s argument, one field over.
    shell.set_elapsed(None);
    let tool_lines = tools.taken();
    let redactor = turns.prepared.redactor();

    // What an interruption owes the next turn is `after`'s, not this
    // function's: this one needs a provider to reach, so a rule stated here is
    // a rule no offline check can drive.
    let ran = match outcome {
        Ok(ran) => ran,
        Err(turned) => return turned,
    };

    // ADR-0013 D1's layer 6, so the next turn assembles over this one, and
    // then ADR-0010 D3's checkpoint over that, so the next *process* does
    // too. Both acts are `compose::boundary`'s rather than this file's: the
    // out-of-session turn owes exactly the same pair, and a rule with two
    // callers lives in one place — which matters most here because building
    // the exchange is where ADR-0008 clause 6's `Redactor` is applied, and
    // that clause's enumeration names the file that applies it.
    turns
        .context
        .record(crate::compose::boundary::exchange_of_turn(
            redactor,
            &said,
            &tool_lines,
            &ran.lines.join("\n"),
        ));
    // A checkpoint that could not be written is the session losing its memory
    // of a turn that happened, so it is said and the session stays open — the
    // turn's answer is already painted and reporting only the failure would
    // discard it. ADR-0016 D6's "partial success is reported as partial".
    let lost = crate::compose::boundary::checkpointed(&turns.context, turns.session).err();

    // ADR-0013 clause 5 and ADR-0012 clause 6, both on ADR-0001 D2's row.
    // **After the record above**, so the number the user reads is the context
    // the *next* turn will assemble over rather than the one this turn saw --
    // which is what D6's "a number that has been visible all along" means for
    // somebody about to type again. A compaction made at the next turn's
    // boundary shows here at the end of that turn, by the same rule.
    //
    // **And after the checkpoint, on both of its outcomes.** A turn whose
    // checkpoint failed still recorded its exchange, so the row would
    // otherwise be one turn stale on exactly the path where the user is being
    // told something went wrong.
    refresh_status(
        shell,
        &turns.context,
        turns.prepared.usage().as_ref(),
        Some(Described::of(turns.prepared)),
        redactor,
    );

    let mut lines = lines_of(&ran);
    if let Some(failure) = lost {
        lines.push(Line::new(
            zaru_tui::shell::port::Register::Failed,
            format!("{failure}"),
        ));
    }
    Turned::Ran(lines)
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
/// - **Token usage was D7's "per turn in the status line", and until
///   2026-09-06 this paragraph read that "a mid-turn read would show a
///   per-*exchange* number where the record says per-turn, which is a reading
///   an implementation would be making rather than a record".** It is a
///   record's now: [ADR-0028] D5's Update of that day gives the row a meter
///   that reads this number on the beat, and what it shows is unchanged — the
///   last exchange, which is what `Provider::usage` reports. No sum was added;
///   ADR-0012 D7's accumulating total is still that record's author's.
///
/// So the beat repaints the context figure without recomputing it, and
/// [`crate::terminal::source::TICK`]'s sentence about nothing changing on a
/// bare tick is true of that figure and no longer true of the row — see
/// [`Meter`], which is what changed and where.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub fn refresh_status(
    shell: &mut Shell,
    context: &crate::compose::SessionContext,
    tokens: Option<&crate::providers::TokenUsage>,
    described: Option<Described<'_>>,
    redactor: &(dyn zaru_core::redaction::Redactor + Sync),
) {
    shell.set_context_usage(Some(crate::cli::render::context_row(
        context.usage(redactor),
    )));
    // `render::usage_row` and not a second spelling: its full form is the same
    // function the session prints on exit, so the row and that line cannot
    // disagree about a word -- the argument `terminal::vocabulary::turn_line`
    // already makes for the pane and the resumed transcript.
    shell.set_token_usage(tokens.map(crate::cli::render::usage_row));
    // ADR-0012 D4's model and ADR-0011 D3's mode, written here because this is
    // the one place the row is written and because writing them from a second
    // place is exactly what ADR-0012's own Update warned about. Both are
    // immutable for the session, so a second call writes the same two values.
    shell.describe(
        described.and_then(|described| crate::cli::render::model_row(described.model)),
        described.map(|described| crate::cli::render::mode_row(described.mode)),
    );
}

/// [ADR-0028] D5's meter: what the row says while a turn is running.
///
/// # The two numbers that move, and the one that must not
///
/// D5 reads "Per-iteration elapsed time, token counts, and cost render **as
/// the work proceeds**", and its 2026-09-06 Update gives the clause a carrier
/// for a turn that runs no iterations at all — where the loop emits nothing
/// between the turn's first line and its last, which is every session on a
/// machine with no `zaru.toml`. This is that carrier, read on
/// [`crate::terminal::source::TICK`]'s beat.
///
/// - **The elapsed figure** is a difference of two [`Clock`] readings, the
///   first taken when the turn started. `Clock` is `zaru-core`'s existing port
///   — no new port and no new dependency — and its product implementation is
///   the only thing in that crate that reads the machine's clock, which is
///   what lets every check here state an exact figure instead of asserting
///   about wall-clock time.
/// - **The token count** is whatever `Provider::usage` most recently reported,
///   which is **the last exchange** and deliberately not a sum. A turn holding
///   several exchanges shows it rise as each completes.
///   [`crate::compose::Prepared::usage`] carries why: ADR-0012 D7's
///   accumulating total is on that record's human-owned list, and "a caller
///   that summed here would settle it silently".
///
/// **[ADR-0013] D6's context figure is deliberately out of reach.** D7 of that
/// record confines compaction to turn boundaries, so the number could not
/// change mid-turn even if something re-read it — and this type holds nothing
/// that could. The constraint is a shape rather than a rule anybody keeps.
///
/// # Its clock is not `run_one`'s, and they measure different spans
///
/// [`crate::compose::turn::run_one`] starts a `SystemClock` of its own for the
/// loop's per-iteration figures. This one starts when the *turn* does and runs
/// until it ends, which is the span the person is waiting on and is strictly
/// wider. Both are monotonic offsets from `Instant` on one machine, so they
/// cannot drift; they are two measurements of two things rather than two
/// answers to one question.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
/// [`Clock`]: zaru_core::iteration::Clock
pub struct Meter<'a> {
    clock: &'a dyn zaru_core::iteration::Clock,
    started: Duration,
    /// What the provider has reported so far, re-read on every beat.
    ///
    /// A closure rather than a port: the one product reader is
    /// `Prepared::usage`, a check stages whatever it wants to see rise, and a
    /// trait here would be a third name for a question `Provider` already
    /// answers.
    reported: &'a dyn Fn() -> Option<crate::providers::TokenUsage>,
}

impl core::fmt::Debug for Meter<'_> {
    /// Names what it is and reads no clock to do it.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Meter")
            .field("started", &self.started)
            .finish_non_exhaustive()
    }
}

impl<'a> Meter<'a> {
    /// Start measuring a turn now, by this clock.
    pub fn started(
        clock: &'a dyn zaru_core::iteration::Clock,
        reported: &'a dyn Fn() -> Option<crate::providers::TokenUsage>,
    ) -> Self {
        Self {
            started: clock.now(),
            clock,
            reported,
        }
    }

    /// Put both numbers on the row.
    ///
    /// The wording is `terminal::vocabulary::seconds` and
    /// [`crate::cli::render::usage_row`], both handed across rather than
    /// spelled again: the running figure and the narrative's own `· 3.92s`
    /// are one renderer, and the row's token segment and the session-exit line
    /// are another, so neither pair can disagree about a word.
    pub fn refresh(&self, shell: &mut Shell) {
        shell.set_elapsed(Some(crate::terminal::vocabulary::seconds(
            self.clock.now() - self.started,
        )));
        shell.set_token_usage(
            (self.reported)()
                .as_ref()
                .map(crate::cli::render::usage_row),
        );
    }
}

/// The two fields a session knows about itself and never changes.
///
/// [ADR-0012] D4's resolved model and [ADR-0011] D3's permission mode are each
/// fixed for the life of a session by `Prepared`, so they are handed to the
/// row **once** and have no mutation surface — the discipline
/// `Shell::set_context_usage` already argues for the tier. They travel
/// together because they are written together and because neither answers a
/// clause on its own: both are [operations/harness-look-and-feel] row 14.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [operations/harness-look-and-feel]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel
#[derive(Debug, Clone, Copy)]
pub struct Described<'a> {
    /// What [ADR-0012] D4's `default` alias resolved to.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub model: &'a crate::providers::ModelId,
    /// [ADR-0011] D3's mode.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub mode: crate::tools::Mode,
}

impl<'a> Described<'a> {
    /// What a prepared session says about itself.
    #[must_use]
    pub const fn of(prepared: &'a crate::compose::Prepared) -> Self {
        Self {
            model: prepared.model(),
            mode: prepared.mode(),
        }
    }
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
    /// The user interrupted it. The future was dropped.
    ///
    /// **It carries no [`Leaving`](zaru_tui::shell::Leaving), and that is the
    /// 2026-09-06 ruling rather than a simplification.** Mid-turn the key does
    /// not leave — it stops the turn and the session stays — so a value naming
    /// *how the user left* would be describing something that did not happen.
    /// `zaru_tui::shell::leaves` is still the one place the key is spelled;
    /// what *stop* means is the caller's, which is the sentence that function
    /// already carried about the interruption being the host's business.
    Interrupted,
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
#[allow(
    clippy::too_many_arguments,
    reason = "\
    every argument is a port or a value some record owns, and the eighth is \
    ADR-0028 D5's meter -- bundling any of them would be a second name for a \
    list `run_a_turn` already carries under the same reason"
)]
pub async fn race<S: Surface + Send, P: Pace + Sync, T>(
    pane: &std::sync::Mutex<Pane<'_, S>>,
    source: &Source,
    pace: &P,
    entries: &dyn zaru_tui::composer::Entries,
    now: &mut Duration,
    deltas: Option<&mut tokio::sync::mpsc::UnboundedReceiver<String>>,
    meter: Option<&Meter<'_>>,
    running: impl Future<Output = T>,
) -> Raced<T> {
    let mut running = core::pin::pin!(running);
    let mut deltas = deltas;
    loop {
        tokio::select! {
            biased;

            ran = &mut running => break Raced::Ran(ran),

            struck = source.next() => {
                let Some(struck) = struck else { break Raced::SourceEnded };
                // The one rule, called from its second caller. Mid-turn it
                // stops the turn; at the prompt `Shell::key` turns the same
                // answer into `Action::Leave`. One key, one meaning -- stop --
                // and what stop does is where it was pressed. **A paste is
                // never asked**: `leaves` is about a key, and a block whose
                // bytes happened to contain one is text.
                if let Struck::Key(input) = &struck
                    && zaru_tui::shell::leaves(input).is_some()
                {
                    break Raced::Interrupted;
                }
                *now += Duration::from_millis(1);
                read_while_busy(pane, struck, *now, entries);
            }

            // The answer's text, as the provider hands it over.
            //
            // **Below the terminal in the biased order, deliberately.** A
            // model that answers in many small frames would otherwise be able
            // to starve a keystroke, and a person who cannot type while the
            // answer scrolls past has a worse terminal than one whose text
            // lags a beat. Above the beat, because a delta is a reason to
            // repaint and the beat is what happens when there is none.
            Some(delta) = next_delta(&mut deltas) => {
                // A pane the delta could not lock is text the reader misses
                // for one beat, not a turn that fails: `try_lock` for the
                // same reason the beat uses it, and the answer is painted
                // whole at the turn's end regardless.
                if let Ok(mut pane) = pane.try_lock() {
                    pane.stream(&delta, meter);
                }
            }

            () = pace.elapse() => {
                // The beat, and since 2026-09-06 the one thing on the screen
                // that moves: ADR-0028 D5's meter is read here -- see `TICK`,
                // whose own sentence about nothing changing on a bare tick
                // this replaced. It is also what turns a suspended future into
                // a surface that is still alive rather than one that stopped.
                if let Ok(mut pane) = pane.try_lock() {
                    pane.tick(meter);
                }
            }
        }
    }
}

/// The next delta, or a future that never completes when nothing is watching.
///
/// `select!` needs a future on every branch every time round, and a surface
/// with no provider streaming to it has no receiver to wait on. A pending
/// future is the branch saying "not me, ever" without the loop needing a
/// shape for its absence.
async fn next_delta(
    deltas: &mut Option<&mut tokio::sync::mpsc::UnboundedReceiver<String>>,
) -> Option<String> {
    match deltas {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

/// One keystroke or paste read while a turn is running.
///
/// # Neither lost nor executed as this turn's task
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
/// not that call. And [ADR-0015]'s 2026-09-13 amendment says what a submission
/// means here: **the line is queued as the next turn's**, replacing whatever
/// was queued before, and the prompt is cleared because the text moved —
/// exactly as a typed `Enter` at the prompt moves it. That amendment reverses
/// this record's own 2026-09-05 ruling, under which the line was refused with
/// a notice and left on the input row.
///
/// Re-ask the fast tier what the strip should say when it has nothing.
///
/// # Why this is asked again rather than set once
///
/// [ADR-0005]'s 2026-09-05 Update makes the honest line something the composer
/// is **handed**, because "whether a token exists and which workspace is
/// attached are the host's knowledge". `terminal::open` still hands it in at
/// session open and that is still right. What changed on 2026-09-14 is that
/// the answer stops being fixed for the life of a session: the corpus is
/// fetched **after** the shell opens — a population against a real server was
/// measured at one to two seconds, and blocking the first frame on that is one
/// to two seconds of dead terminal — so the line is "still looking" while it
/// runs and something else when it lands.
///
/// **Asked on every input rather than on a clock**, and that is the whole
/// trick: the strip only matters when somebody is typing, and a keystroke is
/// exactly the moment it is about to be repainted anyway. There is no timer
/// here, no second repaint path, and nothing that fires while the terminal is
/// idle.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
fn refresh_absence(shell: &mut zaru_tui::shell::Shell, entries: &dyn zaru_tui::composer::Entries) {
    shell.composer_mut().set_absence(entries.absence());
}

/// A pane the beat could not lock is a defect this counts rather than one it
/// hangs on, which is [`Pane`]'s own argument; the keystroke is dropped in
/// that case and the count is asserted zero.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
fn read_while_busy<S: Surface + Send>(
    pane: &std::sync::Mutex<Pane<'_, S>>,
    struck: Struck,
    now: Duration,
    entries: &dyn zaru_tui::composer::Entries,
) {
    let Ok(mut pane) = pane.try_lock() else {
        return;
    };
    // The same re-ask the outer loop makes, so a corpus that lands while the
    // model is thinking reaches the strip the person is typing into rather
    // than waiting for the turn to end. Above the match rather than inside one
    // arm, because all three repaint the composer and the queued-task arm is
    // the one where a person is most likely to be waiting on something.
    refresh_absence(pane.shell, entries);
    match struck {
        // A block pasted during a turn lands in the prompt exactly as one
        // pasted at it does, and waits for the `Enter` that submits it.
        Struck::Pasted(text) => {
            pane.shell.composer_mut().paste(&text, now, entries);
            pane.paint();
        }
        Struck::Key(input) if input.key == zaru_tui::shell::Key::Enter => {
            // Exactly one is queued, and a second `Enter` replaces it. An
            // empty prompt queues nothing: there is no task in it, and a
            // queued nothing would be a row saying a turn was coming that
            // never arrives.
            let line = pane.shell.composer().text();
            if !line.trim().is_empty() {
                *pane.shell.composer_mut() = zaru_tui::composer::Composer::new();
                pane.shell.queue(Queued::of(line));
            }
            pane.paint();
        }
        Struck::Key(input) => {
            pane.shell.composer_mut().key(input, now, entries);
            pane.paint();
        }
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

    // What a turn left queued, waiting to be submitted without a keystroke.
    //
    // **The loop takes its action from here first, and only then from the
    // terminal**, which is the whole of how ADR-0015's 2026-09-13 amendment
    // gets "submitted the moment the running turn ends, without a keystroke"
    // for nothing: while this is `Some` the loop never reaches `source.next`,
    // so no key is waited for and none is consumed. And it goes through
    // `Shell::submit`, which is the same function `Shell::key`'s `Enter` arm
    // calls, so a queued line naming a namespace runs that command and a
    // queued leaving word leaves -- there is no second grammar to keep
    // agreeing with the first.
    let mut pending: Option<String> = None;

    loop {
        // ADR-0005 D8's honest degradation, re-asked because the answer moves
        // during the session. See `refresh_absence`. Above the action rather
        // than inside the terminal arm, so a queued task submitted without a
        // keystroke -- ADR-0015's 2026-09-13 amendment -- repaints a strip
        // that is as current as one a keystroke reached.
        refresh_absence(shell, entries);

        let action = match pending.take() {
            Some(line) => shell.submit(&line, vocabulary),
            None => {
                let Some(struck) = source.next().await else {
                    break;
                };
                // The shell holds no clock, so the pump supplies one. A
                // keystroke is one tick, which is enough for the composer's
                // debounce to be ordered and is not a wall clock -- ADR-0005's
                // whole reason for taking `now` as an argument.
                now += Duration::from_millis(1);
                shell.struck(struck, now, entries, vocabulary)
            }
        };

        match action {
            Action::Idle => {}
            Action::Leave(leaving) => {
                surface.draw(shell)?;
                return Ok(Pump {
                    outcome: Pumped::Left(exit_for(leaving)),
                });
            }
            Action::Run(command) => {
                // ADR-0010 D4's in-session half, asked before the command is
                // dispatched. A switch is not a `Request` the out-of-session
                // `Run` executes -- outside a session those two verbs are
                // *flags*, because there is no session to be inside -- so it
                // cannot arrive through `request_for`, and `switch_for` is its
                // own pure mapping for the reason that one is: a check has to
                // be able to ask what a spelling means without doing it.
                // ADR-0015 D2's `/providers keys add <kind>`, asked before
                // the command is dispatched for `switch_for`'s own reason: the
                // bytes cannot travel on a `Request`, because ADR-0007 D7
                // reads them from standard input and a terminal in raw mode
                // has none. So the pump stands the question, and what it gets
                // goes to the same storing function the out-of-session
                // spelling reaches.
                if let Some(kind) = secret_for(&command) {
                    let request = zaru_tui::shell::SecretRequest::new(
                        secret_statement(kind),
                        SECRET_GUIDANCE,
                    );
                    match ask_for_a_secret(shell, surface, source, request).await? {
                        Asked::Given(offered) => {
                            let outcome = runner.store_a_provider_key(kind, offered.trim_end());
                            let stored = !matches!(outcome.exit, Exit::Failed(_));
                            let mut said: Vec<Line> = outcome
                                .lines
                                .into_iter()
                                .map(|text| Line::new(Register::Plain, text))
                                .collect();
                            if let Exit::Failed(classified) = &outcome.exit {
                                said.push(Line::new(
                                    Register::Failed,
                                    crate::failure::Presentation::of(classified).headline,
                                ));
                            }
                            if stored {
                                said.push(Line::new(Register::Plain, KEY_IS_STORED));
                                // **A stored key re-opens this session, and
                                // that is not a restart anybody invented.** It
                                // is the switch `/session continue` already
                                // takes, under the 2026-09-06 00:20Z ruling
                                // that re-opening the current session "says
                                // nothing"; the terminal and the runtime are
                                // held across it by `terminal::open::open`, so
                                // there is no flash and no second reader. What
                                // it buys is the whole of why the question
                                // exists: `compose::turn::prepare` runs once,
                                // at the door, so without it the session that
                                // stored the key is the one session on the
                                // machine that cannot use it.
                                //
                                // The id comes off the status line because
                                // this arm is reachable on `Turnable::Cannot`,
                                // which is exactly the keyless case and holds
                                // no session. A session id that will not parse
                                // is not a reason to lose the lines: the arm
                                // below keeps them on this shell instead.
                                if let Ok(id) =
                                    crate::session::SessionId::parse(&shell.status().session)
                                {
                                    surface.draw(shell)?;
                                    return Ok(Pump {
                                        outcome: Pumped::Switch {
                                            to: id,
                                            saying: said,
                                        },
                                    });
                                }
                            }
                            for line in said {
                                shell.notice(line);
                            }
                        }
                        // A declined question is not a failure -- the harness
                        // asked and the user answered -- so it is announced
                        // rather than refused, which is the register ADR-0016
                        // D1 leaves for a decision.
                        Asked::Declined => {
                            shell.notice(Line::new(Register::Announced, SECRET_DECLINED.to_owned()))
                        }
                        // A terminal that stopped answering is how a
                        // scripted source ends, and it is the same exit the
                        // pump's own `source.next()` returning `None` takes.
                        // **Not a stored key and not a declined one**: nothing
                        // was read, so nothing is claimed either way.
                        Asked::Ended => {
                            surface.draw(shell)?;
                            return Ok(Pump {
                                outcome: Pumped::Left(Exit::Succeeded),
                            });
                        }
                    }
                } else if let Some(opening) = switch_for(&command) {
                    // Resolved **here**, where the pane is: a terminal in raw
                    // mode has no echo, so a refusal written to standard error
                    // goes into the alternate screen and the person sees
                    // nothing. `Overrides::default()` is not a loss --
                    // `resolve` reads them only to mint, and a switch never
                    // mints.
                    match crate::terminal::open::resolve(
                        &opening,
                        runner.version,
                        runner.report_at,
                        &crate::cli::invocation::Overrides::default(),
                    ) {
                        Ok(id) => {
                            surface.draw(shell)?;
                            return Ok(Pump {
                                outcome: Pumped::Switch {
                                    to: id,
                                    saying: Vec::new(),
                                },
                            });
                        }
                        Err(exit) => {
                            if let Exit::Failed(classified) = &*exit {
                                shell.notice(Line::new(
                                    Register::Failed,
                                    crate::failure::Presentation::of(classified).headline,
                                ));
                            }
                        }
                    }
                } else {
                    for line in dispatch(runner, &command) {
                        shell.notice(line);
                    }
                }
            }
            Action::Task(task) => {
                // ADR-0010 D2's seventh producer, echoed the moment the person
                // presses Enter.
                //
                // The survey's row 5: the typed text "vanishes from the
                // composer on Enter and **is never rendered anywhere**", so a
                // person scrolling a long session "cannot tell which answer
                // belongs to which question". This is that line, composed by
                // `vocabulary::spoken` -- which is also what paints the record
                // back on `--resume`, so the echo and the replay cannot
                // disagree about a word.
                //
                // **Here rather than inside `run_a_turn`, and the reason is
                // this file's own lesson.** `Pane`'s `Drop` records that a
                // line placed in `run_a_turn` had a mutation deleting it
                // redden **nothing**, because that function needs a real
                // provider and no offline check can drive it: "a property
                // whose only guarantee is that somebody remembered to write
                // one line is the shape this workspace keeps replacing". This
                // arm is reachable without one. It is also the better place on
                // its own merits -- it covers `Turnable::Cannot` too, so a
                // person typing into a session that resolved no provider sees
                // their own line above the refusal rather than a refusal
                // floating over nothing.
                //
                // **Noticed rather than drawn**: the arm below ends in
                // `surface.draw`, and the beat repaints within `TICK`
                // regardless, so this adds no second place a paint can fail.
                //
                // **It paints the line as typed, where the replay paints it
                // redacted**, and that asymmetry is deliberate rather than
                // overlooked. It already exists for the answer -- the streamed
                // deltas reach `Shell::stream_delta` with no redactor anywhere
                // on that path -- so what a person watches has always been
                // their own bytes. A harness that altered a person's own line
                // as they typed it would be the opposite of showing them their
                // work. Accepted 2026-09-06 on ADR-0010 D2, open to Jeshua's
                // veto, with that paragraph on the record rather than only
                // here.
                shell.notice(crate::terminal::vocabulary::spoken(
                    crate::session::Voice::User,
                    &task,
                ));

                // ADR-0008 D1: turns are the outer loop's unit, so a second
                // task in the same session is the next turn. The session stays
                // open whatever the turn did -- a turn that failed is not a
                // reason to close the thing the user is inside. **A user who
                // left in the middle of one is a different thing**, and that
                // is the one way out of this arm.
                // **A user who interrupted a turn has not left**, which is
                // the ruling of 2026-09-06: the narrator has already painted
                // the one line there is, the composer still holds whatever was
                // typed during the turn -- `Shell::key` never ran, so nothing
                // cleared it, which is ADR-0005 D1 -- and the next typed line
                // is the next turn, told first about the call that did not
                // complete. `after` is where that is decided, so a check can
                // ask without a provider.
                let lines = match turns {
                    Turnable::Ready(turns) => {
                        let turned = turns_of_one_line(
                            shell, surface, source, pace, entries, &mut now, turns, &task,
                        )
                        .await;
                        let redactor = turns.prepared.redactor();
                        let session = turns.session;
                        match after(
                            turned,
                            &mut turns.interrupted,
                            session,
                            redactor,
                            shell.queued_mut(),
                        ) {
                            AfterTurn::Carries(lines) => lines,
                            AfterTurn::Stops(exit) => {
                                return Ok(Pump {
                                    outcome: Pumped::Left(exit),
                                });
                            }
                        }
                    }
                    Turnable::Cannot(lines) => lines.clone(),
                };
                for line in lines {
                    shell.notice(line);
                }
                // The turn has ended, so whatever was queued during it is the
                // next one. Taking it is what empties it, so nothing here can
                // run the same task twice.
                pending = shell.take_queued().map(|task| task.task);
            }
        }
        surface.draw(shell)?;
    }

    // The event source ran out without the user leaving. A product terminal
    // does not do this -- crossterm blocks -- and a check does, which is what
    // stops a pump that never returns from hanging one.
    Ok(Pump {
        outcome: Pumped::Left(Exit::Succeeded),
    })
}

/// [ADR-0016] D5's code for a user who asked to leave and left.
fn exit_for(leaving: zaru_tui::shell::Leaving) -> Exit {
    debug_assert_eq!(leaving.code(), 0);
    Exit::Succeeded
}

/// The turns one typed line causes.
///
/// # Why a line can cause two of them
///
/// [ADR-0010] D4: "An interrupted tool call is recorded as `Interrupted` **and
/// the model is told it did not complete**." [`Start::Resumed`] carries no
/// task — a resumed session is not a new instruction — so telling the model
/// cannot be folded into the turn the user asked for: making a user's first
/// typed line a resumed turn would **drop the task**, which is what
/// `shell-task-turns` measured and refused. The telling is therefore a turn of
/// its own, and it goes first.
///
/// **It happens here rather than when the shell opens, and that was the
/// ruling.** ADR-0010's own Status tracking held "whether a shell opened over
/// a session whose last call was interrupted owes the model a task-less turn
/// before the user's first one" as a question for its author; it was decided
/// on 2026-09-05 under directive 20, open to Jeshua's veto, and the amendment
/// is on that record. Running it at the door would spend a provider call on a
/// `--resume` somebody opened to read their session back — and would move the
/// refusal [`crate::terminal::open`] deliberately defers, whose reason it
/// states in its own words: "a person who resumed a session to read it back is
/// not asking for a provider, and refusing before they ask would answer a
/// question they did not put." Here, a session with no provider is still
/// [`Turnable::Cannot`] and no resumed turn ever runs.
///
/// **Once**, because [`Pending::tell_once`] empties itself. The second turn
/// after a resume is [`Start::Task`] again, and so is every turn after that.
///
/// A user who leaves during the resumed turn gets the same three arms every
/// turn has, and the interruption is not lost: nothing closed the pair on
/// disk, so the next `--resume` derives it again from the transcript's own
/// shape. Authoring a record for a call that never finished is what [ADR-0010]
/// D2's replayability claim forbids.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the list `run_a_turn` takes, which is the list `run` takes minus two. \
    Every one is a port or a value some record owns, and bundling them would \
    be a second name for the same list -- the argument \
    `compose::turn::run_one` already makes for its own"
)]
async fn turns_of_one_line<S: Surface + Send, P: Pace + Sync>(
    shell: &mut Shell,
    surface: &mut S,
    source: &Source,
    pace: &P,
    entries: &dyn zaru_tui::composer::Entries,
    now: &mut Duration,
    turns: &mut Turns<'_>,
    task: &str,
) -> Turned {
    let mut lines = Vec::new();

    if let Some(interrupted) = turns.interrupted.tell_once() {
        match run_a_turn(
            shell,
            surface,
            source,
            pace,
            entries,
            now,
            turns,
            Start::Resumed(&interrupted),
        )
        .await
        {
            Turned::Ran(told) => lines.extend(told),
            // The user left, or the terminal stopped answering, during the
            // telling. Both are the pump's way out and neither is this
            // function's to interpret.
            other => return other,
        }
    }

    match run_a_turn(
        shell,
        surface,
        source,
        pace,
        entries,
        now,
        turns,
        Start::Task(task),
    )
    .await
    {
        Turned::Ran(answered) => {
            lines.extend(answered);
            Turned::Ran(lines)
        }
        other => other,
    }
}

/// Map one slash command onto the request its subcommand spelling produces.
///
/// # What the fall-through covers, stated because it is a wildcard
///
/// **Not a namespace.** The shell has already refused every namespace this
/// build does not implement, saying so, before anything reaches here — so a
/// namespace with no arm below would be a built one, and the only way to
/// arrive at the fall-through is a verb-and-argument shape that names no
/// request.
///
/// **Corrected 2026-09-14.** This read: "`zaru providers keys add <kind>` is
/// the one that does today, and deliberately: it reads the key from standard
/// input, which a terminal in raw mode has taken." The first half is no longer
/// true — that spelling is answered by [`secret_for`] and [ADR-0011] D3's
/// masked question, before anything reaches `dispatch` — and the second half
/// was never the whole reason: the bytes still cannot travel on a [`Request`],
/// which is why the question is asked by the pump rather than mapped here.
/// `zaru notes tokens add <alias> <host>` is what arrives at the fall-through
/// today, for the three reasons on `operations/known-defects`.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
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
        return unavailable(command);
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
/// **Separate from `dispatch` because a check has to be able to ask this
/// question without answering it.** The first form of the coverage check below
/// walked the vocabulary through `dispatch`, which *executes* — and `/init` is
/// [ADR-0009](https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators)
/// D6's writer, the one command on this surface that changes a file the user
/// owns. It wrote a `zaru.toml` into this repository the first time the suite
/// ran, and it was found by reading `git status` rather than by any verdict.
/// A pure mapping is the seam that makes the question answerable without the
/// side effect, and it is a better shape besides: what a slash spelling
/// *means* and what running it *does* are two things.
/// Which session a slash command asks to be in, deciding nothing and doing
/// nothing.
///
/// **Separate from [`run`] for the reason `request_for` is separate from
/// `dispatch`**: what a spelling *means* and what running it *does* are two
/// things, and a check has to be able to ask the first without the second —
/// which here would mean minting or opening a session directory. Both are
/// `pub(crate)` and named in prose rather than linked, for the reason
/// [`PaneNarrator`] records.
///
/// [ADR-0010] D4 names both verbs and [ADR-0015] D2 governs both spellings.
/// `/session resume <id>` names one; `/session continue` is this directory's
/// most recent, which is the same sentence `--continue` implements and reaches
/// it through the same [`crate::session::most_recent_in`].
///
/// A `resume` whose word is not a ULID answers `None` and falls through to
/// `dispatch`, which refuses it in the sentence that surface already has —
/// the same shape `("/session", Some("rm"))` already takes.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[must_use]
pub fn switch_for(command: &Command) -> Option<crate::terminal::open::Opening> {
    use crate::terminal::open::Opening;
    match (command.slash, command.verb) {
        ("/session", Some("resume")) => command
            .words
            .first()
            .and_then(|word| crate::session::SessionId::parse(word).ok())
            .map(Opening::Existing),
        ("/session", Some("continue")) if command.words.is_empty() => Some(Opening::MostRecentHere),
        _ => None,
    }
}

/// What the fall-through answers: the whole spelling that was typed, and the
/// out-of-session spelling where one exists.
///
/// # It named a command the reader had not typed, and that was a defect
///
/// Until 2026-09-14 this formatted `command.slash` and `command.verb` and
/// **dropped `command.words`**, so a person who typed `/providers keys add
/// gemini` was answered about `` `/providers keys` `` — which is a spelling
/// that works. Measured from the release binary at `515d854` over a
/// pseudo-terminal and filed on [Known Defects]; a refusal that silently
/// absorbs part of what was typed cannot be acted on, which is
/// [ADR-0016] D2's own test: "an error message whose reader cannot act is a
/// stack trace with better grammar".
///
/// # The remedy is the harness's own knowledge, not a new one
///
/// [ADR-0015] D2's "a namespace has two entry points" means every namespace
/// here has an out-of-session spelling, and
/// [`Namespace::subcommand`](crate::cli::Namespace::subcommand) is where it is
/// already written down. So the second line is a lookup rather than an
/// authored remedy per command, and it appears only where the lookup answers
/// — a namespace this build does not implement at all gets the first line and
/// nothing else, because "run it outside a session" would be false for it.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [Known Defects]: https://100monkeys-ai.cortex.page/zaru/p/operations/known-defects
fn unavailable(command: &Command) -> Vec<Line> {
    let mut lines = vec![Line::new(
        Register::Failed,
        format!("`{}` {UNAVAILABLE}", typed_spelling(command)),
    )];
    if let Some(outside) = out_of_session_spelling(command) {
        lines.push(Line::new(
            Register::Plain,
            format!("  outside a session it is `{outside}`."),
        ));
    }
    lines
}

/// The whole of what the person typed, rebuilt from what the grammar read.
///
/// **Every word, which is the point.** `Command` is what
/// [`zaru_tui::shell::command::read`] produced, so this is the typed line as
/// the harness understood it rather than as the terminal received it — which
/// is the more useful thing to be shown, because a word the grammar dropped is
/// a word the refusal must still name.
fn typed_spelling(command: &Command) -> String {
    let mut spelling = command.slash.to_owned();
    if let Some(verb) = command.verb {
        spelling.push(' ');
        spelling.push_str(verb);
    }
    for word in &command.words {
        spelling.push(' ');
        spelling.push_str(word);
    }
    spelling
}

/// The same command as `zaru …` would spell it, if this build has that
/// namespace at all.
///
/// `None` for a slash this build does not implement, because naming an
/// out-of-session spelling for a namespace that exists nowhere would be a
/// remedy that fails.
fn out_of_session_spelling(command: &Command) -> Option<String> {
    let namespace = crate::cli::Namespace::ALL
        .into_iter()
        .find(|namespace| namespace.slash() == command.slash)?;
    if !namespace.is_built() {
        return None;
    }
    let mut spelling = format!("zaru {}", namespace.subcommand());
    if let Some(verb) = command.verb {
        spelling.push(' ');
        spelling.push_str(verb);
    }
    for word in &command.words {
        spelling.push(' ');
        spelling.push_str(word);
    }
    Some(spelling)
}

/// Which secret a slash command asks for, deciding nothing and doing nothing.
///
/// **Separate from the asking for the reason `request_for` is separate from
/// `dispatch` and [`switch_for`] from [`run`]** — the first two named in prose
/// rather than linked, because they are `pub(crate)` and rustdoc's
/// `private_intra_doc_links` is right to refuse a public page pointing at
/// something its reader cannot open, which is the reading [`PaneNarrator`]
/// already records. What a spelling *means* and what running it *does* are two
/// things, and a check has to be able to ask the first without the second —
/// which here would mean standing a question at a terminal and waiting for
/// somebody to type a key into it.
///
/// It is not a [`Request`], and that is the shape rather than an omission.
/// [ADR-0007] D7 reads a key from **standard input**, precisely because an
/// argument is in the shell's history file and in `ps` output for every user
/// on the machine, and a terminal in raw mode has no standard input to hand
/// it. So the bytes cannot travel on the [`Request`]; what travels is the
/// *kind*, and the bytes are read at [ADR-0011] D3's masked question and
/// handed to `cli::Run::store_a_provider_key` — the same function the
/// out-of-session spelling reaches, named in prose for the reason above.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn secret_for(command: &Command) -> Option<ProviderKind> {
    match (command.slash, command.verb) {
        ("/providers", Some("keys")) => match command.words.as_slice() {
            [add, kind] if add == "add" => ProviderKind::parse(kind),
            _ => None,
        },
        _ => None,
    }
}

/// The sentence the masked question states, naming what is being asked for.
///
/// **Authored under a delegated coordinator ruling of 2026-09-14, open to
/// Jeshua's veto**, and composed here rather than in `zaru-tui` for
/// [ADR-0011] D3's own reason: what the user was told and what the harness
/// believes it asked cannot be allowed to drift apart, so the statement
/// crosses the port as a value.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn secret_statement(kind: ProviderKind) -> String {
    format!("the {kind} API key, which is not shown as you type")
}

/// What follows the masked row: how to finish, and how to decline.
///
/// Authored with [`secret_statement`], under the same ruling. It names both
/// ways out, because a question whose only stated answer is the accepting one
/// is a question a person cannot leave.
pub const SECRET_GUIDANCE: &str = "enter to store it · esc or ctrl-c to cancel";

/// What the pane says when a masked question is declined.
///
/// Authored under the same ruling. **Not [`Register::Failed`]**: the harness
/// asked and the user answered, which is a permission-shaped outcome rather
/// than one of [ADR-0016] D1's five classes -- the reading that record already
/// takes for a declined tool prompt and for ADR-0007 D8's `ApexDeclined`.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const SECRET_DECLINED: &str = "nothing was stored.";

/// What the pane says after a key is stored.
///
/// **Authored under a delegated coordinator ruling of 2026-09-14, open to
/// Jeshua's veto, and it exists because of a measurement.**
/// [`crate::compose::turn::prepare`] runs **once**, when the shell opens — a
/// session's tier, model, boundary, manifest, key, client and [ADR-0012]
/// clause 3 witness do not change between two of its turns, which is
/// [`Turnable`]'s whole reason for having two variants and no third. So a key
/// stored inside a session is on disk and sealed, and the session that stored
/// it was, until this line existed, the one session on the machine that could
/// not use it.
///
/// Measured on the release binary over a pseudo-terminal: the pane said
/// *"stored a `gemini` key under the alias `provider.gemini`."* and the very
/// next task said *"the alias `default` resolves to \"gemini-3.6-flash\" and
/// this machine holds no provider key"*. Two true sentences, one after the
/// other, that a person reads as the harness contradicting itself.
///
/// # The person is not told about `prepare`, which is the ruling
///
/// An earlier shape named `/session continue` and asked them to type it. That
/// is honest and it is a harness explaining its own internals to somebody who
/// came to store a key. The session now re-opens itself through
/// [`Pumped::Switch`] — the switch `/session continue` already takes — and
/// this line says what happened rather than what to do next.
///
/// **It is said on the shell the switch opens**, carried across on
/// [`Pumped::Switch::saying`], because a switch builds a fresh shell and a
/// re-open that dropped it would leave a person who had just typed a
/// credential looking at a pane that said nothing about it.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const KEY_IS_STORED: &str =
    "  this session reopened with it, so the next thing you ask will use it.";

/// How a masked question ended.
///
/// **Three cases and no `Option`**, for [`Taken`]'s own reason: a value given,
/// a person who declined, and a terminal that stopped answering are three
/// different things and a caller does three different things with them.
#[derive(Debug)]
pub enum Asked {
    /// The user typed something and pressed `Enter`.
    Given(String),
    /// The user pressed `Esc` or `Ctrl-C`. Nothing was read.
    Declined,
    /// The terminal stopped answering before the question was.
    Ended,
}

/// Stand [ADR-0011] D3's masked question and pump the terminal until it is
/// answered.
///
/// # It awaits, where [`PaneConfirm`] cannot
///
/// `PaneConfirm::confirm` is synchronous and spins on
/// [`Source::try_next`](crate::terminal::source::Source::try_next) because it
/// runs **inside** a turn's own poll, where there is nothing to await on. This
/// question is raised by the pump itself, between turns, so it awaits
/// [`Source::next`](crate::terminal::source::Source::next) — the `poll_fn` that
/// takes the receiver's lock for one poll and never across a suspension. No
/// beat is consumed and no lock is held.
///
/// # Nothing here reads the value
///
/// The bytes go into the shell and come back out through
/// [`Shell::take_secret`](zaru_tui::shell::Shell::take_secret), which is the
/// one accessor that yields them. This function paints between keystrokes and
/// never inspects what it is painting.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub async fn ask_for_a_secret<S: Surface + Send>(
    shell: &mut Shell,
    surface: &mut S,
    source: &Source,
    request: zaru_tui::shell::SecretRequest,
) -> std::io::Result<Asked> {
    shell.ask_secret(request);
    surface.draw(shell)?;

    let mut now = Duration::ZERO;
    loop {
        // A standing question takes every key and every paste, so the composer
        // is handed nothing and needs neither of these.
        let Some(struck) = source.next().await else {
            return Ok(Asked::Ended);
        };
        now += Duration::from_millis(1);
        shell.struck(struck, now, &NoEntries, &NoVocabulary);
        surface.draw(shell)?;

        match shell.secret_answer() {
            None => {}
            Some(zaru_tui::shell::SecretAnswer::Declined) => return Ok(Asked::Declined),
            Some(zaru_tui::shell::SecretAnswer::Given) => {
                return Ok(shell.take_secret().map_or(Asked::Declined, Asked::Given));
            }
        }
    }
}

pub(crate) fn request_for(command: &Command) -> Option<Request> {
    match (command.slash, command.verb) {
        ("/runtime", None) => Some(Request::Runtime),
        // ADR-0015 D2's `/help` row: the in-session spelling of `--help`,
        // reaching the same request and therefore the same lines.
        ("/help", None) => Some(Request::Help),
        ("/models", None) => Some(Request::Models),
        ("/init", None) => Some(Request::Init),
        // **`if command.words.is_empty()`, exactly as `/providers keys`
        // below.** Without the guard `/notes tokens add work host` reached
        // `Request::NotesTokens` and **ran the listing**, saying nothing about
        // the words it had dropped -- measured 2026-09-14 over a
        // pseudo-terminal. A command that silently runs a different command is
        // worse than one that refuses, because the person who typed it has no
        // way to find out. With the guard it reaches `unavailable`, which
        // names the words and the out-of-session spelling.
        //
        // `tokens add` is not reachable in a session for three measured
        // reasons -- a nested `block_on`, ADR-0007 D8's apex confirmation on
        // `/dev/tty`, and a network call between them -- and they are on
        // `operations/known-defects` rather than here.
        ("/notes", Some("tokens")) if command.words.is_empty() => Some(Request::NotesTokens),
        // ADR-0007 D7's `describe` and `rm`, which are reachable in a session
        // for the reason `use` is and `tokens add` is not: each takes words
        // the person typed on this line and reads nothing from standard
        // input, which a terminal in raw mode has taken.
        //
        // **The description is every word after the alias, joined with one
        // space** -- `Command::words` is the typed line split on whitespace,
        // and out of session the same words arrive already split by the
        // shell. Joining both the same way is what makes ADR-0015 D2's two
        // entry points one operation here; see `Request::NotesTokensDescribe`
        // for the one input the two cannot agree on.
        //
        // Neither validates anything. An alias is `Alias::new`'s to refuse and
        // a description is `Description::new`'s, at the store's door, which is
        // the same door the subcommand goes through -- a grammar that checked
        // either here would be a second answer to a question `zaru-cli`
        // already answers in one place.
        ("/notes", Some("tokens")) if command.words.first().is_some_and(|w| w == "describe") => {
            match command.words.as_slice() {
                [_, alias, text @ ..] if !text.is_empty() => crate::credentials::Alias::new(alias)
                    .ok()
                    .map(|alias| Request::NotesTokensDescribe {
                        alias,
                        text: text.join(" "),
                    }),
                // An alias with no description, or neither. It falls through
                // to `unavailable`, which names what was typed.
                _ => None,
            }
        }
        ("/notes", Some("tokens")) if command.words.first().is_some_and(|w| w == "rm") => {
            match command.words.as_slice() {
                [_, alias] => crate::credentials::Alias::new(alias)
                    .ok()
                    .map(|alias| Request::NotesTokensRemove { alias }),
                _ => None,
            }
        }
        // ADR-0007 D7's fifth surface, and it is reachable inside a session
        // where `tokens add` is not: this takes an alias that is already in
        // the store and reads nothing.
        //
        // **Corrected 2026-09-14.** This used to give "`add` reads the token
        // from standard input and a terminal in raw mode has none to hand it"
        // as the whole reason, which stopped being sufficient the day
        // `/providers keys add <kind>` got a masked question for exactly that
        // problem. `tokens add` stays out for three further reasons measured
        // that day -- a `block_on` inside the shell's own `block_on`, ADR-0007
        // D8's apex confirmation on `/dev/tty`, and a network call between
        // them -- and they are on `operations/known-defects`.
        ("/notes", Some("use")) => command
            .words
            .first()
            .and_then(|word| crate::credentials::Alias::new(word).ok())
            .map(|alias| Request::NotesUse { alias }),
        // `providers keys` lists. **`providers keys add <kind>` is not a
        // `Request` and never reaches here**: ADR-0007's reason for reading a
        // key from standard input is that an argument is in the shell history
        // and in `ps`, so the bytes cannot travel on a `Request` either, and
        // `secret_for` maps that spelling onto ADR-0011 D3's masked question
        // instead. The guard stays, so `keys add …` falls past this arm.
        //
        // **Corrected 2026-09-14.** This read "the listing is reachable inside
        // a session and the write is not, and that is a property of the
        // surface rather than an omission". The write is reachable; what was
        // missing was a way to read a secret at a terminal without echoing
        // it.
        ("/providers", Some("keys")) if command.words.is_empty() => Some(Request::ProviderKeys),
        // The provider half of ADR-0007 D7's `rm`, in session for the reason
        // `keys add` is not: it names a kind and reads nothing. D2's two
        // entry points are one operation, so this reaches the same request
        // the subcommand does.
        ("/providers", Some("keys")) if command.words.first().is_some_and(|w| w == "rm") => {
            match command.words.as_slice() {
                [_, kind] => {
                    ProviderKind::parse(kind).map(|kind| Request::ProviderKeysRemove { kind })
                }
                _ => None,
            }
        }
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
        // ADR-0010 D4's two in-session spellings are not requests: outside a
        // session they are *flags*, because there is no session to be inside.
        // `switch_for` above maps them, and `run` asks it before it dispatches.
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
    /// Whether this terminal paints the registers' colours.
    ///
    /// Held here because the terminal is the thing that knows: it is taken
    /// once per session, and `NO_COLOR` is a fact about the process rather
    /// than about the shell's state. See
    /// [`crate::terminal::open::palette_from_environment`].
    palette: Palette,
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
        let terminal = ratatui::try_init()?;
        // Armed **after** the alternate screen and disarmed before it is left,
        // in this one place, so no exit path can hand a terminal back still
        // telling every later program that a paste is bracketed. If the arm
        // itself fails the terminal is given back before the error leaves, so
        // a half-taken terminal is never returned.
        if let Err(failure) = arm(&mut std::io::stdout()) {
            ratatui::restore();
            return Err(failure);
        }
        // Read here, once, after the terminal is taken and armed: `NO_COLOR`
        // is a fact about the process and the terminal is the thing that
        // knows whether it paints colour.
        let palette = crate::terminal::open::palette_from_environment();
        Ok(Self { terminal, palette })
    }
}

/// Ask the terminal to frame a paste, so its newlines arrive as text.
///
/// # It is a function over a writer so that the bytes are checkable
///
/// [`Crossterm`] cannot be constructed in a check — it is three system calls
/// against a terminal a check does not have — so an `execute!` written inline
/// there would be a rule nothing could falsify. Over a [`std::io::Write`] it
/// is a check with a `Vec<u8>` in it, and what the check asserts is the
/// sequence itself: `ESC[?2004h` here and `ESC[?2004l` in [`disarm`]. The
/// look-and-feel survey's row 13 measured the gap by exactly that string —
/// "`ESC[?2004h` appears nowhere in any capture".
///
/// # Errors
///
/// When the sequence cannot be written to the terminal.
pub(crate) fn arm(out: &mut impl std::io::Write) -> std::io::Result<()> {
    ratatui::crossterm::execute!(out, ratatui::crossterm::event::EnableBracketedPaste)
}

/// Stop asking, on the way out. See [`arm`].
///
/// **A failure here is deliberately not reported.** This runs from
/// [`Restore::restore`], which is reached from [`Guard`]'s `Drop` and
/// therefore from an unwind; a `Drop` that returned a result would have
/// nowhere to return it, and a terminal that will not take this sequence is
/// one that will not take the alternate screen's either, which
/// `ratatui::restore` is about to try anyway.
pub(crate) fn disarm(out: &mut impl std::io::Write) {
    let _ = ratatui::crossterm::execute!(out, ratatui::crossterm::event::DisableBracketedPaste);
}

impl Restore for Crossterm {
    fn restore(&mut self) {
        // Before the alternate screen is left, and on every path `Guard` runs
        // on: an ordinary exit, an early return, and an unwind. `ratatui`'s
        // own panic hook calls `ratatui::restore` and knows nothing about
        // bracketed paste, so this is the only thing that disarms it.
        disarm(&mut std::io::stdout());
        ratatui::restore();
    }
}

impl Surface for Crossterm {
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()> {
        let palette = self.palette;
        self.terminal
            .draw(|frame| shell.render(frame, frame.area(), palette))?;
        Ok(())
    }
}
