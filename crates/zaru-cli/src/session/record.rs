// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What one line of [ADR-0010] D2's transcript is.
//!
//! D2: "Every event from [ADR-0008] D3 is written as it occurs, alongside
//! user messages, tool calls, SEAL verdicts, attachments, and learning
//! announcements."
//!
//! # A producer is a variant, never a string
//!
//! **Nine producers exist in this workspace as of 2026-09-15**: `zaru-core`'s
//! [`Event`], its outer-loop [`TurnEvent`], [ADR-0011] D4's transcript entry,
//! [ADR-0016] D1's five classes, [ADR-0013] D2's compaction, a line this
//! session says once and never again, D2's own **user messages** arriving
//! beside the answer they were answered by, the turn that failed, and
//! [ADR-0015] D6's **attribution** for a command that contributed to a turn.
//! Each is a variant of [`Record`], so a tenth producer is a variant and every
//! match over the enum fails to compile rather than a `kind` string being
//! invented at a call site — the same closed-enum discipline
//! [`Class`](crate::failure::Class), [`Layer`](crate::config::Layer) and
//! [`ToolName`](crate::tools::ToolName) already carry in this crate.
//!
//! *This paragraph read "Eight producers are named above and seven exist"
//! until 2026-09-15 and was false twice over: the eighth,
//! [`Record::Failure`], gained its producer on 2026-09-14, and the ninth
//! arrived with the commands that produce it. Corrected rather than annotated,
//! which is what a comment gets.*
//!
//! **The producers that do not exist get no variant at all.** SEAL verdicts
//! and attachments belong to records that are unbuilt, and a
//! variant whose condition nothing can satisfy is a permanent exemption
//! dressed as a promise ([Verification lessons] §7). Absence is what makes the
//! gap findable — which is exactly how the compaction variant arrived: it was
//! absent while nothing compacted, and it is here because something does.
//!
//! **One variant here was that exemption, arrived at from the other
//! direction, and it is not one any more.** [`Record::Failure`] was
//! constructed nowhere in the product tree until 2026-09-14 — every refusal
//! inside [`crate::compose::turn::run_one`] became printed lines and an exit
//! code, so a turn that failed reached no file at all and D5's "every byte"
//! was false for exactly the turns a reader would most want back. It was
//! found 2026-09-06 while [`Record::Conversation`] was being added and closed
//! by the `first-run` arc, which made `run_one` a wrapper that records the
//! classified failure once on `Exit::Failed`. The paragraph is kept, in the
//! past tense, because the rule above it is what the defect falsified and the
//! example is what makes the rule readable.
//!
//! # The compaction record is what makes D2's "history is preserved" true
//!
//! [ADR-0013] D2: "the oldest span of layer 6 is replaced by a generated
//! summary, and **the raw span stays in the transcript**. History is
//! preserved on disk; only the model's view is compacted." That sentence is a
//! claim about this file and nothing else, so [`Record::Compacted`] carries
//! the span **verbatim and unredacted** — the same rule that keeps every other
//! line here raw, stated once more because a span is the one place a reader
//! might expect the model's view rather than the session's.
//!
//! It carries the announcements too. D3 emits one "once, with what it cost",
//! and D2 of this record makes the transcript replayable — "re-rendering it
//! reproduces what the user saw" — so a line the user was shown that the file
//! does not hold would break replay for the one event whose whole purpose is
//! that the user not be left wondering what happened.
//!
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//!
//! # A line said once is a record here, and that is the same argument again
//!
//! [ADR-0011] D2 states "once at session start" that `bare` is not a sandbox;
//! [ADR-0002] D8's event-anchored recommendation — which is [ADR-0009] D4's
//! missing-validators line — "fires at most once ever". Both carriers are
//! rebuilt when a **process** opens, so before [`Record::Said`] there was
//! nothing on disk saying either had been said, and a session resumed a
//! second time said both again.
//!
//! **The counter belongs here rather than anywhere else, and two records say
//! so.** ADR-0002's Status tracking rules that "the transcript is the
//! session's memory and is where that counter belongs — a line that is
//! already a record in a session's transcript is not appended to it again".
//! And D2 of this record makes the transcript "a replayable record:
//! re-rendering it reproduces what the user saw" — which is the same sentence
//! that put D3's compaction announcement inside [`Record::Compacted`] two
//! sections above, applied to two more lines the user was shown and the file
//! did not hold.
//!
//! It is a **sixth producer**, accepted 2026-09-05 under directive 20 as an
//! Update to ADR-0010 D2 and open to Jeshua's veto: D2's own producer list
//! ends at "learning announcements", which are ADR-0002 D4's and D5's, and
//! neither of these two lines is one.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//!
//! # The tool call is stored as its rendered line, and that is D2's own claim
//!
//! [`TranscriptEntry`] has private fields and
//! derives no `Serialize`, and **nothing in `tools/` is changed here**. What
//! is stored is what that type's own public door yields: `render()` — which
//! ADR-0011 D4 calls "the line a transcript shows", produced in exactly one
//! place so the prompt and the record cannot describe one call two ways —
//! together with D4's out-of-tree class and D6's destructive annotation.
//!
//! That is the better fit rather than the concession. D2's replayability
//! claim is that "re-rendering it reproduces what the user saw", and
//! `render()`'s output **is** what the user saw. A checkpoint would need the
//! struct; a transcript needs the line.
//!
//! # A tool call is two records, and that is what makes `Interrupted` possible
//!
//! [ADR-0010] D4 requires an interrupted call to be recorded as `Interrupted`.
//! **A process that has been killed writes nothing**, so that record cannot
//! be appended at the moment of interruption. What is appended is the pair: a
//! [`Phase::Started`] before the call and a [`Phase::Completed`] after it, and
//! on resume a `Started` with no matching `Completed` **is** the
//! interruption. D4 leaves the two framings open and they are the same
//! mechanism from two ends; the reader's end is the only one a killed process
//! can honour.
//!
//! The phase sits beside the entry's data rather than inside it, so D4's "the
//! record is the same at every mode" is undisturbed — a phase is not a mode.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::failure::{Classified, Presentation};
use crate::tools::TranscriptEntry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zaru_core::context::Compaction;
use zaru_core::iteration::Event;
use zaru_core::tool_call::Event as TurnEvent;

