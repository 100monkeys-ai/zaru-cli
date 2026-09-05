// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Server-sent-event framing for `streamGenerateContent`.
//!
//! # What this module is, and what it deliberately is not
//!
//! It turns a byte stream into the payloads of its `data:` fields, and it
//! knows nothing about Gemini. It does not parse JSON, it does not know what
//! a `functionCall` is, and it has no opinion about what a payload means.
//! [`super::map`] is where a payload becomes a response, and keeping the two
//! apart is what lets the framing be checked over bytes that never came from
//! a socket.
//!
//! # Why the transport is `?alt=sse` and not the default
//!
//! `streamGenerateContent` without `alt=sse` returns a streamed **JSON
//! array** — the response is one document delivered in pieces, so a reader
//! has to track bracket depth and string escaping to know where one element
//! ends. With `alt=sse` the same content arrives pre-framed, and the frame
//! terminator is a blank line that cannot occur inside a JSON scalar. The
//! second is a boundary the transport states; the first is a boundary the
//! reader has to infer, and inferring it means writing a partial JSON parser
//! beside the real one.
//!
//! **Measured 2026-09-05** against `gemini-3.6-flash`: `?alt=sse` answers
//! `content-type: text/event-stream` and frames of the form `data: {json}`
//! separated by a blank line.
//!
//! # There is no sentinel, and the stream ends when the body ends
//!
//! Some SSE producers close with a `data: [DONE]` frame. **This one does
//! not** — measured on the same day, across a text stream of three frames and
//! a tool-call stream of two. So end-of-stream is end-of-body and nothing
//! else, which is why [`Frames::finish`] exists: a producer that ends without
//! a trailing blank line has still sent a whole frame, and dropping it would
//! silently lose the frame carrying `finishReason` and the final usage.
//!
//! # What a frame is, by the specification rather than by this producer
//!
//! Events are separated by a blank line. Within an event, a line is a field:
//! everything before the first `:` is the name, everything after it is the
//! value, and one leading space on the value is stripped. A line beginning
//! `:` is a comment. Multiple `data` fields in one event join with `\n`.
//!
//! **This is implemented to the specification rather than to what Google
//! happens to send**, which is one `data` field per event and no comments.
//! The reason is not generality for its own sake: a heartbeat comment is the
//! ordinary way an SSE producer keeps a connection alive through a proxy, and
//! a reader that treated one as a frame would hand [`super::map`] a payload
//! that is not JSON and turn a healthy connection into a reported defect.

/// Reassembles SSE frames from bytes that arrive in arbitrary pieces.
///
/// # A frame is not a read
///
/// [`reqwest::Response::chunk`] returns whatever the socket had, which is a
/// boundary of the network's choosing and never of the producer's. One read
/// can carry half a frame, three frames, or three frames and half of a
/// fourth. So the bytes are buffered and frames are taken out of the buffer,
/// rather than each read being treated as a frame — which is the single
/// assumption that makes an SSE reader work on a fast connection and fail on
/// a slow one, where it is hardest to reproduce.
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
    /// Returns the `data` payload of each completed event, in order. An event
    /// carrying no `data` field — a bare comment, or a producer's heartbeat —
    /// completes and yields nothing, because there is no payload to hand on.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while let Some((event, rest)) = split_event(&self.buffer) {
            if let Some(payload) = payload_of(&event) {
                frames.push(payload);
            }
            self.buffer = rest;
        }
        frames
    }

    /// The frame the body ended without terminating, if there was one.
    ///
    /// Called once, after the last read. See the module documentation for why
    /// a producer that sends no trailing blank line has still sent a frame.
    pub fn finish(&mut self) -> Option<String> {
        let event = core::mem::take(&mut self.buffer);
        if event.iter().all(u8::is_ascii_whitespace) {
            return None;
        }
        payload_of(&event)
    }
}

/// Split the buffer at the first frame terminator.
///
/// Returns the event's bytes and everything after the terminator, or `None`
/// when the buffer holds no complete event yet.
///
/// Both `\n\n` and `\r\n\r\n` terminate, because the specification allows a
/// producer either line ending and a proxy may rewrite them.
fn split_event(buffer: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut at = 0;
    while at < buffer.len() {
        if buffer[at..].starts_with(b"\r\n\r\n") {
            return Some((buffer[..at].to_vec(), buffer[at + 4..].to_vec()));
        }
        if buffer[at..].starts_with(b"\n\n") {
            return Some((buffer[..at].to_vec(), buffer[at + 2..].to_vec()));
        }
        at += 1;
    }
    None
}

/// The joined `data` payload of one event's bytes.
///
/// `None` when the event carries no `data` field at all, which is a comment
/// or a producer's heartbeat rather than a message.
fn payload_of(event: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(event);
    let mut data: Option<String> = None;
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let (name, value) = match line.split_once(':') {
            Some((name, value)) => (name, value.strip_prefix(' ').unwrap_or(value)),
            // A field with no colon is a name with an empty value.
            None => (line, ""),
        };
        // **This one comparison is also what discards a comment**, and there
        // is deliberately no second branch for one. A comment line begins
        // with `:`, so its name is the empty string — which is not `data`,
        // and is dropped here with every other field this reader does not
        // read. An explicit `starts_with(':')` guard was written first and
        // removed: no mutation could redden it, because nothing reached it
        // that this comparison did not already discard, and a branch no
        // check can distinguish from its absence is a branch that is not
        // doing anything.
        if name != "data" {
            continue;
        }
        match data.as_mut() {
            // The specification joins repeated `data` fields with a newline.
            Some(held) => {
                held.push('\n');
                held.push_str(value);
            }
            None => data = Some(value.to_owned()),
        }
    }
    data
}
