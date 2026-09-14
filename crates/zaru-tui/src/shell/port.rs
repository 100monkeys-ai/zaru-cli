// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the shell needs from outside itself, declared here and implemented in
//! `zaru-cli`.
//!
//! # Why these are ports and not imports
//!
//! [ADR-0003] D8 permits this crate exactly one sibling dependency,
//! `zaru-core`, and `scripts/check-crate-boundaries.py` fails on any other
//! edge. Everything the shell must dispatch or render lives in `zaru-cli`:
//! [ADR-0015] D2's closed namespace set, [ADR-0014] D5's nearest match,
//! [ADR-0010] D2's transcript shapes, and [ADR-0011] D3's confirmation. None
//! of them can be named from here.
//!
//! So the consumer declares the port, the owner implements it, and `zaru-cli`
//! — the composition root, which already depends on both — writes the adapter.
//! That is the same dependency inversion [ADR-0005]'s composer already uses
//! for its two search tiers and the tool surface uses for its permission
//! decision. **No edge in the D8 table moves and `zaru-core` does not become a
//! shared-types crate**, which [ADR-0016]'s Status tracking records as
//! deliberately avoided.
//!
//! # What this buys, beyond the boundary
//!
//! The vocabulary stays declared **once**. ADR-0015 D2's ten namespaces, both
//! of each one's spellings, which of them this build implements, and the
//! nearest-match rule are all `zaru-cli`'s and are handed across rather than
//! retyped. The slash grammar in [`crate::shell::command`] is a second
//! *grammar* over one *vocabulary*, which is what D2's two-entry-point
//! sentence asks for — "`/session <verb>` inside a session and `zaru sessions
//! <verb>` outside one are the same commands reached from the two places a
//! user can be" — and not a second copy of the table.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use ratatui::style::{Color, Style};

/// One row of [ADR-0015] D2's namespace table, as the shell needs it.
///
/// Carries the slash spelling rather than the subcommand, because this is the
/// in-session surface and D2 keeps the two spellings distinct on purpose —
/// `/session` against `zaru sessions`. Deriving one from the other would have
/// to encode that difference as a rule, and it is not a rule; it is two words.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// D2's first column, including the leading slash.
    pub slash: &'static str,
    /// D2's second column: what this namespace governs.
    pub governs: &'static str,
    /// Whether this build implements the namespace's in-session half.
    ///
    /// **False is a statement about this build rather than about D2.** A word
    /// naming an unbuilt namespace is refused saying so, never placed against
    /// a nearest — telling a user who typed `stack` that they may have meant
    /// `session` is a worse answer than the truth.
    pub built: bool,
    /// The verbs this namespace takes **inside a session**, in the order help
    /// would list them.
    ///
    /// Not the same list as the subcommand's. `/session` takes `resume` and
    /// `continue` because [ADR-0010] D4 names them in as many words; outside a
    /// session those two are flags rather than verbs.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub verbs: &'static [&'static str],
}

/// [ADR-0015] D2's namespaces, and the nearest-match rule that goes with them.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub trait CommandVocabulary {
    /// Every namespace, in D2's own table order.
    fn namespaces(&self) -> Vec<Namespace>;

    /// The nearest slash spelling to a word that names none.
    ///
    /// [ADR-0014] D5's rule, answered by whoever owns the one implementation
    /// of it. Returns `None` only when the vocabulary is empty, which is a
    /// broken adapter rather than a user's mistake.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    fn nearest(&self, offered: &str) -> Option<&'static str>;

    /// The nearest verb of one namespace to a word that names none of them.
    ///
    /// `None` for a namespace that takes no verb at all, which is a different
    /// thing from a namespace whose verbs nothing matched.
    fn nearest_verb(&self, slash: &str, offered: &str) -> Option<&'static str>;
}