/// Where a tool call had got to when this line was written.
///
/// # Three, and the third is a defect this arc found rather than a widening
///
/// A refused call fits neither of the first two. Writing `Started` and then
/// `Completed` for it says it ran, which is untrue. Writing `Started` alone
/// and stopping there makes [`resume`](crate::session::resume::resume) report a call
/// the user *declined* as a call that was **interrupted** — so a resumed
/// session would tell the model an action it consciously refused did not
/// complete, and ADR-0010 D4's whole point is that the model is told the
/// truth about what was in flight. Writing nothing at all would break
/// ADR-0011 D4's "mode may remove the prompt; it never removes the record".
///
/// So there is a third phase. It is not a mode — ADR-0011 D4's record is the
/// same at every mode and this is orthogonal to that — and it is recorded as
/// a proposed Update on ADR-0010 D4 under a **delegated coordinator ruling of
/// 2026-09-04**, open to Jeshua's veto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The harness is about to act. Written **before** the call.
    Started,
    /// The call returned. Written after it.
    Completed,
    /// The call did not act, because the user declined or nobody could be
    /// asked. Written instead of [`Phase::Completed`], and it closes the
    /// pair exactly as a completion does.
    Refused,
}

/// [ADR-0011] D4's record of one tool call, as a transcript keeps it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    /// ADR-0011 D4's line, exactly as the prompt rendered it.
    pub line: String,
    /// D4's out-of-tree class, which renders differently at every mode.
    pub out_of_tree: bool,
    /// D6's destructive annotation.
    pub destructive: bool,
    /// Whether the call had returned when this line was written.
    pub phase: Phase,
}

impl ToolCall {
    /// The line written **before** the call acts.
    #[must_use]
    pub fn started(entry: &TranscriptEntry) -> Self {
        Self::at(entry, Phase::Started)
    }

    /// The line written after it returns.
    #[must_use]
    pub fn completed(entry: &TranscriptEntry) -> Self {
        Self::at(entry, Phase::Completed)
    }

    /// The line written when the call did not act.
    ///
    /// ADR-0011 D6 gives the harness no veto, so the only ways here are that
    /// the user said no or that there was nobody to ask. Under the ADR-0016
    /// ruling of 2026-09-04 neither is a failure, which is why this is a
    /// phase of the call's own record rather than a
    /// [`Record::Failure`].
    #[must_use]
    pub fn refused(entry: &TranscriptEntry) -> Self {
        Self::at(entry, Phase::Refused)
    }

    fn at(entry: &TranscriptEntry, phase: Phase) -> Self {
        Self {
            line: entry.render(),
            out_of_tree: entry.is_out_of_tree(),
            destructive: entry.is_destructive(),
            phase,
        }
    }
}

