// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D1's identity: a ULID, and the two impure readings it needs.
//!
//! D1: "**ULID rather than UUID: it sorts lexically by creation time, so
//! listing sessions in order costs a directory read.**"
//!
//! # No crate, and that is measured rather than asserted
//!
//! A ULID is a 48-bit millisecond timestamp followed by 80 random bits,
//! rendered as 26 Crockford base32 characters. `std` supplies both halves:
//! [`SystemTime`](std::time::SystemTime) gives the clock, and on Unix
//! `/dev/urandom` gives the bits through [`std::fs`]. [ADR-0003] D2's table
//! names no ULID crate and this module needs none — which is D2's own trigger
//! clause 7 read in the direction it points, that a dependency the harness
//! turns out not to need is removed by amendment rather than left standing.
//!
//! # The timestamp is the first ten characters, and that is load-bearing twice
//!
//! 128 bits over 26 characters is 130 bits, so the leading character carries
//! two bits of zero padding and the first ten characters are exactly the
//! timestamp. That is what makes lexical order equal creation order, and it
//! is also what lets [`SessionId::minted_at`] recover the millisecond from
//! the id alone — which [ADR-0010] D6's pruning needs, because a session's
//! filesystem modification time moves every time its transcript is appended
//! to, so an old session still in use reads as young and a restored directory
//! reads as new. **The age is in the name.**
//!
//! # Two ports' worth of impurity, and one of them is not `zaru-core`'s
//!
//! [`WallClock`] is deliberately **not** `zaru_core::iteration::Clock`. That
//! trait returns a [`Duration`](core::time::Duration) since the clock
//! *started* — a monotonic offset, which is the right quantity for ADR-0008
//! D6's elapsed time and the wrong one for a timestamp that has to mean the
//! same thing across two runs of the program. Two clocks measuring two
//! quantities is a finding worth stating rather than a reuse to force.
//!
//! The random bits arrive through the **pure constructor** rather than
//! through a port: [`SessionId::from_parts`] is what every check drives, and
//! [`SessionId::mint`] is the one function that reads `/dev/urandom`. One
//! named impure function, findable by one search — the shape
//! [`Home::of_this_user`](crate::config::Home::of_this_user)
//! and `zaru-core`'s `SystemClock` already use.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use core::fmt;
use std::io::Read;

/// Crockford base32, as ULID spells it: no `I`, `L`, `O` or `U`.
///
/// Those four are the characters a person transcribing an id from a terminal
/// confuses with `1`, `1`, `0` and `V`, and an id a user cannot read back to
/// you is an id that cannot appear in a bug report — which is what
/// [ADR-0016](https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy)
/// D3 asks a session id to do.
pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many characters a ULID is.
pub const ID_LENGTH: usize = 26;

/// How many of those carry the timestamp.
const TIMESTAMP_LENGTH: usize = 10;

/// How many random bytes follow the timestamp. 80 bits.
const ENTROPY_BYTES: usize = 10;

/// One past the largest millisecond 48 bits can hold.
const TIMESTAMP_CEILING: u64 = 1 << 48;

/// Where the harness's own randomness comes from.
const ENTROPY_SOURCE: &str = "/dev/urandom";

/// Milliseconds since the Unix epoch.
///
/// A newtype rather than a bare `u64` because this module also holds
/// durations and offsets, and a number whose unit lives only in a parameter
/// name is a number two callers will disagree about
/// ([Verification lessons] §21).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Millis(u64);

impl Millis {
    /// Take a reading.
    #[must_use]
    pub const fn new(millis: u64) -> Self {
        Self(millis)
    }

    /// The reading.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// How long after `earlier` this reading is, saturating at zero.
    ///
    /// Saturating rather than wrapping or panicking: a clock that went
    /// backwards is a machine's problem and [ADR-0016] D3 would report a
    /// panic here as a defect in the harness, which it would not be.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub const fn since(self, earlier: Self) -> u64 {
        self.0.saturating_sub(earlier.0)
    }
}

