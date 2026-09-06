// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What `search.global` answers with, **measured** rather than guessed.
//!
//! # This module exists because a recorded guess turned out to be wrong
//!
//! [`listing`](super::listing) documents its shape as an expectation: "no
//! Nuclear Notes token exists in this workspace, so what the live tools put in
//! a result has not been read off the wire". `Session::search` was written
//! against that same reader on the same expectation, and on 2026-09-06 a live
//! `search.global` was read for the first time. **It answers a different
//! shape**, and the client did exactly what it was built to do — it refused,
//! naming what it expected, rather than silently returning nothing:
//!
//! ```text
//! Unreadable { tool: "search.global", expected: "an object carrying an array `results`" }
//! ```
//!
//! That refusal is the reason this module exists rather than a bug it papered
//! over. A reader that had accepted a second shape, or returned an empty list
//! for one it could not read, would have made a search against a live cortex
//! indistinguishable from a cortex holding nothing.
//!
//! # The measured shape, from `cortex.page` on 2026-09-06
//!
//! ```json
//! { "query": "workspace", "scope": "workspace", "tookMs": 22,
//!   "semanticAvailable": true,
//!   "hits": [ { "kind": "atom", "id": "…", "title": "Workspace",
//!               "path": "concepts/workspace",
//!               "workspaceId": "…", "workspaceSlug": "docs",
//!               "snippet": "…<mark>Workspace</mark>…", "score": 0.032,
//!               "matchedVia": "both",
//!               "permalink": "https://cortex.page/docs/a/concepts/workspace",
//!               "uri": "nn://workspace/docs/a/concepts/workspace" } ] }
//! ```
//!
//! The container is **`hits`**, not `results`; the rows are richer than a
//! listing's; and the four fields [ADR-0006] D6 requires an attachment to carry
//! — the workspace slug, the path, the permalink and the `nn://` URI — are all
//! on every row. A search result is therefore enough to build an [`Attachment`]
//! from, which a listing row is not.
//!
//! # What is read and what is deliberately left
//!
//! Read: `path`, `title`, `workspaceSlug`, `permalink`, `uri`, and `snippet`.
//! Left: `kind`, `id`, `score`, `matchedVia`, `workspaceId`, and the four
//! envelope fields beside `hits`.
//!
//! `score` and `matchedVia` are the server's ranking showing its work, and the
//! answer already arrives in that order — reading them would invite a client to
//! re-sort by a number whose scale is the server's. `id` and `workspaceId` are
//! identifiers, and [ADR-0006]'s Context is that an identifier resolves only
//! within its own workspace; **that was re-measured against the live substrate
//! on 2026-09-06** and holds, so carrying one is carrying a value that fails
//! elsewhere in a way that looks like a missing page. `kind` is not read for
//! the reason [`listing`](super::listing) gives about its own.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [`Attachment`]: super::Attachment

use crate::session::error::NotesError;
use serde_json::Value;

/// What the search answer's rows are under.
///
/// Named rather than written inline, because it is the one word this module got
/// wrong before it was measured.
pub const HITS: &str = "hits";

/// One hit a search returned.
///
/// # Why this is not [`Listed`](super::Listed)
///
/// A listing row is a path and a title, and that is all `pages.list` gives. A
/// search hit carries the whole of [ADR-0006] D6's self-locating identity, and
/// dropping it here would mean re-deriving a permalink from a path — which is
/// the client inventing a URL rather than using the one the server sent.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The entity's path inside its own workspace.
    pub path: String,
    /// The entity's title.
    pub title: String,
    /// The slug of the workspace it lives in, which is the other half of
    /// [ADR-0006] D6's identity pair.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub workspace: String,
    /// The server's own permalink.
    pub permalink: String,
    /// The server's own `nn://` URI.
    pub uri: String,
    /// The matching text, as the server marked it up.
    ///
    /// Carried verbatim, including its `<mark>` tags, because whatever renders
    /// it decides what to do with them and a client that stripped them would
    /// have thrown away which words matched.
    pub snippet: String,
}