/// [ADR-0016] D1's classified failure, as a transcript keeps it.
///
/// Stored as the projection rather than as the `Classified` itself, for the
/// reason the tool call is stored as its line: a transcript is a record of
/// what the user was shown, and
/// [`Presentation`] is what a renderer shows.
/// It also means the transcript carries no `PathBuf` and no `Tier`, so the
/// stored shape does not move when those types do.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureLine {
    /// Which of D1's five this was, as that record spells it.
    pub class: String,
    /// The one line saying what happened.
    pub headline: String,
    /// Everything under it.
    pub lines: Vec<String>,
}

impl FailureLine {
    /// Project a classified failure into what a transcript keeps.
    #[must_use]
    pub fn of(classified: &Classified) -> Self {
        let presentation = Presentation::of(classified);
        Self {
            class: presentation.class.as_str().to_owned(),
            headline: presentation.headline,
            lines: presentation
                .lines
                .iter()
                .map(crate::failure::Line::flattened)
                .collect(),
        }
    }
}

/// Which of the two once-ever lines a [`Record::Said`] is about.
///
/// **Two variants and not a string, because the two are decided by two
/// different rules.** [ADR-0011] D2's notice is a property of a *tier* — the
/// sentence is false anywhere there is a membrane, so the tier is re-read
/// every process and this record only says the statement was made. [ADR-0002]
/// D8's recommendation is a *line in the pane* — what "already in the
/// transcript" means for it is literally that this line was shown. A reader
/// that could not tell them apart would decide both by one rule, which is the
/// shape ADR-0002's own Status tracking names as "two rules in one place".
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaidOnce {
    /// [ADR-0011] D2's not-a-sandbox notice, stated "once at session start".
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    Notice,
    /// [ADR-0002] D8's event-anchored recommendation, which "fires at most
    /// once ever" and is [ADR-0009] D4's missing-validators line.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    Recommendation,
}

/// A line this session has said and will not say again.
///
/// The text is carried for the reason [`Record::Compacted`] carries its
/// announcements: D2's replayability claim is that "re-rendering it
/// reproduces what the user saw", and a line the user was shown that the file
/// does not hold breaks it. The [`SaidOnce`] is what the *decision* is made
/// from; the text is what a re-rendering needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Said {
    /// Which of the two lines this was.
    pub line: SaidOnce,
    /// The sentence, exactly as the reader was shown it.
    pub text: String,
}

/// Who said one half of a turn's conversation.
///
/// **Two variants and not a boolean**, for the reason [`SaidOnce`] is two
/// variants rather than a string: a reader of the file has to be able to tell
/// the person's words from the harness's without a convention, and the two
/// halves are written at different moments by different code with different
/// rules about what may be missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    /// The person, in the words they typed.
    User,
    /// The harness, in the words it rendered.
    Zaru,
}

impl Voice {
    /// How a rendering names this voice.
    ///
    /// **One spelling, two callers, and neither of them invented it.**
    /// [`crate::compose::boundary::exchange_of_turn`] has composed a turn's
    /// two halves as `user: <task>` and `zaru: <answer>` since layer 6 gained
    /// its shape, and those are the product's existing words for exactly this
    /// distinction — so the pane reads them from here rather than a second
    /// pair being chosen for the screen. The alternative was a glyph, which
    /// would have been authored: [`Register`](zaru_tui::shell::port::Register)
    /// gives plain narration the absence of a marker, and three of its six
    /// glyphs are already drafted proposals because no record names one.
    #[must_use]
    pub const fn spoken_as(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Zaru => "zaru",
        }
    }
}

