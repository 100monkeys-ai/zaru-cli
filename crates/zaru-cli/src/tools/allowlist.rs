// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0011] D3's allowlist: what the user has pre-approved, read from the
//! one configuration layer that may grant it.
//!
//! # The key, and why it is spelled `tools.allowlist`
//!
//! [ADR-0014]'s Neutral section leaves each record its own keys, and this is
//! ADR-0011's — D2's enforcement row names "prompts and an allowlist" and D3
//! names "the allowlist". It is **not** `tools.allow`, because the permission
//! mode's own value is `allow` ([`Mode::Allow`](crate::tools::Mode)) and a
//! future `tools.mode = "allow"` sitting beside a `tools.allow` list would be
//! two different things a reader has one word for.
//!
//! **`tools.mode` joined it under that table on 2026-09-05**, which is the
//! future the paragraph above was written against, and the leaf-and-branch
//! collision [Verification lessons] §62 describes still cannot arise: §62's
//! collision is a *leaf against a branch* — one name held as both a value and
//! a table — and two sibling leaves under one table are not that. `tools`
//! itself is declared by nothing, which is the property that matters, and it
//! is the property the schema refuses to break rather than one this comment
//! remembers.
//!
//! # Which layer may set it, and the two arms that hold it
//!
//! [ADR-0011] D3 called it "the **project** allowlist" until 2026-09-05.
//! [ADR-0014] D6 says "A repository the user cloned must not be able to
//! configure its way to more privilege than the user granted", and an
//! allowlist honoured without prompting is a repository buying itself fewer
//! prompts. **D6 wins**, and D3 is amended to "the user's allowlist" by an
//! accepted Update of 2026-09-05 under Jeshua's directive of that day, open
//! to his veto. [ADR-0015] D4's per-project admission is the mechanism a
//! record would use to admit a project's list one day, and **it is not
//! built**.
//!
//! Two independent arms hold that, because one of them is a fold this type
//! does not run:
//!
//! 1. [`field`] declares the key
//!    [`Refused`](crate::config::ProjectPolicy::Refused) to the project
//!    layer, so `Resolution::resolve` refuses a `./zaru.toml` that sets it,
//!    naming the key and the reason, before any value exists.
//! 2. [`Allowed::from_configuration`] reads the effective layer off the same
//!    single [`Resolution::explain`] call the value comes from, and refuses
//!    any layer [`Layer::bound_by_the_escalation_ceiling`] binds. That is the
//!    predicate ADR-0014 D6 already has in this crate, asked rather than
//!    spelled a second time.
//!
//! The second arm is what makes the refusal true of *any* caller rather than
//! only of the fold, and each arm reddens on its own — see
//! `a_project_allowlist_is_refused_by_the_fold_and_by_the_constructor`.
//!
//! # Which layers can carry one at all, measured rather than assumed
//!
//! A list. [`FieldKind::coerce`](crate::config::FieldKind::coerce) carries
//! ADR-0014's own rule that **no kind parses text into a list**, "because no
//! record says how an environment variable expresses one, and inventing a
//! separator here would settle that". Layer 4 and layer 5 both supply
//! [`Value::Text`], so both are refused for this key by that rule and no
//! separator is invented to rescue them. Layer 1 could compile one in and the
//! `zaru` binary compiles none, because a pre-approval nobody wrote is the
//! opposite of a grant. Layer 3 is refused twice, above.
//!
//! **So in practice this key is layer 2's, `~/.zaru/config.toml`**, and that
//! sentence is on ADR-0011 D3 and ADR-0014 D1 rather than only here.
//!
//! # An entry is the line the prompt showed
//!
//! `"<tool> <target>"`, split at the first space: the tool is one of
//! [ADR-0011] D1's seven exactly, and the target is the rest, taken
//! literally. That is not a format invented beside the code — it is what
//! [`Decision::question`](crate::tools::Decision::question) already renders,
//! because the question is `Allow {}?` over
//! [`TranscriptEntry::render`](crate::tools::TranscriptEntry::render) and that
//! is `"{tool} {subject}"`. **A user pre-approves by writing down the line
//! they were shown.**
//!
//! Matching is byte-for-byte on both halves. **No glob, no prefix, no path
//! resolution and no normalisation of any kind.** A glob is a permission
//! taxonomy, and the platform agrees: AEGIS's own `allowed_subcommands` is a
//! list of exact strings, and what it globs is a tool *pattern* rather than a
//! subcommand. A pre-approval that matched more than the line the user read
//! would grant something they did not read.
//!
//! What that costs, stated rather than discovered: a path entry has to be the
//! resolved path
//! ([`Invocation::subject_text`](crate::tools::Invocation::subject_text)
//! canonicalises through D4), which is absolute. It is also exactly what the
//! prompt printed, so the cost is paid by copying rather than by knowing.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::config::{Field, FieldKind, Key, Layer, Resolution, Schema, Value};
use crate::failure::THERE_ARE_EXACTLY;
use crate::tools::decision::Invocation;
use crate::tools::name::{Called, ToolName};
use crate::tools::port::Allowlist;
use core::fmt;

