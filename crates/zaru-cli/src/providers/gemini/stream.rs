// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This client's SSE framing, which is [`crate::providers::sse`].
//!
//! # Why this module still exists after the reader moved
//!
//! The reader was written here on 2026-09-05 and moved to
//! [`crate::providers::sse`] on 2026-09-14, when the `openai-compatible`
//! client arrived and needed the same bytes-to-frames step. It was already
//! written to the SSE specification rather than to Google — its own
//! documentation said so — so the move copied no behaviour and changed no
//! line of the reader.
//!
//! **This path stays because it is this client's name for the thing**, and
//! because a re-export keeps the move to one commit that changes nothing:
//! `providers::gemini::stream::Frames` means what it meant, `gemini.rs`'s
//! three call sites are untouched, and `gemini/tests.rs`' nine framing checks
//! exercise the moved module through the path they always used. A check that
//! had to be edited to keep passing would have been evidence that the move
//! was not behaviour-free.

pub use crate::providers::sse::Frames;
