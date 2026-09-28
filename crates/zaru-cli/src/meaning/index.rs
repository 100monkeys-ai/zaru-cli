// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The index of one project: its pieces and their vectors, on disk.
//!
//! # What is kept, and where
//!
//! One folder per project under `~/.zaru/meaning/index/`, named by a digest
//! of the project's path. It holds three files:
//!
//! - `about.json`: the project's path, the model's name, how many numbers a
//!   vector has and which rule cut the pieces. An index made by another model
//!   or cut by another rule is built again.
//! - `chunks.jsonl`: one line per piece, readable with `cat`: its file, the
//!   digest of the file it came from, its first and last line, and the
//!   digest of its text.
//! - `vectors.f32`: the vectors, in the order of `chunks.jsonl`, each 768
//!   little-endian 32-bit numbers. Not readable with `cat`; it holds nothing
//!   `chunks.jsonl` does not name.
//!
//! No text of the code is kept in the index, only where it is and a digest
//! of it.
//!
//! # What is indexed
//!
//! What `fs.search` searches by default, less tests: the walk honours
//! `.gitignore`, `.ignore` and `.git/info/exclude`, skips the folders in
//! [`crate::tools::searching::SKIPPED_FOLDERS`], lock, minified and generated
//! files, binary files, files that are not UTF-8 and files over the search
//! ceiling, and never follows a link. At most [`MOST_FILES`] files and
//! [`MOST_CHUNKS`] pieces; a tree over either is indexed up to it, in path
//! order, and says so.

use super::chunk::{self, Chunk};
use super::embed::similarity;
use crate::config::SizeCeiling;
use crate::tools::searching::{self, Options};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The most files one project's index holds.
pub const MOST_FILES: usize = 4_000;

/// The most pieces one project's index holds: about 61 MB of vectors.
pub const MOST_CHUNKS: usize = 20_000;

/// The mode every file of an index is written at: the owner's alone.
const FILE_MODE: u32 = 0o600;

/// One piece as the index holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    /// Its first line.
    pub start: usize,
    /// Its last line.
    pub end: usize,
    /// The digest of its text.
    pub hash: String,
    /// Its vector.
    pub vector: Vec<f32>,
}

/// One file as the index holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Indexed {
    /// The digest of the whole file when it was indexed.
    pub hash: String,
    /// Its pieces.
    pub chunks: Vec<Stored>,
}

/// The index of one project.
#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    /// The model its vectors were made by.
    pub model: String,
    /// How many numbers each vector has.
    pub dimensions: usize,
    /// Each file, by its path relative to the project.
    pub files: BTreeMap<String, Indexed>,
}

/// A file the walk found and would index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Its path relative to the project.
    pub path: String,
    /// The digest of its contents.
    pub hash: String,
    /// Its contents.
    pub text: String,
}

/// What a walk of the project found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    /// The files to index, in path order, at most [`MOST_FILES`].
    pub candidates: Vec<Candidate>,
    /// Files past [`MOST_FILES`], not indexed.
    pub over: usize,
}

/// One piece of a file that is waiting to be embedded.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    /// The file's path.
    pub path: String,
    /// The file's digest.
    pub file_hash: String,
    /// The piece.
    pub chunk: Chunk,
    /// The digest of its text.
    pub hash: String,
    /// Its vector, when the file changed but this piece did not.
    pub vector: Option<Vec<f32>>,
}

/// The digest of `text`, in hexadecimal: the first 16 bytes of its SHA-256.
#[must_use]
pub fn digest(text: &str) -> String {
    super::fetch::hex(&Sha256::digest(text.as_bytes())[..16])
}

/// The folder a project's index is kept in, under `indexes`.
#[must_use]
pub fn folder_for(indexes: &Path, project: &Path) -> PathBuf {
    indexes.join(digest(&project.display().to_string()))
}

