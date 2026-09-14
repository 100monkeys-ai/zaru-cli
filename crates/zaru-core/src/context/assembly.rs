// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The context itself, and the two things that can be done to it.
//!
//! Both operations live in one file on purpose. They are the two halves of
//! ADR-0013 D7 and the difference between them is one word in each signature:
//!
//! - [`Context::assemble`] takes `&self`. The loop calls it at every
//!   iteration boundary. It renders, it measures, and under pressure it
//!   **refuses** — there is no compaction reachable from a shared borrow.
//! - [`Context::compact`] takes `&mut self`. The turn's owner calls it at a
//!   turn boundary, which is the only place D7 allows compaction to happen.
//!
//! Splitting them across two files would hide the one property that makes D7
//! structural rather than remembered.
//!
//! # What compaction does, in D1's precedence order
//!
//! Layer 6 first, then layer 5, and layer 7 never — see
//! [`crate::context::history`] for why layer 7 is already compacted in every
//! rendering and has nothing left for pressure to take. Within layer 6 the
//! span is the **shortest oldest prefix whose measured tokens cover the
//! overage**: oldest first, as D2 requires, and no larger than the pressure
//! actually calls for. That rule invents no number — the overage is derived
//! from the threshold the caller already passed. It is a reading of D2 rather
//! than something D2 says, ruled 2026-09-04 as a delegated coordinator
//! reading and recorded on ADR-0013.
//!
//! Attachments go last and one at a time, re-measuring after each, because
//! D1 puts layer 5 last and D4 says the user chose to spend that context.
//!
//! # Redaction happens here, at the boundary, and not per layer
//!
//! [ADR-0008]'s trigger clause 6 was decided on 2026-09-05 and names ADR-0013
//! D5's layer 7 as one of the paths from captured bytes into a model prompt.
//! The port is applied to the **whole assembled render** rather than to layer
//! 7 alone, which was a coordinator ruling of 2026-09-05 recorded on
//! ADR-0008.
//!
//! Two reasons, and the second is the one that matters. Layer 7 is not the
//! only layer that carries captured bytes: D1's layer 6 is "conversation
//! **and tool results**", so the moment anything writes a tool result into an
//! [`Exchange`] there is a second place with the same obligation. Redacting
//! per layer would put one rule in two places, which is the shape the
//! duplicate `Layer` enum ruling removed from this workspace. And the render
//! is where every layer meets, so one call there cannot be forgotten by
//! whoever adds the next producer.
//!
//! The measurement passes it too. [`Context::usage`] and [`Context::compact`]
//! count the redacted text, because a marker is not the same length as the
//! value it replaced and a count of bytes that will not be sent is not a
//! measurement of anything.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop

use crate::context::announcement::Announcement;
use crate::context::exchange::Exchange;
use crate::context::history::{self, IterationRecord};
use crate::context::item::AttachedItem;
use crate::context::layer::Layer;
use crate::context::limits::ContextLimits;
use crate::context::port::{Span, Summariser, TokenCounter};
use crate::context::prefix::{SEPARATOR, StablePrefix};
use crate::context::usage::Usage;
use crate::iteration::port::{ContextRefusal, PortFailure};
use crate::redaction::{Redacted, Redactor};
use core::fmt;
use serde::{Deserialize, Serialize};

/// What the model will see, and what it costs.
///
/// The text is [`Redacted`], so a [`Prompt`](crate::iteration::Prompt) built
/// from one has passed ADR-0008 clause 6's port by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    text: Redacted,
    usage: Usage,
}

impl Assembled {
    /// The whole assembled context.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    /// The whole assembled context, as the value a prompt is built from.
    ///
    /// This is the door between ADR-0013's assembly and ADR-0008's
    /// `Prompt`: a context policy hands this straight to `Prompt::new`, and
    /// there is no other way to make one.
    #[must_use]
    pub fn into_redacted(self) -> Redacted {
        self.text
    }

    /// What it costs against the window. ADR-0013 D6's number.
    #[must_use]
    pub const fn usage(&self) -> Usage {
        self.usage
    }
}

