// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What `zaru index fetch` fetches, from where, and how each file is checked.
//!
//! # Nothing is fetched unless a person asks
//!
//! The default install is `zaru` alone. The model and the ONNX Runtime library
//! arrive only when a person runs `zaru index fetch` and answers yes to a
//! question that names both files, their sizes, where each comes from and
//! where each is kept.
//!
//! # Every file is checked against a digest this build carries
//!
//! Each file is checked when it is fetched and again each time it is loaded.
//! A file whose SHA-256 is not the one below is deleted and never loaded. The
//! digests were read on 2026-09-28: the model's from the files at the pinned
//! revision (the large one equals Hugging Face's own record of it), the
//! runtime archives' from GitHub's record of each release file, and each
//! library's from the file inside its archive.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The model's repository.
pub const MODEL_REPOSITORY: &str = "Xenova/bge-base-en-v1.5";

/// The revision of [`MODEL_REPOSITORY`] every file below comes from. Pinned,
/// so the weights cannot change under a digest.
pub const MODEL_REVISION: &str = "4d6cd88e18e51a5e020c2c305726d76ada9c03cf";

/// The model's licence, from its model card and its base model's
/// (`BAAI/bge-base-en-v1.5`).
pub const MODEL_LICENCE: &str = "MIT";

/// The ONNX Runtime release the library comes from. `ort` 2.0.0-rc.13 is
/// built against the 1.28 interface.
pub const RUNTIME_VERSION: &str = "1.28.2";

/// The runtime's licence, from the `LICENSE` file in its archive.
pub const RUNTIME_LICENCE: &str = "MIT";

/// One file to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The name it is kept under.
    pub kept_as: &'static str,
    /// Where it is fetched from.
    pub url: String,
    /// Its size, in bytes.
    pub bytes: u64,
    /// Its SHA-256, in hexadecimal.
    pub sha256: &'static str,
}

/// The model's five files: `(kept as, path in the repository, bytes,
/// SHA-256)`.
pub const MODEL_FILES: [(&str, &str, u64, &str); 5] = [
    (
        "model.onnx",
        "onnx/model.onnx",
        435_811_539,
        "9bc579acdba21c253c62a9bf866891355a63ffa3442b52c8a37d75b2ccb91848",
    ),
    (
        "tokenizer.json",
        "tokenizer.json",
        711_396,
        "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
    ),
    (
        "config.json",
        "config.json",
        717,
        "d83c21fa7366994560727112ef0a31d8a2ec1c280c2a3e66326fdb877f64c91e",
    ),
    (
        "special_tokens_map.json",
        "special_tokens_map.json",
        125,
        "b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3",
    ),
    (
        "tokenizer_config.json",
        "tokenizer_config.json",
        366,
        "9261e7d79b44c8195c1cada2b453e55b00aeb81e907a6664974b4d7776172ab3",
    ),
];

/// The model's files, with the URLs they are fetched from.
#[must_use]
pub fn model_sources() -> Vec<Source> {
    MODEL_FILES
        .iter()
        .map(|(kept_as, path, bytes, sha256)| Source {
            kept_as,
            url: format!(
                "https://huggingface.co/{MODEL_REPOSITORY}/resolve/{MODEL_REVISION}/{path}"
            ),
            bytes: *bytes,
            sha256,
        })
        .collect()
}

/// The runtime library for one kind of machine: an archive, and the one file
/// taken out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    /// The archive, as published.
    pub archive: Source,
    /// The library's path inside the archive.
    pub member: String,
    /// The library, as kept.
    pub library: Source,
}

/// The name the library is kept under.
pub const LIBRARY: &str = "libonnxruntime.so.1.28.2";

/// The runtime for this machine, or `None` where ONNX Runtime publishes no
/// library this harness has a digest for.
#[must_use]
pub fn runtime_for_this_machine() -> Option<Runtime> {
    let (platform, archive_bytes, archive_sha256, library_bytes, library_sha256) =
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            (
                "linux-x64",
                9_128_991,
                "d7209b8751b27b862b0c76332c2e20e203396edb5dab700ecf4bb485cf147415",
                24_277_040,
                "088f24b1fc56714d3efaaeb3ac2ee486a5d7b50ccfbb3dd26fd1a612534a05fb",
            )
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            (
                "linux-aarch64",
                8_119_456,
                "f020b3d31106cc7db03889b4a5c21e7c38ce4a09ad26119c11d1ad6d3fa0ec04",
                20_591_712,
                "01fc142511bd585db60446eb7531e07cc4160e90fb4664fe04a988457d62305f",
            )
        } else {
            return None;
        };
    let folder = format!("onnxruntime-{platform}-{RUNTIME_VERSION}");
    Some(Runtime {
        archive: Source {
            kept_as: "onnxruntime.tgz",
            url: format!(
                "https://github.com/microsoft/onnxruntime/releases/download/v{RUNTIME_VERSION}/{folder}.tgz"
            ),
            bytes: archive_bytes,
            sha256: archive_sha256,
        },
        member: format!("{folder}/lib/{LIBRARY}"),
        library: Source {
            kept_as: LIBRARY,
            url: String::new(),
            bytes: library_bytes,
            sha256: library_sha256,
        },
    })
}

