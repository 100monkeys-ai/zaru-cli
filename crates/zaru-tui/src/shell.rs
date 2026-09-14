// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The terminal session: a status line, a transcript pane, and the composer.
//!
//! # What this is, and what it deliberately is not
//!
//! This is the in-session surface every record has been waiting on. [ADR-0015]
//! D2's slash half, [ADR-0008] clauses 3 to 5's rendering half, [ADR-0002]
//! D8's standing tip, [ADR-0001] D2's status line and [ADR-0010] D4's
//! re-rendered tail all needed a terminal, and four records say so in as many
//! words — each carrying some form of the sentence "an interactive session
//! that does not exist".
//!
//! **No loop runs here and no provider is reached.** The pane renders lines it
//! is handed through [`TranscriptSource`]; a staged transcript is the fixture.
//! [ADR-0008] clause 3 requires the event stream to be "consumed by both the
//! terminal renderer and the transcript writer, **from one emission**", and
//! one emission needs a loop, so that clause does not move here and this
//! module does not pretend it does.
//!
//! # It holds no terminal either
//!
//! Everything below renders into a `ratatui::Frame` and reads
//! `tui_textarea::Input`, both backend-agnostic. The terminal itself — raw
//! mode, the alternate screen, the thread that reads crossterm's events, and
//! the panic hook that restores the terminal — is `zaru-cli`'s, because it is
//! the
//! composition root and because [ADR-0003] D8 lets this crate name only
//! `zaru-core`. So every check here drives `ratatui`'s `TestBackend` and reads
//! cells out of the buffer, and a rendered line quoted from one of them is
//! evidence about the mechanism. **It says nothing about whether a person can
//! read the result**, which is [ADR-0005]'s own "someone has to look at it".
//!
//! # The composer's rows are reserved, and that is [ADR-0005] D2
//!
//! D2: "The strip renders below the input and its height changes never reflow
//! the text the user is composing. The cursor does not move because a search
//! result arrived." Inside the composer's own area that holds by construction
//! — ADR-0005's Update records the input as anchored to the top of that area,
//! so no strip height can move it. **In a shell it needs one thing more**: if
//! the host sized the composer's area by [`Composer::height`] and anchored it
//! to the bottom of the screen, a growing strip would push the input row
//! upward, which is exactly the mutant that record's clause 5 was watched red
//! against.
//!
//! So the shell gives the composer a **fixed** area at the bottom, of
//! [`COMPOSER_ROWS`] rows, and the strip grows downward inside it. The input
//! row is then a function of the terminal's size alone.
//! `the_input_row_is_byte_identical_whatever_the_strip_shows_inside_the_shell`
//! asserts it at strip heights of zero, one and six, which is ADR-0005 clause
//! 5 one layer out.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

pub mod command;
pub mod port;
pub mod render;
pub mod wrap;

pub use command::{Command, LEAVE, Refused, Typed};
pub use port::{
    CommandVocabulary, Confirmation, Line, Namespace, Palette, Register, Row, SecretAnswer,
    SecretRequest, TranscriptSource,
};

use crate::composer::{Composer, Entries};
use core::time::Duration;

/// The backend-agnostic keystroke the shell reads, re-exported.
///
/// `tui-textarea` is taken here with `no-backend` on, which is what exposes
/// these two **without** a terminal backend — the same feature that keeps
/// crossterm out of the composer's search tier. They are re-exported because
/// the host translates real terminal events into them, and ADR-0003 D8 lets
/// that host name this crate but not this crate's own dependencies.
pub use tui_textarea::{Input, Key};

/// What the terminal handed over: a keystroke, or a pasted block.
///
/// # Why the paste is a variant here rather than a string of keystrokes
///
/// Without bracketed paste a terminal delivers a paste as the characters it
/// contains, newlines included, and a newline delivered as a keystroke is
/// `Enter` — so pasting three lines ran two turns and left the third in the
/// prompt, which is the look-and-feel survey's row 13. With it armed the
/// terminal frames the block, and the frame is information the shell needs:
/// **the newlines inside it are text and the block is one prompt**, where a
/// newline a person actually typed is still a submission.
///
/// It lives in this crate for the reason [`Input`] and [`Key`] are re-exported
/// from it: the host translates real terminal events into what the shell
/// reads, and [ADR-0003] D8 lets that host name this crate but not this
/// crate's own dependencies. Nothing here knows what an escape sequence is.
///
/// [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Struck {
    /// One key.
    Key(Input),
    /// A block the terminal framed as a paste, with its own newlines.
    Pasted(String),
}

impl From<Input> for Struck {
    fn from(input: Input) -> Self {
        Self::Key(input)
    }
}

/// What the person typed while a turn was running, waiting for it to end.
///
/// # It holds the task words and nothing else
///
/// No clock, because this crate holds none and every method that needs the
/// time takes it. No identifier, because it is not a record — [ADR-0010] D2's
/// seven producers are unchanged and **a queued task reaches no file at all**;
/// it becomes a `Record::Conversation` at the moment it runs, through the same
/// call a typed line takes, and not before. No count, because [ADR-0015]'s
/// 2026-09-13 amendment says exactly one is queued and a second `Enter`
/// replaces it.
///
/// A type rather than a bare `String` so that "the shell is holding a task"
/// cannot be confused with any other string it holds, and so the one authored
/// prefix word is composed in one place.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    /// The line, exactly as it stood in the prompt.
    pub task: String,
}

impl Queued {
    /// A task waiting for the running turn to end.
    #[must_use]
    pub fn of(task: impl Into<String>) -> Self {
        Self { task: task.into() }
    }