/// Assembling this iteration would not fit the window.
///
/// ADR-0013 D7: the iteration "fails as exhausted with a clear reason rather
/// than continuing on a rewritten context". Not an error — ADR-0008 D5 puts
/// exhaustion in its own register — which is why this converts into
/// [`ContextRefusal::WindowExceeded`] and not into a [`PortFailure`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exceeded {
    /// Tokens the assembled context needs.
    pub needed: u64,
    /// Tokens the window allows.
    pub window: u64,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the assembled context needs {} tokens and the window allows {}",
            self.needed, self.window
        )
    }
}

impl std::error::Error for Exceeded {}

impl From<Exceeded> for ContextRefusal {
    fn from(exceeded: Exceeded) -> Self {
        Self::WindowExceeded {
            needed: exceeded.needed,
            window: exceeded.window,
        }
    }
}

/// What one compaction did.
///
/// # Both directions, because ADR-0010 D2's transcript is read back
///
/// This is the whole of what a compaction produced, and D2 makes the
/// transcript a **replayable** record — so the line that records one has to
/// be readable by the same program that wrote it. Both halves round-trip
/// already: [`Span`] holds [`Exchange`]es, which carry no invariant, and
/// [`Announcement`]'s only guarded part is an
/// [`ItemId`](crate::context::ItemId) whose `Deserialize` goes through its
/// own constructor. See [`crate::context`] for that decision.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    /// What the user is told, in the order it happened. Empty when the
    /// context was already under the threshold and nothing was taken.
    pub announcements: Vec<Announcement>,
    /// The span of layer 6 that was replaced, for ADR-0010's transcript to
    /// keep. `None` when layer 6 was not compacted.
    pub raw: Option<Span>,
}

/// Everything the model is shown, across one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    prefix: StablePrefix,
    attachments: Vec<AttachedItem>,
    exchanges: Vec<Exchange>,
    iterations: Vec<IterationRecord>,
    limits: ContextLimits,
    reserved: u64,
}

impl Context {
    /// Open a session's context around a prefix that is now fixed.
    ///
    /// `reserved` is what **every request spends that this context does not
    /// contain** — see [`Self::reserved`]. Pass zero for a caller that sends
    /// the context and nothing else.
    #[must_use]
    pub const fn opened(prefix: StablePrefix, limits: ContextLimits, reserved: u64) -> Self {
        Self {
            prefix,
            attachments: Vec::new(),
            exchanges: Vec::new(),
            iterations: Vec::new(),
            limits,
            reserved,
        }
    }

    /// What every request spends that this context does not contain.
    ///
    /// # Why a window is compared against more than the context
    ///
    /// ADR-0013 measures a context against a provider's window, and what the
    /// provider measures against that window is **the request** — which for
    /// every provider this workspace speaks to carries a tool surface beside
    /// the prompt. The context does not contain it and never will: the tools
    /// are the loop's, declared per exchange, and putting them into layer 6
    /// would put them into the conversation the model is shown twice.
    ///
    /// **Measured, 2026-09-14, from the release binary against a local
    /// Ollama through a logging proxy:** the first exchange of a session sent
    /// **231 bytes** of message content and **1,760 bytes** of tool schema,
    /// and the provider reported **465** prompt tokens. So a count over the
    /// message content alone is *below* the provider's own count — which is
    /// the direction that overflows a window silently, and the opposite of
    /// what [`TokenCounter`]'s only implementation in this workspace claims
    /// for itself.
    ///
    /// This number closes that gap. It is added to what
    /// [`Self::usage`] reports, to what [`Self::assemble`] refuses on, and to
    /// what [`Self::compact`] compares against the threshold — and it is
    /// deliberately **not** added when a single exchange is measured, because
    /// those measurements answer a different question:
    ///
    /// - [`Self::oldest_span_covering`] asks how many of the oldest exchanges
    ///   it takes to cover an overage. A fixed addend on each exchange would
    ///   make every exchange look larger than it is and take too few.
    /// - ADR-0013 D3's announcement carries the before-and-after counts of
    ///   the span that was replaced. A fixed addend there would report a
    ///   compaction that saved bytes it never held.
    #[must_use]
    pub const fn reserved(&self) -> u64 {
        self.reserved
    }

