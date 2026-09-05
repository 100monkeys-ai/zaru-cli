// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The composer: the input widget and its hint strip.
//!
//! ADR-0005 is the record. One strip renders below the input, its content is a
//! pure function of composer state, and the mode is selected by whether the
//! prompt is empty.
//!
//! # Synchronous, and it holds no clock
//!
//! Nothing here is asynchronous and nothing here reads the machine's clock.
//! Every input carries `now: Duration`, so the debounce D3 fixes at 250 ms is
//! measured in whatever clock the caller is spending — `zaru-cli` passes the
//! system one, a test passes exact values. There is no clock object to read
//! the wrong clock through, which is the testing contract's prohibition on
//! asserting wall-clock time made structural rather than remembered.
//!
//! # What it needs from `zaru-core`: nothing
//!
//! D1 makes the strip's content a pure function of composer state, so the
//! composer subscribes to no event. `zaru-core`'s `EventSink` is how
//! [ADR-0028]'s execution-narrative renderer subscribes — a second surface in
//! this crate, and not this one. The `zaru-tui` to `zaru-core` edge stays as
//! the skeleton left it and the composer does not use it.
//!
//! # The two tiers
//!
//! Tier one is [`Entries`], a trait this crate declares and nothing in this
//! crate's product tree implements. Tier two is not a port at all: the
//! composer emits a [`SearchRequest`] and is handed a [`SearchResponse`].
//! [`entries`] says why the port is declared here rather than in the crate
//! that will own the trie.
//!
//! [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative

pub mod entries;
pub mod render;
pub mod search;
pub mod strip;

pub use entries::{Entries, Entry, EntryKind};
pub use render::KEYWORD_ONLY;
pub use search::{
    DEBOUNCE, MIN_QUERY_CHARS, RequestRefused, Scope, SearchRequest, SearchResponse, SearchState,
};
pub use strip::{PickerKind, StripContent, StripMode};

use core::time::Duration;
use tui_textarea::{Input, TextArea};

/// How many entries the strip will carry.
///
/// **No record names this number.** ADR-0005 sets the debounce and the
/// character floor and says nothing about how many matches a strip holds, so
/// this is a proposal made here, named, and reported on the record rather than
/// invented silently — [ADR Workflow]'s rule for a clause citing a page for
/// something the page does not say. It is a rendering budget: the strip is one
/// surface below the input on a terminal, and D2 forbids it growing into the
/// text the user is composing.
///
/// [ADR Workflow]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-workflow
pub const MATCH_LIMIT: usize = 8;

/// What the text in the input is asking for.
///
/// Derived from the text and the cursor, never stored, because D1 makes the
/// strip's content a pure function of composer state and a cached derivation
/// is a second state that can disagree with the first.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Intent {
    /// The prompt is empty. D1 rows 1 to 3.
    Empty,
    /// The line is a command, per [ADR-0015] D2. Not one of D1's rows.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Command,
    /// The cursor sits inside an explicit picker's token. D1 row 6.
    Picker { kind: PickerKind, filter: String },
    /// Ordinary text. D1 rows 4 and 5, split on the character floor.
    Query { query: String, tag: Option<String> },
}

/// The composer: the input widget and its hint strip.
///
/// Synchronous, and it holds no clock — every method that needs the time takes
/// it. See the module documentation.
#[derive(Debug)]
pub struct Composer {
    input: TextArea<'static>,
    scope: Scope,
    deposits: u32,
    tip: Option<String>,
    /// The trie's matches for whatever the input currently asks for.
    matches: Vec<Entry>,
    /// The server's results, once a response has been delivered for the query
    /// now in the input. `None` until then, so "no results yet" and "a
    /// response carrying no results" stay different states.
    server: Option<Vec<Entry>>,
    search: SearchState,
    /// What to say when the fast tier has nothing to search, or `None` when it
    /// has. Handed in, exactly as a standing tip is — see [`Composer::set_absence`].
    absence: Option<String>,
    /// When the text last changed, in the caller's clock.
    last_edit: Duration,
    /// The query a request has already been emitted for, so one query produces
    /// one request however many times the host steps the composer.
    requested: Option<String>,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// A composer with an empty prompt.
    ///
    /// The scope starts at [`Scope::Workspace`]: ADR-0006 D5 makes the
    /// attached workspace the composer's default and D2 moves it only by user
    /// action.
    #[must_use]
    pub fn new() -> Self {
        Self {
            input: TextArea::default(),
            scope: Scope::Workspace,
            deposits: 0,
            tip: None,
            matches: Vec::new(),
            server: None,
            search: SearchState::Idle,
            absence: None,
            last_edit: Duration::ZERO,
            requested: None,
        }
    }

