// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The acting half of [ADR-0011]: what happens after the permission decision
//! says a call may act.
//!
//! # Two of the seven act, and five do not
//!
//! | Tool | Here |
//! | --- | --- |
//! | `fs.read` | `std::fs`, inside D4's boundary |
//! | `fs.list` | `std::fs`, inside D4's boundary |
//! | `fs.write`, `fs.edit` | [`FileWrites`], no implementation |
//! | `fs.search` | [`Search`], no implementation |
//! | `cmd.run` | [`Subprocess`], no implementation |
//! | `web.fetch` | [`Fetch`], no implementation |
//!
//! The two that act are the two that cannot create a path. That matters
//! because D4's classification resolves through a candidate's **longest
//! existing ancestor**, so a write is classified against a tree that does not
//! yet contain what it is about to make — and deciding what the boundary
//! means for a path that does not exist yet is a decision no record makes.
//! `fs.search` needs a matcher, which is either a dependency outside
//! [ADR-0003] D2's table or a glob semantics this crate would be inventing.
//! `cmd.run` needs a subprocess and `web.fetch` needs a socket, and the
//! harness has neither.
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
use crate::tools::decision::{Decision, Invocation, Permission, RefusedBecause, TranscriptEntry};
use crate::tools::mode::Mode;
use crate::tools::name::ToolName;
use crate::tools::output::{Captured, OutputBudget, Overflow, Presented};
use crate::tools::port::{Allowlist, Confirm, DestructiveMatch, Fetch, Subprocess};
use crate::tools::seal::{Verdict, Verdicts};
use crate::tools::tree::WorkingDirectory;
use crate::tools::writes::{FileWrites, Search};
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
        }
    }
}

impl std::error::Error for NotACall {}

/// Everything the acting half needs, and every one of it a caller's.
///
/// Bundled for the reason `zaru-core`'s port bundles are: a constructor
/// taking eleven arguments is a constructor whose order is a thing to get
/// wrong.
pub struct Executor<'a, W, S, C, F> {
    /// D4's boundary, canonical from construction.
    pub working_directory: &'a WorkingDirectory,
    /// D3's mode. Governs prompting and nothing else.
    pub mode: Mode,
    /// D3's allowlist. No product implementation.
    pub allowlist: &'a (dyn Allowlist + Sync),
    /// D6's four categories. No product implementation.
    pub destructive: &'a (dyn DestructiveMatch + Sync),
    /// D3's prompt. `None` refuses any call that needed one.
    pub confirmer: Option<&'a (dyn Confirm + Sync)>,
    /// ADR-0004's membrane. No product implementation.
    pub verdicts: &'a (dyn Verdicts + Sync),
    /// D5's budget, refused at zero by its own constructor.
    pub budget: OutputBudget,
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
    /// `fs.write` and `fs.edit`. No product implementation.
    pub writes: &'a W,
    /// `fs.search`. No product implementation.
    pub search: &'a S,
    /// `cmd.run`. No product implementation.
    pub subprocess: &'a C,
    /// `web.fetch`. No product implementation.
    pub fetch: &'a F,
}

impl<W, S, C, F> core::fmt::Debug for Executor<'_, W, S, C, F> {
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
            // ADR-0011 D1 names seven tools and no argument schema for any of
            // them. An empty schema says "this record does not specify one",
            // which is true; inventing one here would be authoring the tool
            // surface's wire contract. Raised as an open question on ADR-0011.
            parameters: String::new(),
        })
        .collect()
}