    /// The stable prefix. Borrowed, never handed over.
    #[must_use]
    pub const fn prefix(&self) -> &StablePrefix {
        &self.prefix
    }

    /// What the user has attached, in the order they attached it.
    #[must_use]
    pub fn attachments(&self) -> &[AttachedItem] {
        &self.attachments
    }

    /// Layer 6, oldest first.
    #[must_use]
    pub fn exchanges(&self) -> &[Exchange] {
        &self.exchanges
    }

    /// Layer 7, oldest first.
    #[must_use]
    pub fn iterations(&self) -> &[IterationRecord] {
        &self.iterations
    }

    /// Attach an item the user chose. A turn-boundary act.
    pub fn attach(&mut self, item: AttachedItem) {
        self.attachments.push(item);
    }

    /// Record what was said. A turn-boundary act.
    pub fn record_exchange(&mut self, exchange: Exchange) {
        self.exchanges.push(exchange);
    }

    /// Record an iteration the loop ran. A turn-boundary act.
    pub fn record_iteration(&mut self, record: IterationRecord) {
        self.iterations.push(record);
    }

    /// What the context costs right now, with no turn in progress.
    ///
    /// ADR-0013 D6's continuous number. Takes `&self`: reading the meter
    /// never changes what it measures.
    #[must_use]
    pub fn usage<C: TokenCounter, R: Redactor + ?Sized>(&self, counter: &C, redactor: &R) -> Usage {
        let text = Redacted::by(redactor, &self.render(""));
        Usage::new(
            counter.count(text.as_str()).saturating_add(self.reserved),
            self.limits.window().get(),
        )
    }

    /// Assemble what the model sees for the iteration about to begin.
    ///
    /// `tail` is this turn's own prompt — the task, or the refinement the
    /// loop constructed. It goes last, after every layer.
    ///
    /// **This cannot compact.** It takes `&self`, and ADR-0013 D7 confines
    /// compaction to turn boundaries.
    ///
    /// # Errors
    ///
    /// [`Exceeded`] when the assembled context does not fit the window.
    pub fn assemble<C: TokenCounter, R: Redactor + ?Sized>(
        &self,
        counter: &C,
        redactor: &R,
        tail: &str,
    ) -> Result<Assembled, Exceeded> {
        let text = Redacted::by(redactor, &self.render(tail));
        let needed = counter.count(text.as_str()).saturating_add(self.reserved);
        let window = self.limits.window().get();
        if needed > window {
            return Err(Exceeded { needed, window });
        }
        Ok(Assembled {
            text,
            usage: Usage::new(needed, window),
        })
    }

    /// Relieve window pressure, at a turn boundary.
    ///
    /// Does nothing at all when usage is at or below the threshold: ADR-0013
    /// D2 compacts "when the window pressure threshold is crossed", and a
    /// compaction nobody needed still costs a model call and still announces
    /// itself.
    ///
    /// # Errors
    ///
    /// [`PortFailure`] when the summariser fails. The context is left
    /// unchanged: the span is only removed once a summary exists to put in
    /// its place, so a failed summarisation cannot lose history.
    pub async fn compact<S: Summariser, C: TokenCounter, R: Redactor + ?Sized>(
        &mut self,
        summariser: &S,
        counter: &C,
        redactor: &R,
    ) -> Result<Compaction, PortFailure> {
        let threshold = self.limits.threshold().get();
        let mut used = self.measured(counter, redactor);
        if used <= threshold {
            return Ok(Compaction::default());
        }

        let mut compaction = Compaction::default();

        // --- Layer 6: summarise and replace, oldest first ----------------
        // Saturating, not a plain subtraction. The early return above is the
        // only thing that makes `used > threshold` true here, and a rule that
        // holds because of one branch somewhere else is a rule that holds by
        // circumstance. A mutation of that branch turned this into an
        // arithmetic overflow panic rather than into the failing assertion
        // the check was watching for, which is how it was found.
        let overage = used.saturating_sub(threshold);
        let taken = self.oldest_span_covering(counter, overage);
        if taken > 0 {
            let span = Span::new(self.exchanges[..taken].to_vec());
            let before: u64 = span
                .exchanges()
                .iter()
                .map(|exchange| counter.count(exchange.as_str()))
                .sum();
            // The summary is obtained before anything is removed, so a
            // failing summariser leaves the context exactly as it was.
            let summary = summariser.summarise(&span).await?;
            let after = counter.count(&summary);
            self.exchanges.drain(..taken);
            self.exchanges.insert(0, Exchange::summary(summary));
            compaction.announcements.push(Announcement::Compacted {
                turns: u32::try_from(taken).unwrap_or(u32::MAX),
                before,
                after,
            });
            compaction.raw = Some(span);
            used = self.measured(counter, redactor);
        }

        // --- Layer 5: discarded last, and never silently -----------------
        while used > threshold && !self.attachments.is_empty() {
            let dropped = self.attachments.remove(0);
            compaction
                .announcements
                .push(Announcement::AttachmentDropped {
                    identity: dropped.id().clone(),
                    how_to_reattach: dropped.reattach().to_owned(),
                });
            used = self.measured(counter, redactor);
        }

        Ok(compaction)
    }

