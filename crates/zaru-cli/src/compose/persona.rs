// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0027] D1's served persona, as [ADR-0013] D1's layer 1 gets it.
//!
//! # What this builds, and what [ADR-0027] D1 actually says
//!
//! D1 is an **init call**: it "activates Zaru for a session and returns the
//! system prompt for a mode, the available tool names, and the version", with
//! a mode call beside it, both idempotent. **That is not what this is, and the
//! difference is stated here rather than rounded up.** What this module does is
//! read **one page** out of the workspace the project pins, at a path the user
//! sets, through the surface the harness already reads with — no init call, no
//! mode call, no tool names, no version, no scope. It delivers D1's *effect* —
//! the persona is authored in one place, served, and consumed by the client's
//! LLM, with **no sentence of it compiled into this harness** — through a
//! mechanism D1 does not describe. D5's third-party portability is **not**
//! delivered: a page at a path is portable only to a client that has been told
//! the same path in the same workspace, where D1's init call would have been
//! portable to any client holding the tools. Both are recorded on that
//! record's amendments page as a coordinator ruling of 2026-09-15 open to
//! Jeshua's veto.
//!
//! # Absence is exactly what it was, and that is the load-bearing half
//!
//! No page, no token, no pin, no path, an unreadable cache, or a refusal all
//! leave [`prose::NO_PERSONA`](crate::compose::prose::NO_PERSONA) in layer 1,
//! **byte for byte as before this module existed**, and nothing is authored to
//! say so to a person. That constant is therefore **kept**. ADR-0027's Update
//! of 2026-09-05 says "the day D1's fetch exists, that constant is deleted
//! rather than edited"; that sentence assumed a fetch that always succeeds,
//! and deleting the constant would silently change what a model receives on
//! every machine with no Nuclear Notes token — which is every machine today. A
//! dated correction stands beside it on the record.
//!
//! # The beat is **not** the corpus's, and the reason is a satisfied clause
//!
//! [ADR-0005] D3's corpus arrives **into** an already-open shell and mutates
//! the trie behind an `RwLock`. A persona cannot: [ADR-0013] trigger clause 1
//! — "layers 1 to 4 are byte-identical across every turn of a long session" —
//! is **already satisfied**, held by [`StablePrefix`] having no method taking
//! `&mut self`, and its two checks "watched red … by rewriting the prefix
//! mid-session inside `compact`". A background fetch that landed in layer 1
//! would be exactly that mutation, and it would destroy the prompt caching D1
//! calls "an architectural constraint rather than an optimisation".
//!
//! So:
//!
//! - **At open, synchronously, before `prefix_for`**: [`CachedPersona`] is read
//!   off disk. A hit is layer 1 and costs no network.
//! - **On a miss, one synchronous fetch**, unbounded beyond `reqwest`'s own
//!   timeouts — the same absence of a bound [`corpus_at`](crate::credentials::corpus_at)
//!   already has, and a number nobody chose is worse than none.
//! - **On a hit, a background refresh** on the runtime the shell already
//!   holds, writing the file **for the next session only**. It cannot reach
//!   this session's prefix, because the prefix was built before it started.
//! - **Never mid-session.**
//!
//! **A cost this leaves, stated rather than discovered.** `zaru "<task>"` runs
//! one turn and exits, so there is no runtime that outlives its prefix and
//! **no background refresh happens on that path** — a cache hit is used and
//! nothing is fetched behind it. A person who only ever runs one-shot tasks
//! sees a persona change on their next terminal session rather than on their
//! next task. This is the corpus's own shape (`zaru "<task>"` touches
//! `corpus.jsonl` not at all) and it is named here rather than left to be met.
//!
//! # `~/.zaru/persona.jsonl`, the **tenth** thing under that directory
//!
//! The nine before it are `node.key` ([ADR-0004] D3), `sessions/`
//! ([ADR-0010] D1), `config.toml` ([ADR-0014] D1), `credentials.json`
//! ([ADR-0007]), `commands/` ([ADR-0015] D3), `history.jsonl` ([ADR-0010] D1's
//! amendment of 2026-09-15), `admissions.jsonl` (ADR-0015 D4's),
//! `corpus.jsonl` ([ADR-0005] D3's) and `tips.jsonl` ([ADR-0002] D8's). One
//! JSON object per line, keyed by the instance `host`, the workspace and the
//! **path**, compared at read rather than encoded into a filename — every rule
//! [`CorpusCache`](crate::terminal::corpus::CorpusCache) already argues for at
//! length, borrowed verbatim and not restated: append at [`FILE_MODE`], last
//! matching line wins, [`PersonaCache::compact`] once at open through
//! [`crate::atomic::write`], [`PersonaCache::evict`] at once on a refusal, everything
//! after the last newline never counted.
//!
//! # This file holds a page body, and `corpus.jsonl`'s rule does not forbid it
//!
//! `corpus.jsonl` holds no page body because its `Row` type has three fields
//! and a kind, and its stated reason is **drift** — "a row type that named the
//! fields it wanted would drift into carrying whatever the server grew next".
//! That is a property of that file's type, not a rule about bodies. Its other
//! claim, that **no credential** reaches it, is about the harness's own bearer
//! and holds here too: nothing puts a [`Secret`](crate::credentials::Secret)
//! in this file either.
//!
//! [ADR-0010] D5 — "the user can read every byte the harness stores about them
//! with `cat`" — is **satisfied** by this file rather than strained: mode
//! `0600`, one JSON object per line. And [ADR-0027] D5 removes any
//! confidentiality claim over the content: "Serving the persona to third-party
//! clients means it is readable by anyone with a token. Accepted — it is
//! prompt text, not a moat."
//!
//! **The body is cached rather than a hash of it, and the alternative's cost
//! is measured.** Layer 1 needs the *text*, so a hash-only cache could not
//! serve it: every session with a persona would have to reach the network
//! before its prefix existed — the exact negative ADR-0027's own Consequences
//! names — and an offline session would have no persona at all, ever, where a
//! body cache gives it yesterday's. The fetch that would have to happen every
//! time is one `attach` plus one `pages.read`; the same attach plus **two**
//! listings measured one to two seconds on 2026-09-14.
//!
//! # The disk seam is redacted, and that is why this file calls the port
//!
//! The body is written through [`Redacted::by`] over the same
//! [`HeldSecrets`](crate::redaction::HeldSecrets) the prompt passes, so
//! **`persona.jsonl` never holds what a model would not see**. It is the only
//! write path in this workspace that calls that port, and it is the ninth row
//! on `no_captured_bytes_reach_a_prompt_except_through_the_port`'s enumeration
//! for that reason — the row says what it is, because the other eight are
//! prompts and this one is a file.
//!
//! Ruled 2026-09-15 by the coordinator under directive 20, open to Jeshua's
//! veto. The prompt seam would have covered the model; it would not have
//! covered a person reading the file with `cat`, which ADR-0010 D5 invites
//! them to do.
//!
//! # `persona.path`, and the project layer is closed to it
//!
//! [ADR-0014] D6: a repository the user cloned "must not be able to configure
//! its way to more privilege than the user granted". **A persona is a system
//! prompt**, so a cloned repository that could set its path could hand the
//! user's model a prompt of the repository's choosing. That is the same
//! escalation `provider.<kind>.endpoint` and `notes.<alias>.agent_tools` are
//! already closed for, and it is the strongest of the three. A **proposed
//! sixth escalation** on D6, in their shape.
//!
//! **One residual risk, accepted and recorded rather than silently taken.**
//! [ADR-0006] D5's pin — `project.workspace` — **is** the project's, so a
//! cloned repository still chooses *which workspace* the persona is read from,
//! though only a workspace the user's own token can already read and only at
//! the path the user set. Requiring a user-layer workspace for the persona was
//! considered and refused: it is a key no record asks for, and the pin already
//! scopes everything the composer shows the user. Recorded on ADR-0027's
//! amendments page.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
//! [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
//! [`FILE_MODE`]: crate::credentials::store::FILE_MODE
//! [`StablePrefix`]: zaru_core::context::StablePrefix

