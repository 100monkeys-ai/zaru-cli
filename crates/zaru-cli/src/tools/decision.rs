// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Whether a tool call is prompted for, and the record no mode removes.
//!
//! # ADR-0011 D4's second sentence is the one that shapes this module
//!
//! "**Mode may remove the prompt; it never removes the record.**" So a
//! decision is a pair: what the harness must do before acting, and the
//! transcript entry it owes whatever it does. The entry carries no mode and
//! cannot, which is what makes "the record is the same at every mode" a
//! property of the type rather than a claim about a code path.
//!
//! # D3's rule, read literally, and one place where that surprises
//!
//! | Mode | This module |
//! | --- | --- |
//! | `ask` | prompts on a write or a command (D3), and on anything out of tree (D4) |
//! | `allow` | proceeds on what the allowlist approves, and prompts for anything else (D3) |
//! | `yolo` | never prompts (D3) |
//!
//! D3's `allow` row says "Runs the project allowlist without prompting;
//! **prompts for anything outside it**". Read literally that makes `allow`
//! prompt for an un-allowlisted *read* that `ask` would let through, which is
//! more prompting rather than less. The alternative reading — that outside
//! the allowlist `allow` falls back to `ask`'s rule — is the one that makes
//! `allow` uniformly looser, and telling them apart needs an ordering over
//! the three modes that **no record states**.
//!
//! The literal reading is built, for two reasons: it is what the record says,
//! and of the two it is the one that prompts more, which is the safe
//! direction to be wrong in on a security boundary. It is recorded as an open
//! question on the record rather than presented as settled — nothing here
//! decides which reading is right.
//!
//! # Nothing here vetoes
//!
//! D6: "The harness is not the authority on whether a command is right... A
//! blocklist that cannot be overridden gets worked around by pasting the
//! command into another terminal, which moves the action out of the
//! transcript and makes things worse. **Surfacing beats forbidding.**"
//!
//! So [`Requirement`] has two variants and neither of them is a veto. The
//! only refusal this module can produce is a user answering no — or nobody
//! being there to answer, which is not the harness deciding either.

use crate::process::line::CommandLine;
use crate::tools::grants::SessionGrants;
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::port::{Allowlist, Answer, Confirm, DestructiveMatch, Question};
use crate::tools::tree::{Placement, Target};
use crate::web::url::RequestedUrl;
use core::fmt;

/// A tool call could not be described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationRefused {
    /// The tool that was offered with the wrong kind of subject.
    pub tool: ToolName,
}

impl fmt::Display for InvocationRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the tool {} is not described by a bare filesystem path, so it cannot be built as a \
             call on one. `web.fetch` addresses a URL and `cmd.run` a command line, neither of \
             which the boundary measures; `fs.search` addresses a path and is not described by one \
             alone, because it carries a root and a needle. Each has its own constructor",
            self.tool
        )
    }
}

impl std::error::Error for InvocationRefused {}

