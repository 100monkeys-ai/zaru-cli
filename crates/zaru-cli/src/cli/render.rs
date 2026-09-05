// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One projection per datum, as lines, rendered by whoever calls it.
//!
//! # Why a `Vec<String>` and not a `Display`
//!
//! [ADR-0015] D2's two entry points are one operation, so `/runtime` inside a
//! session and `zaru runtime` outside one must show the same thing. If the
//! binary composed its output with `println!` and the terminal composed its
//! own, there would be two statements of one datum and they would diverge —
//! which is the shape the `Layer` ruling of 2026-09-04 removed from this
//! crate, one level up.
//!
//! So each datum has exactly one projection here, it returns lines, and the
//! caller decides where they go: the binary writes them to standard output,
//! and the composer — when [ADR-0005]'s terminal exists — hands them to a
//! frame. **Nothing in this module prints.**
//!
//! # Nothing here composes a sentence a record already owns
//!
//! Every string a reader sees comes from the datum: [ADR-0001] D1's cells out
//! of `Engagement`, [ADR-0014] D3's `(not set)` and `← effective` out of
//! `config::explain`'s own constants, [ADR-0007] D8's apex marking out of
//! `Reach::marking`, a layer's name out of `Layer::label`. What this module
//! adds is column widths and the order of the lines.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::config::Explanation;
use crate::config::explain::NOT_SET;
use crate::providers::{ModelTable, ResolvedModel};
use crate::runtime::Runtime;

/// [ADR-0014] D3's block, as lines.
///
/// The block itself is [`Explanation`]'s own `Display`, which that module
/// wrote to reproduce D3 exactly. This splits it rather than re-rendering it,
/// so there is no second formatter for the one thing D3 spells out.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn explanation(explanation: &Explanation) -> Vec<String> {
    explanation.to_string().lines().map(str::to_owned).collect()
}

/// [ADR-0001] D2's datum, as lines.
///
/// D2: "`/runtime` prints the current tier and **what changing it would
/// alter**." The second half is arithmetic over D1's own table, computed by
/// [`Runtime`]; this walks it. A tier that differs in no column is rendered as
/// saying so, because [`Runtime::would_change`] carries every other tier
/// including one whose diff is empty, and dropping it would answer a
/// different question from the one D2 asks.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
#[must_use]
pub fn runtime(datum: &Runtime) -> Vec<String> {
    let engagement = datum.engagement;
    let mut lines = vec![
        format!(
            "{} = {} (from {})",
            crate::runtime::KEY,
            datum.tier,
            datum.supplied_by.label()
        ),
        format!("  membrane  {}", engagement.membrane.as_str()),
        format!("  loop      {}", engagement.r#loop.as_str()),
        format!("  cortex    {}", engagement.cortex.as_str()),
        format!("  network   {}", engagement.network.as_str()),
    ];

    for (other, differences) in &datum.would_change {
        lines.push(String::new());
        if differences.is_empty() {
            lines.push(format!("changing to {other} would alter nothing"));
            continue;
        }
        lines.push(format!("changing to {other} would alter"));
        let width = differences
            .iter()
            .map(|difference| difference.column.chars().count())
            .max()
            .unwrap_or(0);
        for difference in differences {
            lines.push(format!(
                "  {:width$}  {} -> {}",
                difference.column, difference.here, difference.there
            ));
        }
    }

    lines
}

/// [ADR-0012] D4's listing, as lines.
///
/// D4: "`zaru models` prints each alias, what it resolved to, and **which
/// layer supplied it**." An alias no layer set is `(not set)` — D3's own
/// spelling, taken from that module's constant rather than retyped, because
/// ADR-0012's own documentation says this listing renders an unresolved alias
/// "the way ADR-0014 D3's block renders `(not set)`".
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[must_use]
pub fn models(table: &ModelTable) -> Vec<String> {
    let rows: Vec<(String, String, &str)> = table
        .rows()
        .map(|(alias, resolved)| match resolved {
            ResolvedModel::Resolved { model, supplied_by } => (
                alias.to_string(),
                model.as_str().to_owned(),
                supplied_by.label(),
            ),
            ResolvedModel::Unresolved => (alias.to_string(), NOT_SET.to_owned(), ""),
        })
        .collect();

    let alias_width = rows
        .iter()
        .map(|(alias, _, _)| alias.chars().count())
        .max()
        .unwrap_or(0);
    let model_width = rows
        .iter()
        .map(|(_, model, _)| model.chars().count())
        .max()
        .unwrap_or(0);

    rows.iter()
        .map(|(alias, model, layer)| {
            format!("  {alias:alias_width$}  {model:model_width$}  {layer}")
                .trim_end()
                .to_owned()
        })
        .collect()
}
