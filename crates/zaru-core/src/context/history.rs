// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Layer 7: ADR-0013 D5's iteration history.
//!
//! "A failed iteration's full candidate and full output are transcript
//! material, not context material. In context, an older iteration becomes one
//! line: what was tried, what failed, why.
//!
//! **The most recent failure keeps its verbatim output**, because that is
//! refinement input per ADR-0008 D4 and paraphrasing it converts iteration
//! back into retry."
//!
//! # This layer is compacted by projection, not by a model call
//!
//! D1's table puts layer 7 beside layer 6 as "compacted first, summarised",
//! and D2's summarise-and-replace is written about layer 6. The difference is
//! that D5 already says what an older iteration compacts *to* — three named
//! parts the loop has already produced as structured data — so the compaction
//! is a projection over values this crate holds, not a summary somebody has
//! to generate. It therefore needs no [`Summariser`], costs no model call,
//! and runs unconditionally rather than under pressure: an older iteration is
//! one line in every rendering, including the first.
//!
//! That also means pressure has nothing left to do to layer 7, which is why
//! [`Context::compact`] touches layer 6 and then layer 5 and stops.
//!
//! # Why "one line" needs no budget
//!
//! D5 says one line, and a line ends at a newline. Each part is reduced to
//! its own first line and no byte count is invented — the truncation budget
//! ADR-0008 D4 needs is a different number for a different purpose, and this
//! module does not borrow it.
//!
//! [`Summariser`]: crate::context::Summariser
//! [`Context::compact`]: crate::context::Context::compact

use serde::{Deserialize, Serialize};

/// One iteration, as layer 7 remembers it.
///
/// The three parts are D5's own words. They arrive from whoever subscribed to
/// ADR-0008 D3's event stream; nothing here reaches into the loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IterationRecord {
    /// Which iteration this was, counting from one.
    pub n: u32,
    /// What was tried.
    pub tried: String,
    /// What failed.
    pub failed: String,
    /// Why it failed.
    pub why: String,
    /// The failing validators' own output, verbatim and untruncated.
    ///
    /// ADR-0008 D4 sends this into refinement unaltered, and D5 keeps it for
    /// the most recent failure only.
    pub verbatim: String,
}

impl IterationRecord {
    /// D5's one line: what was tried, what failed, why.
    #[must_use]
    pub fn one_line(&self) -> String {
        format!(
            "iteration {}: tried {}; {} failed; {}",
            self.n,
            first_line(&self.tried),
            first_line(&self.failed),
            first_line(&self.why)
        )
    }
}

/// Everything before the first newline, or the whole of a text with none.
fn first_line(text: &str) -> &str {
    text.split('\n')
        .next()
        .unwrap_or(text)
        .trim_end_matches('\r')
}

/// Render layer 7: every iteration but the newest as one line, the newest
/// with its failure verbatim.
///
/// "Newest" is the last element, which is the order a loop appends in.
#[must_use]
pub fn render(records: &[IterationRecord]) -> String {
    let Some((newest, older)) = records.split_last() else {
        return String::new();
    };
    let mut out = String::new();
    for record in older {
        out.push_str(&record.one_line());
        out.push('\n');
    }
    out.push_str(&newest.one_line());
    out.push('\n');
    out.push_str(&newest.verbatim);
    out
}
