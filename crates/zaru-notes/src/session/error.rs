// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What this crate raises, in its own words.
//!
//! [ADR-0016] gives each crate its own error enum and `zaru-cli` the mapping
//! into the taxonomy. **Nothing here classifies.** That is not deferral: a
//! `forbidden` from Nuclear Notes could be a revoked token, a scope change, or
//! a workspace the user was removed from — user-correctable, environmental and
//! neither, respectively — and [ADR-0006] D7 says in as many words that the
//! server does not reveal which gate tripped. A mapping made here would be
//! inventing a distinction the substrate refuses to make.
//!
//! # No refusal carries a bearer value
//!
//! Every variant below is assembled from what the server or the port said, and
//! the checks beside this module assert that a planted bearer appears in none
//! of their rendered forms — by raw value **and** by ASCII core, because
//! `{:?}` escapes a combining mark and an absence assertion on the raw value
//! alone reads a published leak as absence.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::session::address::WorkspaceSlug;
use core::fmt;

/// JSON-RPC's code for a method the server does not have.
///
/// Named rather than written inline because [`Invalidation`](super::Invalidation)
/// reads it too, and a magic number in two places is a number that diverges.
pub const METHOD_NOT_FOUND: i32 = -32601;

/// The code this crate uses for a tool error that carries no JSON-RPC code.
///
/// A tool that fails reports it as `isError` on an otherwise successful
/// response, so there is no code on the wire to read. JSON-RPC assigns no
/// meaning to zero, so zero is what stands in — a local sentinel, named rather
/// than written inline, and never a code any server sent.
pub const TOOL_ERROR: i32 = 0;

/// The server refused a tool call.
///
/// Carries the server's own code and message and nothing this crate concluded
/// from them. `detail` is the server's sentence, kept verbatim so that whatever
/// eventually surfaces it surfaces what happened rather than a paraphrase —
/// the same rule [ADR-0009](https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators) D5 applies to validator output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRefused {
    /// The tool that was called.
    pub tool: String,
    /// The JSON-RPC error code the server returned.
    pub code: i32,
    /// The server's own message.
    pub detail: String,
}

impl CallRefused {
    /// Whether the server said it has no such method.
    ///
    /// This is the arm [ADR-0135](https://cortex.page/adrs/p/0135-mcp-token-tool-scope-presets)'s
    /// three gates produce when a token's scope no longer carries a tool: the
    /// tool leaves `tools/list` and calling it is calling a method that is not
    /// there.
    #[must_use]
    pub const fn is_method_not_found(&self) -> bool {
        self.code == METHOD_NOT_FOUND
    }
}

impl fmt::Display for CallRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the server refused {}: {} (code {})",
            self.tool, self.detail, self.code
        )
    }
}

/// Everything this crate can raise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesError {
    /// The [`Endpoint`](super::Endpoint) could not produce a transport.
    Endpoint {
        /// What the implementation said, in its own words.
        detail: String,
    },
    /// The session could not be initialised.
    Attach {
        /// What `rmcp` said went wrong during `initialize`.
        detail: String,
    },
    /// The transport failed, or the peer went away.
    Transport {
        /// What `rmcp` said.
        detail: String,
    },
    /// A tool call was refused.
    Call(CallRefused),
    /// A tool answered in a shape this crate could not read.
    Unreadable {
        /// The tool that answered.
        tool: String,
        /// What was expected of the answer. Never the answer itself: a tool
        /// result is page content the user chose, and a refusal is not where
        /// content belongs.
        expected: &'static str,
    },
    /// A workspace could not be attached, and this crate declines to say why.
    ///
    /// [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces) D7:
    /// failures "throw `forbidden` without revealing which gate tripped —
    /// existence, membership, or the token's `scope.workspaceIds` filter — so
    /// the harness reports 'cannot attach that workspace' rather than inventing
    /// a more specific reason it cannot actually distinguish."
    ///
    /// **This variant carries no cause on purpose.** A field that exists is a
    /// field something will one day render, and rendering a cause the harness
    /// cannot distinguish is how a guess becomes a diagnosis.
    WorkspaceUnattachable {
        /// The slug the user asked for, which is theirs and not the server's.
        slug: WorkspaceSlug,
    },
}

impl fmt::Display for NotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Endpoint { detail } => write!(f, "could not open a transport: {detail}"),
            Self::Attach { detail } => write!(f, "could not attach a session: {detail}"),
            Self::Transport { detail } => write!(f, "the session's transport failed: {detail}"),
            Self::Call(refused) => write!(f, "{refused}"),
            Self::Unreadable { tool, expected } => write!(
                f,
                "{tool} answered in a shape this client could not read; it expected {expected}, \
                 and the answer is deliberately not quoted here"
            ),
            Self::WorkspaceUnattachable { slug } => {
                write!(f, "cannot attach that workspace: {slug}")
            }
        }
    }
}

impl std::error::Error for NotesError {}
