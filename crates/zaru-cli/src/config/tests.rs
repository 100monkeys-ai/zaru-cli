// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The configuration hierarchy's checks, clause by clause.
//!
//! Every check here names the ADR-0014 clause it holds and the mutant that
//! would make it redden. Where a mutant is named in a comment it has been
//! run: the failure sentence is quoted in the commit that carries the check.

use super::fixtures::{
    StagedSource, ascii_core, at, document, key, keys_refused_to_projects, nonce,
    personal_secret_nonce, schema, text,
};
use crate::config::credential::CredentialRef;
use crate::config::environment;
use crate::config::explain::ExplanationRow;
use crate::config::key::Key;
use crate::config::layer::{Contribution, Layer, Source};
use crate::config::port::gather;
use crate::config::refusal::ConfigRefused;
use crate::config::resolve::Resolution;
use crate::config::schema::{Field, FieldKind, Schema};
use crate::config::value::{Table, Value};

// ---------------------------------------------------------------------------
// D1 — five layers, higher wins
// ---------------------------------------------------------------------------

/// D1's table, in the order it lists them.
///
/// The mutant is reordering the enum's variants, which reorders the derived
/// `Ord` and therefore the whole precedence rule.
#[test]
fn the_layers_are_ordered_lowest_to_highest_as_d1_lists_them() {
    assert_eq!(
        Layer::ALL,
        [
            Layer::BuiltIn,
            Layer::User,
            Layer::Project,
            Layer::Environment,
            Layer::Flag,
        ],
    );
    let numbers: Vec<u8> = Layer::ALL.iter().map(|layer| layer.number()).collect();
    assert_eq!(numbers, vec![1, 2, 3, 4, 5]);

    for pair in Layer::ALL.windows(2) {
        assert!(
            pair[0] < pair[1],
            "layer {} ({}) does not sort below layer {} ({}), so `higher wins` is not the enum's \
             own order",
            pair[0].number(),
            pair[0].label(),
            pair[1].number(),
            pair[1].label(),
        );
    }
}

/// D1 — "Higher wins", with a value in every layer.
///
/// Every layer's value is a literal this check owns, so neither side of the
/// assertion travels through the resolver ([Verification lessons] §10).
///
/// The mutant is folding in the other direction.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_layer_that_wins_is_the_highest_one_that_set_the_key() {
    let planted: Vec<(Layer, String)> = Layer::ALL
        .into_iter()
        .map(|layer| (layer, nonce(layer.label())))
        .collect();

    let contributions: Vec<Contribution> = planted
        .iter()
        .map(|(layer, value)| {
            at(
                *layer,
                layer.label(),
                document([("project.name", text(value.clone()))]),
            )
        })
        .collect();

    let resolved = Resolution::resolve(&schema(), contributions).expect("the fixture resolves");
    let flag_value = &planted
        .iter()
        .find(|(layer, _)| *layer == Layer::Flag)
        .expect("Layer::ALL contains the flag layer")
        .1;

    assert_eq!(
        resolved.get(&key("project.name")),
        Some(&text(flag_value.clone())),
        "with all five layers setting `project.name`, the value did not come from layer 5",
    );
}

/// D1 — precedence between **every adjacent pair**, not only the extremes.
///
/// A check that sets all five layers and asserts the flag wins passes against
/// an implementation that has one middle pair backwards. Four pairwise cases
/// are what separate them, which is [Verification lessons] §28's
/// adjacent-coverage disguise applied to the precedence order itself.
///
/// Every pair is reported rather than the first failure alone, so one run
/// says which pairs are wrong.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn precedence_holds_between_every_adjacent_pair_of_layers() {
    let mut wrong: Vec<String> = Vec::new();

    for pair in Layer::ALL.windows(2) {
        let (lower, higher) = (pair[0], pair[1]);
        let lower_value = nonce("lower");
        let higher_value = nonce("higher");

        let resolved = Resolution::resolve(
            &schema(),
            vec![
                at(
                    lower,
                    lower.label(),
                    document([("project.name", text(lower_value.clone()))]),
                ),
                at(
                    higher,
                    higher.label(),
                    document([("project.name", text(higher_value.clone()))]),
                ),
            ],
        )
        .expect("the fixture resolves");

        let effective = resolved.get(&key("project.name"));
        if effective != Some(&text(higher_value.clone())) {
            wrong.push(format!(
                "layer {} ({}) did not beat layer {} ({}): the effective value was {:?}",
                higher.number(),
                higher.label(),
                lower.number(),
                lower.label(),
                effective,
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "ADR-0014 D1 says higher wins, and {} of the four adjacent pairs disagree:\n  {}",
        wrong.len(),
        wrong.join("\n  "),
    );
}

/// D1 has one document per layer and does not say which of two would win.
///
/// The mutant is taking the last contribution silently.
#[test]
fn a_layer_offered_twice_is_refused_rather_than_resolved_by_taking_one() {
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::Project,
                "./zaru.toml",
                document([("project.name", text("first"))]),
            ),
            at(
                Layer::Project,
                "./other.toml",
                document([("project.name", text("second"))]),
            ),
        ],
    )
    .expect_err("two documents claimed layer 3 and the fold accepted them");

    assert_eq!(
        refusal,
        ConfigRefused::DuplicateLayer {
            layer: Layer::Project
        },
    );
}

