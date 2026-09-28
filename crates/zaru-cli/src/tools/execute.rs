// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The acting half of [ADR-0011]: what happens after the permission decision
//! says a call may act.
//!
//! # All seven act
//!
//! | Tool | Here |
//! | --- | --- |
//! | `fs.read`, `fs.list` | [`files`], on `std::fs` inside D4's boundary |
//! | `fs.write`, `fs.edit` | [`files`], through [`crate::atomic`] at the file's own mode |
//! | `fs.search` | [`files`], walking `std::fs` under D4's classified root |
//! | `cmd.run` | [`Subprocess`], over [`Spawn`](crate::process::Spawn), started at D4's boundary root |
//! | `web.fetch` | [`Fetch`], over [`WebClient`](crate::web::WebClient) and the one HTTP client this workspace builds |
//!
//! **`fs.write` and `fs.edit` act as of 2026-09-05.** They were ported until
//! then because D4's classification resolves a candidate through its longest
//! **existing** ancestor, so a write is classified against a tree that does not
//! yet contain what it is about to make. That is still true and is not closed
//! by building them: it is the same check-at-a-moment the read path already
//! has, costing more, and [`files`] says exactly what is and is not claimed.
//! **`fs.search` acts as of 2026-09-05** over a literal needle and a literal
//! filename substring, taking neither a regular-expression engine — which is
//! not in [ADR-0003] D2's table — nor a glob semantics this crate would be
//! inventing. **`web.fetch` acts as of 2026-09-05**, over `http` and `https`
//! only, following no redirect across a host, refusing this machine and the
//! link-local range, and refusing a body over a caller-passed ceiling rather
//! than cutting one short — see [`crate::web`] for each of those and for what
//! is deliberately not claimed.
//!
//! **`cmd.run` acts as of 2026-09-05**, and it is not classified against D4 at
//! all: a command addresses a command line, and its boundary is the working
//! directory `Spawn` starts it in. Nothing contains it there — ADR-0011 D2,
//! "the harness is not a sandbox and says so" — and the module documentation
//! of [`crate::process::spawn`] says exactly what that costs.
//!
//! # A decision is derived here, never accepted from a caller
//!
//! [`Executor::execute`] builds the [`Invocation`] from the request it was
//! handed and classifies it itself. There is no way to hand it a decision
//! made about something else, so a decision reached about an `fs.read`
//! cannot authorise an `fs.write` — a property of the signature rather than
//! of a code path, and one of this arc's security-corpus cases.
//!
//! # What the boundary does and does not hold, stated rather than implied
//!
//! D4's classification is a check at a moment. Between the moment a path is
//! resolved and the moment the file is opened, a segment that did not exist
//! can be created as a symlink out of the tree, and `std` offers no
//! `openat2(RESOLVE_BENEATH)` to close that. **No containment is claimed
//! against a concurrent attacker**, and at `bare` tier that is inside what
//! D2 already says out loud — "the harness is not a sandbox and says so". At
//! `contained` tier the membrane is the answer, which is what [ADR-0004]
//! exists for. Raised as an open question on ADR-0011 rather than papered
//! over, and named in the corpus as a documented limit.
//!
//! # Where a SEAL verdict would gate a call
//!
//! [`Verdicts`] is consulted for every call **whatever the permission mode
//! is**, because [ADR-0011] D3 says mode governs prompting only and "a user
//! in `yolo` inside a membrane is still inside the membrane". The mode is not
//! an input to that path and there is nothing there for it to reach, which is
//! absence rather than a branch. Nothing in any product tree implements it,
//! and [ADR-0004] is blocked upstream on conformance vectors.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::session::{Record, ToolCall, Transcript, TranscriptError};
use crate::tools::arguments::Call;
use crate::tools::decision::{
    Decision, Invocation, Permission, RefusedBecause, Subject, TranscriptEntry,
};
use crate::tools::files;
use crate::tools::grants::SessionGrants;
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::output::{Captured, OutputBudget, Overflow, Presented};
use crate::tools::port::{Allowlist, Confirm, DestructiveMatch, Fetch, Retrieved, Subprocess};
use crate::tools::seal::{Verdict, Verdicts};
use crate::tools::tree::WorkingDirectory;
use crate::web::url::RequestedUrl;
use std::path::PathBuf;
use zaru_core::iteration::PortFailure;
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{
    ToolDecision, ToolDescriptor, ToolExecutor, ToolOutcome, ToolRequest, ToolResult,
};

