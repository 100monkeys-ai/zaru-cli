// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0013] D2's generated summary, as a request to the provider that is
//! already configured.
//!
//! # This is the model call D2 has always described and nothing has made
//!
//! D2 replaces the oldest span of layer 6 "by a generated summary", and that
//! record's own Negative consequence names the cost in as many words:
//! "Summarisation costs a model call at the moment the session is already
//! under pressure". `zaru-core` declares
//! [`Summariser`] for it and implements it
//! nowhere, because a model call is [ADR-0012]'s. This module is the product
//! implementation, and it is the whole of what was missing.
//!
//! # It borrows a [`Model`], not a client, and that is what keeps it thin
//!
//! [`ModelSummariser`] is generic over `zaru-core`'s port rather than
//! concrete over [`GeminiClient`](crate::providers::gemini::GeminiClient), so
//! the composition hands it the [`Classifying`](crate::compose::Classifying)
//! adapter it already built for the turn. Four things follow, and none of
//! them is a new mechanism:
//!
//! - **The typed failure still lands where the surface reads it.** A
//!   summarisation that fails goes through `Classifying::respond`, which
//!   keeps the `GeminiFailure` for `Surface` to classify under [ADR-0016]
//!   D1 — so there is no second failure taxonomy and no second mapping.
//! - **There is exactly one place a request is sent**, which is the client's
//!   own `exchange`. A summariser holding its own client would be a second
//!   call site for the same thing.
//! - **Nothing in `providers/` changes**, and no port changes.
//! - A check can drive it against a stub `Model` with no socket, which is
//!   what [Testing] requires of everything on the runner.
//!
//! # What it sends, and why every part of it is somebody else's words
//!
//! One text: [`prose::SUMMARISE_SPAN`], then the span's exchanges oldest
//! first. The instruction is transcribed from ADR-0013's own sentences — see
//! that constant for which sentence came from where and which one was
//! authored — and the exchanges are the session's, unaltered.
//!
//! **No tools are offered.** `ModelRequest.tools` is empty, which the wire
//! layer turns into *no `tools` key at all* rather than an empty array,
//! because "an empty `tools` array is not the same request as no `tools` key
//! and the second is what 'this turn offers no tools' means". A summarisation
//! is not a turn and has nothing to call.
//!
//! **Nothing is truncated.** [Verification lessons] §65 — "redact before you
//! truncate, because an elision cuts a held value into pieces the masker no
//! longer recognises" — is satisfied here by there being no elision at all:
//! `Context::compact` takes the *shortest oldest prefix whose measured tokens
//! cover the overage*, and the overage is bounded by the threshold, so a span
//! is at most the difference between the threshold and the window. It fits by
//! construction, and that is stated rather than left to be rediscovered by
//! whoever changes the threshold.
//!
//! # The redaction, which is the seventh path
//!
//! [ADR-0008] trigger clause 6 puts one `Redactor` on "every path from
//! captured bytes into a model prompt or request", and **this is a request**:
//! a compacted span carries whatever layer 6 held, including [ADR-0013] D1's
//! "conversation **and tool results**", and it goes to a provider on its own.
//! It was named on that record before the row was added to
//! `no_captured_bytes_reach_a_prompt_except_through_the_port`'s list, which is
//! what the check's own message demands.
//!
//! The seam needs no widening: [`Prompt::new`] takes a
//! [`Redacted`] and `Redacted` has one
//! constructor, so a version of this module that forgot the port would not
//! compile.
//!
//! # It counts what it spent, because [ADR-0012] D7 says every request does
//!
//! D7: "Every request records prompt tokens, completion tokens… per session
//! on exit." A summarisation is a request, so [`ModelSummariser::spent`]
//! reports what the last one cost — the same interior-mutability shape
//! `GeminiClient::last` and `Classifying::taken` already use, and for the same
//! reason: the port's method takes `&self`.
//!
//! **What D7 asks for beyond that is not taken here.** The client's own
//! `usage()` reports the *last* exchange rather than the session's sum, so a
//! turn's line has never been a session total; making it one changes what an
//! already-landed line means and is raised on ADR-0012 as a proposed Update
//! rather than decided by this module.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-3
//! [`Prompt::new`]: zaru_core::iteration::Prompt::new
//! [`prose::SUMMARISE_SPAN`]: crate::compose::prose::SUMMARISE_SPAN

use crate::compose::prose;
use core::fmt;
use std::sync::Mutex;
use zaru_core::context::{Span, Summariser};
use zaru_core::iteration::{PortFailure, Prompt};
use zaru_core::redaction::{Redacted, Redactor};
use zaru_core::tool_call::{Model, ModelRequest, ModelResponse, TokenUsage};

/// [ADR-0013] D2's summariser, over whatever provider the turn is using.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
pub struct ModelSummariser<'a, M: Model + ?Sized> {
    model: &'a M,
    redactor: &'a (dyn Redactor + Sync),
    /// [ADR-0012] D7's number for the last summarisation. See the module
    /// documentation.
    last: Mutex<Option<TokenUsage>>,
    /// The most bytes of span one summarisation sends, where a window bounds
    /// it. See [`Self::within`].
    within: Option<u64>,
}

