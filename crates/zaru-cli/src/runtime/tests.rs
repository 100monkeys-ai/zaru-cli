// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The runtime tiers' checks, clause by clause.
//!
//! Every check names the ADR-0001 clause it holds and the mutant that would
//! make it redden. Where a mutant is named it has been run, and its printed
//! sentence is quoted in the commit that carries the check.

use crate::runtime::tier::{Cortex, Engagement, Loop, Membrane, Network, Tier};

// ---------------------------------------------------------------------------
// D1 — the axis, its three tiers, and the four columns
// ---------------------------------------------------------------------------

/// [ADR-0001] D1's table, transcribed beside the code that answers it.
///
/// Columns in D1's own order: tier, Membrane, Loop, Cortex, Network.
///
/// **The limit of this transcription, stated rather than implied.** Both this
/// literal and [`Tier::engagement`] are transcriptions of the same record, and
/// the record lives in the cortex where a check with no network cannot read
/// it. So editing ADR-0001 D1 reddens this only if whoever edits it also edits
/// this table. What the check *does* hold is that the code cannot drift from
/// this transcription silently, and that the two live far enough apart —
/// different files, different shapes — that one is not a mirror of the other
/// ([Verification lessons] §11).
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
const D1: [(&str, &str, &str, &str, &str); 3] = [
    ("bare", "none", "none", "none", "model provider only"),
    (
        "contained",
        "local containers",
        "local",
        "local",
        "model provider only",
    ),
    (
        "linked",
        "local containers",
        "local, offloadable",
        "Nuclear Notes",
        "model provider + platform",
    ),
];

/// D1 names three tiers and this crate declares exactly those three, in the
/// record's own order and with the record's own spellings.
///
/// The mutant: renaming a variant's `as_str`, or reordering `Tier::ALL`.
#[test]
fn the_three_tiers_are_adr_0001_d1s_three_in_d1s_order() {
    let declared: Vec<&str> = Tier::ALL.iter().map(|tier| tier.as_str()).collect();
    let recorded: Vec<&str> = D1.iter().map(|row| row.0).collect();

    assert_eq!(
        declared, recorded,
        "ADR-0001 D1 names three tiers in this order and this crate does not declare them so",
    );

    // The round trip, so a spelling is a name a user can actually write.
    for tier in Tier::ALL {
        assert_eq!(
            Tier::named(tier.as_str()),
            Some(tier),
            "{tier} does not parse back from its own name",
        );
    }
    assert_eq!(
        Tier::named("Bare"),
        None,
        "a tier's name is D1's spelling exactly; accepting another is a second vocabulary",
    );
    assert_eq!(Tier::named(""), None);
}

/// Every cell of D1's table, per tier and per column.
///
/// The population is walked from [`Tier::ALL`] and looked up in [`D1`], never
/// the reverse, so a tier with no row is reported as missing rather than
/// silently skipped ([Verification lessons] §17). Every mismatch is reported
/// rather than the first, so one run says which cells are wrong.
///
/// The mutant: changing any cell in [`Tier::engagement`].
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_cell_of_d1s_table_is_what_the_record_prints() {
    let mut wrong: Vec<String> = Vec::new();

    for tier in Tier::ALL {
        let Some(row) = D1.iter().find(|row| row.0 == tier.as_str()) else {
            wrong.push(format!(
                "the tier {tier} has no row in ADR-0001 D1's table as transcribed here"
            ));
            continue;
        };

        // Destructured, so a fifth column on `Engagement` stops this check
        // compiling rather than travelling unasserted.
        let Engagement {
            membrane,
            r#loop,
            cortex,
            network,
        } = tier.engagement();

        for (column, found, recorded) in [
            ("Membrane", membrane.as_str(), row.1),
            ("Loop", r#loop.as_str(), row.2),
            ("Cortex", cortex.as_str(), row.3),
            ("Network", network.as_str(), row.4),
        ] {
            if found != recorded {
                wrong.push(format!(
                    "{tier} / {column}: the code says {found:?} and ADR-0001 D1 says {recorded:?}"
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} of {} cells do not match ADR-0001 D1: {wrong:#?}",
        wrong.len(),
        Tier::ALL.len() * 4,
    );
}

/// Each column's variants are exactly the distinct cells D1's table holds for
/// it, so no column carries a value the record never prints.
///
/// This is the arm that stops [`every_cell_of_d1s_table_is_what_the_record_prints`]
/// being satisfied by an enum with an unused fourth variant nobody reaches.
///
/// The mutant: adding a variant to any column enum.
#[test]
fn each_column_holds_exactly_the_distinct_cells_d1_prints_for_it() {
    fn distinct(cells: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
        let mut seen: Vec<&'static str> = Vec::new();
        for cell in cells {
            if !seen.contains(&cell) {
                seen.push(cell);
            }
        }
        seen.sort_unstable();
        seen
    }

    for (column, declared, recorded) in [
        (
            "Membrane",
            distinct(Membrane::ALL.iter().map(|cell| cell.as_str())),
            distinct(D1.iter().map(|row| row.1)),
        ),
        (
            "Loop",
            distinct(Loop::ALL.iter().map(|cell| cell.as_str())),
            distinct(D1.iter().map(|row| row.2)),
        ),
        (
            "Cortex",
            distinct(Cortex::ALL.iter().map(|cell| cell.as_str())),
            distinct(D1.iter().map(|row| row.3)),
        ),
        (
            "Network",
            distinct(Network::ALL.iter().map(|cell| cell.as_str())),
            distinct(D1.iter().map(|row| row.4)),
        ),
    ] {
        assert_eq!(
            declared, recorded,
            "the {column} column declares values ADR-0001 D1 does not print, or is missing one \
             that it does",
        );
    }
}

/// `has_membrane` is D1's Membrane column, not a second statement of it.
///
/// It was a second match over the same three tiers until 2026-09-05, which is
/// the one-rule-in-two-places shape [Verification lessons] §27 names —
/// [ADR-0011] D2's not-a-sandbox line is emitted on this answer, so the two
/// could not be allowed to disagree.
///
/// The mutant: changing `bare`'s Membrane cell in [`Tier::engagement`], which
/// reddens this **and** the table check above — which is the point, because a
/// derivation means there is only one place to change.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn having_a_membrane_is_read_off_d1s_membrane_column() {
    for tier in Tier::ALL {
        let from_the_column = tier.engagement().membrane != Membrane::None;
        assert_eq!(
            tier.has_membrane(),
            from_the_column,
            "{tier}: has_membrane() and D1's Membrane column disagree, so the rule is in two \
             places",
        );
    }

    // The absolute values too, so the check is not satisfied by an
    // implementation where both sides are wrong together
    // (Verification lessons §13).
    assert!(
        !Tier::Bare.has_membrane(),
        "ADR-0011 D2 gives `bare` no enforcement at all, which is why its not-a-sandbox line \
         exists",
    );
    assert!(Tier::Contained.has_membrane() && Tier::Linked.has_membrane());
}