/// What a request could not be turned into a call on.
///
/// Every variant is the **model** having asked for something that is not a
/// call, rather than the harness refusing one. ADR-0011 D6 gives the harness
/// no veto and none of these is one: a name that is not a built-in is not a
/// call at all, and a call with no target is not addressed to anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotACall {
    /// The name is not one of ADR-0011 D1's seven.
    NoSuchTool {
        /// What was asked for.
        asked: String,
    },
    /// A projected call carried a value the credential store holds.
    ///
    /// **Refused rather than redacted, and that is the stronger of the two.**
    /// A redaction puts a marker on the copy a person reads and still sends
    /// the value; a refusal sends nothing. ADR-0007 D3: "a model that can read
    /// its own bearer token can exfiltrate it through any tool that takes a
    /// string" -- and a projected tool takes strings, so this is that sentence
    /// applied to the surface D5 opens.
    ///
    /// **It carries no value and no marker.** What it names is the tool that
    /// was asked for, so a reader can find the call; what it deliberately does
    /// not name is which credential matched, because that is a fact about the
    /// store and this sentence reaches the model.
    CarriedAHeldValue {
        /// The declared name that was asked for.
        asked: String,
    },
    /// A projected call carried no arguments at all.
    CarriedNoArguments {
        /// The declared name that was asked for.
        asked: String,
    },
    /// The arguments carried no target.
    NoTarget {
        /// Which built-in.
        tool: ToolName,
    },
    /// A `cmd.run` carried something that is not a command line.
    ///
    /// **Not a refusal by the harness**, for the same reason its two siblings
    /// are not: a string carrying a shell construct is not a call this
    /// surface can make, so there is nothing to permit or deny. The model is
    /// told which construct and the turn carries on.
    ///
    /// **No transcript record is written**, which is the landed shape for
    /// every `NotACall`: ADR-0011 D4's "mode may remove the prompt; it never
    /// removes the record" is about a *call*, and nothing was attempted here.
    /// The refusal is reached before the decision and before the first
    /// `Phase::Started`, so a resumed session cannot read it as an
    /// interruption.
    NotACommandLine {
        /// Why, in the splitter's own words.
        because: crate::process::line::NotACommandLine,
    },
    /// A `web.fetch` carried something that is not a URL it can retrieve.
    ///
    /// **Not a refusal by the harness**, for the same reason its siblings are
    /// not: text that is not a URL, or one whose scheme this surface does not
    /// retrieve, is not a call this surface can make, so there is nothing to
    /// permit or deny. The model is told which and the turn carries on.
    ///
    /// **No transcript record is written**, because the refusal is reached
    /// before the decision and before the first `Phase::Started`. A
    /// *destination* this surface declines to reach is a different thing and
    /// **is** recorded — see [`crate::web::Destinations`].
    NotARetrievableUrl {
        /// Why, in the parser's own words.
        because: crate::web::url::UrlRefused,
    },
    /// The arguments are not the JSON object the tool declares.
    ///
    /// The same shape as its two siblings and for the same reason: a request
    /// that carries no target is not a call, and neither is one whose target
    /// cannot be found in what arrived. **No transcript record is written**,
    /// because the refusal is reached before the decision and before the first
    /// `Phase::Started`.
    ///
    /// See [`crate::tools::arguments`] for the contract and for why no
    /// refusal on this path renders a field's value.
    NotTheDeclaredArguments {
        /// Why, in the parser's own words.
        because: crate::tools::arguments::ArgumentsRefused,
    },
}

impl core::fmt::Display for NotACall {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSuchTool { asked } => write!(
                f,
                "{asked:?} is not one of the seven built-in tools, so there is \
                 nothing to call. The set is closed and there is no eighth"
            ),
            Self::CarriedAHeldValue { asked } => write!(
                f,
                "the call to {asked:?} carried a credential this harness holds, so it was not \
                 made; a stored secret never leaves the store, and a tool that takes a string is \
                 how one would"
            ),
            Self::CarriedNoArguments { asked } => write!(
                f,
                "the call to {asked:?} carried no arguments, and every tool on a projected server \
                 takes a JSON object"
            ),
            Self::NoTarget { tool } => write!(
                f,
                "the call to {tool} carried no target, so there is nothing to address it to"
            ),
            Self::NotACommandLine { because } => write!(f, "{because}"),
            Self::NotARetrievableUrl { because } => write!(f, "{because}"),
            Self::NotTheDeclaredArguments { because } => write!(f, "{because}"),
        }
    }
}

impl std::error::Error for NotACall {}

/// What a request turned out to be.
///
/// Two arms for the two halves of ADR-0011 D1: the seven, and "everything
/// beyond this is an MCP server". A projected call is not a [`Call`], because
/// [`Call`] is an enum over the seven and ADR-0007 D5's tools are not among
/// them — see [`Called`](crate::tools::Called).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Requested {
    /// One of D1's seven, with its arguments read.
    Builtin(Call),
    /// A tool on a projected server, with its arguments unread.
    Projected {
        /// Which token's namespace.
        alias: crate::credentials::Alias,
        /// The tool, as that instance spells it.
        tool: String,
        /// The arguments, as JSON, exactly as the model wrote them.
        arguments: String,
    },
}

