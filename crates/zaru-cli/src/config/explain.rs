// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0014 D3's explanation, as data a caller renders.
//!
//! # Why this exists at all, in the record's own words
//!
//! "**Layered configuration without an explain command is a debugging problem
//! the user cannot solve.** This command is not a convenience; it is what
//! makes the hierarchy legible, and it is the same commitment as showing the
//! iteration loop."
//!
//! # Five rows, always, including the ones that set nothing
//!
//! D3's block prints every layer:
//!
//! ```text
//! runtime.max_iterations = 8
//!   5  flag              (not set)
//!   4  ZARU_MAX_ITER     (not set)
//!   3  ./zaru.toml       8          ← effective
//!   2  ~/.zaru/config    5
//!   1  built-in          3
//! ```
//!
//! Three of those five rows say `(not set)` and two of them still name a
//! source. So an [`Explanation`] carries a row per layer whatever the layer
//! did, and the row's source is known without a value: for the environment it
//! is the variable the key maps to, and for the rest it is the file the
//! loader was pointed at or the layer's own label. **A trace that recorded
//! only the layers which contributed could not render this block at all**,
//! which is why the trace is not a map of contributors.
//!
//! # Nothing here prints
//!
//! [`Explanation`] is data. Its [`Display`](core::fmt::Display) writes into a
//! formatter the caller supplies, so where the block goes — a terminal, a
//! test's string, a transcript — is the caller's to decide. There is no
//! `println!` anywhere in this module, because D3's command surface is
//! [ADR-0015] D2's `/config` namespace and does not exist yet.
//!
//! # Three things this rendering says about D3's own example
//!
//! D3's block spells layer 2 `~/.zaru/config` where D1 names the file
//! `~/.zaru/config.toml`, and its second column mixes layer *labels*
//! (`built-in`, `flag`) with *sources* (`./zaru.toml`, `ZARU_MAX_ITER`).
//! Here the column is always the source, and a layer with no file falls back
//! to its own label — which reproduces D3's `built-in` and `flag` rows
//! exactly while making the rule uniform.
//!
//! The third is larger and is not a rendering question at all: **the block
//! shows a project file raising `runtime.max_iterations` from 5 to 8, and D6
//! forbids that.** See
//! `adr_0014_d3s_worked_example_is_refused_by_adr_0014_d6`. All three are
//! recorded on the record for the author rather than decided here.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::key::Key;
use crate::config::layer::Layer;
use crate::config::value::Value;
use core::fmt;

/// What one layer had to say about one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplanationRow {
    /// Which of D1's five layers.
    pub layer: Layer,
    /// What D3's second column shows — the source, or the layer's own label
    /// where the source has no name of its own.
    pub source: String,
    /// What this layer set, or `None` for D3's `(not set)`.
    pub value: Option<Value>,
    /// Whether this is the layer the effective value came from.
    ///
    /// **At most one row carries this, and it is the highest layer that set
    /// the key — not simply the highest layer.** The two differ whenever the
    /// top layer is unset, which is the ordinary case.
    pub effective: bool,
}

/// Where one key's value came from, across all five layers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The key that was explained.
    pub key: Key,
    /// What it resolved to, or `None` if no layer set it.
    pub value: Option<Value>,
    /// One row per layer, **highest first**, as D3's block prints them.
    pub rows: Vec<ExplanationRow>,
}

impl Explanation {
    /// The layer the effective value came from, if any layer set it.
    #[must_use]
    pub fn effective_layer(&self) -> Option<Layer> {
        self.rows
            .iter()
            .find(|row| row.effective)
            .map(|row| row.layer)
    }
}

/// How a value appears in D3's block.
///
/// A configuration value is not a secret: a credential-shaped one is refused
/// at load before any resolution exists, and a credential *reference* renders
/// as its alias, which ADR-0007 D2 makes local metadata that its own refusals
/// quote back.
fn render(value: &Value) -> String {
    match value {
        Value::Bool(flag) => flag.to_string(),
        Value::Integer(number) => number.to_string(),
        Value::Text(text) => text.clone(),
        Value::Credential(reference) => reference.to_string(),
        Value::Array(items) => {
            let rendered: Vec<String> = items.iter().map(render).collect();
            format!("[{}]", rendered.join(", "))
        }
        Value::Table(table) => {
            let rendered: Vec<String> = table
                .iter()
                .map(|(name, inner)| format!("{name} = {}", render(inner)))
                .collect();
            format!("{{{}}}", rendered.join(", "))
        }
    }
}

/// What D3 prints where a layer set nothing.
const NOT_SET: &str = "(not set)";

/// What D3 puts beside the layer the value came from.
const EFFECTIVE_MARKER: &str = "← effective";

impl fmt::Display for Explanation {
    /// D3's block, exactly.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(value) => writeln!(f, "{} = {}", self.key, render(value))?,
            None => writeln!(f, "{} {NOT_SET}", self.key)?,
        }

        let rendered: Vec<(&ExplanationRow, String)> = self
            .rows
            .iter()
            .map(|row| {
                let shown = row
                    .value
                    .as_ref()
                    .map_or_else(|| NOT_SET.to_owned(), render);
                (row, shown)
            })
            .collect();

        let source_width = rendered
            .iter()
            .map(|(row, _)| row.source.chars().count())
            .max()
            .unwrap_or(0);
        let value_width = rendered
            .iter()
            .map(|(_, shown)| shown.chars().count())
            .max()
            .unwrap_or(0);

        for (row, shown) in rendered {
            let line = format!(
                "  {}  {:source_width$}  {:value_width$}",
                row.layer.number(),
                row.source,
                shown,
            );
            if row.effective {
                writeln!(f, "{line}  {EFFECTIVE_MARKER}")?;
            } else {
                writeln!(f, "{}", line.trim_end())?;
            }
        }
        Ok(())
    }
}
