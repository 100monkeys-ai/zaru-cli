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
use crate::config::{Home, Variables};
use crate::failure::{Exit, SessionEvidence};
use crate::runtime::ResolvedTier;
use crate::session::{MetaFile, Resumed, SessionId, SessionStore};
use crate::terminal::corpus::CorpusCache;
use crate::terminal::driver::{Crossterm, Guard, Turnable, Turns};
use crate::terminal::source::{Beat, Source};
use crate::terminal::trie::NotesTrie;
use crate::terminal::vocabulary::{Transcript, Vocabulary};
use std::io::IsTerminal;
use zaru_tui::shell::Palette;
use zaru_tui::shell::{Shell, Status};

/// Which session a shell is being asked to open.
///
/// # Three, and each is a spelling a record names
///
/// [`Opening::New`] is a bare `zaru` at a terminal, decided 2026-09-06 as an
/// accepted Update on [ADR-0015] D2's flag-surface contract under directive 25
/// — the survey's row 1 was "there is no way to open a session at a terminal",
/// because the shell was reachable only through `--resume` or `--continue` and
/// both need a session a *non-interactive* run already created.
/// [`Opening::Existing`] is `--resume <id>` and `/session resume <id>`, and
/// [`Opening::MostRecentHere`] is `--continue` and `/session continue` —
/// [ADR-0010] D4's "one operation with two entry points", which is why the
/// two flags and the two slash verbs resolve through this one type.
///
/// **Named `Opening` and not `Target`**, because [ADR-0011] D4's
/// [`Target`](crate::tools::Target) is a path classified against the working
/// directory and this is a session to open. [Ubiquitous Language]'s rule is
/// that the newcomer qualifies.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    /// Mint one. A bare `zaru` at a terminal.
    New,
    /// The one named.
    Existing(SessionId),
    /// [ADR-0010] D4's most recent session started in this directory.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    MostRecentHere,
}

/// Which session this invocation asks for, if it asks for one.
///
/// **Three requests and no others.** A shell that opened for `zaru runtime`
/// would turn a question into a session, and [ADR-0010] D1 makes a session a
/// directory on disk — so opening one to answer a question would create state
/// in order to read state, which the configuration loader already refuses to
/// do for the same reason. A bare `zaru` is not a question: it is the request
/// to be in a session, which is what the 2026-09-06 Update decided.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[must_use]
pub fn opening_for(request: &Request) -> Option<Opening> {
    match request {
        Request::Session => Some(Opening::New),
        Request::Resume { id } => Some(Opening::Existing(id.clone())),
        Request::Continue => Some(Opening::MostRecentHere),
        _ => None,
    }
}

/// Whether standard output is a terminal.
///
/// `std::io::IsTerminal`, so no dependency: ADR-0003 D2's table is closed on
/// purpose and this is one `isatty` call the standard library already makes.
#[must_use]
pub fn a_person_is_watching() -> bool {
    std::io::stdout().is_terminal()
}

/// Whether this process paints the registers' colours.
///
/// # `NO_COLOR`, and it is read exactly once
///
/// [ADR-0028] D2's Update of 2026-09-14: "`NO_COLOR` present in the
/// environment and non-empty disables every colour and leaves the glyphs;
/// `NO_COLOR` present and empty does not", which is the published
/// convention's own wording rather than a reading invented here. The empty
/// case is not an edge nobody meets — `NO_COLOR=` is what a shell leaves
/// behind when a variable is cleared rather than unset.
///
/// **This is the only place in any crate that reads it**, and since
/// 2026-09-27 it reads it out of the [`Variables`] the binary's `main` read
/// once rather than out of the process. `zaru-tui` names no environment
/// variable at all, because [`Palette`] reaches the renderer as an argument;
/// so the value cannot drift between one frame and the next, and a check can
/// paint either palette without touching the process's environment (which is
/// shared state a suite running in one process would owe back).
///
/// No `zaru.toml` key and no flag: neither exists, and [ADR-0015] D2's flag
/// surface is a closed set whose own count is annotated so an eighth fails to
/// compile. Adding one is an amendment to that record rather than a rendering
/// detail.
///
/// # The convention is honoured at two sites, and neither covers the other
///
/// **Measured 2026-09-14 from the release binary over a pseudo-terminal, not
/// read off this function.** `crossterm` 0.28 reads `NO_COLOR` itself, in
/// `Colored::ansi_color_disabled`, with the same empty-string arm this
/// function takes — so under `NO_COLOR` the frame's own reset sequences come
/// out as `ESC[m` where they are `ESC[39m` and `ESC[49m` in colour, which is
/// how the second site announces itself in a capture.
///
/// It is recorded rather than removed because the two cover different things.
/// `crossterm`'s covers `crossterm`'s writer and nothing else: it says nothing
/// about the cell a `TestBackend` holds, which is where every check here reads,
/// and nothing about any other backend. This one decides what is *in* the
/// frame rather than how one backend spells it. A mutation that made
/// `Palette::Monochrome` paint the colours anyway therefore still reddens the
/// cell-level checks while leaving a `NO_COLOR` pty capture clean — which is
/// the reason those checks read cells rather than bytes.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[must_use]
pub fn palette_of(variables: &Variables) -> Palette {
    palette_for(variables.get_os("NO_COLOR"))
}

