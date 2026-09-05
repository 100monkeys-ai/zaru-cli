// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A terminal a check can own.
//!
//! Implements the same three methods [`Crossterm`](super::driver::Crossterm)
//! does — draw, read a key, restore — over a scripted key list and ratatui's
//! `TestBackend`. What it establishes is the pump and the guard; what it
//! establishes nothing about is raw mode and the alternate screen, which are
//! three system calls a check has no terminal to make.

use crate::terminal::driver::{Restore, Surface};
use core::cell::Cell;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::rc::Rc;
use zaru_tui::shell::{Input, Key, Shell};

/// How many times a restorer was asked to give the terminal back.
///
/// Shared with the check rather than read off the surface afterwards, so a
/// surface that was dropped can still be counted.
pub(crate) type Restores = Rc<Cell<usize>>;

/// A terminal made of a script and a buffer.
pub(crate) struct Recording {
    terminal: Terminal<TestBackend>,
    script: std::vec::IntoIter<Input>,
    restores: Restores,
    /// Every frame painted, oldest first, as rows.
    pub(crate) frames: Vec<Vec<String>>,
}

impl Recording {
    /// A terminal that will answer these keys and then stop.
    pub(crate) fn of(keys: Vec<Input>, restores: Restores) -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(72, 16)).expect("test terminal"),
            script: keys.into_iter(),
            restores,
            frames: Vec::new(),
        }
    }
}

impl Restore for Recording {
    fn restore(&mut self) {
        self.restores.set(self.restores.get() + 1);
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

    fn next(&mut self) -> std::io::Result<Option<Input>> {
        Ok(self.script.next())
    }
}

/// A restorer that does nothing but count, for the guard's own checks.
pub(crate) struct Counting(pub(crate) Restores);

impl Restore for Counting {
    fn restore(&mut self) {
        self.0.set(self.0.get() + 1);
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