/// What a tool call is addressed to.
///
/// Three variants, because ADR-0011 D4's boundary is about paths and two of
/// D1's seven built-ins do not address one.
///
/// A URL is carried as itself and is **not** classified: no record defines a
/// boundary for outbound destinations, and inventing one would be authoring a
/// security vocabulary. That gap is recorded on the record rather than filled
/// here.
///
/// A command line is carried as itself for a different reason. **Its boundary
/// is the working directory it is started in**, which
/// [`Spawn`](crate::process::Spawn) fixes at D4's root structurally, so there
/// is nothing left for a path classification to decide. Measuring the command
/// *text* against D4 is what this module did until 2026-09-05 and it was an
/// accident — see
/// [`ToolName::addresses_a_path`](crate::tools::ToolName::addresses_a_path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject<'a> {
    /// A filesystem path, already classified against the working directory.
    Path(&'a Target),
    /// A search: where to look, classified, and what to look for.
    ///
    /// A fourth variant rather than a `Path` with the needle dropped, because
    /// ADR-0011 clause 1 asks that a built-in "appear in the transcript **with
    /// their arguments**" and a search whose record says only where it looked
    /// does not. This is the same move [`Subject::Command`] makes for
    /// `cmd.run`, and it has the same consequence: for these two the clause's
    /// second half is met exactly rather than narrowly.
    Search {
        /// Where the walk starts. Classified against D4 like any other path.
        root: &'a Target,
        /// The literal being looked for.
        needle: &'a str,
    },
    /// A command line, already split into a program and its arguments.
    Command(&'a CommandLine),
    /// An `fs.write`: where the bytes go, classified, and the bytes.
    ///
    /// A variant rather than a [`Subject::Path`] with the contents dropped,
    /// for the reason [`Subject::Search`] is one: ADR-0011 D3's prompt asks
    /// the user about an act, and a question naming a path a person cannot
    /// read and withholding what would be put there is a question its reader
    /// cannot answer — [ADR-0016] D2's "a stack trace with better grammar"
    /// applied to a question rather than to an error.
    ///
    /// **It changes nothing about [`Invocation::subject_text`]**, which still
    /// returns the resolved path exactly as it does for [`Subject::Path`].
    /// The contents reach [`Decision::question`]'s detail and reach neither
    /// the allowlist, nor [`TranscriptEntry::render`], nor the line a
    /// transcript holds.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    Write {
        /// Where the bytes go. Classified against D4 like any other path.
        target: &'a Target,
        /// What would be written.
        contents: &'a str,
    },
    /// An `fs.edit`: where, classified, the exact string replaced, and its
    /// replacement.
    ///
    /// The same shape and the same reason as [`Subject::Write`], and the same
    /// promise about [`Invocation::subject_text`].
    Edit {
        /// Which file. Classified against D4 like any other path.
        target: &'a Target,
        /// The exact string being replaced.
        old: &'a str,
        /// What replaces it.
        new: &'a str,
    },
    /// A URL that parsed, carrying a scheme `web.fetch` retrieves.
    ///
    /// A [`RequestedUrl`] rather than a `&str` for
    /// the reason [`Subject::Command`] is a `CommandLine`: the parse happens
    /// before the decision, so the line a transcript shows is a URL that
    /// parsed rather than text nobody has looked at. D4 still says nothing
    /// about it — a URL has no placement.
    Url(&'a RequestedUrl),
}

/// One tool call, as the permission decision sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation<'a> {
    tool: ToolName,
    subject: Subject<'a>,
}

impl<'a> Invocation<'a> {
    /// A call on a filesystem path.
    ///
    /// # Errors
    ///
    /// [`InvocationRefused`] when `tool` does not address a path, which is
    /// `web.fetch` and `cmd.run`. Those two have their own constructors,
    /// [`Invocation::fetching`] and [`Invocation::running`], each taking the
    /// subject that tool actually addresses.
    pub fn on_path(tool: ToolName, target: &'a Target) -> Result<Self, InvocationRefused> {
        if !matches!(tool.subject_kind(), crate::tools::name::SubjectKind::Path)
            || matches!(tool, ToolName::FsWrite | ToolName::FsEdit)
        {
            return Err(InvocationRefused { tool });
        }
        Ok(Self {
            tool,
            subject: Subject::Path(target),
        })
    }

    /// The one built-in that addresses a root and a needle.
    ///
    /// The tool is not a parameter, for the reason [`Invocation::fetching`]'s
    /// and [`Invocation::running`]'s are not: there is exactly one, and this
    /// is the only constructor that produces [`Subject::Search`].
    #[must_use]
    pub const fn searching(root: &'a Target, needle: &'a str) -> Self {
        Self {
            tool: ToolName::FsSearch,
            subject: Subject::Search { root, needle },
        }
    }

    /// The one built-in that puts bytes somewhere.
    ///
    /// The tool is not a parameter, for the reason [`Invocation::searching`]'s
    /// is not: there is exactly one, and this is the only constructor that
    /// produces [`Subject::Write`], so an `fs.write` carrying no contents is
    /// not a value this crate can build. [`Invocation::on_path`] refuses
    /// `fs.write` for the same reason it refuses `cmd.run`.
    #[must_use]
    pub const fn writing(target: &'a Target, contents: &'a str) -> Self {
        Self {
            tool: ToolName::FsWrite,
            subject: Subject::Write { target, contents },
        }
    }

