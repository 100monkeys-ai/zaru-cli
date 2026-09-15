// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! How a model's answer becomes the rows a pane of a given width paints.
//!
//! # What this module is, and what it deliberately is not
//!
//! It is a **CommonMark renderer for one region of one terminal**. It is not a
//! document viewer, it is not a browser, and it carries no HTML: the parser is
//! taken `default-features = false` precisely so that `pulldown-cmark-escape`
//! — the HTML renderer — is not in the tree at all. See the
//! `[workspace.dependencies]` comment on `pulldown-cmark` for what else that
//! turns off and why.
//!
//! **Nothing is hidden.** That is the test every decision below is made
//! against, and it is the one that separates a rendering from an elision. A
//! link paints its text *and* its destination, because the destination is the
//! part a reader cannot otherwise see. A table and an image paint their own
//! source text, because laying out columns against a width the reader can
//! change at any moment is [ADR-0005] D2's shifting surface — that clause is
//! about the composer and is not amended, but its reasoning is why a pane is
//! not a document viewer. A fenced block is indented and **uncoloured**,
//! because a highlighter needs a grammar set and a pane that colours code by a
//! grammar it shipped is asserting a language it may have guessed wrong.
//!
//! # What stops reaching the buffer, and the clause that says it may
//!
//! The delimiters. `#`, `*`, `_`, a back-tick, a fence, and a link's brackets
//! are replaced by the presentation they denoted. [ADR-0010] D2's pane
//! clause — "the terminal's transcript pane shows what the file holds,
//! unaltered" — is amended for exactly this on 2026-09-15: *no datum the file
//! holds is lost from the screen, and a markup delimiter whose presentation is
//! painted is not a datum.* The amendment quotes the `pane-text` arc's own
//! test, "no datum stops reaching the buffer", as the case it widens and
//! deliberately fails: those delimiters are not a second copy of anything.
//!
//! **The file is untouched.** Nothing here writes, and `cat transcript.jsonl`
//! still shows every character, so [ADR-0010] D5's "every byte" is unaffected.
//!
//! # The one wrap stays the one wrap
//!
//! Every run of inline text goes through [`crate::shell::wrap::rows`], the
//! same function a verbatim line uses, at a budget this module narrows by the
//! block's own indent. A second wrapper would be a second answer to "how wide
//! is a row", and the two would disagree about a wide character on the day it
//! mattered.
//!
//! Inline runs are split at hard breaks **before** they are wrapped, so every
//! string handed to that function is newline-free. That is what makes the byte
//! arithmetic below honest: the function's own contract is that concatenating
//! its rows reproduces its input, and with no newline to consume the
//! reproduction is exact, so a modifier's byte range can be carried across a
//! row boundary by subtraction rather than by a search.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript

use crate::shell::port::{Register, Row};
use core::ops::Range;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::Modifier;

/// A heading's words, in the one weight a terminal has for "this is a heading".
///
/// **Drafted under the delegated coordinator ruling of 2026-09-15, open to
/// Jeshua's veto**, in the same shape as [`Register::glyph`]'s four drafted
/// glyphs and [`crate::shell::render::MASK`]: no record names a modifier for a
/// heading and one is needed, so it is named once here rather than typed at a
/// call site. The [ADR-0028] amendment that licenses a modifier at all is the
/// one that narrows `colour-registers`' "no background colour and no modifier"
/// for CommonMark presentation.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative-updates
pub const HEADING: Modifier = Modifier::BOLD;

/// A strong span. The same weight as [`HEADING`], because a terminal has one.
pub const STRONG: Modifier = Modifier::BOLD;

/// An emphasis span.
pub const EMPHASIS: Modifier = Modifier::ITALIC;

/// Inline code.
///
/// `DIM` rather than `UNDERLINED`: `tui-textarea` 0.7 already emits `4`/`24`
/// on the composer's leading `/`, measured in the look-and-feel survey's row
/// 4, and a second meaning for one sequence on one screen is the collision the
/// `!` glyph is already recorded as carrying. `REVERSED` and every background
/// are refused outright — a background is the one styling a terminal's own
/// theme cannot be trusted to keep legible.
pub const INLINE_CODE: Modifier = Modifier::DIM;

