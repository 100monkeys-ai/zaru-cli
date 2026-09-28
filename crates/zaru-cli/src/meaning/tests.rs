// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for retrieval by meaning, over trees each check builds for itself.
//!
//! The model here is [`Words`], a small deterministic stand-in: it maps each
//! word through a table of words that mean the same thing and counts them
//! into a vector. So the suite needs no download and no native library, and
//! "near in meaning" is something a check can arrange. The real model is
//! checked by `tests/meaning_live.rs`, which runs only where its files are.

use super::embed::Embedder;
use super::index::{self, Index, MOST_CHUNKS};
use super::{Loader, Meaning, Nearest, Progress, chunk, fetch};
use crate::config::SizeCeiling;
use crate::tools::fixtures::nonce;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A directory this check owns, removed when it ends.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(label: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce(label));
        std::fs::create_dir_all(&base).expect("staging: the scratch directory");
        Self(base)
    }

    pub(crate) fn put(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("staging: a folder");
        }
        std::fs::write(path, text).expect("staging: a file");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Words that mean the same thing, each group counted as one.
const SAME: [&[&str]; 4] = [
    &["throttle", "throttled", "rate", "limit", "limits", "calls"],
    &["replay", "replayed", "reused", "duplicate", "again"],
    &["encrypt", "encrypted", "seal", "sealed", "cipher"],
    &["docker", "podman", "container", "runtime"],
];

/// How many numbers a [`Words`] vector has: the real model's, so an index it
/// makes has the shape the real one's has.
const WIDTH: usize = super::embed::DIMENSIONS;

/// The stand-in model. Counts every text it is given in `seen`.
pub(crate) struct Words {
    pub(crate) seen: Arc<AtomicUsize>,
    pub(crate) texts: Arc<Mutex<Vec<String>>>,
}

