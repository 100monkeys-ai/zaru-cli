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

// ---------------------------------------------------------------------------
// D3 — iteration defaults, per tier and per provider
// ---------------------------------------------------------------------------

use crate::runtime::defaults::{Inference, InferenceRefused, Placement, ceiling, iterations};

/// [ADR-0001] D3's table, transcribed beside the code that answers it.
///
/// `(tier, inference, placement, iterations)`, where `None` is a cell the
/// record leaves empty because that tier does not offload. Twelve rows,
/// because the axes are three by two by two and **every cell is written out**
/// — a table with only the eight populated rows could not tell a missing cell
/// from an unavailable one.
///
/// The same limit as [`D1`] applies and for the same reason: this literal and
/// [`iterations`] are two transcriptions of one record, so editing ADR-0001 D3
/// reddens this only if whoever edits it also edits the literal. What it holds
/// is that the code cannot drift from the transcription silently.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
const D3: [(Tier, Inference, Placement, Option<u32>); 12] = [
    // | bare | 1 | 1 |
    (Tier::Bare, Inference::Local, Placement::Local, Some(1)),
    (Tier::Bare, Inference::Frontier, Placement::Local, Some(1)),
    (Tier::Bare, Inference::Local, Placement::Offloaded, None),
    (Tier::Bare, Inference::Frontier, Placement::Offloaded, None),
    // | contained | 3 | 5 |
    (Tier::Contained, Inference::Local, Placement::Local, Some(3)),
    (
        Tier::Contained,
        Inference::Frontier,
        Placement::Local,
        Some(5),
    ),
    (
        Tier::Contained,
        Inference::Local,
        Placement::Offloaded,
        None,
    ),
    (
        Tier::Contained,
        Inference::Frontier,
        Placement::Offloaded,
        None,
    ),
    // | linked | 3 local, 8 offloaded | 5 local, 12 offloaded |
    (Tier::Linked, Inference::Local, Placement::Local, Some(3)),
    (
        Tier::Linked,
        Inference::Local,
        Placement::Offloaded,
        Some(8),
    ),
    (Tier::Linked, Inference::Frontier, Placement::Local, Some(5)),
    (
        Tier::Linked,
        Inference::Frontier,
        Placement::Offloaded,
        Some(12),
    ),
];

