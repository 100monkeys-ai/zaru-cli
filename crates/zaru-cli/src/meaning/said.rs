// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What `zaru index` says, and what it asks before it fetches anything.

use super::fetch::{self, Runtime};
use super::{Places, index};
use std::path::Path;

/// Bytes as megabytes, one decimal: `436.7 MB`.
#[must_use]
pub fn megabytes(bytes: u64) -> String {
    let tenths = (bytes + 50_000) / 100_000;
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

/// The total size of the model's files.
#[must_use]
pub fn model_bytes() -> u64 {
    fetch::MODEL_FILES
        .iter()
        .map(|(_, _, bytes, _)| bytes)
        .sum()
}

/// What `zaru index fetch` asks: the question, and the rows under it that
/// name both files, their sizes, where each comes from and where each is
/// kept.
#[must_use]
pub fn fetch_question(places: &Places, runtime: &Runtime) -> (String, Vec<String>) {
    let statement = String::from(
        "Retrieval by meaning needs two things that are not part of zaru. Fetch them now?",
    );
    let detail = vec![
        format!(
            "The model {} ({} licence): {} files, {} in all.",
            super::embed::MODEL,
            fetch::MODEL_LICENCE,
            fetch::MODEL_FILES.len(),
            megabytes(model_bytes())
        ),
        format!(
            "  From https://huggingface.co/{}, revision {}.",
            fetch::MODEL_REPOSITORY,
            fetch::MODEL_REVISION
        ),
        format!("  Kept in {}.", places.model().display()),
        format!(
            "The ONNX Runtime library {} ({} licence): a {} download, {} once unpacked.",
            fetch::RUNTIME_VERSION,
            fetch::RUNTIME_LICENCE,
            megabytes(runtime.archive.bytes),
            megabytes(runtime.library.bytes)
        ),
        format!("  From {}.", runtime.archive.url),
        format!("  Kept in {}.", places.runtime().display()),
        String::from(
            "Each file is checked against a digest this build carries, now and each time it is \
             loaded. A file that does not match is deleted and never loaded.",
        ),
        String::from(
            "The model runs on this machine. No text of your code is sent anywhere to be indexed.",
        ),
        format!(
            "Fetching turns nothing on. To turn retrieval by meaning on, set {} = true in \
             ~/.zaru/config.toml, or {}=true.",
            super::KEY,
            "ZARU_SEARCH_MEANING"
        ),
    ];
    (statement, detail)
}

/// What `zaru index` says.
#[must_use]
pub fn status(places: &Places, on: bool, project: &Path) -> Vec<String> {
    let mut lines = vec![if on {
        format!(
            "Retrieval by meaning is on. {} sets it; zaru config explain {} says where it is set.",
            super::KEY,
            super::KEY
        )
    } else {
        format!(
            "Retrieval by meaning is off. To turn it on, set {} = true in ~/.zaru/config.toml, \
             or ZARU_SEARCH_MEANING=true.",
            super::KEY
        )
    }];
    lines.push(match fetch::runtime_for_this_machine() {
        None => String::from(
            "ONNX Runtime publishes no library for this kind of machine that this build has a \
             digest for, so retrieval by meaning cannot run here.",
        ),
        Some(_) if places.fetched() => format!(
            "Its model and runtime library are fetched, in {}.",
            places.root.display()
        ),
        Some(runtime) => format!(
            "Its model and runtime library are not fetched. zaru index fetch says what it would \
             fetch ({} in all) and asks first.",
            megabytes(model_bytes() + runtime.archive.bytes)
        ),
    });
    let folder = index::folder_for(&places.indexes(), project);
    let kept = index::Index::load(&folder, super::embed::MODEL, super::embed::DIMENSIONS);
    lines.push(match kept {
        Some(kept) => {
            let bytes: u64 = ["about.json", "chunks.jsonl", "vectors.f32"]
                .iter()
                .filter_map(|name| std::fs::metadata(folder.join(name)).ok())
                .map(|metadata| metadata.len())
                .sum();
            format!(
                "This project's index holds {} files in {} pieces, {} on disk, in {}.",
                kept.files.len(),
                kept.chunks(),
                megabytes(bytes),
                folder.display()
            )
        }
        None => String::from(
            "This project has no index yet. With retrieval by meaning on, a session builds it.",
        ),
    });
    lines
}
