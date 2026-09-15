// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Which of a token's tools the model may be offered: [ADR-0007] D5's grant,
//! as a configuration key.
//!
//! # Nothing is declared by default, and that is the decision
//!
//! D5 projects "every non-composer token ... carrying that token's tools".
//! Read literally that is every tool the token grants, and **every Nuclear
//! Notes token this project has ever held grants 94** — measured on
//! 2026-09-06 and again on 2026-09-14 and 2026-09-15. What that costs was
//! measured rather than estimated, from the release binary at `6bdf080`
//! through a logging proxy: one request carried **1,937 bytes**, of which
//! **1,619** were the seven built-ins' declarations and 224 the message. The
//! seven average 231 bytes each because their schemas have one or two string
//! fields; a Nuclear Notes tool's schema is a different order of thing. Even
//! at the built-ins' own average, 94 of them is about 22 KB on every request
//! of every exchange, against [`crate::providers::ollama`]'s own declared
//! window of 4,096 tokens.
//!
//! ADR-0007's Negative section says exactly this and says it first: "Every
//! namespace the agent sees costs context-window budget for its tool schemas.
//! This is the exact cost ADR-0135 was written to control, now multiplied by
//! token count — so tight per-token scoping matters more here than in a
//! single-token design, not less."
//!
//! So a projection declares **nothing** unless a person says which tools.
//!
//! # Why this is not a filter the harness chose
//!
//! The alternative considered was to project a fixed read-only set — [ADR-0006]
//! D4's nine. That was **withdrawn**, because D4's set is the *composer's*
//! scope and ADR-0006 says the opposite about the agent in as many words: D3,
//! "The agent keeps full Nuclear Notes MCP access at whatever scope the user
//! granted, including autonomous `me.set_current_workspace`", with a Neutral
//! consequence reading "Nothing here constrains the agent" and [ADR-0011]'s
//! reading "Nothing here constrains MCP servers, which carry their own scopes.
//! The credential store governs those." A harness that picked a subset would be
//! authoring a security vocabulary, which [Autonomous development] puts on the
//! human side of the boundary — the same act the `credential-store` and
//! `notes-client` arcs each declined over D4's two spellings.
//!
//! A grant is not that. It does not decide what is safe; it decides what is
//! **declared**, which is a context-window question with the user's name on it.
//!
//! # Measured: only layer 2 can actually carry one, and no separator is invented
//!
//! The coordinator's ruling of 2026-09-15 names "the user, environment and
//! flag layers". The key is declared at all three — the project layer is the
//! only one [`ProjectPolicy::Refused`](crate::config::ProjectPolicy) — and
//! **measured, layers 4 and 5 cannot express a value for it.** A grant is a
//! list, an environment variable and a flag both arrive as text, and
//! [`FieldKind::coerce`] has no text-to-array arm: it answers `WrongShape`.
//!
//! This is not a gap this module could close by itself. It is the same limit
//! [ADR-0011] D3's own amendment already records for `tools.allowlist` —
//! "layers 4 and 5 cannot express a list because no record says how an
//! environment variable spells one and **inventing a separator would settle
//! that**" — and settling it here would decide the same question for every
//! array key in the catalogue on behalf of [ADR-0014]. So the declaration is
//! what the ruling asked for and the reachable surface is `~/.zaru/config.toml`
//! alone, said here rather than discovered by somebody whose
//! `ZARU_NOTES_PLAY_AGENT_TOOLS` did nothing. A check pins it, so deciding it
//! the other way reddens rather than passing unnoticed.
//!
//! # The project layer is closed to it
//!
//! [ADR-0014] D6: a repository the user cloned "must not be able to configure
//! its way to more privilege than the user granted". A grant is reach — it is
//! the set of remote tools a model may ask for — so a cloned repository
//! widening one is exactly that. The user, environment and flag layers set it;
//! layer 3 is [`ProjectPolicy::Refused`](crate::config::ProjectPolicy) and
//! names where it does belong, the shape `tools.allowlist` already has.
//!
//! # A name that is not in the token's own scope is refused
//!
//! Not silently dropped. A person who grants `pages.raed` has made a mistake
//! that would otherwise present as a model that never uses a tool they thought
//! they had turned on, and the token's cached `tools/list` is the list to check
//! it against — D6's cache, which "already reflects exactly what the token
//! grants, so the cache needs no interpretation". The refusal names the token's
//! own list rather than a guess, because that is the list the person can read
//! with `zaru notes tokens`.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [Autonomous development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::config::{Field, FieldKind, Key, Layer, Resolution, Schema, Value};
use crate::credentials::alias::Alias;
use core::fmt;

/// The first segment of every grant key.
pub const PREFIX: &str = "notes";

/// The last segment of every grant key.
pub const SUFFIX: &str = "agent_tools";

/// Why a project may not set a grant, in the words the refusal carries.
///
/// One string, read by [`field`] and by [`GrantRefused`], so the fold's
/// refusal and this module's cannot give a user two different reasons for one
/// rule.
pub const PROJECT_REFUSAL: &str = "which of your Nuclear Notes tools a model may be offered is \
                                   your own grant, and a repository you cloned must not be able \
                                   to widen what a model can reach in your cortex; set it in \
                                   ~/.zaru/config.toml instead";

