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
pub use render::{KEYWORD_ONLY, continues};
pub use search::{
    DEBOUNCE, MIN_QUERY_CHARS, RequestRefused, Scope, SearchRequest, SearchResponse, SearchState,
};
pub use strip::{PickerKind, StripContent, StripMode};

use crate::shell::port::{CommandVocabulary, Namespace};
use core::cell::Cell;
use core::time::Duration;
use tui_textarea::{Input, Key, TextArea};

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

/// What a newline in the prompt paints as, in the one row [ADR-0005] D2 fixes.
///
/// U+23CE, RETURN SYMBOL. **Drafted under a delegated coordinator ruling of
/// 2026-09-06 and 2026-09-13, open to Jeshua's veto**, in the same shape as
/// the six register glyphs and `STRIP_ROWS`: no record names a glyph for a
/// newline and one is needed, so it is named once here with its reasoning
/// rather than typed at a call site. It is recorded on
/// [ADR-0005's amendments page].
///
/// # It is a rendering and never a storage form
///
/// [`Composer::text`] returns the bytes as they were typed or pasted, with
/// real newlines, and a pasted U+23CE is stored and submitted as U+23CE.
/// Substituting at the *storage* end would have been the smaller change and
/// is refused: a person pasting a document that contains the return symbol
/// would have it silently become a line break, and a harness that alters a
/// person's own bytes as they type them is the opposite of showing them their
/// work — the argument [ADR-0010] D2's 2026-09-06 Update already makes for
/// painting a typed line raw.
///
/// **One column wide, and that is asserted rather than assumed.** Its East
/// Asian Width is Neutral, so the `unicode-width` measurement `ratatui` paints
/// with gives it one; `the_newline_marker_occupies_one_column` pins the
/// premise, exactly as `every_register_glyph_occupies_one_column` pins it for
/// the registers, so a wider glyph reddens a check rather than skewing a row.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0005's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
pub const NEWLINE: &str = "\u{23ce}";

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
    /// `picking` carries the namespace word being typed — the first word of
    /// the line, its `/` included — while the line is still that word alone.
    /// **Once the line carries any whitespace the picker closes**: the
    /// namespace has been named, and a picker over a namespace's verbs is a
    /// decision no record makes.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Command {
        /// The word being typed, or `None` once the line carries whitespace.
        picking: Option<String>,
    },
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
    /// The command namespaces the word being typed reaches, in [ADR-0015] D2's
    /// table order, or empty when the line is not a command line being typed.
    ///
    /// Cached here on the keystroke exactly as [`Composer::matches`] is, so
    /// that [`Composer::strip`] stays a pure function of composer state, which
    /// is ADR-0005 D1's own clause. **It is a narrowing of what the vocabulary
    /// answered on this keystroke and never a copy of the vocabulary**, so it
    /// cannot go stale against the table the same line's `Enter` dispatches
    /// against.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    commands: Vec<Namespace>,
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
    /// Where the one painted row's window starts, in display columns.
    ///
    /// # It is sticky, and that is `tui-textarea`'s rule kept deliberately
    ///
    /// The widget this crate used to hand the input row to keeps its viewport
    /// in an `AtomicU64` and moves it only when the caret would leave it:
    /// left of the window the window follows the caret, past its right edge it
    /// tracks the caret, and anywhere inside it the window does not move at
    /// all. Recomputing the window from zero on every paint instead reads
    /// simpler and is not the same thing — a caret moved five columns left
    /// inside a long line would drag the whole row five columns with it, where
    /// today it does not move at all. So the state is kept, in a [`Cell`] for
    /// the reason the widget kept it behind interior mutability: painting
    /// takes `&self`.
    window: Cell<usize>,
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
            commands: Vec::new(),
            server: None,
            search: SearchState::Idle,
            absence: None,
            last_edit: Duration::ZERO,
            requested: None,
            window: Cell::new(0),
        }
    }

    /// The text the user has composed.
    #[must_use]
    pub fn text(&self) -> String {
        self.input.lines().join("\n")
    }

    /// Where the cursor is, as a row and a column in characters.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.input.cursor()
    }

    /// The one row the input paints at `width`, and the cursor's column in it.
    ///
    /// # Why the composer paints its own row, and what that cost
    ///
    /// Until 2026-09-13 [`Self::render`] handed the `tui-textarea` widget a
    /// one-row area and let it scroll its own viewport to keep the cursor
    /// visible. **A widget has no way to paint a newline as a glyph**, and
    /// with several lines in it that one paints only the cursor's line with
    /// nothing on the screen saying the others exist — so with a pasted block
    /// in the prompt it would show one line of it and hide the rest. The row
    /// is therefore composed here.
    ///
    /// **The cost is that the horizontal windowing is this crate's now**, and
    /// it is named on [ADR-0005's amendments page] rather than discovered
    /// later. Two painters — one for plain lines and one for blocks — were
    /// refused: two renderings of one text are two things that can disagree
    /// about a character, which is the argument [`Shell::stream_delta`]
    /// already carries for the streamed answer. What pins the cost is a
    /// byte-identity check over today's single-line frames at several widths
    /// and cursor positions.
    ///
    /// # The window follows the cursor, which is what "the block's tail" means
    ///
    /// The row is the widest `width`-column window that keeps the caret on
    /// screen, computed exactly as the widget computed it: the window starts
    /// at zero until the caret would fall off the right edge, and then tracks
    /// it. With the caret at the end of a pasted block that is the block's
    /// **visible tail**; with the caret moved left by an arrow key it is
    /// wherever the caret is.
    ///
    /// **The returned column is a column and never a row.** A prompt of three
    /// lines has a caret somewhere in one composed row, so [`Self::render`]
    /// has nothing to add to the input area's `y` and the caret cannot land
    /// on the strip.
    ///
    /// [`Shell::stream_delta`]: crate::shell::Shell::stream_delta
    /// [ADR-0005's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
    #[must_use]
    pub fn input_row(&self, width: u16) -> (String, u16) {
        let budget = usize::from(width).max(1);
        let lines = self.input.lines();
        let (row, column) = self.cursor();

        // The caret's column in the composed row: every earlier line and the
        // marker that stands for the newline after it, then the part of the
        // cursor's own line that precedes it.
        let marker = crate::shell::wrap::columns(NEWLINE);
        let mut caret = 0_usize;
        for line in lines.iter().take(row) {
            caret += crate::shell::wrap::columns(line) + marker;
        }
        if let Some(line) = lines.get(row) {
            let before: String = line.chars().take(column).collect();
            caret += crate::shell::wrap::columns(&before);
        }

        let display = lines.join(NEWLINE);
        // `tui_textarea`'s `next_scroll_top`, transcribed: left of the window
        // the window follows the caret, past its right edge it tracks the
        // caret, and anywhere inside it the window does not move. See the
        // `window` field for why the state is kept rather than recomputed.
        let was = self.window.get();
        let start = if caret < was {
            caret
        } else if was + budget <= caret {
            caret + 1 - budget
        } else {
            was
        };
        self.window.set(start);
        (
            columns_of(&display, start, budget),
            u16::try_from(caret.saturating_sub(start)).unwrap_or(u16::MAX),
        )
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

    /// Apply one keystroke at `now`, and refresh both corpora.
    ///
    /// The trie is consulted here, on every keystroke that leaves something to
    /// match — D3's tier one is "instant" and is not behind the debounce. The
    /// slow tier is not consulted here at all. The vocabulary is consulted
    /// here for the same reason the trie is, and on a command line **instead**
    /// of it.
    ///
    /// # `Tab` on a command line is absorbed, and that is the whole rule
    ///
    /// [ADR-0005]'s amendment of 2026-09-15: where the word being typed
    /// reaches exactly one namespace and is not already its whole spelling,
    /// `Tab` replaces the word with **the vocabulary's own spelling**; where it
    /// reaches none, reaches several, or is a bare `/`, `Tab` does nothing at
    /// all. Either way the key never reaches the text area, which is what
    /// makes "does nothing" true rather than nearly true: `tui-textarea`'s own
    /// `Key::Tab` arm calls `insert_tab`, so a fall-through would advance the
    /// caret to the next tab stop — three spaces after a bare `/`, measured on
    /// the release binary at `a8eedf7`. Outside a command line the key falls
    /// through unchanged and still inserts those spaces, which is today's
    /// behaviour and which no record names.
    ///
    /// **A command line past its namespace absorbs `Tab` too**, and that is
    /// this rule reaching one case the proposal left to the fall-through.
    /// `/se list` has no prefix left to complete, so the ruling's "does
    /// nothing when the prefix is ambiguous or empty" applies to it as much as
    /// to a bare `/` — and the alternative, four spaces pushed into a line
    /// whose parser splits on whitespace, is inert noise a person can see.
    /// Found by the fifth arm of
    /// `tab_completes_a_unique_command_prefix_and_otherwise_inserts_nothing`,
    /// which asserted the rule the proposal stated while the code implemented
    /// the narrower one.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates
    pub fn key(
        &mut self,
        input: Input,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) {
        if input.key == Key::Tab
            && !input.ctrl
            && !input.alt
            && let Intent::Command { picking } = self.intent()
        {
            if let Some(filter) = picking {
                self.complete(&filter);
            }
            self.refreshed(now, entries, vocabulary);
            return;
        }
        self.input.input(input);
        self.refreshed(now, entries, vocabulary);
    }

    /// Replace the word being typed with the one namespace it reaches.
    ///
    /// Does nothing unless [`Composer::commands`] holds exactly one namespace
    /// and that namespace's spelling is not already what was typed — the slice
    /// pattern is the uniqueness rule, so "several matched" and "none matched"
    /// cannot be told apart from inside this function and do not need to be.
    ///
    /// Any whitespace before the word is kept, because `picking` is `Some`
    /// only while the trimmed line holds none of its own, so the prompt is
    /// that leading run followed by the word and nothing else.
    fn complete(&mut self, filter: &str) {
        let [namespace] = self.commands.as_slice() else {
            return;
        };
        if namespace.slash == filter {
            return;
        }
        let spelling = namespace.slash;
        let text = self.text();
        let leading: String = text.chars().take_while(|c| c.is_whitespace()).collect();
        let mut input = TextArea::default();
        input.insert_str(format!("{leading}{spelling}"));
        self.input = input;
        // The window is sticky by design (see the field), and the text under
        // it has just been replaced wholesale, so the only honest starting
        // point is the left edge.
        self.window.set(0);
    }

    /// Insert a pasted block at `now`, newlines and all, and refresh the same
    /// tier a keystroke refreshes.
    ///
    /// # A pasted newline is text, and that is [ADR-0005]'s 2026-09-13 Update
    ///
    /// Until bracketed paste was armed, every newline in a paste arrived as
    /// `Enter` and was read as a submission, so pasting three lines ran two
    /// turns and left the third in the prompt — the look-and-feel survey's
    /// row 13. The terminal now hands a paste over whole and it lands here as
    /// **text**, so `Enter` submits the block as one prompt.
    ///
    /// **It refreshes through the same body [`Self::key`] does**, so a paste
    /// and a keystroke cannot disagree about what the strip shows: a pasted
    /// leading `/` is [ADR-0015] D2's command line exactly as a typed one is,
    /// and a pasted three characters reach the fast tier exactly as three
    /// typed ones do. One rule, one place, which is what stops the strip
    /// having two behaviours keyed on how the text arrived.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub fn paste(
        &mut self,
        text: &str,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) {
        self.input.insert_str(text);
        self.refreshed(now, entries, vocabulary);
    }

    /// Mark the text edited at `now` and re-ask the fast tier.
    ///
    /// The body [`Self::key`] and [`Self::paste`] share. It was `key`'s whole
    /// tail until a paste needed it, and it is extracted rather than copied
    /// for the reason this workspace keeps giving: a rule spelled at two call
    /// sites is a rule nothing keeps agreeing.
    fn refreshed(
        &mut self,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) {
        self.last_edit = now;

        let intent = self.intent();
        // The second corpus, and it is filled on exactly the keystrokes the
        // first one is not. A command line narrows the vocabulary by prefix —
        // never by nearest, which is the refusal's job — and everything else
        // leaves it empty.
        self.commands = match &intent {
            Intent::Command {
                picking: Some(filter),
            } => vocabulary
                .namespaces()
                .into_iter()
                .filter(|namespace| namespace.slash.starts_with(filter.as_str()))
                .collect(),
            Intent::Command { picking: None }
            | Intent::Empty
            | Intent::Picker { .. }
            | Intent::Query { .. } => Vec::new(),
        };
        match &intent {
            // ADR-0015 D2 decides this before the strip sees the keystroke: a
            // leading `/` is a command, and a command is not a search.
            Intent::Empty | Intent::Command { .. } => self.matches.clear(),
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
            Intent::Empty | Intent::Command { .. } | Intent::Picker { .. } => None,
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
            // The picker's rows page by prefix rather than scroll: where the
            // narrowed set does not fit the strip's rows, the last row says how
            // many are not shown rather than dropping them silently. The
            // budget is the shell's own `STRIP_ROWS` read here rather than a
            // second number, so the strip cannot be handed more rows than it
            // paints — which is the defect this surface is deliberately not
            // reproducing.
            Intent::Command { .. } => {
                let rows = usize::from(crate::shell::STRIP_ROWS);
                if self.commands.len() <= rows {
                    StripContent::Command {
                        matches: self.commands.clone(),
                        beyond: 0,
                    }
                } else {
                    let shown = rows.saturating_sub(1);
                    StripContent::Command {
                        matches: self.commands[..shown].to_vec(),
                        beyond: self.commands.len() - shown,
                    }
                }
            }
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
        let leading = text.trim_start();
        if leading.starts_with('/') {
            // The picker is open only while the line is the namespace word and
            // nothing else. A trailing space closes it as surely as a verb
            // does, and a pasted newline counts as whitespace for the same
            // reason: the word has been finished either way.
            let picking = (!leading.contains(char::is_whitespace)).then(|| leading.to_owned());
            return Intent::Command { picking };
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

/// The window of `text` from display column `start`, `budget` columns wide.
///
/// Measured with [`crate::shell::wrap::columns`], which is the same
/// `unicode-width` measurement the terminal's own buffer paints with — a
/// second measurement would disagree with the buffer about a wide character
/// and put the caret a column out.
///
/// **A character that straddles either edge is dropped rather than split.**
/// Half a wide character is not a character, and a buffer handed one paints a
/// replacement the reader cannot make sense of; dropping it loses one column
/// of a row that is already a window onto something longer.
fn columns_of(text: &str, start: usize, budget: usize) -> String {
    let mut at = 0_usize;
    let mut taken = 0_usize;
    let mut window = String::new();
    for character in text.chars() {
        let mut buffer = [0_u8; 4];
        let wide = crate::shell::wrap::columns(character.encode_utf8(&mut buffer));
        if at < start {
            // Before the window, or straddling its left edge.
            at += wide;
            continue;
        }
        if taken + wide > budget {
            break;
        }
        window.push(character);
        taken += wide;
        at += wide;
    }
    window
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
    use super::fixtures::{
        CountingTrie, SERVER_NONCE, TRIE_NONCE, TrieOf, press, server_results, typing,
    };
    use super::{Composer, DEBOUNCE, PickerKind, Scope, SearchResponse, StripContent, StripMode};
    use crate::shell::fixtures::StagedVocabulary;
    use crate::shell::port::CommandVocabulary;
    use core::time::Duration;
    use tui_textarea::Key;

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
        assert!(
            matches!(composer.strip(), StripContent::Command { .. }),
            "and the state says the line is a command rather than pretending the trie returned \
             no matches; it was {:?}",
            composer.strip()
        );
        assert_eq!(
            composer.strip().mode(),
            StripMode::Typing,
            "the prompt is not empty, and D1's two modes are keyed on exactly that"
        );
        assert_eq!(
            composer.strip_lines(),
            vec!["/runtime  tier and membrane".to_owned()],
            "the rows it paints are the second corpus and not the trie's; they were {:?}",
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

    /// ADR-0005's amendment of 2026-09-15: a bare `/` lists the command
    /// namespaces in D2's table order, five of them plus the line saying how
    /// many are not shown.
    ///
    /// The order is the discriminating arm: a set that happened to hold the
    /// right five in the wrong order would satisfy a membership check and tell
    /// a person the wrong thing about which command is which.
    #[test]
    fn a_bare_slash_lists_the_namespaces_in_the_records_own_table_order() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "/", Duration::ZERO, &trie);

        let lines = composer.strip_lines();
        assert_eq!(
            lines.len(),
            6,
            "the strip paints six rows and the picker may not ask for a seventh; it asked for \
             {lines:?}"
        );
        assert_eq!(
            lines[..5],
            [
                "/runtime  tier and membrane".to_owned(),
                "/stack    AEGIS component fetch and status".to_owned(),
                "/notes    Nuclear Notes tokens, workspace, search".to_owned(),
                "/config   configuration and explanation".to_owned(),
                "/memory   relationship memory".to_owned(),
            ],
            "the first five rows are D2's first five namespaces, in D2's order, each carrying \
             that record's own second column; they were {lines:?}"
        );
        assert_eq!(
            lines[5],
            crate::composer::continues(6),
            "the sixth row says how many namespaces are not shown rather than dropping them \
             silently; it read {:?}",
            lines[5]
        );
        assert_eq!(
            trie.calls(),
            0,
            "and a bare `/` reached the trie {} time(s)",
            trie.calls()
        );
    }

    /// The picker narrows by prefix, never by nearest, and closes at the first
    /// space.
    ///
    /// Four stagings, and the third is the one that separates this from a
    /// nearest match: `/xyz` names no namespace, so the picker shows nothing
    /// at all rather than offering the closest noun. ADR-0014 D5's nearest is
    /// the refusal's job and it runs on `Enter`.
    #[test]
    fn the_picker_narrows_by_prefix_and_never_by_nearest() {
        for (line, expected) in [
            ("/se", vec!["/session  resume, list, remove".to_owned()]),
            (
                "/s",
                vec![
                    "/stack    AEGIS component fetch and status".to_owned(),
                    "/session  resume, list, remove".to_owned(),
                ],
            ),
            ("/xyz", Vec::new()),
            ("/session ", Vec::new()),
        ] {
            let trie = CountingTrie::staged();
            let mut composer = Composer::new();
            typing(&mut composer, line, Duration::ZERO, &trie);
            assert_eq!(
                composer.strip_lines(),
                expected,
                "{line:?} should narrow the picker to {expected:?}; it painted {:?}",
                composer.strip_lines()
            );
        }
    }

    /// The rows are the vocabulary's own, with a liveness control.
    ///
    /// The positive arm alone cannot fail for a matcher that has widened into
    /// something universal, so the second arm asserts that a spelling the
    /// vocabulary does not carry is absent — which is the only run in which
    /// this check demonstrates it can answer "no".
    #[test]
    fn every_picker_row_is_a_namespace_the_vocabulary_carries() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "/", Duration::ZERO, &trie);

        let carried = StagedVocabulary.namespaces();
        for line in composer.strip_lines() {
            if line == crate::composer::continues(6) {
                continue;
            }
            assert!(
                carried
                    .iter()
                    .any(|namespace| line.starts_with(namespace.slash)
                        && line.ends_with(namespace.governs)),
                "the row {line:?} names no namespace the vocabulary carries"
            );
        }

        assert!(
            !composer
                .strip_lines()
                .iter()
                .any(|line| line.starts_with("/help")),
            "the picker offered `/help`, which this vocabulary does not carry — so the rows are \
             not being read from it; they were {:?}",
            composer.strip_lines()
        );
    }

    /// The `Tab` rule, asserted on the composer's own bytes in five arms.
    ///
    /// Byte for byte rather than by a rendered row, because the behaviour this
    /// replaces is **invisible in a frame**: `tui-textarea`'s `insert_tab`
    /// advances the caret to the next tab stop, and a terminal capture trims
    /// trailing spaces — which is how the look-and-feel survey read `Tab` as a
    /// no-op when it was inserting three of them after a bare `/`.
    ///
    /// The fifth arm is the accepting sibling for the closing rule: once the
    /// line carries a space the namespace has been named, so `Tab` has nothing
    /// to complete and must leave the whole line alone.
    #[test]
    fn tab_completes_a_unique_command_prefix_and_otherwise_inserts_nothing() {
        for (typed, after, why) in [
            ("/se", "/session", "a unique incomplete prefix is completed"),
            (
                "/s",
                "/s",
                "two namespaces share `/s`, so there is nothing unique to complete",
            ),
            (
                "/",
                "/",
                "a bare `/` reaches every namespace and completes none of them",
            ),
            (
                "/session",
                "/session",
                "the whole spelling is already there and there is nothing to add",
            ),
            (
                "/se list",
                "/se list",
                "the line carries a space, so the picker is closed and Tab leaves it alone",
            ),
        ] {
            let trie = CountingTrie::staged();
            let mut composer = Composer::new();
            typing(&mut composer, typed, Duration::ZERO, &trie);
            press(&mut composer, Key::Tab, &trie);
            assert_eq!(
                composer.text(),
                after,
                "{typed:?} then Tab should hold {after:?} because {why}; it holds {:?}",
                composer.text()
            );
        }
    }

    /// Outside a command line `Tab` is unchanged, and that is asserted rather
    /// than assumed.
    ///
    /// The accepting sibling for the four arms above that assert nothing was
    /// inserted: without it, an implementation that swallowed every `Tab`
    /// everywhere would pass all five.
    #[test]
    fn tab_outside_a_command_line_still_reaches_the_text_area() {
        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "édit", Duration::ZERO, &trie);
        press(&mut composer, Key::Tab, &trie);
        assert_eq!(
            composer.text(),
            "édit    ",
            "outside a command line Tab is the text area's and still advances to the next tab \
             stop; the composer holds {:?}",
            composer.text()
        );
    }

    /// The hint strip returns the instant the leading `/` goes, absence line
    /// and all.
    ///
    /// Two arms, because they fail differently: the trie's matches coming back
    /// says the tier is consulted again, and the absence line coming back says
    /// the typing rows are reached again rather than the command row being
    /// rendered empty.
    #[test]
    fn the_hint_strip_returns_the_moment_the_line_stops_being_a_command() {
        const ABSENCE: &str = "no Nuclear Notes token · nothing to search";

        let trie = CountingTrie::staged();
        let mut composer = Composer::new();
        typing(&mut composer, "/se", Duration::ZERO, &trie);
        assert_eq!(trie.calls(), 0, "a command line reaches no tier");

        for _ in 0..3 {
            press(&mut composer, Key::Backspace, &trie);
        }
        typing(&mut composer, "édi", Duration::ZERO, &trie);
        assert!(
            composer
                .strip_lines()
                .iter()
                .any(|line| line.contains(TRIE_NONCE)),
            "with the slash gone the fast tier is back on the strip; it painted {:?}",
            composer.strip_lines()
        );

        let empty = TrieOf::new(0);
        let mut composer = Composer::new();
        composer.set_absence(Some(ABSENCE.to_owned()));
        typing(&mut composer, "/se", Duration::ZERO, &empty);
        assert!(
            !composer.strip_lines().iter().any(|line| line == ABSENCE),
            "the absence line is about a corpus the picker is not showing: {:?}",
            composer.strip_lines()
        );
        for _ in 0..3 {
            press(&mut composer, Key::Backspace, &empty);
        }
        typing(&mut composer, "édi", Duration::ZERO, &empty);
        assert_eq!(
            composer.strip_lines(),
            vec![ABSENCE.to_owned()],
            "and it is back the moment the line stops being a command: {:?}",
            composer.strip_lines()
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

            assert!(
                matches!(composer.strip(), StripContent::Command { .. }),
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