// ---------------------------------------------------------------------------
// D2 — merge per key; arrays replace wholesale
// ---------------------------------------------------------------------------

/// D2 — "A project setting one key does not discard the user's other keys."
///
/// The mutant is replacing the table wholesale instead of merging it.
#[test]
fn a_table_merge_keeps_the_sibling_keys_the_higher_layer_did_not_mention() {
    let kept = nonce("kept-by-the-user");
    let overridden = nonce("set-by-the-project");

    let resolved = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::User,
                "~/.zaru/config.toml",
                document([
                    ("project.name", text(overridden.clone())),
                    ("project.workspace", text(kept.clone())),
                ]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("project.name", text("from-the-project"))]),
            ),
        ],
    )
    .expect("the fixture resolves");

    assert_eq!(
        resolved.get(&key("project.workspace")),
        Some(&text(kept)),
        "the project set `project.name` and the user's `project.workspace` did not survive",
    );
    assert_eq!(
        resolved.get(&key("project.name")),
        Some(&text("from-the-project")),
        "the project's own key did not win",
    );
}

/// D2's merge is per key at **every** depth, not only the top one.
///
/// The mutant is dropping the recursive arm, which merges the outermost table
/// and replaces everything below it.
#[test]
fn a_nested_table_merges_at_every_depth() {
    let mut lower = Table::new();
    let mut inner = Table::new();
    inner.insert("kept", text("from-below"));
    inner.insert("replaced", text("from-below"));
    lower.insert("labels", Value::Table(inner));

    let mut higher = Table::new();
    let mut higher_inner = Table::new();
    higher_inner.insert("replaced", text("from-above"));
    higher.insert("labels", Value::Table(higher_inner));

    lower.merge_over(higher);

    let Some(Value::Table(merged)) = lower.get("labels") else {
        panic!("`labels` stopped being a table")
    };
    assert_eq!(
        merged.get("kept"),
        Some(&text("from-below")),
        "a sibling one level down did not survive the merge",
    );
    assert_eq!(
        merged.get("replaced"),
        Some(&text("from-above")),
        "the higher layer's own nested key did not win",
    );
}

/// D2 — "for arrays, **replace wholesale**".
///
/// The two arrays have **different lengths on purpose**. With equal lengths a
/// concatenation and a replacement are indistinguishable whenever the
/// contents coincide, which is [Verification lessons] §9's too-well-behaved
/// fixture — the length is what makes the mutant visible.
///
/// The mutant is extending rather than replacing.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn an_array_is_replaced_wholesale_and_never_concatenated() {
    let below = Value::Array(vec![text("a"), text("b"), text("c")]);
    let above = Value::Array(vec![text("z")]);

    let resolved = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::User,
                "~/.zaru/config.toml",
                document([("project.validators", below)]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("project.validators", above.clone())]),
            ),
        ],
    )
    .expect("the fixture resolves");

    assert_eq!(
        resolved.get(&key("project.validators")),
        Some(&above),
        "the user's three entries were not discarded; D2 replaces an array wholesale",
    );
}

// ---------------------------------------------------------------------------
// D3 — every setting is explainable
// ---------------------------------------------------------------------------

/// D3's block prints all five layers, including the three that set nothing.
///
/// The mutant is recording only the layers that contributed, which renders a
/// block with fewer rows than D3's and cannot say `(not set)` at all.
#[test]
fn every_layer_appears_in_the_explanation_including_the_ones_that_set_nothing() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::User,
            "~/.zaru/config.toml",
            document([("runtime.max_iterations", Value::Integer(5))]),
        )],
    )
    .expect("the fixture resolves");

    let explanation = resolved.explain(&key("runtime.max_iterations"));
    assert_eq!(
        explanation.rows.len(),
        Layer::ALL.len(),
        "D3's block has one row per layer and this explanation has {} of {}",
        explanation.rows.len(),
        Layer::ALL.len(),
    );

    let layers: Vec<Layer> = explanation.rows.iter().map(|row| row.layer).collect();
    let mut highest_first = Layer::ALL.to_vec();
    highest_first.reverse();
    assert_eq!(
        layers, highest_first,
        "D3 prints layer 5 first and layer 1 last"
    );

    let unset = explanation
        .rows
        .iter()
        .filter(|row| row.value.is_none())
        .count();
    assert_eq!(unset, 4, "four of the five layers set nothing here");
}

