// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Retrieval by meaning: `fs.search` also finds code whose words differ from
//! the question's.
//!
//! # Off unless a person turns it on
//!
//! It needs a model of about 436 MB and a runtime library of about 24 MB that
//! are not part of `zaru`. Nothing is fetched until a person runs `zaru index
//! fetch` and answers yes, and nothing runs until they set [`KEY`] to `true`
//! in `~/.zaru/config.toml` or `ZARU_SEARCH_MEANING`. A repository they cloned
//! cannot turn it on: it costs a download and the machine's time. With it
//! off, nothing in this module runs and `fs.search` answers as it did.
//!
//! # What it does when on
//!
//! A session builds an index of the project in the background: the project's
//! files are cut into pieces ([`chunk`]), each piece is turned into a vector
//! by the model ([`embed`]), and the vectors are kept on disk ([`index`]).
//! The status line says how far it is and how long is left. It stops when
//! the session ends and carries on where it stopped the next time.
//!
//! A search of two or more words also embeds the question and lists the
//! pieces nearest in meaning, ranked together with the text and declaration
//! matches (see [`crate::tools::searching`]). Before any of the index exists
//! the search says so and answers by text alone. When part of it exists, the
//! part is searched and the answer says how much of the tree that is.
//!
//! Each search first brings the index up to date: files whose contents
//! changed are cut and embedded again, by the digest of their contents, and
//! files that are gone are dropped. There is no watcher.
//!
//! # Nothing leaves the machine
//!
//! The model runs here. No text of the project is sent anywhere to be
//! indexed, and the index holds no text, only where each piece is and a
//! digest of it.

pub mod chunk;
pub mod embed;
pub mod fetch;
pub mod index;
pub mod said;

use crate::config::{Field, FieldKind, Key, Resolution, Schema, SizeCeiling, Value};
use embed::Embedder;
use index::{Index, Near, Waiting};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

/// The key, spelled here and nowhere else.
pub const KEY: &str = "search.meaning";

/// What layer 1 holds: off.
pub const BUILT_IN: bool = false;

/// Why the project layer may not set [`KEY`].
pub const PROJECT_REFUSAL: &str = "retrieval by meaning uses a model that costs a 460 MB \
                                   download and this machine's time, and a repository you cloned \
                                   must not turn it on; set it in ~/.zaru/config.toml or \
                                   ZARU_SEARCH_MEANING instead";

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("search.meaning is a well-formed key")
}

/// What [`KEY`] holds: a boolean the project layer may not set.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Bool, PROJECT_REFUSAL)
}

/// Declare [`KEY`] into a caller's schema.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

/// Whether retrieval by meaning is on.
#[must_use]
pub fn on(resolution: &Resolution) -> bool {
    match resolution.get(&key()) {
        Some(Value::Bool(on)) => *on,
        _ => BUILT_IN,
    }
}

/// Where the files of retrieval by meaning are kept, under `~/.zaru`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// `~/.zaru/meaning`.
    pub root: PathBuf,
}

impl Places {
    /// The places under the harness's folder `home`.
    #[must_use]
    pub fn under(home: &Path) -> Self {
        Self {
            root: home.join("meaning"),
        }
    }

    /// The model's folder.
    #[must_use]
    pub fn model(&self) -> PathBuf {
        self.root.join("model")
    }

    /// The runtime library's folder.
    #[must_use]
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// The folder every project's index is under.
    #[must_use]
    pub fn indexes(&self) -> PathBuf {
        self.root.join("index")
    }

    /// Whether every file the model and runtime need is here. Their digests
    /// are checked when they are loaded, not here.
    #[must_use]
    pub fn fetched(&self) -> bool {
        let model = self.model();
        fetch::MODEL_FILES
            .iter()
            .all(|(name, ..)| model.join(name).is_file())
            && self.runtime().join(fetch::LIBRARY).is_file()
    }
}

