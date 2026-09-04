// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The value model, and ADR-0014 D2's merge.
//!
//! # The model is ours rather than a parser's
//!
//! D1 names `~/.zaru/config.toml` and `./zaru.toml`, so TOML is what the two
//! file layers will be written in. This model is not TOML's, for two reasons.
//! D2's merge rule is a statement about *our* semantics — "For tables, merge
//! by key; for arrays, **replace wholesale**" — and it has to exist before
//! any parser does, because no TOML crate is in [ADR-0003] D2's dependency
//! table and declaring one is an amendment to that record rather than an
//! import. And layer 4 supplies strings whatever a file supplies, so a model
//! shaped like one format's would have to be bent for the other layers.
//!
//! # D2's rule, and the mutant that hides in it
//!
//! ```text
//! A project setting one key does not discard the user's other keys.
//! For tables, merge by key; for arrays, replace wholesale.
//! ```
//!
//! Array merging "is the choice that looks helpful and is not: a user who
//! cannot express 'exactly these and nothing inherited' ends up fighting the
//! config, and the failure is silent because the inherited entries look
//! plausible." So [`Table::merge_over`] recurses on tables and replaces
//! everything else, and the check that holds it uses arrays of **different
//! lengths** on the two sides — with equal lengths a concatenation and a
//! replacement are indistinguishable whenever the contents coincide, which
//! is [Verification lessons] §9's too-well-behaved fixture exactly.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::config::credential::CredentialRef;
use crate::config::key::Key;
use std::collections::BTreeMap;

/// One configuration value.
///
/// [`Value::Credential`] is the only variant a credential can be in, and it
/// holds a [`CredentialRef`] rather than a string. See that type for why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// A whole number. D6's "lower its own iteration ceiling" compares these.
    Integer(i64),
    /// Free text. **Never a credential** — see [`Value::Credential`].
    Text(String),
    /// A list. D2 replaces these wholesale rather than merging them.
    Array(Vec<Value>),
    /// A nested table. D2 merges these by key.
    Table(Table),
    /// A reference to a credential the store holds, per ADR-0014 D4.
    Credential(CredentialRef),
}

impl Value {
    /// What this value's shape is called, for a message that must not quote
    /// the value itself.
    ///
    /// Every refusal that mentions a shape uses this rather than rendering
    /// the value, because a type-mismatch message that printed its argument
    /// would publish whatever had landed in a wrongly-typed field.
    #[must_use]
    pub const fn shape(&self) -> &'static str {
        match self {
            Self::Bool(_) => "a boolean",
            Self::Integer(_) => "a whole number",
            Self::Text(_) => "text",
            Self::Array(_) => "a list",
            Self::Table(_) => "a table",
            Self::Credential(_) => "a credential reference",
        }
    }

    /// Text, in the one place text is what is wanted.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The whole number a `LowerOnly` ceiling compares, if this is one.
    #[must_use]
    pub const fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(number) => Some(*number),
            _ => None,
        }
    }
}

/// A table of configuration values, ordered by key.
///
/// `BTreeMap` rather than a hash map so that iteration order is a function of
/// the contents. An explanation whose rows moved between runs would not be
/// evidence about anything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table(BTreeMap<String, Value>);

impl Table {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything is in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many entries are directly in it, not counting nested ones.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Look up one name directly in this table.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// Put a value under one name directly in this table, returning whatever
    /// was there.
    pub fn insert(&mut self, name: impl Into<String>, value: Value) -> Option<Value> {
        self.0.insert(name.into(), value)
    }

    /// Every entry directly in this table, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// Follow a dotted key down through nested tables.
    #[must_use]
    pub fn get_path(&self, key: &Key) -> Option<&Value> {
        let mut here = self;
        let mut segments = key.segments().peekable();
        while let Some(segment) = segments.next() {
            let found = here.get(segment)?;
            if segments.peek().is_none() {
                return Some(found);
            }
            match found {
                Value::Table(inner) => here = inner,
                // A key descends through something that is not a table. That
                // is not a lookup failure to paper over -- the caller asked
                // for a path this document does not have.
                _ => return None,
            }
        }
        None
    }

    /// Put a value at a dotted key, creating the tables above it.
    ///
    /// A non-table sitting where a table is needed is replaced, because the
    /// only caller is [`crate::config::environment`], which builds a document
    /// from a schema whose keys cannot collide with each other that way.
    pub fn insert_path(&mut self, key: &Key, value: Value) {
        let segments: Vec<&str> = key.segments().collect();
        let (last, above) = segments
            .split_last()
            .expect("a Key always has at least one segment");
        let mut here = self;
        for segment in above {
            let slot = here
                .0
                .entry((*segment).to_owned())
                .or_insert_with(|| Value::Table(Self::new()));
            if !matches!(slot, Value::Table(_)) {
                *slot = Value::Table(Self::new());
            }
            let Value::Table(inner) = slot else {
                unreachable!("the slot was just made a table")
            };
            here = inner;
        }
        here.insert((*last).to_owned(), value);
    }

    /// Merge a higher layer's document over this one, per ADR-0014 D2.
    ///
    /// Tables merge by key at **every** depth; arrays, scalars and credential
    /// references are replaced wholesale. A key the higher layer does not
    /// mention keeps the value this table already had, which is D2's "A
    /// project setting one key does not discard the user's other keys."
    pub fn merge_over(&mut self, higher: Self) {
        for (name, incoming) in higher.0 {
            match (self.0.get_mut(&name), incoming) {
                (Some(Value::Table(below)), Value::Table(above)) => below.merge_over(above),
                (_, incoming) => {
                    self.0.insert(name, incoming);
                }
            }
        }
    }
}

impl FromIterator<(String, Value)> for Table {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(entries: I) -> Self {
        Self(entries.into_iter().collect())
    }
}
