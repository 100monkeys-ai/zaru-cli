// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0005] D8's on-disk half: the hint strip's corpus, kept between sessions.
//!
//! # What this is for, in one measurement
//!
//! D8 is "the trie and its backing page map persist to disk", and until this
//! module nothing did. Measured from the release binary at `0c8ea16` over a
//! pseudo-terminal at 150 × 30, two sessions opened one after the other in one
//! directory: the first painted `looking in your notes…` from 519 ms and was
//! still painting it twelve seconds later, and the second was **byte-identical
//! to the first** — it had learned nothing, because `~/.zaru/` held
//! `credentials.json` and `sessions/` and no file a second session could read.
//! That is [ADR-0005]'s trigger clause 10b in as many words: "the corpus is
//! fetched again on every session, because it does not survive a restart".
//!
//! # `~/.zaru/corpus.jsonl`, keyed by a field and never by a filename
//!
//! One JSON object per line, each holding the instance `host`, the Nuclear
//! Notes `workspace` slug, the time the listings were taken, and the entries
//! the strip completes against. The key is **compared at read** rather than
//! encoded into a path, which is the shape
//! [`History`](crate::session::History) already takes for `history.jsonl` and
//! which `pane-navigation`'s amendment to [ADR-0010] D1 argues for at length:
//! a percent-encoded path overruns `NAME_MAX`, a mirrored directory tree
//! collides whenever one directory's mirror is another's leaf, and a digest
//! costs exactly the legibility D1's "inspectable with tools the user already
//! has" is about.
//!
//! # What a row carries, and why that is the whole of "no page body"
//!
//! [`Row`] has **three fields and a kind**: the path, the title, the entity
//! kind. It is deliberately **not** a mirror of
//! [`Listed`](zaru_notes::session::Listed), and not a serialisation of
//! [`CachedEntry`] either — it is what [`Trie`](zaru_notes::trie::Trie)
//! indexes and nothing else. `session::listing::read` accepts rows carrying
//! more than this client reads, and it always will; a row type that named the
//! fields it wanted would drift into carrying whatever the server grew next.
//! So "no page body is cached" is a property of the type rather than a rule
//! somebody keeps, and `the_cache_file_holds_no_page_body_and_no_token` is
//! what says so from outside the crate.
//!
//! **No credential reaches this file.** The bearer lives in
//! [`Secret`](crate::credentials::Secret), which has one door named for the
//! single place it may go, and nothing here can reach it: the host is the
//! instance's name and the entries are what the token could already read.
//! [ADR-0007] D3 governs the credential store and is untouched by this file,
//! because this file holds **content the token could read, not the token**.
//!
//! # Appending, and then one line per key
//!
//! A landed refresh is [`CorpusCache::append`]ed — opened `append` at
//! [`FILE_MODE`], the line and its newline in one `write_all`, `flush`,
//! `sync_data` — which is the discipline `session::transcript` and
//! `session::history` share and which is not described a third time. The
//! reader takes the **last** matching line, so an append supersedes without a
//! rewrite, and [`CorpusCache::compact`] at session open leaves one line per
//! key through [`crate::atomic::write`], exactly as `History::compact` does.
//! [`CorpusCache::evict`] is that same rewrite with one key dropped, and it
//! happens at once rather than at the next open: a token that has lost its
//! membership must not serve its old view of a workspace for one more session.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::credentials::store::FILE_MODE;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use zaru_notes::trie::{CachedEntry, EntryKind};

/// The eighth thing under `~/.zaru/`, per [ADR-0010] D1.
///
/// The seven before it are `node.key` ([ADR-0004] D3), `sessions/`
/// ([ADR-0010] D1), `config.toml` ([ADR-0014] D1), `commands/`
/// ([ADR-0015] D3), `credentials.json` ([ADR-0007]), `history.jsonl`
/// ([ADR-0010] D1's amendment of 2026-09-15) and `admissions.jsonl`
/// ([ADR-0015] D3's). The enumeration is read from
/// `crate::credentials::store`'s own module documentation rather than
/// assembled here.
///
/// [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub const CORPUS_FILE: &str = "corpus.jsonl";

/// What kind of entity a cached row names, as the file spells it.
///
/// This crate's own mirror of [`EntryKind`], for the reason
/// `crate::terminal::trie`'s adapter exists at all: [ADR-0003] D8 gives
/// `zaru-notes` no serde dependency, and deriving one there to write a file
/// this crate owns would put a serialisation format in the crate that is kept
/// arm's-length and publishable. The composition root converts.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    /// A page.
    Page,
    /// An atom.
    Atom,
}

impl RowKind {
    /// The kind the trie holds.
    #[must_use]
    pub const fn cached(self) -> EntryKind {
        match self {
            Self::Page => EntryKind::Page,
            Self::Atom => EntryKind::Atom,
        }
    }

