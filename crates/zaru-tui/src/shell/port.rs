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
/// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
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
    pub const ALL: [Self; 6] = [
        Self::Plain,
        Self::Call,
        Self::Announced,
        Self::Succeeded,
        Self::Exhausted,
        Self::Failed,
    ];

    /// The glyph that opens a line in this register.
    ///
    /// # Three of these are the records' and three are drafted
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
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Self::Plain => " ",
            Self::Call => "·",
            Self::Announced => "◈",
            Self::Succeeded => "✓",
            Self::Exhausted => "⊘",
            Self::Failed => "✗",
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