/// Walk `project` the way `fs.search` does by default, keeping the files an
/// index holds.
#[must_use]
pub fn walk(project: &Path, ceiling: SizeCeiling) -> Walk {
    let folders = Arc::new(Mutex::new(BTreeSet::new()));
    let mut walk = Walk::default();
    for entry in searching::walker(project, &Options::default(), folders).flatten() {
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() || kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(project) else {
            continue;
        };
        let shown = relative.display().to_string();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if searching::is_generated_name(&name) || searching::is_test(&shown) {
            continue;
        }
        let size = std::fs::symlink_metadata(path).map_or(u64::MAX, |metadata| metadata.len());
        if size > ceiling.get() {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes[..bytes.len().min(8 * 1024)].contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if searching::is_marked_generated(&text) || text.trim().is_empty() {
            continue;
        }
        if walk.candidates.len() >= MOST_FILES {
            walk.over += 1;
            continue;
        }
        walk.candidates.push(Candidate {
            path: shown,
            hash: digest(&text),
            text,
        });
    }
    walk
}

impl Index {
    /// An empty index for `model`.
    #[must_use]
    pub fn empty(model: &str, dimensions: usize) -> Self {
        Self {
            model: model.to_owned(),
            dimensions,
            files: BTreeMap::new(),
        }
    }

    /// How many pieces it holds.
    #[must_use]
    pub fn chunks(&self) -> usize {
        self.files.values().map(|file| file.chunks.len()).sum()
    }

    /// Bring the index in line with `walk`: drop what is gone or changed, and
    /// return the pieces waiting to be embedded, in path order. A piece whose
    /// text is unchanged keeps its vector. Past [`MOST_CHUNKS`], nothing more
    /// waits, and the count of pieces left out is returned too.
    pub fn plan(&mut self, walk: &Walk) -> (Vec<Waiting>, usize) {
        let present: BTreeSet<&str> = walk.candidates.iter().map(|c| c.path.as_str()).collect();
        self.files.retain(|path, _| present.contains(path.as_str()));
        let mut room = MOST_CHUNKS.saturating_sub(self.chunks());
        let mut waiting = Vec::new();
        let mut left_out = 0;
        for candidate in &walk.candidates {
            if self
                .files
                .get(&candidate.path)
                .is_some_and(|indexed| indexed.hash == candidate.hash)
            {
                continue;
            }
            let old = self.files.remove(&candidate.path);
            let known: BTreeMap<String, Vec<f32>> = old
                .map(|indexed| {
                    indexed
                        .chunks
                        .into_iter()
                        .map(|stored| (stored.hash, stored.vector))
                        .collect()
                })
                .unwrap_or_default();
            let pieces = chunk::chunks(Path::new(&candidate.path), &candidate.text);
            // A file too short to give a piece is not recorded, so it is not
            // counted as indexed.
            if pieces.is_empty() {
                continue;
            }
            if pieces.len() > room {
                left_out += pieces.len();
                continue;
            }
            room -= pieces.len();
            let mut kept = Vec::new();
            let mut fresh = Vec::new();
            for piece in pieces {
                let hash = digest(&piece.text);
                match known.get(&hash) {
                    Some(vector) => kept.push(Stored {
                        start: piece.start,
                        end: piece.end,
                        hash,
                        vector: vector.clone(),
                    }),
                    None => fresh.push(Waiting {
                        path: candidate.path.clone(),
                        file_hash: candidate.hash.clone(),
                        chunk: piece,
                        hash,
                        vector: None,
                    }),
                }
            }
            if fresh.is_empty() {
                kept.sort_by_key(|stored| stored.start);
                self.files.insert(
                    candidate.path.clone(),
                    Indexed {
                        hash: candidate.hash.clone(),
                        chunks: kept,
                    },
                );
            } else {
                // The file is recorded when its last piece is embedded; the
                // pieces it kept travel with the ones that wait.
                for stored in kept {
                    waiting.push(Waiting {
                        path: candidate.path.clone(),
                        file_hash: candidate.hash.clone(),
                        chunk: Chunk {
                            start: stored.start,
                            end: stored.end,
                            text: String::new(),
                        },
                        hash: stored.hash,
                        vector: Some(stored.vector),
                    });
                }
                waiting.extend(fresh);
            }
        }
        (waiting, left_out)
    }

    /// Record a file whose pieces all have vectors.
    pub fn record(&mut self, path: &str, hash: &str, mut chunks: Vec<Stored>) {
        chunks.sort_by_key(|stored| stored.start);
        self.files.insert(
            path.to_owned(),
            Indexed {
                hash: hash.to_owned(),
                chunks,
            },
        );
    }

    /// The pieces nearest `query`, best first: at most `most`, only in files
    /// under `within` (a path relative to the project, empty for all).
    #[must_use]
    pub fn nearest(&self, query: &[f32], within: &str, most: usize) -> Vec<Near> {
        let mut found: Vec<Near> = self
            .files
            .iter()
            .filter(|(path, _)| under(path, within))
            .flat_map(|(path, indexed)| {
                indexed.chunks.iter().map(move |stored| Near {
                    path: path.clone(),
                    start: stored.start,
                    end: stored.end,
                    score: similarity(query, &stored.vector),
                })
            })
            .collect();
        found.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.start.cmp(&right.start))
        });
        found.truncate(most);
        found
    }

    /// Read the index kept in `folder`, or `None` when there is none, it is
    /// damaged, or it was made by another model.
    #[must_use]
    pub fn load(folder: &Path, model: &str, dimensions: usize) -> Option<Self> {
        let about: About =
            serde_json::from_slice(&std::fs::read(folder.join("about.json")).ok()?).ok()?;
        if about.model != model || about.dimensions != dimensions || about.rule != chunk::RULE {
            return None;
        }
        let lines = std::fs::read_to_string(folder.join("chunks.jsonl")).ok()?;
        let vectors = std::fs::read(folder.join("vectors.f32")).ok()?;
        let rows: Vec<Row> = lines
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()
            .ok()?;
        if vectors.len() != rows.len() * dimensions * 4 {
            return None;
        }
        let mut index = Self::empty(model, dimensions);
        for (at, row) in rows.into_iter().enumerate() {
            let bytes = &vectors[at * dimensions * 4..(at + 1) * dimensions * 4];
            let vector = bytes
                .chunks_exact(4)
                .map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]]))
                .collect();
            index
                .files
                .entry(row.path)
                .or_insert_with(|| Indexed {
                    hash: row.file.clone(),
                    chunks: Vec::new(),
                })
                .chunks
                .push(Stored {
                    start: row.start,
                    end: row.end,
                    hash: row.hash,
                    vector,
                });
        }
        Some(index)
    }

    /// Keep the index in `folder`, for the project at `project`.
    ///
    /// # Errors
    ///
    /// A sentence naming the file that could not be written.
    pub fn save(&self, folder: &Path, project: &Path) -> Result<(), String> {
        std::fs::create_dir_all(folder)
            .map_err(|failure| format!("could not make {}: {failure}", folder.display()))?;
        let about = About {
            root: project.display().to_string(),
            model: self.model.clone(),
            dimensions: self.dimensions,
            rule: chunk::RULE,
        };
        let mut lines = String::new();
        let mut vectors = Vec::with_capacity(self.chunks() * self.dimensions * 4);
        for (path, indexed) in &self.files {
            for stored in &indexed.chunks {
                let row = Row {
                    path: path.clone(),
                    file: indexed.hash.clone(),
                    start: stored.start,
                    end: stored.end,
                    hash: stored.hash.clone(),
                };
                lines
                    .push_str(&serde_json::to_string(&row).map_err(|failure| failure.to_string())?);
                lines.push('\n');
                for number in &stored.vector {
                    vectors.extend_from_slice(&number.to_le_bytes());
                }
            }
        }
        let write = |name: &str, bytes: &[u8]| {
            crate::atomic::write(&folder.join(name), bytes, FILE_MODE).map_err(|failure| {
                format!(
                    "could not write {}: {}",
                    failure.path.display(),
                    failure.source
                )
            })
        };
        write(
            "about.json",
            serde_json::to_string(&about)
                .map_err(|failure| failure.to_string())?
                .as_bytes(),
        )?;
        write("vectors.f32", &vectors)?;
        write("chunks.jsonl", lines.as_bytes())
    }
}

/// Whether `path` is under `within`, both relative to the project.
fn under(path: &str, within: &str) -> bool {
    within.is_empty() || Path::new(path).starts_with(within)
}

/// A piece near a query.
#[derive(Debug, Clone, PartialEq)]
pub struct Near {
    /// Its file, relative to the project.
    pub path: String,
    /// Its first line.
    pub start: usize,
    /// Its last line.
    pub end: usize,
    /// Its cosine similarity to the query.
    pub score: f32,
}

#[derive(Serialize, Deserialize)]
struct About {
    root: String,
    model: String,
    dimensions: usize,
    /// [`chunk::RULE`] when the index was cut.
    #[serde(default)]
    rule: u32,
}

#[derive(Serialize, Deserialize)]
struct Row {
    path: String,
    file: String,
    start: usize,
    end: usize,
    hash: String,
}
