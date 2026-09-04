// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The slow tier: the request the composer emits, the response it is handed,
//! and the two numbers ADR-0005 D3 supplies.
//!
//! # Why this is not a port
//!
//! Tier one is a trait the composer calls ([`Entries`]). Tier two is not: the
//! composer **emits** a [`SearchRequest`] as an output of its own step and
//! **accepts** a [`SearchResponse`] as an input. Nothing is called, two values
//! cross, and the composer stays a synchronous state machine that needs no
//! runtime.
//!
//! That shape is what makes D3's "the fast tier never touches the network"
//! checkable rather than assumed, because "made no network call" becomes "emitted
//! no request" — a count a test owns, taken from the harness rather than from a
//! field the composer keeps about itself.
//!
//! # The two numbers are the record's, not this crate's
//!
//! D3 fixes both: "debounced at 250ms, minimum three characters". Unlike the
//! loop's iteration ceiling and truncation budget, which no record carried and
//! which therefore arrive as parameters, these are decided and are constants
//! here.
//!
//! [`Entries`]: crate::composer::Entries

use crate::composer::entries::Entry;
use core::fmt;
use core::time::Duration;

/// The shortest query the slow tier will carry, from ADR-0005 D3.
///
/// Counted in characters rather than bytes, because D3 says "characters" and
/// because a byte count would let a three-character query containing one
/// non-ASCII character through a floor it does not meet — or hold back one
/// that does.
pub const MIN_QUERY_CHARS: usize = 3;

/// How long the composer waits after the last edit before emitting a request,
/// from ADR-0005 D3.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// Which cortex a search reaches.
///
/// **There is deliberately no `AllPublic`.** ADR-0005 D6 excludes it: Nuclear
/// Notes is public by default, so `all_public` surfaces pages authored by
/// strangers, and a stranger's page appearing in a picker the user trusts is a
/// phishing surface. Modelling it as a variant nobody constructs would leave a
/// reach a future caller could take; leaving it out means the forbidden reach
/// has nothing to call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The attached workspace alone. ADR-0006 D2 moves it only by user action.
    Workspace,
    /// Every workspace this user is a member of.
    AllMine,
}

impl Scope {
    /// Every scope the composer may search in.
    ///
    /// A hand-written list, guarded by the exhaustive match in
    /// `a_search_request_carries_only_the_two_scopes_the_record_allows`:
    /// adding a variant fails to compile there, which is the signal that D6 is
    /// being changed rather than extended.
    pub const ALL: [Self; 2] = [Self::Workspace, Self::AllMine];

    /// The scope's name as the MCP surface spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::AllMine => "all_mine",
        }
    }
}

/// A query too short for the slow tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestRefused {
    /// How many characters the query actually had.
    pub chars: usize,
}

impl fmt::Display for RequestRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a query of {} character(s) is below the {MIN_QUERY_CHARS}-character floor, so no \
             search request exists to make",
            self.chars
        )
    }
}

impl std::error::Error for RequestRefused {}

/// What the composer asks the slow tier for.
///
/// Constructed only through [`SearchRequest::new`], which refuses a query
/// below the floor. Under three characters there is no request to emit, which
/// is D3's fast tier stated as something the type system holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    query: String,
    scope: Scope,
    tag: Option<String>,
}

impl SearchRequest {
    /// Take a request, refusing a query below the floor.
    ///
    /// # Errors
    ///
    /// [`RequestRefused`] when `query` has fewer than [`MIN_QUERY_CHARS`]
    /// characters.
    pub fn new(query: &str, scope: Scope, tag: Option<&str>) -> Result<Self, RequestRefused> {
        let chars = query.chars().count();
        if chars < MIN_QUERY_CHARS {
            return Err(RequestRefused { chars });
        }
        Ok(Self {
            query: query.to_owned(),
            scope,
            tag: tag.map(str::to_owned),
        })
    }

    /// What to search for.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Which cortex to search.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }

    /// The tag the user typed with `#`, which ADR-0005 D4 has scope the live
    /// search and attach nothing.
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }
}

/// What the slow tier returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResponse {
    /// The entries the server ranked, best first.
    pub results: Vec<Entry>,
    /// Whether semantic ranking was available for this search.
    ///
    /// ADR-0005 D8: `search.global` reports this, and when it is false the
    /// strip says so rather than silently serving worse results.
    pub semantic_available: bool,
}

/// Where the slow tier has got to for the query now in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchState {
    /// Nothing has been asked for — the query is below the floor, or a picker
    /// is open, or the prompt is empty.
    Idle,
    /// A request has been emitted and no response has arrived.
    Awaiting,
    /// A response arrived.
    Returned {
        /// Whether semantic ranking was available, carried through from the
        /// response so the strip can render D8's `keyword only`.
        semantic_available: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::{MIN_QUERY_CHARS, RequestRefused, Scope, SearchRequest};

    /// ADR-0005 D6. The check is that the forbidden reach has nothing to call,
    /// not that a call to it is denied — a denial is a code path and a code
    /// path can be wrong.
    ///
    /// The mutant this catches is adding an `AllPublic` variant: the match
    /// below is exhaustive and stops compiling, which is a louder signal than
    /// an assertion, and the length assertion catches a variant added
    /// alongside a widened `ALL`.
    #[test]
    fn a_search_request_carries_only_the_two_scopes_the_record_allows() {
        let mut named = Vec::new();
        for scope in Scope::ALL {
            // Exhaustive on purpose. A third variant fails to compile here.
            let name = match scope {
                Scope::Workspace => "workspace",
                Scope::AllMine => "all_mine",
            };
            assert_eq!(scope.as_str(), name);
            named.push(name);
        }
        assert_eq!(
            named,
            vec!["workspace", "all_mine"],
            "ADR-0005 D6 admits `workspace` and `all_mine` and excludes `all_public`; the scopes \
             this crate can construct are {named:?}"
        );
        assert_eq!(
            Scope::ALL.len(),
            2,
            "a scope was added to `Scope::ALL` without ADR-0005 D6 being changed"
        );
    }

    /// The representational arm of "the fast tier never touches the network":
    /// below the floor there is no request to emit, because none can be built.
    ///
    /// The floor is counted in characters. The three-character query here is
    /// five bytes, so an implementation counting bytes would let a
    /// two-character query through and hold this one back — which the second
    /// and third clauses separate.
    #[test]
    fn a_query_below_the_floor_cannot_be_turned_into_a_request() {
        assert_eq!(
            SearchRequest::new("ed", Scope::Workspace, None),
            Err(RequestRefused { chars: 2 }),
            "two characters is below the {MIN_QUERY_CHARS}-character floor"
        );
        assert!(
            RequestRefused { chars: 2 }.to_string().contains('2'),
            "the refusal should name the count that was refused"
        );

        let three_chars_five_bytes = "édé";
        assert_eq!(three_chars_five_bytes.chars().count(), 3);
        assert_eq!(three_chars_five_bytes.len(), 5);
        assert!(
            SearchRequest::new(three_chars_five_bytes, Scope::Workspace, None).is_ok(),
            "{three_chars_five_bytes:?} is three characters and meets the floor, whatever its \
             byte length is"
        );

        let two_chars_four_bytes = "éé";
        assert_eq!(two_chars_four_bytes.len(), 4);
        assert!(
            SearchRequest::new(two_chars_four_bytes, Scope::Workspace, None).is_err(),
            "{two_chars_four_bytes:?} is two characters and must be refused even though it is \
             four bytes"
        );
    }
}