/// The alias and the tool inside a declared `notes:<alias>.<tool>` name.
///
/// Reads the name this crate composed, so the two spellings cannot drift: the
/// prefix and the separators are [`crate::credentials::NAMESPACE_PREFIX`] and
/// the colon and dot [`Called::rendered`](crate::tools::Called::rendered)
/// writes. **The alias cannot contain a colon** — `Alias::new` refuses one by
/// name — so the first colon ends the prefix and the first dot after it ends
/// the alias.
fn split_projected(declared: &str) -> Option<(crate::credentials::Alias, String)> {
    let bare = declared.strip_prefix(&format!("{}:", crate::credentials::NAMESPACE_PREFIX))?;
    let (alias, tool) = bare.split_once('.')?;
    if tool.is_empty() {
        return None;
    }
    Some((crate::credentials::Alias::new(alias).ok()?, tool.to_owned()))
}

/// Everything the acting half needs, and every one of it a caller's.
///
/// Bundled for the reason `zaru-core`'s port bundles are: a constructor
/// taking eleven arguments is a constructor whose order is a thing to get
/// wrong.
pub struct Executor<'a, C, F, P> {
    /// D4's boundary, canonical from construction.
    pub working_directory: &'a WorkingDirectory,
    /// D3's mode. Governs prompting and nothing else.
    pub mode: Mode,
    /// D3's allowlist. The product implementation is
    /// [`Allowed`](crate::tools::Allowed), reading ADR-0014 D1's layer 2.
    pub allowlist: &'a (dyn Allowlist + Sync),
    /// D3's third answer, held for the life of the session.
    ///
    /// What the user allowed at a prompt, for this exact line, for the rest of
    /// this session. **Never written to any configuration layer** — see
    /// [`SessionGrants`]. It is a shared
    /// reference because the grant set outlives the executor: an `Executor` is
    /// built per turn and a session has many.
    pub session_grants: &'a SessionGrants,
    /// D6's four categories. The product implementation is
    /// [`Shapes`](crate::tools::Shapes), which answers for the two of them
    /// whose shape D6's own words determine and matches nothing for the two
    /// that name no program.
    pub destructive: &'a (dyn DestructiveMatch + Sync),
    /// D3's prompt. The product implementation is
    /// [`Prompt`](crate::tools::prompt::Prompt), over a terminal. `None`
    /// refuses any call that needed one — and so does a confirmer whose ask
    /// could not reach the user, for the same reason and with the same
    /// outcome.
    pub confirmer: Option<&'a (dyn Confirm + Sync)>,
    /// ADR-0004's membrane. No product implementation.
    pub verdicts: &'a (dyn Verdicts + Sync),
    /// D5's budget, refused at zero by its own constructor.
    pub budget: OutputBudget,
    /// How much of a call's arguments D3's question formats.
    ///
    /// A second budget rather than [`Executor::budget`] reused, because the
    /// two bound different things for different readers — a model's context
    /// window and a person's terminal. The binary's is
    /// [`crate::cli::layers::preview_budget`].
    pub preview_budget: OutputBudget,
    /// The largest file `fs.search` will read the contents of.
    ///
    /// Caller-passed and refused at zero, in the shape [`OutputBudget`] and
    /// ADR-0007's `Ttl` already use. No record names a number and this
    /// module invents none; the binary's is
    /// [`crate::cli::layers::search_ceiling`].
    pub search_ceiling: crate::config::SizeCeiling,
    /// D5's overflow sink.
    pub overflow: &'a mut (dyn Overflow + Send),
    /// ADR-0010 D2's transcript. Written around every call.
    pub transcript: &'a mut Transcript,
    /// ADR-0008 clause 6's port, decided 2026-09-05.
    ///
    /// Applied where a capture becomes the text the model is given, and
    /// nowhere else in this module. The `Captured` the transcript and the
    /// overflow sink are written from is untouched: ADR-0010's Negative
    /// section says the record "contain\[s\] whatever the session contained",
    /// and this is the difference between redacting a prompt and redacting a
    /// record.
    pub redactor: &'a (dyn Redactor + Sync),
    /// `cmd.run`. No product implementation.
    pub subprocess: &'a C,
    /// `web.fetch`. The product implementation is
    /// [`WebClient`](crate::web::WebClient).
    pub fetch: &'a F,
    /// [ADR-0007] D5's projected servers. The product implementation is
    /// [`Projection`](crate::credentials::Projection).
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    pub projected: &'a P,
    /// The whole surface this session offers the model.
    ///
    /// **Built once where the session is composed and borrowed here**, rather
    /// than returned from a `OnceLock` like the seven were. A projected
    /// server's tools are a session's — they come from the store D6 cached
    /// and the grant a person wrote — so a process-wide list could not hold
    /// them. Both [`ToolExecutor`] implementations are handed a slice of the
    /// same `Vec`, which keeps the property the `OnceLock` existed to hold:
    /// the set the model is offered and the set this executor will accept are
    /// one set. See [`crate::tools::declared`].
    pub declared: &'a [ToolDescriptor],
}

