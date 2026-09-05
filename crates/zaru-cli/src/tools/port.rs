// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The three ports the permission decision calls out through, none of them
//! implemented here.
//!
//! **All three have a product implementation as of 2026-09-05**, and until
//! then none of them did: [`allowlist::Allowed`](crate::tools::allowlist)
//! reads ADR-0014 D1's layer 2,
//! [`destructive::Shapes`](crate::tools::destructive) answers for the two of
//! ADR-0011 D6's four categories whose shape the record's own words
//! determine, and [`prompt::Prompt`](crate::tools::prompt) asks over a
//! terminal. What each one deliberately does **not** decide is written on its
//! own module.
//!
//! Two of the three were ports for as long as they were because writing what
//! they answer is **authoring a security vocabulary**, which [Autonomous
//! Development] puts on the human side of the boundary: "Adding a name to a
//! capability set, to a permission taxonomy, or to anything else deciding
//! what a model-driven action may reach **is a decision rather than the
//! implementation of one**." That constraint did not go away when the
//! implementations arrived — it is why the allowlist matches byte for byte
//! and never by glob, and why two of D6's four categories match nothing at
//! all.
//!
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::tools::decision::Invocation;

/// What a user has pre-approved, for ADR-0011 D3's `allow` mode.
///
/// D3: "`allow` — Runs the project allowlist without prompting; prompts for
/// anything outside it."
///
/// # Why this is a port and not code
///
/// **No record defines an allowlist.** D3 names one in a single clause;
/// ADR-0014 gives five configuration layers and no schema for this one, and
/// its own Neutral consequence says each record owns its own keys. Writing a
/// format — what a rule matches, whether a path rule is a glob or a prefix,
/// whether a command rule names a binary or a whole line — is authoring a
/// permission taxonomy, so it stayed a declared seam with no implementation
/// until a record settled it.
///
/// **It was settled on 2026-09-05** by a delegated coordinator ruling under
/// Jeshua's directive of that day, open to his veto, and
/// [`allowlist`](crate::tools::allowlist) is the implementation. The format
/// answers each of those questions in the direction that decides least: an
/// entry is the line the prompt already showed, matched byte for byte, never
/// a glob and never a prefix.
///
/// # Which layer an implementation may read, and why that is narrower than D3
///
/// D3 calls it "the **project** allowlist". [ADR-0014] D6 says "A repository
/// the user cloned must not be able to configure its way to more privilege
/// than the user granted", and an allowlist honoured without prompting is a
/// repository buying itself fewer prompts. The two cannot both hold as
/// written.
///
/// Under a delegated coordinator ruling of 2026-09-04, confirmed and closed on
/// 2026-09-05, **D6 wins**: D3 is amended to "the user's allowlist", and
/// [`allowlist::Allowed`](crate::tools::allowlist) reads the user's own layer
/// only. ADR-0015 D4's per-project admission is the mechanism a record would
/// use to admit a project's list one day and **it is not built**.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub trait Allowlist {
    /// Whether this exact call has already been approved by the user.
    ///
    /// The whole invocation is passed rather than a name and a string,
    /// because "pre-approved" is a property of the tool *and* its target
    /// together: a user who approved reading one path has said nothing about
    /// running a command.
    fn approves(&self, invocation: &Invocation<'_>) -> bool;
}

/// Whether a call is one of the shapes ADR-0011 D6 says to surface.
///
/// D6: "A pattern list — recursive removal, force-push, disk operations,
/// package-manager global installs — raises the prompt's prominence and
/// annotates the transcript entry. **It does not veto.**"
///
/// # Why this is a port and not a list of patterns
///
/// D6 names four categories and no patterns, and turning a category into a
/// matcher is authoring a security vocabulary. The record's own Negative
/// consequence says why getting it wrong is expensive in a direction that is
/// hard to see: "Pattern-matching destructive commands produces false
/// positives, and a prompt that cries wolf gets dismissed reflexively. **The
/// list must stay short.**" A list drafted by an implementer is a list nobody
/// chose to keep short.
///
/// The four categories are this port's documented contract. An implementation
/// answers for them and for nothing else.
///
/// **[`destructive::Shapes`](crate::tools::destructive) is that
/// implementation as of 2026-09-05**, and it holds the constraint rather than
/// escaping it: it transcribes the two categories whose shape D6's own words
/// determine, and the two that name no program **match nothing**, pinned by a
/// check so that adding one is a visible act.
///
/// # It never vetoes, and that is checkable from here
///
/// This port returns a `bool` that reaches only the prompt's prominence and
/// the transcript annotation. There is no route from it to a refusal, because
/// [`Decision`](crate::tools::decision::Decision) has no variant it could
/// reach — which is D6's "Surfacing beats forbidding" as a property of the
/// types rather than a rule somebody remembered.
pub trait DestructiveMatch {
    /// Whether this call matches one of D6's four categories.
    fn is_destructive(&self, invocation: &Invocation<'_>) -> bool;
}

/// What the user is asked, and what they are told when they are asked it.
///
/// The statement is composed once, by the decision, and handed here — rather
/// than composed where it is rendered — so that what the user was told and
/// what the harness believes it asked cannot drift apart. That is the same
/// shape [`credentials::Confirm`](crate::credentials::Confirm) uses for
/// ADR-0007 D8's `grants` sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The whole sentence the prompt states, including ADR-0011 D4's
    /// out-of-tree marking and D6's annotation where they apply.
    pub statement: String,
    /// Whether D6 matched, so a renderer can raise the prompt's prominence
    /// without re-deriving why.
    pub prominent: bool,
}