impl fmt::Display for Millis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Where a wall-clock reading comes from.
///
/// A port, so that every check names the millisecond its ids were minted at
/// rather than asserting on the machine's clock — which the testing contract
/// forbids, and which would make [`SessionId`]'s ordering check a statement
/// about how fast the machine is.
pub trait WallClock {
    /// Milliseconds since the Unix epoch.
    fn now(&self) -> Millis;
}

/// The clock the product uses.
///
/// **This is the only place in this crate that reads the machine's wall
/// clock**, the same discipline `zaru-core`'s `SystemClock` holds for the
/// monotonic one.
///
/// A machine whose clock is before the Unix epoch reads as zero rather than
/// panicking. That is a wrong answer and it is the least wrong one available:
/// every session on such a machine gets the same wrong clock, so D1's
/// ordering still holds among them, and a panic would become an ADR-0016 D3
/// defect report about the user's hardware.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemWallClock;

impl WallClock for SystemWallClock {
    fn now(&self) -> Millis {
        Millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| {
                    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
                }),
        )
    }
}

/// A session id could not be minted.
#[derive(Debug)]
pub enum MintFailure {
    /// The clock is past what 48 bits can hold.
    ClockBeyondTheEncoding {
        /// The reading that did not fit.
        millis: Millis,
    },
    /// The machine's randomness could not be read.
    NoEntropy {
        /// What the operating system said.
        source: std::io::Error,
    },
}

impl fmt::Display for MintFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClockBeyondTheEncoding { millis } => write!(
                f,
                "the wall clock reads {millis} milliseconds since the Unix epoch, which is past \
                 the 48 bits a ULID's timestamp holds; a ULID was chosen so that listing \
                 sessions in creation order costs a directory read, and an id whose timestamp \
                 wrapped would sort before every session that came earlier"
            ),
            Self::NoEntropy { source } => write!(
                f,
                "could not read {ENTROPY_SOURCE}: {source}. A session id's 80 random bits are \
                 what keep two sessions started in the same millisecond apart, and an id minted \
                 without them would name a directory another session is already writing to"
            ),
        }
    }
}

impl std::error::Error for MintFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } => Some(source),
            Self::ClockBeyondTheEncoding { .. } => None,
        }
    }
}

/// Why a string was not taken as a session id.
///
/// **Carries no offered value beyond its length and its first offending
/// character.** A session directory's name is read off a filesystem the user
/// controls, so it is a boundary in the sense
/// [Operating Principles](https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles)
/// means — "anything read off disk that a person or an older version could
/// have written" — and a refusal that quoted a whole directory name into a
/// terminal would carry whatever a person put there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIdRefused {
    /// The text is not 26 characters.
    WrongLength {
        /// How many characters it had.
        found: usize,
    },
    /// The text carries a character Crockford base32 does not use.
    ///
    /// The character is escaped and the position given, so a reader can find
    /// it without the whole name being printed.
    NotInTheAlphabet {
        /// Where, counting from zero.
        at: usize,
        /// The character, escaped.
        found: String,
    },
    /// The leading character sets one of the two padding bits, so the
    /// timestamp would not fit in 48 bits.
    BeyondTheEncoding,
}

impl fmt::Display for SessionIdRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongLength { found } => write!(
                f,
                "a session id is {ID_LENGTH} characters and this one is {found}; a session id \
                 is a ULID, and a directory under ~/.zaru/sessions/ whose name is not one is \
                 not a session this harness wrote"
            ),
            Self::NotInTheAlphabet { at, found } => write!(
                f,
                "the character {found} at position {at} is not in Crockford base32, which omits \
                 I, L, O and U so that an id read out of a terminal survives being typed back in"
            ),
            Self::BeyondTheEncoding => write!(
                f,
                "the leading character sets a bit a ULID's two-bit padding leaves clear, so the \
                 timestamp would not fit in the 48 bits the ordering depends on"
            ),
        }
    }
}

impl std::error::Error for SessionIdRefused {}

