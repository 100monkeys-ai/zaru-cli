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
use crate::session::{MetaFile, Resumed, SessionId, SessionStore};
use crate::terminal::driver::{Crossterm, Guard, Turnable, Turns};
use crate::terminal::source::{Beat, Source};
use crate::terminal::trie::NotesTrie;
use crate::terminal::vocabulary::{Transcript, Vocabulary};
use std::io::IsTerminal;
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

/// The Nuclear Notes workspace this session recorded, if it recorded one.
///
/// [ADR-0006] D5 makes the attached workspace the composer's scope, and
/// [ADR-0010] D1's `meta.toml` is where a session keeps it — so the fast tier
/// is scoped by what the session itself says rather than by anything this
/// function decides. A session that recorded none, which is every session on
/// every machine today because **the binary starts no session**, gets an
/// unnamed workspace and therefore a trie with nothing under it.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
fn attached_workspace(directory: &std::path::Path) -> String {
    MetaFile::at(directory.join("meta.toml"))
        .read_if_present()
        .ok()
        .flatten()
        .and_then(|meta| meta.workspace)
        .unwrap_or_default()
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
) -> Result<(Shell, Transcript, NotesTrie, Resumed), Box<Exit>> {
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

    // ADR-0005 D3's fast tier. Nothing populates it on a real machine yet, and
    // the reason changed on 2026-09-05: it was that
    // `zaru_notes::session::Endpoint` had no implementation, so no listing
    // could be made at all. It has one. What is missing now is narrower and is
    // one step rather than a transport -- nothing here reads a stored token
    // into a session -- and the composer is told to say so rather than paint
    // nothing.
    let trie = NotesTrie::nothing_cached(attached_workspace(&directory));
    shell.composer_mut().set_absence(trie.absence());
    Ok((shell, transcript, trie, resumed))
}

