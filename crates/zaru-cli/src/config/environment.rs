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
//! # One `ZARU_*` name is reserved, and it is the only exception
//!
//! [`CREDENTIAL_KEY_VARIABLE`] holds [ADR-0007] D3's sealing key on a machine
//! with no OS keyring. It is **not** a configuration key and must never become
//! one: [ADR-0014] D4 keeps credentials out of configuration because "a config
//! file gets committed to a repository", and a sealing key is worth every
//! credential in the store rather than one of them.
//!
//! So this layer passes exactly that name through untouched — it enters no
//! layer's document, reaches no schema, and appears in no explanation of where
//! a value came from. It is skipped **by reference to the constant** rather
//! than by a second spelling, because a name written twice is a name that
//! diverges, and the divergence would be silent in the direction that matters:
//! the reader would refuse a user for setting their key correctly.
//!
//! The rule needs its other half too. Under this module's transform the key
//! `credential.key` would produce that same variable, so a schema declaring it
//! would make one variable mean a sealing key and a setting at once. That is
//! refused as [`ConfigRefused::ReservedEnvironmentName`], on the same machinery
//! that already refuses two declared keys colliding with each other.
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
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
use crate::credentials::CREDENTIAL_KEY_VARIABLE;
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

/// The key a variable names, when it names a member of a declared family.
///
/// # This is the one place the transform runs backwards, and only a family
/// makes that possible
///
/// [ADR-0014](https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy)
/// D3's transform is `ZARU_` plus the key upper-cased with dots turned into
/// underscores, and it is **not injective**: `ZARU_NOTES_WORK_AGENT_TOOLS`
/// could be `notes.work.agent_tools` or `notes.work.agent.tools` or
/// `notes.work_agent.tools`, and nothing in the name says which. That is why
/// every other key is matched forwards, by computing its variable and looking
/// the name up.
///
/// A family removes the ambiguity, because it fixes both ends: the prefix and
/// the suffix are known, so what is left between them is one segment whatever
/// underscores it contains. `ZARU_NOTES_` and `_AGENT_TOOLS` bracket exactly
/// `WORK`, and the key is `notes.work.agent_tools`.
///
/// **The middle is lower-cased**, which is the inverse of the forward
/// transform and is therefore the only spelling that round-trips. An alias
/// carrying an upper-case letter is reachable from a file and not from layer
/// 4, which is the same limit ADR-0014's own register already records for a
/// key segment carrying a hyphen — a name no POSIX shell can set — and it is
/// recorded here rather than worked around, because inventing a second
/// spelling would be this module deciding what a person's alias is called.
fn family_key(schema: &Schema, variable: &str) -> Option<Key> {
    let bare = variable.strip_prefix(PREFIX)?;
    schema.families().find_map(|family| {
        let head = format!("{}_", family.prefix().to_uppercase());
        let tail = format!("_{}", family.suffix().replace('.', "_").to_uppercase());
        let middle = bare.strip_prefix(&head)?.strip_suffix(&tail)?;
        if middle.is_empty() {
            return None;
        }
        Key::new(&format!(
            "{}.{}.{}",
            family.prefix(),
            middle.to_lowercase(),
            family.suffix()
        ))
        .ok()
    })
}

/// Build layer 4's document from a schema and a set of variables.
///
/// The variables are a parameter rather than read from the process, because
/// [`std::env::set_var`] is `unsafe` in this edition and the workspace denies
/// `unsafe_code`. That makes the seam the product's own: the product passes
/// the `ZARU_` pairs of the [`Variables`](crate::config::Variables) its `main`
/// read once, and a check passes pairs it owns, both through this one
/// function.
///
/// Values arrive as text, so every one is [`Value::Text`]; the schema's
/// [`coerce`](crate::config::schema::FieldKind::coerce) brings them to their
/// declared shape during the load, on the same path a file's values take.
///
/// # Errors
///
/// [`ConfigRefused::AmbiguousEnvironmentName`] when two declared keys produce
/// one variable name, [`ConfigRefused::ReservedEnvironmentName`] when a
/// declared key produces the reserved one, and [`ConfigRefused::UnknownKey`]
/// for a `ZARU_*` variable that maps to no declared key.
pub fn read(
    schema: &Schema,
    variables: impl IntoIterator<Item = (String, String)>,
) -> Result<Table, ConfigRefused> {
    let mut names: BTreeMap<String, &Key> = BTreeMap::new();
    for key in schema.keys() {
        let name = variable_name(key);
        if name == CREDENTIAL_KEY_VARIABLE {
            return Err(ConfigRefused::ReservedEnvironmentName {
                variable: name,
                key: key.clone(),
            });
        }
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
        // The one reserved name, skipped by reference to the constant that
        // declares it rather than by a second spelling. It holds a sealing key
        // rather than a setting, so it enters no document -- and it is not an
        // unknown key either, because refusing it would refuse a user for
        // doing what ADR-0007 D3 tells them to.
        if name == CREDENTIAL_KEY_VARIABLE {
            continue;
        }
        match names.get(&name) {
            Some(key) => document.insert_path(key, Value::Text(value)),
            // A declared *family* has no enumerable members, so its variables
            // cannot be in the map above: they are recognised by shape
            // instead. See `family_key`.
            None => match family_key(schema, &name) {
                Some(key) => document.insert_path(&key, Value::Text(value)),
                None => {
                    return Err(ConfigRefused::UnknownKey {
                        layer: Layer::Environment,
                        offered: name.clone(),
                        suggestion: nearest_variable(&names, &name),
                    });
                }
            },
        }
    }
    Ok(document)
}

/// The declared variable name nearest to one nothing declares.
///
/// The same metric [`Schema::nearest`](crate::config::schema::Schema::nearest)
/// uses, over the variable names rather than the keys — because the vocabulary
/// a person is reading here is variable names, and suggesting a dotted key for
/// a mistyped variable is a remedy they cannot apply.
///
/// Until 2026-09-05 this built a throwaway [`Schema`] out of the names in
/// order to reach that metric, which meant a name `Key::new` refused was a
/// candidate silently dropped. It calls the metric directly now — see
/// [`crate::config::nearest`].
fn nearest_variable(names: &BTreeMap<String, &Key>, offered: &str) -> Option<String> {
    crate::config::nearest::nearest(names.keys().map(String::as_str), offered).map(str::to_owned)
}
