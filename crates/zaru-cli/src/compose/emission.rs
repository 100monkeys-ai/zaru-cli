// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The emission set: every line this harness can put in front of a person
//! without being asked, enumerated once.
//!
//! # What "the emission set" is, and why it had to be decided before this
//! existed
//!
//! [ADR-0002] trigger clause 1 asks that every line the harness emits be shown
//! to trace to a cause "**by enumerating what the harness can emit** rather
//! than by walking one path", and clause 8 asks that no emitted line report on
//! the user's own behaviour, "asserted by **enumerating the emission set
//! exhaustively** rather than by grepping for words". Neither clause defines
//! the term, and the record uses it in three incompatible ways — D1's *"Zaru
//! never emits output the user did not cause"*, where an emission is anything
//! put in front of a person; D8's *"never a separate emission"*, where it is a
//! discrete rendered unit; and clause 8's *"the emission set"*, a set noun
//! with no antecedent.
//!
//! **Decided 2026-09-15 under directive 20 and open to Jeshua's veto, on
//! [ADR-0002's amendments page]: the emission set is the set of lines this
//! harness can emit to a person without being asked** — lines rather than
//! doors, and unprompted rather than every line. That is ADR-0002's own
//! subject: D1 governs output "the user did not cause", and a line rendered in
//! answer to a command the user typed is caused by construction.
//!
//! # An enumeration plus its proof, which is what makes it exhaustive
//!
//! A list of lines is a list somebody has to remember to extend, and clause 1
//! rejects exactly that. So this module is two things:
//!
//! - [`Unprompted`], a closed enum with one variant per line, each **referring
//!   to the constant that already holds its wording** and re-authoring none of
//!   them, and each declaring its [`Cause`], its [`Subject`] and the [`Door`]
//!   it reaches a person through.
//! - [`Door`], the closed set of ways an unprompted line can reach a person at
//!   all, each naming the product files that open it. `emission/tests.rs`
//!   walks this crate's own source off disk and fails when a door is opened
//!   anywhere else.
//!
//! The pair is the enumeration and its proof: every door consults the
//! registry, and no other door exists. Adding a line to an unprompted surface
//! without a member fails the walk; adding a member without a wording or a
//! cause fails to compile.
//!
//! # `Cause::ArmedTrigger` and `Subject::TheUser` have no member, deliberately
//!
//! Clause 1's three terms are "a user message, a turn in progress, or a named
//! armed trigger", and clause 8's prohibition is on a line whose subject is
//! the user. Both are variants here with nothing declaring them, which is what
//! those two facts look like as a shape rather than as prose: no trigger type
//! exists anywhere in this workspace, so nothing can declare
//! [`Cause::ArmedTrigger`], and a line reporting on the user's own behaviour
//! cannot be added without declaring [`Subject::TheUser`] — which is the
//! assertion `emission/tests.rs` fails on.
//!
//! This is the discipline [`Tip`](crate::compose::Tip) already carries in the
//! other direction: "a mechanism with no member is unfalsifiable". Every
//! *other* variant of both enums has a member, so neither enum is a table of
//! possibilities nobody uses.
//!
//! # What is deliberately not a member
//!
//! - **[ADR-0005] D1 row 1's deposit count.** `Composer::set_standing`'s first
//!   argument is a written `0` at its one product call site, and the strip
//!   renders `StripContent::Deposits` only above zero, so no product path can
//!   reach that row. [ADR-0002] D3 gives a deposit one producer — an armed
//!   trigger — and nothing here can arm one. A member for it would be the
//!   "permanent exemption dressed as a promise" [`crate::session::Record`]
//!   refuses in its own words.
//! - **Everything said to a model.** [`prose::SUMMARISE_SPAN`] and
//!   [`prose::ITERATION_IS_ONE_EXCHANGE`] are read by a model and never by a
//!   person, and each says so where it is defined. **[`prose::NO_PERSONA`] is
//!   the third and it was a member until the artefact said otherwise**: its
//!   own documentation says the line is there "so that a reader of the
//!   transcript sees the absence rather than inferring it", and the transcript
//!   does not carry it. `compose::context::prefix_for` puts it in
//!   [ADR-0013] D1's layer 1, which is assembled into the prompt and persisted
//!   nowhere — measured 2026-09-15 on a real session, whose `context.json`
//!   holds only `exchanges`. So it reaches a model and no person, and it is
//!   exempt rather than enumerated.
//! - **Everything a person asked for.** A failure's statement and its remedy,
//!   `--help`'s table, a data projection, a permission question, the two
//!   retrieval commands' lines and the hint strip's typing-mode rows are the
//!   rendering of a request. D1 calls those caused, and they are outside the
//!   set this module holds.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0002's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output-updates
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [`prose::NO_PERSONA`]: crate::compose::prose::NO_PERSONA
//! [`prose::SUMMARISE_SPAN`]: crate::compose::prose::SUMMARISE_SPAN
//! [`prose::ITERATION_IS_ONE_EXCHANGE`]: crate::compose::prose::ITERATION_IS_ONE_EXCHANGE