/// Which register a transcript line was written in.
///
/// # Why exhaustion has a variant of its own
///
/// [ADR-0008] D5: "Hitting the iteration ceiling is not an error and is not a
/// success." A renderer with two registers has to put exhaustion in one of
/// them, which is the conflation that clause exists to prevent, and D5's own
/// words are that it "surfaces as `LoopExhausted`... rather than either
/// claiming completion or reporting a generic failure".
///
/// # Why the loop's own setback has a variant of its own
///
/// [ADR-0028] D2's heading: "Failure is shown, **in its own register**, never
/// as an error." Until 2026-09-14 the loop's failure had no register of its
/// own — an iteration that failed and a validator that failed were
/// [`Register::Plain`], the register of ordinary narration, *because* D2's
/// second half forbids [`Register::Failed`]. So the second half was honoured
/// and the heading was not, and D2's remaining word — "coloured" — could not
/// be satisfied at all, since colouring `Plain` colours every line of
/// narration and is a theme rather than a register.
///
/// [`Setback`] is the register the heading names. It changes nothing about
/// the second half: an iteration's failure is still never in the register
/// reserved for defects, and that is now held by a check that reads the
/// painted cell rather than by the absence of an alternative.
///
/// [`Setback`]: Register::Setback
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    /// Ordinary narration: a user message, an iteration starting, a candidate.
    Plain,
    /// A tool call, per [ADR-0011] D4.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    Call,
    /// An announcement or an attribution, per [ADR-0002] D4, D5 and [ADR-0015]
    /// D6.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Announced,
    /// The loop finished the work.
    Succeeded,
    /// [ADR-0028] D2's own subject: an iteration that failed, or a validator
    /// whose expectation did not hold.
    ///
    /// **Never [`Register::Failed`]**, which is the register D2's second half
    /// reserves for defects — see the type's own documentation for why this
    /// variant exists and what it does not change.
    ///
    /// A validator that *passed* or that never ran is [`Register::Plain`]. A
    /// skip is not a setback: [ADR-0009] D2's `skipped` is a validator whose
    /// prerequisite failed, so the setback is the prerequisite's.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    Setback,
    /// The loop stopped at its ceiling or its window, per [ADR-0008] D5.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    Exhausted,
    /// [ADR-0016] D1's error register.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    Failed,
}

impl Register {
    /// Every register, so a check can walk them rather than list them.
    ///
    /// The length is annotated, so an eighth fails to compile here as well as
    /// in every exhaustive match below.
    pub const ALL: [Self; 7] = [
        Self::Plain,
        Self::Call,
        Self::Announced,
        Self::Succeeded,
        Self::Setback,
        Self::Exhausted,
        Self::Failed,
    ];

    /// The glyph that opens a line in this register.
    ///
    /// # Three of these are the records' and four are drafted
    ///
    /// `◈` is [ADR-0002] D4's and D5's announcement marker and [ADR-0015] D6's
    /// attribution marker, spelled in both records' own examples. `✗` is
    /// [ADR-0016] D2's, from that record's worked failure. The space for plain
    /// narration is the absence of a marker rather than a choice.
    ///
    /// **`✓`, `⊘` and `·` are drafted under a delegated coordinator ruling of
    /// 2026-09-05, open to Jeshua's veto**, because no record names a glyph
    /// for a completed loop, for exhaustion, or for a tool call. They are
    /// named here as one constant apiece rather than typed at a call site, so
    /// changing one is one edit. What is **not** drafted is that exhaustion
    /// gets a glyph distinct from both of the others: ADR-0008 D5 requires
    /// exactly that, and the check holds the distinctness rather than the
    /// characters.
    ///
    /// **`!` is drafted the same way, under the ruling of 2026-09-14 recorded
    /// as an accepted Update on [ADR-0028], and is Jeshua's to veto.** It is
    /// ASCII and one column, so it cannot skew a wrapped line's continuation
    /// indent. **One collision is recorded rather than hidden:**
    /// [`crate::shell::render::PROMINENT`] is also `"!"` and prefixes
    /// [ADR-0011] D6's prominent permission prompt. The two never appear in
    /// one region — that prompt takes the composer's area and a register's
    /// glyph opens a pane row — but one character now means two things on one
    /// screen, and that is a thing to veto rather than a thing to discover.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Plain => " ",
            Self::Call => "·",
            Self::Announced => "◈",
            Self::Succeeded => "✓",
            Self::Setback => "!",
            Self::Exhausted => "⊘",
            Self::Failed => "✗",
        }
    }

    /// The colour this register's glyph is painted in.
    ///
    /// # Sixteen ANSI colours, and why these six of them
    ///
    /// [ADR-0028] D2 requires an iteration's failure to be "**coloured**,
    /// worded, and placed as part of the work", and that record's Update of
    /// 2026-09-14 is where the table below was written before this code. The
    /// palette is drafted under the coordinator's ruling of that day and is
    /// **Jeshua's to veto**; each colour is one named constant beside the
    /// glyph it belongs to, so changing one is one edit.
    ///
    /// `Black`, `White`, `Gray` and `DarkGray` each vanish against one of the
    /// two grounds a terminal can have, or sit too close to both; the bright
    /// variants are chosen for dark backgrounds and wash out on light ones.
    /// The six normal-intensity hues are the members of the sixteen with
    /// usable contrast against both, which is what the ruling asks for
    /// [`Setback`], for [`Failed`] and for the out-of-tree marking's
    /// [`Call`]. **No truecolour and no 256-colour index**: `Color::Rgb` and
    /// `Color::Indexed` are refused by name in
    /// `every_register_colour_is_one_of_the_sixteen_ansi_colours`.
    ///
    /// [`Plain`] is [`Color::Reset`] rather than one of the sixteen, and that
    /// is the absence of a colour rather than a choice — exactly as its glyph
    /// is the absence of a marker. It is also what every [`ratatui`] cell
    /// already holds, so a pane of plain narration is byte-identical with
    /// colour on and with colour off.
    ///
    /// [`Call`]: Register::Call
    /// [`Failed`]: Register::Failed
    /// [`Plain`]: Register::Plain
    /// [`Setback`]: Register::Setback
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    #[must_use]
    pub const fn colour(self) -> Color {
        match self {
            Self::Plain => PLAIN,
            Self::Call => CALL,
            Self::Announced => ANNOUNCED,
            Self::Succeeded => SUCCEEDED,
            Self::Setback => SETBACK,
            Self::Exhausted => EXHAUSTED,
            Self::Failed => FAILED,
        }
    }
}

