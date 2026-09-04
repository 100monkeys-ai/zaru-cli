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
use core::fmt;

/// What the model will see, and what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    text: String,
    usage: Usage,
}

impl Assembled {
    /// The whole assembled context.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
}

impl Context {
    /// Open a session's context around a prefix that is now fixed.
    #[must_use]
    pub const fn opened(prefix: StablePrefix, limits: ContextLimits) -> Self {
        Self {
            prefix,
            attachments: Vec::new(),
            exchanges: Vec::new(),
            iterations: Vec::new(),
            limits,
        }
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
    pub fn usage<C: TokenCounter>(&self, counter: &C) -> Usage {
        Usage::new(counter.count(&self.render("")), self.limits.window().get())
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
    pub fn assemble<C: TokenCounter>(
        &self,
        counter: &C,
        tail: &str,
    ) -> Result<Assembled, Exceeded> {
        let text = self.render(tail);
        let needed = counter.count(&text);
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
    pub async fn compact<S: Summariser, C: TokenCounter>(
        &mut self,
        summariser: &S,
        counter: &C,
    ) -> Result<Compaction, PortFailure> {
        let threshold = self.limits.threshold().get();
        let mut used = counter.count(&self.render(""));
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
            used = counter.count(&self.render(""));
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
            used = counter.count(&self.render(""));
        }

        Ok(compaction)
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
