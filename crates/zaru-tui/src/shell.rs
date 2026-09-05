// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The terminal session: a status line, a transcript pane, and the composer.
//!
//! # What this is, and what it deliberately is not
//!
//! This is the in-session surface every record has been waiting on. [ADR-0015]
//! D2's slash half, [ADR-0008] clauses 3 to 5's rendering half, [ADR-0002]
//! D8's standing tip, [ADR-0001] D2's status line and [ADR-0010] D4's
//! re-rendered tail all needed a terminal, and four records say so in as many
//! words — each carrying some form of the sentence "an interactive session
//! that does not exist".
//!
//! **No loop runs here and no provider is reached.** The pane renders lines it
//! is handed through [`TranscriptSource`]; a staged transcript is the fixture.
//! [ADR-0008] clause 3 requires the event stream to be "consumed by both the
//! terminal renderer and the transcript writer, **from one emission**", and
//! one emission needs a loop, so that clause does not move here and this
//! module does not pretend it does.
//!
//! # It holds no terminal either
//!
//! Everything below renders into a `ratatui::Frame` and reads
//! `tui_textarea::Input`, both backend-agnostic. The terminal itself — raw
//! mode, the alternate screen, the crossterm event loop, and the panic hook
//! that restores the terminal — is `zaru-cli`'s, because it is the
//! composition root and because [ADR-0003] D8 lets this crate name only
//! `zaru-core`. So every check here drives `ratatui`'s `TestBackend` and reads
//! cells out of the buffer, and a rendered line quoted from one of them is
//! evidence about the mechanism. **It says nothing about whether a person can
//! read the result**, which is [ADR-0005]'s own "someone has to look at it".
//!
//! # The composer's rows are reserved, and that is [ADR-0005] D2
//!
//! D2: "The strip renders below the input and its height changes never reflow
//! the text the user is composing. The cursor does not move because a search
//! result arrived." Inside the composer's own area that holds by construction
//! — ADR-0005's Update records the input as anchored to the top of that area,
//! so no strip height can move it. **In a shell it needs one thing more**: if
//! the host sized the composer's area by [`Composer::height`] and anchored it
//! to the bottom of the screen, a growing strip would push the input row
//! upward, which is exactly the mutant that record's clause 5 was watched red
//! against.
//!
//! So the shell gives the composer a **fixed** area at the bottom, of
//! [`COMPOSER_ROWS`] rows, and the strip grows downward inside it. The input
//! row is then a function of the terminal's size alone.
//! `the_input_row_is_byte_identical_whatever_the_strip_shows_inside_the_shell`
//! asserts it at strip heights of zero, one and six, which is ADR-0005 clause
//! 5 one layer out.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

pub mod command;
pub mod port;
pub mod render;

pub use command::{Command, LEAVE, Refused, Typed};
pub use port::{CommandVocabulary, Confirmation, Line, Namespace, Register, TranscriptSource};

use crate::composer::{Composer, Entries};
use core::time::Duration;

/// The backend-agnostic keystroke the shell reads, re-exported.
///
/// `tui-textarea` is taken here with `no-backend` on, which is what exposes
/// these two **without** a terminal backend — the same feature that keeps
/// crossterm out of the composer's search tier. They are re-exported because
/// the host translates real terminal events into them, and ADR-0003 D8 lets
/// that host name this crate but not this crate's own dependencies.
pub use tui_textarea::{Input, Key};

/// How many rows the strip may occupy inside the composer's reserved area.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-05, open to
/// Jeshua's veto**, and it is a rendering budget rather than a retrieval one.
/// [ADR-0005] sets `MATCH_LIMIT` at eight and the strip can add a ninth line
/// for `keyword only`, so a shell that reserved every row the strip could ever
/// want would spend nine rows of permanent furniture on a surface that is
/// usually collapsed.
///
/// Six is the number that record's own clause-5 check already uses — "strips
/// of zero, one and six entries" — so it is taken from the record rather than
/// invented. A person should set it or confirm it.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
pub const STRIP_ROWS: u16 = 6;

/// How many rows the composer's area occupies, at the foot of the screen.
///
/// One for the input and [`STRIP_ROWS`] for the strip. **Fixed**, which is
/// what keeps the input row a function of the terminal's size alone — see the
/// module documentation.
pub const COMPOSER_ROWS: u16 = STRIP_ROWS + 1;