/// The convention itself, as a function of the value rather than of the
/// process.
///
/// Split from the reader above so the two arms can be checked without a check
/// writing to the environment — which is state every other check in the same
/// process shares, and which a check that changed it would owe back
/// ([Verification lessons] §23). The reader is one line and has nothing left
/// to get wrong; this is where the rule lives.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[must_use]
pub fn palette_for(asked: Option<&std::ffi::OsStr>) -> Palette {
    match asked {
        Some(value) if !value.is_empty() => Palette::Monochrome,
        _ => Palette::Coloured,
    }
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

/// Everything [`shell_for`] builds for one session, before a terminal exists.
///
/// A named tuple rather than five positional values in a signature, because
/// `clippy::type_complexity` refuses the second at four and this became five
/// on 2026-09-14 and six on 2026-09-15. The order is the order a caller uses
/// them in: paint, read the transcript, serve the strip, fetch into it,
/// resume the conversation, offer a tip.
///
/// The last is [ADR-0002](https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output)
/// D8's [`Conditions`](crate::compose::Conditions), and it is returned rather
/// than decided here because a tip needs **two** things this function has only
/// one of: the capability's condition, which is the credential store's and is
/// read here, and D8's budget, which is
/// [`Owed`](crate::compose::Owed)'s and is not built until a provider has
/// resolved. See `one_session`.
type Opened = (
    Shell,
    Transcript,
    NotesTrie,
    Option<Populating>,
    Resumed,
    crate::compose::Conditions,
);

/// The credential store this session reads, opened once.
///
/// **One open, two readers**, because two opens of one file are two answers
/// that can disagree: [`composer_reader`] asks which entry the composer reads
/// with, and [ADR-0007] D8's marking asks whether the entry carrying the role
/// is apex. A store that will not open is not an error here and is
/// deliberately not classified: a person with no credential store has no
/// token, which is exactly the state [`NotesTrie::nothing_cached`] describes
/// and the state an unmarked row describes, and refusing to open a shell
/// because `~/.zaru/credentials.json` is unreadable would make the composer's
/// hint strip able to stop a session starting. The strip says what it can see
/// and so does the row.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
fn credential_store(home: &Home) -> Option<crate::credentials::CredentialStore> {
    let root = crate::credentials::CredentialStore::root_in(home).ok()?;
    crate::credentials::CredentialStore::reading(root).ok()
}

/// Which stored token this session's composer reads with, if any.
///
/// Asks [`credentials::notes::composer_token`](crate::credentials::composer_token)
/// which entry serves, over the store [`credential_store`] opened.
fn composer_reader(
    store: &crate::credentials::CredentialStore,
    variables: &Variables,
    workspace: &str,
    cache: CorpusCache,
) -> Option<Populating> {
    let (alias, host) = crate::credentials::composer_token(store)?;
    let keyring = crate::credentials::OsKeyring::for_store(store.root());
    let keys = crate::credentials::HarnessKeys::within(&keyring, variables);
    let secret = store.secret(&alias, &keys).ok()?;
    Some(Populating {
        workspace: workspace.to_owned(),
        host,
        secret,
        cache,
    })
}

/// Everything one session needs to fetch its corpus, and nothing more.
///
/// # Why the secret is carried here rather than read inside the task
///
/// The store, the keyring and the sealing key are all read on the shell's own
/// thread before the terminal is taken, so a store that will not open is a
/// thing that has already happened by the time anything is spawned. Reading
/// them inside the task instead would put a keyring call on a path where its
/// failure has nowhere to go.
///
/// **It holds a [`Secret`](crate::credentials::Secret) and therefore renders
/// nothing.** That type has no `Display`, its `Debug` prints a fixed marker,
/// and the one function that exposes the value is named for the single place
/// it is allowed to go — so this struct inherits ADR-0007 D3 rather than
/// restating it, and there is no field here a value could be copied into.
///
/// **Public only because it appears in [`shell_for`]'s return, and opaque
/// otherwise**: every field is private, its `fetch` is private, and
/// there is no constructor outside this module. So a caller can hold one and
/// hand it back, which is all `one_session` does, and can read nothing out of
/// it — which is the property that matters, since what it holds is a bearer.
pub struct Populating {
    /// ADR-0006 D5's attached workspace, the one workspace the corpus covers.
    workspace: String,
    /// The instance host, from the token's own instance-locked reach.
    host: String,
    /// The stored bearer, sealed until `corpus_at` hands it to the endpoint.
    secret: crate::credentials::Secret,
    /// [ADR-0005] D8's file, so the refresh can record what it fetched and
    /// forget what the instance says this token may no longer read.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    cache: CorpusCache,
}

/// What one refresh did, in the three shapes the strip has words for.
///
/// Named `Refresh` and not `Refreshed` because
/// [`credentials::Refreshed`](crate::credentials::Refreshed) is a refreshed
/// tool scope and is a different thing entirely.
///
/// **Three and not a `Result<_, String>`**, which is what this was until
/// 2026-09-15. A string cannot be branched on, and the branch is the whole of
/// [ADR-0005] D8's eviction rule: an instance that never answered says nothing
/// about whether this token may still read this workspace, while an instance
/// that answered and refused has said exactly that. Flattening both into one
/// sentence made the two indistinguishable at the only place that has to tell
/// them apart.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
#[derive(Debug)]
pub enum Refresh {
    /// The listings came back, and the file now holds them.
    Reached(Vec<zaru_notes::trie::CachedEntry>),
    /// No transport: the instance said nothing, so the cache stands.
    Unreachable(String),
    /// The instance answered and refused, so the entry is gone from the file.
    Refused(String),
}

impl Populating {
    /// What this session's composer has cached for its own instance and
    /// workspace, if anything, with the file left at one line per key.
    ///
    /// The compaction happens **here**, once as a session opens, for the
    /// reason `session::History`'s does: a rewrite races an append, and doing
    /// it at open leaves a window the width of one session's start rather than
    /// one per refresh.
    ///
    /// A cache that will not read is no cache: the session starts cold, which
    /// is exactly what every session did before this file existed. It is not
    /// an error that can stop a shell opening, for `credential_store`'s own
    /// reason — the hint strip must not be able to stop a session starting.
    fn cached(&self) -> Option<crate::terminal::corpus::CachedCorpus> {
        drop(self.cache.compact());
        self.cache.read(&self.host, &self.workspace).ok().flatten()
    }

    /// [ADR-0005] D3's corpus for the attached workspace, over the real
    /// server, and D8's file kept up with what it says.
    ///
    /// Three requests — an attach and two listings — measured at one to two
    /// seconds on 2026-09-14, which is the whole reason this is awaited off
    /// the shell's first frame rather than before it.
    ///
    /// **A file that will not be written is not reported**, and that is a
    /// decision rather than an omission: the corpus is in hand, the session is
    /// unaffected, and the entire cost is that the next session starts cold —
    /// which is the state every session was in before this arc. There is
    /// nothing a person could do with the sentence and nowhere on the strip to
    /// put it that would not displace the corpus it is about.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    async fn refresh(self) -> Refresh {
        let answer = crate::credentials::corpus_at(&self.host, &self.secret, &self.workspace).await;
        refresh_from(
            answer,
            &self.cache,
            &self.host,
            &self.workspace,
            crate::terminal::corpus::now_in_millis(),
        )
    }
}