/// Every cell of D3's table, and every cell of the table is reached.
///
/// The population is the **cross product of the three axes**, walked from
/// `Tier::ALL × Inference::ALL × Placement::ALL`, and each triple is looked up
/// in [`D3`] — never the reverse. So a triple with no row is reported as
/// missing rather than skipped ([Verification lessons] §17), and a fourth tier
/// or a third value on either other axis is caught here as well as by the
/// compiler.
///
/// The mutant: changing any number in [`iterations`].
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_cell_of_d3s_table_is_the_number_the_record_prints() {
    let mut wrong: Vec<String> = Vec::new();
    let mut reached = 0usize;

    for tier in Tier::ALL {
        for inference in Inference::ALL {
            for placement in Placement::ALL {
                let Some(row) = D3
                    .iter()
                    .find(|row| row.0 == tier && row.1 == inference && row.2 == placement)
                else {
                    wrong.push(format!(
                        "({tier}, {inference}, {placement}) has no row in ADR-0001 D3's table as \
                         transcribed here"
                    ));
                    continue;
                };
                reached += 1;

                let found = iterations(tier, inference, placement);
                if found != row.3 {
                    wrong.push(format!(
                        "({tier}, {inference}, {placement}): the code says {found:?} and \
                         ADR-0001 D3 says {:?}",
                        row.3
                    ));
                }
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} of {reached} cells do not match ADR-0001 D3: {wrong:#?}",
        wrong.len(),
    );
    assert_eq!(
        reached,
        D3.len(),
        "the cross product of the three axes is not the size of D3's transcribed table, so one \
         of them has a value the table does not cover",
    );
}

/// The cells with no number are exactly the tiers D1's Loop column says cannot
/// offload.
///
/// **Two transcriptions of two different tables in the same record, held
/// against each other.** D1's Loop column and D3's empty cells are written
/// independently — `Tier::engagement` and `iterations` — so neither is derived
/// from the other and this is not a mirror ([Verification lessons] §11).
/// Deriving one would have made the comparison a tautology.
///
/// The mutant: giving `contained` an offloaded ceiling in [`iterations`], or
/// changing its Loop cell to `LocalOffloadable`. Either reddens, and that is
/// the point — the record cannot be half-changed.
#[test]
fn the_cells_with_no_ceiling_are_the_tiers_d1_says_cannot_offload() {
    for tier in Tier::ALL {
        let offloadable = tier.engagement().r#loop == Loop::LocalOffloadable;

        for inference in Inference::ALL {
            let offloaded = iterations(tier, inference, Placement::Offloaded);
            assert_eq!(
                offloaded.is_some(),
                offloadable,
                "{tier}: D1's Loop column says {:?} and D3's offloaded cell for {inference} says \
                 {offloaded:?}; the two tables in one record disagree",
                tier.engagement().r#loop.as_str(),
            );

            assert!(
                iterations(tier, inference, Placement::Local).is_some(),
                "{tier} has no local ceiling for {inference}, and every tier runs locally",
            );
        }
    }

    // The absolute value, so the check is not satisfied by both sides being
    // wrong together (Verification lessons §13).
    assert!(
        iterations(Tier::Linked, Inference::Local, Placement::Offloaded).is_some(),
        "`linked` is the tier that offloads, and D3 gives it 8 and 12",
    );
    assert!(
        iterations(Tier::Contained, Inference::Local, Placement::Offloaded).is_none(),
        "nothing offloads at `contained`; D1's Loop column for it is \"local\"",
    );
}

/// D3: "`bare` has no loop, so its iteration count is one by definition."
///
/// The only cell in the table with a stated derivation, so it is asserted as
/// the record states it rather than as two more numbers in the table above.
///
/// The mutant: giving `bare` 2 on either column.
#[test]
fn bare_is_one_iteration_on_every_column_by_d3s_own_definition() {
    for inference in Inference::ALL {
        assert_eq!(
            iterations(Tier::Bare, inference, Placement::Local),
            Some(1),
            "ADR-0001 D3: `bare` has no loop, so its iteration count is one by definition — \
             there is no validator to refine against",
        );
    }
    assert_eq!(
        Tier::Bare.engagement().r#loop,
        Loop::None,
        "D3's one-by-definition argument rests on D1's Loop column, and it does not say `none`",
    );
}

/// Every cell reaches `zaru-core` as a `Ceiling`, and none of them is the zero
/// that boundary refuses.
///
/// `Ceiling::new` refuses zero, so a table cell of zero would panic at the
/// conversion rather than being caught. This asserts the conversion succeeds
/// for every populated cell, which is the property that makes the `expect` in
/// [`ceiling`] honest rather than hopeful.
///
/// The mutant: a zero in [`iterations`], which turns this from a pass into a
/// panic naming the cell.
#[test]
fn every_populated_cell_converts_to_a_ceiling_zaru_core_accepts() {
    for tier in Tier::ALL {
        for inference in Inference::ALL {
            for placement in Placement::ALL {
                match (
                    iterations(tier, inference, placement),
                    ceiling(tier, inference, placement),
                ) {
                    (Some(count), Some(taken)) => assert_eq!(
                        taken.get(),
                        count,
                        "({tier}, {inference}, {placement}): the ceiling handed to zaru-core is \
                         not the number D3 prints",
                    ),
                    (None, None) => {}
                    (count, taken) => panic!(
                        "({tier}, {inference}, {placement}): the table says {count:?} and the \
                         ceiling says {}",
                        if taken.is_some() { "a ceiling" } else { "none" },
                    ),
                }
            }
        }
    }
}

/// D3's column is **read** from configuration, not guessed.
///
/// Under Jeshua's directive of 2026-09-05, as amended the same day, the axis is
/// declared per alias at `inference.<alias>` — **a sibling of `model.<alias>`
/// rather than a child**, because the nested spelling would make one key both a
/// value and a table and ADR-0014 D2's merge would resolve that by write order.
/// The key belongs to [ADR-0012] and this module declares no `Field` for it —
/// only the reading is ADR-0001 D3's.
///
/// The mutant: defaulting an unset key to `Local`, which would silently give
/// every alias `contained`'s 3 rather than its 5.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
#[test]
fn the_inference_axis_is_read_from_configuration_for_an_alias() {
    let inference_key = crate::config::Key::new("inference.default").expect("a key");
    assert_eq!(
        Inference::key_for("default").expect("a key").as_str(),
        inference_key.as_str(),
        "the key this module reads is not `inference.<alias>`",
    );

    let with_the_key = schema().with(
        inference_key,
        crate::config::Field::free(crate::config::FieldKind::Text),
    );

    for planted in Inference::ALL {
        let resolved = Resolution::resolve(
            &with_the_key,
            vec![at(
                Layer::User,
                "user config",
                document([(
                    "inference.default",
                    Value::Text(planted.as_str().to_owned()),
                )]),
            )],
        )
        .expect("a declared key resolves");

        assert_eq!(
            Inference::resolved_for(&resolved, "default").expect("the key was set"),
            planted,
            "the axis read back is not the one the check planted",
        );
    }

    // Unset is refused rather than defaulted: a default here would be this
    // record choosing another record's configuration.
    let empty = Resolution::resolve(&with_the_key, Vec::new()).expect("an empty fold resolves");
    let refusal = Inference::resolved_for(&empty, "default")
        .expect_err("no layer set the key, so there is no column to read");
    assert!(
        matches!(refusal, InferenceRefused::NotSet { .. }),
        "{refusal:?}"
    );
    println!("{refusal}");

    // And a value naming neither column is refused naming both.
    let wrong = Resolution::resolve(
        &with_the_key,
        vec![at(
            Layer::Flag,
            "flag",
            document([("inference.default", Value::Text("cloud".to_owned()))]),
        )],
    )
    .expect("the fold takes any text for this key");
    let refusal = Inference::resolved_for(&wrong, "default").expect_err("`cloud` names no column");
    let rendered = refusal.to_string();
    for inference in Inference::ALL {
        assert!(
            rendered.contains(inference.as_str()),
            "the refusal does not offer {inference}: {rendered}",
        );
    }
    println!("{rendered}");
}

/// Placement is local unless ADR-0012 D3's `aegis` kind resolved.
///
/// The directive of 2026-09-05 stated exactly. The one string this module
/// spells from that record is `aegis`, and a `ProviderKind` type will convert
/// into this rather than this growing a second list of kinds.
///
/// The mutant: treating any kind as offloading, which reddens on the three
/// that are not `aegis`.
#[test]
fn work_is_local_unless_the_aegis_provider_kind_resolved() {
    assert_eq!(
        Placement::for_resolved_provider_kind("aegis"),
        Placement::Offloaded,
    );
    for local in ["anthropic", "openai-compatible", "ollama", "", "AEGIS"] {
        assert_eq!(
            Placement::for_resolved_provider_kind(local),
            Placement::Local,
            "the provider kind {local:?} was read as offloading, and only `aegis` offloads",
        );
    }
    assert_eq!(Placement::OFFLOADING_PROVIDER_KIND, "aegis");
}

// ---------------------------------------------------------------------------
// D2 — the status-line and `/runtime` datum, with no renderer
// ---------------------------------------------------------------------------

use crate::runtime::datum::{Difference, Runtime};

/// D2's datum carries the tier, D1's whole row for it, and every other tier.
///
/// Every other tier appears, including one that differed in no column — which
/// would render as "nothing would change" and is a different answer from a
/// tier missing from the list ([Verification lessons] §8).
///
/// The mutant: filtering the list to tiers that differ, which drops nothing
/// today and would drop a tier silently the day D1 gains an identical row.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_datum_carries_the_tier_its_row_and_every_other_tier() {
    for tier in Tier::ALL {
        let datum = Runtime::of(ResolvedTier::supplied(tier, Layer::Flag));

        assert_eq!(datum.tier, tier);
        assert_eq!(datum.supplied_by, Layer::Flag);
        assert_eq!(
            datum.engagement,
            tier.engagement(),
            "the datum's row is not D1's row for its own tier",
        );

        let named: Vec<Tier> = datum.would_change.iter().map(|(other, _)| *other).collect();
        let expected: Vec<Tier> = Tier::ALL
            .into_iter()
            .filter(|other| *other != tier)
            .collect();
        assert_eq!(
            named, expected,
            "the datum at {tier} does not name every other tier, in Tier::ALL's order",
        );
    }
}

/// "What changing it would alter" is the diff of D1's rows, cell by cell.
///
/// The expected differences are computed here from [`D1`] — the check's own
/// transcription — rather than from `Tier::engagement`, so neither arm of the
/// comparison travels through the code under test ([Verification lessons]
/// §11). The mutant that changes a cell in `engagement` reddens this as well
/// as the table check.
///
/// The mutant: comparing tiers rather than columns, which reports every column
/// as different for every pair.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn what_changing_the_tier_would_alter_is_the_diff_of_d1s_rows() {
    for here in Tier::ALL {
        let datum = Runtime::of(ResolvedTier::supplied(here, Layer::BuiltIn));
        let mine = D1
            .iter()
            .find(|row| row.0 == here.as_str())
            .expect("every tier has a row");

        for there in Tier::ALL {
            if there == here {
                assert!(
                    datum.would_change_to(there).is_none(),
                    "the datum offers a diff against its own tier",
                );
                continue;
            }
            let theirs = D1
                .iter()
                .find(|row| row.0 == there.as_str())
                .expect("every tier has a row");

            let expected: Vec<Difference> = [
                ("Membrane", mine.1, theirs.1),
                ("Loop", mine.2, theirs.2),
                ("Cortex", mine.3, theirs.3),
                ("Network", mine.4, theirs.4),
            ]
            .into_iter()
            .filter(|(_, here, there)| here != there)
            .map(|(column, here, there)| Difference {
                column,
                here,
                there,
            })
            .collect();

            assert_eq!(
                datum.would_change_to(there).expect("every other tier"),
                expected,
                "moving from {here} to {there} does not alter what ADR-0001 D1's two rows differ \
                 in",
            );
        }
    }
}

