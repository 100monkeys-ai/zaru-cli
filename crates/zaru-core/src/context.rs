// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Context assembly and compaction: ADR-0013.
//!
//! [Bounded Contexts] gives this crate "context assembly", and this module is
//! it. It decides what the model actually sees, in what order, and what goes
//! when there is no longer room for all of it.
//!
//! # A turn is not an iteration
//!
//! ADR-0013 D3's announcement counts "34 earlier turns" and D7 confines
//! compaction to "turn boundaries only". **A turn is one conversational
//! exchange — the user says something and Zaru answers.** An iteration is the
//! inner cycle of [`crate::iteration`]: generate, execute, evaluate, refine.
//! One turn contains zero or many iterations.
//!
//! [Ubiquitous Language] names *turn* as an anti-term **for an iteration**,
//! and that rule is not relaxed here: nothing in this module calls an
//! iteration a turn. The unit of layer 6 is an [`Exchange`], and the one
//! place D3's word survives is the field name on
//! [`Announcement::Compacted`], because that is the word D3's own rendered
//! line uses and a consumer rendering it should not have to translate.
//!
//! # Why compaction cannot happen during an iteration
//!
//! ADR-0013 D7: "A compaction between generate and evaluate changes the
//! model's view of what it was doing partway through a cycle. Compaction
//! happens at turn boundaries only."
//!
//! The loop calls [`crate::iteration::ContextPolicy::assemble`] at every
//! *iteration* boundary, which is inside a turn. So a policy that compacted
//! whenever it felt pressure would break D7 without ever running between
//! generate and evaluate. What prevents it here is not a rule anybody has to
//! remember:
//!
//! - [`Context::assemble`] takes `&self`. It renders and measures, and there
//!   is no compaction reachable from it.
//! - [`Context::compact`] takes `&mut self`. It is the only thing that
//!   changes what the context holds, and the turn's owner calls it.
//!
//! A `ContextPolicy` implementation holding a `Context` behind the shared
//! borrow `assemble` gives it therefore *cannot* compact — the method is not
//! callable from there. Under pressure it refuses instead, with
//! [`Exceeded`], which the loop reports as ADR-0013 D7's second route to
//! exhaustion rather than as an error.
//!
//! # What this module owns no numbers for
//!
//! The window and the pressure threshold arrive as parameters, because
//! ADR-0013's own Neutral consequence says "Nothing here sets a threshold. It
//! is provider-dependent configuration." Token counts arrive through
//! [`TokenCounter`], because no tokeniser is in ADR-0003 D2's table and a
//! count invented by the thing being counted is not a measurement. Summaries
//! arrive through [`Summariser`], because ADR-0013's own Consequences say
//! summarisation costs a model call and ADR-0012 owns providers. **Nothing in
//! this crate's product tree implements either port.**
//!
//! # What is serialisable here, and what is deliberately only half so
//!
//! [ADR-0010] D2 makes the transcript the event stream and D3 makes
//! `context.json` a checkpoint that is rewritten each turn, so the
//! announcements and the raw spans this module produces will be written by
//! `zaru-cli`. The derives are on the declarations rather than on a mirror in
//! that crate, for the reason [`crate::iteration::event`] gives about
//! ADR-0008 D3's stream: a rule that exists in two places diverges.
//!
//! Three types here guard an invariant their constructor refuses to build
//! without — an identity with no workspace resolves nowhere, and an
//! attachment with no re-attachment instruction cannot satisfy D4's
//! announcement — so a *derived* `Deserialize` on any of them would put a
//! value that never passed those guards straight back into the program from
//! disk. [Operating Principles] calls anything read off disk a boundary, and
//! when this module was written it recorded **how a guarded type is
//! rehydrated through its own constructor** as ADR-0010's question rather
//! than a derive's.
//!
//! **That question is answered as of 2026-09-05, and the answer is: through
//! the constructor.** [`ItemId`] implements `Deserialize` by hand — it reads
//! the two fields and then calls [`ItemId::new`], so a stored identity with an
//! empty workspace is a deserialisation *error* naming which requirement
//! failed rather than a value nobody could have built. [`Announcement`] then
//! derives `Deserialize` safely, because the only guarded thing it carries is
//! that identity and the guard now travels with it.
//!
//! The reason it had to be answered here is that ADR-0013 D2's raw span and
//! D3's announcement now reach ADR-0010 D2's transcript, which is a file
//! something reads back: `zaru-cli`'s resume path parses every line and
//! treats one it cannot parse as a defect. A record that could be written and
//! not read would make that path report a defect in the harness for a line
//! the harness itself wrote. Decided under directive 20 of 2026-09-05 by the
//! `context-summariser` arc, open to Jeshua's veto, and recorded on ADR-0010.
//!
//! **[`AttachedItem`] still derives `Serialize` alone**, and that is the
//! unanswered half rather than an oversight: layer 5 has no producer, so
//! nothing writes one and nothing could read one back. It gains the same
//! treatment on the day something attaches.
//!
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts
//! [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod announcement;
pub mod assembly;
pub mod exchange;
pub mod history;
pub mod item;
pub mod layer;
pub mod limits;
pub mod port;
pub mod prefix;
pub mod usage;

pub use announcement::Announcement;
pub use assembly::{Assembled, Compaction, Context, Exceeded};
pub use exchange::{Exchange, ExchangeKind};
pub use history::IterationRecord;
pub use item::{AttachedItem, ItemId, ItemRefused};
pub use layer::{Layer, Retention};
pub use limits::{ContextLimits, ContextWindow, LimitsRefused, PressureThreshold};
pub use port::{Span, Summariser, TokenCounter};
pub use prefix::{PrefixParts, StablePrefix};
pub use usage::Usage;

#[cfg(test)]
mod fixtures;
#[cfg(test)]
mod tests;