    /// The text the user has composed.
    #[must_use]
    pub fn text(&self) -> String {
        self.input.lines().join("\n")
    }

    /// The text area, for rendering.
    #[must_use]
    pub fn input(&self) -> &TextArea<'static> {
        &self.input
    }

    /// Where the cursor is, as a row and a column in characters.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.input.cursor()
    }

    /// Which cortex the slow tier searches.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }

    /// Move the scope. ADR-0006 D2: only a user action reaches here.
    pub const fn set_scope(&mut self, scope: Scope) {
        self.scope = scope;
    }

    /// Hand the composer what the empty-prompt mode should show.
    ///
    /// Both producers are ADR-0002's and neither exists: deposits come from
    /// D3's deposit channel and `/inbox`, standing tips from D8's budget and
    /// its three-displays-without-action suppression. The composer is handed
    /// the values and decides only the precedence between them, which is D1's
    /// "deposits outrank tips".
    pub fn set_standing(&mut self, deposits: u32, tip: Option<String>) {
        self.deposits = deposits;
        self.tip = tip;
    }

    /// Hand the composer what to say when there is nothing to search.
    ///
    /// # Why the composer is handed this rather than asking
    ///
    /// "The fast tier holds no entry at all" is the host's knowledge and not
    /// this crate's: whether a Nuclear Notes token exists, whether one has been
    /// used, and which workspace is attached are all `zaru-cli`'s, and
    /// [`Entries`] deliberately answers one question — what matches a prefix.
    /// So the line crosses as a value, exactly as [ADR-0002] D8's standing tip
    /// does and for the same reason, and the composer decides only where it
    /// goes.
    ///
    /// `Some` means the tier has nothing to offer and this is why; `None` means
    /// it has something, and a prefix that matches nothing is then an ordinary
    /// miss rather than an absence. **They are different sentences to a user**
    /// and a surface that could not tell them apart would have to guess.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    pub fn set_absence(&mut self, absence: Option<String>) {
        self.absence = absence;
    }

    /// Apply one keystroke at `now`, and refresh the fast tier.
    ///
    /// The trie is consulted here, on every keystroke that leaves something to
    /// match — D3's tier one is "instant" and is not behind the debounce. The
    /// slow tier is not consulted here at all.
    pub fn key(&mut self, input: Input, now: Duration, entries: &dyn Entries) {
        self.input.input(input);
        self.last_edit = now;

        let intent = self.intent();
        match &intent {
            // ADR-0015 D2 decides this before the strip sees the keystroke: a
            // leading `/` is a command, and a command is not a search.
            Intent::Empty | Intent::Command => self.matches.clear(),
            Intent::Picker { kind, filter } => {
                self.matches = entries
                    .matches(filter, MATCH_LIMIT)
                    .into_iter()
                    .filter(|entry| kind.admits(entry.kind))
                    .collect();
            }
            Intent::Query { query, .. } => {
                self.matches = entries.matches(query, MATCH_LIMIT);
            }
        }

        // A changed query invalidates whatever the slow tier said about the
        // old one, and re-arms the debounce for the new one.
        let asking_for = match &intent {
            Intent::Query { query, .. } => Some(query.as_str()),
            Intent::Empty | Intent::Command | Intent::Picker { .. } => None,
        };
        if self.requested.as_deref() != asking_for {
            self.requested = None;
            self.server = None;
            self.search = SearchState::Idle;
        }
    }

    /// Emit a search request if one is due at `now`, and at most one per query.
    ///
    /// Returns `None` in every other case, and the cases are the record's: the
    /// prompt is empty, a picker is open, the query is below D3's floor, the
    /// debounce has not elapsed, or a request for this exact query has already
    /// been emitted. A keystroke inside the window moves `last_edit`, so the
    /// window restarts rather than a second request queuing behind the first.
    pub fn step(&mut self, now: Duration) -> Option<SearchRequest> {
        let Intent::Query { query, tag } = self.intent() else {
            return None;
        };
        if self.requested.as_deref() == Some(query.as_str()) {
            return None;
        }
        if now.saturating_sub(self.last_edit) < DEBOUNCE {
            return None;
        }
        let request = SearchRequest::new(&query, self.scope, tag.as_deref()).ok()?;
        self.requested = Some(query);
        self.search = SearchState::Awaiting;
        Some(request)
    }

    /// Take a response the host fetched for the request it was handed.
    ///
    /// **The duty of matching a response to the query now in the input is the
    /// caller's.** Nothing crosses this boundary that identifies which request
    /// a response answers, so a host that fetches asynchronously discards a
    /// response whose query has since changed rather than delivering it. Said
    /// as a duty on the caller because that is something a reader can check
    /// against the surface in front of them.
    pub fn deliver(&mut self, response: SearchResponse) {
        self.search = SearchState::Returned {
            semantic_available: response.semantic_available,
        };
        self.server = Some(response.results);
    }

    /// What the strip is showing. A pure function of composer state, per D1.
    #[must_use]
    pub fn strip(&self) -> StripContent {
        match self.intent() {
            Intent::Empty => {
                // D1: "Deposits outrank tips. A deposit exists because the
                // user armed something; a tip exists because Zaru chose to
                // offer it. User intent wins."
                if self.deposits > 0 {
                    StripContent::Deposits {
                        count: self.deposits,
                    }
                } else if let Some(text) = &self.tip {
                    StripContent::Tip { text: text.clone() }
                } else {
                    StripContent::Collapsed
                }
            }
            Intent::Command => StripContent::Command,
            Intent::Picker { kind, filter } => StripContent::Picker {
                kind,
                filter,
                matches: self.matches.clone(),
            },
            Intent::Query { query, .. } => {
                if query.chars().count() < MIN_QUERY_CHARS {
                    StripContent::Trie {
                        matches: self.matches.clone(),
                    }
                } else {
                    StripContent::Merged {
                        entries: merge(&self.matches, self.server.as_deref().unwrap_or(&[])),
                        search: self.search,
                    }
                }
            }
        }
    }

    /// What the text and the cursor are asking for.
    ///
    /// The cursor token is the run of non-whitespace characters ending at the
    /// cursor. ADR-0005 D4 gives `[[` and `@` their meanings; `#` is not a
    /// picker here, because D4 has it "scope the live search" and attach
    /// nothing, and D1 row 6 names only the other two.
    fn intent(&self) -> Intent {
        let lines = self.input.lines();
        let text = lines.join("\n");
        if text.trim().is_empty() {
            return Intent::Empty;
        }

        // ADR-0015 D2, inside a session: "a leading `/` says command and
        // everything else is the task". The grammar decides before anything
        // else looks at the text, so a picker sigil inside a command line opens
        // nothing either — the line is not a search and no part of it is.
        if text.trim_start().starts_with('/') {
            return Intent::Command;
        }

        let (row, col) = self.input.cursor();
        let line = lines.get(row).map_or("", String::as_str);
        let before: String = line.chars().take(col).collect();
        let token: String = {
            let tail: Vec<char> = before
                .chars()
                .rev()
                .take_while(|c| !c.is_whitespace())
                .collect();
            tail.into_iter().rev().collect()
        };

        if let Some(filter) = token.strip_prefix("[[") {
            return Intent::Picker {
                kind: PickerKind::PagesAndAtoms,
                filter: filter.to_owned(),
            };
        }
        if let Some(filter) = token.strip_prefix('@') {
            return Intent::Picker {
                kind: PickerKind::Atoms,
                filter: filter.to_owned(),
            };
        }

        let mut tag = None;
        let mut words = Vec::new();
        for word in text.split_whitespace() {
            match word.strip_prefix('#') {
                Some(rest) if tag.is_none() && !rest.is_empty() => tag = Some(rest.to_owned()),
                _ => words.push(word),
            }
        }
        Intent::Query {
            query: words.join(" "),
            tag,
        }
    }
}

