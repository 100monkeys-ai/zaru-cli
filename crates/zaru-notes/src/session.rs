// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A session against Nuclear Notes, and the values that cross it.
//!
//! # The rule this module turns into a type
//!
//! The Zaru workspace grounding calls it "the rule that will bite you": the
//! MCP token has **one** current-workspace pointer, shared by every session
//! presenting that token, and a call that does not name its workspace resolves
//! against wherever the pointer happens to be. A read comes back from the
//! wrong product and a write lands there, and nothing errors.
//!
//! [`Session::read_page`] therefore takes the workspace as a **required
//! argument**. There is no overload that omits it and no default to fall back
//! on, so the failure has no method to call. That is the same argument
//! [ADR-0014] D4 makes about configuration and secrets — "the design decision
//! that prevents it is refusing to have a field to put one in" — applied to an
//! argument rather than a field.
//!
//! The same shape appears twice more. [`Session::attach_workspace`] takes a
//! [`WorkspaceId`] and there is no overload taking a [`WorkspaceSlug`], which
//! is [ADR-0006] D7's rule that apex callers must pass an id made structural.
//! And [`Attachment`] has no constructor that omits any of its four parts, so
//! an attachment that could not locate itself does not compile — [ADR-0006] D6.
//!
//! # Vocabulary
//!
//! [Ubiquitous Language] calls what a session is pointed at the **attached
//! workspace**, and marks "current workspace" and "active workspace" as terms
//! this codebase does not use — even though the wire call is spelled
//! `me.set_current_workspace` and the server's column is
//! `current_workspace_id`. The wire spelling is the substrate's; the word in
//! this crate is [`Session::attached_workspace`].
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

pub mod address;
pub mod attachment;
pub mod bearer;
pub mod client;
pub mod endpoint;
pub mod error;
pub mod invalidation;
pub mod listing;
pub mod transport;

pub use address::{Instance, WorkspaceId, WorkspaceSlug};
pub use attachment::{Attachment, AttachmentRefused};
pub use bearer::{Bearer, REDACTED};
pub use client::{GROUND, LIST_ATOMS, LIST_PAGES, Negotiated, SEARCH_GLOBAL, Session};
pub use endpoint::{Endpoint, EndpointFailure};
pub use error::{CallRefused, NotesError};
pub use invalidation::Invalidation;
pub use listing::Listed;
pub use transport::{HttpEndpoint, MCP_PATH};

#[cfg(test)]
pub(crate) mod fixtures;
#[cfg(test)]
mod tests;
