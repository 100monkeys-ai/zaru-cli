// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the shell paints.
//!
//! # Three regions, and only the middle one changes size
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────┐
//! │ runtime.tier = bare · gemini-3.6-flash · ask · 1.2k/1048.5k · 4.2s │  status, 1 row
//! ├──────────────────────────────────────────────────────────────────┤
//! │ ✓ finished in 4.2s                                               │  transcript pane
//! │ ⊘ stopped at the ceiling after 3                                  │
//! ├──────────────────────────────────────────────────────────────────┤
//! │ > what should I do                                               │  composer, fixed
//! │   a page the trie matched                                        │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! The status line is at the top and the composer's area is a fixed height at
//! the foot, so the input row sits at a position that is a function of the
//! terminal's size and nothing else. That is [ADR-0005] D2 in a shell — see
//! [`crate::shell`]'s own documentation for why the alternative fails.
//!
//! **The status row is one line and is not wrapped.** It is **composed** to
//! the width instead, which is the 2026-09-06 amendment to [ADR-0001] D2 and
//! is what replaced the sentence that stood here: that a row wider than the
//! terminal was clipped at the right edge by `ratatui`, and that D2's "at all
//! times" was therefore a consequence of the tier being *first* rather than of
//! an elision rule nothing states. **Nothing states it no longer**: a clip
//! protects the first field and silently drops every other clause's, so the
//! order in which fields go is now declared as [`crate::shell::Rank`] and read
//! by [`crate::shell::Status::painted`], which takes this region's width. The
//! clip survives in exactly one case — a terminal too narrow for the tier's
//! own spelling — and that is the case D2's clause-6 check already covers.
//! **The transcript pane is the opposite and wraps**, which is not an
//! inconsistency: the status row is one row by construction and its ordering
//! is what protects the clause on it, while a transcript line clipped at the
//! right edge silently loses whatever the producer put last — measured on
//! 2026-09-05 to include the values and the `← effective` marker of
//! `config explain`, and [ADR-0011] D4's out-of-tree marking. See
//! [`crate::shell::wrap`].
//!
//! The pane shows the **tail**, which is [ADR-0010] D4's "re-renders the last
//! stretch of transcript so the user can see where they were" — a tail of
//! painted rows rather than of records, so a record longer than the pane
//! shows its newest rows.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::shell::{COMPOSER_ROWS, Line, Palette, Row, Shell, Viewing, below, transcript_floor};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::Line as TextLine;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

/// What a prominent prompt is prefixed with, per [ADR-0011] D6.
///
/// D6: "Surfacing beats forbidding." The marking raises the prompt without
/// changing what it can do, which is that clause as a rendering rather than as
/// a veto.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const PROMINENT: &str = "!";

/// The last `height` rows `lines` paint as in a pane `width` columns wide.
///
/// The body [`Shell::visible`] carried until 2026-09-14, lifted out because
/// the renderer now takes a tail **twice**: once over the lines the pane was
/// given and once over the answer still arriving, each into its own region.
/// Two copies of a tail is two places a fencepost can be wrong, and the two
/// would agree for the life of any defect in either.
///
/// Rows rather than lines, and `width` rather than a bare count, for the
/// reasons on [`Shell::visible`].
fn tail(lines: &[Line], height: u16, width: u16) -> Vec<Row> {
    let rows: Vec<Row> = lines.iter().flat_map(|line| line.rows(width)).collect();
    let start = rows.len().saturating_sub(usize::from(height));
    rows[start..].to_vec()
}

/// The `height` rows `lines` paint as from row `first`, in a pane `width`
/// columns wide.
///
/// The held counterpart of [`tail`], and deliberately the same shape: one
/// flattening, one slice. `first` is clamped by the caller, which is the one
/// place that knows how many rows there are.
fn window(lines: &[Line], first: usize, height: u16, width: u16) -> Vec<Row> {
    let rows: Vec<Row> = lines.iter().flat_map(|line| line.rows(width)).collect();
    let end = first.saturating_add(usize::from(height)).min(rows.len());
    rows.get(first..end)
        .map(<[Row]>::to_vec)
        .unwrap_or_default()
}