use crate::config::{Field, FieldKind, Key, Resolution, Schema, Value};
use crate::credentials::store::FILE_MODE;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use zaru_core::redaction::{Redacted, Redactor};

/// The file, inside `~/.zaru/`. A **tenth** thing under that directory.
///
/// See the module documentation for the nine before it. Accepted 2026-09-15
/// under directive 20 as a delegated coordinator ruling, open to Jeshua's
/// veto, and written on [ADR-0010's amendments volume 3].
///
/// [ADR-0010's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript-updates-3
pub const PERSONA_FILE: &str = "persona.jsonl";

/// The [ADR-0014] key naming which page carries the persona.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const KEY: &str = "persona.path";

/// The page this harness reads when nobody has said otherwise.
///
/// **A path and not a sentence**, which is why it lives here rather than in
/// [`crate::compose::prose`] and why `compose::emission`'s walk gains no
/// member for it: nothing about it is read by a person as prose. The name is
/// batched for Jeshua.
pub const DEFAULT_PATH: &str = "zaru/persona";

/// Why a project may not name the persona's page, in the words the refusal
/// carries.
///
/// One string, in [`crate::credentials::grant::PROJECT_REFUSAL`]'s shape, so
/// the fold's refusal and this module's cannot give a user two different
/// reasons for one rule.
pub const PROJECT_REFUSAL: &str = "which page carries your assistant's persona is a system prompt, \
                                   and a repository you cloned must not be able to choose what \
                                   your model is told it is; set it in ~/.zaru/config.toml instead";

