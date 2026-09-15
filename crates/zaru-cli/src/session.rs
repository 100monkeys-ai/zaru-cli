// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010]'s session lifecycle: the directory, the append-only transcript,
//! the rewritten checkpoint, resume, and bounded retention.
//!
//! # A session is a directory, and every byte of it is readable with `cat`
//!
//! D1: a session is `~/.zaru/sessions/<ulid>/` holding `meta.toml`,
//! `transcript.jsonl` and `context.json`. "**A directory of plain files is
//! inspectable with tools the user already has.** A harness that shows its
//! work should not store the record of that work somewhere only it can read."
//! D5 makes that a constraint on anything later added to the directory rather
//! than a property of what is there today.
//!
//! # What is built, and what waits
//!
//! | ADR-0010 | Built here |
//! | --- | --- |
//! | D1 — a directory named by a ULID, three files | the directory, the id, and all three files |
//! | D2 — append-only transcript, one event per line | yes, and the kill check that bounds what a crash costs |
//! | D3 — the checkpoint is rewritten, the transcript appended | yes, rewritten atomically |
//! | D4 — resume restores and never re-executes | the invariant half: a pure function over the directory that takes no ports |
//! | D5 — nothing leaves the machine | yes, wider than the clause: this module takes no tier and reaches no network at any of them |
//! | D6 — bounded retention, real deletion | yes, over a caller-passed window |
//!
//! # `meta.toml` has a writer, as of 2026-09-05
//!
//! [ADR-0003] D2's table gained its `toml` row on 2026-09-05 under directive
//! 20, and [`MetaFile`] is what took a caller for it. So D1's directory holds
//! three files and all three are written: the transcript appended, the
//! checkpoint and the metadata each replaced atomically through one function.
//!
//! The replace itself is [`crate::atomic::write`], which the `credential-sealing`
//! arc lifted out of the checkpoint on 2026-09-05 for the credential store; the
//! metadata file is its third caller and `zaru init`'s create-once is
//! deliberately not one, because "never overwrite" is a guarantee a replace
//! cannot make.
//!
//! A `std`-only emitter for five flat keys was considered and refused when this
//! module landed, and that judgement is worth keeping now that it is moot: it
//! is two lines of `format!` only for values that never carry a quote, a
//! newline, a backslash or a control character, and `workspace` and `provider`
//! arrive from outside. The check that holds the writer plants exactly those
//! bytes, and the mutation that replaces the crate's rendering with a `format!`
//! reddens on the read-back.
//!
//! **The file records a sixth thing D1 does not name**, and it has to: a
//! [`Meta`]'s tier is a [`ResolvedTier`](crate::runtime::ResolvedTier), which
//! cannot be built without naming the configuration layer it came from, so a
//! writer that recorded the tier alone would force the reader to invent one.
//! See [`crate::session::meta::file`], and ADR-0010 D1's accepted Update.
//!
//! **And a seventh, for a clause that could not be checked without it.**
//! ADR-0010 D4 is "`zaru --continue` for the most recent session in this
//! directory", and until 2026-09-06 a session recorded nowhere the directory
//! it began in. The file carries `directory` now, and [`most_recent_in`] is
//! the one place a stored one is compared against this process's.
//!
//! **Both halves have product callers**, as of 2026-09-06. This paragraph said
//! "nothing in the product calls it yet" until then, and it was falsified on
//! 2026-09-05 by `composer-wiring`, in a commit that never touched this file:
//! `compose::turn::task` mints a session and writes the file for it. The
//! reader's caller is [`most_recent_in`].
//!
//! # This module designs no redaction, and the transcript is where a value lands
//!
//! [ADR-0008]'s trigger clause 6 was decided on 2026-09-05 — one `Redactor`
//! port on every path from captured bytes into a **model prompt** — and
//! nothing on this path is such a path. The transcript is the record, and the
//! decision is deliberately not applied to it. ADR-0010's own Negative
//! section is explicit about the consequence:
//! "Plain-text transcripts on disk contain whatever the session contained,
//! including secrets that appeared in command output. Filesystem permissions
//! are the only protection, and that is worth saying out loud rather than
//! implying encryption that does not exist." So the transcript carries what
//! it was given, the files carry `0600` and the directories `0700` — read
//! back off the filesystem rather than asserted from what the code asked for
//! — and **no filter is added anywhere**. What the checks assert instead is
//! that a planted value reaches the transcript, where the record puts it, and
//! reaches no refusal and no `Debug` of anything that is *about* a session
//! rather than *is* its data. `tests/redaction_from_outside.rs` asserts the
//! same thing from the other side, on a session a model-driven `fs.read`
//! actually wrote: the value is absent from what the model was given and
//! present in the session's files.
//!
//! One thing on this path does pass the port, and it is not the record.
//! [`resume::Interrupted::for_the_model`] redacts the line it hands a model
//! under ADR-0010 D4, and leaves [`ToolCall::line`] untouched.
//!
//! [`resume::Interrupted::for_the_model`]: crate::session::Interrupted::for_the_model
//! [`ToolCall::line`]: crate::session::ToolCall::line
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