/// What one character typed at a [`SecretRequest`] paints as.
///
/// U+2022, BULLET. **Drafted under a delegated coordinator ruling of
/// 2026-09-14, open to Jeshua's veto**, in the same shape as the register
/// glyphs, `STRIP_ROWS` and the composer's `RETURN`: no record names a glyph
/// for a masked character and one is needed, so it is named once here with its
/// reasoning rather than typed at a call site. It is recorded on
/// [ADR-0011's amendments volume 2].
///
/// # One glyph per character, and the length is the disclosure
///
/// **The cost is stated rather than glossed**: a row of one glyph per
/// character publishes the value's *length* to a shoulder, to a screen capture
/// and to terminal scrollback. A glyph is not a byte of the value — the
/// security corpus asserts bytes — but a count is metadata, and this is where
/// it is admitted.
///
/// **The reason it is paid** is that a row painting nothing is what `sudo`
/// does, and a paste into a row that paints nothing is indistinguishable from
/// a dead terminal. That is row 12 of [the look-and-feel survey] in its
/// general form: a thing that happened, which nothing on the screen said. This
/// workspace's own trade is that the legible option wins, and the alternative
/// is named here so that replacing it is one constant and one check.
///
/// [ADR-0011's amendments volume 2]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface-updates-2
/// [the look-and-feel survey]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel
/// [`SecretRequest`]: crate::shell::SecretRequest
pub const MASK: &str = "\u{2022}";

/// One painted row as the spans that reach the buffer.
///
/// # Why one function rather than the two span lists it replaces
///
/// The pane and the region a streamed answer gets when the two do not fit
/// paint the same rows through the same [`tail`], and they built the same span
/// list twice. A third caller — or a modifier reaching one of them and not the
/// other — is the shape this crate keeps replacing, so the composition is
/// named once.
///
/// **The marker column carries the register's colour and the text carries
/// none of it.** That is [ADR-0028] D2's "coloured" read against
/// [`crate::shell::port::Line`]'s own seam: the shell "chooses the glyph and
/// nothing else", so a colour on a producer's words would be the shell
/// choosing something about them — and `palette` is not consulted for the text
/// at all, which is a stronger property than a rule saying it must not be.
///
/// **A modifier is the answer's own and never the register's.** A row whose
/// `emphasis` is empty yields exactly the one [`Span::raw`] this painted
/// before a modifier existed, byte for byte, which is every row a verbatim
/// line produces. Where it is not empty the text is split on the ranges the
/// CommonMark renderer recorded, and the pieces between them are raw — so
/// what a modifier can reach is bounded by what the answer's own markup asked
/// for. The amendment that licenses it is on [ADR-0028's amendments page].
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
/// [ADR-0028's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative-updates
fn spans_of(row: Row, palette: Palette) -> Vec<Span<'static>> {
    let mut spans = vec![Span::styled(row.lead, palette.marker(row.register))];
    if row.emphasis.is_empty() {
        spans.push(Span::raw(row.text));
        return spans;
    }
    let mut at = 0_usize;
    for (span, modifier) in &row.emphasis {
        if span.start > at {
            spans.push(Span::raw(row.text[at..span.start].to_owned()));
        }
        spans.push(Span::styled(
            row.text[span.start..span.end].to_owned(),
            Style::default().add_modifier(*modifier),
        ));
        at = span.end;
    }
    if at < row.text.len() {
        spans.push(Span::raw(row.text[at..].to_owned()));
    }
    spans
}

