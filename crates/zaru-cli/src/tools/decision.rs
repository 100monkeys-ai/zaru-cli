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
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::port::{Allowlist, Confirm, DestructiveMatch, Question};
use crate::tools::tree::{Placement, Target};
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
            "the tool {} does not address a filesystem path, so it cannot be described as a call \
             on one. ADR-0011 D4's boundary is about paths, and two of D1's seven address \
             something else: `web.fetch` addresses a URL and `cmd.run` addresses a command line, \
             whose boundary is the working directory it is started in",
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
    /// A command line, already split into a program and its arguments.
    Command(&'a CommandLine),
    /// A URL, which D4 says nothing about.
    Url(&'a str),
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
        if !tool.addresses_a_path() {
            return Err(InvocationRefused { tool });
        }
        Ok(Self {
            tool,
            subject: Subject::Path(target),
        })
    }

    /// The one built-in that addresses a URL.
    ///
    /// The tool is not a parameter, because there is exactly one and passing
    /// it would let a caller describe an `fs.read` of a URL.
    #[must_use]
    pub const fn fetching(url: &'a str) -> Self {
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
            Subject::Path(target) => Some(target.placement()),
            Subject::Command(_) | Subject::Url(_) => None,
        }
    }

    /// What the target is called wherever this call is shown.
    ///
    /// For a command line this is the command itself, quoted exactly as
    /// [`CommandLine::split`] would accept it back — which is what makes
    /// ADR-0010's sentence that "a rendered `cmd.run` line **is** a command
    /// line" true rather than approximately true.
    #[must_use]
    pub fn subject_text(&self) -> String {
        match self.subject {
            Subject::Path(target) => target.resolved().display().to_string(),
            Subject::Command(line) => line.render(),
            Subject::Url(url) => url.to_owned(),
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
}

impl Assessment {
    /// Ask both ports about a call.
    #[must_use]
    pub fn gather(
        invocation: &Invocation<'_>,
        allowlist: &dyn Allowlist,
        destructive: &dyn DestructiveMatch,
    ) -> Self {
        Self {
            allowlisted: allowlist.approves(invocation),
            destructive: destructive.is_destructive(invocation),
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
        let mut line = format!("{} {}", self.tool, self.subject);
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
    /// A prompt was required and no confirmer was supplied.
    ///
    /// Refused rather than performed, because a confirmation nobody can
    /// answer is exactly the silent default ADR-0011 D3 and ADR-0007 D8 both
    /// forbid.
    ThereWasNobodyToAsk,
    /// The user was asked and said no.
    TheUserDeclined,
}

impl fmt::Display for RefusedBecause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThereWasNobodyToAsk => f.write_str(
                "the call needed the user's confirmation and no confirmer was supplied, so it \
                 was refused rather than performed. A confirmation nobody can answer is the \
                 silent default ADR-0011 D3 exists to prevent",
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
    /// It may not, and this is why.
    Refused(RefusedBecause),
}

/// What ADR-0011's permission model says about one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    requirement: Requirement,
    entry: TranscriptEntry,
    prominent: bool,
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

        let requirement = match mode {
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

        Self {
            requirement,
            entry: TranscriptEntry {
                tool: invocation.tool(),
                subject: invocation.subject_text(),
                placement,
                destructive: assessment.destructive,
            },
            prominent: assessment.destructive,
        }
    }

    /// Ask the two ports, then apply the rule.
    #[must_use]
    pub fn assess(
        mode: Mode,
        invocation: &Invocation<'_>,
        allowlist: &dyn Allowlist,
        destructive: &dyn DestructiveMatch,
    ) -> Self {
        let assessment = Assessment::gather(invocation, allowlist, destructive);
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
                prominent: self.prominent,
            }),
        }
    }

    /// Resolve the decision, asking the user where the rule says to.
    ///
    /// # A missing confirmer is a refusal, not a pass
    ///
    /// `None` refuses any call that needed a prompt. This is the shape
    /// ADR-0007 D8's apex gate already uses in this crate, for the reason
    /// that record gives: "Never silent, never a default."
    #[must_use]
    pub fn permit(&self, confirmer: Option<&dyn Confirm>) -> Permission {
        let Some(question) = self.question() else {
            return Permission::Granted;
        };
        match confirmer {
            None => Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
            Some(confirmer) => {
                if confirmer.confirm(&question) {
                    Permission::Granted
                } else {
                    Permission::Refused(RefusedBecause::TheUserDeclined)
                }
            }
        }
    }
}