impl<C, F, P> core::fmt::Debug for Executor<'_, C, F, P> {
    /// Names what it holds and renders none of it.
    ///
    /// A tool surface's `Debug` is a thing that ends up in a bug report, and
    /// what it holds includes a working directory and a transcript. Rendering
    /// the ports would say nothing a reader can use and rendering the
    /// transcript would put a session's contents in a panic message.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Executor")
            .field("working_directory", &self.working_directory.root())
            .field("mode", &self.mode)
            .field("has_confirmer", &self.confirmer.is_some())
            .field("declared", &self.declared.len())
            .finish_non_exhaustive()
    }
}

/// The descriptors ADR-0011 D1's seven are offered under.
///
/// Built from [`ToolName::ALL`] rather than from a list retyped here, so an
/// eighth built-in cannot be offered without existing, and the seven cannot
/// drift from what the executor will accept — the two lists are one walk.
#[must_use]
pub fn descriptors() -> Vec<ToolDescriptor> {
    ToolName::ALL
        .into_iter()
        .map(|tool| ToolDescriptor {
            name: tool.as_str().to_owned(),
            description: tool.purpose().to_owned(),
            // ADR-0011 D1 named no argument schema, and this surface offered
            // an empty one until 2026-09-05 -- which said truthfully that the
            // record specified none, and which is **not JSON**, so the first
            // provider client to be handed these seven would have refused all
            // seven. The contract is decided under directive 20 and derived
            // from `ToolName::fields`, so the schema a model is shown and the
            // object `Call::parse` accepts are one list.
            parameters: crate::tools::arguments::schema(tool),
        })
        .collect()
}

/// The same seven, as a slice that outlives any borrow of a caller.
///
/// [`ToolExecutor::descriptors`] returns `&[ToolDescriptor]` borrowed from
/// `&self`, which a caller reaching this surface through a lock cannot
/// produce: the slice would borrow the guard and the guard is dropped at the
/// end of the call. So the list lives in a `OnceLock` here, and **both
/// implementations return this one** — the [`Executor`]'s and
/// [`Shared`](crate::compose::Shared)'s — because the set the model is
/// offered and the set the executor will accept being one set is a property
/// this module already holds, and two accessors over two lists would give it
/// away.
///
/// Owned by the caller and handed in would be a second list; built per call
/// from [`ToolName::ALL`] would need somewhere to live across the borrow. A
/// `OnceLock` over a value derived from a compile-time constant is neither:
/// one walk, one allocation, and the same slice every time.
#[must_use]
pub fn descriptor_set() -> &'static [ToolDescriptor] {
    static DESCRIPTORS: std::sync::OnceLock<Vec<ToolDescriptor>> = std::sync::OnceLock::new();
    DESCRIPTORS.get_or_init(descriptors)
}