/// One half of one turn's conversation.
///
/// # The seventh producer, and half of it was licensed all along
///
/// [ADR-0010] D2's producer list reads "Every event from [ADR-0008] D3 is
/// written as it occurs, **alongside user messages**, tool calls, SEAL
/// verdicts, attachments, and learning announcements" — so the [`Voice::User`]
/// half is a producer this record named on the day it was written and that
/// nothing had built, which the module documentation above says in as many
/// words. The [`Voice::Zaru`] half is the one no clause names, and it is the
/// decision of an accepted Update of 2026-09-06 under directives 20 and 25,
/// open to Jeshua's veto. D2 supplies its argument too: the transcript is "a
/// replayable record: re-rendering it reproduces what the user saw", and an
/// answer the user was shown that the file does not hold breaks that.
///
/// # A `user` with no `zaru` after it is the interruption
///
/// The two halves are written around the loop rather than together — see
/// [`crate::compose::turn::run_one`] — and nothing marks an interruption,
/// for the reason [`Phase`] carries three variants and no fourth: **a killed
/// process writes nothing**, so the pair with no closer is the only marker a
/// reader can be given. A turn that *stopped* stays distinguishable because
/// it has a `turn_ended` record; a turn that was interrupted has none.
///
/// # These two strings are redacted where every other record here is raw
///
/// [ADR-0010]'s Negative section says the transcript "contains whatever the
/// session contained, **including secrets that appeared in command output**",
/// and that stays true: [`ToolCall::line`], a refusal's sentence and
/// [`Record::Compacted`]'s span are all verbatim. [ADR-0008] clause 6's port
/// is a different thing — it is over values **the harness itself holds**,
/// that record saying "a secret the harness never held is not redacted,
/// because nothing pattern-based was adopted". So the rule is that the
/// person's words and the harness's answer are raw except for a credential
/// this harness put in its own sealed store, and no other record moves.
///
/// It was forced rather than preferred, by an instrument that already
/// existed: `corpus_a_stored_key_spoken_in_a_task_does_not_reach_the_checkpoint`
/// puts a stored key **in the task** and then walks every file under the
/// scratch home asserting it absent by value and by ASCII core. This file is
/// one of those files. Both halves are therefore built in
/// [`crate::compose::boundary`], which is already the call site that redacts
/// these same two strings on their way into [ADR-0013] D1's layer 6 — so
/// clause 6's enumeration stays at eight files rather than gaining a ninth.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Utterance {
    /// Which turn of the session this belongs to.
    ///
    /// [ADR-0008] D1's number, the caller's, and the **same** `n` that turn's
    /// `TurnStarted` carries — so a reader joining the two needs no rule, and
    /// `session::resume`'s turn count keeps its single source.
    pub n: u32,
    /// Who said it.
    pub voice: Voice,
    /// What was said, as it was typed or as it was rendered.
    pub text: String,
}

/// [ADR-0015] D6's attribution: what a command contributed to a turn, where
/// it came from, and when the user admitted it.
///
/// D6: "Everything a command or skill contributes is attributed in the
/// transcript... The user must always be able to tell which of their
/// behaviour is Zaru and which is something a repository asked for."
///
/// # It carries the typed line as well as the name, and that is the point
///
/// The pane paints the line the person typed — `/deploy-check main` — while
/// the [`Record::Conversation`] beside this one carries the **expanded** text
/// the model was given. A record holding only the name would leave a
/// `--resume` unable to paint what the live pane painted, which is the
/// asymmetry this file already carries for redaction and which there is no
/// reason to repeat when one field closes it.
///
/// `admitted` is `None` for a user command, because a user command is never
/// admitted and saying it was would be false.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribution {
    /// Which turn of the session this belongs to, the same `n` the
    /// [`Utterance`] beside it carries.
    pub n: u32,
    /// The command's name, without a leading slash.
    pub name: String,
    /// `user` or `project`, [ADR-0015] D3's location.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    pub source: String,
    /// `YYYY-MM-DD`, or absent for a user command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admitted: Option<String>,
    /// The line the person typed, verbatim.
    pub typed: String,
}

/// How [ADR-0015] D4's gate was answered, for one project.
///
/// # What it carries, and why the line is on it
///
/// The directory the answer was about, the names it was about in the order
/// the question showed them, which way it went, and **the line the reader was
/// shown** — the last for [`Said`]'s own reason: [ADR-0010] D2's claim is
/// that "re-rendering it reproduces what the user saw", and a line the user
/// was shown that the file does not hold breaks it.
///
/// **It does not carry the files.** `~/.zaru/admissions.jsonl` is D4's record
/// of what was admitted and carries each file verbatim; this is the session's
/// record that the question was answered. Two copies of a body would be two
/// sources of truth for what a person said yes to, and the one that governs
/// loading is the other file. A decline writes nothing there **by design**,
/// which is exactly why it is written here: without this line a decline
/// leaves no trace anywhere.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admitted {
    /// [ADR-0011](https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface)
    /// D4's canonical root: the project the question was about.
    pub directory: PathBuf,
    /// What the door offered, by name, in the order it showed them.
    pub offered: Vec<String>,
    /// Whether they were admitted.
    ///
    /// **A boolean and not an `Option`**, because the door has exactly two
    /// outcomes: `terminal::driver::ask_at_the_door` already reads a terminal
    /// that stopped answering as a decline — "a question that could not be
    /// answered has not been said yes to" — so there is no third state for
    /// this to carry, and inventing one here would be a second reading of
    /// that rule.
    pub admitted: bool,
    /// The sentence, exactly as the reader was shown it.
    pub text: String,
}

