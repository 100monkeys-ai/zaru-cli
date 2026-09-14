// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A token's local name, and the shapes the store will not take.
//!
//! ADR-0007 D2: an alias is user-supplied local metadata — "Local unique
//! name. The handle everywhere — CLI, transcript, agent tool namespace."
//! Nuclear Notes has no notion of it, so nothing validates it but this.
//!
//! # An alias is not a path segment, and that is the first line of defence
//!
//! The store is **one file** and an alias is a key inside it, so an alias
//! never becomes a directory or a file name and there is nothing for a
//! traversal to traverse. That is the form [Testing] calls absence rather
//! than refusal: the forbidden reach has nothing to call. The refusals below
//! are the second line, and they exist because an alias reaches two other
//! places where its shape does matter — ADR-0007 D5 renders it into an MCP
//! server name as `notes:<alias>`, and D7 renders it into a terminal listing.
//!
//! Each refusal is derived from one of those two, never from a general
//! feeling about what a name should look like. In particular there is **no
//! character allowlist and no length cap**: a cap nobody chose is a value
//! chosen for a different caller ([Verification lessons] §14), and an
//! allowlist would refuse a non-Latin name for no reason any record gives.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use core::fmt;

/// Why the store would not take an alias.
///
/// Every variant names a consequence rather than a taste. An alias is local
/// metadata and carries no secret, so a refusal quotes it back: the reader
/// has to be able to see which of their aliases was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasRefused {
    /// The alias was empty.
    Empty,
    /// The alias was `.` or `..`.
    ///
    /// Refused because these are the two names a reader will assume mean a
    /// directory whatever the store actually does with them, and because an
    /// alias that reads as a traversal in a listing invites somebody to build
    /// one later.
    DotOrDotDot,
    /// The alias carried a path separator.
    Separator {
        /// The alias as it was offered.
        offered: String,
        /// Which separator was found.
        found: char,
    },
    /// The alias carried the character ADR-0007 D5 separates a namespace with.
    ///
    /// D5 projects a token as `notes:<alias>`. An alias containing a colon
    /// makes that name ambiguous, and an ambiguous tool name is a tool call
    /// whose destination cannot be read off the transcript — which is the
    /// property D5 exists to provide.
    NamespaceSeparator {
        /// The alias as it was offered.
        offered: String,
    },
    /// The alias carried a control character.
    ///
    /// ADR-0007 D7 renders aliases into a terminal listing. A control
    /// character there can move the cursor, erase a line, or hide a
    /// neighbouring row, so a listing is no longer evidence about what the
    /// store holds.
    Control {
        /// The alias as it was offered.
        offered: String,
    },
    /// The alias began or ended with whitespace.
    ///
    /// Refused because two aliases differing only in invisible characters
    /// read as one alias in every surface D7 names.
    SurroundingWhitespace {
        /// The alias as it was offered.
        offered: String,
    },
}

impl fmt::Display for AliasRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                // ADR-0007 D2 makes an alias the handle in all three places.
                "an alias is empty; it is the handle in the CLI, the transcript and the agent's \
                 tool namespace, and none of those can name nothing",
            ),
            Self::DotOrDotDot => f.write_str(
                "an alias of \".\" or \"..\" is refused; it reads as a directory traversal in \
                 every listing this harness renders",
            ),
            Self::Separator { offered, found } => write!(
                f,
                "the alias {offered:?} carries the path separator {found:?}; an alias is a key \
                 in the store's single file and never a path segment, and one that looks like a \
                 path invites a reader to build one",
            ),
            Self::NamespaceSeparator { offered } => write!(
                f,
                "the alias {offered:?} carries ':', which separates the namespace in \
                 \"notes:<alias>\"; the projected tool name would be ambiguous and \
                 the transcript would stop showing which context answered",
            ),
            Self::Control { offered } => write!(
                f,
                "the alias {offered:?} carries a control character; aliases are rendered \
                 into a terminal listing, where one can erase or overwrite a neighbouring row",
            ),
            Self::SurroundingWhitespace { offered } => write!(
                f,
                "the alias {offered:?} begins or ends with whitespace; two aliases differing \
                 only there are one alias to every reader of D7's listing",
            ),
        }
    }
}

impl std::error::Error for AliasRefused {}

/// A token's local name.
///
/// Constructed only through [`Alias::new`], so an alias that reached this
/// type has already been refused every shape [`AliasRefused`] names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Alias(String);

impl Alias {
    /// Take an alias, refusing the shapes ADR-0007 D5 and D7 cannot render.
    ///
    /// # Errors
    ///
    /// One variant of [`AliasRefused`] per shape; the first one found is
    /// returned, and the order is fixed so that a given input always names
    /// the same reason.
    pub fn new(offered: &str) -> Result<Self, AliasRefused> {
        if offered.is_empty() {
            return Err(AliasRefused::Empty);
        }
        if offered == "." || offered == ".." {
            return Err(AliasRefused::DotOrDotDot);
        }
        if let Some(found) = offered.chars().find(|c| *c == '/' || *c == '\\') {
            return Err(AliasRefused::Separator {
                offered: offered.to_owned(),
                found,
            });
        }
        if offered.contains(':') {
            return Err(AliasRefused::NamespaceSeparator {
                offered: offered.to_owned(),
            });
        }
        // `is_control` covers NUL, every C0 and C7 code, and the escape that
        // starts an ANSI sequence. Naming NUL separately would be a comment
        // rather than a mechanism.
        if offered.chars().any(char::is_control) {
            return Err(AliasRefused::Control {
                offered: offered.to_owned(),
            });
        }
        if offered.trim() != offered {
            return Err(AliasRefused::SurroundingWhitespace {
                offered: offered.to_owned(),
            });
        }
        Ok(Self(offered.to_owned()))
    }

    /// The alias as the user wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
