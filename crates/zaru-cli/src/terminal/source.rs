// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The terminal's events, read beside the turn rather than instead of it.
//!
//! # The gap this closes, in the words the records used
//!
//! [ADR-0005] and [ADR-0008] both carried the same paragraph until this
//! module existed: "there is no asynchronous terminal source … while the model
//! is thinking the pane does not repaint and no keystroke is read", and a
//! `Ctrl-C` pressed then "is queued by raw mode — which disables the interrupt
//! signal — and acted on when the await returns". Both said what was missing
//! in one phrase: **a source that could be polled beside the turn.** This is
//! it.
//!
//! # Why a thread and a channel rather than `crossterm`'s own stream
//!
//! `crossterm::event::EventStream` is a `futures_core::Stream` and it is
//! **unreachable from this workspace**. It sits behind that crate's own
//! `event-stream` feature, and crossterm is not a dependency here — it arrives
//! as `ratatui::crossterm` through `ratatui`'s `crossterm` feature, which is
//! what [ADR-0003] D2 blessed and what keeps a `crossterm` line out of every
//! manifest. Measured on 2026-09-05 against `ratatui` 0.29.0's own manifest:
//! its features are `all-widgets`, `crossterm`, `macros`, `palette`,
//! `scrolling-regions`, `serde`, `termion`, `termwiz`, `underline-color`, the
//! four `unstable` ones and `widget-calendar` — **there is no `event-stream`
//! passthrough**, and Cargo cannot enable a feature of a transitive optional
//! dependency without naming that dependency directly.
//!
//! So the source is the other shape: `poll` and `read`, which are in the same
//! module the driver already imports, on a thread of their own, feeding a
//! channel. It costs **no manifest line at all** — `tokio`'s `sync` was on
//! this crate before this arc for `crate::compose::shared`'s lock.
//!
//! # The thread cannot outlive the shell, and that is structural
//!
//! A private `Reader` holds the stop flag and the join handle, and its `Drop`
//! sets the flag and joins. The join is bounded by [`POLL`], because that is
//! the longest the thread can be inside `poll` before it looks at the flag
//! again.
//! A thread parked forever in a blocking `read()` would be a thread nothing
//! could end, which is why the loop polls with a timeout rather than reading
//! straight away.
//!
//! # The channel is the seam, so there is no port here
//!
//! [`Source::over_the_terminal`] spawns the thread; [`Source::scripted`] fills
//! a sender and drops it, so the receiver ends where a script ends. **Both
//! produce the same type**, so every check drives the real receiver and the
//! real locking, and what no check covers is the thread body alone — the same
//! "three system calls a check has no terminal to make" argument
//! [`Crossterm`](super::driver::Crossterm) already carries, one type smaller
//! than a port would have been.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop

use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zaru_tui::shell::Input;

/// How long the reader thread waits for an event before looking at the stop
/// flag again.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-05, open to
/// Jeshua's veto**, in the same shape as
/// [`STRIP_ROWS`](zaru_tui::shell::STRIP_ROWS) and the three drafted register
/// glyphs: no record names a number and one is needed, so it is named once
/// here with its reasoning rather than typed at a call site.
///
/// **It does not delay a keystroke.** `poll` returns the instant an event
/// arrives; this bounds only how long the thread can be inside it when the
/// shell is trying to leave, which is the whole of what dropping a [`Source`]
/// waits for. Fifty milliseconds is below [`TICK`], so a user who leaves never
/// waits a visible beat for the terminal to come back, and it is long enough
/// that an idle terminal is not a loop spinning on a syscall.
pub const POLL: Duration = Duration::from_millis(50);

/// How long the pane waits before repainting when nothing has happened.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-05, open to
/// Jeshua's veto**, for the reason [`POLL`] gives.
///
/// A tenth of a second, and the two halves of that: a person reads a response
/// inside about a hundred milliseconds as immediate, so a pane that repaints
/// at this rate is one that never looks stuck while the model is thinking; and
/// ten repaints a second of a terminal-sized cell buffer is work a battery
/// does not notice, where the sixty a frame-rate would ask for is.
///
/// **Two numbers on the status row change on a bare tick, since 2026-09-06.**
/// This paragraph read "**Nothing on the pane changes on a bare tick** — no
/// spinner, no clock, because no record gives either a glyph or a status-line
/// slot", and the second half of that is what stopped being true: [ADR-0028]
/// D5's Update of that day gives the row a slot for an elapsed time and a
/// token count, and [`crate::terminal::driver::Meter`] reads them here. **No
/// spinner and no glyph**, which D1 of that record refuses in as many words —
/// what moves is a number a record names, and nothing else on the pane changes
/// on a bare tick. ADR-0005 D1's strip is still "a pure function of composer
/// state" and ADR-0013 D6's context figure still changes only at a turn
/// boundary.
///
/// What the tick also buys is that a keystroke read during a turn is *shown*
/// at the moment it is read.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
pub const TICK: Duration = Duration::from_millis(100);