    /// The one built-in that replaces an exact string inside a file.
    ///
    /// The tool is not a parameter, for [`Invocation::writing`]'s reason.
    #[must_use]
    pub const fn editing(target: &'a Target, old: &'a str, new: &'a str) -> Self {
        Self {
            tool: ToolName::FsEdit,
            subject: Subject::Edit { target, old, new },
        }
    }

    /// The one built-in that addresses a URL.
    ///
    /// The tool is not a parameter, because there is exactly one and passing
    /// it would let a caller describe an `fs.read` of a URL.
    #[must_use]
    pub const fn fetching(url: &'a RequestedUrl) -> Self {
        Self {
            tool: ToolName::WebFetch,
            subject: Subject::Url(url),
        }
    }

    /// The one built-in that addresses a command line.
    ///
    /// The tool is not a parameter for the reason
    /// [`Invocation::fetching`]'s is not: there is exactly one, and this is
    /// the only constructor that produces [`Subject::Command`], so a
    /// `cmd.run` paired with any other subject is not a value this crate can
    /// build.
    #[must_use]
    pub const fn running(line: &'a CommandLine) -> Self {
        Self {
            tool: ToolName::CmdRun,
            subject: Subject::Command(line),
        }
    }

    /// Which built-in this is.
    #[must_use]
    pub const fn tool(&self) -> ToolName {
        self.tool
    }

    /// What it is addressed to.
    #[must_use]
    pub const fn subject(&self) -> &Subject<'a> {
        &self.subject
    }

    /// Which of ADR-0011 D4's classes the target falls in, where D4 applies.
    ///
    /// `None` for a URL and for a command line. A caller that treats `None`
    /// as in-tree is not wrong — D4's rule is about the working directory,
    /// a URL is not measured against it, and a command line's working
    /// directory *is* it — but saying so is the reason this is an `Option`
    /// rather than a default.
    #[must_use]
    pub const fn placement(&self) -> Option<Placement> {
        match self.subject {
            Subject::Path(target)
            | Subject::Write { target, .. }
            | Subject::Edit { target, .. }
            | Subject::Search { root: target, .. } => Some(target.placement()),
            Subject::Command(_) | Subject::Url(_) => None,
        }
    }

    /// What the target is called wherever this call is shown.
    ///
    /// For a command line this is the command itself, quoted exactly as
    /// [`CommandLine::split`] would accept it back — which is what makes
    /// ADR-0010's sentence that "a rendered `cmd.run` line **is** a command
    /// line" true rather than approximately true.
    ///
    /// For a search it is the resolved root and the needle, the needle quoted
    /// and escaped so that one carrying a space, a quote or a newline cannot
    /// be read as part of the path. That is what makes ADR-0011 clause 1's
    /// "with their arguments" met exactly for `fs.search`, as it already is
    /// for `cmd.run`.
    #[must_use]
    pub fn subject_text(&self) -> String {
        match self.subject {
            // **The three path arms answer identically, and that is the
            // point.** D3's allowlist compares this string byte for byte, an
            // entry is the line a user copied out of a prompt they read, and
            // an `fs.write` that started rendering its contents here would
            // silently stop matching every entry anybody has written down.
            // The arguments `Subject::Write` and `Subject::Edit` carry reach
            // `Decision::question`'s detail and nothing else.
            Subject::Path(target)
            | Subject::Write { target, .. }
            | Subject::Edit { target, .. } => target.resolved().display().to_string(),
            Subject::Search { root, needle } => {
                format!("{} {needle:?}", root.resolved().display())
            }
            Subject::Command(line) => line.render(),
            Subject::Url(url) => url.as_str().to_owned(),
        }
    }
}