impl<C, F, P> Executor<'_, C, F, P>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
    P: crate::tools::port::Projected + Sync,
{
    /// Turn a request into a call this surface can classify.
    ///
    /// # Errors
    ///
    /// [`NotACall`] when the name is not a built-in, the arguments are empty,
    /// or they are not the JSON object [`ToolName::fields`] declares.
    fn call_for(&self, request: &ToolRequest) -> Result<Requested, NotACall> {
        // **A projected name is recognised against what this session actually
        // declared**, never by parsing the shape of the name. A model that
        // invented `notes:someone.pages.apply_patch` has asked for something
        // this session did not offer, and it is told so -- the same answer it
        // gets for an invented built-in, by the same route.
        if let Some(descriptor) = self
            .declared
            .iter()
            .find(|descriptor| descriptor.name == request.name)
            && let Some((alias, tool)) = split_projected(&descriptor.name)
        {
            let arguments = request.arguments.trim();
            if arguments.is_empty() {
                return Err(NotACall::CarriedNoArguments {
                    asked: request.name.to_string(),
                });
            }
            // ADR-0007 D3, at the one surface that could carry a value out.
            // `Redactor::redact` answers `Cow::Borrowed` when nothing matched,
            // which its own documentation says is there so "a caller can
            // distinguish 'nothing was redacted' from 'something was' without
            // comparing strings" -- so this asks the seam rather than the
            // store, and never holds a secret to compare against.
            if matches!(self.redactor.redact(arguments), std::borrow::Cow::Owned(_)) {
                return Err(NotACall::CarriedAHeldValue {
                    asked: request.name.to_string(),
                });
            }
            return Ok(Requested::Projected {
                alias,
                tool,
                arguments: arguments.to_owned(),
            });
        }

        let tool = ToolName::ALL
            .into_iter()
            .find(|tool| tool.as_str() == request.name)
            .ok_or_else(|| NotACall::NoSuchTool {
                asked: request.name.to_string(),
            })?;
        let arguments = request.arguments.trim();
        // Kept as its own refusal rather than folded into the parser's
        // "not JSON" arm, because empty arguments and malformed arguments are
        // different mistakes and the first has a shorter fix.
        if arguments.is_empty() {
            return Err(NotACall::NoTarget { tool });
        }
        Call::parse(tool, arguments)
            .map(Requested::Builtin)
            .map_err(|because| NotACall::NotTheDeclaredArguments { because })
    }

    /// Do the thing, having been permitted to.
    ///
    /// Four of the seven leave through a port with no product implementation.
    /// `fs.read` and `fs.list` act on `std::fs` because neither can create a
    /// path; `cmd.run` acts through [`Spawn`](crate::process::Spawn).
    ///
    /// The invocation is taken rather than the raw arguments because it is
    /// what carries the **classified** path: re-resolving a target here would
    /// be D4's rule in two places, and the second one is the one nothing
    /// prompted about. A `cmd.run` has also already been split, and splitting
    /// it twice would be a second set of refusals that could disagree with the
    /// first.
    async fn act(
        &mut self,
        invocation: &Invocation<'_>,
        call: &Requested,
    ) -> Result<Captured, PortFailure> {
        // Every filesystem act reads its path out of the subject the decision
        // was reached about, so a decision about one path cannot authorise an
        // act on another.
        match (invocation.subject(), call) {
            // ADR-0007 D5, through the port. The bearer is resolved behind it
            // and never crosses it; the arguments go out exactly as the
            // decision was reached about them.
            (
                Subject::Remote { .. },
                Requested::Projected {
                    alias,
                    tool,
                    arguments,
                },
            ) => self.projected.call(alias, tool, arguments).await,
            (Subject::Path(target), Requested::Builtin(Call::OnPath { .. })) => {
                Ok(files::list(target.resolved()))
            }
            (
                Subject::Path(target),
                Requested::Builtin(Call::Read {
                    start_line,
                    line_count,
                    ..
                }),
            ) => Ok(crate::tools::reading::read(
                target.resolved(),
                crate::tools::reading::Lines {
                    start: *start_line,
                    count: *line_count,
                },
                self.budget,
            )),
            (Subject::Write { target, .. }, Requested::Builtin(Call::Write { contents, .. })) => {
                Ok(files::write(target.resolved(), contents))
            }
            (Subject::Edit { target, .. }, Requested::Builtin(Call::Edit { old, new, .. })) => {
                Ok(files::edit(target.resolved(), old, new))
            }
            (Subject::Search { root, needle }, Requested::Builtin(Call::Search { .. })) => {
                Ok(files::search(root.resolved(), needle, self.search_ceiling).await)
            }
            (Subject::Command(line), Requested::Builtin(Call::Run { .. })) => {
                self.subprocess.run(line).await
            }
            (Subject::Url(url), Requested::Builtin(Call::Fetch { .. })) => {
                self.fetch_following(url).await
            }
            // Unbuildable: `Executor::execute` derives the subject from the
            // call it just parsed, and each constructor takes one kind. It is
            // reported as a port failure rather than panicked on, because it
            // would be the harness having built the wrong call — a defect
            // rather than anything the user or the model did.
            _ => Err(PortFailure::new(
                "a tool call was described with a subject of the wrong kind, which is a defect in \
                 the harness rather than anything the call asked for",
            )),
        }
    }
}

impl<C, F, P> Executor<'_, C, F, P>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
    P: crate::tools::port::Projected + Sync,
{
    /// `web.fetch`, following a redirect to another host only as a new call
    /// to that URL would be allowed.
    ///
    /// Until 2026-09-28 a redirect to another host was never followed. Now
    /// the permission decision is reached about the new URL, exactly as for a
    /// call the model made: in `ask` mode the person is asked, showing the
    /// whole new URL, and with nobody to ask it is refused and the model is
    /// told why. Each hop is recorded on the transcript as a call of its own.
    async fn fetch_following(&mut self, first: &RequestedUrl) -> Result<Captured, PortFailure> {
        let mut url = first.clone();
        let mut followed = 0;
        let mut pending: Option<TranscriptEntry> = None;
        loop {
            let retrieved = self.fetch.retrieve(&url, followed).await?;
            if let Some(entry) = pending.take() {
                self.record(&Record::ToolCall(ToolCall::completed(&entry)))?;
            }
            let to = match retrieved {
                Retrieved::Captured(captured) => return Ok(captured),
                Retrieved::Elsewhere { to } => to,
            };
            followed += 1;
            let (entry, permission) = {
                let hop = Invocation::fetching(&to);
                let decision = Decision::assess(
                    self.mode,
                    &hop,
                    self.allowlist,
                    self.destructive,
                    self.session_grants,
                );
                // The same two gates a call passes: the membrane's verdict,
                // then the permission decision.
                let refused = match self.verdicts.verdict(&hop) {
                    Verdict::Denied { code, reason } => Some(format!("{code} — {reason}")),
                    Verdict::Allowed => match decision.permit_asking(self.confirmer).await {
                        Permission::Refused(because) => Some(because.to_string()),
                        Permission::GrantedForTheSession => {
                            self.session_grants.allow(&hop);
                            None
                        }
                        Permission::GrantedForTheHost => {
                            self.session_grants.allow_host(&hop);
                            None
                        }
                        Permission::Granted => None,
                    },
                };
                (decision.entry().clone(), refused)
            };
            self.record(&Record::ToolCall(ToolCall::started(&entry)))?;
            if let Some(because) = permission {
                self.record(&Record::ToolCall(ToolCall::refused(&entry)))?;
                return Ok(Captured {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: format!(
                        "the redirect from {} to {} was not followed: {because}",
                        url.host().escape_debug(),
                        to.host().escape_debug()
                    ),
                });
            }
            pending = Some(entry);
            url = to;
        }
    }
}