impl Shell {
    /// The three regions, top to bottom.
    ///
    /// Public so a check can assert where the input row is without inferring
    /// it from a painted frame, and so a host can size a terminal against it.
    #[must_use]
    pub fn regions(area: Rect) -> [Rect; 3] {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(COMPOSER_ROWS),
        ])
        .areas(area)
    }

    /// The rows the pane paints in a `height` by `width` area, oldest first.
    ///
    /// The **tail**: a pane shorter than the transcript shows the end of it,
    /// because that is where the user was.
    ///
    /// # The tail is a tail of rows, not of records
    ///
    /// It counted records until 2026-09-06, which was the same number only
    /// while every record was one row. A thirty-line answer is thirty rows,
    /// and a tail taken over records would have handed `ratatui` thirty rows
    /// for a pane with room for ten and let the widget keep the first ten —
    /// showing a user the beginning of the answer they had just watched
    /// arrive. Counting rows keeps [ADR-0010] D4's "the last stretch"
    /// true **inside** one record as well as across several.
    ///
    /// `width` is taken because a row count is a function of it. That is the
    /// signature change wrapping forces, and it is the honest one: how much
    /// of the transcript fits genuinely depends on how wide the terminal is.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// **Since 2026-09-15 the tail is where the window sits by default and
    /// not the only place it can sit.** [`Viewing::Tail`] takes the same
    /// `tail` call this made before a window existed — the branch rather than
    /// an equivalent of it, so a session where nobody presses a key paints
    /// what it painted — and [`Viewing::At`] takes the rows from there.
    #[must_use]
    pub fn visible(&self, height: u16, width: u16) -> Vec<Row> {
        match self.viewing() {
            Viewing::Tail => tail(&self.pane_lines(), height, width),
            Viewing::At(_) => window(&self.pane_lines(), self.first(height, width), height, width),
        }
    }

    /// What the composer's area shows: the prompt, or a standing question.
    ///
    /// The answers line is the one `zaru-cli`'s plain prompt writes, handed
    /// across rather than spelled again — see [`Confirmation::answers`].
    ///
    /// **A confirmation's `detail` is not here**, and that is deliberate. It
    /// is painted in a pinned region out of the *pane's* own area by
    /// [`Shell::question_detail`], for the reason a queued task is: the
    /// composer's area is seven rows at every terminal size, and at 40
    /// columns a resolved absolute path wraps to three of them, so a preview
    /// inside this area would be three rows of an elision — which tells a
    /// reader less than the elision marker does.
    ///
    /// [`Confirmation::answers`]: crate::shell::Confirmation::answers
    #[must_use]
    pub fn prompt_lines(&self) -> Vec<String> {
        // **The secret arm reads a count and never the bytes**, because
        // `Shell` exposes no accessor that yields them. What cannot be reached
        // cannot be painted by accident, which is the structural half of
        // ADR-0007 D3 arriving on a surface that did not exist when that
        // clause was written.
        if let Some(request) = self.asking_secret() {
            return vec![
                request.statement.clone(),
                MASK.repeat(self.secret_len()),
                request.guidance.clone(),
            ];
        }
        match self.asking() {
            None => Vec::new(),
            Some(question) => {
                let statement = if question.prominent {
                    format!("{PROMINENT} {}", question.statement)
                } else {
                    question.statement.clone()
                };
                let mut lines = vec![statement];
                // What the question is about, between what it asks and how to
                // answer it. Painted exactly as it was handed across: this
                // crate composes nothing a user reads, and the lines arrive
                // already redacted, because whether a value is a secret is
                // not a thing a renderer can know.
                lines.extend(question.detail.iter().cloned());
                lines.push(question.answers.clone());
                lines
            }
        }
    }

    /// Every row a standing question paints, in reading order, already
    /// wrapped to `width`.
    ///
    /// The statement, then what the question is about, then the answers —
    /// each broken by [`wrap::rows`](crate::shell::wrap::rows), which is the
    /// pane's own row-breaking, so the `unicode-width` measurement and the
    /// loss-free property are the pane's and nothing is authored here.
    ///
    /// **The rows carry no marker column**, unlike a transcript line's. The
    /// question is not a record of something that happened and the composer's
    /// area has never had one; adding one would also take two columns from a
    /// 40-column terminal, which is the width this whole change is for.
    ///
    /// # Why this is wrapped at all, measured rather than assumed
    ///
    /// Until 2026-09-14 the composer's area painted a question's lines
    /// through a `Paragraph` with no `Wrap`, so `ratatui` clipped them at the
    /// right edge. From the release binary at `a8eedf7` at **40 columns**,
    /// the whole of an `fs.write` question read `Allow fs.write
    /// /tmp/claude-1000/-home-th` — a person approving a write to a file
    /// whose name was not on the screen — and an out-of-tree read read `Allow
    /// fs.read /etc/hostname  [OUTSIDE th`, cutting [ADR-0011] D4's class
    /// mid-word on the one row the clause is about. **In the same session two
    /// beats later, the transcript row for that same call wrapped whole**,
    /// because [`crate::shell::wrap`] has broken pane lines since `pane-text`.
    /// The question was the surface that never got it.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub fn question_rows(&self, width: u16) -> Vec<String> {
        self.prompt_lines()
            .into_iter()
            .flat_map(|line| crate::shell::wrap::rows(&line, usize::from(width)))
            .collect()
    }

    /// The pane's own area, and the region a standing question's overflow and
    /// its detail are pinned to.
    ///
    /// # Why any of it leaves the composer's area
    ///
    /// The composer's area is [`COMPOSER_ROWS`] rows at every terminal size
    /// and a standing question takes it whole. At 40 columns a resolved
    /// absolute path wraps the statement alone to three of those seven, so a
    /// content preview painted inside that area would be three rows of an
    /// elision — which tells a reader less than the elision marker does.
    /// **So the question's rows fill the composer's area from the bottom up,
    /// and whatever does not fit is painted in a region pinned immediately
    /// above it**, taken out of the pane exactly as
    /// [`Shell::pane_and_queue`] takes the queued task's row.
    ///
    /// Reading order is unchanged by the split: the pinned region holds the
    /// *earlier* rows and the composer's area the later ones, so a reader
    /// goes top to bottom through statement, detail, answers as before. The
    /// answers row is last, so it is always in the composer's area and is the
    /// one row a narrow terminal can never lose.
    ///
    /// **`COMPOSER_ROWS` does not move and [ADR-0005] is not amended**: the
    /// rows come out of the pane, above the composer, not out of the
    /// composer. That is the same sentence [`Shell::pane_and_queue`] already
    /// carries and the same one `keys-in-session` recorded for the masked
    /// question.
    ///
    /// The region is capped at [`transcript_floor`] of the pane, the
    /// half-share `pane-notices` already authored, so a long preview cannot
    /// take the whole pane and hide the call it is about.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    fn pane_and_question(&self, pane: Rect, composer: Rect) -> (Rect, Option<Rect>) {
        let rows = self.question_rows(composer.width).len();
        let fits = usize::from(COMPOSER_ROWS);
        if rows <= fits {
            return (pane, None);
        }
        let overflow = rows - fits;
        let height = u16::try_from(overflow)
            .unwrap_or(u16::MAX)
            .min(transcript_floor(pane.height));
        if height == 0 {
            return (pane, None);
        }
        let [above, pinned] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(pane);
        (above, Some(pinned))
    }

    /// The pane's own area, and the row a queued task is pinned to.
    ///
    /// # Why the queued row is pinned rather than added to the pane's lines
    ///
    /// [`Shell::pane_lines`] is the transcript, then this session's notices,
    /// then the answer being streamed — in that order — and [`Shell::visible`]
    /// shows the **tail**. So a notice sits *above* an answer that is still
    /// growing, and a long answer pushes it out of the visible tail within a
    /// beat or two. That is measurable rather than theoretical: the
    /// look-and-feel survey's row 12 recorded "nothing is refused, and no line
    /// says why" of a build that had carried a refusal notice for fifty
    /// commits. It had been painted and then scrolled away.
    ///
    /// A queued task that vanished behind a long answer would be useless in
    /// exactly the case a person most needs it, so it takes a reserved row at
    /// the foot of the pane's own region, which no amount of scrolling can
    /// reach.
    ///
    /// **The composer's area is untouched.** [ADR-0005] D2's one input row and
    /// its six reserved strip rows are unchanged, and the input row is still a
    /// function of the terminal's size alone — the row comes out of the pane,
    /// above the composer, not out of the composer.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    fn pane_and_queue(&self, pane: Rect) -> (Rect, Option<Rect>) {
        if self.queued().is_none() {
            return (pane, None);
        }
        let [above, pinned] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(pane);
        (above, Some(pinned))
    }

    /// The region the lines the pane was given get, and the region the answer
    /// still arriving gets, if the two do not fit together.
    ///
    /// # The defect this closes
    ///
    /// [`Shell::pane_lines`] is the transcript, then this session's notices,
    /// then the answer being streamed, and [`Shell::visible`] shows the
    /// **tail**. So every line [`Shell::notice`] is given sits *above* a line
    /// that grows without bound, and the three callers of it in the driver —
    /// [ADR-0008] clause 3's turn events, [ADR-0028] D3's loop events, and the
    /// interrupt line — are pushed off the visible tail within a beat or two
    /// of an answer starting to arrive.
    ///
    /// Measured from the release binary at `cb9f4fc` over a pseudo-terminal:
    /// at 100 columns the narrative was visible for **2.55 s of a 76 s turn**,
    /// and 725 consecutive reconstructed frames held nothing but the answer's
    /// own rows. Six of the nineteen rows that appeared when the turn ended
    /// are iteration events painted the moment they arrived, and not one of
    /// them was visible for a single frame while the work it narrates was
    /// happening. [ADR-0028] D1 has the loop render "as narrative" and D5
    /// requires it "**as the work proceeds**"; that record's Update of
    /// 2026-09-14 reads both as claims about the screen rather than about the
    /// emission, and this is that reading as a layout.
    ///
    /// # It fires only when the pane is over-subscribed
    ///
    /// With nothing streaming, and with the lines and the answer fitting
    /// together, this returns `(pane, None)` — the branch the renderer took
    /// before this existed, rather than an equivalent of it. So a short answer
    /// in a fresh session is painted exactly where it was painted, with no
    /// gap, no boundary and no re-ordering, and the frame at the end of every
    /// turn is the frame it was by **construction**:
    /// [`Shell::clear_streaming`] leaves nothing to split.
    ///
    /// When they do not fit, the lines the pane was given keep
    /// [`transcript_floor`] of it or their own height, whichever is smaller,
    /// and the answer takes every remaining row it can use. Each region then
    /// shows its own tail, so the newest narration and the newest text of the
    /// answer are on the screen at once.
    ///
    /// # Why it is applied after the queued row and not before
    ///
    /// [`Shell::pane_and_queue`] pins a queued task to the foot of the pane,
    /// and that row is [ADR-0015]'s amendment of 2026-09-13. Splitting the
    /// stream out of the whole pane first would put the answer's region below
    /// it and move the row, which
    /// `a_queued_task_paints_above_the_composer_and_survives_a_streaming_answer`
    /// reddens.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    /// The region the pane's rows get, and the row the held notice gets.
    ///
    /// # Why the row comes out of the pane and never out of the composer
    ///
    /// [ADR-0005] D2's input row is a function of the terminal's size alone,
    /// and the shell keeps that true by giving the composer a **fixed** area.
    /// A notice that borrowed a strip row would move the input row the moment
    /// a person scrolled, which is that record's clause-5 mutant arriving
    /// through a different door. `pane_and_queue` settled the same question
    /// for a queued task on 2026-09-13 and this is its shape, not a second
    /// one.
    ///
    /// **It fires only while the pane is held and something is below it.** A
    /// following pane returns `(pane, None)` — the branch that existed before
    /// this did — so nothing about an ordinary frame changes, and a window
    /// held at the bottom of a short transcript says nothing, because there
    /// is nothing to say.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    fn pane_and_notice(&self, pane: Rect) -> (Rect, Option<Rect>) {
        if self.viewing() == Viewing::Tail || pane.height == 0 {
            return (pane, None);
        }
        // Counted against the rows the pane will actually get, which is one
        // fewer than it has: a count taken against the whole region and
        // painted beside a shorter one would be off by exactly the row it is
        // painted on.
        let rows = pane.height.saturating_sub(1);
        if self.rows_below(rows, pane.width) == 0 {
            return (pane, None);
        }
        let [above, notice] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(pane);
        (above, Some(notice))
    }

    fn pane_and_stream(&self, pane: Rect) -> (Rect, Option<Rect>) {
        // **A held pane is never split.** The split below exists so that the
        // newest narration and the newest text of an answer are both on the
        // screen while the pane is *following*, which is the state ADR-0028
        // D5's "as the work proceeds" is about and the state it was measured
        // in. A person who held the window asked for these rows and not for
        // the newest ones, and moving rows around inside a frozen window is
        // the reflow that whole decision is written against.
        if self.viewing() != Viewing::Tail {
            return (pane, None);
        }
        let Some(streamed) = self.streamed_line() else {
            return (pane, None);
        };
        let arriving = streamed.rows(pane.width).len();
        let given: usize = self
            .given_lines()
            .iter()
            .map(|line| line.rows(pane.width).len())
            .sum();
        if given + arriving <= usize::from(pane.height) {
            return (pane, None);
        }

        let kept = given.min(usize::from(transcript_floor(pane.height)));
        // `pane.height` is at least `kept` by the line above, so this cannot
        // wrap; and it cannot exceed `u16::MAX`, because it is bounded by a
        // `u16` the caller handed in.
        let height = arriving.min(usize::from(pane.height) - kept) as u16;
        let [above, below] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(height)]).areas(pane);
        (above, Some(below))
    }

    /// Paint the whole shell into `area`.
    ///
    /// `palette` decides whether the registers' colours are painted, and it
    /// is an argument rather than shell state because the terminal is what
    /// knows — see [`Palette`]. **It reaches exactly one thing**: a
    /// transcript row's marker column. The status line, the composer, the
    /// hint strip and a standing question are painted the same way at either
    /// value, which is `no_register_colour_reaches_the_status_line_the_\
    /// composer_or_the_hint_strip`.
    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, palette: Palette) {
        let [status, pane, composer] = Self::regions(area);
        let (pane, queued) = self.pane_and_queue(pane);
        let (pane, held) = self.pane_and_notice(pane);
        // After the queued row and the held notice so both stay pinned at the
        // pane's foot, and before the stream so an answer still arriving
        // cannot push a standing question's own rows off the screen.
        let (pane, question_overflow) = self.pane_and_question(pane, composer);
        let (pane, arriving) = self.pane_and_stream(pane);

        frame.render_widget(
            Paragraph::new(TextLine::from(self.status().painted(status.width))),
            status,
        );

        // No `Wrap` on this paragraph, and that is deliberate: `visible`
        // has already broken every row to `pane.width`, and a widget
        // re-wrapping them would measure a continuation's indent as content
        // and break it again one column early.
        // The rows the pane was given, and the rows of the answer still
        // arriving, come from the same `tail` -- so the two regions cannot
        // come to disagree about what a tail is. When `arriving` is `None`
        // the first of these is `visible` itself, which is the branch this
        // renderer took before a stream had a region of its own.
        let rows = match arriving {
            None => self.visible(pane.height, pane.width),
            Some(_) => tail(&self.given_lines(), pane.height, pane.width),
        };
        let visible: Vec<TextLine<'_>> = rows
            .into_iter()
            // Two spans rather than one joined string: the marker column
            // carries the register's colour and the producer's words carry
            // nothing. `Paragraph` sets a style per grapheme, so the style
            // reaches the cells `lead` paints and not the blanks past the end
            // of the row.
            //
            // It is also what lets `a_rows_joined_form_is_what_the_pane_\
            // painted_before` compare `Row::joined` against the buffer
            // without both arms travelling through the same function.
            .map(|row| TextLine::from(spans_of(row, palette)))
            .collect();
        if !visible.is_empty() {
            frame.render_widget(Paragraph::new(visible), pane);
        }

        // The answer still arriving, in the region of its own it gets when the
        // pane cannot hold both. Painted through the same two spans and the
        // same `tail`, so nothing about a streamed row differs from a
        // transcript row except which rectangle it lands in.
        if let (Some(area), Some(streamed)) = (arriving, self.streamed_line()) {
            let streaming: Vec<TextLine<'_>> = tail(&[streamed], area.height, area.width)
                .into_iter()
                .map(|row| TextLine::from(spans_of(row, palette)))
                .collect();
            if !streaming.is_empty() {
                frame.render_widget(Paragraph::new(streaming), area);
            }
        }

        // The held pane's own row, at the foot of the pane's region and above
        // whatever `pane_and_queue` pinned. Painted through `Line::rows` like
        // the queued row, so the register, the glyph and the width
        // measurement are the pane's and the only thing authored here is the
        // sentence `below` carries.
        if let Some(area) = held {
            let row = crate::shell::port::Line::new(
                crate::shell::port::Register::Plain,
                below(self.rows_below(pane.height, pane.width)),
            )
            .rows(area.width)
            .first()
            .map(crate::shell::port::Row::joined)
            .unwrap_or_default();
            frame.render_widget(Paragraph::new(TextLine::from(row)), area);
        }

        // The queued task, on its own row immediately above the composer. It
        // goes through `Line::rows` so the register, its glyph and the width
        // measurement are the pane's own and nothing is authored here beyond
        // the one word `QUEUED` carries.
        if let (Some(area), Some(task)) = (queued, self.queued()) {
            // `Row::joined` rather than the two spans the pane uses, and the
            // register stays `Plain`: this row sits **below** the pane, in the
            // composer's own area, and the ruling of 2026-09-13 23:58Z puts a
            // colour on a transcript line's marker and nowhere else. `Plain`'s
            // colour is `Color::Reset` either way, so the two spellings paint
            // the same cells -- what `joined` says is that the choice is
            // deliberate rather than incidental.
            let row =
                crate::shell::port::Line::new(crate::shell::port::Register::Plain, task.painted())
                    .rows(area.width)
                    .first()
                    .map(crate::shell::port::Row::joined)
                    .unwrap_or_default();
            frame.render_widget(Paragraph::new(TextLine::from(row)), area);
        }

        // A standing question takes the composer's area whole. The input is
        // not usable while one stands -- ADR-0011 D3's prompt is not something
        // a user types past -- and painting both would offer a surface that
        // does nothing.
        //
        // The rows are wrapped by the pane's own row-breaking rather than
        // clipped by the widget, and they fill this area from the **bottom**
        // up: whatever does not fit is painted in the region `pane_and_
        // question` pinned immediately above, so the answers row -- which is
        // last -- is the one row a narrow terminal can never lose. No `Wrap`
        // on the paragraph, for the pane's reason: `question_rows` has
        // already broken every row to this width.
        let rows = self.question_rows(composer.width);
        if rows.is_empty() {
            self.composer().render(frame, composer);
        } else {
            let fits = usize::from(COMPOSER_ROWS);
            let start = rows.len().saturating_sub(fits);
            if let Some(area) = question_overflow {
                let above: Vec<TextLine<'_>> = rows[..start]
                    .iter()
                    .rev()
                    .take(usize::from(area.height))
                    .rev()
                    .map(|row| TextLine::from(row.clone()))
                    .collect();
                if !above.is_empty() {
                    frame.render_widget(Paragraph::new(above), area);
                }
            }
            let question: Vec<TextLine<'_>> = rows[start..]
                .iter()
                .map(|row| TextLine::from(row.clone()))
                .collect();
            frame.render_widget(Paragraph::new(question), composer);
        }
    }
}
