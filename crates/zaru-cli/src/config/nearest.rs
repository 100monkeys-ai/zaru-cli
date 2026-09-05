// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0014 D5's "naming the nearest match", as one function.
//!
//! # Why this is a module rather than a method
//!
//! D5 says an unknown key is "an error at load, naming the nearest match", and
//! it names no metric. A coordinator ruling of 2026-09-04 chose Levenshtein
//! edit distance over characters with ties broken lexically, and that choice
//! was written inside [`Schema`](crate::config::Schema) because the schema was
//! the only thing that had unknown names to place.
//!
//! It is not any more. [`crate::config::environment`] places an unknown
//! `ZARU_*` variable, and [`crate::cli`] places an unknown subcommand, verb
//! or flag against [ADR-0015] D2's namespaces — three vocabularies, one
//! question. A second edit distance anywhere in this crate would be the
//! rule-in-two-places the `Layer` ruling of 2026-09-04 removed from this
//! workspace, and it would diverge in the way that is hardest to see: two
//! metrics agree on almost every input and disagree on the one a user
//! actually typed.
//!
//! So the metric lives here, `pub(crate)`, and the three callers pass their
//! own candidates. **Lifted 2026-09-05 under a delegated coordinator ruling**,
//! open to Jeshua's veto; the behaviour is unchanged and
//! `the_suggestion_is_the_nearest_declared_key_and_not_merely_a_declared_one`
//! still holds it.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

/// The candidate nearest to `offered`, or `None` when there are none.
///
/// **Ties are broken by the order the candidates arrive in**, because the
/// comparison is strictly-less-than: the first of several equally near
/// candidates wins. `Schema::keys` iterates a `BTreeMap`, so for a schema that
/// is lexical order and the answer does not depend on insertion order; a
/// caller whose candidates are a fixed list gets that list's own order, which
/// is the record's order for [ADR-0015] D2's namespaces.
///
/// D5 names no distance threshold, so a name resembling nothing still gets the
/// nearest one.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub(crate) fn nearest<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    offered: &str,
) -> Option<&'a str> {
    let mut best: Option<(usize, &'a str)> = None;
    for candidate in candidates {
        let distance = edit_distance(offered, candidate);
        if best.is_none_or(|(shortest, _)| distance < shortest) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// Levenshtein edit distance between two strings, over characters.
///
/// Two rows rather than a full matrix; the strings here are configuration
/// keys and command names, so the cost is irrelevant and the shorter code is
/// the readable one.
fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];

    for (row, left_char) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, right_char) in right.iter().enumerate() {
            let substitution = usize::from(left_char != *right_char);
            current[column + 1] = (previous[column] + substitution)
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        core::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}
