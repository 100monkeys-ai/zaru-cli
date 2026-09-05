// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Opening a session's shell, and deciding whether to.
//!
//! # `--resume` answers two different readers
//!
//! [ADR-0010] D4: "`zaru --resume <id>`, or `zaru --continue` for the most
//! recent session in this directory, restores `context.json` and re-renders
//! the last stretch of transcript so the user can see where they were."
//!
//! **Who "the user" is decides what that means.** A person at a terminal is
//! shown the tail in a pane and left inside the session; a pipe is handed the
//! transcript's own bytes and nothing else, because there is nobody there to
//! be inside anything. That is [ADR-0016] D5's argument from the other side —
//! "CI wraps this harness" — and the shape the surface already has: data to
//! standard output, refusals to standard error.
//!
//! Decided 2026-09-05 under directive 20 and recorded as an accepted Update on
//! ADR-0010 D4, open to Jeshua's veto. The record read two ways: restoring the
//! model's continuation state implies continuing, and "so the user can see
//! where they were" is satisfied by printing. Both readings are now true, of
//! the reader each was written for.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::cli::classify::Surface as Classify;
use crate::cli::invocation::{CommandLine, Overrides, Request};
use crate::failure::{Exit, SessionEvidence};
use crate::runtime::ResolvedTier;
use crate::session::{SessionId, SessionStore};
use crate::terminal::driver::{Crossterm, Guard};
use crate::terminal::vocabulary::{Transcript, Vocabulary};
use std::io::IsTerminal;
use zaru_tui::composer::{Entries, Entry};
use zaru_tui::shell::{Shell, Status};

/// Whether this invocation would open a shell if there were a terminal.
///
/// **Two requests and no others.** A shell that opened for `zaru runtime`
/// would turn a question into a session, and [ADR-0010] D1 makes a session a
/// directory on disk — so opening one to answer a question would create state
/// in order to read state, which the configuration loader already refuses to
/// do for the same reason.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub const fn is_a_session(request: &Request) -> bool {
    matches!(request, Request::Resume { .. } | Request::Continue)
}

/// Whether standard output is a terminal.
///
/// `std::io::IsTerminal`, so no dependency: ADR-0003 D2's table is closed on
/// purpose and this is one `isatty` call the standard library already makes.
#[must_use]
pub fn a_person_is_watching() -> bool {
    std::io::stdout().is_terminal()
}

/// The trie the composer's fast tier reads, which nothing implements.
///
/// [Bounded Contexts] gives the local trie to `zaru-notes` and it is not
/// built; [ADR-0005]'s own Status tracking says so. An empty implementation is
/// what the composer is handed, and the strip therefore shows nothing while a
/// user types. **Recorded rather than disguised**: this is the surface's
/// largest missing piece and a check that staged entries here would be
/// asserting about a fixture.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTrie;

impl Entries for NoTrie {
    fn matches(&self, _prefix: &str, _limit: usize) -> Vec<Entry> {
        Vec::new()
    }
}

/// Build the shell for one session, without taking a terminal.
///
/// The error is boxed for the reason [`crate::cli::run`]'s `Outcome` already
/// is: an [`Exit`] carries a whole [ADR-0016] D1 classification, which is far
/// larger than the success value, and `clippy::result_large_err` refuses a
/// `Result` shaped that way. Boxing the rare side costs one allocation on a
/// path that is about to end the process.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
///
/// Separate from [`open`] so a check can assert everything except the three
/// system calls: the status line's tier, the pane's contents, and that the
/// transcript arriving is the session's own.
///
/// # Errors
///
/// When configuration will not resolve, the session store cannot be reached,
/// or the session does not exist.
pub fn shell_for(
    id: &SessionId,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<(Shell, Transcript), Box<Exit>> {
    let classify = Classify::new(version, report_at);

    let resolution = crate::cli::layers::resolve_from_process(overrides)
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let tier = ResolvedTier::from_configuration(&resolution)
        .map_err(|refusal| Box::new(Exit::Failed(classify.tier(&refusal))))?;

    let root = SessionStore::default_root()
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    let directory = store.sessions_directory().join(id.as_str());
    let resumed = crate::session::resume(&directory, usize::MAX).map_err(|failure| {
        Box::new(Exit::Failed(
            classify.resume(&failure, SessionEvidence::NoSessionExists),
        ))
    })?;

    let mut shell = Shell::open(Status::new(tier.tier().to_string(), id.to_string()));
    let transcript = Transcript::of(&resumed.tail);
    shell.refresh(&transcript);
    Ok((shell, transcript))
}

/// Open the shell over a session and pump it until the user leaves.
///
/// # Errors
///
/// Returns the classified [`Exit`] for anything that stopped it before the
/// terminal was taken. Once the terminal is taken, an I/O failure from it is
/// reported as a defect by [ADR-0016] D3's boundary in `main`.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub fn open(
    id: &SessionId,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<Exit, Box<Exit>> {
    let (mut shell, _) = shell_for(id, version, report_at, overrides)?;

    let crossterm = Crossterm::take().map_err(|_| Box::new(Exit::Succeeded))?;
    let mut guard = Guard::new(crossterm);
    let runner = crate::cli::Run { version, report_at };

    // The guard is what restores. Every path out of this block -- the pump
    // returning, an I/O error, a panic unwinding through it -- drops it.
    let pumped = {
        let surface: &mut Crossterm = guard.get_mut().expect("the guard was just constructed");
        crate::terminal::driver::run(&mut shell, surface, &runner, &NoTrie, &Vocabulary)
    };
    guard.restore_now();

    Ok(match pumped {
        Ok(pump) => pump.exit,
        // A terminal that stopped answering is not the user's fault and is not
        // a defect in the harness either; the session is over and the shell
        // gave the terminal back.
        Err(_) => Exit::Succeeded,
    })
}

/// Which session `--continue` means, per [ADR-0010] D4.
///
/// A ULID sorts lexically by creation time and the store's listing sorts, so
/// the last is the most recent — D1's own reason for choosing a ULID over a
/// UUID, rather than a second reading of any clock. The same sentence
/// `cli::run` already relies on.
///
/// # Errors
///
/// When the store cannot be reached or there is no session at all.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn most_recent(version: &str, report_at: &str) -> Result<SessionId, Box<Exit>> {
    let classify = Classify::new(version, report_at);
    let root = SessionStore::default_root()
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    let ids = store
        .ids()
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    ids.last()
        .cloned()
        .ok_or_else(|| Box::new(Exit::Failed(classify.no_session_to_continue())))
}

/// The whole tty branch, as `main` takes it.
///
/// Returns `None` when this invocation is not a session or nobody is watching,
/// which is the signal to fall through to the out-of-session surface.
#[must_use]
pub fn take_over(line: &CommandLine, version: &str, report_at: &str) -> Option<Exit> {
    if !is_a_session(&line.request) || !a_person_is_watching() {
        return None;
    }
    let id = match &line.request {
        Request::Resume { id } => Ok(id.clone()),
        Request::Continue => most_recent(version, report_at),
        _ => return None,
    };
    Some(
        match id.and_then(|id| open(&id, version, report_at, &line.overrides)) {
            Ok(exit) => exit,
            Err(exit) => *exit,
        },
    )
}