use crate::compose::prose;
use crate::terminal::{paths, trie};

/// Where a line's wording comes from.
///
/// The distinction is whether the sentence exists as text before the moment it
/// is shown. It matters because the two are checkable in different ways: an
/// authored line is compared by value against the constant that declares it,
/// and a composed one can only be named by the function that builds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wording {
    /// The line is a constant somewhere in this crate.
    Authored {
        /// The constant's spelling in source, so the walk over the modules
        /// that declare these can match a declaration to a member.
        name: &'static str,
        /// The constant itself, by reference. **Never a copy**: a second
        /// spelling of one sentence is two sentences that will disagree.
        text: &'static str,
    },
    /// The line is built from data at the moment it is shown, so it has no
    /// text until then. The path names the function that builds it.
    Composed {
        /// The composing function, as a path a reader can open.
        by: &'static str,
    },
}

/// What caused a line, in [ADR-0002] clause 1's own three terms.
///
/// D1 defines "cause" exhaustively — "The user sent a message, ran a command,
/// or is in a turn Zaru is currently serving", or "The user previously
/// **armed** a specific trigger, and that trigger fired" — and clause 1 asks
/// that every emitted line trace to one of them.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The user sent a message or ran a command — including the command that
    /// opened the session this line is painted in.
    UserMessage,
    /// The user is in a turn Zaru is currently serving.
    TurnInProgress,
    /// The user armed a named trigger and it fired.
    ///
    /// **No member declares this**, and that is the state of the harness
    /// rather than an omission: [ADR-0002] D2's four arming acts are "starting
    /// a long-running task, setting a watch, scheduling a run, or enabling a
    /// named surface in configuration", no type in this workspace represents a
    /// trigger, and none of the four exists. See [`Unprompted::cause`] for
    /// what that costs clause 1.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    ArmedTrigger,
}

/// What a line is *about*, in [ADR-0002] D7's vocabulary.
///
/// D7: "Zaru does not report on the user's own behaviour: no activity streaks,
/// no productivity observations, no 'you've been working a while' prompts, no
/// unsolicited characterisation of their habits back to them." Clause 8 asks
/// that to be asserted over the whole set rather than grepped for, and a
/// declared subject per member is how: the prohibition becomes a variant that
/// nothing may declare.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// What this build of the harness does or does not have.
    TheHarness,
    /// A property of the runtime tier in force, per [ADR-0001] D1.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    ATier,
    /// A property of the project the harness was opened in.
    TheProject,
    /// A property of the model answering, or of what it cost.
    TheModel,
    /// A property of this session — its context, its turns, what it dropped.
    TheSession,
    /// The user's own behaviour.
    ///
    /// **No member declares this, and `emission/tests.rs` fails if one ever
    /// does.** That is [ADR-0002] D7 held as a shape: a streak, an
    /// elapsed-session observation or a characterisation cannot be added to an
    /// unprompted surface without writing this variant down, and writing it
    /// down is the red.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    TheUser,
}