impl<M: Model + ?Sized> fmt::Debug for ModelSummariser<'_, M> {
    /// Names what it holds and renders none of it.
    ///
    /// The redactor holds the harness's own bearer values in memory — which
    /// is why [`HeldSecrets`](crate::redaction::HeldSecrets) writes its own
    /// `Debug` by hand — and a `Debug` is what ends up in a panic message.
    /// What is reported is the number, which is a number.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelSummariser")
            .field("spent", &self.spent())
            .finish_non_exhaustive()
    }
}

impl<'a, M: Model + ?Sized> ModelSummariser<'a, M> {
    /// Summarise through `model`, redacting with `redactor`.
    #[must_use]
    pub const fn over(model: &'a M, redactor: &'a (dyn Redactor + Sync)) -> Self {
        Self {
            model,
            redactor,
            last: Mutex::new(None),
            within: None,
        }
    }

    /// Send at most `bytes` bytes of span in one summarisation.
    ///
    /// # A span can be larger than the window it is summarised to relieve
    ///
    /// A turn that read one large result carries it in layer 6, and the span
    /// that covers it is at least that large. Sent whole, the summarisation
    /// would be refused before it left, exactly as the turn's own request was,
    /// and the session could never compact past it. So each exchange keeps
    /// its share of `bytes` — its opening and its end, which is where a task
    /// and an answer are — and the middle is left out with a line saying how
    /// much. The whole span is still in the transcript, which is what
    /// [ADR-0013] D2 keeps it for.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    #[must_use]
    pub const fn within(mut self, bytes: u64) -> Self {
        self.within = Some(bytes);
        self
    }

    /// What the last summarisation cost, if one has happened.
    ///
    /// `None` before the first, which is the same pairing
    /// [`Provider::usage`](crate::providers::Provider::usage) states: a
    /// client that had made no request and reported a zero would be inventing
    /// a datum.
    #[must_use]
    pub fn spent(&self) -> Option<TokenUsage> {
        match self.last.lock() {
            Ok(slot) => *slot,
            // A poisoned lock means a panic while two integers were being
            // written, which this module never does. Reading through the
            // poison keeps a defect report from losing the number.
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// The whole text one summarisation sends, before redaction.
    ///
    /// Separate from [`Summariser::summarise`] so that a check can read what
    /// would be sent without a provider, and so that the composition of the
    /// instruction and the span has one home.
    fn text_for(span: &Span, within: Option<u64>) -> String {
        let mut text = String::from(prose::SUMMARISE_SPAN);
        let share = within.map(|bytes| {
            let count = u64::try_from(span.len().max(1)).unwrap_or(u64::MAX);
            usize::try_from(bytes / count).unwrap_or(usize::MAX)
        });
        for exchange in span.exchanges() {
            text.push_str("\n\n");
            match share {
                Some(share) => text.push_str(&cut_to(exchange.as_str(), share)),
                None => text.push_str(exchange.as_str()),
            }
        }
        text
    }

    /// Record what a summarisation cost.
    fn spent_was(&self, tokens: TokenUsage) {
        match self.last.lock() {
            Ok(mut slot) => *slot = Some(tokens),
            Err(poisoned) => *poisoned.into_inner() = Some(tokens),
        }
    }
}

impl<M: Model + ?Sized + Sync> Summariser for ModelSummariser<'_, M> {
    /// Ask the model for [ADR-0013] D2's generated summary.
    ///
    /// # Errors
    ///
    /// [`PortFailure`] when the provider fails, when it stops without text,
    /// or when it asks for a tool it was not offered. `Context::compact`
    /// obtains the summary **before** it removes anything, so a failure here
    /// leaves the context exactly as it was and loses no history.
    ///
    /// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
    async fn summarise(&self, span: &Span) -> Result<String, PortFailure> {
        let prompt = Prompt::new(Redacted::by(
            self.redactor,
            &Self::text_for(span, self.within),
        ));
        let request = ModelRequest {
            prompt: &prompt,
            tools: &[],
            turn: &[],
        };
        match self.model.respond(&request).await? {
            ModelResponse::Text { text, tokens, .. } => {
                self.spent_was(tokens);
                Ok(text)
            }
            // Neither of the remaining arms is a summary, and neither is
            // silently turned into one. An empty string here would replace a
            // span of real conversation with nothing and announce that it had
            // summarised it, which is worse than the compaction not happening
            // — `Context::compact` leaves the context untouched on an error.
            ModelResponse::Calls { calls, tokens, .. } => {
                self.spent_was(tokens);
                Err(PortFailure::new(format!(
                    "the model asked for {} tool call(s) in answer to a summarisation, which \
                     offered it none. A summarisation asks for a generated summary and a tool \
                     call is not one",
                    calls.len()
                )))
            }
            ModelResponse::Stopped { reason, tokens } => {
                self.spent_was(tokens);
                Err(PortFailure::new(format!(
                    "the model stopped without summarising the span: {reason}"
                )))
            }
        }
    }
}

/// `text` in at most `share` bytes: its opening and its end, with a line
/// between them saying how many bytes were left out.
///
/// Cut on character boundaries, so the result is always valid text.
fn cut_to(text: &str, share: usize) -> String {
    if text.len() <= share {
        return text.to_owned();
    }
    let keep = share.saturating_sub(prose::SPAN_LEFT_OUT_ROOM) / 2;
    let mut head = keep.min(text.len());
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len().saturating_sub(keep);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    let left_out = tail.saturating_sub(head);
    format!(
        "{}\n{}\n{}",
        &text[..head],
        prose::span_left_out(left_out),
        &text[tail..]
    )
}