/// The persona key.
///
/// # Panics
///
/// Never. [`KEY`] is two well-formed segments.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("`persona.path` is a well-formed key")
}

/// What the key holds, and what the project layer may do to it.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Text, PROJECT_REFUSAL)
}

/// Declare the persona key on a schema.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

/// Which page this session's persona is read from.
///
/// **Absent is [`DEFAULT_PATH`]**, which is the shape
/// [`tips::enabled`](crate::compose::tips::enabled) already uses for an
/// absent key: no built-in row is declared at [ADR-0014] D1's layer 1, so
/// `zaru config explain persona.path` reports honestly that nobody has set it
/// while the harness still knows where to look.
///
/// A value that is not text has already been refused by the schema.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn path_in(resolution: &Resolution) -> String {
    match resolution.get(&key()) {
        Some(Value::Text(path)) if !path.is_empty() => path.clone(),
        _ => DEFAULT_PATH.to_owned(),
    }
}

/// One line of `persona.jsonl`: a key, a time, and a body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The instance the page was read from.
    pub host: String,
    /// The Nuclear Notes workspace it lives in.
    pub workspace: String,
    /// The path inside that workspace.
    pub path: String,
    /// When it was read, in milliseconds since the Unix epoch.
    ///
    /// The same unit `session::Meta`'s `started` and `corpus.jsonl`'s
    /// `fetched` use, so three files under `~/.zaru/` do not spell a time
    /// three ways.
    pub fetched: u128,
    /// The page's body, **as the server sent it and as a model will see it**,
    /// through the redaction port. See the module documentation.
    pub body: String,
}

/// A persona read back off disk: what it holds and when it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPersona {
    /// The body, as layer 1 takes it.
    pub body: String,
    /// When the page behind it was read.
    pub fetched: u128,
}