/// How long the shell waits when nothing has happened.
///
/// # One concept, two shapes, because the two callers differ in kind
///
/// The pump races the wait against a turn's future and needs it as a future.
/// [`Confirm`](crate::tools::port::Confirm) is synchronous — it is what a tool
/// executor is awaiting, so there is no runtime to hand a future back to — and
/// needs it as a call that returns. Declaring one trait with both is what
/// keeps the *number* in one place; two ports would be two answers to "how
/// long is a beat".
///
/// A check implements this and never sleeps, which is what makes every check
/// in this module deterministic rather than a coin (library verification
/// lessons §57: a mutation that only raises a defect's probability is not an
/// instrument). It is the discipline
/// [`Composer`](zaru_tui::composer::Composer) already carries one layer down —
/// "every method that needs the time takes it, so there is nothing here that
/// could read the machine's clock".
pub trait Pace {
    /// Wait, on this thread, with no runtime.
    fn wait(&self);

    /// Wait, as a future something else can race.
    fn elapse(&self) -> impl Future<Output = ()> + Send;
}

/// [`TICK`], as the product paces itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Beat;

impl Pace for Beat {
    fn wait(&self) {
        std::thread::sleep(TICK);
    }

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(TICK)
    }
}

/// What a synchronous read of the source found.
///
/// Three cases and no `Option`, because "nothing yet" and "never again" are
/// different answers and a caller has to tell them apart: the first is a beat
/// to paint through and the second is a terminal that stopped answering, which
/// [`crate::tools::prompt`]'s rule says is a failure and never a `no`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// A keystroke.
    Key(Input),
    /// Nothing has arrived. The caller waits a beat and asks again.
    Nothing,
    /// The source will produce no more keys, ever.
    Ended,
}

/// The terminal's keystrokes, readable from a turn and from a pump.
///
/// See the module documentation. Every method takes `&self`, because the two
/// readers borrow it at once and neither can hold `&mut`.
pub struct Source {
    receiver: Mutex<UnboundedReceiver<Input>>,
    /// `None` for a scripted source, which has no thread.
    reader: Option<Reader>,
    contended: AtomicUsize,
}