/// Create `path` for writing, readable by its owner alone, as every file
/// under `~/.zaru` is.
fn create(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

/// Why a file could not be fetched or kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchFailure(pub String);

impl core::fmt::Display for FetchFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FetchFailure {}

/// The SHA-256 of what `reader` holds, in hexadecimal.
///
/// # Errors
///
/// When `reader` cannot be read.
pub fn digest_of(reader: &mut impl Read) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Bytes as lowercase hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Check the file at `path` against `source`'s size and digest. A file that
/// does not match is deleted, so it can never be loaded.
///
/// # Errors
///
/// A sentence saying the file is missing, or that it did not match and was
/// deleted.
pub fn check(path: &Path, source: &Source) -> Result<(), FetchFailure> {
    let size = std::fs::metadata(path)
        .map_err(|_| FetchFailure(format!("{} is not there", path.display())))?
        .len();
    let digest = if size == source.bytes {
        let mut file = std::fs::File::open(path).map_err(|failure| {
            FetchFailure(format!("could not read {}: {failure}", path.display()))
        })?;
        digest_of(&mut file).map_err(|failure| {
            FetchFailure(format!("could not read {}: {failure}", path.display()))
        })?
    } else {
        String::new()
    };
    if digest == source.sha256 {
        return Ok(());
    }
    let _ = std::fs::remove_file(path);
    Err(FetchFailure(format!(
        "{} did not match the digest this build carries for it, so it was deleted and not loaded. \
         Run zaru index fetch to fetch it again",
        path.display()
    )))
}

/// Fetch `source` into `folder`, checking its digest before it is kept.
/// `told` hears how many bytes have arrived so far.
///
/// # Errors
///
/// A sentence saying what failed. A file whose digest does not match is
/// deleted, and so is a file that arrived only in part.
pub async fn download(
    client: &reqwest::Client,
    source: &Source,
    folder: &Path,
    told: &mut (dyn FnMut(u64) + Send),
) -> Result<PathBuf, FetchFailure> {
    let kept = folder.join(source.kept_as);
    let part = folder.join(format!("{}.part", source.kept_as));
    let result = receive(client, source, &part, told).await;
    match result {
        Ok(digest) if digest == source.sha256 => {
            std::fs::rename(&part, &kept).map_err(|failure| {
                FetchFailure(format!("could not keep {}: {failure}", kept.display()))
            })?;
            Ok(kept)
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            Err(FetchFailure(format!(
                "{} from {} did not match the digest this build carries for it, so it was deleted \
                 and not kept",
                source.kept_as, source.url
            )))
        }
        Err(failure) => {
            let _ = std::fs::remove_file(&part);
            Err(failure)
        }
    }
}

/// Write the body at `source.url` to `part`, returning its digest.
async fn receive(
    client: &reqwest::Client,
    source: &Source,
    part: &Path,
    told: &mut (dyn FnMut(u64) + Send),
) -> Result<String, FetchFailure> {
    let failed = |what: String| FetchFailure(format!("could not fetch {}: {what}", source.url));
    let mut response = client
        .get(&source.url)
        .send()
        .await
        .map_err(|failure| failed(failure.to_string()))?;
    if !response.status().is_success() {
        return Err(failed(format!("the server answered {}", response.status())));
    }
    let mut file = create(part).map_err(|failure| {
        FetchFailure(format!("could not write {}: {failure}", part.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut arrived: u64 = 0;
    while let Some(piece) = response
        .chunk()
        .await
        .map_err(|failure| failed(failure.to_string()))?
    {
        arrived += piece.len() as u64;
        if arrived > source.bytes {
            return Err(failed(format!(
                "it was larger than the {} bytes this build expects",
                source.bytes
            )));
        }
        hasher.update(&piece);
        file.write_all(&piece).map_err(|failure| {
            FetchFailure(format!("could not write {}: {failure}", part.display()))
        })?;
        told(arrived);
    }
    file.sync_all().map_err(|failure| {
        FetchFailure(format!("could not write {}: {failure}", part.display()))
    })?;
    Ok(hex(&hasher.finalize()))
}

/// Take the library out of a checked archive and keep it, checking its own
/// digest first. The archive is deleted either way.
///
/// # Errors
///
/// A sentence saying what failed.
pub fn unpack(archive: &Path, runtime: &Runtime, folder: &Path) -> Result<PathBuf, FetchFailure> {
    let result = unpack_member(archive, runtime, folder);
    let _ = std::fs::remove_file(archive);
    result
}

fn unpack_member(
    archive: &Path,
    runtime: &Runtime,
    folder: &Path,
) -> Result<PathBuf, FetchFailure> {
    let failed =
        |what: String| FetchFailure(format!("could not unpack {}: {what}", archive.display()));
    let file = std::fs::File::open(archive).map_err(|failure| failed(failure.to_string()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let kept = folder.join(runtime.library.kept_as);
    let part = folder.join(format!("{}.part", runtime.library.kept_as));
    for entry in tar
        .entries()
        .map_err(|failure| failed(failure.to_string()))?
    {
        let mut entry = entry.map_err(|failure| failed(failure.to_string()))?;
        let path = entry
            .path()
            .map_err(|failure| failed(failure.to_string()))?;
        if path.as_ref() != Path::new(&runtime.member) || !entry.header().entry_type().is_file() {
            continue;
        }
        let mut out = create(&part).map_err(|failure| {
            FetchFailure(format!("could not write {}: {failure}", part.display()))
        })?;
        std::io::copy(&mut (&mut entry).take(runtime.library.bytes + 1), &mut out)
            .map_err(|failure| failed(failure.to_string()))?;
        out.sync_all()
            .map_err(|failure| failed(failure.to_string()))?;
        drop(out);
        check(&part, &runtime.library).map_err(|_| {
            FetchFailure(format!(
                "{} in {} did not match the digest this build carries for it, so it was deleted \
                 and not kept",
                runtime.member,
                archive.display()
            ))
        })?;
        std::fs::rename(&part, &kept).map_err(|failure| {
            FetchFailure(format!("could not keep {}: {failure}", kept.display()))
        })?;
        return Ok(kept);
    }
    Err(failed(format!("it holds no {}", runtime.member)))
}