/// [ADR-0013] D1's layer 6 as this session left it, and which turn is next.
///
/// # A resumed session does not remember its own conversation, until now
///
/// [ADR-0010] D4 says a resume "restores `context.json`", D3 says that file
/// "holds what the model needs to continue", and until 2026-09-05 this
/// function's caller opened a **fresh** context and threw the checkpoint
/// away — so the first thing a person typed after resuming was answered by a
/// model that had been told nothing about the session they were sitting in.
///
/// **The checkpoint is canonical and the transcript is history.** D3 and D4
/// name `context.json` and nothing else as what a resume restores; rebuilding
/// layer 6 out of the transcript's records instead would be a second source
/// of truth for one thing, and it would restore the **raw** span of a
/// compaction where [ADR-0013] D2 says "only the model's view is compacted".
/// So a session whose checkpoint holds a summary comes back holding the
/// summary, and the span it replaced stays where D2 put it.
///
/// **The prefix and the limits are this invocation's**, built fresh, which is
/// D1's "never rewritten mid-session" read across a resume: a session resumed
/// after a configuration change gets the configuration it was resumed under
/// rather than the one it was started under.
///
/// # A checkpoint this harness did not write is a defect, not a fresh session
///
/// `SessionContext::restored` refuses a document it did not write, and the
/// refusal is [ADR-0016] D1's **Defect** at D5's `70` — the same class, by the
/// same argument, that `Classify::resume` already gives a checkpoint that will
/// not parse: "the transcript and the checkpoint have exactly one writer in
/// this workspace and it is this harness". Reading it as an empty conversation
/// instead would drop a session's whole history and look exactly like a
/// session that had none.
///
/// **An absent checkpoint is not that.** `Checkpoint::read` already calls
/// `None` "not an error: D3 overwrites it each turn and a session with no
/// turns has had none", so a session that never checkpointed opens empty.
///
/// # Errors
///
/// The classified defect, when the stored document is not what
/// [`crate::compose::SessionContext`] writes.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub fn restored_context(
    resumed: &Resumed,
    classify: &Classify,
    evidence: SessionEvidence,
) -> Result<crate::compose::SessionContext, Box<Exit>> {
    let prefix = crate::compose::prefix_for();
    let limits = crate::cli::layers::context_limits();
    match &resumed.checkpoint {
        Some(checkpoint) => crate::compose::SessionContext::restored(prefix, limits, checkpoint)
            .map_err(|error| {
                Box::new(Exit::Failed(classify.checkpoint_contents(&error, evidence)))
            }),
        None => Ok(crate::compose::SessionContext::opened(prefix, limits)),
    }
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
    let (mut shell, _, trie, resumed) = shell_for(id, version, report_at, overrides)?;

    // ADR-0008 D1's turns, resolved once for the whole session. Everything
    // below happens **before** the terminal is taken, so a refusal is written
    // to a terminal that is still echoing.
    let classify = Classify::new(version, report_at);
    let resolution = crate::cli::layers::resolve_from_process(overrides)
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let prepared = crate::compose::turn::prepare(version, report_at, &resolution);
    let root = SessionStore::default_root()
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    let session = store
        .existing(id)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;

    // ADR-0010 D3's checkpoint, read back into ADR-0013 D1's layer 6, before
    // the terminal is taken so a refusal reaches a terminal that still echoes.
    let context = restored_context(&resumed, &classify, session.evidence())?;

    let mut turns = match &prepared {
        Ok(prepared) => Turnable::Ready(Box::new(Turns {
            version,
            report_at,
            resolution: &resolution,
            prepared,
            session: &session,
            // ADR-0011 D2's notice and ADR-0002 D8's recommendation, each
            // already spent if this session's transcript says it said it.
            // The same source as `next` below, read once by `session::resume`.
            owed: crate::compose::Owed::of(prepared, &resumed.said),
            // ADR-0013 D1's layers, restored from ADR-0010 D3's checkpoint
            // rather than opened empty. Layer 6 is what this session said
            // before the process it said it in ended, and it is what every
            // turn from here assembles over. See `restored_context`.
            context,
            // ADR-0008 D1's turn number, continued rather than restarted.
            // The transcript numbers every turn it holds, so the next one is
            // one past the greatest it holds -- ADR-0010 D4's accepted
            // Update of 2026-09-05, and `session::resume`'s `turns_so_far`
            // carries why it is the greatest rather than the count.
            next: resumed.turns + 1,
            // ADR-0010 D4's second half, from the same read of the same
            // transcript as `next` above and `said` beside it. It is built
            // *here*, inside the arm that resolved a provider, and told at
            // the first thing the user asks rather than at the door -- see
            // `driver::turns_of_one_line`, which carries the ruling and its
            // reason. A session with no provider builds no `Turns` at all,
            // so it tells nothing and spends nothing.
            interrupted: crate::terminal::driver::Pending::of(&resumed, prepared.redactor()),
        })),
        // The real refusal, shown when the user types a task rather than at
        // the door: a person who resumed a session to read it back is not
        // asking for a provider, and refusing before they ask would answer a
        // question they did not put.
        Err(refused) => Turnable::Cannot(crate::terminal::driver::lines_of(refused)),
    };

    // ADR-0013 D6's number, on the row from the session's very first frame:
    // "it is a number that has been visible all along". A session whose
    // composition could not resolve a provider has no context at all, so the
    // segment stays absent there rather than showing a zero -- and the token
    // segment is absent in both cases, because no exchange has happened.
    if let Turnable::Ready(turns) = &turns {
        crate::terminal::driver::refresh_status(
            &mut shell,
            &turns.context,
            None,
            turns.prepared.redactor(),
        );
    }

    // **One runtime for the session, built before the terminal is taken.**
    // Every turn of this session is polled on it, where until 2026-09-05 each
    // turn built and dropped one of its own; and it is what lets the pump race
    // a turn against the terminal at all, since a turn is a future now rather
    // than a call that blocks.
    //
    // The `expect` is the one `compose::turn::block_on` already carried and is
    // deliberately not a classification: a reactor that will not register with
    // the operating system is ADR-0016 D3's defect, caught by the boundary in
    // `main` and reported as a bug in the harness, and inventing a
    // user-correctable class for it would be that record's "never present a
    // defect as a user error".
    let runtime = crate::compose::turn::runtime()
        .expect("a current-thread runtime with the io and time drivers");

    let crossterm = Crossterm::take().map_err(|_| Box::new(Exit::Succeeded))?;
    let mut guard = Guard::new(crossterm);
    let runner = crate::cli::Run { version, report_at };

    // The terminal's own reader, on a thread of its own. It stops within
    // `terminal::POLL` of this value being dropped, which is before the guard
    // gives the terminal back.
    let source = Source::over_the_terminal();

    // The guard is what restores. Every path out of this block -- the pump
    // returning, an I/O error, a panic unwinding through it -- drops it.
    let pumped = {
        let surface: &mut Crossterm = guard.get_mut().expect("the guard was just constructed");
        runtime.block_on(crate::terminal::driver::run(
            &mut shell,
            surface,
            &source,
            &Beat,
            &runner,
            &trie,
            &Vocabulary,
            &mut turns,
        ))
    };
    drop(source);
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
/// **The most recent session *in this directory*, which is the clause's own
/// wording.** The selection is [`crate::session::most_recent_in`] and this
/// function is one of its two callers; `cli::Run::resume_latest` is the other,
/// and until 2026-09-06 each took `store.ids().last()` instead — a recency
/// test where D4 asks for a locality one, which the `harness-look-and-feel`
/// survey measured resuming a session from a different checkout.
///
/// # Errors
///
/// When the store cannot be reached, a session's `meta.toml` will not parse,
/// or no session in this directory exists.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn most_recent(version: &str, report_at: &str) -> Result<SessionId, Box<Exit>> {
    let classify = Classify::new(version, report_at);
    let here = crate::tools::WorkingDirectory::of_this_process()
        .map_err(|failure| Box::new(Exit::Failed(Classify::working_directory(&failure))))?;
    let root = SessionStore::default_root()
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    crate::session::most_recent_in(&store, here.root())
        .map_err(|failure| Box::new(Exit::Failed(classify.continuing(&failure))))?
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