/// The grant key for one alias.
///
/// # Panics
///
/// Never. [`Alias::new`] refuses every character [`Key::new`] does — a control
/// character, and a segment that is empty or surrounded by whitespace — so an
/// alias that exists cannot make an ill-formed key.
#[must_use]
pub fn key(alias: &Alias) -> Key {
    Key::new(&format!("{PREFIX}.{alias}.{SUFFIX}"))
        .expect("an alias that passed Alias::new is a well-formed key segment")
}

/// What a grant holds, and what the project layer may do to it.
///
/// A list, which [ADR-0014](https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy)
/// D2 replaces wholesale rather than merging — the right semantics here for
/// the reason it is right for `tools.allowlist`: a user who cannot say
/// "exactly these and nothing inherited" is fighting the configuration over
/// what a model may reach.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Array, PROJECT_REFUSAL)
}

/// Declare the grant family on a schema.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with_family(PREFIX, SUFFIX, field())
}

/// Why a grant could not be taken.
///
/// Every variant quotes back what the user wrote, because a configuration
/// value they typed is what they have to find in order to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantRefused {
    /// The value arrived from a layer ADR-0014 D6's escalation ceiling binds.
    FromAClonedRepository {
        /// Which alias.
        alias: Alias,
        /// Which layer supplied it.
        layer: Layer,
    },
    /// The key held something other than a list.
    WrongShape {
        /// Which alias.
        alias: Alias,
        /// What shape it held. **Never the value.**
        found: &'static str,
    },
    /// An entry was not text.
    EntryWrongShape {
        /// Which alias.
        alias: Alias,
        /// Which entry, counting from one.
        position: usize,
        /// What shape it held.
        found: &'static str,
    },
    /// A granted name is not one the token's cached scope carries.
    NotInTheTokensScope {
        /// Which alias.
        alias: Alias,
        /// What was granted.
        offered: String,
        /// What the token actually grants, as D6's cache reports it.
        cached: Vec<String>,
    },
}

impl fmt::Display for GrantRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromAClonedRepository { alias, layer } => write!(
                f,
                "`{}` was set by {}, and {PROJECT_REFUSAL}",
                key(alias),
                layer.label(),
            ),
            Self::WrongShape { alias, found } => write!(
                f,
                "`{}` holds {found}, and a grant is a list of tool names",
                key(alias),
            ),
            Self::EntryWrongShape {
                alias,
                position,
                found,
            } => write!(
                f,
                "entry {position} of `{}` is {found}, and a tool name is text",
                key(alias),
            ),
            Self::NotInTheTokensScope {
                alias,
                offered,
                cached,
            } => write!(
                f,
                "`{}` grants {offered:?}, which the token `{alias}` does not carry. It grants \
                 {} tool(s): {}",
                key(alias),
                cached.len(),
                cached.join(", "),
            ),
        }
    }
}

impl std::error::Error for GrantRefused {}

/// Which of one token's tools a person granted the model.
///
/// **Empty is the default and it declares nothing**, which is not the same as
/// absent: see the module documentation for the measurement behind it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Granted {
    names: Vec<String>,
}

impl Granted {
    /// A grant of nothing.
    #[must_use]
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Read one token's grant out of a resolved configuration, against the
    /// scope D6 cached for that token.
    ///
    /// The value and the layer that supplied it come from **one**
    /// [`Resolution::explain`] call, which is ADR-0014 D3's own trace, so the
    /// layer this refuses on and the layer `zaru config explain` would print
    /// cannot disagree — the shape
    /// [`Allowed::from_configuration`](crate::tools::Allowed::from_configuration)
    /// already has.
    ///
    /// # Errors
    ///
    /// [`GrantRefused`].
    pub fn from_configuration(
        resolution: &Resolution,
        alias: &Alias,
        cached: &[&str],
    ) -> Result<Self, GrantRefused> {
        let key = key(alias);
        let explanation = resolution.explain(&key);

        let (Some(value), Some(layer)) =
            (explanation.value.as_ref(), explanation.effective_layer())
        else {
            return Ok(Self::nothing());
        };

        if layer.bound_by_the_escalation_ceiling() {
            return Err(GrantRefused::FromAClonedRepository {
                alias: alias.clone(),
                layer,
            });
        }

        let Value::Array(items) = value else {
            return Err(GrantRefused::WrongShape {
                alias: alias.clone(),
                found: value.shape(),
            });
        };

        let mut names = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let Some(text) = item.as_text() else {
                return Err(GrantRefused::EntryWrongShape {
                    alias: alias.clone(),
                    position: index + 1,
                    found: item.shape(),
                });
            };
            if !cached.contains(&text) {
                return Err(GrantRefused::NotInTheTokensScope {
                    alias: alias.clone(),
                    offered: text.to_owned(),
                    cached: cached.iter().map(|name| (*name).to_owned()).collect(),
                });
            }
            names.push(text.to_owned());
        }
        Ok(Self { names })
    }

    /// The granted names, in the order the user wrote them.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Whether a name was granted.
    #[must_use]
    pub fn carries(&self, name: &str) -> bool {
        self.names.iter().any(|granted| granted == name)
    }

    /// Whether nothing was granted, in which case nothing is declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}
