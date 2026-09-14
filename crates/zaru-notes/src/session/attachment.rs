// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0006] D6's self-locating attachment.
//!
//! # Why this type has no short constructor
//!
//! D6: "Because UUIDs do not resolve across workspaces, an attachment carries
//! the **workspace slug and path** together with the permalink and `nn://`
//! URI — never a bare path or bare identifier."
//!
//! That clause exists because of a measurement rather than a preference. The
//! record's Context reports it, and [Verification Lessons] §7 records it
//! again: reading a known-good page identifier with the workspace pointer
//! elsewhere returns **not found**, not a permission error. So an attachment
//! carrying a bare identifier fails in the shape of a missing page, which is
//! the hardest failure to diagnose because it looks like the data is gone.
//!
//! [`Attachment::new`] therefore takes all four parts and there is no
//! constructor that takes fewer. An attachment that could not tell a reader
//! which workspace to point at does not exist as a value.
//!
//! # The four parts are checked against each other, not merely present
//!
//! A permalink naming one workspace beside a slug naming another is an
//! attachment that cannot locate itself while passing every non-empty check.
//! So the permalink and the URI must each carry the slug **and** the path. Both
//! hold for every entity this substrate returns — a permalink reads
//! `https://<host>/<slug>/p/<path>` and a URI reads `nn://workspace/<slug>/p/<path>`
//! — which is what makes the check an invariant rather than a guess.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [Verification Lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons

use crate::session::address::WorkspaceSlug;
use core::fmt;

/// An attachment was offered that could not locate itself.
///
/// Carries which of D6's requirements failed and nothing that was offered,
/// because an attachment is assembled from a page a user chose and quoting one
/// back into a refusal is how a refusal becomes a place content travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentRefused {
    /// The workspace slug was empty, so the attachment names no workspace.
    NoWorkspace,
    /// The path was empty, so the attachment names nothing within one.
    NoPath,
    /// The permalink does not carry the workspace slug.
    PermalinkOmitsWorkspace,
    /// The permalink does not carry the path.
    PermalinkOmitsPath,
    /// The `nn://` URI does not carry the workspace slug.
    UriOmitsWorkspace,
    /// The `nn://` URI does not carry the path.
    UriOmitsPath,
}

impl fmt::Display for AttachmentRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let which = match self {
            Self::NoWorkspace => "its workspace slug is empty",
            Self::NoPath => "its path is empty",
            Self::PermalinkOmitsWorkspace => "its permalink does not carry its workspace slug",
            Self::PermalinkOmitsPath => "its permalink does not carry its path",
            Self::UriOmitsWorkspace => "its nn:// URI does not carry its workspace slug",
            Self::UriOmitsPath => "its nn:// URI does not carry its path",
        };
        write!(
            f,
            // ADR-0006 D6 requires the workspace slug and path together with the
            // permalink and nn:// URI, never a bare path or bare identifier.
            // `which` already names which of those is missing.
            "this attachment cannot locate itself: {which}, and an identifier resolves only \
             within its own workspace -- one read from elsewhere comes back as a missing page \
             rather than as a refusal"
        )
    }
}

impl std::error::Error for AttachmentRefused {}

/// A reference to a page or atom that carries where it is.
///
/// The fields are private and the accessors are read-only, so an attachment
/// cannot be taken apart and reassembled around a different workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    workspace: WorkspaceSlug,
    path: String,
    permalink: String,
    uri: String,
}

impl Attachment {
    /// Take an attachment, refusing one that cannot locate itself.
    ///
    /// # Errors
    ///
    /// [`AttachmentRefused`] naming which of D6's requirements failed.
    pub fn new(
        workspace: WorkspaceSlug,
        path: impl Into<String>,
        permalink: impl Into<String>,
        uri: impl Into<String>,
    ) -> Result<Self, AttachmentRefused> {
        let path = path.into();
        let permalink = permalink.into();
        let uri = uri.into();
        let slug = workspace.as_str();

        if slug.is_empty() {
            return Err(AttachmentRefused::NoWorkspace);
        }
        if path.is_empty() {
            return Err(AttachmentRefused::NoPath);
        }
        if !permalink.contains(slug) {
            return Err(AttachmentRefused::PermalinkOmitsWorkspace);
        }
        if !permalink.contains(&path) {
            return Err(AttachmentRefused::PermalinkOmitsPath);
        }
        if !uri.contains(slug) {
            return Err(AttachmentRefused::UriOmitsWorkspace);
        }
        if !uri.contains(&path) {
            return Err(AttachmentRefused::UriOmitsPath);
        }

        Ok(Self {
            workspace,
            path,
            permalink,
            uri,
        })
    }

    /// The workspace this attachment lives in.
    #[must_use]
    pub const fn workspace(&self) -> &WorkspaceSlug {
        &self.workspace
    }

    /// The path within that workspace.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The permalink a human follows.
    #[must_use]
    pub fn permalink(&self) -> &str {
        &self.permalink
    }

    /// The `nn://` URI.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }
}
