// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A terminal a check can own, and a beat that never sleeps.
//!
//! [`Recording`] implements the same two methods
//! [`Crossterm`](super::driver::Crossterm) does — draw and restore — over
//! ratatui's `TestBackend`. What it establishes is the pump and the guard;
//! what it establishes nothing about is raw mode and the alternate screen,
//! which are system calls a check has no terminal to make. **Keys are
//! [`Source::scripted`](super::source::Source::scripted)'s**, not this type's:
//! the reader and the painter are two things since 2026-09-05, which is what
//! lets a turn and the terminal be waited on at once.
//!
//! [`Held`] is the other half of that determinism. Every check in this module
//! paces itself through a [`Pace`] that counts and returns rather than one
//! that sleeps, so no check here can pass or fail on how the machine happened
//! to schedule — library verification-lessons §57, a mutation that only raises
//! a defect'"'"'s probability is not an instrument.

use crate::terminal::driver::{Restore, Surface};
use crate::terminal::source::Pace;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zaru_tui::shell::{Input, Key, Shell};

/// How many times a restorer was asked to give the terminal back.
///
/// Shared with the check rather than read off the surface afterwards, so a
/// surface that was dropped can still be counted.
///
/// **`Arc<AtomicUsize>` rather than `Rc<Cell<usize>>` since 2026-09-05.** A
/// turn borrows the shell and the terminal together behind a `Mutex`, which is
/// `Sync` only if what it holds is `Send` — so the product's `Crossterm` and
/// this fixture must both be, and an `Rc` is the one thing here that was not.
/// Nothing about what this counts changed.
pub(crate) type Restores = Arc<AtomicUsize>;

/// A terminal made of a buffer, and every frame it painted.
pub(crate) struct Recording {
    terminal: Terminal<TestBackend>,
    restores: Restores,
    /// Every frame painted, oldest first, as rows.
    pub(crate) frames: Vec<Vec<String>>,
}

impl Recording {
    /// A terminal that paints into a buffer and counts its restores.
    pub(crate) fn of(restores: Restores) -> Self {
        Self::wide(restores, 72)
    }

    /// The same, at a chosen width.
    ///
    /// A pane truncates each line to its width, so a check whose subject is a
    /// marking near the end of a long line is otherwise asserting the default
    /// 72 rather than the renderer. Used by
    /// `an_out_of_tree_call_renders_distinctly_on_the_frame_at_yolo`, whose
    /// line carries a scratch directory's absolute path.
    pub(crate) fn wide(restores: Restores, width: u16) -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(width, 16)).expect("test terminal"),
            restores,
            frames: Vec::new(),
        }
    }
}

impl Restore for Recording {
    fn restore(&mut self) {
        self.restores.fetch_add(1, Ordering::SeqCst);
    }
}

impl Surface for Recording {
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()> {
        self.terminal
            .draw(|frame| shell.render(frame, frame.area()))?;
        let buffer = self.terminal.backend().buffer();
        self.frames.push(
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect(),
        );
        Ok(())
    }
}

/// A beat that returns at once and counts, so no check waits on a clock.
///
/// The asynchronous half yields to the runtime before returning, which is what
/// makes it a real suspension point the `select!` can be woken at rather than
/// a branch that is always ready — a beat that never yielded would starve the
/// turn'"'"'s own future and the check would measure the starvation.
#[derive(Debug, Default)]
pub(crate) struct Held(std::sync::atomic::AtomicUsize);

impl Pace for Held {
    fn wait(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        self.0.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now()
    }
}

/// A restorer that does nothing but count, for the guard's own checks.
pub(crate) struct Counting(pub(crate) Restores);

impl Restore for Counting {
    fn restore(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// One ordinary keystroke.
pub(crate) fn press(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

/// Type a line and send it.
pub(crate) fn typed(text: &str) -> Vec<Input> {
    let mut keys: Vec<Input> = text.chars().map(|ch| press(Key::Char(ch))).collect();
    keys.push(press(Key::Enter));
    keys
}