/// What opens a bulleted list item, in the pane's own glyph column.
///
/// ASCII and one column, so it cannot skew a wrapped item's continuation
/// indent — the same property `Register::Setback`'s `!` was chosen for.
pub const BULLET: &str = "-";

/// How far a block set inside another is indented, in columns.
///
/// Two, which is the width of the pane's own glyph column, so a fenced block
/// and a quoted block line up under the text they belong to rather than at a
/// width chosen for them.
pub const BLOCK_INDENT: usize = 2;

/// What a link's destination is wrapped in, after its text.
pub const URL_OPEN: &str = " (";

/// The other half of [`URL_OPEN`].
pub const URL_CLOSE: &str = ")";

/// The rows an answer paints as, in `register`, in a pane `width` wide.
///
/// The entry point the 2026-09-15 ruling names. `register` is the line's own
/// and is never chosen here: a parser that picked a register would be deciding
/// what a line *is* from what it says, which is the conflation
/// [`Register`]'s own documentation exists to prevent.
#[must_use]
pub fn rows(answer: &str, register: Register, width: u16) -> Vec<Row> {
    rows_after("", answer, register, width)
}

/// The same, after a label the harness wrote that is **not** parsed.
///
/// Only [`crate::shell::port::Line::rows`] calls this, and only for the
/// replayed conversation line — see [`crate::shell::port::Prose::CommonMark`]
/// for why that label is kept out of the parse.
#[must_use]
pub(crate) fn rows_after(lead: &str, answer: &str, register: Register, width: u16) -> Vec<Row> {
    paint(&blocks_of(lead, answer), register, width)
}

/// One thing the pane paints on consecutive rows.
///
/// A paragraph, a heading, one list item, or one fenced block. Blocks are
/// separated by a blank row, which is what a reader already saw when an
/// answer's own newlines were painted verbatim.
#[derive(Debug, Default)]
struct Block {
    /// Columns this block sits right of the text column.
    indent: usize,
    /// What opens its first row, inside the glyph column: a list marker.
    marker: Option<String>,
    /// Its lines, already split at every hard break.
    lines: Vec<Styled>,
    /// Whether a blank row separates it from the block above.
    ///
    /// False between the items of one list, so a tight list reads as a list.
    spaced: bool,
}

/// One newline-free run of text, and where modifiers apply inside it.
#[derive(Debug, Default)]
struct Styled {
    text: String,
    spans: Vec<(Range<usize>, Modifier)>,
}

/// The walk over the parser's events, and the blocks it produces.
struct Builder<'a> {
    source: &'a str,
    blocks: Vec<Block>,
    lines: Vec<Styled>,
    current: Styled,
    /// The modifier stack; the effective one is their union.
    modifiers: Vec<Modifier>,
    indent: usize,
    /// One entry per open list: the next ordinal, or `None` for a bulleted one.
    lists: Vec<Option<u64>>,
    /// The marker the next block that opens will take.
    pending_marker: Option<String>,
    /// Whether the next block is the second or later item of one list.
    tight: bool,
    /// Depth of a construct whose own source text is painted instead of it.
    verbatim: usize,
    /// A link's destination, held until its text has been pushed.
    links: Vec<String>,
    /// The label the harness wrote, until the first text joins it.
    ///
    /// **Held rather than written into the first line at construction.** The
    /// first event a document produces is a block start, and a block start
    /// closes whatever is open — so a lead sitting in the line being built
    /// became a block of its own, and `zaru: one, two, three` replayed as
    /// `zaru:`, a blank row, then the answer. Caught by `zaru-cli`'s
    /// `adr_0010_d2s_conversation_replays_in_order_above_the_new_turn`, which
    /// is the check ADR-0010 D2's seventh producer landed with.
    pending_lead: Option<String>,
}

