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

// ---------------------------------------------------------------------------
// D2 — the key, the resolution, and immutability for the life of a session
// ---------------------------------------------------------------------------

use crate::config::fixtures::{at, document, schema};
use crate::config::{ConfigRefused, Layer, Resolution, Value};
use crate::runtime::resolve::{KEY, ResolvedTier, TierRefused, key};

/// One contribution setting the tier at a named layer.
fn tier_at(layer: Layer, tier: &str) -> crate::config::Contribution {
    at(
        layer,
        layer.label(),
        document([(KEY, Value::Text(tier.to_owned()))]),
    )
}

/// D2's tier resolves from each layer that may set it, and the resolution
/// names which one did.
///
/// The four layers ADR-0014 D6 leaves to the user: built-in, user config,
/// environment, flag. **Every expected value is a literal this check owns** —
/// the layer it planted at and the tier it wrote — rather than one the
/// resolver computed ([Verification lessons] §10).
///
/// The mutant: reading `Resolution::get` and reporting a fixed layer, which
/// passes the value arm and reddens on every layer but one.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_tier_resolves_from_every_layer_a_user_may_set_it_in() {
    let mut wrong: Vec<String> = Vec::new();

    for layer in [Layer::BuiltIn, Layer::User, Layer::Environment, Layer::Flag] {
        for planted in Tier::ALL {
            let resolved = Resolution::resolve(&schema(), vec![tier_at(layer, planted.as_str())])
                .expect("a tier at a layer the user owns resolves");
            let taken = ResolvedTier::from_configuration(&resolved)
                .expect("the tier was set, so it resolves");

            if taken.tier() != planted {
                wrong.push(format!(
                    "{}: resolved {} where {planted} was planted",
                    layer.label(),
                    taken.tier(),
                ));
            }
            if taken.supplied_by() != layer {
                wrong.push(format!(
                    "{}: reported {} as the supplying layer",
                    layer.label(),
                    taken.supplied_by().label(),
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} of 12 (layer, tier) pairs did not resolve to what was planted: {wrong:#?}",
        wrong.len(),
    );
}

/// The higher layer wins, which is ADR-0014 D1 reaching the tier.
///
/// Not redundant with the check above: that one plants one layer at a time and
/// would pass against a resolver that ignored precedence entirely.
///
/// The mutant: taking the lowest layer that set the key.
#[test]
fn a_higher_layer_overrides_a_lower_one_for_the_tier() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            tier_at(Layer::BuiltIn, "bare"),
            tier_at(Layer::User, "contained"),
            tier_at(Layer::Flag, "linked"),
        ],
    )
    .expect("three layers the user owns resolve");

    let taken = ResolvedTier::from_configuration(&resolved).expect("a tier was set");
    assert_eq!(taken.tier(), Tier::Linked);
    assert_eq!(taken.supplied_by(), Layer::Flag);
}

/// **ADR-0001 D2 against ADR-0014 D6, as D2 reads once corrected.**
///
/// D2 said "Config key `runtime` in `zaru.toml`" — a project file — while
/// [ADR-0014] D6 says a project may not "move the runtime tier upward" and
/// that record's implementation refuses the project layer the key outright.
/// Under Jeshua's directive of 2026-09-05 D2 is corrected to read that the key
/// is `runtime.tier`, "settable at the user, environment and flag layers and
/// never by a project", so this check asserts the corrected sentence: **both
/// arms, because the refusal alone is satisfied by an implementation that
/// refuses everything** ([Verification lessons] §13, and the shape ADR-0014's
/// own Status tracking says was measured rather than argued).
///
/// The mutants: making D6's refusal arm a no-op (the first arm reddens), and
/// refusing the key at every layer (the second reddens).
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_project_may_not_set_the_tier_and_the_user_layer_may() {
    // The arm D6 states.
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            tier_at(Layer::BuiltIn, "bare"),
            tier_at(Layer::Project, "contained"),
        ],
    )
    .expect_err("ADR-0014 D6 refuses a project setting the runtime tier");

    let ConfigRefused::ProjectMayNotSet { key: refused, .. } = &refusal else {
        panic!("expected D6's escalation refusal, got {refusal:?}");
    };
    assert_eq!(
        refused.as_str(),
        KEY,
        "the refusal names a key other than the one ADR-0001 owns",
    );
    println!("{refusal}");

    // The arm that makes the first one mean something: the user's own layer is
    // the grant D6 protects, and it may set the tier.
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            tier_at(Layer::BuiltIn, "bare"),
            tier_at(Layer::User, "contained"),
        ],
    )
    .expect("D6 constrains the project layer and not the user's own grant");
    let taken = ResolvedTier::from_configuration(&resolved).expect("a tier was set");
    assert_eq!(taken.tier(), Tier::Contained);
    assert_eq!(taken.supplied_by(), Layer::User);
}