/// Fetch the model and the runtime library into `places`, checking each
/// file's digest before it is kept. How far each file is goes to standard
/// error, about every tenth of it.
///
/// # Errors
///
/// A sentence saying which file could not be fetched or kept, and why.
pub async fn fetch_all(
    places: &Places,
    runtime: &fetch::Runtime,
) -> Result<(), fetch::FetchFailure> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|failure| fetch::FetchFailure(format!("could not start a client: {failure}")))?;
    let mut sources: Vec<(fetch::Source, PathBuf)> = fetch::model_sources()
        .into_iter()
        .map(|source| (source, places.model()))
        .collect();
    sources.push((runtime.archive.clone(), places.runtime()));
    for (source, folder) in &sources {
        let size = said::megabytes(source.bytes);
        let mut last = 0;
        let mut told = |arrived: u64| {
            let tenth = arrived * 10 / source.bytes.max(1);
            if tenth > last && source.bytes >= 1_000_000 {
                last = tenth;
                eprintln!("{}: {} of {size}", source.kept_as, said::megabytes(arrived));
            }
        };
        fetch::download(&client, source, folder, &mut told).await?;
    }
    fetch::unpack(
        &places.runtime().join(&runtime.archive.kept_as),
        runtime,
        &places.runtime(),
    )?;
    Ok(())
}

/// How far the index is.
#[derive(Debug, Clone, PartialEq)]
pub enum Progress {
    /// The model's files are being checked and loaded.
    Loading,
    /// The model is loaded, and the index is being compared with the files.
    Checking,
    /// Pieces are being embedded.
    Building {
        /// Pieces embedded so far in this run.
        done: usize,
        /// Pieces to embed in this run.
        total: usize,
        /// About how long is left, once there is a rate to go by.
        left: Option<Duration>,
    },
    /// The index is up to date with the tree.
    Ready {
        /// Files indexed.
        files: usize,
        /// Pieces indexed.
        chunks: usize,
        /// Files and pieces past the limits, not indexed.
        left_out: usize,
    },
    /// It stopped, and why.
    Failed(String),
}

impl Progress {
    /// What the status line says, full and narrow, or `None` when there is
    /// nothing to say: an index that is ready says nothing.
    #[must_use]
    pub fn status(&self) -> Option<(String, String)> {
        match self {
            Self::Loading => Some((
                String::from("index: loading the model"),
                String::from("index loading"),
            )),
            Self::Checking => Some((
                String::from("index: checking which files changed"),
                String::from("index checking"),
            )),
            Self::Building { done, total, left } => {
                let percent = if *total == 0 { 100 } else { done * 100 / total };
                let full = match left {
                    Some(left) => {
                        format!("indexing {done}/{total}, about {} left", duration(*left))
                    }
                    None => format!("indexing {done}/{total}"),
                };
                Some((full, format!("index {percent}%")))
            }
            Self::Ready { .. } => None,
            Self::Failed(_) => Some((
                String::from("index stopped: zaru index says why"),
                String::from("index stopped"),
            )),
        }
    }
}

/// A duration as a person reads it: seconds under two minutes, then minutes.
#[must_use]
pub fn duration(time: Duration) -> String {
    let seconds = time.as_secs();
    if seconds < 120 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds.div_ceil(60))
    }
}

/// What a search asked of the index found.
#[derive(Debug, Clone, PartialEq)]
pub enum Nearest {
    /// The pieces nearest the question, best first.
    Found {
        /// The pieces, at most [`CANDIDATES`].
        places: Vec<Near>,
        /// Files in the index.
        indexed: usize,
        /// Files the tree holds that an index would hold.
        files: usize,
        /// How far the index is.
        progress: Progress,
    },
    /// Retrieval by meaning cannot answer yet, and why.
    NotReady(String),
}

/// How many pieces a search takes from the index before ranking them with
/// the text and declaration matches.
pub const CANDIDATES: usize = 30;