/// D3 marks the layer the value **came from**, not the highest layer.
///
/// The two differ whenever the top layer is unset, which is the ordinary
/// case, so an implementation that marked layer 5 unconditionally would pass
/// a check that only ever set every layer.
///
/// The mutant is marking the first row rather than the first row with a value.
#[test]
fn the_effective_marker_names_the_layer_that_supplied_the_value_not_the_highest_layer() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::BuiltIn,
                "built-in",
                document([("runtime.max_iterations", Value::Integer(3))]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("runtime.max_iterations", Value::Integer(2))]),
            ),
        ],
    )
    .expect("the fixture resolves");

    let explanation = resolved.explain(&key("runtime.max_iterations"));
    assert_eq!(
        explanation.effective_layer(),
        Some(Layer::Project),
        "layers 4 and 5 set nothing, so the effective layer is 3 and not 5",
    );
    assert_eq!(
        explanation.rows.iter().filter(|row| row.effective).count(),
        1,
        "exactly one row carries D3's marker",
    );
}

/// D3's block, read back out of the rendered text.
///
/// One arm of this comparison does not travel through the renderer: the
/// expected values are the literals planted in each layer, per
/// [Verification lessons] §11. A check that asked the explanation for its own
/// numbers would agree with itself for as long as a defect lived.
///
/// The numbers are D3's own — 3, 5 and 8 — but the key is not. D3's example
/// explains `runtime.max_iterations`, which is a ceiling, and the block it
/// prints has the project *raising* it from 5 to 8. **D6 refuses that**, so
/// the record's own worked example cannot be resolved by an implementation
/// that holds the record. That contradiction has its own check below; this
/// one renders the same block over a key the project may set freely.
///
/// The mutant is rendering the effective marker on every row, and separately,
/// dropping the header line.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_explanation_renders_d3s_block_with_the_values_this_check_planted() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::BuiltIn,
                "built-in",
                document([("runtime.log_lines", Value::Integer(3))]),
            ),
            at(
                Layer::User,
                "~/.zaru/config.toml",
                document([("runtime.log_lines", Value::Integer(5))]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("runtime.log_lines", Value::Integer(8))]),
            ),
        ],
    )
    .expect("the fixture resolves");

    let block = resolved.explain(&key("runtime.log_lines")).to_string();
    let lines: Vec<&str> = block.lines().collect();

    assert_eq!(
        lines.first().copied(),
        Some("runtime.log_lines = 8"),
        "D3's header is the key and its effective value; the block was:\n{block}",
    );
    assert_eq!(
        lines.len(),
        6,
        "a header and five rows; the block was:\n{block}"
    );

    assert!(
        lines[3].contains("./zaru.toml")
            && lines[3].contains('8')
            && lines[3].contains("← effective"),
        "layer 3 planted 8 and its row is not the marked one; the block was:\n{block}",
    );
    assert!(
        lines[4].contains('5') && !lines[4].contains("← effective"),
        "layer 2 planted 5 and its row must carry no marker; the block was:\n{block}",
    );
    assert!(
        lines[5].contains("built-in") && lines[5].contains('3'),
        "layer 1 planted 3; the block was:\n{block}",
    );
    assert_eq!(
        block.matches("← effective").count(),
        1,
        "exactly one row is marked; the block was:\n{block}",
    );
    assert_eq!(
        block.matches("(not set)").count(),
        2,
        "layers 4 and 5 set nothing; the block was:\n{block}",
    );
}