/// A way an unprompted line reaches a person.
///
/// A door is a function this crate calls to put a line somewhere a person is
/// looking, on a path the person did not ask to be filled. The set is closed
/// and the walk in `emission/tests.rs` fails when one is opened in a file
/// [`Door::opened_in`] does not name — which is what makes [`Unprompted`] an
/// enumeration with a proof rather than a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Door {
    /// `Composer::set_standing` — [ADR-0005] D1's empty-prompt strip.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    StandingStrip,
    /// `Composer::set_absence` — the strip's absence line.
    AbsenceStrip,
    /// `Composer::set_path_absence` — the same, for the path corpus.
    ///
    /// A second door and not a second use of the one above, because the two
    /// corpora fail independently: a session can have a cortex to search and a
    /// working directory with nothing to name, or the reverse, and one field
    /// would make the strip say the wrong sentence in exactly those sessions.
    PathAbsenceStrip,
    /// `SessionNotice::state_once` — [ADR-0011] D2's notice.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    SessionNotice,
    /// `MissingManifest::state_once` — [ADR-0009] D4's recommendation.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    Recommendation,
    /// `Narrator::announce_interrupted` — the pane, from the signal path.
    InterruptNotice,
    /// `cli::render::announcement` — [ADR-0013] D3's and D4's lines.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    ContextAnnouncement,
    /// `Shell::set_context_usage` — [ADR-0013] D6's segment.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    ContextSegment,
    /// `Shell::set_token_usage` — [ADR-0012] D7's segment.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    TokenSegment,
    /// `Shell::set_elapsed` — [ADR-0028] D5's meter.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    ElapsedSegment,
    /// `terminal::vocabulary::line_for` — [ADR-0028] D3's narrative rows.
    ///
    /// Only one narrative row is in this set; see
    /// [`Unprompted::TurnCounter`] for which and why the others are not.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    NarrativeRow,
    /// `Pane::say_still_generating` — the pane, on the shell's beat.
    ///
    /// Distinct from [`Door::NarrativeRow`] because nothing emits it: it is
    /// said by the beat when an armed exchange has put nothing on the screen
    /// for [`crate::terminal::source::QUIET`], so it reaches a person through
    /// the pane rather than through [ADR-0008] D3's event stream. That is
    /// also why it is in no transcript — see [`Unprompted::StillGenerating`].
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    StillGeneratingRow,
    /// `cli::render::opening` — the one line a session opens on.
    ///
    /// The door is the composing function rather than `Shell::notice`, which
    /// puts dozens of *caused* lines on the pane across two files and would
    /// name a needle every one of them matched. This is the rule
    /// [`Door::ContextAnnouncement`] already follows for the same reason.
    OpeningLine,
}

impl Door {
    /// Every door, so a check can walk them rather than list them.
    ///
    /// The length is annotated, so a twelfth fails to compile here as well as
    /// in every exhaustive match below.
    pub const ALL: [Self; 12] = [
        Self::StandingStrip,
        Self::AbsenceStrip,
        Self::SessionNotice,
        Self::Recommendation,
        Self::InterruptNotice,
        Self::ContextAnnouncement,
        Self::ContextSegment,
        Self::TokenSegment,
        Self::ElapsedSegment,
        Self::NarrativeRow,
        Self::StillGeneratingRow,
        Self::OpeningLine,
    ];

