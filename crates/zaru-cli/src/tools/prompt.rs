// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0011] D3's prompt, over a terminal: one line, `y/N`, and no terminal
//! means no prompt.
//!
//! # One line, and the marking is already in it
//!
//! [`line()`] is `"{statement} [y/N] "` and nothing else. The statement is
//! composed once, by [`Decision::question`](crate::tools::Decision::question),
//! and handed here rather than composed where it is shown — so what the user
//! was told and what the harness believes it asked cannot drift apart. It
//! already carries [ADR-0011] D4's out-of-tree class and D6's annotation,
//! because the question is rendered through
//! [`TranscriptEntry::render`](crate::tools::TranscriptEntry::render), the
//! same function the transcript uses.
//!
//! **So D6's "raises the prompt's prominence" needs no second rendering
//! here.** A destructive call's line says so in the line. A richer
//! prominence — colour, a pause, a differently-shaped confirmation — is
//! `zaru-tui`'s, and it reaches this same [`Confirm`] port with the same
//! [`Question`]. This is the plain one, for the surface that writes plain
//! lines to standard output.
//!
//! # No terminal is not an answer
//!
//! [`Prompt::over`] returns `None` when its input is not a terminal, and
//! [`Prompt::from_process`] is that over this process's own handles. A caller
//! then holds no confirmer, and
//! [`Decision::permit`](crate::tools::Decision::permit) refuses the call as
//! [`ThereWasNobodyToAsk`](crate::tools::RefusedBecause::ThereWasNobodyToAsk)
//! — which is the record's existing refusal, reached without a new outcome
//! and without this module deciding anything.
//!
//! **A default answer would be exactly the silent default D3 forbids**, and
//! answering `false` from a pipe would be worse than refusing: it would put
//! "the user declined" in the transcript of a run no user was watching.
//!
//! # The handles are a parameter, and that is not a seam for a check
//!
//! `std::io::stdin().is_terminal()` is `true` when `cargo test` is run from a
//! terminal and `false` under CI, so a check written against
//! [`Prompt::from_process`] would answer differently on two machines. The
//! handles are therefore taken as arguments, exactly as
//! [`config::environment::read`](crate::config::environment::read) takes its
//! variables and [`Files::at`](crate::cli::layers::Files::at) takes its paths,
//! and for the same reason those do. A check passes a real file it owns and
//! gets `None` from **the product's own [`IsTerminal`] call** rather than from
//! a fixture answering on its behalf.
//!
//! What that leaves unverified is stated rather than glossed: **a check cannot
//! make a terminal**, so the accepting path — a person typing `y` — is not
//! exercised anywhere. [`answer`] is where the y/N rule lives and it is
//! exercised exhaustively; what is not is the reading of a real tty.
//!
//! # No dependency
//!
//! [`std::io::IsTerminal`], stabilised in Rust 1.70 and available on the
//! 1.98 this workspace pins. [ADR-0003] D2's table is untouched and
//! `Cargo.lock` gains nothing.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::tools::port::{Confirm, ConfirmFailure, Question};
use std::io::{BufRead, BufReader, IsTerminal, Read, Stdin, Stdout, Write};
use std::sync::Mutex;

/// What follows the statement on ADR-0011 D3's prompt line.
///
/// The capital `N` is the default and it is the default by being the only
/// thing every input other than a yes produces — see [`answer`]. One
/// constant, so the line a user reads and the rule that reads them back
/// cannot disagree about which way an empty answer goes.
pub const SUFFIX: &str = " [y/N] ";

/// What ADR-0011 D3's prompt writes.
///
/// The statement, then each line of [`Question::detail`], then [`SUFFIX`].
/// **Nothing else**: no second sentence, no re-derived annotation, no
/// separate prominence marker, and nothing composed here — every line
/// arrives from the decision. See the module documentation for where D6's
/// prominence actually lives.
///
/// It was one line until 2026-09-14, when D3's question gained what it is
/// about. The detail is empty for `web.fetch` and the three reading tools, so
/// for those this is byte-for-byte the line it always wrote.
#[must_use]
pub fn line(question: &Question) -> String {
    let mut written = question.statement.clone();
    for row in &question.detail {
        written.push('\n');
        written.push_str(row);
    }
    written.push_str(SUFFIX);
    written
}

