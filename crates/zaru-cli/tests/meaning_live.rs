// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The check that needs the real model: the vectors this harness makes are
//! the vectors the model's own reference code makes.
//!
//! # It is `#[ignore]`d, and that is the honest shape
//!
//! The model is 436 MB and the gate fetches nothing, so this does not run on
//! the gate. It runs wherever the files are:
//!
//! ```text
//! ZARU_MEANING_LIVE_DIR=~/.zaru/meaning \
//!   cargo test -p zaru-cli --test meaning_live -- --ignored
//! ```
//!
//! `ZARU_MEANING_LIVE_DIR` names a folder laid out as `zaru index fetch`
//! leaves `~/.zaru/meaning`: `model/` with the five model files and
//! `runtime/` with the ONNX Runtime library. Every file is checked against
//! its digest before it is loaded, as a session checks it.
//!
//! # Where the reference vectors come from
//!
//! `tests/fixtures/meaning-reference.json`, made on 2026-09-28 from
//! `BAAI/bge-base-en-v1.5` at revision
//! `a5beb1e3e68b9ab74eb54cfd186867f64f240e1a` with Hugging Face
//! `transformers` 5.17.0 on `torch` 2.14.0 (CPU): the model's own PyTorch
//! weights, the first token's hidden state, normalised to length one, no
//! prefix, at most 512 tokens. That is what the model's card says to do and
//! what Nuclear Notes' embedder does. The file records how it was made.
//!
//! # The tolerance
//!
//! Each of the 768 numbers within 0.00001 of the reference, and the two
//! vectors' cosine similarity at least 0.99999. The reference is PyTorch,
//! rounded to seven decimals, and this is ONNX Runtime running an ONNX export
//! of the same weights. Measured on 2026-09-28 the largest difference was
//! 2.7e-7 and the lowest cosine 0.9999993; a vector pooled or normalised
//! differently misses by orders of magnitude more.

use serde::Deserialize;
use std::path::PathBuf;
use zaru_cli::meaning::embed::{Bge, DIMENSIONS, Embedder, similarity};
use zaru_cli::meaning::fetch;

#[derive(Deserialize)]
struct Reference {
    how: String,
    sentences: Vec<String>,
    vectors: Vec<Vec<f32>>,
}

fn folder() -> PathBuf {
    PathBuf::from(std::env::var("ZARU_MEANING_LIVE_DIR").unwrap_or_else(|_| {
        panic!(
            "ZARU_MEANING_LIVE_DIR is not set. This check needs the model's files; see the \
             module documentation and run it with `--ignored`."
        )
    }))
}

#[test]
#[ignore = "needs the model's files; see the module documentation"]
fn the_vectors_made_here_are_the_reference_implementation_s() {
    let folder = folder();
    let runtime = fetch::runtime_for_this_machine().expect("a runtime for this machine");
    let library = folder.join("runtime").join(fetch::LIBRARY);
    fetch::check(&library, &runtime.library).expect("the runtime library matches its digest");
    for source in fetch::model_sources() {
        fetch::check(&folder.join("model").join(source.kept_as), &source)
            .expect("each model file matches its digest");
    }
    let mut model = Bge::load(&folder.join("model"), &library).expect("the model loads");

    let reference: Reference = serde_json::from_str(include_str!("fixtures/meaning-reference.json"))
        .expect("the reference vectors read");
    println!("reference: {}", reference.how);
    let texts: Vec<&str> = reference.sentences.iter().map(String::as_str).collect();
    let made = model.embed(&texts).expect("the model embeds");
    assert_eq!(made.len(), reference.vectors.len());
    for ((sentence, here), there) in reference.sentences.iter().zip(&made).zip(&reference.vectors) {
        assert_eq!(here.len(), DIMENSIONS);
        let length = here.iter().map(|x| x * x).sum::<f32>().sqrt();
        let worst = here
            .iter()
            .zip(there)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        let cosine = similarity(here, there);
        println!("{sentence:?}: length {length:.6}, largest difference {worst:.2e}, cosine {cosine:.7}");
        assert!((length - 1.0).abs() < 1e-4, "{sentence:?} is not normalised: {length}");
        assert!(worst <= 1e-5, "{sentence:?} differs by {worst} somewhere");
        assert!(cosine >= 0.99999, "{sentence:?} has cosine {cosine}");
    }
}