/// Read one search answer.
///
/// # Errors
///
/// [`NotesError::Unreadable`] naming the expectation, when the answer is not a
/// JSON object carrying an array `hits`, or a row is missing one of the five
/// fields [`Found`] is built from.
pub fn read(tool: &str, answer: &str) -> Result<Vec<Found>, NotesError> {
    let unreadable = |expected| NotesError::Unreadable {
        tool: tool.to_owned(),
        expected,
    };

    let parsed: Value = serde_json::from_str(answer)
        .map_err(|_| unreadable("a JSON object carrying an array `hits`"))?;
    let hits = parsed
        .get(HITS)
        .and_then(Value::as_array)
        .ok_or_else(|| unreadable("a JSON object carrying an array `hits`"))?;

    hits.iter()
        .map(|hit| {
            let text = |field: &str| hit.get(field).and_then(Value::as_str).map(str::to_owned);
            Ok(Found {
                path: text("path").ok_or_else(|| unreadable("a string `path` on every hit"))?,
                title: text("title").ok_or_else(|| unreadable("a string `title` on every hit"))?,
                workspace: text("workspaceSlug")
                    .ok_or_else(|| unreadable("a string `workspaceSlug` on every hit"))?,
                permalink: text("permalink")
                    .ok_or_else(|| unreadable("a string `permalink` on every hit"))?,
                uri: text("uri").ok_or_else(|| unreadable("a string `uri` on every hit"))?,
                // The only optional one. A hit whose match was the title alone
                // has nothing to quote, and refusing the whole answer for a
                // missing excerpt would lose four usable fields over a fifth
                // that is decoration.
                snippet: text("snippet").unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured answer, trimmed to one hit and kept byte-for-byte
    /// otherwise.
    ///
    /// Taken from `cortex.page` on 2026-09-06 rather than composed here, which
    /// is the difference between a check that pins the shape and one that pins
    /// what somebody imagined it to be.
    const MEASURED: &str = r#"{"query":"workspace","scope":"workspace","hits":[{"kind":"atom","id":"f0876ce3-9eed-4c18-8682-8e3b8cba9730","title":"Workspace","path":"concepts/workspace","workspaceId":"3fbab507-d9fa-41de-85b7-1de12c547c39","workspaceSlug":"docs","snippet":"<mark>Workspace</mark> \u2014 the unit you work in","score":0.032,"matchedVia":"both","permalink":"https://cortex.page/docs/a/concepts/workspace","uri":"nn://workspace/docs/a/concepts/workspace"}],"tookMs":22,"semanticAvailable":true}"#;

    #[test]
    fn the_measured_answer_reads_and_carries_adr_0006_d6s_four_parts() {
        let found = read("search.global", MEASURED).expect("the shape the live server sends");
        assert_eq!(found.len(), 1);
        let hit = &found[0];
        assert_eq!(hit.path, "concepts/workspace");
        assert_eq!(hit.title, "Workspace");
        assert_eq!(hit.workspace, "docs");
        assert_eq!(
            hit.permalink,
            "https://cortex.page/docs/a/concepts/workspace"
        );
        assert_eq!(hit.uri, "nn://workspace/docs/a/concepts/workspace");
        assert!(hit.snippet.contains("<mark>Workspace</mark>"));
    }

    #[test]
    fn the_shape_this_client_used_to_expect_is_refused_naming_hits() {
        // The container a listing uses. Refused here, and the refusal names
        // what this tool actually sends -- which is the whole reason the wrong
        // guess was findable rather than silent.
        let refusal = read("search.global", r#"{"results":[{"path":"a","title":"b"}]}"#)
            .expect_err("a listing's container is not a search's");
        assert!(
            refusal.to_string().contains("`hits`"),
            "the refusal does not name the container this tool uses: {refusal}"
        );
    }

    #[test]
    fn a_hit_missing_one_of_the_four_identity_parts_is_refused_naming_it() {
        for missing in ["path", "title", "workspaceSlug", "permalink", "uri"] {
            let mut hit = serde_json::json!({
                "path": "a", "title": "b", "workspaceSlug": "c",
                "permalink": "d", "uri": "e"
            });
            hit.as_object_mut()
                .expect("an object")
                .remove(missing)
                .expect("the field was there to remove");
            let answer = serde_json::json!({ "hits": [hit] }).to_string();
            let refusal = read("search.global", &answer)
                .expect_err("a hit that cannot locate itself is not a hit");
            assert!(
                refusal.to_string().contains(missing),
                "dropping {missing} was refused without naming it: {refusal}"
            );
        }
    }

    #[test]
    fn a_hit_with_no_snippet_is_read_rather_than_refused() {
        // The one field that is decoration. Refusing four usable parts over a
        // missing excerpt would be the reader deciding a search failed.
        let answer = serde_json::json!({
            "hits": [{ "path": "a", "title": "b", "workspaceSlug": "c",
                       "permalink": "d", "uri": "e" }]
        })
        .to_string();
        let found = read("search.global", &answer).expect("a hit with no excerpt is still a hit");
        assert_eq!(found[0].snippet, "");
    }

    #[test]
    fn an_answer_with_no_hits_is_an_empty_answer_and_not_a_refusal() {
        let found = read("search.global", r#"{"query":"x","hits":[],"tookMs":1}"#)
            .expect("a search that matched nothing is an answer");
        assert!(found.is_empty());
    }
}