    /// The line the pinned row paints: the prefix word, then the task.
    ///
    /// **The task verbatim**, so a person sees what will run rather than a
    /// count or a summary — the reading [ADR-0011] D3's prompt already takes
    /// for the call it is about to make.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    #[must_use]
    pub fn painted(&self) -> String {
        format!("{QUEUED} {}", self.task)
    }
}

/// The one word the pinned row says before the queued task.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-06 and
/// 2026-09-13, open to Jeshua's veto**, in the same shape as the six register
/// glyphs, [`STRIP_ROWS`] and [`crate::composer::NEWLINE`]: no record supplies
/// a word and one is needed, so it is named once here and recorded on
/// [ADR-0015's amendments page] rather than typed at a call site.
///
/// [ADR-0015's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates
pub const QUEUED: &str = "queued";

/// How many rows the strip may occupy inside the composer's reserved area.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-05, open to
/// Jeshua's veto**, and it is a rendering budget rather than a retrieval one.
/// [ADR-0005] sets `MATCH_LIMIT` at eight and the strip can add a ninth line
/// for `keyword only`, so a shell that reserved every row the strip could ever
/// want would spend nine rows of permanent furniture on a surface that is
/// usually collapsed.
///
/// Six is the number that record's own clause-5 check already uses — "strips
/// of zero, one and six entries" — so it is taken from the record rather than
/// invented. A person should set it or confirm it.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
pub const STRIP_ROWS: u16 = 6;

/// How much of the pane the lines it was given keep while an answer streams.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-14, open to
/// Jeshua's veto**, in the same shape as [`STRIP_ROWS`], [`QUEUED`] and the
/// seven register glyphs: no record names a number and one is needed. It is
/// recorded on [ADR-0028]'s Update of that day together with the two
/// alternatives that were rejected and the measurement that decides it.
///
/// # Why a share rather than a row count
///
/// A fixed reservation of six rows is 38% of a 24-row pane and 13% of a
/// 50-row one, and the thing being reserved for — the execution narrative a
/// turn produces — scales with the screen rather than with a widget. So this
/// takes the pane's own height and returns a fraction of it.
///
/// # Why a half
///
/// **It is the only division that asserts nothing about which of the two
/// matters more.** [ADR-0028] D1 has the loop's events "surface as
/// plain-English events **inline in the conversation**", which makes the
/// narrative a peer of the answer rather than a footnote under it, and a
/// number that gave either side more would be this crate deciding a question
/// D1 has already answered the other way.
///
/// It is a **floor and not an allocation**: the lines the pane was given take
/// this many rows only if they have that many, and every row they do not want
/// goes to the answer. In the turn this was measured against, the narrative
/// wanted three rows of the eleven it could have had.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[must_use]
pub const fn transcript_floor(pane: u16) -> u16 {
    pane / 2
}

/// How many rows the composer's area occupies, at the foot of the screen.
///
/// One for the input and [`STRIP_ROWS`] for the strip. **Fixed**, which is
/// what keeps the input row a function of the terminal's size alone — see the
/// module documentation.
pub const COMPOSER_ROWS: u16 = STRIP_ROWS + 1;

/// How the user left.
///
/// # Both exit `0`, and no record names either of them
///
/// [ADR-0016] D5's `0` is "success", and a user who asked to leave and left
/// got what they asked for. Nothing here is a failure, so nothing here reaches
/// another code.
///
/// **The two ways to leave are drafted under a delegated coordinator ruling of
/// 2026-09-05, open to Jeshua's veto.** [ADR-0015] D2's table has ten rows and
/// leaving is not one of them, and no record in the catalogue names an exit
/// key. Both are recorded rather than one chosen: a terminal user reaches for
/// `Ctrl-C` before reading anything, and a user who has read the hint strip
/// reaches for the word.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leaving {
    /// The user typed [`LEAVE`].
    Word,
    /// The user pressed `Ctrl-C`.
    Interrupt,
}

impl Leaving {
    /// Every way to leave, so a check walks them rather than listing them.
    pub const ALL: [Self; 2] = [Self::Word, Self::Interrupt];

    /// What the process exits with. [ADR-0016] D5's `0`, both ways.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Word | Self::Interrupt => 0,
        }
    }
}

/// What the shell wants its host to do next.
///
/// The shell decides nothing about a command's meaning. It reads a line, says
/// which of [ADR-0015] D2's namespaces it named, and hands it over; the host
/// maps it onto the same function the out-of-session spelling calls, which is
/// what D2's "they are one operation" means concretely.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing. The keystroke was absorbed.
    Idle,
    /// Run this in-session command and hand back what it printed.
    Run(Command),
    /// The user asked for work. **Nothing in this workspace can do it**, and
    /// the host refuses it with the same sentence the out-of-session surface
    /// already prints.
    Task(String),
    /// The user is leaving.
    Leave(Leaving),
}

