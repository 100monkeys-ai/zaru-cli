// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a `pages.list` or `atoms.list` answer carries, and how it is read.
//!
//! # The shape is expected, not measured, and it fails loudly rather than
//! guessing
//!
//! No Nuclear Notes token exists in this workspace, so what the live tools put
//! in a result has not been read off the wire. This module expects a JSON array
//! of objects each carrying a string `path` and a string `title`, and refuses
//! with [`NotesError::Unreadable`] naming that expectation when it does not
//! find one — the same discipline [`Session::resolve_slug`] already uses, and
//! for the same reason: a client that silently accepted a second shape would
//! hide the day the guess was wrong.
//!
//! **One thing was observed rather than assumed, and it is still not a wire
//! measurement.** On 2026-09-05 a `pages.list` against the Zaru workspace,
//! read through an agent's MCP tool surface, answered with an array of objects
//! carrying `kind`, `id`, `path`, `title`, `visibility` and `updatedAt`. That
//! is the rendering an agent was handed and not necessarily the bytes of the
//! result's first text block, so it narrows the guess without settling it. It
//! is recorded here and on [ADR-0006]'s Status tracking beside the two shapes
//! the `notes-client` arc already recorded as guesses.
//!
//! # `kind` is ignored on purpose
//!
//! The rows carry one and this module does not read it. Which kind a listing
//! returned is decided by **which tool was called** — [`Session::pages`]
//! answers pages and [`Session::atoms`] answers atoms — so a row whose `kind`
//! disagreed with the tool that produced it cannot mislabel an entry here.
//! Reading it would make the answer depend on a field the caller already knows.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [`Session::atoms`]: crate::session::Session::atoms
//! [`Session::pages`]: crate::session::Session::pages
//! [`Session::resolve_slug`]: crate::session::Session::resolve_slug

use crate::session::error::NotesError;
use serde_json::Value;

/// One entity a listing returned.
///
/// Two fields and no identifier. [ADR-0006] D6 makes an entity locatable by its
/// **workspace and path**, and the workspace is what the caller named on the
/// call — so a listing row needs to carry the path, and an identifier it could
/// carry would be one that resolves nowhere else, which is that record's whole
/// Context.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The entity's path inside the workspace the call named.
    pub path: String,
    /// The entity's title.
    pub title: String,
}

/// The cursor a listing hands back to ask for the next page, if it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The rows this page carried.
    pub listed: Vec<Listed>,
    /// What to pass back for the next page, when there is one.
    pub next: Option<String>,
}

/// Read one listing answer.
///
/// # Errors
///
/// [`NotesError::Unreadable`] naming the expectation, when the answer is not
/// JSON, is not an array or an object carrying one, or carries a row without a
/// string `path` and a string `title`.
pub fn read(tool: &str, answer: &str) -> Result<Page, NotesError> {
    let unreadable = |expected| NotesError::Unreadable {
        tool: tool.to_owned(),
        expected,
    };

    let parsed: Value =
        serde_json::from_str(answer).map_err(|_| unreadable("a JSON array or object"))?;

    // Two containers are accepted because the tool documents cursor-based
    // pagination and an array cannot carry a cursor. A bare array is a page
    // with no next; an object must carry the rows under `results` and may
    // carry `nextCursor`. Neither is a fallback for the other failing -- they
    // are different documents, told apart by their own type.
    let (rows, next) = match &parsed {
        Value::Array(rows) => (rows.clone(), None),
        Value::Object(map) => {
            let rows = map
                .get("results")
                .and_then(Value::as_array)
                .ok_or_else(|| unreadable("an object carrying an array `results`"))?;
            let next = map
                .get("nextCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            (rows.clone(), next)
        }
        _ => return Err(unreadable("a JSON array or object")),
    };

    let mut listed = Vec::with_capacity(rows.len());
    for row in rows {
        let path = row
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| unreadable("every row to carry a string `path`"))?;
        let title = row
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| unreadable("every row to carry a string `title`"))?;
        listed.push(Listed {
            path: path.to_owned(),
            title: title.to_owned(),
        });
    }
    Ok(Page { listed, next })
}

#[cfg(test)]
mod tests {
    use super::{Listed, read};
    use crate::session::error::NotesError;

    const TOOL: &str = "pages.list";

    /// A bare array is a page with no next; an object carries its cursor.
    #[test]
    fn both_containers_are_read_and_only_one_can_carry_a_cursor() {
        let array = read(
            TOOL,
            r#"[{"path":"édge/one","title":"Édge ✦","kind":"page"}]"#,
        )
        .expect("an array is a listing");
        assert_eq!(
            array.listed,
            vec![Listed {
                path: "édge/one".to_owned(),
                title: "Édge ✦".to_owned()
            }]
        );
        assert_eq!(array.next, None, "an array cannot carry a cursor");

        let object = read(
            TOOL,
            r#"{"results":[{"path":"édge/two","title":"Édge ✦ 2"}],"nextCursor":"c-9f2a"}"#,
        )
        .expect("an object carrying `results` is a listing");
        assert_eq!(object.listed.len(), 1);
        assert_eq!(object.next.as_deref(), Some("c-9f2a"));
    }

    /// Every refusal names what was expected rather than what was found, so a
    /// reader is told the contract instead of the symptom.
    #[test]
    fn every_unreadable_shape_is_refused_naming_the_expectation() {
        for (answer, expected) in [
            ("not json at all", "a JSON array or object"),
            ("42", "a JSON array or object"),
            (r#"{"pages":[]}"#, "an object carrying an array `results`"),
            (
                r#"[{"title":"Édge ✦"}]"#,
                "every row to carry a string `path`",
            ),
            (
                r#"[{"path":"édge","title":7}]"#,
                "every row to carry a string `title`",
            ),
        ] {
            match read(TOOL, answer) {
                Err(NotesError::Unreadable { tool, expected: e }) => {
                    assert_eq!(tool, TOOL, "the refusal names the tool that answered");
                    assert_eq!(e, expected, "for the answer {answer:?}");
                }
                other => panic!("{answer:?} should have been refused; it gave {other:?}"),
            }
        }
    }

    /// The accepting sibling of the refusals above: a well-formed answer with
    /// extra fields is read, so the refusals are not "refuse anything".
    #[test]
    fn a_row_carrying_more_than_this_client_reads_is_still_read() {
        let page = read(
            TOOL,
            r#"[{"kind":"page","id":"b73ea90d","path":"adrs/0005","title":"Ω ✦","visibility":"public","updatedAt":"2026-09-05"}]"#,
        )
        .expect("the shape observed on 2026-09-05 is read");
        assert_eq!(page.listed[0].path, "adrs/0005");
        assert_eq!(
            page.listed[0].title, "Ω ✦",
            "and the title comes back exactly as the server spelled it"
        );
    }
}