/// What the ports said about a call.
///
/// Split out from [`Decision::reach`] so that the rule is a pure function of
/// values and can be checked with no port at all — the same shape
/// `zaru-core`'s refinement construction uses, and for the same reason:
/// ADR-0011's prompting rule *is* the mechanism, so it is testable in
/// isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Assessment {
    /// Whether the user has pre-approved this exact call. ADR-0011 D3.
    pub allowlisted: bool,
    /// Whether it matches one of ADR-0011 D6's four categories.
    pub destructive: bool,
    /// Whether the user has already allowed this exact line for this session.
    ///
    /// D3's third answer, since 2026-09-14. Distinct from
    /// [`Assessment::allowlisted`] in what it is and in what it does:
    /// `allowlisted` is ADR-0014 layer 2's durable list and decides only the
    /// `allow` mode, where this is an answer the user gave at the prompt, is
    /// never written anywhere, and removes the prompt at **every** mode —
    /// because a person who has just been asked about this exact line and
    /// said "for the session" has answered the question the mode would ask
    /// again.
    pub session_granted: bool,
}

impl Assessment {
    /// Ask both ports about a call.
    #[must_use]
    pub fn gather(
        invocation: &Invocation<'_>,
        allowlist: &dyn Allowlist,
        destructive: &dyn DestructiveMatch,
        granted: &SessionGrants,
    ) -> Self {
        Self {
            allowlisted: allowlist.approves(invocation),
            destructive: destructive.is_destructive(invocation),
            session_granted: granted.approves(invocation),
        }
    }
}

/// What the harness must do before this call may act.
///
/// Two variants, and **neither is a veto** — see the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requirement {
    /// Nothing. The call may act.
    Proceed,
    /// The user is asked first.
    Ask,
}

/// The transcript entry a call owes whatever the mode is.
///
/// **It carries no mode.** ADR-0011 D4: "Mode may remove the prompt; it never
/// removes the record." An entry that carried the mode would render
/// differently at each one, and "the record is the same whatever the mode"
/// would become a thing to assert rather than a thing that is true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptEntry {
    tool: ToolName,
    subject: String,
    placement: Option<Placement>,
    destructive: bool,
}

impl TranscriptEntry {
    /// Which built-in was called.
    #[must_use]
    pub const fn tool(&self) -> ToolName {
        self.tool
    }

    /// Whether ADR-0011 D4's out-of-tree class applies.
    #[must_use]
    pub fn is_out_of_tree(&self) -> bool {
        self.placement.is_some_and(Placement::is_out_of_tree)
    }

    /// Whether ADR-0011 D6 annotated this entry.
    #[must_use]
    pub const fn is_destructive(&self) -> bool {
        self.destructive
    }

    /// The line a transcript shows.
    ///
    /// D4 requires an out-of-tree call to "render differently in the
    /// transcript at every mode including `yolo`", and D6 requires a
    /// destructive one to be annotated. Both markings come from here and from
    /// nowhere else, so the transcript and the prompt cannot describe one
    /// call two ways — [`Decision::question`] renders through this same
    /// function.
    #[must_use]
    pub fn render(&self) -> String {
        let mut line = format!("{}", self.tool);
        // **D4's class goes before the subject, since 2026-09-14.** It was
        // appended after the resolved absolute path from the day the tool
        // surface landed, and `narrative-rendering` recorded on 2026-09-05
        // that a deep enough path pushed it off a narrow frame. The *loss* is
        // closed by wrapping -- the pane has wrapped since `pane-text` and the
        // question does now too -- so this moves on **prominence** rather than
        // on loss, which is the weaker of the two arguments that finding
        // offered and is said here rather than rounded up: a class a reader
        // must see to answer correctly should not arrive after three rows of
        // path. The position was a shape no record gave; it is one now.
        //
        // It costs the persisted `ToolCall.line` its old spelling and costs
        // D3's matching nothing: the allowlist compares
        // `Invocation::subject_text`, never this.
        if let Some(placement) = self.placement
            && placement.is_out_of_tree()
        {
            line.push_str("  [");
            line.push_str(placement.as_str());
            line.push(']');
        }
        if self.destructive {
            line.push_str("  [");
            line.push_str(DESTRUCTIVE_MARKING);
            line.push(']');
        }
        line.push(' ');
        line.push_str(&self.subject);
        line
    }
}

/// How ADR-0011 D6's annotation reads, wherever it appears.
///
/// One constant read by the transcript entry and by the prompt, so that a
/// call annotated in one and unannotated in the other is not a state this
/// code can reach.
pub const DESTRUCTIVE_MARKING: &str = "matches a destructive pattern";