/// How many pieces the builder embeds between saves of the index.
pub const SAVE_EVERY: usize = 256;

/// How long a search waits for the model to load, and for an idle index to
/// take in changed files.
pub const REFRESH_WAIT: Duration = Duration::from_secs(10);

/// What loads the embedder, on the builder's thread.
pub type Loader = Box<dyn FnOnce() -> Result<Box<dyn Embedder>, String> + Send>;

/// Retrieval by meaning for one session: the index of one project, built in
/// the background.
pub struct Meaning {
    inner: Inner,
}

enum Inner {
    /// It is on, and cannot run, for the reason held.
    Unavailable(String),
    /// It runs.
    Running(Arc<Shared>),
}

impl core::fmt::Debug for Meaning {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.inner {
            Inner::Unavailable(reason) => {
                f.debug_tuple("Meaning::Unavailable").field(reason).finish()
            }
            Inner::Running(_) => f.debug_struct("Meaning::Running").finish_non_exhaustive(),
        }
    }
}

struct Shared {
    project: PathBuf,
    folder: PathBuf,
    ceiling: SizeCeiling,
    index: RwLock<Index>,
    embedder: Mutex<Option<Box<dyn Embedder>>>,
    control: Mutex<Control>,
    woken: Condvar,
    progress: tokio::sync::watch::Sender<Progress>,
}

#[derive(Default)]
struct Control {
    /// Refreshes asked for.
    requested: u64,
    /// Refreshes finished.
    finished: u64,
    /// Whether a refresh is embedding pieces now.
    building: bool,
    /// Files the last walk found.
    files: usize,
    /// The session ended.
    stop: bool,
    /// Why it stopped, when it failed.
    failed: Option<String>,
    /// Whether the model is loaded.
    loaded: bool,
    /// How long a search waits; [`REFRESH_WAIT`] but in checks.
    wait: Duration,
}

impl Meaning {
    /// Retrieval by meaning for a session in `project`, or `None` when it is
    /// off. When it is on and cannot run, it says why on every search.
    #[must_use]
    pub fn for_session(
        home: &crate::config::Home,
        resolution: &Resolution,
        project: &Path,
        ceiling: SizeCeiling,
    ) -> Option<Self> {
        if !on(resolution) {
            return None;
        }
        let Some(home) = home.root() else {
            return Some(Self::unavailable(
                "retrieval by meaning is on, but this machine has no home folder to keep its files in",
            ));
        };
        let places = Places::under(home);
        let Some(runtime) = fetch::runtime_for_this_machine() else {
            return Some(Self::unavailable(
                "retrieval by meaning is on, but ONNX Runtime publishes no library for this kind \
                 of machine that this build has a digest for",
            ));
        };
        if !places.fetched() {
            return Some(Self::unavailable(
                "retrieval by meaning is on, but its model is not fetched. Run zaru index fetch",
            ));
        }
        let folder = index::folder_for(&places.indexes(), project);
        let model = places.model();
        let library = places.runtime().join(fetch::LIBRARY);
        let loader: Loader = Box::new(move || {
            fetch::check(&library, &runtime.library).map_err(|failure| failure.0)?;
            for source in fetch::model_sources() {
                fetch::check(&model.join(source.kept_as), &source).map_err(|failure| failure.0)?;
            }
            let bge = embed::Bge::load(&model, &library)?;
            Ok(Box::new(bge) as Box<dyn Embedder>)
        });
        Some(Self::start(project, &folder, ceiling, loader))
    }

    /// Retrieval by meaning that is on and cannot run, for `reason`.
    #[must_use]
    pub fn unavailable(reason: &str) -> Self {
        Self {
            inner: Inner::Unavailable(reason.to_owned()),
        }
    }