/// What a typed line means.
///
/// `y` or `yes`, in any case, after trimming — anything else is no, including
/// an empty line and end of input. That is what makes `N` the default rather
/// than a branch somebody could forget: there is one accepting shape and
/// everything else falls through it.
///
/// `None` is end of input: the user pressed the end-of-file key, or the
/// stream ran out. It is a no rather than a failure, because a person who
/// closed the prompt has answered it.
#[must_use]
pub fn answer(typed: Option<&str>) -> bool {
    let Some(typed) = typed else {
        return false;
    };
    let typed = typed.trim();
    typed.eq_ignore_ascii_case("y") || typed.eq_ignore_ascii_case("yes")
}

/// Put the question and read the answer, over any pair of handles.
///
/// **Where the I/O rule lives**, so that it is a thing a check can drive with
/// real handles rather than a body only a terminal can reach.
/// [`Prompt`] is this plus the terminal gate and the locking, and it has no
/// other behaviour — which is what makes "a check cannot make a terminal" a
/// statement about the gate alone rather than about the whole prompt.
///
/// # Errors
///
/// [`ConfirmFailure`] when the line could not be written or the answer could
/// not be read. **Never** for an answer of no.
pub fn ask(
    input: &mut impl BufRead,
    output: &mut impl Write,
    question: &Question,
) -> Result<bool, ConfirmFailure> {
    let statement = line(question);
    output
        .write_all(statement.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|failure| {
            ConfirmFailure::new(format!("the prompt could not be written: {failure}"))
        })?;

    let mut typed = String::new();
    let read = input.read_line(&mut typed).map_err(|failure| {
        ConfirmFailure::new(format!("the answer could not be read: {failure}"))
    })?;
    Ok(answer((read > 0).then_some(typed.as_str())))
}

/// ADR-0011 D3's prompt, over a terminal.
///
/// The product implementation of [`Confirm`]. Before 2026-09-05 that trait
/// had none anywhere in this workspace.
///
/// It is [`ask`] plus two things and nothing else: the terminal gate in
/// [`Prompt::over`], and a lock around each handle so the type is `Sync`,
/// which [`Executor`](crate::tools::Executor) requires of a confirmer.
///
/// **Nothing in the `zaru` binary constructs one yet**, because no command
/// reaches the tool surface: a task is refused for want of a provider, so
/// there is no tool call to confirm. Wiring [`Prompt::from_process`] into a
/// composition belongs to the arc that first runs a task, and that is said
/// here rather than left for a reader to infer from an unused constructor.
#[derive(Debug)]
pub struct Prompt<R, W> {
    input: Mutex<R>,
    output: Mutex<W>,
}

impl<R: Read + IsTerminal, W: Write> Prompt<BufReader<R>, W> {
    /// A prompt over these handles, if the input is a terminal.
    ///
    /// `None` when it is not, and the bound is what makes that unbypassable:
    /// there is no other constructor, so a `Prompt` that exists was built
    /// over something [`IsTerminal`] said yes to. See the module
    /// documentation — no terminal is not an answer, and a caller holding
    /// `None` refuses the call rather than defaulting it.
    ///
    /// **The buffering happens here rather than at the call site**, so that
    /// the handle the gate asks and the handle the answer is read from are
    /// the same one. A caller that wrapped its own reader could hand a
    /// buffered pipe to a gate that never saw the pipe.
    #[must_use]
    pub fn over(input: R, output: W) -> Option<Self> {
        if !input.is_terminal() {
            return None;
        }
        Some(Self {
            input: Mutex::new(BufReader::new(input)),
            output: Mutex::new(output),
        })
    }
}

impl Prompt<BufReader<Stdin>, Stdout> {
    /// A prompt over this process's own handles, if standard input is a
    /// terminal.
    ///
    /// The product path. A check calls [`Prompt::over`] with handles it owns,
    /// because whether *this* process has a terminal depends on how the
    /// checks were started — see the module documentation.
    #[must_use]
    pub fn from_process() -> Option<Self> {
        Self::over(std::io::stdin(), std::io::stdout())
    }
}

impl<R: BufRead + Send, W: Write + Send> Confirm for Prompt<R, W> {
    fn confirm(&self, question: &Question) -> Result<bool, ConfirmFailure> {
        let mut input = self
            .input
            .lock()
            .map_err(|_| ConfirmFailure::new("the prompt's input handle is poisoned"))?;
        let mut output = self
            .output
            .lock()
            .map_err(|_| ConfirmFailure::new("the prompt's output handle is poisoned"))?;
        ask(&mut *input, &mut *output, question)
    }
}