    /// The kind this file spells, from the one the trie holds.
    #[must_use]
    pub const fn of(kind: EntryKind) -> Self {
        match kind {
            EntryKind::Page => Self::Page,
            EntryKind::Atom => Self::Atom,
        }
    }
}

/// One cached entity: what the trie indexes, and nothing else.
///
/// Three fields and a kind. See the module documentation for why this is not
/// a mirror of the listing row the server sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// The entity's path inside the workspace this line names.
    pub path: String,
    /// The entity's title.
    pub title: String,
    /// Page or atom.
    pub kind: RowKind,
}

/// One line of `corpus.jsonl`: a key, a time, and a corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    /// The instance the listings were taken from.
    pub host: String,
    /// The Nuclear Notes workspace slug they cover.
    pub workspace: String,
    /// When they were taken, in milliseconds since the Unix epoch.
    ///
    /// The same unit `session::Meta`'s `started` uses, so two files under
    /// `~/.zaru/` do not spell a time two ways.
    pub fetched: u128,
    /// The entities the strip completes against.
    pub entries: Vec<Row>,
}

/// A corpus read back off disk: what it holds and when it was taken.
///
/// Named for the corpus rather than for the act because
/// [`credentials::Cached`](crate::credentials::Cached) is a cached **tool
/// scope** and the two are unrelated; one crate holding two `Cached` types is
/// a reader's problem rather than a compiler's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedCorpus {
    /// The entries, as the trie takes them.
    pub entries: Vec<CachedEntry>,
    /// When the listings behind them were taken.
    pub fetched: u128,
}

/// What can go wrong reading or writing the cache.
#[derive(Debug)]
pub enum CorpusError {
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

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the cached corpus at {} did not parse: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "a cached corpus line could not be rendered: {detail}")
            }
        }
    }
}

impl std::error::Error for CorpusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// The hint strip's corpus, as it sits under `~/.zaru/`.
#[derive(Debug, Clone)]
pub struct CorpusCache {
    path: PathBuf,
}

