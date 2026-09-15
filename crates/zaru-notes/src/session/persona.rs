// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one page read [ADR-0027]'s served persona needs, and nothing else.
//!
//! # Why this is a second port and not a third method on [`Corpus`](super::corpus::Corpus)
//!
//! [`Corpus`](super::corpus::Corpus) is the composer's port and its module
//! documentation states the guarantee in as many words: it has "**two
//! methods**, both listings, and there is no third: nothing that writes,
//! **nothing that reads a page's body**, and **nothing that moves a workspace
//! pointer**". A `Recording` implementation in that module's own tests is the
//! compile-time half of that assertion — "a third method added to `Corpus` …
//! stops this block compiling" — so widening it to read the persona would
//! **delete a landed guarantee** rather than extend one.
//!
//! So the persona gets its own port, built the same way and for the same
//! reason. [`super::Session`] offers `read_page`, `search`, `ground`,
//! `attach_workspace` and the rest, and with an unscoped token every one of
//! those is a call that would succeed — measured through the harness's own
//! surface, where every Nuclear Notes token this project holds reports **94
//! tools**. [`Persona`] replaces the missing scope with a type: **one method**,
//! no defaulted body, so a second stops every implementation compiling.
//!
//! **What this port can do that `Corpus` deliberately cannot is read a page's
//! body**, because that body *is* [ADR-0013] D1's layer 1. What it still cannot
//! do is write, ground, search, or move a pointer — and **it cannot move a
//! pointer even by accident**, because [`super::Session::read_page`] takes its
//! workspace as a required argument and resolves against that argument rather
//! than against the token's current-workspace pointer. [ADR-0006] D2 — "the
//! composer's pointer moves only by user action" — is therefore untouched by
//! this port existing.
//!
//! **`pages.read` is already inside [ADR-0006] D4's set**, the
//! `read_only_memory` tools `pages.{list,read}`, `atoms.{list,read}`,
//! `search.global`, `kg.{related,list_cross_links}`, `discovery.entities`. So
//! this port widens the composer's credential by nothing; it is the second
//! consumer of a scope that already allowed it.
//!
//! **A reading that expires, in [`Corpus`](super::corpus::Corpus)' own shape.** The persona is
//! neither of ADR-0006's two surfaces, and it is read with the composer's
//! credential only because that is the one credential a real machine has. The
//! day the substrate can scope a token below an instance, or two tokens exist
//! on one machine, the persona reads with its own and this note is what says
//! the choice was made rather than assumed.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract

use crate::session::address::WorkspaceId;
use crate::session::error::NotesError;
use core::future::Future;

/// The one `read_only_memory` read [ADR-0027]'s persona is assembled from.
///
/// Implemented by [`super::Session`], and by whatever a check stages. **The
/// trait declares one method and no defaulted body**, on purpose: see the
/// module documentation.
///
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
pub trait Persona {
    /// One page's body, naming the workspace it lives in.
    ///
    /// The answer is whatever the tool gave, unparsed — see
    /// [`super::Session::read_page`] for why this crate does not model a page.
    /// **Nothing here reads a section out of it**: [ADR-0031] D3 appends the
    /// relationship memory to the served prompt *before it is returned*, so a
    /// harness that split the body into parts would be authoring a grammar for
    /// a document it does not own.
    ///
    /// # Errors
    ///
    /// [`NotesError`] exactly as [`super::Session::read_page`] raises it;
    /// nothing here interprets a refusal, because [ADR-0006] D7 says the
    /// server does not reveal which gate tripped.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    /// [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
    fn read_page(
        &self,
        path: &str,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<String, NotesError>> + Send;
}

impl Persona for super::Session {
    fn read_page(
        &self,
        path: &str,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<String, NotesError>> + Send {
        Self::read_page(self, path, workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::Persona;
    use crate::session::address::WorkspaceId;
    use crate::session::error::NotesError;
    use std::sync::Mutex;

    /// Every call a staged persona source was asked for, in order.
    #[derive(Default)]
    struct Recording {
        asked: Mutex<Vec<String>>,
    }

    // **This impl is the assertion.** The trait declares no defaulted method,
    // so a second method added to `Persona` -- anything that writes, anything
    // that lists, anything that moves a pointer -- stops this block compiling
    // with "not all trait items implemented". That is the same mechanism
    // `corpus`'s `Recording` uses, and it is why this port is a trait rather
    // than a function taking a closure.
    impl Persona for Recording {
        async fn read_page(
            &self,
            path: &str,
            workspace: &WorkspaceId,
        ) -> Result<String, NotesError> {
            self.asked
                .lock()
                .expect("no check panics while holding this")
                .push(format!("pages.read:{workspace}:{path}"));
            Ok("Ω ✦ a served persona".to_owned())
        }
    }

    /// The port offers one read and there is no second thing to call.
    ///
    /// The compile-time half is the impl above. The runtime half is here: a
    /// consumer driving the whole port reaches exactly the one tool
    /// [ADR-0006] D4 puts in the composer's set for this purpose, and nothing
    /// else — so a future method that quietly became reachable would show up
    /// as a second recorded call rather than only as a wider type.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    #[tokio::test]
    async fn the_whole_port_is_one_read_and_reaches_no_other_tool() {
        let recording = Recording::default();
        let workspace = WorkspaceId::new("a-workspace");

        let body = Persona::read_page(&recording, "zaru/persona", &workspace)
            .await
            .expect("the staged source answers");

        assert_eq!(
            body, "Ω ✦ a served persona",
            "the body did not come back as the source gave it"
        );

        let asked = recording
            .asked
            .lock()
            .expect("no check panics while holding this")
            .clone();
        assert_eq!(
            asked,
            vec!["pages.read:a-workspace:zaru/persona".to_owned()],
            "driving the whole port reached something other than D4's one read"
        );
        assert!(
            !asked.iter().any(|call| call.starts_with("me.")
                || call.contains("apply_patch")
                || call.contains("update")
                || call.contains("create")
                || call.contains(".list")),
            "the port reached a tool that writes, lists or moves a pointer: {asked:?}"
        );
    }
}