    /// The text that opens this door, as it is written at a call site.
    ///
    /// Matched against product source with the comment lines removed, so a
    /// doc comment naming a door is prose about the rule rather than an
    /// instance of it — the discrimination
    /// `corpus_one_place_in_the_terminal_renders_a_classified_failure`
    /// already makes.
    ///
    /// **Every needle is a call or a type pattern and none is a spelling of
    /// the line itself.** `unprompted-output`'s dated finding on
    /// [operations/autonomous-development] is that a walk keyed on an
    /// argument's spelling asserts a habit rather than the code, so
    /// [`Door::NarrativeRow`] is keyed on `Event::TurnStarted { n, of }` —
    /// the destructuring that reads both the number and the ceiling, which
    /// `session::resume`'s `{ n, .. }` deliberately does not match — rather
    /// than on the row's own words.
    ///
    /// [operations/autonomous-development]: https://100monkeys-ai.cortex.page/zaru/p/operations/autonomous-development
    #[must_use]
    pub const fn needle(self) -> &'static str {
        match self {
            Self::StandingStrip => ".set_standing(",
            Self::AbsenceStrip => ".set_absence(",
            Self::PathAbsenceStrip => ".set_path_absence(",
            Self::SessionNotice | Self::Recommendation => ".state_once()",
            Self::InterruptNotice => "self.announce_interrupted()",
            Self::ContextAnnouncement => "render::announcement(",
            Self::ContextSegment => ".set_context_usage(",
            Self::TokenSegment => ".set_token_usage(",
            Self::ElapsedSegment => ".set_elapsed(",
            Self::NarrativeRow => "Event::TurnStarted { n, of }",
            // A call and not the sentence, by the same rule: the line's
            // wording lives in `prose` and this names the one method that
            // puts it on the shell.
            Self::StillGeneratingRow => "self.say_still_generating()",
            Self::OpeningLine => "render::opening(",
        }
    }

    /// The product files in this crate that may open this door, relative to
    /// the crate root.
    ///
    /// **Files rather than lines.** A line number is invalidated by every edit
    /// above it, so a walk keyed on one asserts the calendar; a file is the
    /// unit a reviewer can act on and the unit that changes when a door moves.
    ///
    /// Two doors share a needle — [`Door::SessionNotice`] and
    /// [`Door::Recommendation`] are both `state_once` — so both name the same
    /// file, and the walk compares sets rather than counts.
    #[must_use]
    pub const fn opened_in(self) -> &'static [&'static str] {
        match self {
            Self::StandingStrip => &["src/terminal/open.rs"],
            Self::AbsenceStrip => &["src/terminal/driver.rs", "src/terminal/open.rs"],
            Self::PathAbsenceStrip => &["src/terminal/driver.rs"],
            Self::SessionNotice | Self::Recommendation => &["src/compose/turn.rs"],
            Self::InterruptNotice => &["src/compose/iterate.rs"],
            Self::ContextAnnouncement => &["src/compose/turn.rs", "src/terminal/vocabulary.rs"],
            Self::ContextSegment | Self::TokenSegment | Self::ElapsedSegment => {
                &["src/terminal/driver.rs"]
            }
            Self::NarrativeRow => &["src/terminal/vocabulary.rs"],
            Self::StillGeneratingRow => &["src/terminal/driver.rs"],
            Self::OpeningLine => &["src/terminal/open.rs"],
        }
    }
}