/// What one fetch's answer does to [ADR-0005] D8's file, and what the strip is
/// then told.
///
/// **Separate from `Populating::refresh` because the decision is the whole
/// of D8's freshness rule and the fetch is a socket.** A check cannot open an
/// instance, and driving this with a staged answer exercises the same branch,
/// the same writes and the same sentences the binary takes — see
/// `tests/corpus_cache_from_outside.rs`, which drives all three arms.
///
/// **The discriminator is whether the instance answered, and it was wrong
/// once.** It read `ReachFailure::Endpoint` as "no transport", which that
/// variant is not: `Endpoint` means no HTTP client could be *built*, so a DNS
/// failure, a refused connection and a revoked token all arrived as `Session`
/// and a person on a train would have had their corpus evicted. It is
/// [`ReachFailure::Refused`](crate::credentials::ReachFailure) that says the
/// instance answered, and only that arm forgets anything.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
#[must_use]
pub fn refresh_from(
    answer: Result<Vec<zaru_notes::trie::CachedEntry>, crate::credentials::ReachFailure>,
    cache: &CorpusCache,
    host: &str,
    workspace: &str,
    fetched: u128,
) -> Refresh {
    match answer {
        Ok(entries) => {
            drop(cache.append(host, workspace, &entries, fetched));
            Refresh::Reached(entries)
        }
        // The instance said nothing -- no client, no session, no answer, or an
        // answer this client could not read. Silence is not a refusal, so
        // whatever is cached stays cached and the strip says how old it is.
        Err(
            failure @ (crate::credentials::ReachFailure::Endpoint(_)
            | crate::credentials::ReachFailure::Session(_)),
        ) => Refresh::Unreachable(failure.to_string()),
        // The instance answered, and its answer is that this token cannot have
        // this workspace's listings. So the harness stops holding them.
        //
        // **What this cannot tell apart is stated rather than hidden**: a rate
        // limit or a 5xx that arrives as a JSON-RPC error is an answer too, and
        // evicting on one costs a cold session. The alternative -- evict on
        // nothing -- leaves a revoked token serving its old view of a workspace
        // indefinitely, and there is no finer signal to read, because
        // [ADR-0006] D7 is that the server does not reveal which gate tripped.
        //
        // [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
        Err(failure @ crate::credentials::ReachFailure::Refused(_)) => {
            drop(cache.evict(host, workspace));
            Refresh::Refused(failure.to_string())
        }
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
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<Opened, Box<Exit>> {
    let classify = Classify::new(version, report_at);

    let resolution = crate::cli::layers::resolve_for(home, variables, overrides)
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let tier = ResolvedTier::from_configuration(&resolution)
        .map_err(|refusal| Box::new(Exit::Failed(classify.tier(&refusal))))?;

    let root = SessionStore::root_in(home)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    let directory = store.sessions_directory().join(id.as_str());
    // ADR-0005 D8's file, taken before `store` is shadowed by the credential
    // store below. It sits beside `history.jsonl` under `~/.zaru/` rather than
    // inside a session directory, which is the whole of what makes it survive
    // a restart: a session directory is a new directory every time.
    let cache = CorpusCache::under(store.root());
    let resumed = crate::session::resume(&directory, usize::MAX).map_err(|failure| {
        Box::new(Exit::Failed(
            classify.resume(&failure, SessionEvidence::NoSessionExists),
        ))
    })?;

    // The store is opened here rather than inside `composer_reader`, because
    // two things read it and one of them is not conditional on a workspace
    // being attached. `None` is a machine with no store, which is every
    // machine before the first `notes tokens add`.
    let store = credential_store(home);

    let mut status = Status::new(tier.tier().to_string(), id.to_string());
    // ADR-0007 D8's third marking place, and the only one that is a session's
    // rather than a command's: "Apex entries are marked wherever the token
    // appears: `/notes tokens`, the status line when the composer holds one,
    // and the description the agent reads."
    //
    // **It is set here and never afterwards**, which is why `Status` has no
    // setter for it: the 2026-09-06 amendment to ADR-0001 D2 argues the same
    // discipline for the model and the mode -- "a value the record fixes for
    // the session is handed to the row once, so nothing can change it and the
    // immutability is a shape rather than a rule anybody keeps". A role moved
    // mid-session reaches the row on the next paint anyway, because
    // `terminal::open`'s loop re-enters this function on every switch and the
    // switch a stored key or token takes is one of them -- so re-reading is
    // the mechanism the session already has rather than a second call site.
    //
    // A store that will not open leaves the row unmarked, which is the same
    // thing an absent store and an instance-locked composer say and is the
    // right answer for all three: the row states a marking it read, never the
    // absence of one it could not.
    status.credential = store
        .as_ref()
        .and_then(crate::credentials::composer_apex_marking)
        .map(str::to_owned);

    let mut shell = Shell::open(status);
    let transcript = Transcript::of(&resumed.tail);
    shell.refresh(&transcript);

    // ADR-0005 D3's fast tier. The comment here recorded, on 2026-09-05, that
    // "nothing here reads a stored token into a session" and that the composer
    // was told to say so. **That is what this arc closed.** A token is
    // selected below and the corpus is fetched by `one_session`, which holds
    // the runtime; what is decided here is only which sentence the strip opens
    // with, because the shell is painted before any of it has happened.
    //
    // **Two things must both be true before anything is fetched.** A workspace
    // to search -- ADR-0006 D5's pin, read from this session's own `meta.toml`
    // -- and a token to search it with. Either missing is the no-token line,
    // and that is a ruling rather than an oversight: D5's other half falls back
    // to the account's personal workspace, and the handshake's
    // `_grounding.you` was measured on 2026-09-14 and does **not** name one a
    // token can read -- the workspace it reports as current was refused on the
    // very next call. So there is nothing to fall back to, and inventing a
    // second sentence about a missing pin was refused: the existing line is
    // what a person sees, unchanged.
    let attached = attached_workspace(&directory);
    let populating = store
        .as_ref()
        .filter(|_| !attached.is_empty())
        .and_then(|store| composer_reader(store, variables, &attached, cache));
    // **ADR-0005 D8, and the reason clause 10b existed.** A session that has a
    // token and a pin asks the file first: where the last session left a
    // corpus for this instance and this workspace, the strip completes from
    // the first beat and the fetch below is a refresh behind it. Where it did
    // not, this is byte for byte what the session did yesterday.
    let trie = match populating.as_ref().and_then(Populating::cached) {
        Some(cached) => NotesTrie::from_cache(cached.entries, attached, cached.fetched),
        None => match &populating {
            Some(_) => NotesTrie::awaiting(attached),
            None => NotesTrie::nothing_cached(attached),
        },
    };
    shell.composer_mut().set_absence(trie.absence());

    // **The fetch is handed back rather than started here**, and the reason is
    // that this function deliberately takes no terminal and no runtime: its
    // whole purpose is that a check can assert the status line, the pane and
    // the transcript without the three system calls `open` makes. Spawning a
    // task would have put a reactor in that path. `one_session` holds the
    // runtime and starts it there.
    // ADR-0002 D8's condition, read from the store this function already
    // opened. `composer` is ADR-0006 D4's role holder -- the token the hint
    // strip searches with -- so its absence is exactly "a capability the user
    // has not discovered", and its arrival is exactly D8's "without action".
    let conditions = crate::compose::Conditions {
        composer_token: store
            .as_ref()
            .is_some_and(|store| store.composer().is_some()),
        // `terminal.mouse`, the same answer `open` gave `driver::arm`.
        mouse_captured: crate::terminal::mouse::held(&resolution),
    };

    Ok((shell, transcript, trie, populating, resumed, conditions))
}

/// The redactor a session that resolved no provider paints its row through.
///
/// [ADR-0008] clause 6's port is over the credential store, and a session
/// whose composition refused never read it — so holding nothing is the honest
/// state rather than a stand-in. Redacting with this is the identity, which is
/// what `HeldSecrets::none`'s own documentation says it is for.
///
/// A `static` rather than a value built at the call site because
/// [`crate::terminal::driver::refresh_status`] takes a reference and the
/// alternative is a temporary whose lifetime the call has to be written
/// around.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
static NOTHING_HELD: crate::redaction::HeldSecrets = crate::redaction::HeldSecrets::none();

/// How a session's context is sized, whether or not a provider was prepared.
///
/// **A session whose `prepare` refused still opens**, says what is wrong, and
/// shows [ADR-0013] D6's row while the reader reads that refusal — so it
/// needs a window, and there is no provider to give it one. See
/// [`crate::cli::layers::WINDOW_WHEN_NO_PROVIDER`]; the reserve is zero,
/// because no client means no tool surface will be sent.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[must_use]
pub fn context_shape_of(
    prepared: Option<&crate::compose::Prepared>,
) -> crate::compose::ContextShape {
    prepared.map_or_else(
        || {
            crate::compose::ContextShape::of(
                crate::cli::layers::context_limits(crate::cli::layers::WINDOW_WHEN_NO_PROVIDER),
                0,
            )
        },
        crate::compose::Prepared::context_shape,
    )
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
    shape: crate::compose::ContextShape,
    persona: Option<&str>,
) -> Result<crate::compose::SessionContext, Box<Exit>> {
    let prefix = crate::compose::prefix_for(persona);
    match &resumed.checkpoint {
        Some(checkpoint) => crate::compose::SessionContext::restored(prefix, shape, checkpoint)
            .map_err(|error| {
                Box::new(Exit::Failed(classify.checkpoint_contents(&error, evidence)))
            }),
        None => Ok(crate::compose::SessionContext::opened(prefix, shape)),
    }
}

/// Which session an [`Opening`] names, minting one where it says to.
///
/// # One home, for all three arms and everything the mint reads
///
/// A check has to be able to drive all three arms, including the one that
/// mints, so the home is a parameter. **It was a session root until
/// 2026-09-27, under the name `resolve_in`, and that was half a seam**: the
/// mint wrote the session under the root it was handed and then read
/// configuration layer 2, the credential store and the persona cache through
/// the process's own `$HOME`. A check minting under a scratch root therefore
/// recorded whichever provider the person running it had a key for — red on
/// a machine whose owner uses Zaru and green on a CI runner. A [`Home`] is the
/// whole of `~/.zaru`, so every reader below it reads the one it names.
///
/// # Errors
///
/// The classified [`Exit`] for a store that cannot be reached, a directory
/// that holds no session, or a session that cannot be minted.
pub fn resolve(
    opening: &Opening,
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<SessionId, Box<Exit>> {
    let classify = Classify::new(version, report_at);
    let root = SessionStore::root_in(home)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    match opening {
        // **The session is checked to exist here, and that is what makes a
        // switch survivable.** Found by driving the built binary over a
        // pseudo-terminal: `/session resume <a well-formed id nothing has>`
        // resolved to the id, and the failure then surfaced from `shell_for`
        // *after* the pump had returned -- so it propagated out of `open`'s
        // loop and ended the whole session, where the person had asked to move
        // between two of them. A resolution that can fail must fail where the
        // pane is, which is the argument `run`'s `Action::Run` arm already
        // makes for the lookup it does.
        Opening::Existing(id) => {
            SessionStore::reading(root)
                .existing(id)
                .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
            Ok(id.clone())
        }
        Opening::MostRecentHere => most_recent_in_store(root, version, report_at),
        Opening::New => mint(home, variables, version, report_at, overrides),
    }
}

/// [ADR-0010] D1's session, minted for a bare `zaru` at a terminal.
///
/// # It mints whether or not a provider resolved
///
/// `compose::turn::start` takes the provider as an `Option` for exactly this
/// caller. A shell opens over a session it cannot run a turn in — that is what
/// `--resume` has always done, and the refusal is shown when the user types a
/// task rather than at the door, which is the sentence [`open`] already
/// carries. Refusing to *start* one would make a person on a fresh machine
/// unable to reach the interactive surface at all, which is the survey's row 1
/// and the whole reason this exists.
///
/// The tier comes from the resolved configuration and the directory from
/// [`WorkingDirectory::of_this_process`](crate::tools::WorkingDirectory::of_this_process),
/// which is the one place the process is asked where it is.
///
/// # Errors
///
/// The classified [`Exit`] for configuration that will not resolve, a tier
/// that will not, a store that cannot be reached, or a session that cannot be
/// written.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn mint(
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<SessionId, Box<Exit>> {
    let classify = Classify::new(version, report_at);
    let root = SessionStore::root_in(home)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let resolution = crate::cli::layers::resolve_for(home, variables, overrides)
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let tier = ResolvedTier::from_configuration(&resolution)
        .map_err(|refusal| Box::new(Exit::Failed(classify.tier(&refusal))))?;
    let here = crate::tools::WorkingDirectory::of_this_process()
        .map_err(|failure| Box::new(Exit::Failed(Classify::working_directory(&failure))))?;
    // The provider is whatever `prepare` could resolve, and `None` where it
    // could not: D1 makes the field optional, and a session that records a
    // kind it never reached would be a worse record than one that records
    // none.
    let prepared =
        crate::compose::turn::prepare(home, variables, version, report_at, &resolution).ok();
    let provider = prepared.as_ref().map(crate::compose::Prepared::kind);
    // ADR-0013's window is the prepared provider's, and this session may have
    // none -- see `cli::layers::WINDOW_WHEN_NO_PROVIDER` for what a session
    // that cannot run a turn carries instead.
    let shape = context_shape_of(prepared.as_ref());
    let workspace = crate::manifest::attached_workspace(&resolution);
    // ADR-0027 D1's persona, resolved before the prefix exists -- see
    // `crate::compose::persona` for why it cannot arrive afterwards. The
    // refresh is dropped here rather than spawned: this function mints a
    // session and returns, and `one_session` opens the same session moments
    // later with its own runtime and starts the refresh there.
    let mut serving =
        crate::compose::persona::for_session(home, variables, &resolution, workspace.as_deref());
    drop(serving.take_refreshing());
    let (session, _) = crate::compose::turn::start(
        root,
        tier,
        provider,
        workspace,
        here.root(),
        shape,
        serving.body(),
        &classify,
    )
    .map_err(|classified| Box::new(Exit::Failed(*classified)))?;
    Ok(session.id().clone())
}

/// Open the shell over one session and pump it until the person leaves it.
///
/// Returns a [`Pumped`](crate::terminal::driver::Pumped), because leaving a
/// session and asking for another one are two different things — see
/// [`open`], which is what loops over them.
///
/// # Errors
///
/// Returns the classified [`Exit`] for anything that stopped it before the
/// terminal was taken. Once the terminal is taken, an I/O failure from it is
/// reported as a defect by [ADR-0016] D3's boundary in `main`.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[allow(
    clippy::too_many_arguments,
    reason = "\
    the same list `driver::run` already carries, plus the lines a switch asked \
    to be said on the shell it opens. Each is a port or a value some record \
    owns, and bundling them would be a second name for the same list -- the \
    argument `driver::run` and `compose::turn::run_one` both already make for \
    their own"
)]
fn one_session(
    id: &SessionId,
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
    guard: &mut Guard<Crossterm>,
    runtime: &tokio::runtime::Runtime,
    source: &Source,
    saying: Vec<zaru_tui::shell::Line>,
) -> Result<crate::terminal::driver::Pumped, Box<Exit>> {
    let (mut shell, _, trie, populating, resumed, conditions) =
        shell_for(id, home, variables, version, report_at, overrides)?;

    // What the switch that opened this session had to say, put on the pane
    // before anything else. Empty for a switch the person asked for; see
    // `Pumped::Switch::saying` for the one that carries lines and why.
    for line in saying {
        shell.notice(line);
    }

    // ADR-0005 D3's corpus, fetched **into** a shell that is already open.
    //
    // The trie is shared rather than moved because two things hold it: the
    // pump, which reads it on every keystroke, and the task below, which
    // writes it once. `Entries::matches` takes `&self`, so this needed no
    // change to the port and no dependency -- the corpus sits behind an
    // `RwLock` inside `NotesTrie` and this is an `Arc` of the same value.
    //
    // **The runtime is the one this shell already built**, before the terminal
    // was taken. It is a current-thread runtime, so a task spawned here is
    // driven by the same `block_on` that runs the pump -- every time the pump
    // awaits a keystroke or the beat, which is at least ten times a second.
    // No second runtime and no second thread.
    //
    // A refusal ends in `unreachable`, carrying the client's own sentence, so
    // that ADR-0005 D8's "degrade honestly" reaches the person who typed
    // rather than a log nobody reads. There is deliberately no retry: D3
    // builds the corpus at session start, and a loop here would spend a rate
    // budget against a server that has already said no.
    let trie = std::sync::Arc::new(trie);
    if let Some(populating) = populating {
        let filling = std::sync::Arc::clone(&trie);
        runtime.spawn(async move {
            match populating.refresh().await {
                Refresh::Reached(entries) => filling.reached(entries),
                // No transport. A session that opened from the file keeps what
                // it opened with and says when it was taken; one that did not
                // says the client's own sentence, exactly as before.
                Refresh::Unreachable(detail) => filling.unreachable(detail),
                // The instance answered and refused, so the strip stops
                // serving what it refused — in memory here, and on disk in
                // `refresh` itself.
                Refresh::Refused(detail) => filling.refused(detail),
            }
        });
    }

    // ADR-0008 D1's turns, resolved once for the whole session, and before the
    // pump rather than before the terminal.
    //
    // **The terminal is already taken when this runs.** `open` takes it --
    // raw mode and the alternate screen both, through `Crossterm::take` --
    // before the loop that calls this function, so nothing below can write to
    // a terminal that is still echoing and this comment said it did until
    // 2026-09-15. What happens instead is that a refusal leaves as the `Exit`
    // this function returns, `open` gives the terminal back on that path
    // before it hands it up, and the binary writes ADR-0016's presentation to
    // the restored screen. **Until that day it wrote nothing at all**: the
    // binary returned the terminal path's `Exit` past its own writers, so
    // every refusal here exited with its code and said nothing -- measured
    // from the release binary over a pseudo-terminal at five refusal kinds,
    // zero bytes each. See `cli::Outcome::written`, which is the one writer
    // now.
    let classify = Classify::new(version, report_at);
    let resolution = crate::cli::layers::resolve_for(home, variables, overrides)
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let prepared = crate::compose::turn::prepare(home, variables, version, report_at, &resolution);
    let root = SessionStore::root_in(home)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    let store = SessionStore::reading(root);
    let session = store
        .existing(id)
        .map_err(|failure| Box::new(Exit::Failed(classify.session(&failure))))?;
    // ADR-0016 D3: the session this shell is about to open over is the one a
    // defect names from here -- the one resumed, continued or switched to, and
    // again the one `mint` just started, which told the boundary as it did.
    session.entered();

    // ADR-0010 D1's sixth thing under `~/.zaru/`, read into the shell before
    // the terminal is taken. **The directory is `WorkingDirectory`'s and not
    // a second reading of the process**, which is the same rule `meta.toml`'s
    // `directory` follows and why `--continue` and this file agree about what
    // "here" is. A session on a machine with no resolvable home or working
    // directory simply has no history: the walk is empty and nothing is
    // recorded, which is what `Recording` being an `Option` says.
    //
    // **Compaction happens here and nowhere else.** A rewrite races an
    // append, and doing it once as a session opens leaves a window the width
    // of one session's start rather than one per line typed.
    let here = crate::tools::WorkingDirectory::of_this_process().ok();

    // ADR-0002 D1's one opening line, painted once, before anything a session
    // goes on to say.
    //
    // **Row 9 of the second look-and-feel audit measured the frame this
    // replaces**: one status row and twenty-nine blank rows at 100x30,
    // twenty-three at 40x24, with nothing saying what to type and the working
    // directory -- which bounds every tool call and decides which session
    // `--continue` resumes -- on no surface at all. `cli::render::opening`
    // carries the reading that makes it caused output rather than an
    // unprompted emission, and `compose::emission::Unprompted::Opening` is its
    // member.
    //
    // **Only where the directory is known.** A line naming a working directory
    // this process could not read is not the line; a session that reaches here
    // with `None` gets the frame it had. That `None` is also the one that
    // leaves `Recording` absent one screen down, so the two silences are the
    // same fact rather than two rules.
    if let Some(here) = &here {
        shell.notice(zaru_tui::shell::Line::new(
            zaru_tui::shell::Register::Plain,
            crate::cli::render::opening(here.root()),
        ));
    }

    let history = crate::session::History::under(store.root());
    if let Some(here) = &here {
        match history
            .compact()
            .and_then(|_| history.lines_in(here.root()))
        {
            Ok(lines) => shell.recall(lines),
            // The failure's own sentence, in the register ADR-0016 D1 gives
            // an error, rather than a silence a person would read as "I have
            // never typed anything here". Nothing is authored.
            Err(failure) => shell.notice(zaru_tui::shell::Line::new(
                zaru_tui::shell::Register::Failed,
                failure.to_string(),
            )),
        }
    }

    // ADR-0015 D3's two built locations, read once before the terminal is
    // busy. **D4's gate is not asked here**: `driver::run` puts it, because
    // this module is allowed exactly one `block_on` -- the outer one the whole
    // session runs on -- and a second would make "no `block_on` inside a
    // `block_on`" a property nothing could check. The pump is already inside
    // the runtime and awaits.
    //
    // A machine with no home and a directory with no `.zaru/commands/` both
    // load nothing, silently, which is ADR-0002 D1: the question is caused by
    // a project offering something this user has not answered for, and by
    // nothing else.
    let admissions = crate::commands::Admissions::under(store.root());
    let ceiling = crate::cli::layers::file_ceiling();
    let mut extensions = crate::terminal::driver::Extensions {
        loaded: crate::commands::load_from(
            Some(store.root()),
            here.as_ref().map(crate::tools::WorkingDirectory::root),
            &admissions,
            ceiling,
        ),
        admissions: &admissions,
        home: Some(store.root()),
        directory: here.as_ref().map(crate::tools::WorkingDirectory::root),
        ceiling,
    };

    // ADR-0010 D3's checkpoint, read back into ADR-0013 D1's layer 6, before
    // the pump -- and not before the terminal is taken, which is what this
    // comment claimed until 2026-09-15. See the note above the turns for what
    // is actually true of a refusal raised here.
    //
    // A checkpoint this harness did not write is ADR-0016 D1's **Defect** at
    // D5's `70`, so what a person lost while the binary returned past its own
    // writers was the report URL: exit 70, the alternate screen entered and
    // left, and not one byte saying a bug had been found. It reaches them now.
    // ADR-0027 D1's persona, resolved **before** the prefix this session
    // restores -- `SessionContext::restored` rebuilds layers 1 to 4 from
    // `prefix_for` rather than reading them out of `context.json`, which holds
    // only `exchanges`, so a resumed session re-assembles layer 1 and reads
    // the cache again. The workspace is this session's own `meta.toml`, not a
    // second reading of the process, for the reason `attached_workspace`
    // exists at all.
    let pinned = attached_workspace(&store.sessions_directory().join(id.as_str()));
    let mut serving =
        crate::compose::persona::for_session(home, variables, &resolution, Some(pinned.as_str()));
    let context = restored_context(
        &resumed,
        &classify,
        session.evidence(),
        context_shape_of(prepared.as_ref().ok()),
        serving.body(),
    )?;

    // The refresh, on the runtime this shell already holds, **for the next
    // session only**. It cannot reach the prefix built two statements above:
    // `StablePrefix` has no method that changes it, which is ADR-0013 trigger
    // clause 1 held by the type. `None` where the cache missed, because the
    // page was read on this thread a moment ago and reading it twice would
    // spend a rate budget against an instance that has already answered.
    //
    // A refusal, an unreachable instance and a file that will not be written
    // are all silent here, and that is the same decision `Populating::refresh`
    // records for itself: the persona is in hand, the session is unaffected,
    // and there is nowhere on the pane to put a sentence about it that would
    // not be a sentence about the persona -- which ADR-0027's absence line
    // question reserves to Jeshua.
    if let Some(refreshing) = serving.take_refreshing() {
        runtime.spawn(async move {
            drop(refreshing.refresh().await);
        });
    }

    // **The row is written before the turns are built, since 2026-09-15.**
    // This stood below the `match`, inside `if let Turnable::Ready(turns)`, so
    // a session that resolved no provider never reached `refresh_status` at
    // all and the row kept its opening spelling for the whole session -- no
    // context figure at any width, where ADR-0013 D6's trigger clause 5 asks
    // for one "throughout". Row 6 of the second look-and-feel audit measured
    // it. The context exists either way: `restored_context` above built one
    // over `context_shape_of`'s `None` branch, whose window is
    // `cli::layers::WINDOW_WHEN_NO_PROVIDER` -- a constant whose own
    // documentation says it decides "the second figure on ADR-0013 D6's row
    // while the reader reads that refusal", written for the row this guard
    // prevented.
    //
    // ADR-0012 D4's model and ADR-0011 D3's mode ride the same `Described`,
    // and it is `None` here for the session that has neither -- which is the
    // real state of a session whose composition could not resolve a provider:
    // there is no model answering and no mode governing a tool call that
    // cannot happen. It is also what `cli::render::Window` reads to choose the
    // figure's spelling, so the row cannot claim a window and disown a model
    // in one paint. The redactor is the prepared provider's where there is one
    // and holds nothing where there is not, which is the honest state of a
    // harness that read no secret.
    crate::terminal::driver::refresh_status(
        &mut shell,
        &context,
        None,
        prepared
            .as_ref()
            .ok()
            .map(crate::terminal::driver::Described::of),
        prepared
            .as_ref()
            .ok()
            .map_or(&NOTHING_HELD, |prepared| prepared.redactor()),
    );

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
            owed: crate::compose::Owed::of(
                prepared,
                &resumed.said,
                crate::compose::tips::enabled(&resolution),
            ),
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
    //
    // ADR-0012 D4's model and ADR-0011 D3's mode arrive here too, and this is
    // the only call that writes them: both are fixed for the life of the
    // session by `prepare`, so there is nothing for a later call to update.
    // The `Cannot` arm supplies neither, which is the real state of a session
    // whose composition could not resolve a provider -- there is no model
    // answering and no mode governing a tool call that cannot happen.

    // ADR-0002 D8's standing tip, offered once, before the session's first
    // frame. **This is the only product caller of
    // `Composer::set_standing`**, which had none until 2026-09-15: the
    // renderer, the precedence over a deposit count and the instant yield on
    // a keystroke have all been in `zaru-tui` since 2026-09-05 with nothing
    // to hand them.
    //
    // **The deposit count is `0` and that is not a placeholder.** D3 gives a
    // deposit one producer -- an armed trigger -- and its own last paragraph
    // ships every trigger but one interrupt disarmed. Nothing in this harness
    // can arm one, so the count is a fact rather than a value waiting to be
    // filled in, and it is written here rather than defaulted so that
    // whoever builds D2's arming surface finds the call site.
    //
    // **The display is recorded at the same moment**, which is the reading on
    // `compose::tips`: a display is one session's showing. This is before the
    // terminal is taken and therefore before the row that carries the tip is
    // painted, so a process that dies in between costs one count -- the bound
    // ADR-0010 D2 already accepts for the event in flight.
    //
    // A tips file that cannot be read or written leaves the strip collapsed
    // rather than ending the session, which is the same answer the history
    // file's failure gets one screen up: a person opened a session to work,
    // not to be told about a counter.
    //
    // **Offered whenever the shell opens, `Ready` or not, since 2026-09-15.**
    // This stood inside `if let Turnable::Ready(turns)`, the guard the status
    // refresh above it uses, where it is correct and here it was borrowed: a
    // session that resolved no provider is exactly the session whose person
    // has discovered nothing, and it was the one the tip was withheld from.
    // Row 5 of the second look-and-feel audit measured that the line painted
    // on no machine that survey could produce. `compose::tips::room_for_a_tip`
    // carries why the budget survives the absence of an `Owed`;
    // `Owed::has_room_for_a_tip` is unedited and the condition is still
    // `conditions.composer_token`, which is a fact about the credential store
    // and not about a model.
    let room = crate::compose::tips::room_for_a_tip(
        match &turns {
            Turnable::Ready(turns) => Some(&turns.owed),
            Turnable::Cannot(_) => None,
        },
        crate::compose::tips::enabled(&resolution),
    );
    let tips = crate::compose::Tips::under(store.root());
    if let Ok(Some(tip)) = crate::compose::tips::eligible(room, conditions, &tips)
        && tips
            .record_a_showing(tip, &crate::commands::date::today())
            .is_ok()
    {
        shell
            .composer_mut()
            .set_standing(0, Some(tip.line().to_owned()));
    }

    // ADR-0010 D1's transcript, for the one record the pump writes outside a
    // turn -- ADR-0015 D4's answered door. Composed here, by the session, and
    // handed to the pump in `Recording`: this module owns the session and the
    // pump owns neither.
    let transcript_path = session.transcript_path();

    // The working directory as the strip's third corpus, built here because
    // this is where `here` is and handed to the pump by reference. It is the
    // same `WorkingDirectory` the opening line names and the same one every
    // tool call is classified against -- one reading of "here" per session,
    // which is `WorkingDirectory::of_this_process`'s own rule.
    let paths = crate::terminal::ProjectPaths::under(here.clone());

    let runner = crate::cli::Run {
        version,
        report_at,
        home,
        variables,
    };

    // The guard is what restores, and it is the caller's: a switch keeps the
    // terminal rather than giving it back and taking it again, which would
    // flash the alternate screen between two sessions.
    let pumped = {
        let surface: &mut Crossterm = guard.get_mut().expect("the guard holds the terminal");
        runtime.block_on(crate::terminal::driver::run(
            &mut shell,
            surface,
            source,
            &Beat,
            &runner,
            // The `Arc` is what the filling task holds; the pump takes the
            // value inside it, because `Entries` is implemented for the trie
            // and not for a smart pointer around it.
            trie.as_ref(),
            &Vocabulary,
            // ADR-0005's third corpus. Nothing is walked until the first `@`,
            // so a session that never names a file pays for none of it.
            &paths,
            &mut turns,
            here.as_ref()
                .map(|here| crate::terminal::driver::Recording {
                    history: &history,
                    directory: here.root(),
                    transcript: &transcript_path,
                }),
            &mut extensions,
        ))
    };

    Ok(match pumped {
        Ok(pump) => pump.outcome,
        // A terminal that stopped answering is not the user's fault and is not
        // a defect in the harness either; the session is over and the shell
        // gave the terminal back.
        Err(_) => crate::terminal::driver::Pumped::Left(Exit::Succeeded),
    })
}

/// Open a session's shell, and every session the person switches to after it.
///
/// # One terminal, one runtime, and a loop over sessions
///
/// [ADR-0010] D4's in-session half — `/session resume <id>` and `/session
/// continue` — is "one operation with two entry points", and outside a session
/// that operation puts the person *inside* the named session. So inside one it
/// does the same thing, and the pump hands back the session it was asked for
/// rather than an exit.
///
/// The terminal and the runtime are taken **once**, before the first session
/// and outside the loop. A switch that gave the terminal back and took it
/// again would leave and re-enter the alternate screen between two sessions,
/// which a person sees as a flash and a lost frame.
///
/// **A switch resolves before the current shell is replaced**, inside the pump
/// — see [`crate::terminal::driver::run`]'s `Action::Run` arm. The target is
/// looked up while the old pane is still alive, so a `/session resume` naming
/// a session that does not exist paints its refusal where the person is
/// looking. A terminal in raw mode has no echo, and writing to standard error
/// there is writing into the alternate screen; this loop therefore only ever
/// receives a session that resolved.
///
/// # Errors
///
/// Returns the classified [`Exit`] for anything that stopped it before the
/// terminal was taken.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn open(
    opening: &Opening,
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
    overrides: &Overrides,
) -> Result<Exit, Box<Exit>> {
    let mut id = resolve(opening, home, variables, version, report_at, overrides)?;

    // **One runtime for the whole shell, built before the terminal is taken.**
    // Every turn of every session it opens is polled on it, and it is what
    // lets the pump race a turn against the terminal at all, since a turn is a
    // future rather than a call that blocks.
    //
    // The `expect` is the one `compose::turn::block_on` already carried and is
    // deliberately not a classification: a reactor that will not register with
    // the operating system is ADR-0016 D3's defect, caught by the boundary in
    // `main` and reported as a bug in the harness, and inventing a
    // user-correctable class for it would be that record's "never present a
    // defect as a user error".
    let runtime = crate::compose::turn::runtime()
        .expect("a current-thread runtime with the io and time drivers");

    // **The signals are taken before the terminal is**, so there is no moment
    // at which the terminal is ours and a signal would leave it raw. The
    // `expect` is the runtime's own, for the runtime's own reason: an
    // operating system that refuses `sigaction` is ADR-0016 D3's defect, not
    // the person's error.
    let signals = {
        let _inside = runtime.enter();
        Signals::take().expect("the session's runtime registers SIGTERM, SIGINT and SIGHUP")
    };
    // `terminal.mouse` is read before the terminal is taken, because it
    // decides what taking it asks for; a configuration that will not resolve
    // is refused here, on the screen the person is looking at, exactly as
    // `shell_for` would refuse it a moment later.
    let hold_the_mouse = crate::cli::layers::resolve_for(home, variables, overrides)
        .map(|resolution| crate::terminal::mouse::held(&resolution))
        .map_err(|failure| Box::new(Exit::Failed(Classify::load(&failure))))?;
    let crossterm = Crossterm::take(hold_the_mouse, palette_of(variables))
        .map_err(|_| Box::new(Exit::Succeeded))?;
    let mut guard = Guard::new(crossterm);
    // Polled whenever the pump is, which is whenever a session is open: every
    // session runs inside `runtime.block_on`, and the pump awaits between
    // beats. Dropped with the runtime when this function returns.
    runtime.spawn(signals.give_the_terminal_back());

    // The terminal's own reader, on a thread of its own, and one for every
    // session: a second reader would race the first for the same keystrokes.
    // It stops within `terminal::POLL` of being dropped, which is before the
    // guard gives the terminal back.
    let source = Source::over_the_terminal();

    // What the last switch asked this loop to say on the shell it opens.
    let mut saying: Vec<zaru_tui::shell::Line> = Vec::new();
    let exit = loop {
        match one_session(
            &id,
            home,
            variables,
            version,
            report_at,
            overrides,
            &mut guard,
            &runtime,
            &source,
            core::mem::take(&mut saying),
        ) {
            Ok(crate::terminal::driver::Pumped::Left(exit)) => break exit,
            // Already resolved, and resolved **inside** the pump so a refusal
            // reached the pane rather than a terminal in raw mode. See
            // `driver::run`'s `Action::Run` arm.
            Ok(crate::terminal::driver::Pumped::Switch { to, saying: said }) => {
                id = to;
                saying = said;
            }
            Err(exit) => {
                drop(source);
                guard.restore_now();
                return Err(exit);
            }
        }
    };
    drop(source);
    guard.restore_now();
    Ok(exit)
}

