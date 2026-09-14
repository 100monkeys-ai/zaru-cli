// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Everything a load can refuse, in words that never carry a value.
//!
//! # No variant has a field a configuration value could occupy
//!
//! A refusal is exactly the text that gets pasted into a report, and the
//! library's [Credentials] page is blunt about it: "A failing test that
//! quotes the value it was handed has published it." So no variant here
//! carries a [`Value`](crate::config::value::Value). Where a shape has to be
//! named it is named as a `&'static str` — "text", "a whole number" — never
//! by rendering what was offered. This is the same argument
//! [`SecretRefused`](crate::credentials::SecretRefused) makes by carrying
//! nothing at all, one layer up.
//!
//! Two variants carry whole numbers, and that is deliberate:
//! [`ConfigRefused::ProjectMayNotRaise`] has to say which ceiling was granted
//! and which was asked for, or ADR-0014 D6's refusal is unactionable. A whole
//! number cannot be credential-shaped — [ADR-0007] D2 discriminates by string
//! prefix — and a credential-shaped value is refused long before it could
//! reach a key declared as a ceiling.
//!
//! # The redaction check is stronger than "does the value appear"
//!
//! The `credential-store` arc lost a mutation here on 2026-09-04: putting a
//! bearer value into a refusal's `Display` through `{:?}` left every check
//! green, because `{:?}` on a string escapes a combining mark to `\u{301}`
//! and the raw value was therefore genuinely absent from a rendering that
//! published every byte of it. The checks on this module assert the absence
//! of a planted value **and of its ASCII core**, which no escaping scheme
//! alters. See [Verification lessons] §50.
//!
//! [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons-2

use crate::config::key::{Key, KeyRefused};
use crate::config::layer::Layer;
use crate::credentials::AliasRefused;
use core::fmt;

/// Why a configuration would not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRefused {
    /// Two contributions claimed the same layer.
    ///
    /// A caller error rather than a user's, and refused rather than resolved
    /// by taking one: ADR-0014 D1 has five layers and which of two documents
    /// is "layer 3" is not a question the record answers.
    DuplicateLayer {
        /// The layer offered twice.
        layer: Layer,
    },

    /// ADR-0014 D5 — a key nothing declares.
    ///
    /// `offered` and `suggestion` are in the vocabulary of the layer they
    /// came from: dotted keys for the file layers, `ZARU_*` variable names
    /// for the environment. The message says which.
    UnknownKey {
        /// Where it arrived.
        layer: Layer,
        /// What was written.
        offered: String,
        /// The nearest declared key, absent only when nothing is declared.
        suggestion: Option<String>,
    },

    /// A name in a document that no configuration key can be built from.
    ///
    /// Refused rather than skipped: a skip is exactly the setting that
    /// silently does nothing, which D5 calls "the worst outcome of any config
    /// system".
    UnusableKey {
        /// Where it arrived.
        layer: Layer,
        /// The dotted path it would have had.
        offered: String,
        /// Why no key could be built from it.
        refusal: KeyRefused,
    },

    /// ADR-0014 D4 — a value shaped like a credential.
    ///
    /// **Carries the key and never the value.**
    CredentialShaped {
        /// Where it arrived.
        layer: Layer,
        /// Which key it was under.
        key: Key,
        /// Whether the key is declared as holding a credential reference, or
        /// merely happened to carry a credential-shaped string in a file.
        /// The two have different remedies.
        declared_as_a_reference: bool,
    },

    /// The value's shape is not the one the key declares.
    WrongShape {
        /// Where it arrived.
        layer: Layer,
        /// Which key.
        key: Key,
        /// What the schema declares. A shape name, never a value.
        expected: &'static str,
        /// What was offered. A shape name, never a value.
        found: &'static str,
    },

    /// The value was text but does not read as the shape the key declares.
    UnparsableText {
        /// Where it arrived.
        layer: Layer,
        /// Which key.
        key: Key,
        /// What the schema declares.
        expected: &'static str,
    },

    /// A credential reference whose alias the store would not take.
    ///
    /// An alias is local metadata carrying no secret, so ADR-0007's own
    /// refusal quotes it and this one carries it. A value that *is* a
    /// credential is refused as [`ConfigRefused::CredentialShaped`] strictly
    /// before it could reach here, so nothing credential-shaped is ever
    /// quoted by this variant.
    UnusableAlias {
        /// Where it arrived.
        layer: Layer,
        /// Which key.
        key: Key,
        /// ADR-0007's own refusal.
        refusal: AliasRefused,
    },

    /// ADR-0014 D6 — the project layer set a key it may not set.
    ProjectMayNotSet {
        /// Which key.
        key: Key,
        /// Why, as the schema declares it.
        reason: String,
    },

    /// ADR-0014 D6 — the project layer tried to raise a ceiling.
    ProjectMayNotRaise {
        /// Which key.
        key: Key,
        /// What the layers below the project granted.
        granted: i64,
        /// What the project asked for.
        asked: i64,
    },

    /// Two declared keys map to one `ZARU_*` variable name.
    ///
    /// The transform upper-cases a dotted key and turns its dots into
    /// underscores, which cannot distinguish `runtime.max_iterations` from
    /// `runtime.max.iterations`. A schema that collides is refused loudly
    /// rather than resolved by picking one.
    AmbiguousEnvironmentName {
        /// The variable both keys produce.
        variable: String,
        /// The lexically first of the two.
        first: Key,
        /// The lexically second.
        second: Key,
    },
    /// A declared key produces the one `ZARU_*` name that is reserved.
    ///
    /// [ADR-0007] D3's sealing key lives in an environment variable when there
    /// is no OS keyring, and that variable is deliberately **not** a
    /// configuration key: [ADR-0014] D4 keeps credentials out of configuration,
    /// and a key is the one thing worth more than a credential. So layer 4
    /// passes that one name through untouched, and a schema that declared a key
    /// producing it would make the same variable mean two things at once.
    ///
    /// Ours rather than the user's, like the ambiguity above: a schema is built
    /// by this crate.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    ReservedEnvironmentName {
        /// The reserved variable.
        variable: String,
        /// The key that produces it.
        key: Key,
    },
}

