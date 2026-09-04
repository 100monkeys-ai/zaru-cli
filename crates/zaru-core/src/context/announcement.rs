// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0013 D3 and D4, as data.
//!
//! ```text
//! ◈ compacted 34 earlier turns · 18.2k → 2.1k tokens · full history in transcript
//! ◈ dropped attachment: adrs/0117-aegis-edge-mode · re-attach with [[
//! ```
//!
//! **Those two lines are not produced here.** ADR-0008 D2 makes this crate
//! headless: it emits what a consumer needs to display and `zaru-tui`
//! decides how it looks. So this type carries two numbers and a count, or an
//! identity and an instruction, and it deliberately has no `Display`
//! implementation — the glyph, the separators and the abbreviated thousands
//! are a renderer's, and a headless crate that formatted them would be
//! rendering with extra steps.
//!
//! # Both are caused output
//!
//! ADR-0002 D1 permits output the user caused, and D8 counts a line that
//! rides on a turn the user started as caused. An announcement here can only
//! come from [`Context::compact`], which the turn's owner calls at a turn
//! boundary — so there is no path by which one is produced on a timer, which
//! is the shape D8 forbids.
//!
//! # Once, and once only
//!
//! D3: "Compaction is announced, **once**, with what it cost." An
//! announcement is a value returned by the operation that caused it, not a
//! flag on the context that a later render could read again — so a second
//! showing would need a second compaction.
//!
//! [`Context::compact`]: crate::context::Context::compact

use crate::context::item::ItemId;
use serde::Serialize;

/// Something the user is told, because something was taken away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Announcement {
    /// ADR-0013 D3. A span of layer 6 was replaced by a summary.
    Compacted {
        /// How many exchanges were replaced. D3's rendered line calls these
        /// turns and so does this field; the type is
        /// [`Exchange`](crate::context::Exchange), because *turn* is
        /// [Ubiquitous Language]'s anti-term for an iteration.
        ///
        /// [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language
        turns: u32,
        /// What the replaced span cost, in tokens.
        before: u64,
        /// What the summary that replaced it costs, in tokens.
        after: u64,
    },
    /// ADR-0013 D4. A user attachment was dropped.
    ///
    /// "The user chose to spend that context. Removing their choice without
    /// telling them is worse than running out."
    AttachmentDropped {
        /// Which attachment. Carries its workspace as well as its path,
        /// because a path alone resolves nowhere.
        identity: ItemId,
        /// How to get it back, in the words whoever attached it used.
        how_to_reattach: String,
    },
}