impl Words {
    pub(crate) fn new() -> Self {
        Self {
            seen: Arc::new(AtomicUsize::new(0)),
            texts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn vector(text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; WIDTH];
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() > 2)
        {
            let word = word.to_lowercase();
            let group = SAME
                .iter()
                .position(|group| group.contains(&word.as_str()))
                .map_or_else(|| format!("w:{word}"), |at| format!("g:{at}"));
            let slot = group.bytes().fold(2_166_136_261_u32, |hash, byte| {
                (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
            }) as usize
                % WIDTH;
            vector[slot] += 1.0;
        }
        let length = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
        if length > 0.0 {
            vector.iter_mut().for_each(|x| *x /= length);
        }
        vector
    }
}

impl Embedder for Words {
    fn model(&self) -> &str {
        super::embed::MODEL
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        self.seen.fetch_add(texts.len(), Ordering::SeqCst);
        if let Ok(mut seen) = self.texts.lock() {
            seen.extend(texts.iter().map(|text| (*text).to_owned()));
        }
        Ok(texts.iter().map(|text| Self::vector(text)).collect())
    }
}

/// A loader that hands the builder `words`.
pub(crate) fn words_loader(words: Words) -> Loader {
    Box::new(move || Ok(Box::new(words) as Box<dyn Embedder>))
}

pub(crate) fn roomy() -> SizeCeiling {
    SizeCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

/// Wait until the index is ready, and say how it ended.
pub(crate) fn ready(meaning: &Meaning) -> Progress {
    let receiver = meaning.progress().expect("it runs");
    let started = Instant::now();
    loop {
        let now = receiver.borrow().clone();
        if matches!(now, Progress::Ready { .. } | Progress::Failed(_)) {
            return now;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the index was not ready within a minute; it said {now:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A small project: three source files, a test, an ignored file and a build
/// folder.
fn project(scratch: &Scratch) {
    scratch.put(
        "src/limits.rs",
        "/// Per-capability rate limit configuration.\npub struct RateLimit {\n    pub calls: u32,\n    pub per_seconds: u32,\n}\n",
    );
    scratch.put(
        "src/tokens.rs",
        "/// Reject a replayed token: a duplicate identifier was seen before.\npub fn record_jti(seen: &mut Vec<String>, jti: &str) -> bool {\n    if seen.iter().any(|one| one == jti) {\n        return false;\n    }\n    seen.push(jti.to_owned());\n    true\n}\n",
    );
    scratch.put(
        "src/greeting.rs",
        "/// Say hello to a person by name.\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n",
    );
    scratch.put(
        "tests/limits_tests.rs",
        "#[test]\nfn throttle_calls_rate_limit() {\n    assert!(true);\n}\n",
    );
    scratch.put(".gitignore", "secret_notes.rs\n");
    scratch.put(
        "secret_notes.rs",
        "/// throttle throttle throttle calls calls rate limit\npub fn ignored() {}\n",
    );
    scratch.put(
        "target/built.rs",
        "/// throttle calls rate limit\npub fn built() {}\n",
    );
}

#[test]
fn an_index_is_built_in_the_background_and_kept_on_disk_without_the_code_s_text() {
    let scratch = Scratch::new("meaning-built");
    project(&scratch);
    let marker = nonce("words");
    scratch.put(
        "src/marked.rs",
        &format!("/// A comment holding {marker}.\npub fn marked() {{}}\n"),
    );
    let kept = Scratch::new("meaning-index");
    let folder = kept.0.clone();
    let words = Words::new();
    let seen = Arc::clone(&words.seen);
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), words_loader(words));

    let Progress::Ready { files, chunks, .. } = ready(&meaning) else {
        panic!("the index did not become ready");
    };
    assert_eq!(files, 4, "four source files are indexed");
    assert!(chunks >= 4, "each source file gives at least one piece");
    assert_eq!(
        seen.load(Ordering::SeqCst),
        chunks,
        "each piece was embedded once"
    );

    let rows = std::fs::read_to_string(folder.join("chunks.jsonl")).expect("the index is on disk");
    assert_eq!(rows.lines().count(), chunks, "one row per piece");
    assert!(
        rows.contains("\"path\":\"src/limits.rs\"") && rows.contains("\"start\":1"),
        "a row names its file and its lines: {rows}"
    );
    let vectors = std::fs::metadata(folder.join("vectors.f32")).expect("the vectors are on disk");
    assert_eq!(
        vectors.len(),
        (chunks * super::embed::DIMENSIONS * 4) as u64
    );
    for name in ["about.json", "chunks.jsonl"] {
        let kept = std::fs::read_to_string(folder.join(name)).expect("readable");
        assert!(
            !kept.contains(&marker),
            "{name} holds the code's own text, and an index holds only where each piece is"
        );
    }
}

#[test]
fn ignored_files_build_output_and_tests_are_not_indexed() {
    let scratch = Scratch::new("meaning-ignored");
    project(&scratch);
    let kept = Scratch::new("meaning-index");
    let folder = kept.0.clone();
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), words_loader(Words::new()));
    ready(&meaning);
    let rows = std::fs::read_to_string(folder.join("chunks.jsonl")).expect("the index is on disk");
    for left_out in ["secret_notes.rs", "target/", "tests/"] {
        assert!(!rows.contains(left_out), "{left_out} was indexed: {rows}");
    }
    assert!(
        rows.contains("src/greeting.rs"),
        "the accepting arm: {rows}"
    );
}

#[test]
fn a_changed_file_is_embedded_again_on_the_next_search_and_an_unchanged_one_is_not() {
    let scratch = Scratch::new("meaning-changed");
    project(&scratch);
    let kept = Scratch::new("meaning-index");
    let folder = kept.0.clone();
    let words = Words::new();
    let seen = Arc::clone(&words.seen);
    let texts = Arc::clone(&words.texts);
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), words_loader(words));
    ready(&meaning);
    let before = seen.load(Ordering::SeqCst);
    texts.lock().expect("not poisoned").clear();