    /// Start building the index of `project`, kept in `folder`, on a thread
    /// of its own, with the embedder `loader` loads.
    ///
    /// # Panics
    ///
    /// When the operating system cannot start a thread.
    #[must_use]
    pub fn start(project: &Path, folder: &Path, ceiling: SizeCeiling, loader: Loader) -> Self {
        let (progress, _) = tokio::sync::watch::channel(Progress::Loading);
        let shared = Arc::new(Shared {
            project: project.to_path_buf(),
            folder: folder.to_path_buf(),
            ceiling,
            index: RwLock::new(Index::empty(embed::MODEL, embed::DIMENSIONS)),
            embedder: Mutex::new(None),
            control: Mutex::new(Control {
                requested: 1,
                wait: REFRESH_WAIT,
                ..Control::default()
            }),
            woken: Condvar::new(),
            progress,
        });
        let builder = Arc::clone(&shared);
        std::thread::Builder::new()
            .name(String::from("zaru-meaning-index"))
            .spawn(move || build(&builder, loader))
            .expect("the operating system starts a thread");
        Self {
            inner: Inner::Running(shared),
        }
    }

    /// Wait at most `wait` in a search, rather than [`REFRESH_WAIT`].
    #[cfg(test)]
    pub(crate) fn waiting_at_most(&self, wait: Duration) {
        if let Inner::Running(shared) = &self.inner
            && let Ok(mut control) = shared.control.lock()
        {
            control.wait = wait;
        }
    }

    /// Watch how far the index is, for the status line. `None` when it is on
    /// and cannot run: the search says why.
    #[must_use]
    pub fn progress(&self) -> Option<tokio::sync::watch::Receiver<Progress>> {
        match &self.inner {
            Inner::Unavailable(_) => None,
            Inner::Running(shared) => Some(shared.progress.subscribe()),
        }
    }

    /// Why it cannot run, when it is on and cannot.
    #[must_use]
    pub fn cannot(&self) -> Option<&str> {
        match &self.inner {
            Inner::Unavailable(reason) => Some(reason),
            Inner::Running(_) => None,
        }
    }

    /// The project this index is of.
    #[must_use]
    pub fn project(&self) -> Option<&Path> {
        match &self.inner {
            Inner::Unavailable(_) => None,
            Inner::Running(shared) => Some(&shared.project),
        }
    }

    /// The pieces nearest `question`, in files under `within` (relative to
    /// the project; empty for all), asked on a thread that may block.
    pub async fn nearest(&self, within: &str, question: &str) -> Nearest {
        let shared = match &self.inner {
            Inner::Unavailable(reason) => return Nearest::NotReady(reason.clone()),
            Inner::Running(shared) => Arc::clone(shared),
        };
        let (within, question) = (within.to_owned(), question.to_owned());
        tokio::task::spawn_blocking(move || nearest(&shared, &within, &question))
            .await
            .unwrap_or_else(|_| Nearest::NotReady(String::from("the index stopped")))
    }
}

