// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A byte budget on the stream an SSE parser is fed from.
//!
//! # Why this exists at all
//!
//! `rmcp`'s `StreamableHttpClient`
//! carries a `max_sse_event_size` on every call and says in its own
//! documentation: *"The built-in reqwest and Unix socket clients enforce this
//! value. Custom `StreamableHttpClient` implementations must override the
//! corresponding `*_with_max_sse_event_size` methods to enforce it."* The SDK's
//! own enforcement is `bounded_sse_stream`, which is `pub(crate)` and therefore
//! not reachable from here, so an implementation outside the SDK either writes
//! one or ships a network-fed buffer with no ceiling. This crate writes one.
//!
//! The thing being bounded is not a niche case. An SSE parser accumulates bytes
//! until it sees an event terminator; a peer that never sends one grows that
//! buffer without limit, and the peer is on the other side of a socket.
//!
//! # What this bound is, and what it deliberately is not
//!
//! It counts **raw bytes since the last event terminator** — `\n\n` or
//! `\r\n\r\n` — and refuses when that count passes the ceiling. It is
//! deliberately coarser than the SDK's, which additionally discards completed
//! comment lines from the count and models the parser's own line state. Coarser
//! in the safe direction: this one refuses a stream the SDK's would have
//! accepted, and never accepts one the SDK's would have refused, because every
//! byte the SDK counts is a byte this counts too.
//!
//! Saying that plainly matters more than matching. A bound that claimed to be
//! the SDK's and was not would be a number nobody could reason about; a bound
//! that is stated as a raw-byte ceiling is one anybody can.
//!
//! # The terminator search crosses chunk boundaries
//!
//! A network body arrives in chunks chosen by the network, so `\r\n\r\n` can
//! be split across two of them. The scan therefore keeps the last three bytes
//! of the previous chunk and searches the join, rather than searching each
//! chunk alone — which would miss every terminator that straddled one and
//! refuse a perfectly ordinary stream.

use core::fmt;

/// The ceiling this crate applies when `rmcp` does not name one.
///
/// The same number `rmcp`'s own `DEFAULT_MAX_SSE_EVENT_SIZE` carries, read off
/// `rmcp-3.2.0/src/transport/common/client_side_sse.rs` rather than chosen
/// here: that constant is `pub(crate)`, so the value is copied and its source
/// named. A ceiling this crate invented would be a number nobody decided.
pub const DEFAULT_MAX_SSE_EVENT_SIZE: usize = 16 * 1024 * 1024;

/// How many trailing bytes of one chunk can be part of a terminator that
/// completes in the next.
///
/// Three, because the longest terminator is four bytes and a terminator that
/// began in a previous chunk contributes at most its first three.
const TERMINATOR_OVERLAP: usize = 3;

/// A stream carried more bytes without an event terminator than the ceiling
/// allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTooLarge {
    /// The ceiling that was passed.
    pub ceiling: usize,
    /// How many bytes had accumulated when it was.
    pub accumulated: usize,
}

impl fmt::Display for EventTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the server sent {} byte(s) with no event terminator, past the maximum size of {} \
             bytes",
            self.accumulated, self.ceiling
        )
    }
}

impl std::error::Error for EventTooLarge {}

/// The running count, kept across the chunks of one stream.
///
/// A plain struct rather than a closure's captured state, so that the counting
/// can be checked without a stream, a runtime, or a socket.
#[derive(Debug)]
pub struct Budget {
    ceiling: usize,
    accumulated: usize,
    /// The tail of the previous chunk, so a terminator split across two is
    /// still found.
    carry: Vec<u8>,
}

impl Budget {
    /// A budget with `ceiling` bytes between event terminators.
    #[must_use]
    pub const fn new(ceiling: usize) -> Self {
        Self {
            ceiling,
            accumulated: 0,
            carry: Vec::new(),
        }
    }

    /// Account for one chunk, refusing when the ceiling is passed.
    ///
    /// **Every run in the chunk is checked, not only the trailing one.** A scan
    /// that watched the tail alone would accept one chunk carrying a hundred
    /// megabytes followed by a terminator, which is the growth this exists to
    /// refuse arriving in one piece instead of many.
    ///
    /// # Errors
    ///
    /// [`EventTooLarge`] naming the ceiling and what had accumulated.
    pub fn take(&mut self, chunk: &[u8]) -> Result<(), EventTooLarge> {
        let carried = self.carry.len();
        let mut joined = core::mem::take(&mut self.carry);
        joined.extend_from_slice(chunk);

        let mut at = 0;
        while at < joined.len() {
            let terminator = if joined[at..].starts_with(b"\r\n\r\n") {
                Some(4)
            } else if joined[at..].starts_with(b"\n\n") {
                Some(2)
            } else {
                None
            };
            match terminator {
                Some(width) => {
                    // Everything up to here has been delivered as an event, so
                    // the parser's buffer is empty again.
                    self.accumulated = 0;
                    at += width;
                }
                None => {
                    // The carried bytes were counted when their own chunk
                    // arrived. Re-walking them is what finds a terminator split
                    // across the boundary; counting them again would inflate
                    // every run by up to three bytes.
                    if at >= carried {
                        self.accumulated += 1;
                        if self.accumulated > self.ceiling {
                            let accumulated = self.accumulated;
                            self.carry.clear();
                            return Err(EventTooLarge {
                                ceiling: self.ceiling,
                                accumulated,
                            });
                        }
                    }
                    at += 1;
                }
            }
        }

        let keep = joined.len().saturating_sub(TERMINATOR_OVERLAP);
        self.carry = joined.split_off(keep);
        Ok(())
    }
}