/// **ADR-0014 D3's worked example is refused by ADR-0014 D6.**
///
/// Found by running rather than by reading ([Verification lessons] §29): the
/// first version of the check above staged D3's block verbatim and the fold
/// refused it.
///
/// D3 explains `runtime.max_iterations` and prints `3  ./zaru.toml  8  ←
/// effective` above `2  ~/.zaru/config  5`. That is a project file raising an
/// iteration ceiling from 5 to 8. D6 says a project "may lower its own
/// iteration ceiling" and lists what it may not do; raising one is not on the
/// permitted list, and the whole clause exists so that "a repository the user
/// cloned must not be able to configure its way to more privilege than the
/// user granted".
///
/// Both clauses cannot hold as written. This check pins the contradiction so
/// that correcting the record reddens it and whoever corrects it sees this
/// note, rather than the disagreement being smoothed over in an
/// implementation. It is recorded on the record as a question for the author
/// and is **not** settled here.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn adr_0014_d3s_worked_example_is_refused_by_adr_0014_d6() {
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::User,
                "~/.zaru/config",
                document([("runtime.max_iterations", Value::Integer(5))]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("runtime.max_iterations", Value::Integer(8))]),
            ),
        ],
    )
    .expect_err(
        "ADR-0014 D3's worked example raises a ceiling from 5 to 8, which D6 forbids; if this \
         now resolves, the record has been corrected and this check should be read again",
    );

    assert_eq!(
        refusal,
        ConfigRefused::ProjectMayNotRaise {
            key: key("runtime.max_iterations"),
            granted: 5,
            asked: 8,
        },
    );
}

/// D3's block names the environment's variable on a row that set nothing.
///
/// The record's own example does exactly this — `4  ZARU_MAX_ITER  (not
/// set)` — so a source that were only known when a layer contributed could
/// not render it.
///
/// The mutant is falling back to the layer's label for the environment.
#[test]
fn an_unset_environment_row_still_names_the_variable_the_key_maps_to() {
    let resolved = Resolution::resolve(&schema(), Vec::new()).expect("an empty fold resolves");
    let explanation = resolved.explain(&key("runtime.max_iterations"));

    let row = explanation
        .rows
        .iter()
        .find(|row| row.layer == Layer::Environment)
        .expect("there is an environment row");

    assert!(row.value.is_none(), "nothing was staged in the environment");
    assert_eq!(
        row.source, "ZARU_RUNTIME_MAX_ITERATIONS",
        "an unset environment row still names the variable the key maps to",
    );
}

/// An explanation row is four fields, and a fifth would not compile.
///
/// The destructuring is the mechanism: a field added to
/// [`ExplanationRow`] stops this check compiling rather than travelling
/// unasserted. The same signal ADR-0007's agent projection uses.
#[test]
fn an_explanation_row_is_four_fields_and_a_fifth_would_not_compile() {
    let resolved = Resolution::resolve(&schema(), Vec::new()).expect("an empty fold resolves");
    let explanation = resolved.explain(&key("project.name"));
    for row in explanation.rows {
        let ExplanationRow {
            layer,
            source,
            value,
            effective,
        } = row;
        assert!(
            !source.is_empty(),
            "layer {} rendered no source",
            layer.number()
        );
        assert!(value.is_none());
        assert!(!effective);
    }
}

// ---------------------------------------------------------------------------
// D4 — secrets never live in config files
// ---------------------------------------------------------------------------

/// D4 — "the design decision that prevents it is refusing to have a field to
/// put one in", as a property of the type.
///
/// The destructuring is the mechanism: adding a `String` to
/// [`CredentialRef`] — the field a bearer value would occupy — stops this
/// check compiling.
#[test]
fn a_credential_reference_is_one_alias_and_a_second_field_would_not_compile() {
    let alias = crate::credentials::Alias::new("work").expect("a well-formed alias");
    let reference = CredentialRef::new(alias.clone());
    let CredentialRef { alias: only } = reference;
    assert_eq!(only, alias);
}

/// D4 and Trigger clause 3 — a credential-shaped value in a config file is
/// refused at load, pointing at the credential store.
///
/// The mutant is accepting it; and separately, pointing the message anywhere
/// but ADR-0007's store.
#[test]
fn a_credential_shaped_value_in_a_config_file_is_refused_pointing_at_the_store() {
    let planted = personal_secret_nonce();

    let refusal = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::User,
            "~/.zaru/config.toml",
            document([("project.name", text(planted.clone()))]),
        )],
    )
    .expect_err("a config file carried a bearer value and the load accepted it");

    assert!(
        matches!(
            refusal,
            ConfigRefused::CredentialShaped {
                layer: Layer::User,
                ..
            }
        ),
        "the refusal was {refusal:?}",
    );
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("credential store"),
        "D4 sends the value to the credential store and the refusal does not say so: {rendered}",
    );
}