/// [`Meaning::nearest`], on the thread that may block.
fn nearest(shared: &Shared, within: &str, question: &str) -> Nearest {
    // Bring the index up to date first, when nothing else is being built:
    // a changed file is embedded again before it is searched.
    if let Ok(mut control) = shared.control.lock() {
        // Loading the model takes a few seconds; a search that arrives while
        // it loads waits for it rather than answering by text alone.
        let started = Instant::now();
        let wait = control.wait;
        while !control.loaded && control.failed.is_none() && !control.stop {
            let Some(rest) = wait.checked_sub(started.elapsed()) else {
                break;
            };
            control = match shared.woken.wait_timeout(control, rest) {
                Ok((control, _)) => control,
                Err(_) => return Nearest::NotReady(String::from("the index stopped")),
            };
        }
        if let Some(reason) = &control.failed {
            return Nearest::NotReady(format!("the index stopped: {reason}"));
        }
        let idle = matches!(*shared.progress.borrow(), Progress::Ready { .. });
        if idle && !control.building {
            control.requested += 1;
            let mine = control.requested;
            shared.woken.notify_all();
            let started = Instant::now();
            while control.finished < mine && control.failed.is_none() && !control.stop {
                let Some(rest) = wait.checked_sub(started.elapsed()) else {
                    break;
                };
                control = match shared.woken.wait_timeout(control, rest) {
                    Ok((control, _)) => control,
                    Err(_) => return Nearest::NotReady(String::from("the index stopped")),
                };
            }
            if let Some(reason) = &control.failed {
                return Nearest::NotReady(format!("the index stopped: {reason}"));
            }
        }
    }
    let vector = {
        let Ok(mut embedder) = shared.embedder.lock() else {
            return Nearest::NotReady(String::from("the index stopped"));
        };
        let Some(embedder) = embedder.as_mut() else {
            return Nearest::NotReady(String::from("the model is still being checked and loaded"));
        };
        match embedder.embed(&[question]) {
            Ok(mut vectors) if !vectors.is_empty() => vectors.swap_remove(0),
            Ok(_) => return Nearest::NotReady(String::from("the model gave no vector")),
            Err(reason) => return Nearest::NotReady(reason),
        }
    };
    let Ok(index) = shared.index.read() else {
        return Nearest::NotReady(String::from("the index stopped"));
    };
    if index.files.is_empty() {
        return Nearest::NotReady(String::from("no file is indexed yet"));
    }
    let files = shared.control.lock().map_or(0, |control| control.files);
    Nearest::Found {
        places: index.nearest(&vector, within, CANDIDATES),
        indexed: index.files.len(),
        files: files.max(index.files.len()),
        progress: shared.progress.borrow().clone(),
    }
}

impl Drop for Meaning {
    /// The builder stops after the batch it is on. What it embedded is kept.
    fn drop(&mut self) {
        if let Inner::Running(shared) = &self.inner
            && let Ok(mut control) = shared.control.lock()
        {
            control.stop = true;
            shared.woken.notify_all();
        }
    }
}

/// The builder: load the model, then refresh the index each time a refresh is
/// asked for, until the session ends.
fn build(shared: &Shared, loader: Loader) {
    shared.progress.send_replace(Progress::Loading);
    if let Ok(mut index) = shared.index.write()
        && let Some(kept) = Index::load(&shared.folder, embed::MODEL, embed::DIMENSIONS)
    {
        *index = kept;
    }
    match loader() {
        Ok(embedder) => {
            if let Ok(mut slot) = shared.embedder.lock() {
                *slot = Some(embedder);
            }
            shared.progress.send_replace(Progress::Checking);
            if let Ok(mut control) = shared.control.lock() {
                control.loaded = true;
                shared.woken.notify_all();
            }
        }
        Err(reason) => return fail(shared, reason),
    }
    loop {
        let target = match shared.control.lock() {
            Ok(control) if control.stop => return,
            Ok(control) => control.requested,
            Err(_) => return,
        };
        if let Err(reason) = refresh(shared) {
            return fail(shared, reason);
        }
        let Ok(mut control) = shared.control.lock() else {
            return;
        };
        control.finished = target;
        shared.woken.notify_all();
        while control.requested == target && !control.stop {
            control = match shared.woken.wait(control) {
                Ok(control) => control,
                Err(_) => return,
            };
        }
        if control.stop {
            return;
        }
    }
}

fn fail(shared: &Shared, reason: String) {
    if let Ok(mut control) = shared.control.lock() {
        control.failed = Some(reason.clone());
        control.building = false;
        shared.woken.notify_all();
    }
    shared.progress.send_replace(Progress::Failed(reason));
}

