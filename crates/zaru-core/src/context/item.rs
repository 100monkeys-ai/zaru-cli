// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Layer 5: what the user attached, and the identity a drop announcement can
//! name it by.
//!
//! # Why this crate has its own identity rather than `zaru-notes`'s
//!
//! ADR-0006 D6's [`Attachment`] carries the workspace slug, the path, the
//! permalink and the `nn://` URI, and it is the right type for an attachment
//! that has to locate itself against a live substrate. It lives in
//! `zaru-notes`, and ADR-0003 D8 gives `zaru-core` no sibling dependencies at
//! all — `scripts/check-crate-boundaries.py` fails on any edge, and adding
//! one would also make this crate a shared-types crate, which ADR-0016's
//! Status tracking records as deliberately avoided.
//!
//! So [`ItemId`] is this crate's own, and it is deliberately **the same pair**
//! `Attachment` keys on and the composer's `Entry::identity` already returns:
//! the workspace slug and the path together. The reason is a measurement
//! rather than a preference — [Verification Lessons] §7: an identifier read
//! with the workspace pointer elsewhere comes back as *not found* rather than
//! as a refusal, so a bare path is an attachment that cannot locate itself.
//! `zaru-cli` is the composition root and writes the adapter between the two.
//!
//! # Why an item carries how to re-attach it
//!
//! ADR-0013 D4: "When one must go, the harness says which … `re-attach with
//! [[`". The `[[` is ADR-0005 D4's composer grammar and this crate does not
//! know it — a headless crate that hard-coded a keystroke would be rendering.
//! So the instruction travels **with the item**, supplied by whoever attached
//! it, and is refused when empty. An item that could not tell its user how to
//! get it back does not exist as a value, which is the same shape
//! `Attachment::new` uses for its four parts.
//!
//! [`Attachment`]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [Verification Lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons

use core::fmt;
use serde::{Deserialize, Serialize};

/// An attached item was offered that could not be announced if it were
/// dropped.
///
/// Carries which requirement failed and nothing that was offered, because an
/// item is assembled from a page the user chose and quoting one back into a
/// refusal is how a refusal becomes a place content travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemRefused {
    /// The workspace slug was empty, so the item names no workspace.
    NoWorkspace,
    /// The path was empty, so the item names nothing within one.
    NoPath,
    /// No re-attachment instruction was given, so ADR-0013 D4's announcement
    /// could not say how to get the item back.
    NoReattachInstruction,
}

impl fmt::Display for ItemRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let which = match self {
            Self::NoWorkspace => "its workspace slug is empty",
            Self::NoPath => "its path is empty",
            Self::NoReattachInstruction => "it says nothing about how to re-attach it",
        };
        write!(
            f,
            "this attached item could not be announced if it were dropped: {which}. ADR-0013 D4 \
             requires that a dropped attachment be named and that the announcement state how to \
             re-attach it, and an identifier without its workspace resolves nowhere"
        )
    }
}

impl std::error::Error for ItemRefused {}

/// What makes two attached items the same thing.
///
/// The workspace is part of the identity and not decoration: two workspaces
/// may each hold `architecture/bounded-contexts` and they are different pages.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ItemId {
    workspace: String,
    path: String,
}

impl ItemId {
    /// Take an identity, refusing one that names no workspace or no path.
    ///
    /// # Errors
    ///
    /// [`ItemRefused::NoWorkspace`] or [`ItemRefused::NoPath`].
    pub fn new(workspace: impl Into<String>, path: impl Into<String>) -> Result<Self, ItemRefused> {
        let workspace = workspace.into();
        let path = path.into();
        if workspace.is_empty() {
            return Err(ItemRefused::NoWorkspace);
        }
        if path.is_empty() {
            return Err(ItemRefused::NoPath);
        }
        Ok(Self { workspace, path })
    }

    /// The workspace the item lives in.
    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    /// The path within that workspace.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// The two fields as they sit on disk, with no guard applied.
///
/// Private, and it exists only so the real implementation below can read the
/// shape before deciding whether it is a value. `deny_unknown_fields` because
/// a stored identity carrying a third field is a file written by something
/// that disagreed with this type, and accepting it silently is how two
/// readings of one record start.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredId {
    workspace: String,
    path: String,
}

impl<'de> Deserialize<'de> for ItemId {
    /// Read a stored identity **through [`ItemId::new`]**.
    ///
    /// # Why this is written out rather than derived
    ///
    /// A derived implementation constructs the struct field by field and
    /// never calls the constructor, so it can produce an `ItemId` with an
    /// empty workspace — a value [`ItemId::new`] refuses, because "an
    /// identifier read without its workspace comes back as a missing page
    /// rather than as a refusal". [Operating Principles] calls anything read
    /// off disk a boundary; this is the boundary, and the guard is the point
    /// of it.
    ///
    /// The failure is a deserialisation error carrying [`ItemRefused`]'s own
    /// sentence, so a caller reading a transcript learns which requirement
    /// the stored line failed rather than that "the line did not parse".
    ///
    /// [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredId::deserialize(deserializer)?;
        Self::new(stored.workspace, stored.path).map_err(serde::de::Error::custom)
    }
}

/// One thing the user attached, and everything a drop announcement needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachedItem {
    id: ItemId,
    body: String,
    reattach: String,
}

impl AttachedItem {
    /// Take an attached item, refusing one with no re-attachment instruction.
    ///
    /// The body may be empty — a user may attach an empty page, and refusing
    /// that would be this crate deciding what is worth attaching, which is
    /// ADR-0005 D5's decision and the user's.
    ///
    /// # Errors
    ///
    /// [`ItemRefused::NoReattachInstruction`] when `reattach` is empty.
    pub fn new(
        id: ItemId,
        body: impl Into<String>,
        reattach: impl Into<String>,
    ) -> Result<Self, ItemRefused> {
        let reattach = reattach.into();
        if reattach.is_empty() {
            return Err(ItemRefused::NoReattachInstruction);
        }
        Ok(Self {
            id,
            body: body.into(),
            reattach,
        })
    }

    /// What makes this item itself.
    #[must_use]
    pub const fn id(&self) -> &ItemId {
        &self.id
    }

    /// The item's content, as it goes into the context.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// How the user gets this item back, in the words whoever attached it
    /// used.
    #[must_use]
    pub fn reattach(&self) -> &str {
        &self.reattach
    }
}
