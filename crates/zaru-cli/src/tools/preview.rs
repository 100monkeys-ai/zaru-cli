// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What [ADR-0011] D3's question shows of the call it is about.
//!
//! # The measurement this module exists for
//!
//! From the release binary at `a8eedf7` over a pseudo-terminal at `--mode
//! ask`, the whole prompt for an `fs.write` was `Allow fs.write <path>?` over
//! `[y/N]`. A create and an overwrite of the same path with different content
//! produced **byte-identical** questions, and `fs.edit` showed neither the
//! string it was replacing nor its replacement. [ADR-0016] D2's test — a
//! message whose reader cannot act "is a stack trace with better grammar" —
//! is the one that applies: a question whose reader cannot see its subject is
//! a permission model in name only.
//!
//! # What is shown, and what is deliberately not
//!
//! | Tool | Detail |
//! | --- | --- |
//! | `fs.write` | whether the path exists, then the bytes that would be written |
//! | `fs.edit` | the exact string being replaced, then its replacement |
//! | `cmd.run` | the program and each argument on its own row |
//! | `web.fetch` | **nothing** — a URL is the whole argument and the statement already is it |
//! | `fs.read`, `fs.list` | **nothing** — neither has an argument beyond its path |
//! | `fs.search` | **nothing** — its root and quoted needle are already in the rendered line |
//!
//! # Three things this module does not do
//!
//! **It does not touch [`Invocation::subject_text`]**, which D3's allowlist
//! compares byte for byte. Everything here is carried beside that string and
//! reaches neither the allowlist nor the line a transcript holds.
//!
//! **It authors no elision marker.** D5 already decided how an elision reads
//! and `tools::output`'s own `excerpt` is that decision — named in prose
//! rather than linked, because it is `pub(crate)` and rustdoc is right to
//! refuse a public page pointing at something a reader of that page cannot
//! open. This module calls it rather than spelling `[... N bytes elided ...]`
//! a second time.
//!
//! **It redacts before it composes.** [ADR-0008] clause 6's port is applied
//! where a capture becomes text a *model* is given, and a prompt runs the
//! other way, so nothing already decided covers this direction. It is applied
//! anyway, on [ADR-0007] D3's structural argument that no byte of a held
//! secret reaches a frame. **The cost is stated rather than discovered**: the
//! file receives the bytes the model asked for and the pane shows the marker
//! in their place, so a person approving a write that contains a stored key
//! sees a marker where the key is and the key is still what lands on disk.
//! The alternative — painting the value so the preview is literally what will
//! be written — puts a credential on a screen, in a capture and in terminal
//! scrollback, which is the disclosure `MASK`'s own reasoning already refuses
//! to pay twice.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::tools::decision::{Invocation, Subject};
use crate::tools::output::{OutputBudget, excerpt};
use zaru_core::redaction::Redactor;

/// The heading over an `fs.write` whose path is not there yet.
///
/// **Drafted under a delegated coordinator ruling of 2026-09-15 00:13:45Z,
/// open to Jeshua's veto**, in the same shape as `MASK`, `PROMINENT`,
/// `STRIP_ROWS` and `QUEUED` one crate over: no record gives the words, one
/// is needed, so it is named once here with its reasoning rather than typed
/// at a call site. Recorded on [ADR-0011's amendments volume 3].
///
/// # It is a check at a moment, and that is said out loud
///
/// Whether the path exists is read when the question is composed. Between
/// that read and the act, a path can appear or vanish — the same window
/// [`files`](crate::tools::files) already admits for D4's classification,
/// which resolves a write against a tree that does not yet contain what it is
/// about to make. **It changes nothing about D4** and claims no containment:
/// it is a sentence about what the harness saw, at the moment it asked.
///
/// [ADR-0011's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface-updates-3
pub const CREATES: &str = "creates it, with:";