/// Why an ask did not reach the user.
///
/// Not an answer and not a refusal by the harness: the question was never
/// put. A terminal that closed between the statement and the answer is the
/// ordinary cause.
///
/// It carries a sentence rather than an error chain, for the reason
/// [`OverflowFailure`](crate::tools::output::OverflowFailure) does: the
/// implementations are a terminal, and one day a graphical prompt, and there
/// is no error type the two share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmFailure {
    detail: String,
}

impl ConfirmFailure {
    /// Say why the user was not reached.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// What went wrong, in the implementation's own words.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl core::fmt::Display for ConfirmFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for ConfirmFailure {}

/// How the user answers ADR-0011 D3's prompt.
///
/// # A missing answer and a wrong answer are different things
///
/// A confirmation nobody can answer is the silent default the record forbids,
/// so a call that needs one and has no confirmer is **refused** rather than
/// performed — see
/// [`Decision::permit`](crate::tools::decision::Decision::permit). That is the
/// same refusal ADR-0007 D8's apex gate makes for the same reason.
///
/// **The answer is a `Result` rather than a `bool`, decided 2026-09-05.**
/// Until then an implementation whose ask failed mid-prompt had only `false`
/// to return, and `false` is
/// [`TheUserDeclined`](crate::tools::decision::RefusedBecause::TheUserDeclined)
/// — a transcript entry saying the user declined when nobody was asked
/// anything. `Err` reaches
/// [`ThereWasNobodyToAsk`](crate::tools::decision::RefusedBecause::ThereWasNobodyToAsk)
/// instead, which is exactly true of a question that did not reach a person,
/// however it failed to. A delegated coordinator ruling of 2026-09-05, open
/// to Jeshua's veto.
///
/// What that costs, stated rather than discovered: the
/// [`ConfirmFailure`]'s own sentence is not carried into the refusal, because
/// `RefusedBecause` is a closed set of *outcomes* and widening it to carry a
/// diagnosis would make a permission outcome an error report. A caller that
/// wants the detail has it at the call site.
///
/// Rendering a rich prompt is `zaru-tui`'s; the decision is not the
/// terminal's. [`Prompt`](crate::tools::prompt::Prompt) is the plain one this
/// crate owns, over a terminal, and it is the first implementation of this
/// trait anywhere outside a check.
pub trait Confirm {
    /// Ask the user, having stated what is about to happen.
    ///
    /// # Errors
    ///
    /// [`ConfirmFailure`] when the question did not reach the user at all.
    /// **Never** for an answer of no, which is `Ok(false)`.
    fn confirm(&self, question: &Question) -> Result<bool, ConfirmFailure>;
}

/// Executes a command line. `cmd.run`.
///
/// # It takes a [`CommandLine`](crate::process::CommandLine) and not a
/// string, and that is the no-shell rule
///
/// Splitting text into a program and arguments is
/// [`crate::process::line`]'s, and it happens **before** the permission
/// decision, because [ADR-0011] D4's transcript entry and D3's prompt both
/// show the command and a string that has not been split is not yet one. So
/// by the time this port is reached the shell constructs are already refused
/// and the program is already separated from its arguments; an implementation
/// has nothing left to interpret, which is what makes "the harness runs no
/// shell" a property of this signature.
///
/// The product implementation is [`Spawn`](crate::process::Spawn), which is
/// the first thing in this workspace to start a child process. It is also the
/// tool ADR-0011 D6's four destructive categories are about, and since
/// 2026-09-05 [`DestructiveMatch`] has an implementation — so a `cmd.run`
/// matching one of the two transcribed categories is given the prompt
/// prominence D6 requires.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub trait Subprocess {
    /// Run the command line and capture what it produced.
    fn run(
        &self,
        line: &crate::process::line::CommandLine,
    ) -> impl core::future::Future<
        Output = Result<crate::tools::output::Captured, zaru_core::iteration::PortFailure>,
    > + Send;
}

/// Retrieves a URL. `web.fetch`.
///
/// # It takes a [`RequestedUrl`](crate::web::RequestedUrl) and not a string,
/// and that is the scheme rule
///
/// Parsing text into a URL is [`crate::web::url`]'s, and it happens **before**
/// the permission decision, because [ADR-0011] D4's transcript entry and D3's
/// prompt both show the target and a string nobody has parsed is not yet one.
/// It is the same signature [`Subprocess`] has and it carries the same
/// guarantee: by the time this port is reached the scheme is already one of
/// the two `web.fetch` retrieves, so an implementation has nothing left to
/// decide about what kind of thing it was handed. A `file://` URL cannot
/// reach here at all.
///
/// # No credential can be attached to a request made through it
///
/// One parameter, and it is a URL. There is no header argument, no options
/// struct and no builder — so the harness's held secrets cannot travel on a
/// request by any route, which is a property of this signature rather than a
/// rule an implementation keeps. [ADR-0011] D1's argument contract gives
/// `web.fetch` the single field `url`, so there is no route from the model
/// either.
///
/// # A URL allowlist is not this
///
/// D4's boundary is about paths and says nothing about outbound destinations,
/// so **no record answers which URLs a model may choose** and nothing here
/// classifies one. What [`crate::web::Destinations`] refuses is narrower and
/// structural — this machine and the link-local range — and it is not an
/// allowlist: it names no site, admits no configuration and cannot be widened
/// without a record. The allowlist question itself is open and is recorded on
/// ADR-0011 rather than answered by an implementer.
///
/// The product implementation is [`WebClient`](crate::web::WebClient), over
/// the one `reqwest::Client` this workspace builds.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
pub trait Fetch {
    /// Retrieve the URL and capture what came back.
    fn retrieve(
        &self,
        url: &crate::web::url::RequestedUrl,
    ) -> impl core::future::Future<
        Output = Result<crate::tools::output::Captured, zaru_core::iteration::PortFailure>,
    > + Send;
}