/// What [ADR-0001] D2's status line says.
///
/// D2: "Status line renders the tier at all times. A user must never be
/// uncertain which membrane they are inside." The tier arrives as text rather
/// than as a `Tier`, because that type is `zaru-cli`'s and the words are its
/// record's; composing them here would be a second statement of D1's table.
///
/// # Four records want this one row, and this is the arbitration
///
/// [ADR-0013] D6 — "The status line carries context usage continuously" — and
/// [ADR-0012] D7 — "Per turn in the status line, per session on exit" — both
/// want a number here, and until 2026-09-05 nothing had said how one row
/// carries two. ADR-0012's own proposed Update names the deadlock in as many
/// words: "the status line is ADR-0001 D2's row, which ADR-0013 D6 also wants
/// a number on, with nothing having arbitrated between them". **This type is
/// that arbitration**, settled under a delegated coordinator ruling of
/// 2026-09-05 and open to Jeshua's veto.
///
/// The row is **ordered**, and the order is what makes ADR-0001 D2 structural
/// rather than a rule somebody keeps: the tier is first, so a terminal too
/// narrow to hold the row clips the segments that arrive after it and never
/// the one D2 requires "at all times". Nothing here elides anything by a rule
/// of its own — `ratatui` clips the right edge, and being first is the whole
/// mechanism.
///
/// # The segments arrive rendered, for the reason the tier does
///
/// Every field below is text rather than the number or the enumeration it
/// stands for, by exactly the argument the `tier` field above already makes.
/// ADR-0013 D3's abbreviation, ADR-0012 D7's wording, ADR-0011 D3's three mode
/// names and ADR-0012 D4's resolved identifier are all `zaru-cli`'s, and one
/// of them — `cli::render::usage` — is the very line the session prints on
/// exit. Composing any of them here would be a second statement of a register
/// that already has one, and the two spellings would then be free to disagree
/// about a word.
///
/// # The row is composed against a width, and that is the 2026-09-06 amendment
///
/// Until then this joined every present segment and let `ratatui` clip the
/// right edge, and D2's "at all times" was a consequence of the tier being
/// **first**. That mechanism protects exactly one clause and silently
/// destroys the other: at 40 columns the row read `runtime.tier = bare ·
/// session 01M1SYN1XG` and stopped, so [ADR-0013] D6's number — required
/// "continuously", and "throughout" by its trigger clause 5 — was gone, while
/// a session identifier no record puts on this row had taken the columns it
/// needed. A clip is not an arbitration between two clauses; it is the absence
/// of one, and which clause it protects is a consequence of the order somebody
/// typed.
///
/// So [`Self::painted`] takes the width and drops fields by [`Rank`]. See that
/// type for the order and why it is the records' rather than a taste, and see
/// [ADR-0001's amendments volume 1] for what each of five widths keeps.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0001's amendments volume 1]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers-updates
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The tier, spelled as [ADR-0001] D1 spells it. [`Rank::Tier`].
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    pub tier: String,
    /// The session this shell is inside, per [ADR-0010] D1. [`Rank::Session`].
    ///
    /// **No record puts this on the row**, which is why it is the last rank
    /// and the first thing dropped. It is also never shortened: a ULID's
    /// leading characters are its timestamp, so a prefix does not discriminate
    /// between two sessions started the same minute, and a truncated
    /// identifier cannot be typed into `--resume <id>` — a field that is
    /// present and useless is worse than one that is absent and leaves room
    /// for a field a clause requires.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub session: String,
    /// [ADR-0013] D6's context usage, rendered by the host. [`Rank::Context`].
    ///
    /// `None` before a host supplies one, which is a real state rather than a
    /// placeholder: a shell whose composition could not resolve a provider
    /// holds no context at all, and a zero would be a measurement of nothing.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    pub context: Option<Segment>,
    /// [ADR-0012] D7's token line, rendered by the host. [`Rank::Tokens`].
    ///
    /// `None` until an exchange has happened. `Provider::usage` answers `None`
    /// before the first request for the same reason `providers::usage` refuses
    /// to invent a cost — "a client that had made no request and reported a
    /// zero would be inventing a datum".
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub tokens: Option<Segment>,
    /// [ADR-0028] D5's meter, rendered by the host. [`Rank::Elapsed`].
    ///
    /// `None` except while a turn is running, which is the whole of what this
    /// field says: the number is the turn's own wall clock and there is no
    /// turn to measure at the prompt. What a finished turn took is already on
    /// the pane, in the narrative's own line, so keeping it here afterwards
    /// would be a second rendering of one datum.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    pub elapsed: Option<String>,
    /// Which model is answering. [`Rank::Model`].
    ///
    /// **The resolved identifier and not the alias**, because the question a
    /// person is asking of this field is *which model is answering* and an
    /// alias answers it only for somebody who already knows the resolution.
    /// `None` where the composition could not resolve a provider.
    pub model: Option<String>,
    /// [ADR-0011] D3's permission mode. [`Rank::Mode`].
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub mode: Option<String>,
}

/// One field of the row: what it says, and what it says when the row is narrow.
///
/// # A narrow spelling drops labelling and never a number
///
/// `context 1.2k/1048.5k tokens` becomes `1.2k/1048.5k` and `tokens: 390
/// prompt + 79 completion = 469` becomes `469 tokens`. Both keep every number
/// the full form carries: [ADR-0013] D6's argument is that "approaching is a
/// relation", so a context figure without its window is one nobody can read as
/// near or far, and the token total is the same sum the exit line prints, so
/// the row and that line still cannot disagree about the datum.
///
/// A segment with one spelling is [`Self::same`], and `From<String>` builds
/// one — so a host that has only one wording says so by handing over a
/// `String`.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What the field says when the row has room for it.
    pub full: String,
    /// What it says when the row does not.
    pub narrow: String,
}

impl Segment {
    /// A field with two spellings.
    #[must_use]
    pub fn new(full: impl Into<String>, narrow: impl Into<String>) -> Self {
        Self {
            full: full.into(),
            narrow: narrow.into(),
        }
    }

    /// A field with one spelling, said the same way at every width.
    #[must_use]
    pub fn same(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            narrow: text.clone(),
            full: text,
        }
    }
}

impl From<String> for Segment {
    fn from(text: String) -> Self {
        Self::same(text)
    }
}

impl From<&str> for Segment {
    fn from(text: &str) -> Self {
        Self::same(text)
    }
}