impl<'a> Builder<'a> {
    fn new(source: &'a str, lead: &str) -> Self {
        Self {
            source,
            blocks: Vec::new(),
            lines: Vec::new(),
            current: Styled::default(),
            modifiers: Vec::new(),
            indent: 0,
            lists: Vec::new(),
            pending_marker: None,
            tight: false,
            verbatim: 0,
            links: Vec::new(),
            pending_lead: (!lead.is_empty()).then(|| lead.to_owned()),
        }
    }

    /// Put the harness's label at the head of the first thing painted.
    ///
    /// **Outside every modifier span**, so a label before a heading is not
    /// itself a heading: it is the harness's word about the answer rather than
    /// a word of it.
    fn join_lead(&mut self) {
        if let Some(lead) = self.pending_lead.take() {
            self.current.text.push_str(&lead);
        }
    }

    /// The union of every modifier currently open.
    fn effective(&self) -> Modifier {
        self.modifiers
            .iter()
            .fold(Modifier::empty(), |all, one| all | *one)
    }

    /// Append text to the line being built, carrying whatever is open.
    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.join_lead();
        let modifier = self.effective();
        let start = self.current.text.len();
        self.current.text.push_str(text);
        if !modifier.is_empty() {
            self.current
                .spans
                .push((start..self.current.text.len(), modifier));
        }
    }

    /// End the line being built and start another. A hard break.
    fn break_line(&mut self) {
        self.lines.push(core::mem::take(&mut self.current));
    }

    /// End the block being built, if it has anything in it.
    fn close(&mut self) {
        if !self.current.text.is_empty() || !self.current.spans.is_empty() {
            self.break_line();
        }
        if self.lines.is_empty() {
            return;
        }
        let spaced = !core::mem::replace(&mut self.tight, false);
        self.blocks.push(Block {
            indent: self.indent,
            marker: self.pending_marker.take(),
            lines: core::mem::take(&mut self.lines),
            spaced,
        });
    }

    /// The whole source of an event, for a construct painted as itself.
    fn source_of(&self, range: &Range<usize>) -> &'a str {
        self.source.get(range.clone()).unwrap_or_default()
    }

    fn finish(mut self) -> Vec<Block> {
        // An answer with nothing in it still owes its label a row, so a
        // `zaru:` half that is somehow empty is visible rather than silent.
        self.join_lead();
        self.close();
        self.blocks
    }
}

/// Walk `answer` and produce the blocks the pane paints it as.
///
/// `Options::empty()`, so **no GFM extension is parsed**: a table, a
/// strikethrough, a footnote and a task list are not constructs here and their
/// characters arrive as ordinary text, which is the "painted as their own
/// source text" the ruling asks for, reached by not asking for them rather
/// than by a special case.
fn blocks_of(lead: &str, answer: &str) -> Vec<Block> {
    let mut builder = Builder::new(answer, lead);
    for (event, range) in Parser::new_ext(answer, Options::empty()).into_offset_iter() {
        // Inside a construct painted as its own source, every nested event is
        // already covered by the text that was pushed for the outer one.
        if builder.verbatim > 0 {
            match event {
                Event::Start(_) => builder.verbatim += 1,
                Event::End(_) => builder.verbatim -= 1,
                _ => {}
            }
            continue;
        }
        apply(&mut builder, event, range);
    }
    builder.finish()
}