/// The one worked case, spelled out, so the check above is not satisfied by
/// two identically wrong derivations.
///
/// From `bare` to `contained`, D1 changes Membrane, Loop and Cortex and leaves
/// Network alone — both rows say "model provider only". That last is the cell
/// that discriminates: a diff that reported all four would pass a check
/// comparing two computed lists and fails here.
#[test]
fn moving_from_bare_to_contained_alters_three_of_d1s_four_columns() {
    let datum = Runtime::of(ResolvedTier::supplied(Tier::Bare, Layer::User));
    let moving = datum
        .would_change_to(Tier::Contained)
        .expect("contained is another tier");

    let columns: Vec<&str> = moving.iter().map(|change| change.column).collect();
    assert_eq!(
        columns,
        vec!["Membrane", "Loop", "Cortex"],
        "ADR-0001 D1 gives `bare` and `contained` the same Network cell, so a diff naming it is \
         wrong and a diff missing one of the other three is too",
    );

    // Destructured, so a fourth field on `Difference` stops this compiling.
    let Difference {
        column,
        here,
        there,
    } = moving[0];
    assert_eq!(
        (column, here, there),
        ("Membrane", "none", "local containers")
    );

    // And the whole-row move, which alters all four.
    let all_four = datum
        .would_change_to(Tier::Linked)
        .expect("linked is another tier");
    assert_eq!(
        all_four.len(),
        4,
        "every one of D1's columns differs between `bare` and `linked`",
    );
}