/// Bring the index in line with the tree, embedding what waits, in batches.
fn refresh(shared: &Shared) -> Result<(), String> {
    let walk = index::walk(&shared.project, shared.ceiling);
    let (mut waiting, left_out) = shared
        .index
        .write()
        .map_err(|_| String::from("the index could not be read"))?
        .plan(&walk);
    let total = waiting
        .iter()
        .filter(|piece| piece.vector.is_none())
        .count();
    if let Ok(mut control) = shared.control.lock() {
        control.files = walk.candidates.len() + walk.over;
        control.building = total > 0;
    }
    let started = Instant::now();
    let mut done = 0;
    let mut since_saved = 0;
    let mut recorded = 0;
    if total > 0 {
        shared.progress.send_replace(Progress::Building {
            done,
            total,
            left: None,
        });
    }
    while done < total {
        if shared.control.lock().map_or(true, |control| control.stop) {
            break;
        }
        let batch: Vec<usize> = waiting
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece.vector.is_none())
            .map(|(at, _)| at)
            .take(embed::BATCH)
            .collect();
        let texts: Vec<&str> = batch
            .iter()
            .map(|at| waiting[*at].chunk.text.as_str())
            .collect();
        let vectors = {
            let mut embedder = shared
                .embedder
                .lock()
                .map_err(|_| String::from("the model could not be reached"))?;
            let embedder = embedder
                .as_mut()
                .ok_or_else(|| String::from("the model is not loaded"))?;
            embedder.embed(&texts)?
        };
        for (at, vector) in batch.iter().zip(vectors) {
            waiting[*at].vector = Some(vector);
        }
        done += batch.len();
        since_saved += batch.len();
        let frontier = waiting
            .iter()
            .position(|piece| piece.vector.is_none())
            .unwrap_or(waiting.len());
        recorded = record_complete(shared, &waiting, recorded, frontier)?;
        if since_saved >= SAVE_EVERY {
            save(shared)?;
            since_saved = 0;
        }
        let elapsed = started.elapsed();
        let left = (done > 0 && elapsed > Duration::from_secs(5)).then(|| {
            let per = elapsed.as_secs_f64() / done as f64;
            Duration::from_secs_f64(per * (total - done) as f64)
        });
        shared
            .progress
            .send_replace(Progress::Building { done, total, left });
    }
    record_complete(shared, &waiting, recorded, waiting.len())?;
    save(shared)?;
    let stopped = shared.control.lock().map_or(true, |control| control.stop);
    if let Ok(mut control) = shared.control.lock() {
        control.building = false;
    }
    if !stopped {
        let (files, chunks) = shared
            .index
            .read()
            .map(|index| (index.files.len(), index.chunks()))
            .unwrap_or_default();
        shared.progress.send_replace(Progress::Ready {
            files,
            chunks,
            left_out: left_out + walk.over,
        });
    }
    Ok(())
}

/// Record every file whose pieces all lie before `frontier`, from
/// `recorded` on, and return where recording stopped.
fn record_complete(
    shared: &Shared,
    waiting: &[Waiting],
    recorded: usize,
    frontier: usize,
) -> Result<usize, String> {
    let mut at = recorded;
    while at < waiting.len() {
        let path = &waiting[at].path;
        let end = waiting[at..]
            .iter()
            .position(|piece| &piece.path != path)
            .map_or(waiting.len(), |offset| at + offset);
        if end > frontier || waiting[at..end].iter().any(|piece| piece.vector.is_none()) {
            break;
        }
        let stored = waiting[at..end]
            .iter()
            .map(|piece| index::Stored {
                start: piece.chunk.start,
                end: piece.chunk.end,
                hash: piece.hash.clone(),
                vector: piece.vector.clone().unwrap_or_default(),
            })
            .collect();
        shared
            .index
            .write()
            .map_err(|_| String::from("the index could not be written"))?
            .record(path, &waiting[at].file_hash, stored);
        at = end;
    }
    Ok(at)
}

fn save(shared: &Shared) -> Result<(), String> {
    let index = shared
        .index
        .read()
        .map_err(|_| String::from("the index could not be read"))?
        .clone();
    index.save(&shared.folder, &shared.project)
}

#[cfg(test)]
pub(crate) mod tests;