/// Apply one event.
#[allow(clippy::too_many_lines)]
fn apply(builder: &mut Builder<'_>, event: Event<'_>, range: Range<usize>) {
    match event {
        // ---- blocks ----
        Event::Start(Tag::Paragraph) => builder.close(),
        Event::End(TagEnd::Paragraph) => builder.close(),

        Event::Start(Tag::Heading { .. }) => {
            builder.close();
            builder.modifiers.push(HEADING);
        }
        Event::End(TagEnd::Heading(_)) => {
            builder.modifiers.pop();
            builder.close();
        }

        // A fenced or indented block: its own lines, indented, no fence, no
        // info string, and no highlighting of any kind.
        Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_) | CodeBlockKind::Indented)) => {
            builder.close();
            builder.indent += BLOCK_INDENT;
        }
        Event::End(TagEnd::CodeBlock) => {
            builder.close();
            builder.indent -= BLOCK_INDENT;
        }

        Event::Start(Tag::BlockQuote(_)) => {
            builder.close();
            builder.indent += BLOCK_INDENT;
        }
        Event::End(TagEnd::BlockQuote(_)) => {
            builder.close();
            builder.indent -= BLOCK_INDENT;
        }

        // ---- lists ----
        Event::Start(Tag::List(first)) => {
            builder.close();
            builder.lists.push(first);
        }
        Event::End(TagEnd::List(_)) => {
            builder.close();
            builder.lists.pop();
            builder.tight = false;
        }
        Event::Start(Tag::Item) => {
            builder.close();
            let marker = match builder.lists.last_mut() {
                Some(Some(n)) => {
                    let marker = format!("{n}.");
                    *n += 1;
                    marker
                }
                _ => BULLET.to_owned(),
            };
            builder.pending_marker = Some(marker);
            // Every item but the first of one list is tight against the one
            // above it, so a list reads as a list rather than as a column of
            // paragraphs.
            builder.tight = builder
                .blocks
                .last()
                .is_some_and(|block| block.marker.is_some());
            builder.indent += (builder.lists.len().saturating_sub(1)) * BLOCK_INDENT;
        }
        Event::End(TagEnd::Item) => {
            builder.close();
            builder.indent -= (builder.lists.len().saturating_sub(1)) * BLOCK_INDENT;
        }

        // ---- inline ----
        Event::Start(Tag::Strong) => builder.modifiers.push(STRONG),
        Event::End(TagEnd::Strong) => {
            builder.modifiers.pop();
        }
        Event::Start(Tag::Emphasis) => builder.modifiers.push(EMPHASIS),
        Event::End(TagEnd::Emphasis) => {
            builder.modifiers.pop();
        }

        // A link paints its text and then its destination, so the one
        // construct that could conceal a value conceals nothing. The
        // parenthetical is omitted where the text **is** the destination —
        // an autolink — because there is then nothing left to reveal, which is
        // this rule's own reason rather than a second rule.
        Event::Start(Tag::Link { dest_url, .. }) => builder.links.push(dest_url.into_string()),
        Event::End(TagEnd::Link) => {
            if let Some(url) = builder.links.pop()
                && !builder.current.text.ends_with(&url)
            {
                builder.push(URL_OPEN);
                builder.push(&url);
                builder.push(URL_CLOSE);
            }
        }

        // An image is painted as its own source text: a pane cannot show one,
        // and a rendering that dropped it would hide that the answer had one.
        Event::Start(Tag::Image { .. }) => {
            builder.push(builder.source_of(&range).to_owned().as_str());
            builder.verbatim = 1;
        }

        Event::Code(code) => {
            builder.modifiers.push(INLINE_CODE);
            builder.push(&code);
            builder.modifiers.pop();
        }

        Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
            // A code block's text arrives with its own newlines and each is a
            // row of its own; everywhere else a newline inside one text event
            // is a soft break the wrap decides.
            let mut pieces = text.split('\n');
            if let Some(first) = pieces.next() {
                builder.push(first);
            }
            for piece in pieces {
                builder.break_line();
                builder.push(piece);
            }
        }

        // **A soft break is a row break, and this is the one place this
        // renderer deliberately differs from what a browser would do.**
        //
        // CommonMark folds a single newline into a space, so an answer written
        // as `1\n2\n3` is one paragraph and a browser paints it `1 2 3`. That
        // is exactly the defect row 3 of the look-and-feel survey measured --
        // "Asked to count 1 to 30 one per line, the pane painted
        // `123456789101112...`" -- and that the `pane-text` arc closed on
        // 2026-09-06 by making an answer's own newlines the answer's. Folding
        // them here would reopen a closed row, and it would contradict
        // ADR-0010 D2's pane clause in the one direction its 2026-09-15
        // amendment does **not** licence: a newline is a datum, not a
        // delimiter whose presentation is painted.
        //
        // The specification permits it in as many words -- a renderer may
        // render a soft line break as a hard line break -- so this is the
        // conforming choice rather than a deviation, and it is the
        // conservative one: markdown then changes delimiters and indentation
        // and changes nothing about where an answer's lines are.
        Event::SoftBreak | Event::HardBreak => builder.break_line(),

        // A thematic break has no presentation on a pane, so it paints as what
        // the author typed.
        Event::Rule => {
            builder.close();
            builder.push(
                builder
                    .source_of(&range)
                    .trim_end_matches('\n')
                    .to_owned()
                    .as_str(),
            );
            builder.close();
        }

        // Everything else -- a footnote reference, a task-list marker, maths --
        // cannot arrive under `Options::empty()`, and anything that does paints
        // as itself rather than vanishing.
        other => {
            if let Event::Start(_) = other {
                builder.push(builder.source_of(&range).to_owned().as_str());
                builder.verbatim = 1;
            }
        }
    }
}