/// One line this harness can emit to a person without being asked.
///
/// Every variant refers to the wording that already exists and authors none of
/// it. See the module documentation for what the set is and for the three
/// kinds of line that are deliberately outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unprompted {
    /// [ADR-0011] D2's statement that `bare` tier has no membrane.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    NotASandbox,
    /// [ADR-0009] D4's missing-validators half.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    MissingValidators,
    /// [ADR-0009] D4's remedy half, which is [ADR-0016] D2's requirement that
    /// a reader can act.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    DeclareOne,
    /// What the pane says when a turn was interrupted.
    Interrupted,
    /// [ADR-0002] D8's standing tip.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    StandingTip,
    /// The absence line when no stored token holds the composer role.
    NothingCached,
    /// The absence line when the working directory offers no path to name.
    ///
    /// [ADR-0005]'s third corpus, added 2026-09-15. It is on the same surface
    /// as the four above and says a different thing about a different corpus.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer-updates-2
    NothingToName,
    /// The absence line while the corpus is still being fetched.
    LookingInNotes,
    /// The absence line when the token's instance could not be reached.
    NotesUnreachable,
    /// The absence line when a cached corpus is being served instead.
    NotesFromCache,
    /// [ADR-0013] D3's compaction announcement.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    Compacted,
    /// [ADR-0013] D4's dropped-attachment announcement.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    AttachmentDropped,
    /// [ADR-0013] D6's context segment on the status row.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    ContextUsage,
    /// [ADR-0012] D7's token segment on the status row.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    TokenUsage,
    /// [ADR-0028] D5's meter on the status row.
    ///
    /// # This is not clause 8's "elapsed-session observation"
    ///
    /// **Ruled 2026-09-15 under directive 20 and open to Jeshua's veto.** The
    /// meter starts at each turn and is taken off the row at that turn's end —
    /// `Status::elapsed` is `None` "except while a turn is running" — so it
    /// measures the work, not the person. An observation about how long the
    /// *session* has been open would be a different field and there is none.
    ///
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    Elapsed,
    /// [ADR-0028] D3's turn row: `turn {n}, up to {of} exchange(s)`.
    ///
    /// # This is the one member whose subject is a question, and clause 8
    /// waits on it
    ///
    /// Every other narrative row is about the work — a tool call, a candidate,
    /// an iteration, what it cost — and is outside this set because it is the
    /// rendering of the turn the user asked for. This one is a **count of the
    /// user's own turns**, painted unasked at the head of each, and
    /// [ADR-0002] D7 forbids "activity streaks".
    ///
    /// It is declared [`Subject::TheSession`] here, on the reading that `n`
    /// numbers the session's turns rather than characterising the person, and
    /// that reading is **stated rather than claimed**: a reading under which
    /// it is [`Subject::TheUser`] is available, nothing in the record settles
    /// which, and ADR-0002 clause 8 is therefore withheld on exactly this
    /// line. See [ADR-0002's amendments page]; the decision is the record
    /// author's and no wording is changed here.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    /// [ADR-0002's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output-updates
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    TurnCounter,
    /// [ADR-0028] D5's line for an exchange that has generated nothing.
    ///
    /// # Why a line and not a number
    ///
    /// D5 asks for elapsed time, token counts and cost "as the work
    /// proceeds", and on a reasoning turn there is no work to report:
    /// measured 2026-09-15, the provider puts **no frame of any kind** on the
    /// wire for 82 to 87 per cent of the request, so every number the row
    /// could carry is either stale or absent. And at forty columns the row
    /// carries neither number at all. A sentence is what is left, and
    /// [`crate::compose::prose::STILL_GENERATING`] carries the measurements
    /// behind it.
    ///
    /// # It is in no transcript, deliberately
    ///
    /// Every other narrative row is an [ADR-0008] D3 event and reaches
    /// `transcript.jsonl` through the same emission. This one is said by the
    /// pane's beat, and what it says — that nothing has come back **yet** —
    /// is a fact about waiting. A person reading the turn back on `--resume`
    /// is not waiting, and the answer is already there. So the line has no
    /// producer to widen and no record to write, which is what keeps
    /// `zaru-core` untouched.
    ///
    /// The precedent is [`Unprompted::Interrupted`], which reaches the pane
    /// from the signal path rather than from a loop event, and the three
    /// status segments, which reach no transcript either.
    ///
    /// [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
    /// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
    StillGenerating,
    /// The one line a session opens on, naming the working directory and
    /// `/help`.
    ///
    /// # Composed, although its sentence is a constant
    ///
    /// `cli::render::OPENING` is the whole of what is authored, and the line a
    /// person meets carries a path beside it — so the wording exists as text
    /// before the moment it is shown and the *line* does not. Every other
    /// [`Wording::Authored`] member is quoted verbatim, and claiming this one
    /// among them would make `Authored`'s own "compared by value against the
    /// constant that declares it" false for one member.
    ///
    /// # Its cause is the command that opened the session
    ///
    /// [ADR-0002] D1's causes are "The user sent a message, **ran a command**,
    /// or is in a turn Zaru is currently serving", and running `zaru` is
    /// running a command — which is what [`Cause::UserMessage`] here has said
    /// since this registry was written. **Accepted 2026-09-15 under directive
    /// 20 and open to Jeshua's veto**, on that record's amendments page. The
    /// subject is this session: where it is, and what may be typed into it.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    Opening,
}

impl Unprompted {
    /// Every line in the set, so a check can walk them rather than list them.
    ///
    /// The length is annotated, so an eighteenth fails to compile here as well
    /// as in every exhaustive match below.
    pub const ALL: [Self; 18] = [
        Self::NotASandbox,
        Self::MissingValidators,
        Self::DeclareOne,
        Self::Interrupted,
        Self::StandingTip,
        Self::NothingCached,
        Self::NothingToName,
        Self::LookingInNotes,
        Self::NotesUnreachable,
        Self::NotesFromCache,
        Self::Compacted,
        Self::AttachmentDropped,
        Self::ContextUsage,
        Self::TokenUsage,
        Self::Elapsed,
        Self::TurnCounter,
        Self::StillGenerating,
        Self::Opening,
    ];

