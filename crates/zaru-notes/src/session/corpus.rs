// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two listings the composer's fast tier is built from, and nothing else.
//!
//! # This narrow port is what carries [ADR-0006] D4's "cannot write"
//!
//! D4 scopes the composer's credential to the `read_only_memory` set, and its
//! claim is that the token "cannot write, enforced at all three gates,
//! regardless of what any code in the harness attempts". **Those gates are the
//! server's, and as of 2026-09-14 no token exists that can be scoped to them.**
//! Measured through the harness's own surface that day: every Nuclear Notes
//! token this project holds reports **94 tools**, the whole surface, and
//! Jeshua's own reading of 2026-09-05 is "I do not believe we have workspace
//! scoped tokens, only instance scope". So [`super::Session`] — which offers
//! `read_page`, `search`, `ground`, `attach_workspace` and the rest — is
//! exactly the value the composer must *not* be handed, because with an
//! unscoped token every one of those is a call that would succeed.
//!
//! [`Corpus`] is the port that replaces the missing scope with a type. It has
//! **two methods**, both listings, and there is no third: nothing that writes,
//! nothing that reads a page's body, and **nothing that moves a workspace
//! pointer** — which is [ADR-0006] D2's "the composer's pointer moves only by
//! user action" made structural rather than remembered, since a builder handed
//! only this has no `me.set_current_workspace` to reach.
//!
//! That is the whole of the guarantee and it is deliberately modest: it does
//! not stop a *different* caller doing anything, and it says nothing about what
//! the server would permit. What it does is make the composer's own path
//! unable to express a write, where before this arc nothing constrained that
//! path at all because nothing populated it.
//!
//! Recorded as a proposed amendment to [ADR-0006] D4 and [ADR-0007] D4 on 2026-09-14,
//! accepted under a delegated coordinator ruling and open to Jeshua's veto.
//! **It is a reading that expires** — when the substrate can scope a token
//! below an instance, or when D4's two corrected spellings are settled and a
//! preset token is minted, the composer reads with a credential that carries
//! the scope and this port stops being the only thing holding the line.
//!
//! # Why a trait and not a function taking two closures
//!
//! A check has to be able to assert the method set, and a trait is the only
//! shape where "there are exactly these two" is a thing a compiler enforces:
//! the trait declares **no defaulted method**, so every implementation must
//! name both and adding a third stops every implementation compiling. A pair
//! of closures would make the same assertion a count somebody maintains.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use crate::session::address::WorkspaceId;
use crate::session::error::NotesError;
use crate::session::listing::Listed;
use core::future::Future;

/// The two `read_only_memory` listings [ADR-0005] D3's trie is built from.
///
/// Implemented by [`super::Session`], and by whatever a check stages. **No
/// method has a default body**, on purpose: see the module documentation.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
pub trait Corpus {
    /// Every page in `workspace`, following the listing's own cursor.
    ///
    /// # Errors
    ///
    /// [`NotesError`] exactly as [`super::Session::pages`] raises it; nothing
    /// here interprets a refusal, because [ADR-0006] D7 says the server does
    /// not reveal which gate tripped.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    fn pages(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<Vec<Listed>, NotesError>> + Send;

    /// Every atom in `workspace`, following the listing's own cursor.
    ///
    /// # Errors
    ///
    /// As [`Corpus::pages`].
    fn atoms(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<Vec<Listed>, NotesError>> + Send;
}

impl Corpus for super::Session {
    fn pages(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<Vec<Listed>, NotesError>> + Send {
        Self::pages(self, workspace)
    }

    fn atoms(
        &self,
        workspace: &WorkspaceId,
    ) -> impl Future<Output = Result<Vec<Listed>, NotesError>> + Send {
        Self::atoms(self, workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::Corpus;
    use crate::session::address::WorkspaceId;
    use crate::session::error::NotesError;
    use crate::session::listing::Listed;
    use std::sync::Mutex;

    /// Every call a staged corpus was asked for, in order.
    #[derive(Default)]
    struct Recording {
        asked: Mutex<Vec<String>>,
    }

    // **This impl is the assertion.** The trait declares no defaulted method,
    // so a third method added to `Corpus` -- anything that writes, anything
    // that reads a body, anything that moves a pointer -- stops this block
    // compiling with "not all trait items implemented". That is a stronger
    // statement than any runtime check could make, and it is why the port is
    // a trait rather than a pair of closures.
    impl Corpus for Recording {
        async fn pages(&self, workspace: &WorkspaceId) -> Result<Vec<Listed>, NotesError> {
            self.asked
                .lock()
                .expect("no check panics while holding this")
                .push(format!("pages.list:{workspace}"));
            Ok(vec![Listed {
                path: "adrs/0005".to_owned(),
                title: "Ω ✦".to_owned(),
            }])
        }

        async fn atoms(&self, workspace: &WorkspaceId) -> Result<Vec<Listed>, NotesError> {
            self.asked
                .lock()
                .expect("no check panics while holding this")
                .push(format!("atoms.list:{workspace}"));
            Ok(Vec::new())
        }
    }

    /// The port offers two listings and there is no third thing to call.
    ///
    /// The compile-time half is the impl above. The runtime half is here: a
    /// consumer driving the whole port reaches exactly the two tools
    /// [ADR-0006] D4 puts in the composer's set for this purpose, and nothing
    /// else — so a future method that quietly became reachable would show up
    /// as a third recorded call rather than only as a wider type.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    #[tokio::test]
    async fn the_whole_port_is_two_listings_and_reaches_no_other_tool() {
        let recording = Recording::default();
        let workspace = WorkspaceId::new("a-workspace");

        let pages = Corpus::pages(&recording, &workspace)
            .await
            .expect("the staged corpus answers");
        let atoms = Corpus::atoms(&recording, &workspace)
            .await
            .expect("the staged corpus answers");

        assert_eq!(pages.len(), 1, "the page listing did not come back");
        assert!(atoms.is_empty(), "the atom listing did not come back");

        let asked = recording
            .asked
            .lock()
            .expect("no check panics while holding this")
            .clone();
        assert_eq!(
            asked,
            vec![
                "pages.list:a-workspace".to_owned(),
                "atoms.list:a-workspace".to_owned()
            ],
            "driving the whole port reached something other than D4's two listings"
        );
        assert!(
            !asked.iter().any(|call| call.starts_with("me.")
                || call.contains("apply_patch")
                || call.contains("update")
                || call.contains("create")),
            "the port reached a tool that writes or moves a pointer: {asked:?}"
        );
    }
}