impl<C, F, P> ToolExecutor for Executor<'_, C, F, P>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
    P: crate::tools::port::Projected + Sync,
{
    fn descriptors(&self) -> &[ToolDescriptor] {
        self.declared
    }

    async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
        let call = match self.call_for(request) {
            Ok(call) => call,
            // The model asked for something that is not a call. It is told so
            // and the turn carries on, which is the same shape a refusal
            // takes: not a failure, and the model's to correct.
            Err(refused) => {
                return Ok(ToolOutcome::Refused {
                    decision: ToolDecision {
                        statement: refused.to_string(),
                        permitted: false,
                    },
                    id: request.id.clone(),
                    because: refused.to_string(),
                });
            }
        };

        // The subject is derived here, from the request, and the invocation is
        // built from it. A caller cannot supply either. Both bindings are
        // declared before the branch because the invocation borrows whichever
        // one its arm produced.
        let classified;
        let line;
        let requested;
        let invocation = match &call {
            // **No classification and no parse.** D4's boundary is about paths
            // and a projected call has none; the arguments are the instance's
            // to read across ninety-four tools this harness has never seen,
            // and a second reading here could disagree with the server's. What
            // the decision is reached about is exactly what would be sent.
            Requested::Projected {
                alias,
                tool,
                arguments,
            } => Invocation::projecting(alias.clone(), tool.as_str(), arguments.as_str()),
            Requested::Builtin(Call::Fetch { url }) => {
                // Parsed before the decision, for the reason `cmd.run` is
                // split before it: D4's transcript entry and D3's prompt both
                // show the target, and a string nobody has parsed is not yet
                // one. A scheme this surface does not retrieve is therefore
                // refused before anything is recorded or asked.
                requested = match crate::web::RequestedUrl::parse(url) {
                    Ok(requested) => requested,
                    Err(because) => {
                        let refused = NotACall::NotARetrievableUrl { because };
                        return Ok(ToolOutcome::Refused {
                            decision: ToolDecision {
                                statement: refused.to_string(),
                                permitted: false,
                            },
                            id: request.id.clone(),
                            because: refused.to_string(),
                        });
                    }
                };
                Invocation::fetching(&requested)
            }
            Requested::Builtin(Call::Run { command }) => {
                // Split before the decision, because D4's transcript entry
                // and D3's prompt both show the command, and a string that
                // has not been split is not yet one. A shell construct is
                // therefore refused before anything is recorded or asked.
                line = match crate::process::line::CommandLine::split(command) {
                    Ok(line) => line,
                    Err(because) => {
                        let refused = NotACall::NotACommandLine { because };
                        return Ok(ToolOutcome::Refused {
                            decision: ToolDecision {
                                statement: refused.to_string(),
                                permitted: false,
                            },
                            id: request.id.clone(),
                            because: refused.to_string(),
                        });
                    }
                };
                Invocation::running(&line)
            }
            Requested::Builtin(Call::Search { root, needle }) => {
                // The root is classified exactly as any other path is: D4
                // applies to where a search looks, and a search that started
                // outside the tree prompts and is marked like any other
                // out-of-tree call.
                classified = self.working_directory.classify(root);
                Invocation::searching(&classified, needle)
            }
            Requested::Builtin(Call::Write { path, contents }) => {
                // The contents travel with the invocation rather than being
                // looked up again where the question is composed: D3's prompt
                // and the act must be about the same bytes, and two reads of
                // one argument is two things that can come to disagree.
                classified = self.working_directory.classify(path);
                Invocation::writing(&classified, contents)
            }
            Requested::Builtin(Call::Edit { path, old, new }) => {
                classified = self.working_directory.classify(path);
                Invocation::editing(&classified, old, new)
            }
            Requested::Builtin(inner @ (Call::OnPath { path, .. } | Call::Read { path, .. })) => {
                classified = self.working_directory.classify(path);
                Invocation::on_path(inner.tool(), &classified).map_err(|refused| {
                    // A tool that is not described by a bare path given a path
                    // subject is the harness having built the wrong call,
                    // which is a defect rather than anything the user or the
                    // model did.
                    PortFailure::new(refused.to_string())
                })?
            }
        };

        // D3's question shows what it is about. The detail is composed once,
        // here, from the same invocation the decision was reached about — so
        // a question cannot describe one call and a decision another — and it
        // arrives already redacted, because whether a value is a secret is
        // not a thing a renderer can know. See `crate::tools::preview`.
        let decision = Decision::assess(
            self.mode,
            &invocation,
            self.allowlist,
            self.destructive,
            self.session_grants,
        )
        .showing(crate::tools::preview::detail_for(
            &invocation,
            self.preview_budget,
            self.redactor,
        ));
        let entry = decision.entry().clone();

        // ADR-0004 D2: at `contained` and above the membrane decides, and
        // ADR-0011 D3 says the mode governs prompting only. So the verdict is
        // asked for whatever the mode is, and a denial refuses regardless.
        if let Verdict::Denied { code, reason } = self.verdicts.verdict(&invocation) {
            let said = format!("{code} — {reason}");
            return self.refuse(request, &entry, said.clone(), said);
        }

        let statement = decision
            .question()
            .map_or_else(|| entry.render(), |question| question.statement);

        // **A call that is asked about is recorded as started before the
        // question is put**, so a turn stopped while the question stands --
        // `Ctrl-C`, or the terminal going away -- leaves a `Started` with
        // nothing closing it, which is how ADR-0010 D4 reads a call that did
        // not finish. It is the same record a call stopped while it ran
        // leaves, and the conversation closes both the same way. A call that
        // is refused after the question gets its `Refused` below, as before.
        let asked = decision.question().is_some();
        if asked {
            self.record(&Record::ToolCall(ToolCall::started(&entry)))?;
        }
        let permission = decision.permit_asking(self.confirmer).await;

        // The grant is remembered **before** the act, so a call that fails or
        // is interrupted mid-act does not lose the answer a person gave about
        // it. Recorded here rather than inside `Decision::permit`, which is
        // pure and holds no session.
        let statement = match permission {
            Permission::GrantedForTheSession => {
                self.session_grants.allow(&invocation);
                format!("{statement}{GRANTED_FOR_THE_SESSION}")
            }
            Permission::GrantedForTheHost => {
                self.session_grants.allow_host(&invocation);
                format!("{statement}{GRANTED_FOR_THE_HOST}")
            }
            Permission::Granted | Permission::Refused(_) => statement,
        };

        match permission {
            Permission::Refused(because) if asked => self.refused_having_started(
                request,
                &entry,
                statement,
                RefusedBecause::to_string(&because),
            ),
            Permission::Refused(because) => self.refuse(
                request,
                &entry,
                statement,
                RefusedBecause::to_string(&because),
            ),
            Permission::Granted
            | Permission::GrantedForTheSession
            | Permission::GrantedForTheHost => {
                // The record is written *before* the act, so a process killed
                // inside the act leaves a `Started` with nothing closing it —
                // which is what ADR-0010 D4's `Interrupted` is derived from. A
                // call that was asked about has had its `Started` already.
                if !asked {
                    self.record(&Record::ToolCall(ToolCall::started(&entry)))?;
                }
                let captured = self.act(&invocation, &call).await?;
                let presented = captured
                    .present(self.budget, self.redactor, Some(&mut *self.overflow))
                    .map_err(|refused| PortFailure::new(refused.to_string()))?;
                self.record(&Record::ToolCall(ToolCall::completed(&entry)))?;
                Ok(ToolOutcome::Completed {
                    decision: ToolDecision {
                        statement,
                        permitted: true,
                    },
                    result: ToolResult {
                        id: request.id.clone(),
                        content: render(self.redactor, &presented),
                        failed: presented.exit_code != 0,
                    },
                })
            }
        }
    }
}