impl<W, S, C, F> Executor<'_, W, S, C, F>
where
    W: FileWrites + Sync,
    S: Search + Sync,
    C: Subprocess + Sync,
    F: Fetch + Sync,
{
    /// Turn a request into a call this surface can classify.
    ///
    /// # Errors
    ///
    /// [`NotACall`] when the name is not a built-in, or the arguments carry
    /// no target.
    fn call_for(request: &ToolRequest) -> Result<(ToolName, String), NotACall> {
        let tool = ToolName::ALL
            .into_iter()
            .find(|tool| tool.as_str() == request.name)
            .ok_or_else(|| NotACall::NoSuchTool {
                asked: request.name.escape_debug().to_string(),
            })?;
        let target = request.arguments.trim();
        if target.is_empty() {
            return Err(NotACall::NoTarget { tool });
        }
        Ok((tool, target.to_owned()))
    }

    /// Do the thing, having been permitted to.
    ///
    /// Five of the seven leave through a port with no product implementation.
    /// The two that stay are the two that cannot create a path.
    async fn act(&mut self, tool: ToolName, target: &str) -> Result<Captured, PortFailure> {
        match tool {
            ToolName::FsRead => {
                let resolved = self.working_directory.classify(target);
                read_file(resolved.resolved())
            }
            ToolName::FsList => {
                let resolved = self.working_directory.classify(target);
                list_directory(resolved.resolved())
            }
            ToolName::FsWrite | ToolName::FsEdit => self.writes.apply(tool, target).await,
            ToolName::FsSearch => self.search.find(target).await,
            ToolName::CmdRun => self.subprocess.run(target).await,
            ToolName::WebFetch => self.fetch.retrieve(target).await,
        }
    }
}

impl<W, S, C, F> ToolExecutor for Executor<'_, W, S, C, F>
where
    W: FileWrites + Sync,
    S: Search + Sync,
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
        let (tool, target) = match Self::call_for(request) {
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

        // The target is classified here, from the request, and the invocation
        // is built from that classification. A caller cannot supply either.
        let classified = self.working_directory.classify(&target);
        let invocation = if tool.addresses_a_path() {
            Invocation::on_path(tool, &classified).map_err(|refused| {
                // A URL-addressing tool given a path subject is the harness
                // having built the wrong call, which is a defect rather than
                // anything the user or the model did.
                PortFailure::new(refused.to_string())
            })?
        } else {
            Invocation::fetching(&target)
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
                let captured = self.act(tool, &target).await?;
                let presented = captured
                    .present(self.budget, Some(&mut *self.overflow))
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

impl<W, S, C, F> Executor<'_, W, S, C, F> {
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
/// # This is where ADR-0008 clause 6's port applies on the tool path
///
/// It replaces the identity seam the `tool-surface` arc left in
/// [`crate::tools::output`], and it sits **here** rather than there
/// deliberately. That seam was on the path from a capture to *its caller*,
/// which includes the human; D5 says both streams are surfaced, and redacting
/// what the user is shown of their own machine's output is not what clause 6
/// asks for. This function is the narrower thing: the point where a
/// [`Presented`] becomes the bytes a **model** reads. The `Presented` itself,
/// the `Captured` behind it, the transcript and the preserved overflow file
/// all keep raw bytes.
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

/// Read a file with `std::fs`, inside the boundary.
///
/// Shaped as a [`Captured`] so that one presentation path serves every tool:
/// a read that succeeded is exit code 0 with the contents on standard
/// output, and one that failed is a non-zero code with the operating
/// system's own words on standard error. The alternative — a second
/// presentation for filesystem tools — would be D5's truncation rule written
/// twice.
fn read_file(path: &std::path::Path) -> Result<Captured, PortFailure> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Captured {
            exit_code: 0,
            stdout: String::from_utf8_lossy(&bytes).into_owned(),
            stderr: String::new(),
        }),
        Err(source) => Ok(Captured {
            exit_code: 1,
            stdout: String::new(),
            stderr: format!("could not read {}: {source}", path.display()),
        }),
    }
}

/// List a directory with `std::fs`, inside the boundary.
///
/// Entries are sorted, because a directory's own order is a property of the
/// filesystem rather than of the directory, and a listing that changes order
/// between two identical calls is a listing a model cannot reason about.
fn list_directory(path: &std::path::Path) -> Result<Captured, PortFailure> {
    let reading = match std::fs::read_dir(path) {
        Ok(reading) => reading,
        Err(source) => {
            return Ok(Captured {
                exit_code: 1,
                stdout: String::new(),
                stderr: format!("could not list {}: {source}", path.display()),
            });
        }
    };
    let mut names: Vec<String> = Vec::new();
    for entry in reading {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(source) => {
                return Ok(Captured {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: format!("could not list {}: {source}", path.display()),
                });
            }
        }
    }
    names.sort();
    Ok(Captured {
        exit_code: 0,
        stdout: names.join("\n"),
        stderr: String::new(),
    })
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