/// The order the row's fields are dropped in, worst first.
///
/// # The order is the records' and not a taste
///
/// Ranks 0 to 3 each answer a clause, ordered by how absolutely the clause is
/// worded: [ADR-0001] D2's "at all times", then [ADR-0013] D6's "continuously"
/// and its clause 5's "throughout", then [ADR-0012] clause 6's "Token counts
/// and cost appear in the status line", then [ADR-0028] D5's "as the work
/// proceeds". **Ranks 4 to 6 answer no clause at all**, which is why they are
/// the three that go first — a field nobody decided should outlive a field a
/// record required is exactly the outcome a clip produces by accident.
///
/// **This is the drop order, not the order the row reads in.** See
/// [`Status::painted`].
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Rank {
    /// [ADR-0001] D2. Never dropped and never narrowed.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    Tier = 0,
    /// [ADR-0013] D6 and its trigger clause 5.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    Context = 1,
    /// [ADR-0012] clause 6.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Tokens = 2,
    /// [ADR-0028] D5, as amended 2026-09-06.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    Elapsed = 3,
    /// No record. Survey row 14.
    Model = 4,
    /// No record. Survey row 14.
    Mode = 5,
    /// No record. The first thing dropped.
    Session = 6,
}

impl Rank {
    /// Every rank, worst first, so a check walks them rather than listing them.
    pub const WORST_FIRST: [Self; 7] = [
        Self::Session,
        Self::Mode,
        Self::Model,
        Self::Elapsed,
        Self::Tokens,
        Self::Context,
        Self::Tier,
    ];
}

/// What the row's fields are separated by. One spelling, used by every segment.
///
/// **Public because a host composing a field has to be able to keep it out.**
/// A field whose text contained this would paint as two, and one of the row's
/// fields — the model — is a string a repository the user cloned can choose,
/// since `model.<alias>` is free at every configuration layer. The row cannot
/// neutralise it here, because a field that legitimately carries the separator
/// also exists: [ADR-0012] D7's token line appends a cost after one. So the
/// rule is the host's and the spelling is this crate's, which is the only way
/// the two cannot disagree.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const SEPARATOR: &str = " · ";

/// What [ADR-0001] D2's tier field says before the tier itself.
///
/// Public for [`SEPARATOR`]'s reason, one step further. A field whose text
/// contained this would put a **second** membrane claim on the one row that
/// record exists to make unambiguous — and a reader scanning for the tier
/// would find two. The row cannot strip it, for the same reason it cannot
/// strip the separator: only the host knows which of its fields a repository
/// the user cloned can choose. So the spelling is this crate's and the rule is
/// the host's, which is the only arrangement in which the two cannot disagree.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
pub const TIER_PREFIX: &str = "runtime.tier = ";

impl Status {
    /// What the row is separated by. One spelling, used by every segment.
    const SEPARATOR: &'static str = SEPARATOR;

    /// A status line carrying [ADR-0001] D2's tier and [ADR-0010] D1's session.
    ///
    /// Every other field starts absent. The signature is deliberately
    /// unchanged from what it was before any of them existed: a session opens
    /// knowing its tier and its identity and nothing else, which is exactly
    /// the state this constructor describes — and it is why a row carrying
    /// none of them is byte-identical to what this printed then.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    #[must_use]
    pub fn new(tier: impl Into<String>, session: impl Into<String>) -> Self {
        Self {
            tier: tier.into(),
            session: session.into(),
            context: None,
            tokens: None,
            elapsed: None,
            model: None,
            mode: None,
        }
    }

    /// The row's fields in **display order**, each with its rank.
    ///
    /// Display order is not [`Rank`] order, and that is deliberate: fields are
    /// dropped by rank and the survivors keep this order, so a field never
    /// moves sideways because another one disappeared. A row whose fields
    /// reshuffled on every resize is a row nobody can read at a glance, which
    /// is the whole purpose a status line serves.
    fn fields(&self) -> Vec<(Rank, &str, &str)> {
        let mut fields: Vec<(Rank, &str, &str)> = Vec::with_capacity(7);
        // The tier's own spelling is composed here rather than stored, and it
        // is the one field with no narrow form: shortening it is what
        // ADR-0001 clause 6's check forbids.
        fields.push((Rank::Tier, "", ""));
        if let Some(model) = self.model.as_deref() {
            fields.push((Rank::Model, model, model));
        }
        if let Some(mode) = self.mode.as_deref() {
            fields.push((Rank::Mode, mode, mode));
        }
        if let Some(context) = self.context.as_ref() {
            fields.push((Rank::Context, &context.full, &context.narrow));
        }
        if let Some(elapsed) = self.elapsed.as_deref() {
            fields.push((Rank::Elapsed, elapsed, elapsed));
        }
        if let Some(tokens) = self.tokens.as_ref() {
            fields.push((Rank::Tokens, &tokens.full, &tokens.narrow));
        }
        fields.push((Rank::Session, "", ""));
        fields
    }

    /// The two fields whose text this type composes rather than is handed.
    fn composed(&self, rank: Rank) -> Option<String> {
        match rank {
            Rank::Tier => Some(format!("{TIER_PREFIX}{}", self.tier)),
            Rank::Session => Some(format!("session {}", self.session)),
            _ => None,
        }
    }

    /// The row at `keep` and narrower, in display order.
    fn joined(&self, fields: &[(Rank, &str, &str)], keep: Rank, narrow: bool) -> String {
        let mut line = String::new();
        for (rank, full, short) in fields {
            if *rank > keep {
                continue;
            }
            if !line.is_empty() {
                line.push_str(Self::SEPARATOR);
            }
            match self.composed(*rank) {
                Some(text) => line.push_str(&text),
                None => line.push_str(if narrow { short } else { full }),
            }
        }
        line
    }