/// Merge what the two tiers returned, keyed on entity identity.
///
/// Local trie matches first, in the trie's order; server-only results
/// appended, in the server's order; an entity present in both shown once, in
/// the trie group. Deterministic, so a strip that has just received results
/// does not reorder under the user's eye.
///
/// The trie's copy wins because it is the one the user has already seen: the
/// trie answered instantly and the server answered 250 ms later, and replacing
/// a row's text at that moment is the reflow D2 forbids, one dimension over.
fn merge(trie: &[Entry], server: &[Entry]) -> Vec<Entry> {
    let mut merged = trie.to_vec();
    for result in server {
        if !merged
            .iter()
            .any(|entry| entry.identity() == result.identity())
        {
            merged.push(result.clone());
        }
    }
    merged
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests {
    use super::fixtures::{CountingTrie, SERVER_NONCE, TRIE_NONCE, TrieOf, server_results, typing};
    use super::{Composer, DEBOUNCE, PickerKind, Scope, SearchResponse, StripContent, StripMode};
    use core::time::Duration;

    /// One millisecond either side of the debounce, and the debounce itself.
    const JUST_UNDER: Duration = Duration::from_millis(249);
    const EXACTLY: Duration = Duration::from_millis(250);
    const JUST_OVER: Duration = Duration::from_millis(251);

    /// ADR-0005 D1 row 4 and D3's floor. Under three characters the slow tier
    /// is not reached, and this asserts it from the mechanism: the count of
    /// requests the composer emitted, taken from this test's own tally rather
    /// than from anything the composer says about itself.
    #[test]
    fn two_characters_emit_no_search_request() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "éd", Duration::ZERO, &trie);

        let mut emitted = 0;
        for millis in [0, 249, 250, 1_000, 10_000] {
            if composer.step(Duration::from_millis(millis)).is_some() {
                emitted += 1;
            }
        }
        assert_eq!(
            emitted, 0,
            "\"éd\" is two characters, which is below the floor, so no request may be emitted \
             however long the host waits"
        );
        assert!(
            matches!(composer.strip(), StripContent::Trie { .. }),
            "under the floor the strip shows trie matches only; it showed {:?}",
            composer.strip()
        );
    }

    /// ADR-0005 D3's 250 ms, measured in a clock this test owns. The boundary
    /// is asserted on both sides, so a `>` written for a `>=` reddens; a check
    /// that only stepped a second later could not separate the two.
    #[test]
    fn three_characters_emit_one_request_and_only_after_the_debounce() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édi", Duration::ZERO, &trie);

        assert!(
            composer.step(JUST_UNDER).is_none(),
            "at {JUST_UNDER:?} the debounce has not elapsed and no request is due"
        );
        let request = composer
            .step(EXACTLY)
            .expect("at exactly the debounce the request is due");
        assert_eq!(request.query(), "édi");
        assert_eq!(request.scope(), Scope::Workspace);
        assert!(
            composer.step(JUST_OVER).is_none(),
            "one query emits one request; stepping again must not emit a second"
        );
        assert!(
            composer.step(Duration::from_secs(10)).is_none(),
            "and still not a second one much later"
        );
    }

    /// ADR-0005 D3. A keystroke inside the window restarts it. The failure
    /// this prevents is a request per keystroke, which is what the debounce
    /// exists to collapse.
    #[test]
    fn a_keystroke_inside_the_window_restarts_it_rather_than_queuing_a_second() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édi", Duration::ZERO, &trie);
        typing(&mut composer, "t", Duration::from_millis(200), &trie);

        assert!(
            composer.step(Duration::from_millis(400)).is_none(),
            "the last edit was at 200 ms, so at 400 ms only 200 ms has passed and nothing is due"
        );
        let request = composer
            .step(Duration::from_millis(450))
            .expect("250 ms after the last edit the request is due");
        assert_eq!(
            request.query(),
            "édit",
            "the request carries the query as it stood at the last edit, not as it stood when \
             the window first opened"
        );
    }

    /// ADR-0005 D3's two tiers, separated. The trie's call count and the
    /// request count are two readings of two different mechanisms and are
    /// asserted apart, so a mutation routing one through the other moves
    /// exactly one number.
    #[test]
    fn every_keystroke_consults_the_trie_and_none_of_them_consults_the_search() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "éd", Duration::ZERO, &trie);

        assert_eq!(
            trie.calls(),
            2,
            "the fast tier is instant and is not behind the debounce, so two keystrokes are two \
             consultations"
        );
        assert!(
            composer.step(Duration::from_secs(10)).is_none(),
            "and neither keystroke reached the slow tier"
        );
    }

    /// ADR-0005 D4: `#` "scopes the live search; attaches nothing". It opens
    /// no picker, so the mode does not change.
    #[test]
    fn a_hash_scopes_the_search_and_changes_no_mode() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édit #zaru", Duration::ZERO, &trie);

        // The mode clauses come first and nothing returns before them. A
        // clause that early-returns makes every later clause unwatchable by
        // the same mutation, and `#` opening a picker is exactly a mutation
        // that would have stopped the request arm below from being reached.
        assert!(
            !matches!(composer.strip(), StripContent::Picker { .. }),
            "`#` opened a picker, which D1 row 6 gives only to `[[` and `@`: {:?}",
            composer.strip()
        );
        assert_eq!(
            composer.strip().mode(),
            StripMode::Typing,
            "`#` attaches nothing and opens nothing, so the strip stays in the typing mode"
        );

        let request = composer
            .step(EXACTLY)
            .expect("the query is above the floor and the debounce has elapsed");
        assert_eq!(
            request.tag(),
            Some("zaru"),
            "the tag rides on the request, which is what scoping the live search means"
        );
        assert_eq!(
            request.query(),
            "édit",
            "the tag is lifted out of the query rather than searched for"
        );
    }

    /// ADR-0005 D4. The two pickers mean different things and offer different
    /// things: `[[` is a citation over pages and atoms, `@` a transclusion
    /// over atoms. The staged trie returns one of each kind, so a picker that
    /// offered everything would be visible here.
    #[test]
    fn a_double_bracket_and_an_at_open_different_pickers() {
        let trie = CountingTrie::staged();

        let mut citing = Composer::new();
        typing(&mut citing, "[[é", Duration::ZERO, &trie);
        let StripContent::Picker {
            kind,
            filter,
            matches,
        } = citing.strip()
        else {
            panic!(
                "`[[` did not open a picker; the strip was {:?}",
                citing.strip()
            );
        };
        assert_eq!(kind, PickerKind::PagesAndAtoms);
        assert_eq!(filter, "é");
        assert_eq!(
            matches.len(),
            2,
            "`[[` offers pages and atoms, and the staged trie holds one of each: {matches:?}"
        );

        let mut transcluding = Composer::new();
        typing(&mut transcluding, "@é", Duration::ZERO, &trie);
        let StripContent::Picker {
            kind,
            filter,
            matches,
        } = transcluding.strip()
        else {
            panic!(
                "`@` did not open a picker; the strip was {:?}",
                transcluding.strip()
            );
        };
        assert_eq!(kind, PickerKind::Atoms);
        assert_eq!(filter, "é");
        assert_eq!(
            matches.len(),
            1,
            "`@` offers atoms alone, and the staged trie holds one atom and one page: {matches:?}"
        );
        assert!(
            matches
                .iter()
                .all(|entry| entry.kind == super::EntryKind::Atom),
            "a page reached the `@` picker: {matches:?}"
        );
    }

    /// ADR-0005 D1 row 6 names no server results, and D3 describes the slow
    /// tier for the typing rows. An open picker is explicit and local.
    #[test]
    fn an_open_picker_emits_no_search_request() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "[[édit", Duration::ZERO, &trie);

        assert!(
            composer.step(Duration::from_secs(10)).is_none(),
            "a picker is explicit and local, so no request is emitted however long the host waits"
        );
    }

    /// ADR-0005 D1 row 5, under the ruling of 2026-09-04: trie matches first
    /// in trie order, server-only results appended in server order, an entity
    /// in both shown once in the trie group.
    ///
    /// The two fixtures carry disjoint nonces and overlap by exactly one
    /// entity, so an implementation that appended blindly, that preferred the
    /// server's copy, or that rendered one tier while claiming the other,
    /// cannot pass. A fixture whose two sets agreed would separate none of
    /// those.
    #[test]
    fn an_entity_in_both_the_trie_and_the_server_appears_once_in_the_trie_group() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édi", Duration::ZERO, &trie);
        composer.step(EXACTLY).expect("the request is due");
        composer.deliver(SearchResponse {
            results: server_results(),
            semantic_available: true,
        });

        let StripContent::Merged { entries, .. } = composer.strip() else {
            panic!("the strip was {:?}", composer.strip());
        };

        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "architecture/bounded-contexts",
                "atoms/membrane",
                "operations/testing"
            ],
            "two trie matches in trie order, then the one server-only result"
        );
        assert!(
            entries[1].title.contains(TRIE_NONCE),
            "the entity in both tiers must carry the trie's copy, which the user has already \
             seen; it carried {:?}",
            entries[1].title
        );
        assert!(
            !entries[1].title.contains(SERVER_NONCE),
            "the server's copy replaced the trie's for the shared entity: {:?}",
            entries[1].title
        );
        assert!(
            entries[2].title.contains(SERVER_NONCE),
            "the server-only result must be the server's copy: {:?}",
            entries[2].title
        );
    }

    /// D1: "Deposits outrank tips. A deposit exists because the user armed
    /// something; a tip exists because Zaru chose to offer it."
    #[test]
    fn a_deposit_outranks_a_standing_tip_when_both_are_present() {
        let mut composer = Composer::new();
        composer.set_standing(3, Some(format!("a tip carrying {TRIE_NONCE}")));
        assert_eq!(
            composer.strip(),
            StripContent::Deposits { count: 3 },
            "user intent wins"
        );

        composer.set_standing(0, Some(format!("a tip carrying {TRIE_NONCE}")));
        assert!(
            matches!(composer.strip(), StripContent::Tip { .. }),
            "with no deposits the tip is what the strip has to show: {:?}",
            composer.strip()
        );

        composer.set_standing(0, None);
        assert_eq!(
            composer.strip(),
            StripContent::Collapsed,
            "empty, neither: D1 row 3 collapses the strip"
        );
    }

    /// ADR-0015 D2, inside a session: "a leading `/` says command and
    /// everything else is the task". **The grammar decides before the strip
    /// sees a keystroke**, so a command line never reaches the fast tier.
    ///
    /// The count is the discriminating arm and it is taken from the trie's own
    /// tally rather than from anything the composer reports about itself. The
    /// accepting sibling is the same word without the slash: without it, a
    /// composer that had simply stopped consulting the trie would pass.
    #[test]
    fn a_slash_prefixed_line_never_reaches_the_trie_and_the_same_word_does() {
        let commanded = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "/runtime", Duration::ZERO, &commanded);
        assert_eq!(
            commanded.calls(),
            0,
            "`/runtime` is a command and eight keystrokes of it reached the trie {} time(s);              ADR-0015 D2's grammar decides before the strip sees a keystroke",
            commanded.calls()
        );
        assert_eq!(
            composer.strip(),
            StripContent::Command,
            "a command line renders nothing, and the state says so rather than pretending the              trie returned no matches"
        );
        assert_eq!(
            composer.strip().mode(),
            StripMode::Typing,
            "the prompt is not empty, and D1's two modes are keyed on exactly that"
        );
        assert!(
            composer.strip_lines().is_empty(),
            "and it paints no rows: {:?}",
            composer.strip_lines()
        );

        let searched = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "runtime", Duration::ZERO, &searched);
        assert_eq!(
            searched.calls(),
            7,
            "the same word without the slash is a search, and every keystroke of it consults the              trie"
        );
    }

    /// A picker sigil inside a command line opens nothing, because the line is
    /// not a search and no part of it is.
    ///
    /// Staged with the sigil in the middle rather than at either end, so that
    /// "the line begins with a slash" and "the token at the cursor begins with
    /// a slash" cannot give the same answer.
    #[test]
    fn a_picker_sigil_inside_a_command_line_opens_no_picker() {
        // Two stagings, because they fail differently. In the first the
        // sigil's token is AT THE CURSOR, which is the only position that
        // would open a picker at all -- so that is the case separating "the
        // line decides" from "the token at the cursor decides". In the second
        // the sigil is behind the cursor, where without the leading `/` the
        // line would be an ordinary query.
        for line in ["/notes [[é", "/notes [[é workspace"] {
            let trie = CountingTrie::staged();
            let mut composer = Composer::new();
            typing(&mut composer, line, Duration::ZERO, &trie);

            assert_eq!(
                composer.strip(),
                StripContent::Command,
                "{line:?} begins with `/`, so ADR-0015 D2 has already decided it is a command \
                 and no part of it is a search; the strip was {:?}",
                composer.strip()
            );
            assert_eq!(
                trie.calls(),
                0,
                "and nothing in {line:?} reached the trie; it was consulted {} time(s)",
                trie.calls()
            );
        }
    }

    /// The absence line is shown when there is nothing else to show, and never
    /// instead of something.
    ///
    /// Three states in one check because the line's whole job is to be
    /// distinguishable from the other two: a match hides it, a miss shows it,
    /// and a host that handed nothing in leaves the strip as it was.
    #[test]
    fn the_absence_line_appears_only_when_there_is_nothing_else_to_show() {
        const ABSENCE: &str = "no Nuclear Notes token · nothing to search";

        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        typing(&mut composer, "éd", Duration::ZERO, &empty);
        assert_eq!(
            composer.strip_lines(),
            vec![ABSENCE.to_owned()],
            "with nothing to show and a line handed in, the strip says why rather than going blank"
        );

        let populated = TrieOf::new(2);
        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        typing(&mut composer, "éd", Duration::ZERO, &populated);
        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            2,
            "matches are what the strip is for: {lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line == ABSENCE),
            "the absence line appeared beside matches, which is the one thing it must never do:              {lines:?}"
        );

        let mut composer = Composer::new();
        typing(&mut composer, "éd", Duration::ZERO, &empty);
        assert!(
            composer.strip_lines().is_empty(),
            "with no line handed in there is nothing to say, and the composer invents nothing:              {:?}",
            composer.strip_lines()
        );
    }

    /// The absence line never reaches the empty prompt or a picker.
    ///
    /// D1 rows 1 to 3 are the standing tip's and the deposit count's, and
    /// ADR-0002 D8's tip must not be crowded out by a line about search. An
    /// open picker with no matches is what a miss looks like.
    #[test]
    fn the_absence_line_yields_the_empty_prompt_to_the_tip_and_never_enters_a_picker() {
        const ABSENCE: &str = "no Nuclear Notes token · nothing to search";
        let empty = TrieOf::new(0);

        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        composer.set_standing(0, Some(format!("a tip carrying {TRIE_NONCE}")));
        let lines = composer.strip_lines();
        assert_eq!(lines.len(), 1, "one line on an empty prompt: {lines:?}");
        assert!(
            lines[0].contains(TRIE_NONCE),
            "the tip owns the empty prompt and the absence line took it: {lines:?}"
        );

        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        composer.set_standing(0, None);
        assert_eq!(
            composer.strip(),
            StripContent::Collapsed,
            "an empty prompt with neither still collapses; the absence line is a typing-mode row"
        );
        assert!(composer.strip_lines().is_empty());

        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        typing(&mut composer, "[[é", Duration::ZERO, &empty);
        assert!(
            composer.strip_lines().is_empty(),
            "an open picker with no matches is a miss, not an absence: {:?}",
            composer.strip_lines()
        );
    }

    /// The debounce is spent in the caller's clock and nowhere else. The
    /// composer holds no clock at all, so this asserts the consequence: the
    /// same keystrokes stepped at the same `Duration` values behave the same
    /// however long the machine actually took.
    #[test]
    fn the_debounce_is_spent_in_the_clock_the_caller_passes() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édi", Duration::from_secs(86_400), &trie);

        assert!(
            composer
                .step(Duration::from_secs(86_400) + JUST_UNDER)
                .is_none(),
            "the window is measured from the last edit, not from any origin"
        );
        assert!(
            composer
                .step(Duration::from_secs(86_400) + DEBOUNCE)
                .is_some(),
            "and it elapses exactly one debounce after it"
        );
    }
}