/// The three signals that can end a session and still give the terminal back,
/// and the terminal going away without one.
///
/// # Why a session takes signals at all
///
/// The terminal is restored by [`Guard`]'s `Drop`, which runs on an ordinary
/// exit, an early return and an unwind. **A process a signal ends runs no
/// `Drop`.** Measured on the release binary at `2a7544b`, before this existed:
/// a `SIGTERM` wrote no reset at all. The person's shell was left reading
/// `-isig -icanon -echo`, on the alternate screen, with bracketed paste and
/// every mouse mode on, so each movement of the mouse typed a report into
/// their prompt.
///
/// # Which signals, and the one that cannot be taken
///
/// `SIGTERM` is what `kill` and most supervisors send. `SIGHUP` is what a
/// closing terminal or a dropped connection sends. `SIGINT` arrives only as a
/// signal someone sent: raw mode turns off the terminal's `ISIG`, so `Ctrl-C`
/// is a key the shell reads, never this signal. **`SIGKILL` cannot be caught
/// by any process**, so a `kill -9` still leaves the terminal as it was, and
/// `reset` is the person's remedy.
///
/// # What happens, in order
///
/// The terminal is given back ([`crate::terminal::driver::give_back`], the
/// function [`Guard`] reaches too), and then the process exits with
/// [`crate::failure::signalled`]: `128 + n`, the status a shell already
/// reported for a process that signal ended. Nothing else is flushed or
/// finalised, and nothing needs to be. [ADR-0010] D2's transcript is written a
/// record at a time precisely so that a killed process loses at most the event
/// in flight, and this is a killed process that tidied the terminal first.
///
/// # A terminal that goes away is a hang-up, whoever says so
///
/// `SIGHUP` reaches `zaru` only because the shell leading its terminal's
/// session ends on its own hang-up and the kernel then hangs up the
/// foreground. A shell that ignores the signal — everything under `nohup` —
/// does not end, so nothing is sent, and until 2026-09-28 the session then
/// never ended at all: measured by the `harness-orphans-and-reader-panic` arc,
/// eight such processes lived five hours on `/dev/pts/N (deleted)`, each with
/// its terminal reader spinning a core, because crossterm's `poll` reads a
/// hung-up terminal's end of file as "nothing yet" and loops inside itself, so
/// the reader never looks at its stop flag again and cannot be joined.
///
/// So the listener also watches the terminal itself, once a
/// [`TICK`](crate::terminal::source::TICK): standard output stops being a
/// terminal the moment it is hung up, because the terminal answers every
/// question after that with `EIO`. That is taken as the hang-up it is, with
/// the same restore and the same `129`; the spinning reader ends with the
/// process, which is the only thing that can end it.
///
/// # When it runs
///
/// The listener is a task on the session's current-thread runtime, so it
/// runs when the pump yields, which it does between beats. Between the end of
/// the last session and the process exiting, a signal is taken and not acted
/// on, because tokio never returns a caught signal to its default action. That
/// window is the terminal already having been given back and `main` writing
/// its outcome.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
struct Signals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
    hang_up: tokio::signal::unix::Signal,
}