/// An unset tier is refused, **not defaulted**.
///
/// ADR-0001 names no fallback tier. Choosing one here would decide which
/// membrane a user gets when they said nothing, which is a security posture
/// and on the human side of the autonomy boundary.
///
/// The mutant: returning `Tier::Bare` when no layer set the key — which would
/// hand a user with a typo'd config the tier with no membrane at all.
#[test]
fn a_tier_no_layer_set_is_refused_rather_than_defaulted() {
    let resolved = Resolution::resolve(&schema(), Vec::new()).expect("an empty fold resolves");
    let refusal = ResolvedTier::from_configuration(&resolved)
        .expect_err("no layer set the tier, so there is none");

    assert_eq!(refusal, TierRefused::NotSet { key: key() });
    let rendered = refusal.to_string();
    assert!(
        rendered.contains(KEY),
        "the refusal does not name the key: {rendered}",
    );
    println!("{rendered}");
}

/// A value naming no tier is refused, and the refusal names the layer that
/// offered it and every spelling that would have worked.
///
/// The layer matters: D2's key is settable in four places and a reader who is
/// not told which file to edit is in exactly the position ADR-0014 D3 exists
/// to get them out of.
///
/// The mutant: dropping the layer from the refusal, or listing the tiers from
/// a second hand-typed list rather than from `Tier::ALL`.
#[test]
fn a_value_naming_no_tier_is_refused_naming_the_layer_and_the_three() {
    let resolved = Resolution::resolve(&schema(), vec![tier_at(Layer::Environment, "sandboxed")])
        .expect("the fold takes any text for this key; naming a tier is this module's check");

    let refusal = ResolvedTier::from_configuration(&resolved)
        .expect_err("`sandboxed` names no tier ADR-0001 D1 defines");

    let TierRefused::NoSuchTier { layer, offered, .. } = &refusal else {
        panic!("expected NoSuchTier, got {refusal:?}");
    };
    assert_eq!(*layer, Layer::Environment);
    assert_eq!(offered, "sandboxed");

    let rendered = refusal.to_string();
    for tier in Tier::ALL {
        assert!(
            rendered.contains(tier.as_str()),
            "the refusal does not offer {tier} as one of the three: {rendered}",
        );
    }
    assert!(rendered.contains("environment"), "{rendered}");
    println!("{rendered}");
}

/// The key is declared once, and the declaration is the one ADR-0014 D6's
/// ceiling reads.
///
/// The mutant: declaring the key `Free` to projects, which reddens the pin
/// check above; or spelling `KEY` as D2's bare `runtime`, which reddens here
/// because the fixture manifest's `[runtime]` table would then collide with a
/// key rather than nest under one.
#[test]
fn the_key_is_runtime_tier_and_the_field_refuses_the_project_layer() {
    assert_eq!(key().as_str(), KEY);
    assert_eq!(KEY, "runtime.tier");

    let field = crate::runtime::field();
    assert!(
        matches!(field.project, crate::config::ProjectPolicy::Refused { .. }),
        "ADR-0014 D6 makes the tier a key the project layer may not set, and this declaration \
         does not say so",
    );

    let crate::config::ProjectPolicy::Refused { reason } = &field.project else {
        unreachable!("just asserted")
    };
    assert!(
        reason.contains("ADR-0001 D2"),
        "D6 requires the error name the key and the reason, and ADR-0016 D2 requires the reader \
         be able to act on it: {reason}",
    );
}