pub mod checkpoint;
pub mod history;
pub mod id;
pub mod meta;
pub mod record;
pub mod resume;
pub mod retention;
pub mod store;
pub mod transcript;

pub use checkpoint::{Checkpoint, CheckpointError, TEMPORARY_SUFFIX};
pub use history::{Entry, HISTORY_FILE, HISTORY_LINES, History, HistoryError};
pub use id::{
    ALPHABET, ID_LENGTH, Millis, MintFailure, SessionId, SessionIdRefused, SystemWallClock,
    WallClock,
};
pub use meta::file::MetaFile;
pub use meta::{Meta, MetaFailure, MetaStore};
pub use record::{FailureLine, Phase, Record, Said, SaidOnce, ToolCall, Utterance, Voice};
pub use resume::{AlreadySaid, Interrupted, ResumeFailure, Resumed, resume};
pub use retention::{PruneFailure, Pruned, RetentionWindow, WindowRefused, prune, remove};
pub use store::{
    CHECKPOINT_FILE, META_FILE, SESSIONS_DIRECTORY, Session, SessionError, SessionStore,
    TRANSCRIPT_FILE,
};
pub use transcript::{Reading, Transcript, TranscriptError};

/// Which session [ADR-0010] D4's `--continue` means, in this directory.
///
/// # One function, because the defect was two call sites agreeing
///
/// D4 is "`zaru --continue` for the **most recent session in this
/// directory**". Until 2026-09-06 `terminal::open::most_recent` took
/// `store.ids().last()` for the terminal reader and `cli::Run::resume_latest`
/// did the same for the piped one, each carrying the same comment about a
/// ULID sorting by creation time — which is true, and is a **recency** test
/// where the clause asks for a **locality** one. The
/// `harness-look-and-feel` survey measured the consequence from the built
/// binary at `8179f8a`: run in a directory that had never held a session,
/// `--continue` resumed one created in a different checkout. `operations/known-defects`
/// recorded the cause as "structural rather than one call site", so the fix
/// is one function with two callers rather than the same term added twice.
///
/// # The order is still the ULID's own, and the filter is inside it
///
/// `ids()` sorts, so walking it in reverse is most-recent-first — D1's own
/// reason for choosing a ULID over a UUID, rather than a second reading of any
/// clock. The directory term selects *within* that order, so the answer is the
/// most recent session **that began here** rather than the most recent session
/// filtered afterwards.
///
/// # A session that recorded no directory is nobody's
///
/// Every session directory written before 2026-09-06 has no `directory` key,
/// which [`MetaFile::read_if_present`] reads as an empty path. `here` is a
/// canonical root and is therefore absolute, so such a session matches no
/// directory at all. That is the honest answer: a session that does not say
/// where it began cannot be shown to have begun here, and claiming it would be
/// the same wrong answer the defect already produced.
///
/// A session with no `meta.toml` at all is skipped for the same reason and by
/// the same test. **A `meta.toml` that will not parse is refused**, naming the
/// session, because `file-readers` ruled that an absent one is a datum while a
/// malformed one is a defect — this harness is that file's only writer.
///
/// # Errors
///
/// [`ContinueFailure`] when the store cannot be listed, or when a session's
/// `meta.toml` is present and will not parse.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn most_recent_in(
    store: &SessionStore,
    here: &std::path::Path,
) -> Result<Option<SessionId>, ContinueFailure> {
    for id in store
        .ids()
        .map_err(ContinueFailure::Store)?
        .into_iter()
        .rev()
    {
        let session = store.existing(&id).map_err(ContinueFailure::Store)?;
        let meta = MetaFile::at(session.meta_path())
            .read_if_present()
            .map_err(|failure| ContinueFailure::Meta {
                evidence: session.evidence(),
                failure,
            })?;
        if meta.is_some_and(|meta| meta.directory == here) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Why [`most_recent_in`] could not answer.
///
/// # Two variants because they are two [ADR-0016] D1 classes
///
/// A store that cannot be listed is the **user's** — it is under their own
/// `$HOME` and [`crate::cli::classify::Surface::session`] already carries the
/// remedy. A `meta.toml` that will not parse is **ours**: this harness is that
/// file's only writer, which is the reading `file-readers` recorded on
/// ADR-0010 D1 and the same argument ADR-0016's Update makes for
/// `StoreError::Malformed`. Folding the second into [`SessionError`] would put
/// a defect inside an enum every arm of which is user-correctable, and the
/// classifier would then have to tell them apart by something on the value —
/// which is exactly what that Update says never to do.
///
/// The defect arm carries the evidence rather than the id, because
/// [ADR-0016] D3's report names the session **and** says where its transcript
/// is, and [`Session::evidence`] is the one constructor that can promise both.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug)]
pub enum ContinueFailure {
    /// The session store could not be listed or reached.
    Store(SessionError),
    /// A session's `meta.toml` is present and will not parse.
    Meta {
        /// What ADR-0016 D3's report is told about the session it was read from.
        evidence: crate::failure::SessionEvidence,
        /// What the reader said, in its own words and carrying no value.
        failure: MetaFailure,
    },
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