/// [ADR-0010] D1's session identity.
///
/// Ordered by the derived `Ord` over its text, which is D1's whole reason for
/// choosing a ULID: **lexical order is creation order**, so listing sessions
/// in order costs a directory read and a sort.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Build an id from a timestamp and 80 bits, with no clock and no
    /// randomness read.
    ///
    /// **This is the constructor every check drives**, so that an assertion
    /// about ordering is an assertion about the encoding rather than about
    /// how fast the machine ran.
    ///
    /// # Errors
    ///
    /// [`MintFailure::ClockBeyondTheEncoding`] when `minted_at` needs more
    /// than 48 bits.
    pub fn from_parts(
        minted_at: Millis,
        entropy: [u8; ENTROPY_BYTES],
    ) -> Result<Self, MintFailure> {
        if minted_at.get() >= TIMESTAMP_CEILING {
            return Err(MintFailure::ClockBeyondTheEncoding { millis: minted_at });
        }

        // 48 timestamp bits, then 80 entropy bits, as one 128-bit number.
        let mut value: u128 = u128::from(minted_at.get());
        for byte in entropy {
            value = (value << 8) | u128::from(byte);
        }

        let mut text = [0u8; ID_LENGTH];
        for slot in text.iter_mut().rev() {
            *slot = ALPHABET[(value & 0x1f) as usize];
            value >>= 5;
        }

        Ok(Self(
            String::from_utf8(text.to_vec()).expect("every ALPHABET byte is ASCII"),
        ))
    }

    /// Mint an id now, reading the clock and `/dev/urandom`.
    ///
    /// # Errors
    ///
    /// [`MintFailure::NoEntropy`] when the machine's randomness cannot be
    /// read, and [`MintFailure::ClockBeyondTheEncoding`] from
    /// [`SessionId::from_parts`].
    pub fn mint(clock: &dyn WallClock) -> Result<Self, MintFailure> {
        Self::from_parts(clock.now(), entropy()?)
    }

    /// Take an id off a directory listing.
    ///
    /// # Errors
    ///
    /// [`SessionIdRefused`] for anything that is not 26 Crockford base32
    /// characters whose leading character leaves the padding bits clear.
    pub fn parse(text: &str) -> Result<Self, SessionIdRefused> {
        let bytes = text.as_bytes();
        if bytes.len() != ID_LENGTH {
            return Err(SessionIdRefused::WrongLength {
                found: text.chars().count(),
            });
        }
        for (at, byte) in bytes.iter().enumerate() {
            if !ALPHABET.contains(byte) {
                return Err(SessionIdRefused::NotInTheAlphabet {
                    at,
                    found: char::from(*byte).escape_debug().to_string(),
                });
            }
        }
        // The leading character carries five bits of which the top two are a
        // ULID's padding, so anything past index 7 in the alphabet overflows.
        if value_of(bytes[0]) > 7 {
            return Err(SessionIdRefused::BeyondTheEncoding);
        }
        Ok(Self(text.to_owned()))
    }

    /// The id.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// When this id was minted, recovered from the id itself.
    ///
    /// The first ten characters are fifty bits of which the top two are zero,
    /// so this is exact rather than approximate. [ADR-0010] D6's pruning
    /// reads the age here rather than from the filesystem, because a
    /// transcript that is still being appended to moves its directory's
    /// modification time.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn minted_at(&self) -> Millis {
        let mut value: u64 = 0;
        for byte in self.0.as_bytes().iter().take(TIMESTAMP_LENGTH) {
            value = (value << 5) | u64::from(value_of(*byte));
        }
        Millis(value)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a Crockford byte sits in the alphabet.
///
/// Returns 0 for a byte that is not in it, which is unreachable from
/// [`SessionId`] because both constructors refuse one first.
fn value_of(byte: u8) -> u8 {
    ALPHABET
        .iter()
        .position(|candidate| *candidate == byte)
        .and_then(|index| u8::try_from(index).ok())
        .unwrap_or(0)
}

/// The one function in this crate that reads the machine's randomness.
///
/// # Errors
///
/// [`MintFailure::NoEntropy`] when `/dev/urandom` cannot be read.
fn entropy() -> Result<[u8; ENTROPY_BYTES], MintFailure> {
    let mut bytes = [0u8; ENTROPY_BYTES];
    std::fs::File::open(ENTROPY_SOURCE)
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|source| MintFailure::NoEntropy { source })?;
    Ok(bytes)
}