/// Ordinary narration carries no colour: the terminal's own foreground.
pub const PLAIN: Color = Color::Reset;

/// A tool call is the machine acting, and [ADR-0011] D4's out-of-tree marking
/// rides one of these lines.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub const CALL: Color = Color::Blue;

/// [ADR-0002] D4 and D5's announcement and [ADR-0015] D6's attribution — the
/// one class the user did not ask for, so it takes the hue furthest from the
/// four outcome colours.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub const ANNOUNCED: Color = Color::Magenta;

/// The conventional success hue.
pub const SUCCEEDED: Color = Color::Green;

/// [ADR-0028] D2's own subject: calm rather than alarming on both grounds.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
pub const SETBACK: Color = Color::Cyan;

/// [ADR-0008] D5: exhaustion "is not an error and is not a success", so
/// neither the success hue nor the error one.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
pub const EXHAUSTED: Color = Color::Yellow;

/// [ADR-0016] D1's error register, and the only place red appears.
///
/// D1: an expected failure must never be "**coloured like a crash**". This is
/// the crash colour, and [`Register::Setback`] carrying [`SETBACK`] instead is
/// that sentence holding by construction rather than by argument — before
/// 2026-09-14 no colour existed at all, so nothing could be coloured like one.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const FAILED: Color = Color::Red;

/// Whether the pane paints the registers' colours at all.
///
/// # Why this is an argument and not a field
///
/// The terminal is the thing that knows whether it paints colour, so it is
/// the thing that holds this and hands it to [`Shell::render`]. The shell
/// keeps no display state, and `zaru-tui` names no environment variable —
/// `NO_COLOR` is read once by `zaru-cli` at the composition, which is the only
/// place a process's environment is a fact rather than an ambient read.
///
/// [`Shell::render`]: crate::shell::Shell::render
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    /// Each register's glyph carries its own colour.
    Coloured,
    /// Nothing carries a colour and the glyphs stand alone.
    ///
    /// This is what `NO_COLOR` asks for, and it is why a capture taken under
    /// it is comparable with every capture taken before a colour existed:
    /// every cell keeps [`Color::Reset`], and `ratatui`'s crossterm backend
    /// emits a colour sequence only where a cell's colour differs from the
    /// last one — so a frame of `Reset` cells emits none at all.
    Monochrome,
}

impl Palette {
    /// The style a register's marker column is painted in.
    ///
    /// **The text is not passed to this function**, which is a stronger
    /// property than "colour does not alter the text": [`Line`]'s own
    /// documentation says the shell "chooses the glyph and nothing else", and
    /// a colour on a producer's words would be the shell choosing something
    /// about them.
    #[must_use]
    pub fn marker(self, register: Register) -> Style {
        match self {
            Self::Monochrome => Style::default(),
            Self::Coloured => Style::default().fg(register.colour()),
        }
    }
}

