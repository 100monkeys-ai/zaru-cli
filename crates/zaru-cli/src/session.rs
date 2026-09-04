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
//! # `meta.toml` has no writer, and that is a dependency stop
//!
//! [ADR-0003] D2's table names no TOML crate. Its **first proposed
//! amendment** predicts this exact file — "ADR-0014 D1 and ADR-0010 D1 will
//! want `toml` on the same reading, for `~/.zaru/config.toml` and
//! `meta.toml`" — and its third proposes the crate outright; **neither is
//! accepted**, and the same amendment holds [ADR-0007] clause 4 and
//! [ADR-0014]'s file layers. So [`MetaStore`] is a port with no
//! implementation in this crate's product tree, exactly as ADR-0014's layers
//! 2, 3 and 5 are and as ADR-0007's sealing is.
//!
//! A `std`-only emitter for five flat keys was considered and refused. It is
//! two lines of `format!` only for values that never carry a quote, a
//! newline, a backslash or a control character, and `workspace` and
//! `provider` arrive from outside; getting TOML's basic-string escaping
//! *nearly* right and calling the file `meta.toml` is the "for now" the
//! harness forbids, and it would be a file the accepted crate later reads
//! differently. There is no honest `std`-only **reader** at all, because a
//! reader must survive whatever a person hand-edited.
//!
//! # This module designs no redaction, and the transcript is where a value lands
//!
//! [ADR-0008]'s trigger clause 6 is open and nothing here answers it.
//! ADR-0010's own Negative section is explicit about the consequence:
//! "Plain-text transcripts on disk contain whatever the session contained,
//! including secrets that appeared in command output. Filesystem permissions
//! are the only protection, and that is worth saying out loud rather than
//! implying encryption that does not exist." So the transcript carries what
//! it was given, the files carry `0600` and the directories `0700` — read
//! back off the filesystem rather than asserted from what the code asked for
//! — and **no filter is added anywhere**. What the checks assert instead is
//! that a planted value reaches the transcript, where the record puts it, and
//! reaches no refusal and no `Debug` of anything that is *about* a session
//! rather than *is* its data.
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
pub use meta::{Meta, MetaFailure, MetaStore};
pub use record::{FailureLine, Phase, Record, ToolCall};
pub use resume::{Interrupted, ResumeFailure, Resumed, resume};
pub use retention::{PruneFailure, Pruned, RetentionWindow, WindowRefused, prune};
pub use store::{
    CHECKPOINT_FILE, META_FILE, SESSIONS_DIRECTORY, Session, SessionError, SessionStore,
    TRANSCRIPT_FILE,
};
pub use transcript::{Reading, Transcript, TranscriptError};

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