    /// Where this line's wording comes from — by reference, never retyped.
    #[must_use]
    pub const fn wording(self) -> Wording {
        match self {
            Self::NotASandbox => Wording::Authored {
                name: "NOT_A_SANDBOX",
                text: prose::NOT_A_SANDBOX,
            },
            Self::MissingValidators => Wording::Authored {
                name: "NO_VALIDATORS",
                text: prose::NO_VALIDATORS,
            },
            Self::DeclareOne => Wording::Authored {
                name: "DECLARE_ONE",
                text: prose::DECLARE_ONE,
            },
            Self::Interrupted => Wording::Authored {
                name: "INTERRUPTED",
                text: prose::INTERRUPTED,
            },
            Self::NothingCached => Wording::Authored {
                name: "NOTHING_CACHED",
                text: trie::NOTHING_CACHED,
            },
            Self::NothingToName => Wording::Authored {
                name: "NOTHING_TO_OFFER",
                text: paths::NOTHING_TO_OFFER,
            },
            Self::LookingInNotes => Wording::Authored {
                name: "LOOKING",
                text: trie::LOOKING,
            },
            Self::NotesUnreachable => Wording::Authored {
                name: "UNREACHABLE",
                text: trie::UNREACHABLE,
            },
            Self::NotesFromCache => Wording::Authored {
                name: "FROM_CACHE",
                text: trie::FROM_CACHE,
            },
            // A tip's line is a method on a closed enum rather than a
            // constant, because there is one line per variant and `Tip::ALL`
            // is already walked by `compose::tips`' own checks.
            Self::StandingTip => Wording::Composed {
                by: "crate::compose::tips::Tip::line",
            },
            Self::Compacted | Self::AttachmentDropped => Wording::Composed {
                by: "crate::cli::render::announcement",
            },
            Self::ContextUsage => Wording::Composed {
                by: "crate::cli::render::context_row",
            },
            Self::TokenUsage => Wording::Composed {
                by: "crate::cli::render::usage_row",
            },
            Self::Elapsed => Wording::Composed {
                by: "crate::terminal::vocabulary::seconds",
            },
            Self::TurnCounter => Wording::Composed {
                by: "crate::terminal::vocabulary::line_for",
            },
            Self::StillGenerating => Wording::Authored {
                name: "STILL_GENERATING",
                text: prose::STILL_GENERATING,
            },
            Self::Opening => Wording::Composed {
                by: "crate::cli::render::opening",
            },
        }
    }

    /// What caused this line, in clause 1's own vocabulary.
    ///
    /// # Every member is a message or a turn, and that is two of clause 1's
    /// three terms
    ///
    /// Nothing returns [`Cause::ArmedTrigger`], because nothing can: there is
    /// no trigger type in this workspace. So this enumeration shows that every
    /// line in the set traces to a user message or a turn in progress, which
    /// is two of the clause's three terms — claimed as two rather than as
    /// whole, which is the reading already on [ADR-0002's amendments page].
    ///
    /// [ADR-0002's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output-updates
    #[must_use]
    pub const fn cause(self) -> Cause {
        match self {
            // The command that opened the session. Each of these is decided
            // before the first frame and before any turn exists.
            Self::StandingTip
            | Self::NothingCached
            | Self::NothingToName
            | Self::LookingInNotes
            | Self::NotesUnreachable
            | Self::NotesFromCache
            | Self::Opening => Cause::UserMessage,
            Self::NotASandbox
            | Self::MissingValidators
            | Self::DeclareOne
            | Self::Interrupted
            | Self::Compacted
            | Self::AttachmentDropped
            | Self::ContextUsage
            | Self::TokenUsage
            | Self::Elapsed
            | Self::TurnCounter
            | Self::StillGenerating => Cause::TurnInProgress,
        }
    }

    /// What this line is about, in [ADR-0002] D7's vocabulary.
    ///
    /// Nothing returns [`Subject::TheUser`], and `emission/tests.rs` asserts
    /// it. See [`Unprompted::TurnCounter`] for the one member whose subject is
    /// argued rather than settled.
    ///
    /// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
    #[must_use]
    pub const fn subject(self) -> Subject {
        match self {
            Self::NotASandbox => Subject::ATier,
            Self::MissingValidators | Self::DeclareOne => Subject::TheProject,
            Self::StandingTip
            | Self::NothingCached
            | Self::LookingInNotes
            | Self::NotesUnreachable
            | Self::NotesFromCache => Subject::TheHarness,
            // The project's, and not the harness's: what it reports is what
            // this working directory holds, which is a fact about the tree
            // somebody opened the session in.
            Self::NothingToName => Subject::TheProject,
            // The model's, and emphatically not the user's: what is
            // reported is what the provider is doing, never how long
            // somebody has been waiting on it. D7 forbids the second.
            Self::TokenUsage | Self::StillGenerating => Subject::TheModel,
            Self::Interrupted
            | Self::Compacted
            | Self::AttachmentDropped
            | Self::ContextUsage
            | Self::Elapsed
            | Self::TurnCounter
            | Self::Opening => Subject::TheSession,
        }
    }