    scratch.put(
        "src/greeting.rs",
        "/// Say goodbye to a person by name.\npub fn part(name: &str) -> String {\n    format!(\"goodbye {name}\")\n}\n",
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let found = runtime.block_on(meaning.nearest("", "say goodbye"));
    let Nearest::Found { places, .. } = found else {
        panic!("the search was answered by meaning");
    };
    let embedded: Vec<String> = texts.lock().expect("not poisoned").clone();
    assert!(
        embedded.iter().any(|text| text.contains("goodbye")),
        "the changed file was not embedded again before the search: {embedded:?}"
    );
    assert!(
        !embedded
            .iter()
            .any(|text| text.contains("RateLimit") || text.contains("record_jti")),
        "an unchanged file was embedded again: {embedded:?}"
    );
    assert_eq!(
        seen.load(Ordering::SeqCst) - before,
        embedded.len(),
        "the count and the texts agree"
    );
    assert_eq!(
        places[0].path, "src/greeting.rs",
        "the new text is what is searched"
    );

    std::fs::remove_file(scratch.0.join("src/greeting.rs")).expect("staging: a removal");
    let found = runtime.block_on(meaning.nearest("", "say goodbye"));
    let Nearest::Found { places, .. } = found else {
        panic!("the search was answered by meaning");
    };
    assert!(
        places.iter().all(|place| place.path != "src/greeting.rs"),
        "a file that is gone was still searched"
    );
}

#[test]
fn a_stopped_index_carries_on_where_it_stopped() {
    let scratch = Scratch::new("meaning-resume");
    project(&scratch);
    let kept = Scratch::new("meaning-index");
    let folder = kept.0.clone();
    let first = Words::new();
    let first_seen = Arc::clone(&first.seen);
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), words_loader(first));
    ready(&meaning);
    drop(meaning);
    assert!(
        first_seen.load(Ordering::SeqCst) > 0,
        "the first session embedded"
    );

    let second = Words::new();
    let second_seen = Arc::clone(&second.seen);
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), words_loader(second));
    let Progress::Ready { files, .. } = ready(&meaning) else {
        panic!("the second session's index did not become ready");
    };
    assert_eq!(files, 3, "the kept index was read back whole");
    assert_eq!(
        second_seen.load(Ordering::SeqCst),
        0,
        "a second session embedded pieces the first had kept"
    );
}

#[test]
fn a_search_waits_a_while_for_the_model_to_load_and_then_is_told_so() {
    let scratch = Scratch::new("meaning-early");
    project(&scratch);
    let kept = Scratch::new("meaning-index");
    let folder = kept.0.clone();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let loader: Loader = Box::new(move || {
        let _ = wait.recv_timeout(Duration::from_secs(30));
        Ok(Box::new(Words::new()) as Box<dyn Embedder>)
    });
    let meaning = Meaning::start(&scratch.0, &folder, roomy(), loader);
    meaning.waiting_at_most(Duration::from_millis(200));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let asked = Instant::now();
    let early = runtime.block_on(meaning.nearest("", "throttle calls"));
    assert!(
        asked.elapsed() >= Duration::from_millis(200),
        "the search did not wait for the model to load"
    );
    assert_eq!(
        early,
        Nearest::NotReady(String::from("the model is still being checked and loaded"))
    );
    release.send(()).expect("the loader waits");
    assert!(
        matches!(ready(&meaning), Progress::Ready { .. }),
        "the accepting arm"
    );
}