/// What can go wrong reading or writing the cache.
#[derive(Debug)]
pub enum PersonaError {
    /// The file could not be read or written.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The file it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A complete line did not parse.
    Malformed {
        /// The file the line is in.
        path: PathBuf,
        /// Which line, counting from one.
        line: usize,
        /// What the reader expected.
        detail: String,
    },
    /// A line could not be rendered.
    NotSerialisable {
        /// What the serialiser said.
        detail: String,
    },
}

impl fmt::Display for PersonaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the cached persona at {} did not parse: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "a cached persona line could not be rendered: {detail}")
            }
        }
    }
}

impl std::error::Error for PersonaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// [ADR-0027]'s persona, as it sits under `~/.zaru/`.
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[derive(Debug, Clone)]
pub struct PersonaCache {
    path: PathBuf,
}

impl PersonaCache {
    /// The cache at an explicit path.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The cache under a `~/.zaru`-equivalent root.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(PERSONA_FILE))
    }

    /// Where the file is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every complete line the file holds, oldest first.
    ///
    /// An absent file reads as no lines. Everything after the last newline is
    /// the line that was in flight and is never counted — `session::transcript`
    /// and `terminal::corpus` share that rule, and the reason a killed process
    /// costs at most one append.
    ///
    /// # Errors
    ///
    /// [`PersonaError::Io`] for a file that will not read, and
    /// [`PersonaError::Malformed`] naming the line that did not parse.
    pub fn lines(&self) -> Result<Vec<Line>, PersonaError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(PersonaError::Io {
                    action: "read the cached persona",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        let complete = raw.rfind('\n').map_or("", |at| &raw[..=at]);
        complete
            .lines()
            .enumerate()
            .map(|(at, line)| {
                serde_json::from_str(line).map_err(|error| PersonaError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// The persona cached for one instance, workspace and path, if any.
    ///
    /// **The last matching line wins**, because an append supersedes rather
    /// than duplicating.
    ///
    /// # Errors
    ///
    /// As [`PersonaCache::lines`].
    pub fn read(
        &self,
        host: &str,
        workspace: &str,
        path: &str,
    ) -> Result<Option<CachedPersona>, PersonaError> {
        Ok(self
            .lines()?
            .into_iter()
            .rfind(|line| line.host == host && line.workspace == workspace && line.path == path)
            .map(|line| CachedPersona {
                body: line.body,
                fetched: line.fetched,
            }))
    }

    /// Record a persona that has just been read, **through the redaction
    /// port**.
    ///
    /// The body reaching the file is the body a model would see: see the
    /// module documentation for why the disk seam is redacted and not only the
    /// prompt seam.
    ///
    /// # Errors
    ///
    /// [`PersonaError::NotSerialisable`] and [`PersonaError::Io`].
    pub fn append<R: Redactor + ?Sized>(
        &self,
        host: &str,
        workspace: &str,
        path: &str,
        body: &str,
        fetched: u128,
        redactor: &R,
    ) -> Result<(), PersonaError> {
        let line = Line {
            host: host.to_owned(),
            workspace: workspace.to_owned(),
            path: path.to_owned(),
            fetched,
            body: Redacted::by(redactor, body).as_str().to_owned(),
        };
        let mut rendered =
            serde_json::to_string(&line).map_err(|error| PersonaError::NotSerialisable {
                detail: error.to_string(),
            })?;
        rendered.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| PersonaError::Io {
                action: "open the cached persona for appending",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(rendered.as_bytes())
            .map_err(|source| PersonaError::Io {
                action: "append to the cached persona",
                path: self.path.clone(),
                source,
            })?;
        file.flush().map_err(|source| PersonaError::Io {
            action: "flush the cached persona",
            path: self.path.clone(),
            source,
        })?;
        file.sync_data().map_err(|source| PersonaError::Io {
            action: "sync the cached persona",
            path: self.path.clone(),
            source,
        })
    }

    /// Leave one line per key, the newest, and report whether anything moved.
    ///
    /// # Errors
    ///
    /// As [`PersonaCache::lines`], plus [`PersonaError::Io`] for a rewrite that
    /// will not land.
    pub fn compact(&self) -> Result<bool, PersonaError> {
        let lines = self.lines()?;
        let kept = newest_per_key(&lines);
        if kept.len() == lines.len() {
            return Ok(false);
        }
        self.rewrite(&kept)?;
        Ok(true)
    }

    /// Forget one instance, workspace and path, at once.
    ///
    /// Called where the instance **answered and refused**: a token that can no
    /// longer read a page must not keep serving its old body as a system
    /// prompt, and leaving that to the next compaction would serve it for one
    /// more session. Reports whether anything was there.
    ///
    /// # Errors
    ///
    /// As [`PersonaCache::compact`].
    pub fn evict(&self, host: &str, workspace: &str, path: &str) -> Result<bool, PersonaError> {
        let lines = self.lines()?;
        let kept: Vec<Line> = newest_per_key(&lines)
            .into_iter()
            .filter(|line| !(line.host == host && line.workspace == workspace && line.path == path))
            .collect();
        if kept.len() == lines.len() {
            return Ok(false);
        }
        self.rewrite(&kept)?;
        Ok(true)
    }

    /// Write the file whole, atomically, at [`FILE_MODE`].
    fn rewrite(&self, lines: &[Line]) -> Result<(), PersonaError> {
        let mut rendered = String::new();
        for line in lines {
            let text =
                serde_json::to_string(line).map_err(|error| PersonaError::NotSerialisable {
                    detail: error.to_string(),
                })?;
            rendered.push_str(&text);
            rendered.push('\n');
        }
        if rendered.is_empty() && !self.path.exists() {
            return Ok(());
        }
        crate::atomic::write(&self.path, rendered.as_bytes(), FILE_MODE).map_err(|failed| {
            PersonaError::Io {
                action: "rewrite the cached persona",
                path: failed.path.clone(),
                source: failed.source,
            }
        })
    }
}

/// The newest line for each key, in the file's own order.
///
/// Keeping the file's order rather than sorting means a compaction changes
/// which lines are there and never the order they arrived in — `History`'s and
/// `CorpusCache`'s rule, for the same reason.
fn newest_per_key(lines: &[Line]) -> Vec<Line> {
    let mut kept: Vec<Line> = Vec::with_capacity(lines.len());
    for (at, line) in lines.iter().enumerate() {
        let newest = lines.iter().rposition(|other| {
            other.host == line.host && other.workspace == line.workspace && other.path == line.path
        });
        if newest == Some(at) {
            kept.push(line.clone());
        }
    }
    kept
}

/// What one persona fetch did, in the three shapes [ADR-0005] D8's rule has.
///
/// **Three and not a `Result<_, String>`**, for the reason
/// [`Refresh`](crate::terminal::open::Refresh) is three: an instance that never
/// answered says nothing about whether this token may still read this page,
/// while an instance that answered and refused has said exactly that.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
#[derive(Debug)]
pub enum Fetched {
    /// The page came back, and the file now holds it.
    Reached(String),
    /// No transport: the instance said nothing, so the cache stands.
    Unreachable(String),
    /// The instance answered and refused, so the entry is gone from the file.
    Refused(String),
}

impl Fetched {
    /// The body this fetch can serve, if any.
    ///
    /// `Unreachable` and `Refused` serve none: the first because the caller
    /// keeps whatever the cache already gave it, the second because the file
    /// has just been emptied of it.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        match self {
            Self::Reached(body) => Some(body),
            Self::Unreachable(_) | Self::Refused(_) => None,
        }
    }
}

/// What one fetch's answer does to the file, and what the caller is then told.
///
/// **Separate from the fetch because the decision is the whole of the freshness
/// rule and the fetch is a socket.** A check cannot open an instance, and
/// driving this with a staged answer exercises the same branch, the same writes
/// and the same sentences the binary takes.
///
/// **The discriminator is whether the instance answered.** It is
/// [`ReachFailure::Refused`](crate::credentials::ReachFailure) that says so,
/// and only that arm forgets anything — the same reading `refresh_from`
/// records having got wrong once, where `Endpoint` was read as "no transport"
/// and a person on a train would have had their cache evicted.
#[must_use]
pub fn fetched_from<R: Redactor + ?Sized>(
    answer: Result<String, crate::credentials::ReachFailure>,
    cache: &PersonaCache,
    host: &str,
    workspace: &str,
    path: &str,
    fetched: u128,
    redactor: &R,
) -> Fetched {
    match answer {
        Ok(body) => {
            drop(cache.append(host, workspace, path, &body, fetched, redactor));
            // The body handed back is the **redacted** one, so that what a
            // model is shown and what the file holds are one string rather
            // than two that could differ. Reading it back off the cache would
            // be a second file read on the session's own path; redacting twice
            // is idempotent and costs nothing.
            Fetched::Reached(Redacted::by(redactor, &body).as_str().to_owned())
        }
        Err(
            failure @ (crate::credentials::ReachFailure::Endpoint(_)
            | crate::credentials::ReachFailure::Session(_)),
        ) => Fetched::Unreachable(failure.to_string()),
        Err(failure @ crate::credentials::ReachFailure::Refused(_)) => {
            drop(cache.evict(host, workspace, path));
            Fetched::Refused(failure.to_string())
        }
    }
}

/// Everything a later refresh needs, and nothing more.
///
/// **It holds a [`Secret`](crate::credentials::Secret) and therefore renders
/// nothing**, which is [`Populating`](crate::terminal::open::Populating)'s own
/// property and is inherited rather than restated: that type has no `Display`,
/// its `Debug` prints a fixed marker, and the one function that exposes the
/// value is named for the single place it is allowed to go.
///
/// **Public only because it appears in [`Serving`]**, and opaque otherwise:
/// every field is private and there is no constructor outside this module, so
/// a caller can hold one and hand it back and can read nothing out of it.
pub struct Refreshing {
    host: String,
    workspace: String,
    path: String,
    secret: crate::credentials::Secret,
    cache: PersonaCache,
    held: crate::redaction::HeldSecrets,
}

impl fmt::Debug for Refreshing {
    /// Names what it holds and renders none of it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Refreshing")
            .field("host", &self.host)
            .field("workspace", &self.workspace)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Refreshing {
    /// Read the page again and leave the file agreeing with the instance.
    ///
    /// **What this cannot do is change the session it was started from.** The
    /// prefix was built before this was spawned and [`StablePrefix`] has no
    /// method that changes it, so the only thing this can affect is the next
    /// session — which is [ADR-0013] trigger clause 1 being a property of the
    /// type rather than a rule this function keeps.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    /// [`StablePrefix`]: zaru_core::context::StablePrefix
    pub async fn refresh(self) -> Fetched {
        let answer =
            crate::credentials::persona_at(&self.host, &self.secret, &self.path, &self.workspace)
                .await;
        fetched_from(
            answer,
            &self.cache,
            &self.host,
            &self.workspace,
            &self.path,
            crate::terminal::corpus::now_in_millis(),
            &self.held,
        )
    }
}

/// What this session's layer 1 is, and what a later refresh would need.
///
/// `body` is `None` on every machine that has no pin, no token, no readable
/// cache and no reachable page — which is every machine today — and that is
/// **not** a failure: it is [ADR-0027]'s absence, and the prefix says so in
/// [`prose::NO_PERSONA`](crate::compose::prose::NO_PERSONA).
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[derive(Debug)]
pub struct Serving {
    body: Option<String>,
    refreshing: Option<Refreshing>,
}

impl Serving {
    /// Nothing served and nothing to refresh.
    #[must_use]
    pub const fn nothing() -> Self {
        Self {
            body: None,
            refreshing: None,
        }
    }

    /// What layer 1 takes, if anything.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    /// The refresh, taken out so the caller can spawn it.
    ///
    /// `None` where there is nothing to refresh **and where the fetch already
    /// happened on this thread**: a session that missed the cache has just
    /// read the page, and reading it twice in one session would spend a rate
    /// budget against an instance that has already answered — the same "there
    /// is deliberately no retry" [ADR-0005] D3's corpus states for itself.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    #[must_use]
    pub fn take_refreshing(&mut self) -> Option<Refreshing> {
        self.refreshing.take()
    }
}

/// [ADR-0027]'s persona for one session, resolved **before** its prefix exists.
///
/// The whole beat is in the module documentation. In one paragraph: the cache
/// is read on this thread; a hit is layer 1 and the refresh is handed back for
/// the caller to spawn; a miss is one synchronous fetch, unbounded beyond
/// `reqwest`'s own, and nothing is handed back because the page has just been
/// read.
///
/// **Every absence returns [`Serving::nothing`] rather than an error**, and
/// that is the whole of [ADR-0027]'s 2026-09-05 Update being preserved: no
/// pin, no store, no token, no key, an unreadable cache and an unreachable
/// instance all leave layer 1 exactly as it was before this function existed.
/// A persona must not be able to stop a session starting, which is
/// `credential_store`'s own rule for the hint strip arriving somewhere that
/// matters more.
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[must_use]
pub fn for_session(resolution: &Resolution, workspace: Option<&str>) -> Serving {
    // ADR-0006 D5's pin. No pin is no workspace to read a page out of, and
    // there is nothing to fall back to: the handshake's `_grounding.you` was
    // measured on 2026-09-14 and does not name a workspace a token can read.
    let Some(workspace) = workspace.filter(|pinned| !pinned.is_empty()) else {
        return Serving::nothing();
    };
    let Ok(root) = crate::credentials::CredentialStore::default_root() else {
        return Serving::nothing();
    };
    let Ok(store) = crate::credentials::CredentialStore::reading(root) else {
        return Serving::nothing();
    };
    let Some((_, host, secret)) = crate::credentials::composer_secret(&store) else {
        return Serving::nothing();
    };
    // ADR-0008 clause 6's port, over every value the store holds. Built here
    // because the disk seam is redacted with the same redactor the prompt
    // seam uses -- see the module documentation.
    let keyring = crate::credentials::OsKeyring::for_store(store.root());
    let keys = crate::credentials::HarnessKeys::from_process(&keyring);
    let Ok(held) = crate::redaction::held_secrets_for_redaction(&store, &keys) else {
        return Serving::nothing();
    };

    let path = path_in(resolution);
    let cache = PersonaCache::under(store.root());
    // The compaction happens **here**, once as a session opens, for the reason
    // `History`'s and `CorpusCache`'s do: a rewrite races an append, and doing
    // it at open leaves a window the width of one session's start rather than
    // one per refresh.
    drop(cache.compact());

    match cache.read(&host, workspace, &path).ok().flatten() {
        Some(hit) => Serving {
            body: Some(hit.body),
            refreshing: Some(Refreshing {
                host,
                workspace: workspace.to_owned(),
                path,
                secret,
                cache,
                held,
            }),
        },
        None => {
            // One synchronous fetch, on a runtime built for it and dropped
            // again -- the shape `compose::turn::block_on` already has, and
            // not a nested one: this function is called before the shell's
            // runtime is entered and before `task`'s.
            let Ok(runtime) = crate::compose::turn::runtime() else {
                return Serving::nothing();
            };
            let answer = runtime.block_on(crate::credentials::persona_at(
                &host, &secret, &path, workspace,
            ));
            let fetched = fetched_from(
                answer,
                &cache,
                &host,
                workspace,
                &path,
                crate::terminal::corpus::now_in_millis(),
                &held,
            );
            Serving {
                body: fetched.body().map(str::to_owned),
                refreshing: None,
            }
        }
    }
}

#[cfg(test)]
mod tests;
