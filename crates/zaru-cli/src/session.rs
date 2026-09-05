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
//! | D1 — a directory named by a ULID, three files | the directory, the id, and two of the three files |
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
//! **Nothing in the product calls it yet.** The binary starts no session, so no
//! product path writes a `meta.toml` and none reads one; `resume` and `sessions
//! list` are deliberately not wired to it, because every session directory that
//! exists predates this writer and a read wired into resume would report a
//! defect for each of them.
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
pub mod id;
pub mod meta;
pub mod record;
pub mod resume;
pub mod retention;
pub mod store;
pub mod transcript;

pub use checkpoint::{Checkpoint, CheckpointError, TEMPORARY_SUFFIX};
pub use id::{
    ALPHABET, ID_LENGTH, Millis, MintFailure, SessionId, SessionIdRefused, SystemWallClock,
    WallClock,
};
pub use meta::file::MetaFile;
pub use meta::{Meta, MetaFailure, MetaStore};
pub use record::{FailureLine, Phase, Record, ToolCall};
pub use resume::{Interrupted, ResumeFailure, Resumed, resume};
pub use retention::{PruneFailure, Pruned, RetentionWindow, WindowRefused, prune, remove};
pub use store::{
    CHECKPOINT_FILE, META_FILE, SESSIONS_DIRECTORY, Session, SessionError, SessionStore,
    TRANSCRIPT_FILE,
};
pub use transcript::{Reading, Transcript, TranscriptError};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
