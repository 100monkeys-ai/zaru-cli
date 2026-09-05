// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The acting half of [ADR-0011]: what happens after the permission decision
//! says a call may act.
//!
//! # Six of the seven act, and one does not
//!
//! | Tool | Here |
//! | --- | --- |
//! | `fs.read`, `fs.list` | [`files`], on `std::fs` inside D4's boundary |
//! | `fs.write`, `fs.edit` | [`files`], through [`crate::atomic`] at the file's own mode |
//! | `fs.search` | [`files`], walking `std::fs` under D4's classified root |
//! | `cmd.run` | [`Subprocess`], over [`Spawn`](crate::process::Spawn), started at D4's boundary root |
//! | `web.fetch` | [`Fetch`], no implementation |
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
//! inventing. `web.fetch` needs a socket, and the harness has none.
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
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::output::{Captured, OutputBudget, Overflow, Presented};
use crate::tools::port::{Allowlist, Confirm, DestructiveMatch, Fetch, Subprocess};
use crate::tools::seal::{Verdict, Verdicts};
use crate::tools::tree::WorkingDirectory;
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
        /// What was asked for, escaped.
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
                "{asked:?} is not one of the seven built-in tools ADR-0011 D1 names, so there is \
                 nothing to call. The set is closed and there is no eighth"
            ),
            Self::NoTarget { tool } => write!(
                f,
                "the call to {tool} carried no target, so there is nothing to address it to"
            ),
            Self::NotACommandLine { because } => write!(f, "{because}"),
            Self::NotTheDeclaredArguments { because } => write!(f, "{because}"),
        }
    }
}

impl std::error::Error for NotACall {}

/// Everything the acting half needs, and every one of it a caller's.
///
/// Bundled for the reason `zaru-core`'s port bundles are: a constructor
/// taking eleven arguments is a constructor whose order is a thing to get
/// wrong.
pub struct Executor<'a, C, F> {
    /// D4's boundary, canonical from construction.
    pub working_directory: &'a WorkingDirectory,
    /// D3's mode. Governs prompting and nothing else.
    pub mode: Mode,
    /// D3's allowlist. The product implementation is
    /// [`Allowed`](crate::tools::Allowed), reading ADR-0014 D1's layer 2.
    pub allowlist: &'a (dyn Allowlist + Sync),
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
    /// `web.fetch`. No product implementation.
    pub fetch: &'a F,
}

impl<C, F> core::fmt::Debug for Executor<'_, C, F> {
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

impl<C, F> Executor<'_, C, F>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
{
    /// Turn a request into a call this surface can classify.
    ///
    /// # Errors
    ///
    /// [`NotACall`] when the name is not a built-in, the arguments are empty,
    /// or they are not the JSON object [`ToolName::fields`] declares.
    fn call_for(request: &ToolRequest) -> Result<Call, NotACall> {
        let tool = ToolName::ALL
            .into_iter()
            .find(|tool| tool.as_str() == request.name)
            .ok_or_else(|| NotACall::NoSuchTool {
                asked: request.name.escape_debug().to_string(),
            })?;
        let arguments = request.arguments.trim();
        // Kept as its own refusal rather than folded into the parser's
        // "not JSON" arm, because empty arguments and malformed arguments are
        // different mistakes and the first has a shorter fix.
        if arguments.is_empty() {
            return Err(NotACall::NoTarget { tool });
        }
        Call::parse(tool, arguments)
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
        call: &Call,
    ) -> Result<Captured, PortFailure> {
        // Every filesystem act reads its path out of the subject the decision
        // was reached about, so a decision about one path cannot authorise an
        // act on another.
        match (invocation.subject(), call) {
            (Subject::Path(target), Call::OnPath { tool, .. }) => Ok(match tool {
                ToolName::FsList => files::list(target.resolved()),
                _ => files::read(target.resolved()),
            }),
            (Subject::Path(target), Call::Write { contents, .. }) => {
                Ok(files::write(target.resolved(), contents))
            }
            (Subject::Path(target), Call::Edit { old, new, .. }) => {
                Ok(files::edit(target.resolved(), old, new))
            }
            (Subject::Search { root, needle }, Call::Search { .. }) => {
                Ok(files::search(root.resolved(), needle, self.search_ceiling))
            }
            (Subject::Command(line), Call::Run { .. }) => self.subprocess.run(line).await,
            (Subject::Url(url), Call::Fetch { .. }) => self.fetch.retrieve(url).await,
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

impl<C, F> ToolExecutor for Executor<'_, C, F>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
{
    fn descriptors(&self) -> &[ToolDescriptor] {
        // Owned by the caller and handed in would be a second list; built
        // here from `ToolName::ALL` would need somewhere to live across the
        // borrow. A `OnceLock` over a value derived from a compile-time
        // constant is neither: one walk, one allocation, and the same slice
        // every time.
        static DESCRIPTORS: std::sync::OnceLock<Vec<ToolDescriptor>> = std::sync::OnceLock::new();
        DESCRIPTORS.get_or_init(descriptors)
    }

    async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
        let call = match Self::call_for(request) {
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
        let invocation = match &call {
            Call::Fetch { url } => Invocation::fetching(url),
            Call::Run { command } => {
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
            Call::Search { root, needle } => {
                // The root is classified exactly as any other path is: D4
                // applies to where a search looks, and a search that started
                // outside the tree prompts and is marked like any other
                // out-of-tree call.
                classified = self.working_directory.classify(root);
                Invocation::searching(&classified, needle)
            }
            Call::OnPath { path, .. } | Call::Write { path, .. } | Call::Edit { path, .. } => {
                classified = self.working_directory.classify(path);
                Invocation::on_path(call.tool(), &classified).map_err(|refused| {
                    // A tool that is not described by a bare path given a path
                    // subject is the harness having built the wrong call,
                    // which is a defect rather than anything the user or the
                    // model did.
                    PortFailure::new(refused.to_string())
                })?
            }
        };

        let decision = Decision::assess(self.mode, &invocation, self.allowlist, self.destructive);
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

        match decision.permit(self.confirmer.map(|confirmer| confirmer as &dyn Confirm)) {
            Permission::Refused(because) => self.refuse(
                request,
                &entry,
                statement,
                RefusedBecause::to_string(&because),
            ),
            Permission::Granted => {
                // The record is written *before* the act, so a process killed
                // inside the act leaves a `Started` with nothing closing it —
                // which is what ADR-0010 D4's `Interrupted` is derived from.
                self.record(&Record::ToolCall(ToolCall::started(&entry)))?;
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

impl<C, F> Executor<'_, C, F> {
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