/// The heading over an `fs.write` whose path is already there.
///
/// The counterpart to [`CREATES`], with the same provenance and the same
/// check-at-a-moment caveat. Two headings rather than one, because "creates
/// or overwrites" told a reader nothing the tool's own name did not, and the
/// two cases were byte-identical on the frame until 2026-09-14.
pub const REPLACES_THE_FILE: &str = "replaces what is there, with:";

/// The heading over the string an `fs.edit` is replacing.
///
/// Drafted with [`CREATES`] and on the same ruling.
pub const REPLACES: &str = "replaces:";

/// The heading over an `fs.edit`'s replacement.
///
/// Drafted with [`CREATES`] and on the same ruling.
pub const WITH: &str = "with:";

/// The heading over a `cmd.run`'s split program and argument vector.
///
/// Drafted with [`CREATES`] and on the same ruling. The rendered command line
/// is already the statement; this is the same line **as the harness split
/// it**, which is the only form in which a quoted argument containing a space
/// reads as one argument rather than as two words.
pub const AS_SPLIT: &str = "runs, as split:";

/// What every detail row under a heading is indented by.
///
/// Layout rather than vocabulary — two columns, the same relationship a
/// continuation row has to its lead one crate over — so it is not on the list
/// of authored sentences batched for veto.
const INDENT: &str = "  ";

/// The lines [`Decision::question`](crate::tools::Decision::question) shows
/// under its statement.
///
/// Empty for the four tools whose whole argument is already in the statement.
/// See the module documentation for the table and for why each half is where
/// it is.
#[must_use]
pub fn detail_for(
    invocation: &Invocation<'_>,
    budget: OutputBudget,
    redactor: &dyn Redactor,
) -> Vec<String> {
    match invocation.subject() {
        Subject::Write { target, contents } => {
            // Read at the moment the question is composed. See `CREATES`.
            let heading = if target.resolved().exists() {
                REPLACES_THE_FILE
            } else {
                CREATES
            };
            let mut lines = vec![heading.to_owned()];
            lines.extend(indented(contents, budget, redactor));
            lines
        }
        Subject::Edit { old, new, .. } => {
            let mut lines = vec![REPLACES.to_owned()];
            lines.extend(indented(old, budget, redactor));
            lines.push(WITH.to_owned());
            lines.extend(indented(new, budget, redactor));
            lines
        }
        Subject::Command(line) => {
            let mut lines = vec![AS_SPLIT.to_owned()];
            lines.push(format!("{INDENT}{}", redactor.redact(line.program())));
            for argument in line.arguments() {
                lines.push(format!("{INDENT}{}", redactor.redact(argument)));
            }
            lines
        }
        // **A projected call's arguments are in the statement, not here**, and
        // that is the whole of what distinguishes it from `fs.write`. A
        // write's subject is its *path* -- the contents reach this detail and
        // are redacted on the way -- because the path is what D3's allowlist
        // matches. A projected call has no path, so its arguments *are* its
        // subject: they are what makes one `pages.read` a different line from
        // another, and therefore what D3's third answer grants for the
        // session. Showing them twice, once raw in the statement and once
        // redacted here, would show the reader two versions of one call.
        //
        // What keeps a held value out of that statement is a refusal rather
        // than a redaction -- see `execute`'s projected arm -- which is the
        // stronger of the two: a marker in a question still sends the value.
        Subject::Path(_)
        | Subject::Search { .. }
        | Subject::Url(_)
        | Subject::Remote { .. } => Vec::new(),
    }
}

/// `text`, redacted, cut to `budget` by D5's own elision, one row per line,
/// each indented.
///
/// **Redacted before it is cut**, which is the order that matters: cutting
/// first could split a held value across the elision boundary and leave half
/// of it on the frame with nothing matching it — the same straddling failure
/// `output`'s own module documentation records for the model's copy.
fn indented(text: &str, budget: OutputBudget, redactor: &dyn Redactor) -> Vec<String> {
    let redacted = redactor.redact(text);
    excerpt(&redacted, budget)
        .as_str()
        .lines()
        .map(|line| format!("{INDENT}{line}"))
        .collect()
}