/// Nothing in the runtime module prints, because D2's two surfaces are not
/// this module's to build.
///
/// The status line is `zaru-tui`'s and `/runtime` is ADR-0015 D2's namespace;
/// neither exists. A `println!` here would be this module deciding where the
/// block goes, which is the caller's to decide — the same shape ADR-0014 D3's
/// `Explanation` takes.
///
/// The mutant: a `println!` anywhere in the module, which this reports by path
/// and line.
#[test]
fn nothing_in_the_runtime_module_prints() {
    let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("runtime");
    let mut printing: Vec<String> = Vec::new();
    let mut scanned = 0usize;

    for entry in std::fs::read_dir(&module).expect("the runtime module is a directory") {
        let path = entry.expect("an entry").path();
        if path.extension().is_none_or(|kind| kind != "rs") {
            continue;
        }
        // This file is the checks and prints refusals on purpose.
        if path.file_name().is_some_and(|name| name == "tests.rs") {
            continue;
        }
        scanned += 1;
        let source = std::fs::read_to_string(&path).expect("the source is readable");
        for (number, line) in source.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for macro_name in ["println!", "eprintln!", "print!", "eprint!"] {
                if code.contains(macro_name) {
                    printing.push(format!("{}:{}: {macro_name}", path.display(), number + 1));
                }
            }
        }
    }

    assert!(
        scanned >= 3,
        "the scan found {scanned} source files under {}, so it asserted almost nothing",
        module.display(),
    );
    assert!(
        printing.is_empty(),
        "{} line(s) in the runtime module print. ADR-0001 D2's status line is `zaru-tui`'s and \
         `/runtime` is ADR-0015 D2's; this module builds the datum and no renderer: {printing:#?}",
        printing.len(),
    );
}