#[test]
fn a_model_that_cannot_load_stops_the_index_and_says_why() {
    let scratch = Scratch::new("meaning-broken");
    project(&scratch);
    let loader: Loader = Box::new(|| Err(String::from("the stand-in model refused")));
    let kept = Scratch::new("meaning-index");
    let meaning = Meaning::start(&scratch.0, &kept.0, roomy(), loader);
    assert_eq!(
        ready(&meaning),
        Progress::Failed(String::from("the stand-in model refused"))
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    assert_eq!(
        runtime.block_on(meaning.nearest("", "anything at all")),
        Nearest::NotReady(String::from(
            "the index stopped: the stand-in model refused"
        ))
    );
}

#[test]
fn the_nearest_pieces_are_the_ones_whose_meaning_is_nearest() {
    let scratch = Scratch::new("meaning-near");
    project(&scratch);
    let kept = Scratch::new("meaning-index");
    let meaning = Meaning::start(&scratch.0, &kept.0, roomy(), words_loader(Words::new()));
    ready(&meaning);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    for (question, answer) in [
        ("where are calls throttled", "src/limits.rs"),
        ("where is a reused token refused", "src/tokens.rs"),
    ] {
        let Nearest::Found { places, .. } = runtime.block_on(meaning.nearest("", question)) else {
            panic!("the search was answered by meaning");
        };
        assert_eq!(places[0].path, answer, "for {question:?}: {places:?}");
    }
    let Nearest::Found { places, .. } =
        runtime.block_on(meaning.nearest("src/greeting.rs", "throttle calls"))
    else {
        panic!("the search was answered by meaning");
    };
    assert!(
        places.iter().all(|place| place.path == "src/greeting.rs"),
        "a search under one file found pieces elsewhere: {places:?}"
    );
}

#[test]
fn a_piece_is_a_declaration_with_its_comment_or_a_window_of_lines() {
    let text = "use std::fmt;\n\n/// Adds.\n#[inline]\nfn add(a: u8, b: u8) -> u8 {\n    a + b\n}\n\nstruct Point {\n    x: u8,\n    y: u8,\n}\n\nimpl Point {\n    /// Moves.\n    fn shift(&mut self) {\n        self.x += 1;\n    }\n}\n";
    let pieces = chunk::chunks(Path::new("a.rs"), text);
    let spans: Vec<(usize, usize)> = pieces
        .iter()
        .map(|piece| (piece.start, piece.end))
        .collect();
    assert_eq!(
        spans,
        vec![(3, 7), (9, 12), (15, 18)],
        "each declaration with its comment and attribute, and the impl's method alone"
    );
    assert!(pieces[0].text.starts_with("/// Adds."));

    let long: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    let windows: Vec<(usize, usize)> = chunk::chunks(Path::new("notes.txt"), &long)
        .iter()
        .map(|piece| (piece.start, piece.end))
        .collect();
    assert_eq!(windows, vec![(1, 40), (41, 80), (81, 100)]);
}

#[test]
fn tests_inside_a_file_are_left_out_and_small_pieces_are_joined() {
    let text = "pub const A: u32 = 1;\npub const B: u32 = 2;\n\n/// Seals a key.\npub fn seal(key: &[u8]) -> Vec<u8> {\n    let mut out = key.to_vec();\n    out.reverse();\n    out\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn seals() {\n        assert!(true);\n        assert!(true);\n        assert!(true);\n    }\n}\n";
    let spans: Vec<(usize, usize)> = chunk::chunks(Path::new("a.rs"), text)
        .iter()
        .map(|piece| (piece.start, piece.end))
        .collect();
    assert_eq!(
        spans,
        vec![(1, 9)],
        "the two one-line constants join the function beside them, and the tests module is left out"
    );
}

#[test]
fn a_tree_over_the_limit_is_indexed_up_to_it_and_says_so() {
    let walk = index::Walk {
        candidates: (0..=MOST_CHUNKS)
            .map(|n| index::Candidate {
                path: format!("notes/{n:06}.txt"),
                hash: format!("{n}"),
                text: format!("note {n}\nsecond line\nthird line\n"),
            })
            .collect(),
        over: 0,
    };
    let mut index = Index::empty(super::embed::MODEL, super::embed::DIMENSIONS);
    let (waiting, left_out) = index.plan(&walk);
    assert_eq!(waiting.len(), MOST_CHUNKS, "the index fills to its limit");
    assert_eq!(left_out, 1, "and counts what it left out");
}

#[test]
fn the_status_line_says_how_far_and_how_long_is_left_and_nothing_when_ready() {
    let building = Progress::Building {
        done: 340,
        total: 2_100,
        left: Some(Duration::from_secs(700)),
    };
    assert_eq!(
        building.status(),
        Some((
            String::from("indexing 340/2100, about 12 min left"),
            String::from("index 16%")
        ))
    );
    let ready = Progress::Ready {
        files: 1,
        chunks: 1,
        left_out: 0,
    };
    assert_eq!(ready.status(), None);
}

/// A loopback server that answers each connection with `body` and status
/// 200, `times` times.
fn serving(body: Vec<u8>, times: usize) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("staging: a loopback port");
    let address = listener.local_addr().expect("staging: the address");
    std::thread::spawn(move || {
        for _ in 0..times {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().expect("staging: a clone"));
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if line == "\r\n" => break,
                    Ok(_) => {}
                }
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{address}/file")
}

fn sha256(bytes: &[u8]) -> &'static str {
    let digest = fetch::digest_of(&mut &bytes[..]).expect("in memory");
    Box::leak(digest.into_boxed_str())
}

