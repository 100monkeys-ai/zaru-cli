// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! ADR-0014 D1's layer 4, and the transform that names its variables.
//!
//! # The transform, and the example in D3 that no transform produces
//!
//! The rule built here is `ZARU_` followed by the dotted key upper-cased with
//! its dots turned into underscores, so `runtime.max_iterations` becomes
//! `ZARU_RUNTIME_MAX_ITERATIONS`.
//!
//! **ADR-0014 D3's own worked example prints `ZARU_MAX_ITER` for
//! `runtime.max_iterations`.** No mechanical transform produces that: it
//! drops a segment and abbreviates a word. [ADR-0012] D4's example,
//! `ZARU_MODEL_DEFAULT`, is a third spelling again. So either every key
//! carries its own variable name in the schema, or a transform exists and
//! D3's example is wrong. **The transform is what is built and the alias
//! table is not**, and the divergence is recorded on the record rather than
//! papered over here.
//!
//! # The mapping runs forward only, because backwards it is ambiguous
//!
//! `ZARU_RUNTIME_MAX_ITERATIONS` could be `runtime.max_iterations` or
//! `runtime.max.iterations` or `runtime_max.iterations`; the transform throws
//! away the difference between a dot and an underscore. So the schema's keys
//! are walked forward to build the names, never the environment parsed
//! backwards, and a schema whose keys collide is refused by
//! [`ConfigRefused::AmbiguousEnvironmentName`] rather than resolved by
//! picking one.
//!
//! # A variable nothing declares is D5's error, not silence
//!
//! D5: "A typo that silently does nothing is the worst outcome of any config
//! system, because the user sees no change and concludes the setting does not
//! work." That is as true of `ZARU_RUNTIEM_MAX_ITERATIONS` as of a typo in a
//! file, so every `ZARU_*` variable that maps to no declared key is refused
//! with the nearest variable name suggested.
//!
//! [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction

use crate::config::key::Key;
use crate::config::layer::Layer;
use crate::config::refusal::ConfigRefused;
use crate::config::schema::Schema;
use crate::config::value::{Table, Value};
use std::collections::BTreeMap;

/// The prefix ADR-0014 D1 gives layer 4.
pub const PREFIX: &str = "ZARU_";

/// The variable a key maps to.
///
/// `ZARU_` plus the key upper-cased with dots turned into underscores.
#[must_use]
pub fn variable_name(key: &Key) -> String {
    let mut name = String::from(PREFIX);
    name.push_str(&key.as_str().replace('.', "_").to_uppercase());
    name
}

/// Build layer 4's document from a schema and a set of variables.
///
/// The variables are a parameter rather than read from the process, because
/// [`std::env::set_var`] is `unsafe` in this edition and the workspace denies
/// `unsafe_code`. That makes the seam the product's own: [`from_process`]
/// passes [`std::env::vars`] and a check passes pairs it owns, both through
/// this one function.
///
/// Values arrive as text, so every one is [`Value::Text`]; the schema's
/// [`coerce`](crate::config::schema::FieldKind::coerce) brings them to their
/// declared shape during the load, on the same path a file's values take.
///
/// # Errors
///
/// [`ConfigRefused::AmbiguousEnvironmentName`] when two declared keys produce
/// one variable name, and [`ConfigRefused::UnknownKey`] for a `ZARU_*`
/// variable that maps to no declared key.
pub fn read(
    schema: &Schema,
    variables: impl IntoIterator<Item = (String, String)>,
) -> Result<Table, ConfigRefused> {
    let mut names: BTreeMap<String, &Key> = BTreeMap::new();
    for key in schema.keys() {
        let name = variable_name(key);
        if let Some(existing) = names.get(&name) {
            return Err(ConfigRefused::AmbiguousEnvironmentName {
                variable: name,
                first: (*existing).clone(),
                second: key.clone(),
            });
        }
        names.insert(name, key);
    }

    let mut document = Table::new();
    for (name, value) in variables {
        if !name.starts_with(PREFIX) {
            continue;
        }
        match names.get(&name) {
            Some(key) => document.insert_path(key, Value::Text(value)),
            None => {
                return Err(ConfigRefused::UnknownKey {
                    layer: Layer::Environment,
                    offered: name.clone(),
                    suggestion: nearest_variable(&names, &name),
                });
            }
        }
    }
    Ok(document)
}

/// Build layer 4's document from this process's own environment.
///
/// The product path. Nothing reaches it yet: no binary resolves configuration
/// — ADR-0014 D3's `config explain` and [ADR-0015] D2's `/config` namespace
/// both need a command surface that does not exist.
///
/// # Errors
///
/// As [`read`].
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
pub fn from_process(schema: &Schema) -> Result<Table, ConfigRefused> {
    read(schema, std::env::vars())
}

/// The declared variable name nearest to one nothing declares.
///
/// The same edit distance and the same lexical tie-break
/// [`Schema::nearest`](crate::config::schema::Schema::nearest) uses, over the
/// variable names rather than the keys — because the vocabulary a person is
/// reading here is variable names, and suggesting a dotted key for a mistyped
/// variable is a remedy they cannot apply.
fn nearest_variable(names: &BTreeMap<String, &Key>, offered: &str) -> Option<String> {
    let mut schema = Schema::new();
    // The names are keys' worth of text and are re-validated here rather than
    // assumed: `Key::new` refuses nothing a variable name can contain, so the
    // conversion cannot lose a candidate silently.
    let mut candidates: Vec<&String> = names.keys().collect();
    candidates.sort();
    for name in candidates {
        if let Ok(key) = Key::new(name) {
            schema = schema.with(
                key,
                crate::config::schema::Field::free(crate::config::schema::FieldKind::Text),
            );
        }
    }
    schema.nearest(offered).map(|key| key.as_str().to_owned())
}