/// The configuration key ADR-0011 D3's allowlist is read from.
///
/// Spelled here and nowhere else. See the module documentation for why it is
/// `tools.allowlist` rather than `tools.allow`.
pub const KEY: &str = "tools.allowlist";

/// Why a project may not set [`KEY`], in the words the refusal carries.
///
/// One string, read by [`field`] and by [`AllowlistRefused`], so the fold's
/// refusal and this module's cannot give a user two different reasons for one
/// rule. [ADR-0016] D2 wants an error whose reader can act, so it names where
/// the key does belong.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const PROJECT_REFUSAL: &str = "what the harness may run without asking is the user's own \
                                   grant, and a repository they cloned must not be able to grant \
                                   itself fewer prompts; set it in ~/.zaru/config.toml instead";

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("tools.allowlist is a well-formed key")
}

/// What [`KEY`] holds, and what the project layer may do to it.
///
/// A list, which [ADR-0014] D2 replaces wholesale rather than merging — the
/// right semantics for a grant, because a user who cannot say "exactly these
/// and nothing inherited" is fighting the configuration over a security
/// boundary, and D2's own reason is that "the failure is silent because the
/// inherited entries look plausible".
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Array, PROJECT_REFUSAL)
}

/// Declare ADR-0011's configuration key into a caller's schema.
///
/// The shape [`crate::providers::declare`] and [`crate::manifest::declare`]
/// already use, so a caller building a schema asks each record for its own
/// keys rather than transcribing them.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

/// Why an allowlist could not be taken.
///
/// Every variant quotes back what it was handed, because a configuration
/// value a user wrote is what they have to find in order to fix it. None can
/// carry a value from anywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowlistRefused {
    /// The value arrived from a layer ADR-0014 D6's escalation ceiling binds.
    ///
    /// The second of the two arms in the module documentation. The first is
    /// the fold's, and this one is reached by a caller who obtained a
    /// resolution some other way.
    FromAClonedRepository {
        /// Which layer supplied it.
        layer: Layer,
    },
    /// The key held something other than a list.
    WrongShape {
        /// What shape it held. **Never the value.**
        found: &'static str,
    },
    /// An entry was not text.
    EntryWrongShape {
        /// Which entry, counting from one as a reader would.
        position: usize,
        /// What shape it held. **Never the value.**
        found: &'static str,
    },
    /// An entry named no tool and no target.
    EmptyEntry {
        /// Which entry, counting from one.
        position: usize,
    },
    /// An entry named a tool and stopped.
    NoTarget {
        /// Which entry, counting from one.
        position: usize,
        /// The entry as it was written, escaped.
        offered: String,
    },
    /// An entry's first word is not one of ADR-0011 D1's seven built-ins.
    NoSuchTool {
        /// Which entry, counting from one.
        position: usize,
        /// The word that was offered, escaped.
        offered: String,
    },
}