/// One rendered line of [ADR-0010] D2's transcript.
///
/// # The text is the producer's, not the shell's
///
/// [ADR-0008] D3: "**Rendering never reads loop internals.** If the terminal
/// needs something to display, the loop emits it; the terminal does not reach
/// in." So a line arrives already worded — including [ADR-0008] D6's elapsed
/// time, which is a field on the events that end an iteration and is composed
/// into the text by whoever holds those events. The shell chooses the glyph
/// and nothing else.
///
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// What the line says.
    pub text: String,
    /// Which register it was written in.
    pub register: Register,
}

impl Line {
    /// A line in a register.
    #[must_use]
    pub fn new(register: Register, text: impl Into<String>) -> Self {
        Self {
            register,
            text: text.into(),
        }
    }

    /// The line as the pane paints it: the register's glyph, a space, the
    /// text.
    ///
    /// **The text is not transformed.** [ADR-0010] D2 makes the transcript the
    /// replayable record and its Negative section says the file "contains
    /// whatever the session contained"; the pane is a view of that file, and a
    /// view that differs from what it views cannot be the thing D2 calls
    /// replayable. Redaction is [ADR-0008]'s `Redactor`, which applies on
    /// every path into a **model prompt or request** and on none into a pane.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn painted(&self) -> String {
        format!("{} {}", self.register.glyph(), self.text)
    }

    /// How many columns the glyph and its trailing space occupy.
    ///
    /// Every glyph in [`Register::ALL`] is one column wide, so this is two —
    /// but it is measured rather than written as `2`, because a register
    /// given a wide glyph would otherwise wrap one column late and the
    /// continuation rows would sit a column left of the text they continue.
    /// `every_register_glyph_occupies_one_column` asserts the premise, so a
    /// wide glyph reddens a check rather than skewing a pane.
    #[must_use]
    pub fn indent(&self) -> usize {
        crate::shell::wrap::columns(self.register.glyph()) + 1
    }

    /// The rows a pane `width` columns wide paints this line as.
    ///
    /// # One record is one record however many rows it takes
    ///
    /// The register's glyph opens the **first** row and every row after it is
    /// indented to the same column, so a line that wrapped still reads as one
    /// thing rather than as several. The glyph is not repeated: a second `✗`
    /// would say a second failure happened.
    ///
    /// The text's own newlines are honoured before any wrapping, which is
    /// what makes a thirty-line answer thirty rows. [ADR-0010] D2's Update of
    /// 2026-09-05 — "the terminal's transcript pane shows what the file
    /// holds, unaltered" — is the reason nothing is dropped at a break; see
    /// [`crate::shell::wrap`], where that property is stated with the check
    /// that holds it.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<Row> {
        let indent = self.indent();
        let budget = usize::from(width).saturating_sub(indent);
        let mut wrapped = crate::shell::wrap::rows(&self.text, budget).into_iter();
        let first = wrapped.next().unwrap_or_default();
        let mut painted = vec![Row {
            register: self.register,
            lead: format!("{} ", self.register.glyph()),
            text: first,
        }];
        painted.extend(wrapped.map(|text| Row {
            register: self.register,
            lead: " ".repeat(indent),
            text,
        }));
        painted
    }
}

/// One painted row of a [`Line`]: the marker column, then the text.
///
/// # Why the two halves are carried apart rather than joined
///
/// The pane paints the register's colour on `lead` and nothing on `text`,
/// which is [ADR-0028] D2's "coloured" read against this module's own seam —
/// [`Line`]'s documentation above says the shell "chooses the glyph and
/// nothing else", and a colour on the producer's words would be the shell
/// choosing something about them. Carried as one string, the renderer would
/// have to re-derive where the marker ends, which is the same rule living in
/// two places.
///
/// [`joined`] is the two put back together, byte for byte as the pane painted
/// them before 2026-09-14. Nothing about what reaches the buffer changed when
/// this type arrived, and `a_rows_joined_form_is_what_the_pane_painted_before`
/// is the check that says so rather than the commit message.
///
/// [`joined`]: Row::joined
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Which register the line this row belongs to was written in.
    pub register: Register,
    /// The marker column: the register's glyph and its trailing space on the
    /// first row of a line, and the same number of spaces on every
    /// continuation row, so a wrapped record still reads as one record.
    pub lead: String,
    /// What this row carries, already wrapped to fit beside `lead`.
    pub text: String,
}