/// D4 — a key that *names* a credential refuses a value at **every** layer,
/// including the two no file scan reaches.
///
/// A bearer value is a perfectly well-formed alias, so without this it would
/// become the *name* of a credential and travel into `notes:<alias>` tool
/// names and D7's listing. The scan therefore has two triggers: the layer's
/// own rule, and the key being declared as a reference.
///
/// The mutant is scanning on the layer's rule alone, which leaves layers 1, 4
/// and 5 open.
#[test]
fn a_bearer_value_offered_to_a_credential_key_is_refused_at_every_layer() {
    let mut missed: Vec<String> = Vec::new();

    for layer in Layer::ALL {
        let planted = personal_secret_nonce();
        let outcome = Resolution::resolve(
            &schema(),
            vec![at(
                layer,
                layer.label(),
                document([("provider.credential", text(planted.clone()))]),
            )],
        );

        match outcome {
            Err(ConfigRefused::CredentialShaped {
                declared_as_a_reference: true,
                ..
            }) => {}
            other => missed.push(format!(
                "layer {} ({}) did not refuse a bearer value offered to a credential key: {:?}",
                layer.number(),
                layer.label(),
                other.map(|_| "it resolved"),
            )),
        }
    }

    assert!(
        missed.is_empty(),
        "{} of the five layers let a bearer value become an alias:\n  {}",
        missed.len(),
        missed.join("\n  "),
    );
}

/// The refusal carries neither the planted value nor its ASCII core.
///
/// **Both arms, and the second is the one that matters.** A check asserting
/// only that the raw value is absent is blind to a formatter that escapes:
/// `{:?}` on a string renders a combining mark as `\u{301}`, so a rendering
/// that published every byte would still not `contain` the nonce. That
/// mutation survived in the credential store on 2026-09-04; the ASCII core is
/// what no escaping scheme alters.
///
/// The mutant is interpolating the value into the refusal's `Display` through
/// `{:?}`.
#[test]
fn the_refusal_for_a_credential_shaped_value_carries_neither_the_value_nor_its_ascii_core() {
    let planted = personal_secret_nonce();
    let core = ascii_core(&planted);

    let refusal = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::Project,
            "./zaru.toml",
            document([("project.workspace", text(planted.clone()))]),
        )],
    )
    .expect_err("a project file carried a bearer value and the load accepted it");

    let rendered = refusal.to_string();
    assert!(
        rendered.contains("project.workspace"),
        "a refusal that names no key is not actionable: {rendered}",
    );
    assert!(
        !rendered.contains(&planted),
        "the refusal published the bearer value verbatim: it is in {rendered}",
    );
    assert!(
        !rendered.contains(core),
        "the refusal published the bearer value in an escaped form; its ASCII core {core} is in \
         {rendered}",
    );
    let debug = format!("{refusal:?}");
    assert!(
        !debug.contains(&planted) && !debug.contains(core),
        "the refusal's Debug published the bearer value or its ASCII core {core}: {debug}",
    );
}

/// A credential key holds an alias once the load succeeds — the other half of
/// the refusal, so that "refuse everything" is not what is being checked.
#[test]
fn a_credential_key_holds_an_alias_after_a_successful_load() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::User,
            "~/.zaru/config.toml",
            document([("provider.credential", text("work"))]),
        )],
    )
    .expect("an alias is not a bearer value and the load takes it");

    let alias = crate::credentials::Alias::new("work").expect("a well-formed alias");
    assert_eq!(
        resolved.get(&key("provider.credential")),
        Some(&Value::Credential(CredentialRef::new(alias))),
    );
}

// ---------------------------------------------------------------------------
// D5 — unknown keys are an error at load, naming the nearest match
// ---------------------------------------------------------------------------

/// D5 — "A typo that silently does nothing is the worst outcome of any config
/// system."
///
/// The mutant is ignoring a key nothing declares.
#[test]
fn an_unknown_key_fails_the_load_naming_the_key_it_could_not_place() {
    let refusal = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::Project,
            "./zaru.toml",
            document([("runtime.max_iteration", Value::Integer(8))]),
        )],
    )
    .expect_err("an undeclared key loaded without complaint");

    let rendered = refusal.to_string();
    assert!(
        rendered.contains("runtime.max_iteration"),
        "the refusal does not name the key it could not place: {rendered}",
    );
}