    /// The one line the status bar paints, composed to fit `width`.
    ///
    /// # The rule, stated so it can be falsified
    ///
    /// For the largest rank *k* such that every present field at *k* or better
    /// fits `width` at its full spelling, render those; otherwise retry them
    /// at their narrow spellings; otherwise drop rank *k* and repeat.
    /// **Preferring more fields abbreviated over fewer fields spelled out is
    /// the ruled half**, and it is ruled that way because every field on this
    /// row is a number or a name and none of them needs its label to be read.
    ///
    /// The tier is [`Rank::Tier`] and is therefore never dropped, so a width
    /// too narrow even for its own spelling still leaves `ratatui` clipping a
    /// row that begins with the tier — the one place the old mechanism is
    /// still the mechanism, and the case ADR-0001 clause 6's check already
    /// covers at width 10.
    ///
    /// A field that is `None` contributes nothing at all, separator included,
    /// so a row carrying none of them is byte-identical to what this printed
    /// before any of them existed.
    #[must_use]
    pub fn painted(&self, width: u16) -> String {
        let fields = self.fields();
        let budget = usize::from(width);
        for keep in Rank::WORST_FIRST {
            for narrow in [false, true] {
                let line = self.joined(&fields, keep, narrow);
                if crate::shell::wrap::columns(&line) <= budget {
                    return line;
                }
            }
        }
        // Narrower than `runtime.tier = <tier>` itself. The tier is rendered
        // anyway and the right edge is clipped, which is D2's "at all times"
        // for a terminal that cannot hold even one field.
        self.joined(&fields, Rank::Tier, false)
    }
}

/// Which keystroke leaves, if this one does.
///
/// # One key, one meaning, declared in one place
///
/// [`Shell::key`] calls this, and so does a host that reads a keystroke while
/// a turn is running — where the shell is not the thing deciding what to do
/// with the key, because the shell is not what the turn is waiting on. Both
/// need the same answer, and `ctrl` plus `c` spelled at two call sites is the
/// rule-in-two-places that nothing keeps agreeing.
///
/// **A mid-turn `Ctrl-C` therefore leaves, exactly as one at the prompt
/// does.** [ADR-0015]'s ruling of 2026-09-05 gives this key one meaning and
/// [ADR-0016] D5's `0` for both; what makes it also an *interruption* is what
/// the host does with the turn it was running, which is
/// [ADR-0010](https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript)
/// D2's business and not this crate's.
///
/// [`LEAVE`] is the other way out and is not here: it is a *line*, read by
/// [`command::read`] after `Enter`, and a word is not a keystroke.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
pub fn leaves(input: &Input) -> Option<Leaving> {
    (input.ctrl && input.key == Key::Char('c')).then_some(Leaving::Interrupt)
}

/// A terminal session.
///
/// Synchronous and clockless in the same way [`Composer`] is: every method
/// that needs the time takes it, so there is nothing here that could read the
/// machine's clock.
#[derive(Debug)]
pub struct Shell {
    composer: Composer,
    status: Status,
    /// What [`TranscriptSource`] last said, per [ADR-0010] D2.
    transcript: Vec<Line>,
    /// Lines this session produced that no transcript holds — a refusal, a
    /// command's output. Kept apart from `transcript` so a refresh cannot
    /// lose them and so nothing here can be mistaken for the record on disk.
    notices: Vec<Line>,
    /// The answer being streamed, painted below everything else until the
    /// turn ends. See [`Shell::stream_delta`].
    streaming: Option<String>,
    /// The one question standing, of either kind.
    ///
    /// **One field rather than two**, so two questions cannot stand at once.
    /// A `Shell` holding both a confirmation and a secret request would have
    /// to decide which keystrokes belong to which, and there is no answer to
    /// that question that is not a rule somebody has to remember.
    standing: Option<Standing>,
    answered: Option<bool>,
    secret_answered: Option<SecretAnswer>,
    /// What was typed at a [`SecretRequest`], from `Enter` until it is taken.
    ///
    /// `Some` only after [`SecretAnswer::Given`] and only until
    /// [`Shell::take_secret`] takes it. A declined question leaves it `None`,
    /// which is what makes "a declined question stores nothing" a property of
    /// the type rather than of a caller.
    secret_given: Option<String>,
    /// The one task waiting for the running turn to end, if there is one.
    queued: Option<Queued>,
}

impl Shell {
    /// Open a shell over a status line.
    #[must_use]
    pub fn open(status: Status) -> Self {
        Self {
            composer: Composer::new(),
            status,
            transcript: Vec::new(),
            notices: Vec::new(),
            streaming: None,
            standing: None,
            answered: None,
            secret_answered: None,
            secret_given: None,
            queued: None,
        }
    }