/// Why the inference axis is a **sibling** of `model.<alias>` and not a child.
///
/// The directive of 2026-09-05 first put it at `model.<alias>.inference` and
/// was amended the same day because `provider-aliases` measured that the
/// nested spelling makes one key both a value and a table. This check pins the
/// measurement, so the reason for the spelling survives in code rather than
/// only in a ruling somebody has to find.
///
/// **Measured here, and it is worse than "one order drops the setting":
/// both orders drop one.** `Table::insert_path` replaces a non-table sitting
/// where a table is needed, so writing the model id first loses the id when
/// the nested key arrives, and writing the nested key first loses the
/// inference when the id arrives. Which setting is lost depends on write
/// order, and nothing reports either loss — [ADR-0014] D5's silent-typo
/// failure arriving through the schema's shape rather than through a typo.
///
/// The mutant: none is needed on the product, because this is a measurement of
/// a shape rather than a rule. What would redden it is `insert_path` learning
/// to refuse a collision, which would be a change to ADR-0014's own merge.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn the_inference_axis_is_a_sibling_because_the_nested_spelling_loses_a_setting() {
    let model = crate::config::Key::new("model.default").expect("a key");
    let nested = crate::config::Key::new("model.default.inference").expect("a key");
    let sibling = Inference::key_for("default").expect("a key");

    assert_eq!(
        sibling.as_str(),
        "inference.default",
        "the axis is read from a sibling of `model.<alias>`",
    );

    // The model id first, then the nested axis: the id is gone.
    let mut first = crate::config::Table::new();
    first.insert_path(&model, Value::Text("a-model".to_owned()));
    first.insert_path(&nested, Value::Text("frontier".to_owned()));
    assert_eq!(
        first.get_path(&model).and_then(Value::as_text),
        None,
        "the nested spelling kept the model id, so this check no longer measures the collision \
         it was written for",
    );

    // The nested axis first, then the model id: the axis is gone.
    let mut second = crate::config::Table::new();
    second.insert_path(&nested, Value::Text("frontier".to_owned()));
    second.insert_path(&model, Value::Text("a-model".to_owned()));
    assert_eq!(
        second.get_path(&nested).and_then(Value::as_text),
        None,
        "the nested spelling kept the inference axis under the other write order",
    );

    // The sibling spelling keeps both, under either order.
    let mut both = crate::config::Table::new();
    both.insert_path(&model, Value::Text("a-model".to_owned()));
    both.insert_path(&sibling, Value::Text("frontier".to_owned()));
    assert_eq!(
        both.get_path(&model).and_then(Value::as_text),
        Some("a-model"),
    );
    assert_eq!(
        both.get_path(&sibling).and_then(Value::as_text),
        Some("frontier"),
        "the sibling spelling is the one that holds both settings, which is why it was chosen",
    );
}