/// D5 — the suggestion is the **nearest** declared key, not merely a declared
/// one.
///
/// The fixture schema carries eleven keys, of which exactly one is at edit
/// distance 1 from the offered key. A fixture with a single declared key
/// would make "nearest" and "any" indistinguishable, which is
/// [Verification lessons] §9 arriving through the fixture rather than the
/// assertion.
///
/// The mutant is suggesting the first declared key.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_suggestion_is_the_nearest_declared_key_and_not_merely_a_declared_one() {
    let schema = schema();
    assert!(
        schema.len() > 2,
        "a schema with fewer than three keys cannot tell `nearest` from `any`; it has {}",
        schema.len(),
    );
    let first_declared = schema
        .keys()
        .next()
        .expect("the fixture schema is not empty")
        .clone();

    let refusal = Resolution::resolve(
        &schema,
        vec![at(
            Layer::Project,
            "./zaru.toml",
            document([("runtime.max_iteration", Value::Integer(8))]),
        )],
    )
    .expect_err("an undeclared key loaded without complaint");

    let ConfigRefused::UnknownKey { suggestion, .. } = &refusal else {
        panic!("the refusal was {refusal:?}")
    };
    assert_eq!(
        suggestion.as_deref(),
        Some("runtime.max_iterations"),
        "D5 suggests the nearest declared key; the lexically first is `{first_declared}` and \
         suggesting that would pass a weaker check",
    );
}

/// D5, on layer 4, in the environment's own vocabulary.
///
/// A mistyped variable is as silent as a mistyped file key, so it is refused
/// the same way — and the suggestion is a *variable name*, because a dotted
/// key is a remedy a person reading `ZARU_*` cannot apply.
///
/// The mutant is ignoring a `ZARU_*` variable that maps to no declared key.
#[test]
fn an_unknown_environment_variable_is_refused_in_the_environments_own_vocabulary() {
    let refusal = environment::read(
        &schema(),
        vec![("ZARU_RUNTIEM_MAX_ITERATIONS".to_owned(), "8".to_owned())],
    )
    .expect_err("a mistyped ZARU_ variable was read without complaint");

    let ConfigRefused::UnknownKey {
        layer,
        offered,
        suggestion,
    } = &refusal
    else {
        panic!("the refusal was {refusal:?}")
    };
    assert_eq!(*layer, Layer::Environment);
    assert_eq!(offered, "ZARU_RUNTIEM_MAX_ITERATIONS");
    assert_eq!(
        suggestion.as_deref(),
        Some("ZARU_RUNTIME_MAX_ITERATIONS"),
        "the suggestion must be a variable name, not a dotted key",
    );
}

/// A key that is declared holds its whole subtree rather than having every
/// branch reported as unknown.
#[test]
fn a_key_declared_as_a_table_takes_its_whole_subtree() {
    let mut labels = Table::new();
    labels.insert("team", text("platform"));
    labels.insert("tier", text("internal"));

    let resolved = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::User,
            "~/.zaru/config.toml",
            document([("project.labels", Value::Table(labels.clone()))]),
        )],
    )
    .expect("a declared table takes its own contents");

    assert_eq!(
        resolved.get(&key("project.labels")),
        Some(&Value::Table(labels)),
    );
}

// ---------------------------------------------------------------------------
// D6 — project config cannot raise a security posture
// ---------------------------------------------------------------------------

/// D6's escalations, **each asserted separately**.
///
/// The record's own Status tracking asks for exactly this: "Each of its four
/// escalations wants its own assertion rather than one test covering
/// 'escalation is rejected' — that shape is exactly the adjacent-coverage
/// disguise in Verification Lessons §4." The population is the fixture
/// schema's own declarations, so a fifth refused key is covered the moment it
/// is declared rather than when somebody remembers to add a case.
///
/// The mutant is refusing only the first refused key.
#[test]
fn each_key_the_project_may_not_set_is_refused_separately_naming_the_key_and_the_reason() {
    let schema = schema();
    let cases = keys_refused_to_projects();
    assert_eq!(
        cases.len(),
        4,
        "D6 names four escalations and this check stages {}",
        cases.len(),
    );

    let mut wrong: Vec<String> = Vec::new();
    for (offending, value) in cases {
        let mut project = Table::new();
        project.insert_path(&offending, value);

        let outcome = Resolution::resolve(
            &schema,
            vec![Contribution::new(
                Layer::Project,
                Source::named("./zaru.toml"),
                project,
            )],
        );

        match outcome {
            Err(ConfigRefused::ProjectMayNotSet {
                ref key,
                ref reason,
            }) if *key == offending && !reason.is_empty() => {
                let rendered = outcome.unwrap_err().to_string();
                if !rendered.contains(offending.as_str()) {
                    wrong.push(format!("`{offending}` was refused without naming the key"));
                }
            }
            Err(other) => wrong.push(format!("`{offending}` was refused as {other:?}")),
            Ok(_) => wrong.push(format!(
                "`{offending}` was accepted from the project layer, which D6 forbids"
            )),
        }
    }

    assert!(
        wrong.is_empty(),
        "{} of D6's escalations are not held:\n  {}",
        wrong.len(),
        wrong.join("\n  "),
    );
}