#[test]
fn a_fetched_file_is_kept_only_when_its_digest_matches() {
    let scratch = Scratch::new("meaning-fetch");
    let body = b"the model's bytes".to_vec();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let client = reqwest::Client::new();

    let good = fetch::Source {
        kept_as: "good.bin",
        url: serving(body.clone(), 1),
        bytes: body.len() as u64,
        sha256: sha256(&body),
    };
    let kept = runtime
        .block_on(fetch::download(&client, &good, &scratch.0, &mut |_| {}))
        .expect("a file whose digest matches is kept");
    assert_eq!(std::fs::read(kept).expect("kept"), body);

    let bad = fetch::Source {
        kept_as: "bad.bin",
        url: serving(body.clone(), 1),
        bytes: body.len() as u64,
        sha256: sha256(b"other bytes"),
    };
    let refused = runtime
        .block_on(fetch::download(&client, &bad, &scratch.0, &mut |_| {}))
        .expect_err("a file whose digest does not match is refused");
    assert!(refused.0.contains("did not match"), "{refused}");
    assert!(
        !scratch.0.join("bad.bin").exists(),
        "a refused file was kept"
    );
    assert!(
        !scratch.0.join("bad.bin.part").exists(),
        "a refused file's part was kept"
    );

    // A file on disk that no longer matches is deleted when it is checked.
    std::fs::write(scratch.0.join("good.bin"), b"the model's bytez").expect("staging");
    let checked = fetch::check(&scratch.0.join("good.bin"), &good).expect_err("changed bytes");
    assert!(checked.0.contains("deleted and not loaded"), "{checked}");
    assert!(
        !scratch.0.join("good.bin").exists(),
        "a file that did not match was left to load"
    );
}

#[test]
fn the_runtime_library_is_taken_from_its_archive_and_checked() {
    let scratch = Scratch::new("meaning-unpack");
    let library = b"a library's bytes".to_vec();
    let mut archive = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut archive, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(library.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(
                &mut header,
                "onnxruntime-test/lib/libonnxruntime.so.1.28.2",
                &library[..],
            )
            .expect("staging: an archive");
        builder
            .into_inner()
            .expect("staging")
            .finish()
            .expect("staging");
    }
    let runtime = |sha: &'static str| fetch::Runtime {
        archive: fetch::Source {
            kept_as: "onnxruntime.tgz",
            url: String::new(),
            bytes: archive.len() as u64,
            sha256: sha256(&archive),
        },
        member: String::from("onnxruntime-test/lib/libonnxruntime.so.1.28.2"),
        library: fetch::Source {
            kept_as: fetch::LIBRARY,
            url: String::new(),
            bytes: library.len() as u64,
            sha256: sha,
        },
    };
    let path = scratch.0.join("onnxruntime.tgz");
    std::fs::write(&path, &archive).expect("staging");
    let kept = fetch::unpack(&path, &runtime(sha256(&library)), &scratch.0).expect("unpacked");
    assert_eq!(std::fs::read(kept).expect("kept"), library);
    assert!(!path.exists(), "the archive is deleted once unpacked");

    std::fs::remove_file(scratch.0.join(fetch::LIBRARY)).expect("staging");
    std::fs::write(&path, &archive).expect("staging");
    let refused = fetch::unpack(&path, &runtime(sha256(b"another library")), &scratch.0)
        .expect_err("a library whose digest does not match is refused");
    assert!(refused.0.contains("did not match"), "{refused}");
    assert!(
        !scratch.0.join(fetch::LIBRARY).exists(),
        "a refused library was kept"
    );
}