impl CorpusCache {
    /// The cache at an explicit path.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The cache under a `~/.zaru`-equivalent root.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(CORPUS_FILE))
    }

    /// Where the file is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every complete line the file holds, oldest first.
    ///
    /// An absent file reads as no lines: a machine that has never fetched a
    /// corpus has not gone wrong. Everything after the last newline is the
    /// line that was in flight and is never counted — `session::transcript`'s
    /// own rule, and the reason a killed process costs at most one append.
    ///
    /// # Errors
    ///
    /// [`CorpusError::Io`] for a file that will not read, and
    /// [`CorpusError::Malformed`] naming the line that did not parse.
    pub fn lines(&self) -> Result<Vec<Line>, CorpusError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(CorpusError::Io {
                    action: "read the cached corpus",
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
                serde_json::from_str(line).map_err(|error| CorpusError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// The corpus cached for one instance and workspace, if any.
    ///
    /// **The last matching line wins**, because an append supersedes rather
    /// than duplicating: a refresh that lands between two compactions leaves
    /// two lines for one key and the later one is the fresher.
    ///
    /// # Errors
    ///
    /// As [`CorpusCache::lines`].
    pub fn read(&self, host: &str, workspace: &str) -> Result<Option<CachedCorpus>, CorpusError> {
        Ok(self
            .lines()?
            .into_iter()
            .rfind(|line| line.host == host && line.workspace == workspace)
            .map(|line| CachedCorpus {
                entries: line
                    .entries
                    .into_iter()
                    .map(|row| {
                        CachedEntry::new(&line.workspace, row.path, row.title, row.kind.cached())
                    })
                    .collect(),
                fetched: line.fetched,
            }))
    }

    /// Record a corpus that has just been fetched.
    ///
    /// # Errors
    ///
    /// [`CorpusError::NotSerialisable`] and [`CorpusError::Io`].
    pub fn append(
        &self,
        host: &str,
        workspace: &str,
        entries: &[CachedEntry],
        fetched: u128,
    ) -> Result<(), CorpusError> {
        let line = Line {
            host: host.to_owned(),
            workspace: workspace.to_owned(),
            fetched,
            entries: entries
                .iter()
                .map(|entry| Row {
                    path: entry.path.clone(),
                    title: entry.title.clone(),
                    kind: RowKind::of(entry.kind),
                })
                .collect(),
        };
        let mut rendered =
            serde_json::to_string(&line).map_err(|error| CorpusError::NotSerialisable {
                detail: error.to_string(),
            })?;
        rendered.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| CorpusError::Io {
                action: "open the cached corpus for appending",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(rendered.as_bytes())
            .map_err(|source| CorpusError::Io {
                action: "append to the cached corpus",
                path: self.path.clone(),
                source,
            })?;
        file.flush().map_err(|source| CorpusError::Io {
            action: "flush the cached corpus",
            path: self.path.clone(),
            source,
        })?;
        file.sync_data().map_err(|source| CorpusError::Io {
            action: "sync the cached corpus",
            path: self.path.clone(),
            source,
        })
    }

    /// Leave one line per key, the newest, and report whether anything moved.
    ///
    /// # Errors
    ///
    /// As [`CorpusCache::lines`], plus [`CorpusError::Io`] for a rewrite that
    /// will not land.
    pub fn compact(&self) -> Result<bool, CorpusError> {
        let lines = self.lines()?;
        let kept = newest_per_key(&lines);
        if kept.len() == lines.len() {
            return Ok(false);
        }
        self.rewrite(&kept)?;
        Ok(true)
    }

    /// Forget one instance and workspace, at once.
    ///
    /// Called where the instance **answered and refused**: a token that can no
    /// longer read a workspace must not keep serving its old view of it, and
    /// leaving that to the next compaction would serve it for one more
    /// session. Reports whether anything was there.
    ///
    /// # Errors
    ///
    /// As [`CorpusCache::compact`].
    pub fn evict(&self, host: &str, workspace: &str) -> Result<bool, CorpusError> {
        let lines = self.lines()?;
        let kept: Vec<Line> = newest_per_key(&lines)
            .into_iter()
            .filter(|line| !(line.host == host && line.workspace == workspace))
            .collect();
        if kept.len() == lines.len() {
            return Ok(false);
        }
        self.rewrite(&kept)?;
        Ok(true)
    }

    /// Write the file whole, atomically, at [`FILE_MODE`].
    fn rewrite(&self, lines: &[Line]) -> Result<(), CorpusError> {
        let mut rendered = String::new();
        for line in lines {
            let text =
                serde_json::to_string(line).map_err(|error| CorpusError::NotSerialisable {
                    detail: error.to_string(),
                })?;
            rendered.push_str(&text);
            rendered.push('\n');
        }
        if rendered.is_empty() && !self.path.exists() {
            return Ok(());
        }
        crate::atomic::write(&self.path, rendered.as_bytes(), FILE_MODE).map_err(|failed| {
            CorpusError::Io {
                action: "rewrite the cached corpus",
                path: failed.path.clone(),
                source: failed.source,
            }
        })
    }
}

/// The newest line for each key, in the file's own order.
///
/// Keeping the file's order rather than sorting means a compaction changes
/// which lines are there and never the order they arrived in — `History`'s own
/// rule, for the same reason: a file whose bytes move without its meaning
/// moving is a file nobody can diff.
fn newest_per_key(lines: &[Line]) -> Vec<Line> {
    let mut kept: Vec<Line> = Vec::with_capacity(lines.len());
    for (at, line) in lines.iter().enumerate() {
        let newest = lines
            .iter()
            .rposition(|other| other.host == line.host && other.workspace == line.workspace);
        if newest == Some(at) {
            kept.push(line.clone());
        }
    }
    kept
}

/// Milliseconds since the Unix epoch, or zero on a machine whose clock is
/// before it.
///
/// Zero rather than a refusal, and the reason is what the number is for: it
/// is shown to a person so they know how old their corpus is, and refusing to
/// cache a corpus because a clock is wrong would trade a real capability for a
/// cosmetic one. A zero renders as the epoch, which reads as obviously wrong
/// rather than as plausibly recent.
#[must_use]
pub fn now_in_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis())
}

/// A stored time as a person reads it: `2026-09-15 04:12 UTC`.
///
/// # UTC, said out loud rather than quietly assumed
///
/// The standard library carries no timezone database and no local offset, so
/// a harness rendering a local time would be **guessing** one — and a guess
/// that is eight hours wrong looks exactly like a corpus that is eight hours
/// stale, which is the one thing this line exists to say correctly. So the
/// zone is UTC and the string says `UTC`, which is honest and four characters.
/// [ADR-0003] D2's table is closed on purpose and a date crate is not in it.
///
/// The arithmetic is the civil-from-days algorithm, shifted so the era starts
/// at March: it is exact for every day this program can be handed and needs no
/// table of month lengths, because the March-based year makes the leap day the
/// last day rather than a hole in the middle.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
#[must_use]
pub fn stamp(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1000).unwrap_or(i64::MAX);
    let days = seconds.div_euclid(86_400);
    let within = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (hour, minute) = (within / 3600, (within % 3600) / 60);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// The civil date for a count of days since 1970-01-01, proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so that a four-century era is exactly
    // 146,097 days and February's length is never a special case.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = u32::try_from(day_of_year - (153 * march_month + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if march_month < 10 {
        march_month + 3
    } else {
        march_month - 9
    })
    .unwrap_or(1);
    (year + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests;