    /// The door this line reaches a person through.
    #[must_use]
    pub const fn door(self) -> Door {
        match self {
            Self::NotASandbox => Door::SessionNotice,
            Self::MissingValidators | Self::DeclareOne => Door::Recommendation,
            Self::Interrupted => Door::InterruptNotice,
            Self::StandingTip => Door::StandingStrip,
            Self::NothingCached
            | Self::LookingInNotes
            | Self::NotesUnreachable
            | Self::NotesFromCache => Door::AbsenceStrip,
            Self::NothingToName => Door::PathAbsenceStrip,
            Self::Compacted | Self::AttachmentDropped => Door::ContextAnnouncement,
            Self::ContextUsage => Door::ContextSegment,
            Self::TokenUsage => Door::TokenSegment,
            Self::Elapsed => Door::ElapsedSegment,
            Self::TurnCounter => Door::NarrativeRow,
            Self::StillGenerating => Door::StillGeneratingRow,
            Self::Opening => Door::OpeningLine,
        }
    }

    /// The record and clause that puts this line in front of a person.
    ///
    /// A pair rather than a sentence, so the walk can check the shape of the
    /// record's number and a reader can follow it.
    #[must_use]
    pub const fn clause(self) -> (&'static str, &'static str) {
        match self {
            Self::NotASandbox => ("ADR-0011", "D2"),
            Self::MissingValidators | Self::DeclareOne => ("ADR-0009", "D4"),
            Self::Interrupted => ("ADR-0010", "D2"),
            Self::StandingTip => ("ADR-0002", "D8"),
            Self::NothingCached
            | Self::LookingInNotes
            | Self::NotesUnreachable
            | Self::NotesFromCache => ("ADR-0005", "D3"),
            Self::NothingToName => ("ADR-0005", "D1"),
            Self::Compacted => ("ADR-0013", "D3"),
            Self::AttachmentDropped => ("ADR-0013", "D4"),
            Self::ContextUsage => ("ADR-0013", "D6"),
            Self::TokenUsage => ("ADR-0012", "D7"),
            Self::Elapsed => ("ADR-0028", "D5"),
            Self::TurnCounter => ("ADR-0028", "D3"),
            Self::StillGenerating => ("ADR-0028", "D5"),
            Self::Opening => ("ADR-0002", "D1"),
        }
    }
}

/// The modules whose authored sentences must all be in the set or exempt.
///
/// These are the files that declare a `pub const … : &str` a person can read
/// without having asked for it. The walk in `emission/tests.rs` reads each one
/// off disk and fails when a constant is declared in one of them and neither
/// referenced by a member nor named in [`EXEMPT`].
pub const UNPROMPTED_HOMES: [&str; 4] = [
    "src/compose/prose.rs",
    "src/compose/tips.rs",
    "src/terminal/trie.rs",
    "src/terminal/paths.rs",
];

/// Constants declared in an [`UNPROMPTED_HOMES`] file that are not members,
/// each with the reason it is not.
///
/// An exemption list that grows until it excuses everything is the shape
/// [Verification lessons] §8 names, so each entry here carries its reason and
/// the list is walked rather than trusted: a name that stops being declared
/// fails the check as loudly as one that appears without a member.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
pub const EXEMPT: [(&str, &str); 7] = [
    (
        "SUMMARISE_SPAN",
        "read by a model and never by a person; the constant says so where it is defined",
    ),
    (
        "ITERATION_IS_ONE_EXCHANGE",
        "read by a model and never by a person; the constant says so where it is defined",
    ),
    (
        "NO_DEPOSITS",
        "the answer `/inbox` and `zaru inbox` give, so a person asked for it",
    ),
    (
        "NOTHING_LEARNED",
        "the answer `/learned` and `zaru learned` give, so a person asked for it",
    ),
    (
        "NO_PERSONA",
        "assembled into ADR-0013 D1's layer 1 and nowhere else, so it is read by a model and \
         never by a person — measured on the artefact of 2026-09-15, where `context.json` holds \
         only `exchanges` and the transcript holds no prefix",
    ),
    (
        "TIPS_FILE",
        "a file name under `~/.zaru`, not a sentence anybody reads",
    ),
    (
        "KEY",
        "a configuration key's spelling, not a sentence anybody reads",
    ),
];

#[cfg(test)]
mod tests;