impl fmt::Display for AllowlistRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromAClonedRepository { layer } => write!(
                f,
                "the key {KEY} was set in {}, and the allowlist is not that layer's to set: \
                 {PROJECT_REFUSAL}. A repository the user cloned must not be able to configure \
                 its way to more privilege than the user granted",
                layer.label()
            ),
            Self::WrongShape { found } => write!(
                f,
                "{KEY} holds {found}, and an allowlist is a list of entries each naming one tool \
                 and one target"
            ),
            Self::EntryWrongShape { position, found } => write!(
                f,
                "entry {position} of {KEY} holds {found}, and an allowlist entry is text: the line \
                 the prompt showed you, such as \"fs.read /home/you/project/src/main.rs\""
            ),
            Self::EmptyEntry { position } => write!(
                f,
                "entry {position} of {KEY} is empty, so it names neither a tool nor a target"
            ),
            Self::NoTarget { position, offered } => write!(
                f,
                "entry {position} of {KEY} is {offered:?}, which names a tool and no target. An \
                 entry is the line the prompt showed you — the tool, a space, and what it was \
                 addressed to"
            ),
            Self::NoSuchTool { position, offered } => write!(
                f,
                "entry {position} of {KEY} begins with {offered:?}, which names no built-in tool. \
                 {THERE_ARE_EXACTLY} seven: {}",
                spellings()
            ),
        }
    }
}

impl std::error::Error for AllowlistRefused {}

/// Every built-in's name, listed as a refusal lists them.
///
/// Walked from [`ToolName::ALL`] rather than typed out, so an eighth built-in
/// appears in this refusal the moment it is declared ([Verification lessons]
/// §17).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn spellings() -> String {
    let quoted: Vec<String> = ToolName::ALL
        .iter()
        .map(|tool| format!("{:?}", tool.as_str()))
        .collect();
    quoted.join(", ")
}

/// One pre-approved call: a tool, and the target it was approved for.
///
/// The pair is the unit rather than either half, for the reason
/// [`Allowlist::approves`] takes the whole invocation: "a user who approved
/// reading one path has said nothing about running a command".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    tool: Called,
    target: String,
}

impl Entry {
    /// Take one entry, as the user wrote it.
    ///
    /// `position` is only carried into a refusal, so a reader can find the
    /// line in their own file.
    ///
    /// # Errors
    ///
    /// [`AllowlistRefused`], naming the entry's position and quoting what it
    /// held.
    pub fn parse(position: usize, offered: &str) -> Result<Self, AllowlistRefused> {
        if offered.is_empty() {
            return Err(AllowlistRefused::EmptyEntry { position });
        }
        let Some((word, target)) = offered.split_once(' ') else {
            return Err(AllowlistRefused::NoTarget {
                position,
                offered: offered.escape_debug().to_string(),
            });
        };
        let Some(tool) = ToolName::ALL.into_iter().find(|tool| tool.as_str() == word) else {
            return Err(AllowlistRefused::NoSuchTool {
                position,
                offered: word.escape_debug().to_string(),
            });
        };
        if target.is_empty() {
            return Err(AllowlistRefused::NoTarget {
                position,
                offered: offered.escape_debug().to_string(),
            });
        }
        Ok(Self {
            tool: Called::Builtin(tool),
            target: target.to_owned(),
        })
    }

    /// One entry from a tool and a target the harness already holds.
    ///
    /// **Not a parse.** [`Entry::parse`] takes what a *user* wrote and refuses
    /// what is not an entry; this takes a pair the harness composed and cannot
    /// fail. It exists for
    /// [`SessionGrants`](crate::tools::grants::SessionGrants), so a grant made
    /// at the prompt is the same value an allowlist line becomes and is
    /// compared by the same [`Entry::approves`] — rather than a second
    /// matching rule that could come to disagree with D3's.
    #[must_use]
    pub fn of(tool: Called, target: String) -> Self {
        Self { tool, target }
    }

