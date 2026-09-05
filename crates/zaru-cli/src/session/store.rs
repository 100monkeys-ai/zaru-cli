// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D1's directory: where a session lives, what it is called, and
//! what it holds.
//!
//! # Unix only, for the reason the credential store is
//!
//! D5 says filesystem permissions are the only thing protecting a transcript,
//! and modes are a Unix concept. A build that carried no modes while every
//! check still passed is [Verification lessons] §26 exactly — a rule holding
//! by circumstance reading identically to one holding by construction. The
//! credential store refuses to build off Unix for the same reason and the
//! Windows question has its own row on the ADR backlog.
//!
//! # The root is a parameter, and the directory has one creator
//!
//! [`SessionStore::open`] takes a root because ADR-0014's layers can move it
//! and because that is what lets a check be an ordinary caller writing to the
//! paths the product writes to. It creates `sessions/` and each session's own
//! directory through [`crate::config::home::ensure`], which is the harness's
//! single creator of a `0700` directory under `~/.zaru` — see that module.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

#[cfg(not(unix))]
compile_error!(
    "the session store enforces ADR-0010 D5's file modes through std::os::unix, and D5 says \
     filesystem permissions are the only protection a transcript has. Building here without \
     those modes would leave every transcript readable by every process on the machine while \
     every check still passed. See the ADR backlog row \"Windows support strategy\"."
);

use crate::config::home::{self, HomeFailure};
use crate::failure::{SessionEvidence, SessionId as EvidenceId};
use crate::session::id::{SessionId, SessionIdRefused};
use core::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// The directory sessions live under, inside `~/.zaru/`. ADR-0010 D1.
pub const SESSIONS_DIRECTORY: &str = "sessions";

/// D1's append-only transcript.
pub const TRANSCRIPT_FILE: &str = "transcript.jsonl";

/// D1's checkpoint.
pub const CHECKPOINT_FILE: &str = "context.json";

/// D1's metadata. Written by [`MetaFile`](crate::session::MetaFile) since
/// 2026-09-05, and by nothing in the product, because the binary starts no
/// session — see [`crate::session::meta::file`].
pub const META_FILE: &str = "meta.toml";

/// The mode every file in a session directory carries.
///
/// Read from the credential store's declaration rather than re-typed: ADR-0004
/// D3 set the precedent and one transcription of a mode is one thing to get
/// right.
pub use crate::credentials::store::{DIRECTORY_MODE, FILE_MODE};

/// Something went wrong reaching a session on disk.
///
/// **No variant carries a session's contents.** A refusal is what gets pasted
/// into a bug report, and a session directory's name is read off a filesystem
/// a person can write to.
#[derive(Debug)]
pub enum SessionError {
    /// No home directory could be resolved.
    NoHome,
    /// A directory under `~/.zaru` could not be made ready.
    Home(HomeFailure),
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A directory under `sessions/` is not named by a ULID.
    NotASessionName {
        /// Why the name was refused. **Not the name itself.**
        refusal: SessionIdRefused,
    },
    /// There is no session directory with that id.
    NoSuchSession {
        /// The id asked for. A ULID this crate minted, so it carries nothing
        /// a person wrote.
        id: SessionId,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHome => f.write_str(
                "no home directory could be resolved, so there is nowhere to put ~/.zaru/sessions",
            ),
            Self::Home(failure) => write!(f, "{failure}"),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
            Self::NotASessionName { refusal } => write!(
                f,
                "a directory under ~/.zaru/sessions/ is not named by a ULID: {refusal}"
            ),
            Self::NoSuchSession { id } => {
                write!(f, "there is no session directory named {id}")
            }
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Home(failure) => Some(failure),
            Self::NotASessionName { refusal } => Some(refusal),
            Self::NoHome | Self::NoSuchSession { .. } => None,
        }
    }
}

impl From<HomeFailure> for SessionError {
    fn from(failure: HomeFailure) -> Self {
        Self::Home(failure)
    }
}

/// Where every session on this machine lives.
#[derive(Debug)]
pub struct SessionStore {
    root: PathBuf,
}

impl SessionStore {
    /// Where sessions live when nobody says otherwise.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoHome`] when no home directory can be resolved.
    pub fn default_root() -> Result<PathBuf, SessionError> {
        crate::config::home::default_root().ok_or(SessionError::NoHome)
    }

