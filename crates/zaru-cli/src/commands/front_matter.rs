// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D3's "Markdown with TOML front matter", split into its two
//! halves.
//!
//! # `+++` rather than `---`, and the reason
//!
//! D3 says "Markdown with TOML front matter" and names no delimiter, so one
//! is chosen here under a delegated coordinator ruling of 2026-09-15, open to
//! Jeshua's veto and recorded on [ADR-0015's amendments volume 2].
//!
//! **`---` is a CommonMark thematic break.** A file fenced with it is
//! ambiguous Markdown: its first construct means one thing to a reader who
//! knows about front matter and another to one who does not, and every tool
//! that renders the body — including this harness's own pane, which parses an
//! answer as CommonMark since 2026-09-15 — has to be told which. `+++` is the
//! established TOML front-matter delimiter and is not Markdown syntax at all,
//! so the file reads the same way to everything.
//!
//! # The split is by line, and the offset is carried
//!
//! The opening fence is the file's **first** line, and the block ends at the
//! next line that is exactly the fence. Everything after that line is the
//! body, verbatim, including its blank lines. The number of lines above the
//! head is handed to [`crate::config::file::TomlFile::parse_text`] so that a
//! refusal names the line of the **file** rather than of the slice — see that
//! function for why an off-by-a-fence is [ADR-0016] D2's "an error message
//! whose reader cannot act".
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0015's amendments volume 2]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates-2
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

/// The fence a command file's front matter opens and closes with.
pub const FENCE: &str = "+++";

/// The head and the body of a command file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split<'a> {
    /// The TOML between the fences, without either of them.
    pub head: &'a str,
    /// How many lines of the file sit above [`Split::head`]. Always one, and
    /// carried rather than assumed so that the position arithmetic has one
    /// source.
    pub above: usize,
    /// Everything after the closing fence's line, verbatim.
    pub body: &'a str,
}

/// Split `text` at its front matter, or `None` when there is none to split
/// at.
///
/// `None` covers both shapes a reader has to be told apart from a parse
/// failure: a file that does not open with the fence at all, and one that
/// opens with it and never closes it. Neither is a TOML error — the parser is
/// never reached — so neither can be reported as one.
#[must_use]
pub fn split(text: &str) -> Option<Split<'_>> {
    // A byte-order mark is stripped before the fence is looked for, because a
    // file written by an editor that emits one is a file a person wrote and
    // the mark is not something they can see.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text.strip_prefix(FENCE)?;
    let rest = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n'))?;

    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == FENCE {
            return Some(Split {
                head: &rest[..offset],
                above: 1,
                body: &rest[offset + line.len()..],
            });
        }
        offset += line.len();
    }
    None
}