/// Why a call was not permitted.
///
/// Neither variant is the harness vetoing. ADR-0011 D6 gives it no veto, so
/// the only ways a call is refused are that the user said no, or that there
/// was nobody to ask.
///
/// **Neither maps to an ADR-0016 exit code here.** A user answering "no" is
/// not "the work failed", not user-correctable, not environmental, not a
/// capability the tier lacks and not a defect; where it sits in that taxonomy
/// is a question for ADR-0016 and is recorded as open rather than answered by
/// this crate picking a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusedBecause {
    /// A prompt was required and the question did not reach the user.
    ///
    /// Two routes, and the variant is deliberately one for both: no confirmer
    /// was supplied, or the one that was could not put the question. Refused
    /// rather than performed, because a confirmation nobody can answer is
    /// exactly the silent default ADR-0011 D3 and ADR-0007 D8 both forbid.
    ///
    /// **The second route arrived on 2026-09-05** with [`Confirm::confirm`]
    /// answering a `Result`. Before it, a confirmer whose terminal had closed
    /// could only answer `false`, which is [`Self::TheUserDeclined`] — a
    /// record saying the user declined when nobody was asked anything. The
    /// failure's own sentence is not carried here: this enum is a closed set
    /// of permission *outcomes*, and widening it to hold a diagnosis would
    /// make one of them an error report. A caller that wants the detail has
    /// it at the call site.
    ThereWasNobodyToAsk,
    /// The user was asked and said no.
    TheUserDeclined,
}

impl fmt::Display for RefusedBecause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThereWasNobodyToAsk => f.write_str(
                "the call needed the user's confirmation and the question did not reach them — \
                 either no confirmer was supplied, or the one that was could not ask — so it was \
                 refused rather than performed. A confirmation nobody can answer is the silent \
                 default the permission model exists to prevent",
            ),
            Self::TheUserDeclined => {
                f.write_str("the user was asked about the call and did not permit it")
            }
        }
    }
}

impl std::error::Error for RefusedBecause {}

/// Whether a call may act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// It may.
    Granted,
    /// It may, and the user said so for every later call on this exact line.
    ///
    /// A separate variant rather than a flag on [`Permission::Granted`],
    /// because the caller does two different things with them: this one is
    /// also an instruction to remember the line, and a record of a decision a
    /// person made that the transcript owes a line to.
    GrantedForTheSession,
    /// It may not, and this is why.
    Refused(RefusedBecause),
}

impl Permission {
    /// Whether the call may act.
    #[must_use]
    pub const fn permits(self) -> bool {
        matches!(self, Self::Granted | Self::GrantedForTheSession)
    }
}

/// What ADR-0011's permission model says about one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    requirement: Requirement,
    entry: TranscriptEntry,
    prominent: bool,
    /// What the question shows of the call, under the statement.
    ///
    /// Empty until [`Decision::showing`] attaches it, and empty forever for
    /// the four tools whose whole argument is already in the statement. It is
    /// **not** set by [`Decision::reach`], because composing it needs a
    /// budget, a redactor and a look at the filesystem, and that function is
    /// pure over the mode, the call and what the ports already said — which
    /// is what lets D3's prompting rule be checked with no port at all.
    detail: Vec<String>,
}

