// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The three ports the permission decision calls out through, none of them
//! implemented here.
//!
//! **Nothing in this crate's product tree implements any of the three**,
//! exactly as nothing in `zaru-core`'s implements one of the loop's five and
//! nothing implements [`credentials`](crate::credentials)'s two. A check
//! implements them; the product does not, and that is why this arc prompts
//! nobody, reads no configuration and matches no command.
//!
//! Two of the three are ports specifically because writing what they answer
//! would be **authoring a security vocabulary**, which [Autonomous
//! Development] puts on the human side of the boundary: "Adding a name to a
//! capability set, to a permission taxonomy, or to anything else deciding
//! what a model-driven action may reach **is a decision rather than the
//! implementation of one**." An allowlist format and a destructive-command
//! pattern list are both exactly that.
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
/// permission taxonomy, and it would also need a parser for a file format
/// that is not in ADR-0003 D2's dependency table. So the shape that is honest
/// is a declared seam with no implementation, and the format stays the
/// record's to decide.
///
/// # Which layer an implementation may read, and why that is narrower than D3
///
/// D3 calls it "the **project** allowlist". [ADR-0014] D6 says "A repository
/// the user cloned must not be able to configure its way to more privilege
/// than the user granted", and an allowlist honoured without prompting is a
/// repository buying itself fewer prompts. The two cannot both hold as
/// written.
///
/// Under a delegated coordinator ruling of 2026-09-04, **D6 wins**: an
/// implementation of this port reads the user's own layer only, until a
/// record admits a project layer — ADR-0015 D4's per-project admission is the
/// obvious mechanism for admitting one and no record connects the two. The
/// conflict and this resolution are recorded as a proposed Update on ADR-0011
/// rather than settled by whichever layer an implementer happened to read.
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

/// How the user answers ADR-0011 D3's prompt.
///
/// # There is no implementation, and no default answer
///
/// A confirmation nobody can answer is the silent default the record forbids,
/// so a call that needs one and has no confirmer is **refused** rather than
/// performed — see
/// [`Decision::permit`](crate::tools::decision::Decision::permit). That is the
/// same refusal ADR-0007 D8's apex gate makes for the same reason.
///
/// Rendering the prompt is `zaru-tui`'s; the decision is not the terminal's.
pub trait Confirm {
    /// Ask the user, having stated what is about to happen.
    fn confirm(&self, question: &Question) -> bool;
}