impl fmt::Display for ConfigRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateLayer { layer } => write!(
                f,
                // ADR-0014 D1 has one document per layer.
                "two contributions both claim layer {} ({}); there is one document per layer and \
                 nothing says which of two would win",
                layer.number(),
                layer.label(),
            ),

            Self::UnknownKey {
                layer,
                offered,
                suggestion,
            } => {
                write!(
                    f,
                    "unknown key `{offered}` in {} (layer {})",
                    layer.label(),
                    layer.number(),
                )?;
                match suggestion {
                    Some(nearest) => write!(f, "; did you mean `{nearest}`?"),
                    None => {
                        f.write_str("; nothing is declared, so there is no nearer key to suggest")
                    }
                }
            }

            Self::UnusableKey {
                layer,
                offered,
                refusal,
            } => write!(
                f,
                "the name `{offered}` in {} (layer {}) is not a usable configuration key: \
                 {refusal}",
                layer.label(),
                layer.number(),
            ),

            Self::CredentialShaped {
                layer,
                key,
                declared_as_a_reference,
            } => {
                write!(
                    f,
                    "the value under `{key}` in {} (layer {}) is shaped like a bearer token, and \
                     it is deliberately not quoted here. ",
                    layer.label(),
                    layer.number(),
                )?;
                if *declared_as_a_reference {
                    f.write_str(
                        "That key names a credential rather than holding one: put the value in \
                         the credential store and write the alias here instead",
                    )
                } else {
                    f.write_str(
                        // ADR-0014 D4 keeps credentials out of configuration files.
                        "credentials are kept out of configuration files because a config file \
                         gets committed to a repository; the value belongs in the credential \
                         store, and configuration names it by alias",
                    )
                }
            }

            Self::WrongShape {
                layer,
                key,
                expected,
                found,
            } => write!(
                f,
                "`{key}` in {} (layer {}) is declared as {expected} and was given {found}",
                layer.label(),
                layer.number(),
            ),

            Self::UnparsableText {
                layer,
                key,
                expected,
            } => write!(
                f,
                "`{key}` in {} (layer {}) is declared as {expected} and the text given does not \
                 read as one",
                layer.label(),
                layer.number(),
            ),

            Self::UnusableAlias {
                layer,
                key,
                refusal,
            } => write!(
                f,
                "`{key}` in {} (layer {}) names a credential, and {refusal}",
                layer.label(),
                layer.number(),
            ),

            Self::ProjectMayNotSet { key, reason } => write!(
                f,
                // ADR-0014 D6.
                "the project's configuration sets `{key}`, which it may not: {reason}. A \
                 repository the user cloned must not be able to configure its way to more \
                 privilege than the user granted",
            ),

            Self::ProjectMayNotRaise {
                key,
                granted,
                asked,
            } => write!(
                f,
                // ADR-0014 D6.
                "the project's configuration raises `{key}` from {granted} to {asked}; a project \
                 may lower its own ceiling and never raise one",
            ),

            Self::AmbiguousEnvironmentName {
                variable,
                first,
                second,
            } => write!(
                f,
                "the keys `{first}` and `{second}` both map to the environment variable \
                 {variable}; the transform upper-cases a key and turns its dots into \
                 underscores, so it cannot tell them apart",
            ),
            Self::ReservedEnvironmentName { variable, key } => write!(
                f,
                "the key `{key}` maps to the environment variable {variable}, which is reserved:                  it holds the sealing key on a machine with no OS keyring, and it is                  deliberately not a configuration key, because credentials are kept out                  of configuration. No key may be declared that produces it",
            ),
        }
    }
}

impl std::error::Error for ConfigRefused {}