    /// What the context costs, measured on the bytes that would be sent.
    ///
    /// The redaction is part of the measurement rather than applied after it:
    /// a marker is not the same length as the value it replaced, so counting
    /// the raw render would be counting text nobody will ever be shown.
    fn measured<C: TokenCounter, R: Redactor + ?Sized>(&self, counter: &C, redactor: &R) -> u64 {
        counter
            .count(Redacted::by(redactor, &self.render("")).as_str())
            .saturating_add(self.reserved)
    }

    /// How many of the oldest exchanges it takes to cover `overage`.
    ///
    /// The shortest such prefix, and the whole of layer 6 when even that is
    /// not enough. Zero when layer 6 is empty.
    fn oldest_span_covering<C: TokenCounter>(&self, counter: &C, overage: u64) -> usize {
        let mut covered: u64 = 0;
        for (index, exchange) in self.exchanges.iter().enumerate() {
            covered = covered.saturating_add(counter.count(exchange.as_str()));
            if covered >= overage {
                return index + 1;
            }
        }
        self.exchanges.len()
    }

    /// The whole context as the model would read it, `tail` last.
    ///
    /// The prefix comes first and every other layer follows in
    /// [`Layer::ALL`] order, so the ordering has one home. The prefix leading
    /// is what prompt caching needs — a cache matches a prefix, so a stable
    /// region anywhere but the front buys nothing.
    fn render(&self, tail: &str) -> String {
        let mut out = String::from(self.prefix.as_str());
        for layer in Layer::ALL {
            let Some(section) = self.section(layer) else {
                continue;
            };
            push_section(&mut out, &section);
        }
        push_section(&mut out, tail);
        out
    }

    /// One discardable layer's rendered text, or `None` when the layer is in
    /// the prefix or has nothing in it.
    ///
    /// Exhaustive over [`Layer`] rather than falling through on a wildcard,
    /// so an eighth layer fails to compile here and has to be placed.
    fn section(&self, layer: Layer) -> Option<String> {
        let text = match layer {
            Layer::SystemPromptAndPersona
            | Layer::Grounding
            | Layer::RelationshipMemory
            | Layer::ProjectManifestSummary => return None,
            Layer::UserAttachments => self
                .attachments
                .iter()
                .map(AttachedItem::body)
                .collect::<Vec<_>>()
                .join(SEPARATOR),
            Layer::ConversationAndToolResults => self
                .exchanges
                .iter()
                .map(Exchange::as_str)
                .collect::<Vec<_>>()
                .join(SEPARATOR),
            Layer::IterationHistory => history::render(&self.iterations),
        };
        (!text.is_empty()).then_some(text)
    }
}

/// Append a section, separated from whatever is already there.
fn push_section(out: &mut String, section: &str) {
    if section.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str(SEPARATOR);
    }
    out.push_str(section);
}