    /// What this entry approves.
    #[must_use]
    pub const fn called(&self) -> &Called {
        &self.tool
    }

    /// Which built-in this entry approves, if it names one.
    ///
    /// Always `Some` for an entry a user wrote: [`Entry::parse`] refuses a
    /// name that is not one of ADR-0011 D1's seven, and it still does. **A
    /// projected tool is therefore in no allowlist anybody can write**, which
    /// is not an omission — D3's allowlist is "what the harness may run
    /// without asking", and a grant that ran a call into somebody's cortex
    /// without asking is a decision no record has taken. The session grant,
    /// D3's third answer, is the route that exists: the user answering this
    /// exact question for this exact line.
    #[must_use]
    pub const fn tool(&self) -> Option<ToolName> {
        self.tool.builtin()
    }

    /// What it approves that built-in for.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Whether this entry approves that call.
    ///
    /// Both halves compared byte for byte. See the module documentation for
    /// why there is no glob and no path resolution here.
    #[must_use]
    pub fn approves(&self, invocation: &Invocation<'_>) -> bool {
        &self.tool == invocation.called() && self.target == invocation.subject_text()
    }
}

/// What the user has pre-approved, for ADR-0011 D3's `allow` mode.
///
/// The product implementation of [`Allowlist`]. Before 2026-09-05 that trait
/// had none anywhere in this workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowed {
    entries: Vec<Entry>,
}

impl Allowed {
    /// An allowlist that approves nothing.
    ///
    /// What a machine with no `~/.zaru/config.toml` has, and what
    /// [`Allowed::from_configuration`] returns when no layer set the key. It
    /// is a grant of nothing rather than an absence, because ADR-0011 D3's
    /// `allow` mode prompts for everything outside the list and an empty list
    /// is therefore the same as `ask` for anything with an effect.
    #[must_use]
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Read the allowlist out of a resolved configuration.
    ///
    /// The value and the layer that supplied it come from **one**
    /// [`Resolution::explain`] call, which is ADR-0014 D3's own trace, so the
    /// layer this refuses on and the layer `zaru config explain
    /// tools.allowlist` would print cannot disagree.
    ///
    /// # Errors
    ///
    /// [`AllowlistRefused`]. `FromAClonedRepository` is the second of D6's
    /// two arms; the rest name an entry the user has to fix.
    pub fn from_configuration(resolution: &Resolution) -> Result<Self, AllowlistRefused> {
        let key = key();
        let explanation = resolution.explain(&key);

        let (Some(value), Some(layer)) =
            (explanation.value.as_ref(), explanation.effective_layer())
        else {
            return Ok(Self::nothing());
        };

        // ADR-0014 D6's escalation ceiling, asked of the predicate that
        // already holds it rather than spelled again here. This is the arm
        // the fold does not run: a caller holding a resolution built some
        // other way reaches it and the fold's refusal does not.
        if layer.bound_by_the_escalation_ceiling() {
            return Err(AllowlistRefused::FromAClonedRepository { layer });
        }

        let Value::Array(items) = value else {
            return Err(AllowlistRefused::WrongShape {
                found: value.shape(),
            });
        };

        let mut entries = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let position = index + 1;
            let Some(text) = item.as_text() else {
                return Err(AllowlistRefused::EntryWrongShape {
                    position,
                    found: item.shape(),
                });
            };
            entries.push(Entry::parse(position, text)?);
        }
        Ok(Self { entries })
    }

    /// Every entry, in the order the user wrote them.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    /// How many entries the user granted.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the user granted nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Allowlist for Allowed {
    fn approves(&self, invocation: &Invocation<'_>) -> bool {
        self.entries.iter().any(|entry| entry.approves(invocation))
    }
}