impl<C, F, P> Executor<'_, C, F, P> {
    /// Append one record and carry a transcript failure out as a port failure.
    fn record(&mut self, record: &Record) -> Result<(), PortFailure> {
        self.transcript.record(record).map_err(|failure| {
            // The failure's own wording, never the record's contents: a
            // transcript holds whatever the session held.
            PortFailure::new(TranscriptError::to_string(&failure))
        })
    }

    /// Record a refusal and report it, without ever having acted.
    ///
    /// The `Started`/`Refused` pair rather than a bare `Started`: a `Started`
    /// alone would make a resumed session report a call the user declined as
    /// one that was interrupted.
    fn refuse(
        &mut self,
        request: &ToolRequest,
        entry: &TranscriptEntry,
        statement: String,
        because: String,
    ) -> Result<ToolOutcome, PortFailure> {
        self.record(&Record::ToolCall(ToolCall::started(entry)))?;
        self.refused_having_started(request, entry, statement, because)
    }

    /// Record the `Refused` that closes a call already recorded as started,
    /// and report it.
    fn refused_having_started(
        &mut self,
        request: &ToolRequest,
        entry: &TranscriptEntry,
        statement: String,
        because: String,
    ) -> Result<ToolOutcome, PortFailure> {
        self.record(&Record::ToolCall(ToolCall::refused(entry)))?;
        Ok(ToolOutcome::Refused {
            decision: ToolDecision {
                statement,
                permitted: false,
            },
            id: request.id.clone(),
            because,
        })
    }
}