    /// Open the store under `root`, making `root` and `root/sessions` ready.
    ///
    /// Both directories get [`DIRECTORY_MODE`] on **every** open rather than
    /// only on creation, for the reason the credential store re-asserts its
    /// own: a session directory readable by every process on the machine is a
    /// defect whoever created it, and D5 says the mode is the only protection
    /// a transcript has.
    ///
    /// # Errors
    ///
    /// [`SessionError::Home`] when either directory cannot be made ready.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, SessionError> {
        let root = root.into();
        home::ensure(&root)?;
        let store = Self { root };
        home::ensure(&store.sessions_directory())?;
        Ok(store)
    }

    /// Reach the store under `root` **without creating anything**.
    ///
    /// [ADR-0014]'s port carries the argument this exists for: "a loader that
    /// created a directory in order to find nothing in it would be creating
    /// state to read state". A user asking `zaru sessions list` on a machine
    /// that has never run a session is owed an empty listing, not a `~/.zaru`
    /// they did not ask for and an inode ADR-0010 D6's own Negative section
    /// counts.
    ///
    /// **The mode is not asserted here, and that is the trade.** [`open`]
    /// re-asserts `0700` on every call because it is about to write; this
    /// never writes, so it has nothing to protect and nothing to fix. A caller
    /// that is going to write calls [`open`].
    ///
    /// [`open`]: SessionStore::open
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    #[must_use]
    pub fn reading(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The `~/.zaru`-equivalent directory this store sits under.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/sessions`.
    #[must_use]
    pub fn sessions_directory(&self) -> PathBuf {
        self.root.join(SESSIONS_DIRECTORY)
    }

    /// Create the directory for a session that is starting.
    ///
    /// The id is a parameter rather than minted here, so that a check names
    /// the millisecond its sessions were started at rather than asserting on
    /// the machine's clock.
    ///
    /// # Errors
    ///
    /// [`SessionError::Home`] when the directory cannot be made ready.
    pub fn start(&self, id: SessionId) -> Result<Session, SessionError> {
        let directory = self.sessions_directory().join(id.as_str());
        home::ensure(&directory)?;
        Ok(Session { id, directory })
    }

    /// Reach a session that already exists.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoSuchSession`] when there is no such directory.
    pub fn existing(&self, id: &SessionId) -> Result<Session, SessionError> {
        let directory = self.sessions_directory().join(id.as_str());
        if !directory.is_dir() {
            return Err(SessionError::NoSuchSession { id: id.clone() });
        }
        Ok(Session {
            id: id.clone(),
            directory,
        })
    }

    /// Every session on this machine, **in creation order**.
    ///
    /// D1's whole reason for a ULID: the order is the sort, and the sort is
    /// lexical over the directory names, so this costs a directory read.
    ///
    /// A directory whose name is not a ULID is **reported, never deleted and
    /// never silently skipped**: the harness does not own what it cannot
    /// identify, and a listing that quietly omitted a name would make
    /// [`crate::session::retention`]'s pruning look complete over a
    /// population it never saw ([Verification lessons] §17).
    ///
    /// # Errors
    ///
    /// [`SessionError::Io`] when the directory cannot be listed, and
    /// [`SessionError::NotASessionName`] for an entry that is not a ULID.
    ///
    /// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
    pub fn ids(&self) -> Result<Vec<SessionId>, SessionError> {
        let directory = self.sessions_directory();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            // A directory that was never created holds no sessions, which is
            // a different statement from a directory that cannot be read. A
            // store reached through `reading` has created nothing, so this is
            // the ordinary case on a machine that has never run a session.
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(source) => {
                return Err(SessionError::Io {
                    action: "list the sessions directory",
                    path: directory.clone(),
                    source,
                });
            }
        };

        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| SessionError::Io {
                action: "read an entry of the sessions directory",
                path: directory.clone(),
                source,
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let id = SessionId::parse(&name)
                .map_err(|refusal| SessionError::NotASessionName { refusal })?;
            ids.push(id);
        }
        ids.sort();
        Ok(ids)
    }
}

/// One session's directory, and the paths inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    id: SessionId,
    directory: PathBuf,
}

impl Session {
    /// D1's ULID.
    #[must_use]
    pub const fn id(&self) -> &SessionId {
        &self.id
    }

    /// The directory itself.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// D1's `transcript.jsonl`.
    #[must_use]
    pub fn transcript_path(&self) -> PathBuf {
        self.directory.join(TRANSCRIPT_FILE)
    }

    /// D1's `context.json`.
    #[must_use]
    pub fn checkpoint_path(&self) -> PathBuf {
        self.directory.join(CHECKPOINT_FILE)
    }

    /// D1's `meta.toml`. [`MetaFile`](crate::session::MetaFile) is what reads
    /// and writes it; nothing in the product calls that yet, because this
    /// binary starts no session.
    #[must_use]
    pub fn meta_path(&self) -> PathBuf {
        self.directory.join(META_FILE)
    }

    /// What [ADR-0016] D3's defect boundary is told about this session.
    ///
    /// **This is the producer of the seam that record left open.** D3 says a
    /// defect report names the session and says the transcript is on disk,
    /// and `SessionEvidence::NoSessionExists` exists so that claiming a
    /// transcript that was never written is unrepresentable. This is the
    /// other arm.
    ///
    /// The `expect` is unreachable rather than optimistic:
    /// [`EvidenceId::new`](crate::failure::SessionId::new) refuses an empty id
    /// and one carrying a control character, and a
    /// [`SessionId`] is twenty-six characters of Crockford base32 by both of
    /// its constructors. The mutant that would make it fire is widening
    /// [`SessionId::parse`] to accept a name off the filesystem unchecked.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub fn evidence(&self) -> SessionEvidence {
        SessionEvidence::Session {
            id: EvidenceId::new(self.id.as_str())
                .expect("a ULID is neither empty nor a control character"),
            transcript: self.transcript_path(),
        }
    }
}