impl Row {
    /// The row as one string, which is what the pane painted before the two
    /// halves were carried apart.
    #[must_use]
    pub fn joined(&self) -> String {
        format!("{}{}", self.lead, self.text)
    }
}

/// Where the pane's lines come from.
///
/// A port rather than a `Vec` handed in once, because [ADR-0010] D2 makes the
/// transcript append-only and a session that runs adds to it; the pane asks
/// again rather than being told.
pub trait TranscriptSource {
    /// Every line, oldest first.
    fn lines(&self) -> Vec<Line>;
}

/// What the user is asked, and how loudly.
///
/// Mirrors [ADR-0011] D3's question without naming `zaru-cli`'s type, for the
/// boundary reason in the module documentation. The statement is composed by
/// the decision and handed here — never composed at the point of rendering —
/// so that what the user was told and what the harness believes it asked
/// cannot drift apart.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirmation {
    /// The whole sentence the prompt states.
    pub statement: String,
    /// What follows it: the answers, and which of them is the default.
    ///
    /// # Handed across, for the same reason the statement is
    ///
    /// `zaru-cli`'s plain prompt already spells this once, as
    /// `tools::prompt::SUFFIX`, and that constant landed first. Spelling it
    /// again here would put the vocabulary a user reads in two places, and
    /// [ADR-0011] D3's whole argument for composing the statement once is
    /// that what the user was told and what the harness believes it asked
    /// cannot be allowed to drift apart. **The `y/N` a user reads is part of
    /// what they were told.** So it crosses the port as a value and this
    /// crate holds no constant for it.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub answers: String,
    /// Whether [ADR-0011] D6 matched, so the prompt can be raised without
    /// re-deriving why.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub prominent: bool,
}

impl Confirmation {
    /// A question, its answers, and whether it is a loud one.
    #[must_use]
    pub fn new(statement: impl Into<String>, answers: impl Into<String>, prominent: bool) -> Self {
        Self {
            statement: statement.into(),
            answers: answers.into(),
            prominent,
        }
    }
}

/// What the user is asked for when the answer is a secret.
///
/// The second kind of question this shell stands, beside [`Confirmation`], and
/// **not a second confirmer**: nothing here answers yes or no, and no port in
/// `zaru-cli` abstracts over the two. [ADR-0015] D2's `/providers keys add
/// <kind>` had no in-session half because [ADR-0007] D7 reads the key from
/// standard input — an argument being in the shell's history file and in `ps`
/// output for every user on the machine — and a terminal in raw mode has no
/// standard input to hand it. What was missing is a way to read a secret at a
/// terminal without echoing it, and this is that question's half of it.
///
/// Both fields are **composed by the caller and handed here**, for
/// [`Confirmation`]'s own reason: what the user was told and what the harness
/// believes it asked cannot be allowed to drift apart. This crate authors one
/// thing about this question and it is the mask glyph — see
/// [`MASK`](crate::shell::render::MASK).
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRequest {
    /// The whole sentence the question states, naming what is being asked for.
    pub statement: String,
    /// What follows the masked row: how to finish, and how to decline.
    ///
    /// Handed across exactly as [`Confirmation::answers`] is, and for the same
    /// reason stated there — the words a user reads are part of what they were
    /// told, so they cross the port as a value and this crate holds no
    /// constant for them.
    pub guidance: String,
}

impl SecretRequest {
    /// A question for a secret, and the line telling the reader how to answer.
    #[must_use]
    pub fn new(statement: impl Into<String>, guidance: impl Into<String>) -> Self {
        Self {
            statement: statement.into(),
            guidance: guidance.into(),
        }
    }
}

/// How a [`SecretRequest`] ended.
///
/// **Two variants and no `Option<String>`**, for the reason `zaru-cli`'s
/// `Taken` and `Turned` already carry — named in prose rather than linked,
/// because this crate cannot name that one: a user who declined and a user who
/// typed nothing are different facts, and a caller does different things with
/// them. The bytes are never in here — they leave the shell only through
/// [`Shell::take_secret`](crate::shell::Shell::take_secret), which is named so
/// that one search finds every call site, exactly as `zaru-cli`'s
/// `Secret::expose_for_dispatch` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretAnswer {
    /// The user finished typing and pressed `Enter`.
    Given,
    /// The user pressed `Esc` or `Ctrl-C`. Nothing is stored.
    Declined,
}