/// What is added to a call's statement when the user allowed it for the
/// session.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-15 00:13:45Z,
/// open to Jeshua's veto**, and recorded on [ADR-0011's amendments volume 3].
///
/// D3's grant is an answer a person gave, and [ADR-0010] D2's transcript is
/// the record of what happened in a session — so a call that ran because of a
/// grant and a call that ran because someone answered `y` must not read the
/// same. Without this, the only difference between the two would be the
/// **absence** of a later prompt, which is a thing a reader cannot see. It is
/// appended to the statement rather than carried as a field, because
/// `ToolDecision` is a projection of what the user was told and the user was
/// told this.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface-updates-3
pub const GRANTED_FOR_THE_SESSION: &str = " — allowed for the rest of this session";

/// What is added to a `web.fetch` statement when the user allowed its host
/// for the session.
pub const GRANTED_FOR_THE_HOST: &str =
    " — every URL on this host allowed for the rest of this session";

/// What the model is shown of a capture.
///
/// D5: "Stdout and stderr are captured separately, both surfaced, and both
/// fed to the model." Both are here, labelled, and the path of any preserved
/// overflow with them — which is D5's "with the path shown".
///
/// # The port is applied twice on this path, and each pass has a job
///
/// [`Captured::present`] applies it to each stream **before** truncating, so
/// that a held value cannot be cut in half at the elision boundary and leave
/// its head behind. This function applies it again to the assembled text,
/// which is what produces the [`Redacted`] a [`ToolResult`] can be built
/// from — the type gate rather than the filter. Redaction is idempotent, so
/// the second pass changes nothing.
fn render<R: Redactor + ?Sized>(redactor: &R, presented: &Presented) -> Redacted {
    let mut out = format!("exit code: {}\n", presented.exit_code);
    out.push_str("stdout:\n");
    out.push_str(presented.stdout.as_str());
    out.push_str("\nstderr:\n");
    out.push_str(presented.stderr.as_str());
    if let Some(path) = &presented.full_text_at {
        out.push_str(&format!("\nfull output: {}\n", path.display()));
    }
    Redacted::by(redactor, &out)
}

/// [ADR-0011] D5's overflow sink, over [ADR-0010] D1's session directory.
///
/// The first product implementation of that port. D5 requires the full text
/// be written "to the session directory, with the path shown", and until that
/// directory existed a capture that overflowed with nowhere to keep the rest
/// was refused rather than clipped. It exists now.
///
/// The file is named by the capture's ordinal within the session, so two
/// overflows in one session do not overwrite each other and a reader can tell
/// which call each belongs to by its order in the transcript.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug)]
pub struct SessionOverflow {
    directory: PathBuf,
    next: u32,
}

/// How an overflow file is named, wherever one is named.
pub const OVERFLOW_PREFIX: &str = "output-";

impl SessionOverflow {
    /// Preserve overflowing captures in this session's directory.
    #[must_use]
    pub fn in_session(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
            next: 1,
        }
    }
}

impl Overflow for SessionOverflow {
    fn preserve(
        &mut self,
        captured: &Captured,
    ) -> Result<PathBuf, crate::tools::output::OverflowFailure> {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;

        let path = self
            .directory
            .join(format!("{OVERFLOW_PREFIX}{:04}.txt", self.next));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            // ADR-0010 D5: filesystem permissions are the only protection a
            // transcript has, and this file holds exactly what the transcript
            // would have held.
            .mode(crate::session::store::FILE_MODE)
            .open(&path)
            .map_err(|source| {
                crate::tools::output::OverflowFailure::new(format!(
                    "could not open {} to preserve the full output: {source}",
                    path.display()
                ))
            })?;
        let whole = format!(
            "exit code: {}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
            captured.exit_code, captured.stdout, captured.stderr
        );
        file.write_all(whole.as_bytes()).map_err(|source| {
            crate::tools::output::OverflowFailure::new(format!(
                "could not write {}: {source}",
                path.display()
            ))
        })?;
        file.sync_all().map_err(|source| {
            crate::tools::output::OverflowFailure::new(format!(
                "could not sync {}: {source}",
                path.display()
            ))
        })?;
        self.next += 1;
        Ok(path)
    }
}

#[cfg(test)]
mod tests;