/// D6's **permitted** direction — "A project may lower its own iteration
/// ceiling".
///
/// This is the arm that separates a correct implementation from one that
/// simply refuses everything the project layer offers. A refuse-always
/// implementation passes every refusal check above perfectly and fails here,
/// which is [Verification lessons] §13 — an invariant holding because both
/// sides are wrong together.
///
/// The mutant is refusing every project contribution.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_project_may_lower_a_ceiling() {
    let resolved = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::User,
                "~/.zaru/config.toml",
                document([("runtime.max_iterations", Value::Integer(8))]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("runtime.max_iterations", Value::Integer(3))]),
            ),
        ],
    )
    .expect("D6 lets a project lower its own ceiling");

    assert_eq!(
        resolved.get(&key("runtime.max_iterations")),
        Some(&Value::Integer(3)),
        "the project lowered the ceiling from 8 to 3 and the lower value did not take effect",
    );
}

/// D6 — and it may not raise one.
///
/// The mutant is dropping the direction, which allows both.
#[test]
fn a_project_may_not_raise_a_ceiling() {
    let refusal = Resolution::resolve(
        &schema(),
        vec![
            at(
                Layer::User,
                "~/.zaru/config.toml",
                document([("runtime.max_iterations", Value::Integer(3))]),
            ),
            at(
                Layer::Project,
                "./zaru.toml",
                document([("runtime.max_iterations", Value::Integer(99))]),
            ),
        ],
    )
    .expect_err("a project raised a ceiling the user had lowered");

    assert_eq!(
        refusal,
        ConfigRefused::ProjectMayNotRaise {
            key: key("runtime.max_iterations"),
            granted: 3,
            asked: 99,
        },
    );
    let rendered = refusal.to_string();
    assert!(
        rendered.contains('3') && rendered.contains("99"),
        "D6's refusal must say what was granted and what was asked: {rendered}",
    );
}