/// How the user left.
///
/// # Both exit `0`, and no record names either of them
///
/// [ADR-0016] D5's `0` is "success", and a user who asked to leave and left
/// got what they asked for. Nothing here is a failure, so nothing here reaches
/// another code.
///
/// **The two ways to leave are drafted under a delegated coordinator ruling of
/// 2026-09-05, open to Jeshua's veto.** [ADR-0015] D2's table has ten rows and
/// leaving is not one of them, and no record in the catalogue names an exit
/// key. Both are recorded rather than one chosen: a terminal user reaches for
/// `Ctrl-C` before reading anything, and a user who has read the hint strip
/// reaches for the word.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    /// The user typed [`LEAVE`].
    Word,
    /// The user pressed `Ctrl-C`.
    Interrupt,
}

impl Leaving {
    /// Every way to leave, so a check walks them rather than listing them.
    pub const ALL: [Self; 2] = [Self::Word, Self::Interrupt];

    /// What the process exits with. [ADR-0016] D5's `0`, both ways.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Word | Self::Interrupt => 0,
        }
    }
}

/// What the shell wants its host to do next.
///
/// The shell decides nothing about a command's meaning. It reads a line, says
/// which of [ADR-0015] D2's namespaces it named, and hands it over; the host
/// maps it onto the same function the out-of-session spelling calls, which is
/// what D2's "they are one operation" means concretely.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing. The keystroke was absorbed.
    Idle,
    /// Run this in-session command and hand back what it printed.
    Run(Command),
    /// The user asked for work. **Nothing in this workspace can do it**, and
    /// the host refuses it with the same sentence the out-of-session surface
    /// already prints.
    Task(String),
    /// The user is leaving.
    Leave(Leaving),
}

/// What [ADR-0001] D2's status line says.
///
/// D2: "Status line renders the tier at all times. A user must never be
/// uncertain which membrane they are inside." The tier arrives as text rather
/// than as a `Tier`, because that type is `zaru-cli`'s and the words are its
/// record's; composing them here would be a second statement of D1's table.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The tier, spelled as [ADR-0001] D1 spells it.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    pub tier: String,
    /// The session this shell is inside, per [ADR-0010] D1.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub session: String,
}

impl Status {
    /// A status line.
    #[must_use]
    pub fn new(tier: impl Into<String>, session: impl Into<String>) -> Self {
        Self {
            tier: tier.into(),
            session: session.into(),
        }
    }

    /// The one line the status bar paints.
    ///
    /// The tier is first and is never elided, which is D2's "at all times".
    #[must_use]
    pub fn painted(&self) -> String {
        format!("runtime.tier = {} · session {}", self.tier, self.session)
    }
}

/// Which keystroke leaves, if this one does.
///
/// # One key, one meaning, declared in one place
///
/// [`Shell::key`] calls this, and so does a host that reads a keystroke while
/// a turn is running — where the shell is not the thing deciding what to do
/// with the key, because the shell is not what the turn is waiting on. Both
/// need the same answer, and `ctrl` plus `c` spelled at two call sites is the
/// rule-in-two-places that nothing keeps agreeing.
///
/// **A mid-turn `Ctrl-C` therefore leaves, exactly as one at the prompt
/// does.** [ADR-0015]'s ruling of 2026-09-05 gives this key one meaning and
/// [ADR-0016] D5's `0` for both; what makes it also an *interruption* is what
/// the host does with the turn it was running, which is
/// [ADR-0010](https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript)
/// D2's business and not this crate's.
///
/// [`LEAVE`] is the other way out and is not here: it is a *line*, read by
/// [`command::read`] after `Enter`, and a word is not a keystroke.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
pub fn leaves(input: &Input) -> Option<Leaving> {
    (input.ctrl && input.key == Key::Char('c')).then_some(Leaving::Interrupt)
}

/// A terminal session.
///
/// Synchronous and clockless in the same way [`Composer`] is: every method
/// that needs the time takes it, so there is nothing here that could read the
/// machine's clock.
#[derive(Debug)]
pub struct Shell {
    composer: Composer,
    status: Status,
    /// What [`TranscriptSource`] last said, per [ADR-0010] D2.
    transcript: Vec<Line>,
    /// Lines this session produced that no transcript holds — a refusal, a
    /// command's output. Kept apart from `transcript` so a refresh cannot
    /// lose them and so nothing here can be mistaken for the record on disk.
    notices: Vec<Line>,
    asking: Option<Confirmation>,
    answered: Option<bool>,
}

