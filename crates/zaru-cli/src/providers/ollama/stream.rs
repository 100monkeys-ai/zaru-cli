// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Newline-delimited-JSON framing for `/api/chat`.
//!
//! # What this module is, and what it deliberately is not
//!
//! It turns a byte stream into the text of its lines, and it knows nothing
//! about Ollama. It does not parse JSON, it does not know what a tool call is,
//! and it has no opinion about what a line means. [`super::map`] is where a
//! line becomes a response, and keeping the two apart is what lets the framing
//! be checked over bytes that never came from a socket.
//!
//! # Why this is a separate reader from the `gemini` client's, rather than a
//! shared one
//!
//! **The two transports are genuinely different, and measuring said so.**
//! Ollama's `/api/chat` answers `content-type: application/x-ndjson` with
//! `transfer-encoding: chunked`, and sends **one JSON object per line** —
//! measured 2026-09-14 against v0.34.0. There is no `data:` field name, no
//! blank-line separator, no comment syntax and no multi-line event to rejoin.
//! The frame terminator is a single `\n`.
//!
//! An SSE reader cannot read this and an NDJSON reader cannot read SSE, so a
//! shared reader would be a parameterised one whose two configurations have
//! nothing in common but the buffering. **The buffering is the part worth
//! saying twice**, and it is four lines; the framing is the part that differs,
//! and it is all of it. This is [ADR-0012] D3's own Negative consequence
//! arriving exactly where it predicted — "each addition is a maintenance
//! surface with its own streaming quirks".
//!
//! # A newline cannot occur inside a JSON scalar, which is what makes this
//! safe
//!
//! A literal newline inside a JSON string is not legal — it must be escaped as
//! `\n` — so a line boundary in NDJSON is a document boundary with no
//! ambiguity, and splitting on `\n` never cuts a value in half. That is the
//! same property the `gemini` client relies on for its blank-line terminator,
//! and it is why neither reader needs to track bracket depth or string
//! escaping.
//!
//! # There is no sentinel, and the stream ends when the body ends
//!
//! The terminal frame is marked by `"done": true` **inside** the JSON, which
//! is [`super::map`]'s business rather than this module's. Measured across a
//! two-frame tool-call stream, a six-frame text stream and a thirteen-frame
//! answer: the body simply ends after the frame carrying `done`. So
//! [`Frames::finish`] exists for the same reason the SSE reader's does — a
//! producer that ends without a trailing newline has still sent a whole frame,
//! and dropping it would silently lose the one frame carrying `done_reason`
//! and the entire usage, which for this API is carried nowhere else.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

/// Reassembles NDJSON frames from bytes that arrive in arbitrary pieces.
///
/// # A frame is not a read
///
/// [`reqwest::Response::chunk`] returns whatever the socket had, which is a
/// boundary of the network's choosing and never of the producer's. One read
/// can carry half a frame, three frames, or three frames and half of a fourth.
/// So the bytes are buffered and frames are taken out of the buffer, rather
/// than each read being treated as a frame — the single assumption that makes
/// such a reader work on a fast connection and fail on a slow one, where it is
/// hardest to reproduce. Stated again here rather than cross-referenced,
/// because it is the property this type exists for.
#[derive(Debug, Default)]
pub struct Frames {
    /// Bytes received and not yet consumed by a completed frame.
    buffer: Vec<u8>,
}

impl Frames {
    /// An empty reader.
    #[must_use]
    pub const fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    /// Take `bytes` from the socket and return every frame they completed.
    ///
    /// Returns one string per completed line, in order. A blank line yields
    /// nothing: it is not a frame, and handing an empty string to a JSON
    /// parser would turn a producer's harmless padding into a reported defect.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while let Some(at) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=at).collect();
            if let Some(frame) = frame_of(&line) {
                frames.push(frame);
            }
        }
        frames
    }

    /// The frame the body ended without terminating, if there was one.
    ///
    /// Called once, after the last read. See the module documentation for why
    /// a producer that sends no trailing newline has still sent a frame.
    pub fn finish(&mut self) -> Option<String> {
        let line = core::mem::take(&mut self.buffer);
        frame_of(&line)
    }
}

/// One line's payload, or `None` when the line carries no document.
///
/// A trailing `\r` is stripped as well as the `\n`, because a proxy may
/// rewrite line endings and a stray carriage return would make an otherwise
/// valid document fail to parse — a transport artefact reported as the
/// provider having sent nonsense.
fn frame_of(line: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(line);
    let trimmed = text.trim_matches(|character: char| character == '\n' || character == '\r');
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}
