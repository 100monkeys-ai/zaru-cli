// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A dotted configuration key, and the shapes no layer will take.
//!
//! ADR-0014 D3 explains `runtime.max_iterations` and D5 refuses
//! `runtime.max_iteration` naming the nearest match, so a key is a
//! dot-separated path and both of those surfaces render it back to a person.
//!
//! # Every refusal is derived from a surface, never from taste
//!
//! There is **no character allowlist and no length cap**. A cap nobody chose
//! is a value chosen for a different caller ([Verification lessons] §14), and
//! an allowlist would refuse a non-Latin key for no reason any record gives.
//! What is refused is what D3's explain block, D5's suggestion, or
//! ADR-0014 D1's own dotted spelling cannot represent.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use core::fmt;

/// Why a key was not taken.
///
/// A key is not a value, so a refusal quotes it back: a reader has to be able
/// to see which key was rejected. Nothing here can carry a value, which is
/// what keeps [`crate::config::refusal::ConfigRefused`]'s promise reachable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRefused {
    /// The key was empty.
    Empty,
    /// A segment between two dots was empty.
    ///
    /// This covers a leading dot, a trailing dot and a doubled dot at once,
    /// because all three produce the same thing: a level of the table with no
    /// name, which D3's explain block would render as a blank.
    EmptySegment {
        /// The key as it was offered.
        offered: String,
    },
    /// The key carried a control character.
    ///
    /// D3 renders keys into a terminal block. A control character there can
    /// move the cursor or erase a neighbouring row, so the block stops being
    /// evidence about what the layers hold.
    Control {
        /// The key as it was offered.
        offered: String,
    },
    /// A segment began or ended with whitespace.
    ///
    /// Two keys differing only in invisible characters are one key to every
    /// reader of D3's block and of D5's suggestion.
    SurroundingWhitespace {
        /// The key as it was offered.
        offered: String,
    },
}

impl fmt::Display for KeyRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                // ADR-0014 D3 explains a key by name and D5 suggests one by name.
                "a configuration key is empty; a key is explained and suggested by name, and \
                 neither can name nothing",
            ),
            Self::EmptySegment { offered } => write!(
                f,
                "the key {offered:?} has a segment with no name; a leading dot, a trailing dot \
                 or a doubled dot each produce a level of the table D3's explain block would \
                 render as a blank",
            ),
            Self::Control { offered } => write!(
                f,
                "the key {offered:?} carries a control character; keys are rendered into \
                 a terminal block where one can erase or overwrite a neighbouring row",
            ),
            Self::SurroundingWhitespace { offered } => write!(
                f,
                "the key {offered:?} has a segment beginning or ending with whitespace; two \
                 keys differing only there are one key to every reader of D3's block",
            ),
        }
    }
}

impl std::error::Error for KeyRefused {}

/// A dotted configuration key, such as `runtime.max_iterations`.
///
/// Constructed only through [`Key::new`], so a key that reached this type has
/// already been refused every shape [`KeyRefused`] names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key(String);

impl Key {
    /// Take a key, refusing the shapes D3 and D5 cannot render.
    ///
    /// # Errors
    ///
    /// One variant of [`KeyRefused`] per shape; the first found is returned,
    /// and the order is fixed so a given input always names the same reason.
    pub fn new(offered: &str) -> Result<Self, KeyRefused> {
        if offered.is_empty() {
            return Err(KeyRefused::Empty);
        }
        if offered.chars().any(char::is_control) {
            return Err(KeyRefused::Control {
                offered: offered.to_string(),
            });
        }
        for segment in offered.split('.') {
            if segment.is_empty() {
                return Err(KeyRefused::EmptySegment {
                    offered: offered.to_owned(),
                });
            }
            if segment.trim() != segment {
                return Err(KeyRefused::SurroundingWhitespace {
                    offered: offered.to_owned(),
                });
            }
        }
        Ok(Self(offered.to_owned()))
    }

    /// The key as it was written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The key's dot-separated segments, outermost first.
    ///
    /// Never empty: [`Key::new`] refuses an empty key and an empty segment,
    /// so every key has at least one.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('.')
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