impl Decision {
    /// Apply ADR-0011 D3, D4 and D6 to one call.
    ///
    /// Pure: it reads the mode, the call and what the ports already said, and
    /// nothing else. [`Decision::assess`] is the same thing with the ports
    /// asked for you.
    #[must_use]
    pub fn reach(mode: Mode, invocation: &Invocation<'_>, assessment: Assessment) -> Self {
        let placement = invocation.placement();
        let out_of_tree = placement.is_some_and(Placement::is_out_of_tree);

        // D4 is not conditional on the effect: an out-of-tree *read* prompts
        // in `ask` and `allow` too.
        let would_prompt_in_ask = out_of_tree || invocation.tool().effect().prompts_in_ask();

        let mut requirement = match mode {
            // D3: "No prompts."
            Mode::Yolo => Requirement::Proceed,
            // D3: "Prompts before any write or command", plus D4.
            Mode::Ask => {
                if would_prompt_in_ask {
                    Requirement::Ask
                } else {
                    Requirement::Proceed
                }
            }
            // D3: "Runs the project allowlist without prompting; prompts for
            // anything outside it", read literally. See the module docs.
            Mode::Allow => {
                if assessment.allowlisted {
                    Requirement::Proceed
                } else {
                    Requirement::Ask
                }
            }
        };

        // D3's third answer, since 2026-09-14. **One line, applied after the
        // mode's own rule and never inside it**, because that is exactly what
        // it is: the mode decides whether this call would be asked about, and
        // a session grant is the user having already answered that question
        // for this exact line. Folding it into the three arms would make it
        // three rules that could come to disagree, and would hide that it
        // holds at `ask`, `allow` and `yolo` alike.
        if assessment.session_granted {
            requirement = Requirement::Proceed;
        }

        Self {
            requirement,
            entry: TranscriptEntry {
                tool: invocation.tool(),
                subject: invocation.subject_text(),
                placement,
                destructive: assessment.destructive,
            },
            prominent: assessment.destructive,
            detail: Vec::new(),
        }
    }

    /// Attach what the question shows of the call.
    ///
    /// Composed once, by [`preview::detail_for`](crate::tools::preview), and
    /// attached here — rather than composed where it is rendered — for the
    /// reason the statement already is: what the user was told and what the
    /// harness believes it asked cannot drift apart. A decision with nothing
    /// attached asks the question this record asked before 2026-09-14.
    #[must_use]
    pub fn showing(mut self, detail: Vec<String>) -> Self {
        self.detail = detail;
        self
    }

    /// Ask the two ports, then apply the rule.
    #[must_use]
    pub fn assess(
        mode: Mode,
        invocation: &Invocation<'_>,
        allowlist: &dyn Allowlist,
        destructive: &dyn DestructiveMatch,
        granted: &SessionGrants,
    ) -> Self {
        let assessment = Assessment::gather(invocation, allowlist, destructive, granted);
        Self::reach(mode, invocation, assessment)
    }

    /// What the harness must do before the call may act.
    #[must_use]
    pub const fn requirement(&self) -> Requirement {
        self.requirement
    }

    /// The transcript entry the call owes whatever the mode was.
    #[must_use]
    pub const fn entry(&self) -> &TranscriptEntry {
        &self.entry
    }

    /// The sentence the user is asked, if they are asked one.
    ///
    /// Rendered from the same [`TranscriptEntry::render`] the transcript
    /// shows, so the prompt and the record cannot describe one call two ways.
    #[must_use]
    pub fn question(&self) -> Option<Question> {
        match self.requirement {
            Requirement::Proceed => None,
            Requirement::Ask => Some(Question {
                statement: format!("Allow {}?", self.entry.render()),
                detail: self.detail.clone(),
                prominent: self.prominent,
            }),
        }
    }

    /// Resolve the decision, asking the user where the rule says to.
    ///
    /// # A missing confirmer is a refusal, not a pass — and so is an ask that
    /// could not be put
    ///
    /// `None` refuses any call that needed a prompt. This is the shape
    /// ADR-0007 D8's apex gate already uses in this crate, for the reason
    /// that record gives: "Never silent, never a default."
    ///
    /// A confirmer that answers
    /// [`Err`](crate::tools::port::ConfirmFailure) refuses for the **same**
    /// reason: the question did not reach a person. Mapping it to
    /// [`RefusedBecause::TheUserDeclined`] instead would write into the
    /// transcript that the user said no, which is a different event and one
    /// nobody could later distinguish from a real decline.
    #[must_use]
    pub fn permit(&self, confirmer: Option<&dyn Confirm>) -> Permission {
        let Some(question) = self.question() else {
            return Permission::Granted;
        };
        match confirmer.map(|confirmer| confirmer.confirm(&question)) {
            None | Some(Err(_)) => Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
            Some(Ok(Answer::Once)) => Permission::Granted,
            Some(Ok(Answer::ForThisSession)) => Permission::GrantedForTheSession,
            Some(Ok(Answer::No)) => Permission::Refused(RefusedBecause::TheUserDeclined),
        }
    }
}