impl core::fmt::Debug for Source {
    /// Names what it is and renders no keystroke it holds.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Source")
            .field("reading_a_terminal", &self.reader.is_some())
            .field("contended", &self.contended.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl Source {
    /// A source that will answer these keys and then end.
    ///
    /// The sender is dropped here, so the receiver reports [`Taken::Ended`]
    /// once the script is drained — which is what a check needs and what a
    /// product terminal never does.
    #[must_use]
    pub fn scripted(keys: Vec<Input>) -> Self {
        let (sender, receiver) = unbounded_channel();
        for key in keys {
            // The receiver is alive on this stack, so a send cannot fail.
            let _ = sender.send(key);
        }
        drop(sender);
        Self {
            receiver: Mutex::new(receiver),
            reader: None,
            contended: AtomicUsize::new(0),
        }
    }

    /// A source reading the real terminal on a thread of its own.
    ///
    /// The thread stops when this value is dropped, within [`POLL`].
    #[must_use]
    pub fn over_the_terminal() -> Self {
        Self::over(read_until_stopped)
    }

    /// A source fed by `body`, on a thread this value owns.
    ///
    /// # Why the body is a parameter rather than the terminal
    ///
    /// The thread's **lifecycle** — the flag, the join, the channel closing
    /// behind it — is the part that can be got wrong and is the part a check
    /// must reach. Reading a terminal is two system calls a check has no
    /// terminal to make. Taking the body as an argument separates the two, so
    /// [`over_the_terminal`](Self::over_the_terminal) is one call and
    /// everything around it is exercised by a body a check wrote.
    ///
    /// `body` returns when the flag it is handed is set, or sooner. A body
    /// that never looked at the flag would hang [`Reader::drop`], which is
    /// what `a_reader_body_that_ignores_the_flag_is_what_drop_waits_for`
    /// exists to state rather than to hide.
    pub(crate) fn over(
        body: impl FnOnce(&UnboundedSender<Input>, &AtomicBool) + Send + 'static,
    ) -> Self {
        let (sender, receiver) = unbounded_channel();
        Self {
            receiver: Mutex::new(receiver),
            reader: Some(Reader::spawn(sender, body)),
            contended: AtomicUsize::new(0),
        }
    }

    /// How many times a reader found the receiver already locked.
    ///
    /// **Zero by construction**, and counted rather than argued: neither
    /// reader holds the lock across a suspension, so neither can be holding
    /// it while the other runs. [`try_next`](Self::try_next) takes it for one
    /// `try_recv` and [`next`](Self::next) for one `poll_recv`, both inside a
    /// single poll. That is the same argument
    /// [`Pane`](crate::terminal::driver::Pane) already makes for its own lock,
    /// and a check asserts the zero rather than the argument for it.
    ///
    /// # The argument this replaced, and the defect it permitted
    ///
    /// It used to read: the pump's branch "is polled only while the turn's
    /// future is suspended", [`crate::terminal::driver::PaneConfirm`] "runs
    /// *inside* that future's poll, so the two are never live at the same
    /// instant". Both halves are true and the conclusion does not follow —
    /// **a branch the `select!` is not polling is a branch that is still
    /// holding**, and a future suspended in `recv().await` holds the guard
    /// precisely while nothing is polling it. `next` did exactly that until
    /// 2026-09-05, so a question raised inside
    /// [`race`](crate::terminal::driver::race) found the receiver locked by
    /// the pump's own branch, every `try_next` failed before reaching the
    /// channel, and the answer a person typed was never read: ADR-0011 D3's
    /// prompt could not be answered at the default mode. Measured at 21
    /// contentions across 21 beats, one per beat, with the key already in the
    /// channel.
    #[must_use]
    pub fn contended(&self) -> usize {
        self.contended.load(Ordering::SeqCst)
    }

    /// The next keystroke if one is already here, without waiting.
    ///
    /// For a caller with no runtime to await on — see [`Pace`].
    pub fn try_next(&self) -> Taken {
        let Ok(mut receiver) = self.receiver.try_lock() else {
            self.contended.fetch_add(1, Ordering::SeqCst);
            return Taken::Nothing;
        };
        match receiver.try_recv() {
            Ok(input) => Taken::Key(input),
            Err(TryRecvError::Empty) => Taken::Nothing,
            Err(TryRecvError::Disconnected) => Taken::Ended,
        }
    }

    /// The next keystroke, waiting for one.
    ///
    /// `None` when the source has ended. A product terminal never ends; a
    /// script does, which is what stops a pump that never leaves from hanging
    /// a check.
    ///
    /// # The lock is held for one poll and never across a suspension
    ///
    /// A `poll_fn` rather than an `async fn`, and that is the whole of why.
    /// [`UnboundedReceiver::poll_recv`] registers the waker and returns within
    /// the poll, so the guard is taken and dropped inside a single `poll` and
    /// there is no instant at which this future is suspended holding it. An
    /// `async fn` awaiting `recv()` under the guard reads the same and is not
    /// the same: `tokio::select!` builds its branch futures once per
    /// invocation and keeps them alive across every poll until a branch wins,
    /// so the pump's branch would hold the receiver for the whole of a
    /// suspended turn — including the poll in which that turn reaches
    /// [`crate::terminal::driver::PaneConfirm`] and needs it. See
    /// [`contended`](Self::contended) for the defect that was.
    ///
    /// The `Mutex` is `std`'s for the same reason. Nothing here may hold it
    /// across an await ever again, and `clippy::await_holding_lock` under the
    /// documentation and lint gates' `-D warnings` says so at the moment
    /// somebody writes one, which an asynchronous mutex would not.
    pub fn next(&self) -> impl Future<Output = Option<Input>> {
        core::future::poll_fn(move |context| {
            let Ok(mut receiver) = self.receiver.try_lock() else {
                self.contended.fetch_add(1, Ordering::SeqCst);
                context.waker().wake_by_ref();
                return core::task::Poll::Pending;
            };
            receiver.poll_recv(context)
        })
    }
}

/// The thread that reads the terminal, and the flag that ends it.
struct Reader {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Reader {
    /// Start reading. The thread ends when [`Reader`] is dropped.
    fn spawn(
        sender: UnboundedSender<Input>,
        body: impl FnOnce(&UnboundedSender<Input>, &AtomicBool) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = std::thread::spawn(move || body(&sender, &flag));
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Reader {
    /// Stop the thread and wait for it, so it cannot outlive the shell.
    ///
    /// The wait is bounded by [`POLL`]. A detached thread would keep the
    /// terminal's event source open after the shell gave the terminal back,
    /// and the next thing to read standard input would be racing it.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            // A reader thread that panicked has already ended, which is what
            // the join is waiting for; there is nothing to report and nothing
            // to do about it here.
            drop(handle.join());
        }
    }
}

/// The thread body: poll, read, send, until the flag is set.
///
/// **Nothing in this workspace's checks runs this function**, for the reason
/// the module documentation gives — it is `poll` and `read` against a terminal
/// a check does not have. Everything around it is checked: the channel, the
/// locking, the flag and the join.
fn read_until_stopped(sender: &UnboundedSender<Input>, stop: &AtomicBool) {
    use ratatui::crossterm::event::{Event, poll, read};

    while !stop.load(Ordering::Acquire) {
        match poll(POLL) {
            Ok(false) => continue,
            // A terminal that will not be polled is a terminal that will not
            // be read either, and there is nobody on this thread to report to.
            // Ending the thread closes the channel, which the pump reads as
            // the source having ended -- the same path a drained script takes.
            Err(_) => return,
            Ok(true) => {}
        }
        let input = match read() {
            Ok(Event::Key(key)) => translate(key),
            // Everything else is redrawn around rather than acted on. A resize
            // changes the regions, which the next draw reads from the frame's
            // own area, so an empty input is the whole response.
            Ok(_) => Input::default(),
            Err(_) => return,
        };
        if sender.send(input).is_err() {
            // The shell dropped the source. Nothing is listening.
            return;
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
fn translate(key: ratatui::crossterm::event::KeyEvent) -> Input {
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
    Input {
        key: code,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    }
}