impl Signals {
    /// Register the three, on the runtime the caller has entered.
    fn take() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
            hang_up: signal(SignalKind::hangup())?,
        })
    }

    /// Wait for the first of the three, or for the terminal to go away, give
    /// the terminal back, and exit.
    async fn give_the_terminal_back(mut self) {
        // The numbers are POSIX's, and the same on every Unix: `SIGHUP` 1,
        // `SIGINT` 2, `SIGTERM` 15.
        let number: u8 = tokio::select! {
            _ = self.terminate.recv() => 15,
            _ = self.interrupt.recv() => 2,
            _ = self.hang_up.recv() => 1,
            () = the_terminal_goes_away() => 1,
        };
        crate::terminal::driver::give_back();
        std::process::exit(i32::from(crate::failure::signalled(number)));
    }
}

/// Resolves once standard output has stopped being a terminal: the session's
/// terminal has gone away. See [`Signals`].
async fn the_terminal_goes_away() {
    loop {
        tokio::time::sleep(crate::terminal::source::TICK).await;
        if !a_person_is_watching() {
            return;
        }
    }
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
pub fn most_recent_in_store(
    root: std::path::PathBuf,
    version: &str,
    report_at: &str,
) -> Result<SessionId, Box<Exit>> {
    let classify = Classify::new(version, report_at);
    let here = crate::tools::WorkingDirectory::of_this_process()
        .map_err(|failure| Box::new(Exit::Failed(Classify::working_directory(&failure))))?;
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
pub fn take_over(
    line: &CommandLine,
    home: &Home,
    variables: &Variables,
    version: &str,
    report_at: &str,
) -> Option<Exit> {
    let opening = opening_for(&line.request)?;
    if !a_person_is_watching() {
        return None;
    }
    Some(
        match open(
            &opening,
            home,
            variables,
            version,
            report_at,
            &line.overrides,
        ) {
            Ok(exit) => exit,
            Err(exit) => *exit,
        },
    )
}