/// D6 binds the project layer alone; the user's own layers are the grant.
///
/// The mutant is applying the ceiling to every layer, which would stop a user
/// configuring their own machine.
#[test]
fn the_user_layer_may_set_what_the_project_layer_may_not() {
    let mut wrong: Vec<String> = Vec::new();
    for (offending, value) in keys_refused_to_projects() {
        let mut user = Table::new();
        user.insert_path(&offending, value.clone());

        if let Err(refusal) = Resolution::resolve(
            &schema(),
            vec![Contribution::new(
                Layer::User,
                Source::named("~/.zaru/config.toml"),
                user,
            )],
        ) {
            wrong.push(format!(
                "the user's own file could not set `{offending}`: {refusal}"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "D6 constrains the project layer and not the user's:\n  {}",
        wrong.join("\n  "),
    );
}

// ---------------------------------------------------------------------------
// Layer 4 — the environment
// ---------------------------------------------------------------------------

/// The transform, stated once and checked here.
///
/// **ADR-0014 D3's own example prints `ZARU_MAX_ITER` for
/// `runtime.max_iterations`**, which no mechanical transform produces. The
/// transform is what is built; the divergence is recorded on the record.
#[test]
fn the_variable_a_key_maps_to_is_zaru_plus_the_key_upper_cased() {
    assert_eq!(
        environment::variable_name(&key("runtime.max_iterations")),
        "ZARU_RUNTIME_MAX_ITERATIONS",
    );
    assert_eq!(
        environment::variable_name(&key("provider.credential")),
        "ZARU_PROVIDER_CREDENTIAL",
    );
    assert_ne!(
        environment::variable_name(&key("runtime.max_iterations")),
        "ZARU_MAX_ITER",
        "D3's worked example is not producible by this transform, and that is recorded on the \
         record rather than worked around here",
    );
}

/// The environment's values reach the fold and are coerced to their declared
/// shape, because layer 4 supplies text whatever a file supplies.
#[test]
fn an_environment_variable_reaches_the_fold_as_its_declared_shape() {
    let document = environment::read(
        &schema(),
        vec![("ZARU_RUNTIME_MAX_ITERATIONS".to_owned(), "4".to_owned())],
    )
    .expect("a declared variable reads");

    let resolved = Resolution::resolve(
        &schema(),
        vec![at(Layer::Environment, "environment", document)],
    )
    .expect("the fixture resolves");

    assert_eq!(
        resolved.get(&key("runtime.max_iterations")),
        Some(&Value::Integer(4)),
        "the environment supplies text and the schema's declared shape is a whole number",
    );
}

/// The transform cannot tell `a.b_c` from `a.b.c`, so a schema that collides
/// is refused rather than resolved by picking one.
#[test]
fn a_schema_whose_keys_collide_on_one_variable_is_refused() {
    let colliding = Schema::new()
        .with(key("runtime.max_iterations"), Field::ceiling())
        .with(key("runtime.max.iterations"), Field::ceiling());

    let refusal = environment::read(&colliding, Vec::new())
        .expect_err("two keys produced one variable and the read accepted it");

    assert!(
        matches!(refusal, ConfigRefused::AmbiguousEnvironmentName { .. }),
        "the refusal was {refusal:?}",
    );
}

/// A variable outside the prefix is not this hierarchy's business.
#[test]
fn a_variable_outside_the_prefix_is_ignored() {
    let document = environment::read(
        &schema(),
        vec![
            ("PATH".to_owned(), "/usr/bin".to_owned()),
            ("HOME".to_owned(), "/home/somebody".to_owned()),
        ],
    )
    .expect("variables outside the prefix are not read");

    assert!(
        document.is_empty(),
        "layer 4 is `ZARU_*` and nothing else; it read {document:?}",
    );
}

// ---------------------------------------------------------------------------
// The port
// ---------------------------------------------------------------------------

/// Every layer reaches the fold through the port, and each keeps the name its
/// source gave it.
///
/// **Nothing in the product tree implements [`LayerSource`]** — a TOML parser
/// and an argument parser are two dependencies ADR-0003 D2's table does not
/// name — so this check is the caller those parsers will be.
#[test]
fn every_layer_reaches_the_fold_through_the_source_that_named_it() {
    let user = StagedSource::new(
        Layer::User,
        "~/.zaru/config.toml",
        document([("project.name", text("from-the-user"))]),
    );
    let project = StagedSource::new(
        Layer::Project,
        "./zaru.toml",
        document([("project.name", text("from-the-project"))]),
    );

    let sources: Vec<&dyn crate::config::port::LayerSource> = vec![&user, &project];
    let contributions = gather(sources).expect("both sources read");
    let resolved = Resolution::resolve(&schema(), contributions).expect("the fixture resolves");

    let explanation = resolved.explain(&key("project.name"));
    let named: Vec<&str> = explanation
        .rows
        .iter()
        .map(|row| row.source.as_str())
        .collect();

    assert!(
        named.contains(&"~/.zaru/config.toml") && named.contains(&"./zaru.toml"),
        "D3's block shows the source each layer was read from; it showed {named:?}",
    );
    assert_eq!(
        resolved.get(&key("project.name")),
        Some(&text("from-the-project")),
    );
}

/// A key whose declared shape the value does not have is refused, and the
/// refusal names shapes rather than quoting what was offered.
#[test]
fn a_wrong_shape_is_refused_without_quoting_the_value() {
    let planted = nonce("not-a-number");
    let refusal = Resolution::resolve(
        &schema(),
        vec![at(
            Layer::User,
            "~/.zaru/config.toml",
            document([("runtime.max_iterations", text(planted.clone()))]),
        )],
    )
    .expect_err("text that is not a number was accepted for a whole-number key");

    let rendered = refusal.to_string();
    assert!(
        rendered.contains("a whole number"),
        "the refusal names the declared shape: {rendered}",
    );
    assert!(
        !rendered.contains(&planted) && !rendered.contains(ascii_core(&planted)),
        "a shape refusal must not quote the value that landed in the field: {rendered}",
    );
}

/// A key is refused the shapes D3's block cannot render.
#[test]
fn a_key_with_an_empty_segment_is_refused() {
    assert!(Key::new("runtime..max").is_err());
    assert!(Key::new(".runtime").is_err());
    assert!(Key::new("runtime.").is_err());
    assert!(Key::new("").is_err());
    assert!(Key::new("runtime.max_iterations").is_ok());
}

/// The kinds a value can be brought to, and the one conversion that is
/// deliberately absent.
#[test]
fn no_kind_parses_text_into_a_list() {
    assert!(
        FieldKind::Array
            .coerce(Value::Text("a,b,c".to_owned()))
            .is_err(),
        "no record says how an environment variable expresses a list, and inventing a separator \
         here would settle it",
    );
    assert!(
        FieldKind::Integer
            .coerce(Value::Text("7".to_owned()))
            .is_ok()
    );
    assert!(
        FieldKind::Bool
            .coerce(Value::Text("true".to_owned()))
            .is_ok()
    );
}
