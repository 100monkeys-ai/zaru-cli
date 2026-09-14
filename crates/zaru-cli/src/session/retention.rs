// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D6: bounded retention, and deletion that is deletion.
//!
//! D6: "Sessions older than a configurable window (default 30 days) are
//! pruned on startup... **Deletion removes the directory rather than marking
//! it deleted.** A harness that keeps a tombstone the user believed was gone
//! has broken a promise that is very cheap to keep."
//!
//! # The window is a parameter and the module carries no default
//!
//! D6 names one — thirty days — and unlike ADR-0007's TTL or ADR-0008's
//! ceiling that number is in a record rather than invented. It is still not
//! here: a compiled-in default is [ADR-0014] D1's **layer 1**, which is the
//! configuration hierarchy's to hold, and that record's schema declares no
//! key for this. A key chosen by the code that reads it is a name nobody
//! decided. So the window arrives from the caller, is refused at zero in the
//! shape [`Ttl`](crate::credentials::Ttl) and
//! [`RetryCeiling`](crate::failure::RetryCeiling) already use, and D6's
//! thirty days is recorded on the record for whoever declares the key.
//!
//! # The age is in the name, not on the filesystem
//!
//! A directory's modification time moves every time its transcript is
//! appended to, so an old session still in use reads as young; a directory
//! that was copied or restored from a backup reads as new. The ULID [ADR-0010]
//! D1 chose already carries the creation millisecond, exactly, so
//! [`SessionId::minted_at`](crate::session::SessionId::minted_at) is the age
//! and the filesystem is not consulted for it at all.
//!
//! # What is not pruned, and why each is not
//!
//! **The current session**, because pruning runs at startup and a session
//! that has just been created is younger than any window — until a clock that
//! went backwards says otherwise, which is a machine's problem and not a
//! reason to delete the transcript being written. It is passed in and skipped.
//!
//! **A directory whose name is not a ULID**, because the harness does not own
//! what it cannot identify. [`SessionStore::ids`](crate::session::SessionStore::ids)
//! reports one rather than skipping it, so pruning refuses rather than
//! silently working over a population it never saw.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::session::id::{Millis, SessionId};
use crate::session::store::{SessionError, SessionStore};
use core::fmt;
use core::time::Duration;
use std::fs;

/// A retention window the harness cannot work with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRefused;

impl fmt::Display for WindowRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a retention window of zero is refused; sessions older than the \
             window are pruned on startup, and a window of zero deletes the session that is \
             starting; thirty days is the default and this module carries none, because a \
             compiled-in default is the built-in layer and no schema declares a key for it",
        )
    }
}

impl std::error::Error for WindowRefused {}

/// How long a session is kept. [ADR-0010] D6.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionWindow(Duration);

impl RetentionWindow {
    /// Take a window from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`WindowRefused`] when `window` is zero.
    pub const fn new(window: Duration) -> Result<Self, WindowRefused> {
        if window.is_zero() {
            return Err(WindowRefused);
        }
        Ok(Self(window))
    }

    /// The window.
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }

    /// Whether a session minted at `minted_at` is past this window at `now`.
    ///
    /// Saturating, so a clock that went backwards makes every session look
    /// young rather than making one look infinitely old. Deleting a
    /// transcript because a laptop's clock jumped is not a trade this record
    /// would make.
    #[must_use]
    pub fn is_past(self, minted_at: Millis, now: Millis) -> bool {
        u128::from(now.since(minted_at)) > self.0.as_millis()
    }
}

/// What pruning removed, and what it left.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pruned {
    /// The sessions whose directories were removed, in creation order.
    pub removed: Vec<SessionId>,
    /// The sessions that were inside the window, in creation order.
    pub kept: Vec<SessionId>,
    /// The current session, if one was named and it was past the window.
    ///
    /// Reported rather than silently kept: a user whose window is shorter
    /// than their session is entitled to know the rule did not fire, and
    /// **[Verification lessons] §30 — a comment is not a mechanism** — a
    /// skip nobody can observe is a skip nobody can check.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    pub spared_as_current: Option<SessionId>,
}

/// Something went wrong pruning.
#[derive(Debug)]
pub enum PruneFailure {
    /// The sessions directory could not be read.
    Store(SessionError),
    /// A session directory could not be removed.
    NotRemoved {
        /// Which session.
        id: SessionId,
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl fmt::Display for PruneFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(failure) => write!(f, "{failure}"),
            Self::NotRemoved { id, source } => write!(
                f,
                "the session {id} is past the retention window and its directory could not be \
                 removed: {source}. Deletion is real rather than a tombstone, so \
                 a removal that half happened is worse than one that did not start"
            ),
        }
    }
}

impl std::error::Error for PruneFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(failure) => Some(failure),
            Self::NotRemoved { source, .. } => Some(source),
        }
    }
}

/// Remove every session past the window. [ADR-0010] D6.
///
/// `now` is a reading the caller took, not one this function takes: a
/// function that read the clock could not be checked against a session aged
/// exactly at the boundary.
///
/// `current` is the session this process is running, which is never pruned.
///
/// # Errors
///
/// [`PruneFailure::Store`] when the sessions directory cannot be listed —
/// including when it holds a directory that is not named by a ULID, which is
/// reported rather than deleted — and [`PruneFailure::NotRemoved`] when a
/// directory cannot be removed.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn prune(
    store: &SessionStore,
    window: RetentionWindow,
    now: Millis,
    current: Option<&SessionId>,
) -> Result<Pruned, PruneFailure> {
    let ids = store.ids().map_err(PruneFailure::Store)?;
    let mut pruned = Pruned::default();

    for id in ids {
        if !window.is_past(id.minted_at(), now) {
            pruned.kept.push(id);
            continue;
        }
        if current == Some(&id) {
            pruned.spared_as_current = Some(id.clone());
            pruned.kept.push(id);
            continue;
        }
        remove(store, &id)?;
        pruned.removed.push(id);
    }

    Ok(pruned)
}

/// Remove one session's directory. [ADR-0010] D6.
///
/// **The one deletion path in this crate**, called by [`prune`] as well as by
/// `zaru sessions rm`, so D6's "deletion removes the directory rather than
/// marking it deleted" is one implementation rather than two that agree today.
///
/// It takes no `current` and spares nothing. D6's guard belongs to the caller
/// that knows whether a session is the one it is inside — which, outside a
/// session, is nobody: the `zaru sessions rm` entry point has no current
/// session to spare, and `/session rm` inside one does. See
/// [`prune`]'s `current`.
///
/// # Errors
///
/// [`PruneFailure::Store`] when there is no such session, and
/// [`PruneFailure::NotRemoved`] when the directory will not go.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
pub fn remove(store: &SessionStore, id: &SessionId) -> Result<(), PruneFailure> {
    let directory = store.sessions_directory().join(id.as_str());
    if !directory.is_dir() {
        return Err(PruneFailure::Store(SessionError::NoSuchSession {
            id: id.clone(),
        }));
    }
    fs::remove_dir_all(&directory).map_err(|source| PruneFailure::NotRemoved {
        id: id.clone(),
        source,
    })
}