/// Wrap one chunk result in the budget, turning either failure into an
/// [`std::io::Error`] the SSE parser can carry.
///
/// The parser's own error type takes a boxed `std::error::Error`, so both a
/// transport failure and a budget refusal reach the caller with their own
/// wording intact.
///
/// Generic over the chunk type rather than taking `bytes::Bytes`, so that this
/// crate names no crate the transport does not already need: what arrives from
/// `reqwest` is a `Bytes` and what leaves is the same value, unread except for
/// its length.
pub fn account<B, E>(budget: &mut Budget, chunk: Result<B, E>) -> Result<B, std::io::Error>
where
    B: AsRef<[u8]>,
    E: std::error::Error + Send + Sync + 'static,
{
    let bytes = chunk.map_err(std::io::Error::other)?;
    budget.take(bytes.as_ref()).map_err(std::io::Error::other)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_under_the_ceiling_is_accepted() {
        let mut budget = Budget::new(16);
        budget
            .take(b"data: ab\n\n")
            .expect("ten bytes under a ceiling of sixteen");
    }

    #[test]
    fn a_run_with_no_terminator_is_refused_naming_both_numbers() {
        let mut budget = Budget::new(8);
        let refusal = budget
            .take(b"data: aaaaaaaaaaaa")
            .expect_err("eighteen bytes with no terminator must pass a ceiling of eight");
        assert_eq!(refusal.ceiling, 8);
        assert_eq!(
            refusal.accumulated, 9,
            "the refusal reports the count at the byte that passed the ceiling, not the chunk"
        );
        let rendered = refusal.to_string();
        assert!(
            rendered.contains("9 byte(s)") && rendered.contains("maximum size of 8 bytes"),
            "the refusal names neither number: {rendered}"
        );
    }

    #[test]
    fn the_count_resets_at_an_event_terminator() {
        let mut budget = Budget::new(8);
        // Twenty bytes in total, and every run between terminators is eight,
        // so nothing passes the ceiling.
        budget
            .take(b"data: aa\n\ndata: bb\n\n")
            .expect("two runs of eight under a ceiling of eight must be accepted");
    }

    #[test]
    fn a_long_run_inside_one_chunk_is_refused_even_though_it_ends_in_a_terminator() {
        // The sibling of the check above, and the one that fails if only the
        // trailing run is watched: this chunk's tail is empty and its body is
        // twelve bytes.
        let mut budget = Budget::new(8);
        let refusal = budget
            .take(b"data: aaaaaa\n\n")
            .expect_err("a twelve-byte run must be refused however the chunk ends");
        assert_eq!(refusal.accumulated, 9);
    }

    #[test]
    fn a_run_split_across_chunks_still_accumulates() {
        let mut budget = Budget::new(8);
        budget.take(b"data: ").expect("six bytes is under eight");
        let refusal = budget
            .take(b"aaaaa")
            .expect_err("six plus five is eleven and must pass a ceiling of eight");
        assert_eq!(refusal.accumulated, 9);
    }

    #[test]
    fn a_terminator_split_across_chunks_is_found() {
        let mut budget = Budget::new(16);
        budget
            .take(b"data: aa\r")
            .expect("nine bytes under a ceiling of sixteen");
        budget
            .take(b"\n\r\ndata: bb")
            .expect("the terminator resets the count, so the second run is nine");
    }

    #[test]
    fn the_same_bytes_without_the_terminator_are_refused() {
        // The discriminating sibling of the check above. Same lengths, same
        // ceiling; only the terminator differs, so a scan that never found one
        // could not pass both.
        let mut budget = Budget::new(16);
        budget.take(b"data: aax").expect("nine bytes under sixteen");
        budget
            .take(b"xxxxdata: bb")
            .expect_err("nine plus twelve with no terminator must pass sixteen");
    }

    #[test]
    fn a_transport_failure_reaches_the_caller_with_its_own_wording() {
        let mut budget = Budget::new(64);
        let failure: Result<Vec<u8>, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        ));
        let carried = account(&mut budget, failure).expect_err("a failed chunk cannot be a chunk");
        assert!(
            carried.to_string().contains("connection reset by peer"),
            "the transport's own sentence was lost: {carried}"
        );
    }

    #[test]
    fn an_accepted_chunk_reaches_the_caller_unchanged() {
        let mut budget = Budget::new(64);
        let chunk: Result<Vec<u8>, std::io::Error> = Ok(b"data: hello\n\n".to_vec());
        let passed = account(&mut budget, chunk).expect("thirteen bytes under sixty-four");
        assert_eq!(passed, b"data: hello\n\n".to_vec());
    }

    #[test]
    fn the_default_ceiling_is_the_sdks_own_number() {
        assert_eq!(DEFAULT_MAX_SSE_EVENT_SIZE, 16 * 1024 * 1024);
    }
}
