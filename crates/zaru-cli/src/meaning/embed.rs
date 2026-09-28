// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The embedding model: text in, a vector of numbers out.
//!
//! # The same vector space as Nuclear Notes, in fact
//!
//! Nuclear Notes' running embedder (its `services/embedder`, read
//! 2026-09-28 at `a56422a`) calls fastembed's `BGEBaseENV15`: the file
//! `onnx/model.onnx` of `Xenova/bge-base-en-v1.5`, full precision, pooled on
//! the first token, and normalised to length one. It puts no instruction
//! before a query or a passage. [`Bge`] loads that same file, pools the same
//! way, normalises the same way, and adds no prefix either, so a vector made
//! here and one made there for the same text are the same vector.
//! `tests/meaning_live.rs` compares [`Bge`]'s vectors with ones made by the
//! model's own reference code.
//!
//! # Memory
//!
//! Loading takes about 974 MB at its peak, because the model's bytes are read
//! and the runtime copies them; it then holds about 500 MB. A batch is at
//! most [`BATCH`] pieces of at most 512 tokens on [`THREADS`] threads, which
//! measured under 1.0 GB in all on 2026-09-28.

use std::path::Path;

/// The most pieces embedded at once.
pub const BATCH: usize = 8;

/// The threads the runtime uses for one batch.
pub const THREADS: usize = 2;

/// The most tokens of one piece the model reads.
pub const MAX_TOKENS: usize = 512;

/// The model's name, as an index records it.
pub const MODEL: &str = "bge-base-en-v1.5";

/// How many numbers a vector has.
pub const DIMENSIONS: usize = 768;

/// Something that turns text into vectors of length one.
///
/// A query and a passage go through the same call: Nuclear Notes puts no
/// prefix before either, and neither does this.
pub trait Embedder: Send {
    /// The model's name, which an index records so that vectors from two
    /// models are never compared.
    fn model(&self) -> &str;

    /// One vector for each text, in order.
    ///
    /// # Errors
    ///
    /// A sentence saying why the model could not run.
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String>;
}

/// bge-base-en-v1.5, run on this machine by ONNX Runtime.
pub struct Bge {
    model: fastembed::TextEmbedding,
}

impl core::fmt::Debug for Bge {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bge").finish_non_exhaustive()
    }
}

impl Bge {
    /// Load the model from `model` (a folder holding the five files
    /// [`super::fetch::MODEL_FILES`] names), running on the ONNX Runtime
    /// library at `runtime`.
    ///
    /// The caller checks both against their digests first; this only loads.
    ///
    /// # Errors
    ///
    /// A sentence saying what could not be read or loaded.
    pub fn load(model: &Path, runtime: &Path) -> Result<Self, String> {
        load_runtime(runtime)?;
        let read = |name: &str| {
            std::fs::read(model.join(name)).map_err(|failure| {
                format!("could not read {}: {failure}", model.join(name).display())
            })
        };
        let files = fastembed::TokenizerFiles {
            tokenizer_file: read("tokenizer.json")?,
            config_file: read("config.json")?,
            special_tokens_map_file: read("special_tokens_map.json")?,
            tokenizer_config_file: read("tokenizer_config.json")?,
        };
        let defined = fastembed::UserDefinedEmbeddingModel::new(read("model.onnx")?, files)
            .with_pooling(fastembed::Pooling::Cls);
        let options = fastembed::InitOptionsUserDefined::new()
            .with_max_length(MAX_TOKENS)
            .with_intra_threads(THREADS);
        let model = fastembed::TextEmbedding::try_new_from_user_defined(defined, options)
            .map_err(|failure| format!("the model could not be loaded: {failure}"))?;
        Ok(Self { model })
    }
}

impl Embedder for Bge {
    fn model(&self) -> &str {
        MODEL
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        let mut vectors = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            let embedded = self
                .model
                .embed(batch, Some(BATCH))
                .map_err(|failure| format!("the model could not embed a batch: {failure}"))?;
            vectors.extend(embedded);
        }
        Ok(vectors)
    }
}

/// Load the ONNX Runtime library once for this process.
///
/// The runtime can be loaded only once, so a second call with another path
/// answers the first call's result.
fn load_runtime(path: &Path) -> Result<(), String> {
    static LOADED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    LOADED
        .get_or_init(|| match ort::init_from(path) {
            Ok(builder) => {
                builder.commit();
                Ok(())
            }
            Err(failure) => Err(format!(
                "the ONNX Runtime library at {} could not be loaded: {failure}",
                path.display()
            )),
        })
        .clone()
}

/// Cosine similarity of two vectors of length one: their dot product.
#[must_use]
pub fn similarity(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(l, r)| l * r).sum()
}