/// One line of [ADR-0010] D2's transcript.
///
/// **Externally tagged**, so a line is one JSON object whose single key names
/// the producer it came from and a reader can tell them apart without a
/// convention.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    /// ADR-0008 D3's event stream, which D2 says the transcript is.
    Loop(Event),
    /// ADR-0011 D4's record, which no permission mode removes.
    ToolCall(ToolCall),
    /// ADR-0008 D1's **outer** loop's event stream.
    ///
    /// A fourth producer, and a variant rather than a widening of
    /// [`Record::Loop`]: D1 makes the two loops different loops, and D3's
    /// eight events are all iteration-shaped. See
    /// [`zaru_core::tool_call::event`] for why that stream is its own enum.
    TurnLoop(TurnEvent),
    /// ADR-0016 D1's classified failure, as it was presented.
    Failure(FailureLine),
    /// ADR-0013 D2's compaction: the raw span it replaced and what the user
    /// was told about it.
    ///
    /// A fifth producer, written by whoever owns the turn boundary rather
    /// than by either loop — a compaction is not a state transition of
    /// anything and appears on neither event stream, which is why it is a
    /// variant here and not an `Event`.
    Compacted(Compaction),
    /// A line this session said once and will not say again.
    ///
    /// A sixth producer, and the one that makes "once" a property of the
    /// **session** rather than of the process it was said in: the two
    /// carriers are rebuilt when a process opens, and this is what a later
    /// process reads to know the line is already spent. See the module
    /// documentation for why the transcript is where that counter belongs and
    /// why it is not derived from the checkpoint.
    Said(Said),
    /// One half of one turn's conversation: the task as the user typed it,
    /// or the answer as the turn rendered it.
    ///
    /// A **seventh producer**, and the one that makes `cat transcript.jsonl`
    /// show a person what they asked and what they were told. Until it
    /// existed this file held loop bookkeeping and nothing else, so
    /// [ADR-0010] D5's "the user can read every byte the harness stores about
    /// them with `cat`" was contradicted rather than merely unbuilt — the
    /// prose survived only in `context.json`, which D3 calls the checkpoint
    /// that is overwritten each turn. See [`Utterance`] for why the
    /// [`Voice::User`] half is D2's own unnamed producer arriving, why the
    /// two halves are written around the loop rather than together, and why
    /// these are the only two strings on this file that pass a redactor.
    Conversation(Utterance),
    /// [ADR-0015] D6's attribution for a command that contributed to a turn.
    ///
    /// A **ninth producer**, written immediately before the
    /// [`Record::Conversation`] whose `user` half it is about, so `cat` reads
    /// in the order the turn happened: what the command was, then what the
    /// model was actually given.
    ///
    /// It is a `zaru-cli` variant and nothing in `zaru-core` was widened, for
    /// the reason the seventh and eighth producers are: an expansion is not a
    /// state transition of either loop and appears on neither event stream.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Attribution(Attribution),
    /// [ADR-0015] D4's gate, as it was answered.
    ///
    /// A **tenth producer**, and the first line a session can hold that
    /// belongs to no turn: the door is put by the pump before a keystroke is
    /// read, so this record sits above turn 1 and carries no `n`.
    ///
    /// **Why a producer rather than a [`Record::Said`].** The near miss was
    /// that variant, and it fails on a case a person can reach: a session that
    /// declines and is then resumed puts the door again, in the same session,
    /// onto the same transcript — so a line `SaidOnce` calls "said once and
    /// will not say again" would be on the file twice and the counter
    /// [`crate::session::resume()`] builds from it would be a lie. The other
    /// eight do not fit either: an admission is not a tool call, a decline is
    /// not a failure, and [`Record::Conversation`] and [`Record::Attribution`]
    /// are both keyed to the turn number this has none of.
    ///
    /// It is a `zaru-cli` variant and nothing in `zaru-core` is widened, for
    /// the reason the seventh, eighth and ninth producers are: answering a
    /// door is a state transition of neither loop and appears on neither
    /// event stream.
    ///
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Admitted(Admitted),
}

impl Record {
    /// Which producer this line came from.
    ///
    /// A total function over the enum with **no wildcard arm**, so a further
    /// producer cannot arrive without a name being chosen for it here.
    #[must_use]
    pub const fn producer(&self) -> &'static str {
        match self {
            Self::Loop(_) => "loop",
            Self::TurnLoop(_) => "turn_loop",
            Self::ToolCall(_) => "tool_call",
            Self::Failure(_) => "failure",
            Self::Compacted(_) => "compacted",
            Self::Said(_) => "said",
            Self::Conversation(_) => "conversation",
            Self::Attribution(_) => "attribution",
            Self::Admitted(_) => "admitted",
        }
    }
}
