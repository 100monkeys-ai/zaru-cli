// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Lower-case hexadecimal, by hand, because the dependency table says so.
//!
//! # Why no crate
//!
//! [ADR-0003] D2's table is closed on purpose and names no encoding crate.
//! Its own trigger clause 7 reads in the direction that settles this: "a
//! dependency the harness turns out not to need is removed by amendment
//! rather than left standing unused". `session/id.rs` set the precedent by
//! hand-writing Crockford base32 for the same reason, and hexadecimal is the
//! smaller of the two.
//!
//! # Why hexadecimal rather than base64
//!
//! Base64 would be shorter and would need an alphabet, a padding rule and a
//! decoder that agrees with whichever variant the writer chose. Hexadecimal
//! has one variant. The sealed blob is 61 bytes for an ordinary bearer, so
//! 122 characters against base64's 84 — twenty-odd bytes on a file nobody
//! reads by hand is not worth a second encoding to get wrong.
//!
//! # Both halves refuse rather than guess
//!
//! [`decode`] returns `None` for an odd length and for any character outside
//! `0-9a-f`. **Upper case is refused**, not accepted-and-lowered: this module
//! is the only writer of what it reads, it writes lower case, and a decoder
//! that accepts a spelling the encoder never produces is a decoder nobody can
//! test against its own output.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing

/// The digits, in the order their values name them.
const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Render bytes as lower-case hexadecimal.
pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Read lower-case hexadecimal back, refusing anything else.
///
/// `None` for an odd length, for an upper-case digit, and for any character
/// that is not a hexadecimal digit.
pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let (pairs, remainder) = bytes.as_chunks::<2>();
    debug_assert!(remainder.is_empty(), "the length was checked above");
    for [high, low] in pairs {
        let high = value(*high)?;
        let low = value(*low)?;
        out.push((high << 4) | low);
    }
    Some(out)
}

/// One digit's value, or `None` for anything this module does not write.
const fn value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}