impl Shell {
    /// Open a shell over a status line.
    #[must_use]
    pub fn open(status: Status) -> Self {
        Self {
            composer: Composer::new(),
            status,
            transcript: Vec::new(),
            notices: Vec::new(),
            asking: None,
            answered: None,
        }
    }

    /// Read the transcript again.
    ///
    /// [ADR-0010] D2 makes the transcript append-only, so this replaces rather
    /// than merges: the source is the authority on what the record holds.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub fn refresh(&mut self, source: &dyn TranscriptSource) {
        self.transcript = source.lines();
    }

    /// Add a line this session produced.
    pub fn notice(&mut self, line: Line) {
        self.notices.push(line);
    }

    /// The composer, for a host that needs to hand it a search response.
    #[must_use]
    pub const fn composer(&self) -> &Composer {
        &self.composer
    }

    /// The composer, mutably, for the same reason.
    pub const fn composer_mut(&mut self) -> &mut Composer {
        &mut self.composer
    }

    /// The status line.
    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// Everything the pane would show, oldest first: the transcript, then this
    /// session's own notices.
    #[must_use]
    pub fn pane_lines(&self) -> Vec<Line> {
        let mut lines = self.transcript.clone();
        lines.extend(self.notices.iter().cloned());
        lines
    }

    /// Put a question to the user. [ADR-0011] D3's `ask`.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn ask(&mut self, question: Confirmation) {
        self.asking = Some(question);
        self.answered = None;
    }

    /// The question standing, if one is.
    #[must_use]
    pub const fn asking(&self) -> Option<&Confirmation> {
        self.asking.as_ref()
    }

    /// What the user answered, once they have.
    ///
    /// `None` while a question stands **and** when none was asked, which is
    /// why a host asks and then pumps until this is `Some` rather than reading
    /// it speculatively.
    #[must_use]
    pub const fn answer(&self) -> Option<bool> {
        self.answered
    }

    /// Apply one keystroke at `now`.
    ///
    /// # A standing question takes every key
    ///
    /// [ADR-0011] D3's `ask` mode "prompts before any write or command", and a
    /// prompt the user can type past is not a prompt. So while a question
    /// stands the composer receives nothing.
    ///
    /// # The default is decline, and only `y` is not
    ///
    /// `y` accepts. `n`, `Esc` and `Enter` decline — `Enter` being the default
    /// answer, which is why the prompt renders `[y/N]`. **Every other key is
    /// ignored and the question stays**, rather than declining: a stray
    /// keystroke that silently answered would make the prompt's outcome depend
    /// on something the user did not mean, and the safe direction is to keep
    /// asking. [ADR-0011] D6 gives the harness no veto and this gives it no
    /// accidental one either.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn key(
        &mut self,
        input: Input,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) -> Action {
        if self.asking.is_some() {
            match input.key {
                Key::Char('y' | 'Y') => self.resolve(true),
                Key::Char('n' | 'N') | Key::Esc | Key::Enter => self.resolve(false),
                _ => {}
            }
            return Action::Idle;
        }

        // Ctrl-C leaves from anywhere, including mid-line and mid-turn. The
        // rule is `leaves`, so this is its one caller inside the shell rather
        // than a second spelling of it.
        if let Some(leaving) = leaves(&input) {
            return Action::Leave(leaving);
        }

        if input.key != Key::Enter {
            self.composer.key(input, now, entries);
            return Action::Idle;
        }

        let line = self.composer.text();
        self.composer = Composer::new();
        match command::read(&line, vocabulary) {
            Typed::Nothing => Action::Idle,
            Typed::Leave => Action::Leave(Leaving::Word),
            Typed::Task(task) => Action::Task(task),
            Typed::Command(command) => Action::Run(command),
            Typed::Refused(refusal) => {
                self.notice(Line::new(Register::Failed, refusal.to_string()));
                Action::Idle
            }
        }
    }

    fn resolve(&mut self, answer: bool) {
        self.asking = None;
        self.answered = Some(answer);
    }
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests;