    /// Read the transcript again.
    ///
    /// [ADR-0010] D2 makes the transcript append-only, so this replaces rather
    /// than merges: the source is the authority on what the record holds.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    pub fn refresh(&mut self, source: &dyn TranscriptSource) {
        self.transcript = source.lines();
    }

    /// Add a line this session produced.
    pub fn notice(&mut self, line: Line) {
        self.notices.push(line);
    }

    /// The composer, for a host that needs to hand it a search response.
    #[must_use]
    pub const fn composer(&self) -> &Composer {
        &self.composer
    }

    /// The composer, mutably, for the same reason.
    pub const fn composer_mut(&mut self) -> &mut Composer {
        &mut self.composer
    }

    /// The status line.
    #[must_use]
    pub const fn status(&self) -> &Status {
        &self.status
    }

    /// Put [ADR-0013] D6's context usage on the row, or take it off.
    ///
    /// # Why this and not a `&mut Status`
    ///
    /// [ADR-0001] D2 ends "Tier is resolved at session start and is immutable
    /// for the life of a session. Changing it starts a new session. A membrane
    /// that can be dropped mid-session is not a membrane." A `status_mut`
    /// would hand every caller the power that sentence forbids, and the
    /// immutability would then be a rule somebody keeps rather than a shape.
    /// Two setters that can reach only the two segments a turn produces leave
    /// the tier and the session with no mutation surface at all, which is the
    /// same discipline [`Status`]'s own ordering uses for D2's other half.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    pub fn set_context_usage(&mut self, rendered: Option<impl Into<Segment>>) {
        self.status.context = rendered.map(Into::into);
    }

    /// Put [ADR-0012] D7's token line on the row, or take it off.
    ///
    /// See [`Self::set_context_usage`] for why these are setters rather than a
    /// borrow of the whole row.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub fn set_token_usage(&mut self, rendered: Option<impl Into<Segment>>) {
        self.status.tokens = rendered.map(Into::into);
    }

    /// Put [ADR-0028] D5's meter on the row, or take it off at the turn's end.
    ///
    /// See [`Self::set_context_usage`] for why this is a setter. It is a
    /// separate one from the pair above for a reason of its own: those two
    /// change at a turn **boundary** and this one changes on every beat of the
    /// turn, so a single call that wrote all three would have to recompute two
    /// numbers that cannot have changed — and [ADR-0013] D7's "compaction
    /// happens at turn boundaries only" is exactly the rule such a call would
    /// be quietly asserting against.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    pub fn set_elapsed(&mut self, rendered: Option<String>) {
        self.status.elapsed = rendered;
    }

    /// Say which model is answering and which permission mode is in force.
    ///
    /// **One call for both, and called once.** Both are resolved once for the
    /// life of a session — [ADR-0012] D4's alias resolution and [ADR-0011]
    /// D3's mode are each `Prepared`'s, fixed before the first turn — so
    /// neither has anything to update and a second call would be describing a
    /// change that cannot happen. `None` for both is the real state of a
    /// session whose composition could not resolve a provider.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub fn describe(&mut self, model: Option<String>, mode: Option<String>) {
        self.status.model = model;
        self.status.mode = mode;
    }

    /// Add to the answer being streamed, painting it as it arrives.
    ///
    /// # A provisional line, cleared rather than promoted
    ///
    /// This is what a reader watches grow while the model is answering. It is
    /// **not** the answer: when the turn ends, [`Self::clear_streaming`]
    /// takes it away and the turn's own rendered lines are added through
    /// [`Self::notice`] like every other line the session produced.
    ///
    /// **Cleared rather than promoted, and that is the whole design.**
    /// Keeping this line and having the turn omit its answer would make the
    /// answer's bytes come from two places — this accumulator on the streamed
    /// path, and the renderer everywhere else — and two renderings of one
    /// answer are two things that can disagree about a word. Clearing costs
    /// one repaint of text the reader has already read; the alternative costs
    /// a second source of truth for what the model said.
    ///
    /// So what a reader sees is text that grows across beats and then stops
    /// growing, never text that appears twice.
    pub fn stream_delta(&mut self, text: &str) {
        self.streaming
            .get_or_insert_with(String::new)
            .push_str(text);
    }

    /// Take the streamed answer off the pane, at the end of the turn.
    ///
    /// Idempotent: a turn that streamed nothing clears nothing.
    pub fn clear_streaming(&mut self) {
        self.streaming = None;
    }

    /// What is being streamed right now, if anything.
    #[must_use]
    pub fn streaming(&self) -> Option<&str> {
        self.streaming.as_deref()
    }

    /// Hold a task until the running turn ends.
    ///
    /// **Replaces**, and that is [ADR-0015]'s 2026-09-13 amendment in one
    /// word: exactly one task is queued, and a second `Enter` while one waits
    /// puts the prompt's text in its place rather than adding to a list.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub fn queue(&mut self, task: Queued) {
        self.queued = Some(task);
    }

    /// The task waiting, if one is.
    #[must_use]
    pub const fn queued(&self) -> Option<&Queued> {
        self.queued.as_ref()
    }

    /// Take the task waiting, leaving none.
    ///
    /// Taking it is what empties it, so a host that drains the queue cannot
    /// run the same task twice however it is written — the shape
    /// `Pending::tell_once` already uses one crate over.
    pub fn take_queued(&mut self) -> Option<Queued> {
        self.queued.take()
    }

    /// The task waiting, mutably, so an interruption can discard it.
    ///
    /// Narrower than a `&mut Shell` deliberately: `terminal::driver::after` is
    /// the one place an interruption's meaning is decided and the one place a
    /// check can drive it without a provider or a key, and it wants this and
    /// nothing else.
    pub const fn queued_mut(&mut self) -> &mut Option<Queued> {
        &mut self.queued
    }

    /// Everything the pane was **given**, oldest first: the transcript, then
    /// this session's own notices.
    ///
    /// # Why the streamed line is not one of them
    ///
    /// Every line here is a line somebody handed the shell and expects to stay
    /// handed over — a transcript record, a refusal, a command's output, one
    /// of [`Shell::notice`]'s three callers in the driver. The streamed answer
    /// is the opposite: it is provisional, it is replaced rather than kept,
    /// and [`Shell::clear_streaming`] takes it away at the end of the turn.
    ///
    /// The two are separable because the renderer paints them in **two
    /// regions** when the pane cannot hold both. That split is
    /// `render`'s `pane_and_stream`, named in prose rather than linked because
    /// it is private and rustdoc is right to refuse a public page pointing at
    /// something a reader of that page cannot open — the same reason
    /// `PaneNarrator` names `vocabulary::loop_line` in prose one crate over.
    /// A tail taken over the two together shows
    /// the newest rows of whichever one is growing, which is the answer, every
    /// time; so a line the pane was given would be visible for exactly as long
    /// as the answer was short.
    #[must_use]
    pub fn given_lines(&self) -> Vec<Line> {
        let mut lines = self.transcript.clone();
        lines.extend(self.notices.iter().cloned());
        lines
    }

    /// The answer still arriving, as the line the pane would paint it as.
    ///
    /// `None` when nothing is streaming, which is every moment outside a turn
    /// and every turn whose model asked for a tool and said nothing.
    #[must_use]
    pub fn streamed_line(&self) -> Option<Line> {
        self.streaming
            .as_ref()
            .map(|streaming| Line::new(Register::Plain, streaming.clone()))
    }

    /// Everything the pane would show, oldest first: the transcript, then this
    /// session's own notices, then the answer still arriving.
    ///
    /// The streamed line is **last** because it is the newest thing on the
    /// pane and because it is the only line that will be replaced rather than
    /// kept — anywhere else, the lines below it would shift as it grew.
    ///
    /// **Composed from [`Self::given_lines`] and [`Self::streamed_line`]
    /// rather than assembled a second time**, so the two halves and the whole
    /// cannot come to disagree about what the pane holds. That is the same
    /// discipline the crate applies everywhere a vocabulary is handed across
    /// rather than retyped, and it is what
    /// `the_pane_lines_are_exactly_what_the_pane_was_given_and_what_is_arriving`
    /// asserts.
    #[must_use]
    pub fn pane_lines(&self) -> Vec<Line> {
        let mut lines = self.given_lines();
        lines.extend(self.streamed_line());
        lines
    }

    /// Put a question to the user. [ADR-0011] D3's `ask`.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn ask(&mut self, question: Confirmation) {
        self.standing = Some(Standing::Confirm(question));
        self.answered = None;
    }

    /// The confirmation standing, if one is.
    #[must_use]
    pub const fn asking(&self) -> Option<&Confirmation> {
        match &self.standing {
            Some(Standing::Confirm(question)) => Some(question),
            _ => None,
        }
    }

    /// Ask the user for a secret, per [ADR-0011] D3's 2026-09-14 amendment.
    ///
    /// The second question kind, and **not a second confirmer** — see
    /// [`SecretRequest`]. [ADR-0015] D2's `/providers keys add <kind>` is what
    /// needs it: [ADR-0007] D7 reads the key from standard input, and a
    /// terminal in raw mode has none to hand it.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub fn ask_secret(&mut self, request: SecretRequest) {
        self.standing = Some(Standing::Secret(request, String::new()));
        self.secret_answered = None;
        self.secret_given = None;
    }

    /// The secret question standing, if one is.
    ///
    /// **The request and never the bytes.** There is no accessor on this type
    /// that yields what has been typed so far, which is what makes "no byte of
    /// a secret reaches a frame" a property of the shell's surface rather than
    /// of every renderer's discipline: a renderer cannot paint what it cannot
    /// reach. The count is reachable, because the mask is one glyph per
    /// character — see [`crate::shell::render::MASK`], where the
    /// length disclosure that buys is recorded.
    #[must_use]
    pub const fn asking_secret(&self) -> Option<&SecretRequest> {
        match &self.standing {
            Some(Standing::Secret(request, _)) => Some(request),
            _ => None,
        }
    }

    /// How many characters have been typed at the standing secret question.
    ///
    /// For the renderer, which paints that many mask glyphs and nothing else.
    #[must_use]
    pub fn secret_len(&self) -> usize {
        match &self.standing {
            Some(Standing::Secret(_, typed)) => typed.chars().count(),
            _ => 0,
        }
    }

    /// How the secret question ended, once it has.
    #[must_use]
    pub const fn secret_answer(&self) -> Option<SecretAnswer> {
        self.secret_answered
    }

    /// Take what was typed at the secret question.
    ///
    /// **The one accessor that yields a typed secret**, named so that one
    /// search finds every call site — the discipline `zaru-cli`'s
    /// `Secret::expose_for_dispatch` already uses, stated here for the same
    /// reason. It **takes**: a second call answers `None`, so nothing can read
    /// the value twice and no copy is left behind in the shell for a later
    /// frame to reach.
    ///
    /// `None` when the question was declined, when it has not been answered,
    /// and when it has already been taken. A caller distinguishes the first
    /// from the rest with [`Self::secret_answer`].
    pub fn take_secret(&mut self) -> Option<String> {
        self.secret_given.take()
    }

    /// What the user answered, once they have.
    ///
    /// `None` while a question stands **and** when none was asked, which is
    /// why a host asks and then pumps until this is `Some` rather than reading
    /// it speculatively.
    #[must_use]
    pub const fn answer(&self) -> Option<bool> {
        self.answered
    }

    /// Whether a question of either kind is standing.
    const fn questioned(&self) -> bool {
        self.standing.is_some()
    }

    /// Apply one keystroke at `now`.
    ///
    /// # A standing question takes every key
    ///
    /// [ADR-0011] D3's `ask` mode "prompts before any write or command", and a
    /// prompt the user can type past is not a prompt. So while a question
    /// stands the composer receives nothing.
    ///
    /// # A secret question takes every key too, and its table is not this one
    ///
    /// [ADR-0011] D3's 2026-09-14 amendment gives the shell a second question
    /// kind, and its keys are: `Enter` completes, `Esc` **and `Ctrl-C`**
    /// decline, `Backspace` removes one character, a printable character is
    /// taken, and everything else leaves the question standing. **`Ctrl-C`
    /// declining is the one deliberate divergence from the table below**,
    /// where it is ignored: everywhere else in this shell `ctrl` plus `c`
    /// means *stop*, and a person who changes their mind halfway through
    /// typing an API key should not lose the session for it. Declining a
    /// question is what stopping one is, so no third meaning is invented.
    ///
    /// # The default is decline, and only `y` is not
    ///
    /// `y` accepts. `n`, `Esc` and `Enter` decline — `Enter` being the default
    /// answer, which is why the prompt renders `[y/N]`. **Every other key is
    /// ignored and the question stays**, rather than declining: a stray
    /// keystroke that silently answered would make the prompt's outcome depend
    /// on something the user did not mean, and the safe direction is to keep
    /// asking. [ADR-0011] D6 gives the harness no veto and this gives it no
    /// accidental one either.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn key(
        &mut self,
        input: Input,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) -> Action {
        if let Some(Standing::Secret(_, typed)) = &mut self.standing {
            match input.key {
                Key::Enter => {
                    let given = core::mem::take(typed);
                    self.resolve_secret(SecretAnswer::Given, Some(given));
                }
                Key::Esc => self.resolve_secret(SecretAnswer::Declined, None),
                Key::Char('c' | 'C') if input.ctrl => {
                    self.resolve_secret(SecretAnswer::Declined, None);
                }
                Key::Backspace => {
                    typed.pop();
                }
                Key::Char(character) if !input.ctrl && !input.alt => typed.push(character),
                _ => {}
            }
            return Action::Idle;
        }

        if self.asking().is_some() {
            match input.key {
                Key::Char('y' | 'Y') => self.resolve(true),
                Key::Char('n' | 'N') | Key::Esc | Key::Enter => self.resolve(false),
                _ => {}
            }
            return Action::Idle;
        }

        // Ctrl-C leaves from anywhere, including mid-line and mid-turn. The
        // rule is `leaves`, so this is its one caller inside the shell rather
        // than a second spelling of it.
        if let Some(leaving) = leaves(&input) {
            return Action::Leave(leaving);
        }

        if input.key != Key::Enter {
            self.composer.key(input, now, entries);
            return Action::Idle;
        }

        let line = self.composer.text();
        self.composer = Composer::new();
        self.submit(&line, vocabulary)
    }

    /// Apply whatever the terminal handed over at `now`.
    ///
    /// One entry point for both kinds, so a host reading a source does not
    /// decide what a paste means — it hands over what arrived.
    pub fn struck(
        &mut self,
        struck: Struck,
        now: Duration,
        entries: &dyn Entries,
        vocabulary: &dyn CommandVocabulary,
    ) -> Action {
        match struck {
            Struck::Key(input) => self.key(input, now, entries, vocabulary),
            Struck::Pasted(text) => {
                self.pasted(&text, now, entries);
                Action::Idle
            }
        }
    }

    /// Put a pasted block into the prompt at `now`.
    ///
    /// **A paste is never a submission.** Its newlines are text, so the block
    /// waits in the prompt for the `Enter` that submits all of it as one — the
    /// 2026-09-13 amendment to [ADR-0005] D1 and D2.
    ///
    /// **A standing question absorbs it**, exactly as [`Self::key`] has a
    /// question absorb every key but five: [ADR-0011] D3's `ask` "prompts
    /// before any write or command", and a prompt a user can paste past is no
    /// more a prompt than one they can type past.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub fn pasted(&mut self, text: &str, now: Duration, entries: &dyn Entries) {
        // **A secret question takes it**, where a confirmation absorbs it, and
        // the two rules are consistent rather than in tension. The failure a
        // confirmation guards against is a `[y/N]` answered by a block of text
        // nobody read; this question has no such failure and a paste is how an
        // API key actually arrives. ADR-0011 D3's 2026-09-14 amendment.
        if let Some(Standing::Secret(_, typed)) = &mut self.standing {
            typed.push_str(text);
            return;
        }
        if self.questioned() {
            return;
        }
        self.composer.paste(text, now, entries);
    }

    /// Read one composed line as [ADR-0015] D2's grammar reads it.
    ///
    /// # One path, and that is what stops a second grammar existing
    ///
    /// [`Self::key`]'s `Enter` arm calls this, and so does the host draining a
    /// task queued during a turn — the 2026-09-13 amendment on
    /// [ADR-0015's amendments page]. So a queued line naming a namespace runs
    /// that command, a queued [`LEAVE`] leaves, a queued blank line does
    /// nothing, and a queued anything-else is the next turn: the queue is not
    /// a third entry point, it is the same one deferred by a turn.
    ///
    /// **It does not touch the composer.** Clearing the prompt belongs to the
    /// caller that took the line out of it, and a drain has no prompt to
    /// clear.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    /// [ADR-0015's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates
    pub fn submit(&mut self, line: &str, vocabulary: &dyn CommandVocabulary) -> Action {
        match command::read(line, vocabulary) {
            Typed::Nothing => Action::Idle,
            Typed::Leave => Action::Leave(Leaving::Word),
            Typed::Task(task) => Action::Task(task),
            Typed::Command(command) => Action::Run(command),
            Typed::Refused(refusal) => {
                self.notice(Line::new(Register::Failed, refusal.to_string()));
                Action::Idle
            }
        }
    }

    fn resolve(&mut self, answer: bool) {
        self.standing = None;
        self.answered = Some(answer);
    }

    fn resolve_secret(&mut self, answer: SecretAnswer, given: Option<String>) {
        self.standing = None;
        self.secret_answered = Some(answer);
        self.secret_given = given;
    }
}

/// The one question standing, of either kind.
///
/// Private, and an enum rather than two `Option` fields, so the invariant
/// "at most one question stands" is the type's rather than a caller's.
#[derive(Debug)]
enum Standing {
    /// [ADR-0011] D3's confirmation.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    Confirm(Confirmation),
    /// A secret being typed: the question, and what has been typed so far.
    ///
    /// **The second field is the only place in this crate a typed secret
    /// lives**, it is reachable through no public accessor, and
    /// [`Shell::take_secret`] moves it out rather than copying it.
    Secret(SecretRequest, String),
}

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests;
