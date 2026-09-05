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
/// The statement crosses unchanged. That record's port says the sentence "is
/// composed once, by the decision, and handed here — rather than composed
/// where it is rendered — so that what the user was told and what the harness
/// believes it asked cannot drift apart", and a conversion that reworded it
/// would be the drift that sentence forbids.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn question_for_the_shell(question: &Question) -> Confirmation {
    Confirmation::new(question.statement.clone(), question.prominent)
}

/// What the fall-through in [`dispatch`] says.
///
/// Named once so a check can look for it rather than for a phrase somebody
/// retyped.
pub(crate) const UNAVAILABLE: &str = "needs something this harness does not have yet";

/// What one turn of the pump produced.
#[derive(Debug)]
pub struct Pump {
    /// What the process should exit with.
    pub exit: Exit,
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
pub fn run(
    shell: &mut Shell,
    surface: &mut impl Surface,
    runner: &crate::cli::Run<'_>,
    entries: &dyn zaru_tui::composer::Entries,
    vocabulary: &dyn zaru_tui::shell::CommandVocabulary,
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
            Action::Task(_) => {
                // ADR-0012 D3's provider trait has no implementation in any
                // product tree, so this is the refusal the out-of-session
                // surface already prints, shown in the pane. The session stays
                // open: the user asked for something the harness cannot do,
                // which is not a reason to close the thing they are inside.
                for line in refuse_a_task(runner) {
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

/// The refusal a task gets, in the pane.
fn refuse_a_task(runner: &crate::cli::Run<'_>) -> Vec<Line> {
    let line = crate::cli::invocation::CommandLine {
        request: Request::Task { words: Vec::new() },
        overrides: crate::cli::invocation::Overrides::default(),
    };
    let outcome = runner.execute(&line);
    match &outcome.exit {
        Exit::Failed(classified) => {
            let presentation = crate::failure::Presentation::of(classified);
            let mut lines = vec![Line::new(Register::Failed, presentation.headline)];
            lines.extend(presentation.lines.into_iter().map(|line| {
                Line::new(
                    Register::Plain,
                    match line.lead {
                        Some(lead) => format!("{lead} {}", line.text),
                        None => line.text,
                    },
                )
            }));
            lines
        }
        Exit::Succeeded => Vec::new(),
    }
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
