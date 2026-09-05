// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one port every path from captured bytes into a model prompt passes
//! through, and the type that proves it was passed.
//!
//! # What was decided, and by whom
//!
//! [ADR-0008]'s trigger clause 6 — "a decision exists for secret redaction in
//! failure text" — was open from the day the loop landed and blocked that
//! record's acceptance. Jeshua decided it on 2026-09-05: **one [`Redactor`]
//! port in this crate, applied on every path from captured bytes into a model
//! prompt, with exactly one product implementation that redacts values the
//! harness itself holds and nothing pattern-based.** That product
//! implementation is `zaru_cli::redaction::HeldSecrets`, over [ADR-0007]'s
//! credential store. **Nothing in this crate's product tree implements this
//! trait**, exactly as nothing here implements the loop's other ports.
//!
//! # Unknown secrets in command output are out of scope, and that is the decision
//!
//! The harness redacts what it **holds** — a bearer value it put in the store
//! itself — and nothing else. It does not look for things that appear
//! secret-shaped. No regular expression, no entropy heuristic, no
//! `nn_mcp_`-prefix matcher.
//!
//! The reason is [ADR-0011] D6's, arriving somewhere worse. That record
//! refuses to author a destructive-command pattern list because "a prompt
//! that cries wolf gets dismissed reflexively"; a redaction matcher fails the
//! same way in both directions at once — a false positive silently corrupts
//! the model's input, and a false negative is invisible, because nothing
//! about a prompt says a secret went through it. A matcher also cannot be
//! wrong *safely*: the harness would be claiming a protection it cannot
//! deliver, which is the dishonesty [ADR-0011] D2 exists to refuse. What the
//! harness holds it can redact exactly; what it does not hold it says it does
//! not cover.
//!
//! # The bypass is prevented by an absent constructor, not by a rule
//!
//! [`Redacted`] has one constructor, [`Redacted::by`], and it takes a
//! [`Redactor`]. There is no `new`, no `From<String>`, no `Default` and no
//! `Deserialize`. Every type that reaches a provider is built from one:
//! [`Prompt`](crate::iteration::Prompt) and
//! [`ToolResult::content`](crate::tool_call::ToolResult). So a path that
//! forgot the port does not compile, rather than passing a check somebody
//! remembered to write.
//!
//! It is the shape this workspace already uses for a value refused at a
//! boundary and carried afterwards as evidence the boundary was crossed —
//! [`ToolCalling`](crate::tool_call::ToolCalling),
//! [`Ceiling`](crate::iteration::Ceiling),
//! [`TruncationBudget`](crate::iteration::TruncationBudget).
//!
//! # Which paths, and what enumerates them
//!
//! **`zaru-cli`'s `no_captured_bytes_reach_a_prompt_except_through_the_port`
//! is the authority on the list, and this table is not.** It walks the
//! product sources and asserts the set of files calling [`Redacted::by`]
//! against a list it holds, so a path added or removed reddens there; a
//! number written *here* would go stale the first time somebody added one,
//! which is the failure ADR-0008 clause 6 exists to prevent. The table below
//! is a reader's orientation and is dated for that reason.
//!
//! Seven files as of 2026-09-05:
//!
//! | Path | Where the port is called |
//! | --- | --- |
//! | The refinement prompt ([ADR-0008] D4) | [`crate::iteration::refinement`] |
//! | The assembled context, layers 6 and 7 ([ADR-0013] D5 and D1) | [`crate::context::assembly`] |
//! | A refusal's sentence becoming the next turn's content ([ADR-0011] D6) | [`crate::tool_call::port`] |
//! | A tool's two captured streams, before truncation ([ADR-0011] D5) | `zaru_cli::tools::output` |
//! | The assembled tool result a `ToolResult` is built from | `zaru_cli::tools::execute` |
//! | A resumed session's interrupted call ([ADR-0010] D4) | `zaru_cli::session::resume` |
//! | The span a compaction sends as its own request ([ADR-0013] D2) | `zaru_cli::compose::summarise` |
//!
//! The decision named three. The next were found by reading the code — a
//! resumed interruption's datum is a rendered transcript line, and once
//! `cmd.run` exists that line is a command line, which is where a `--token=`
//! argument lives. **The last was not findable by reading at all**, because
//! the thing at the end of it did not exist: a compaction sends a span of
//! layer 6 to a model as a request of its own, so everything layer 6 holds
//! becomes prompt text a second time. It reddened this check on its first
//! compile and was named on ADR-0008 before its row was added.
//!
//! # What is deliberately not redacted
//!
//! **The record.** [ADR-0010]'s own Negative section says the transcript
//! "contain\[s\] whatever the session contained, including secrets that
//! appeared in command output. Filesystem permissions are the only
//! protection, and that is worth saying out loud rather than implying
//! encryption that does not exist." So the transcript, the checkpoint and
//! [ADR-0011] D5's preserved overflow file carry raw bytes, and the checks
//! assert a planted value is **present** there. Redaction is on prompts and
//! not on the record, and that difference is checked rather than assumed.
//!
//! **The event streams**, for the same reason: they are what the transcript
//! writer consumes. The two exceptions are the two events whose own contract
//! is "what the model was given" —
//! [`Event::RefinementConstructed`](crate::iteration::Event::RefinementConstructed)'s
//! excerpt and
//! [`Event::ToolCompleted`](crate::tool_call::Event::ToolCompleted)'s byte
//! count — and each says so where it is declared.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management

use std::borrow::Cow;

/// Removes from text the secrets the harness itself holds.
///
/// One method, over `&str`, and no configuration. A port that took a policy
/// would be the decision settled at a call site instead of on the record —
/// the failure the identity seams this replaces were careful not to commit,
/// and [Agent Lessons] §5.
///
/// [Agent Lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/agent-lessons
pub trait Redactor {
    /// `text` with every held secret replaced by a marker.
    ///
    /// Returns [`Cow::Borrowed`] when nothing matched. That is not an
    /// optimisation: it lets a caller distinguish "nothing was redacted" from
    /// "something was" without comparing strings, and it means the ordinary
    /// case — a prompt with no secret in it — allocates nothing.
    ///
    /// An implementation **must not** put a held value into the replacement.
    /// `zaru-cli`'s marker names the alias and never the value, and a check
    /// asserts both the marker's presence and the value's absence, because an
    /// implementation that erased the whole string would satisfy an absence
    /// assertion on its own.
    fn redact<'a>(&self, text: &'a str) -> Cow<'a, str>;
}

/// Text that has passed through a [`Redactor`].
///
/// **The only constructor is [`Redacted::by`].** See the module
/// documentation for why that absence is the mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redacted(String);

impl Redacted {
    /// Redact `text` with `redactor`.
    ///
    /// Idempotent for any implementation that honours the trait's contract:
    /// a marker carries no held value, so a second pass has nothing left to
    /// find. Two paths in this crate rely on that — they redact their parts
    /// before truncating, so that a secret cannot survive as a fragment
    /// across an elision, and then redact the assembled whole so that the
    /// value this type carries was produced by the port rather than
    /// assembled around it.
    #[must_use]
    pub fn by<R: Redactor + ?Sized>(redactor: &R, text: &str) -> Self {
        Self(redactor.redact(text).into_owned())
    }

    /// The redacted text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// How many bytes the redacted text occupies.
    ///
    /// The bytes that will actually be sent, which is what a token count and
    /// an event's byte count are both about.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the redacted text is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