/// Turn blocks into the rows a pane `width` wide paints them as.
///
/// The glyph column is the register's, exactly as a verbatim line's is: the
/// first row of the whole answer opens with the register's glyph and every row
/// after it is indented to the same column, so an answer of forty rows is
/// still one record.
fn paint(blocks: &[Block], register: Register, width: u16) -> Vec<Row> {
    let glyph = crate::shell::wrap::columns(register.glyph()) + 1;
    let mut rows: Vec<Row> = Vec::new();

    for block in blocks {
        if block.spaced && !rows.is_empty() {
            rows.push(Row {
                register,
                lead: String::new(),
                text: String::new(),
                emphasis: Vec::new(),
            });
        }
        let marker = block.marker.as_deref().unwrap_or_default();
        let marker_width = crate::shell::wrap::columns(marker);
        // A marker is followed by one space; a block with none is not.
        let after_marker = if marker.is_empty() {
            0
        } else {
            marker_width + 1
        };
        let inner = block.indent + after_marker;
        let budget = usize::from(width).saturating_sub(glyph + inner);
        let mut first_of_block = true;

        for line in &block.lines {
            let mut consumed = 0_usize;
            for text in crate::shell::wrap::rows(&line.text, budget) {
                let taken = text.len();
                let emphasis = carried(&line.spans, consumed..consumed + taken);
                consumed += taken;

                let opener = if first_of_block && !marker.is_empty() {
                    format!("{}{marker} ", " ".repeat(block.indent))
                } else {
                    " ".repeat(inner)
                };
                let lead = if rows.iter().any(|row| !row.lead.is_empty()) {
                    format!("{}{opener}", " ".repeat(glyph))
                } else {
                    format!("{} {opener}", register.glyph())
                };
                rows.push(Row {
                    register,
                    lead,
                    text,
                    emphasis,
                });
                first_of_block = false;
            }
        }
    }

    if rows.is_empty() {
        rows.push(Row {
            register,
            lead: format!("{} ", register.glyph()),
            text: String::new(),
            emphasis: Vec::new(),
        });
    }
    rows
}

/// The spans of one line that fall inside one row, rebased onto that row.
///
/// The wrap's own contract is that concatenating its rows reproduces its
/// input, and every string it is given here is newline-free — so a row's bytes
/// are exactly `window` of the line and a span is carried by subtraction. A
/// span straddling a break is clipped into both rows rather than dropped from
/// either, which is what stops a bold run losing its second half at a width
/// nobody tested.
fn carried(
    spans: &[(Range<usize>, Modifier)],
    window: Range<usize>,
) -> Vec<(Range<usize>, Modifier)> {
    spans
        .iter()
        .filter_map(|(span, modifier)| {
            let start = span.start.max(window.start);
            let end = span.end.min(window.end);
            (start < end).then(|| (start - window.start..end - window.start, *modifier))
        })
        .collect()
}

#[cfg(test)]
mod tests;
