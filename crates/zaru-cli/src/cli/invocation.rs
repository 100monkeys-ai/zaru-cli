// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the user asked for, typed.
//!
//! # Why the name is `CommandLine`
//!
//! The obvious names are taken, both by records this one has to sit beside.
//! `Invocation` is [ADR-0011] D1's *tool call* — [`crate::tools::Invocation`]
//! — and `Command` is [ADR-0015] D1's *extension kind*, "a named prompt
//! template with arguments", which is a thing this harness will one day load
//! from a file and which is emphatically not this. A second type of either
//! name in one crate is the collision [Ubiquitous Language] exists to prevent,
//! and that page's own rule is that the newcomer is the one that qualifies.
//! Named 2026-09-05 under a delegated coordinator ruling, open to Jeshua's
//! veto, with the vocabulary rows written in the same change.
//!
//! # Nothing here is a string the rest of the program reparses
//!
//! A [`Request`] carries values that are already what they will be used as: a
//! [`Key`] the configuration hierarchy can be asked about, a [`SessionId`] the
//! session store can be asked for. Whatever the parser could validate, it did.
//!
//! **[`Overrides`] is the exception and it is deliberate.** `--runtime` and
//! `--model` carry raw text, because their destination is [ADR-0014] D1's
//! layer 5 and a layer's values arrive as [`Value::Text`](crate::config::Value)
//! and are coerced by the schema during the fold — which is exactly what
//! [`crate::config::environment`] does for layer 4. Validating a tier here
//! would produce a *second* refusal for a bad `--runtime`, beside
//! [`TierRefused::NoSuchTier`](crate::runtime::TierRefused), and the second
//! one would not be able to name the layer the value came from.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [Ubiquitous Language]: https://100monkeys-ai.cortex.page/zaru/p/architecture/ubiquitous-language

use crate::config::Key;
use crate::credentials::Alias;
use crate::providers::ProviderKind;
use crate::session::SessionId;

/// What [ADR-0014] D1's layer 5 was told, before it becomes a layer.
///
/// **Two fields and no third.** D1 spells layer 5 "`--tier`, `--model`, and
/// the rest", and the rest does not exist: every other flag this surface takes
/// is a request rather than a setting. A flag that set a third key would add a
/// field here and would have to name the key it sets.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `--runtime <tier>`, destined for `runtime.tier`. [ADR-0001] D2.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    pub tier: Option<String>,
    /// `--model <identifier>`, destined for `model.default`.
    ///
    /// [ADR-0012] D4's own layered example ends `→ flag  --model`, and its D1
    /// says configuration names aliases rather than models — so the flag
    /// supplies the *identifier* for one alias, and the alias it supplies is
    /// `default`, which D2 calls "the one nearly everything uses". Recorded
    /// 2026-09-05 as a delegated coordinator ruling; a flag naming another
    /// alias would need a spelling no record gives.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    pub model: Option<String>,
    /// `--mode <mode>`, destined for `tools.mode`. [ADR-0011] D3.
    ///
    /// The value is not parsed here. It travels to layer 5 as text, exactly as
    /// `--runtime` does, so `--mode fast` is refused by [`crate::tools::mode`]
    /// naming the layer rather than by the parser with no layer to name.
    ///
    /// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
    pub mode: Option<String>,
}

impl Overrides {
    /// Whether any flag set anything.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tier.is_none() && self.model.is_none() && self.mode.is_none()
    }
}

/// What the user asked the binary to do.
///
/// A closed set with no wildcard match anywhere, so a tenth request cannot
/// arrive without `main`, `--help` and the renderer each answering for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `zaru --help`.
    Help,
    /// `zaru`, with no arguments at all.
    ///
    /// # One request, and the reader decides what it means
    ///
    /// **A bare `zaru` was [`Request::Help`] until 2026-09-06**, and both
    /// readers got the usage. [ADR-0015] D2's flag-surface contract gains an
    /// accepted Update under directive 25: `--help` is unchanged and still
    /// lists exactly what runs, and a bare `zaru` is not `--help` — it is the
    /// request to be in a session. At a terminal a new session's shell opens;
    /// through a pipe the usage is printed and nothing is minted.
    ///
    /// **The parser does not make that decision**, which is why this is one
    /// request rather than two. `cli::parse::parse` takes an iterator
    /// precisely so it reads no process state, and a parser that called
    /// `isatty` would be a second reader of the terminal beside
    /// [`crate::terminal::open`]'s. The test lives where [ADR-0010] D4's
    /// two-readers ruling already put it.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    /// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
    Session,
    /// `zaru --version`.
    Version,
    /// `zaru runtime` — [ADR-0001] D2's datum.
    ///
    /// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
    Runtime,
    /// `zaru config explain <key>` — [ADR-0014] D3's block, for one key.
    ///
    /// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
    ConfigExplain {
        /// The key, already validated by [`Key::new`].
        key: Key,
    },
    /// `zaru models` — [ADR-0012] D4's listing.
    ///
    /// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
    Models,
    /// `zaru init` — [ADR-0009] D6's writer, the one thing on this surface
    /// that changes a file the user owns.
    ///
    /// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
    Init,
    /// `zaru sessions list`.
    SessionsList,
    /// `zaru sessions rm <id>` — [ADR-0010] D6.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    SessionsRemove {
        /// The id, already validated by [`SessionId::parse`].
        id: SessionId,
    },
    /// `zaru providers keys` — which providers this machine holds a key for.
    ///
    /// A sibling of [`Request::NotesTokens`] rather than a widening of it:
    /// [ADR-0007] D7's listing is Nuclear Notes tokens', and every column it
    /// prints -- workspace, tool count, instance, composer role -- is a Notes
    /// token's. The two listings share one projection so that "never a value"
    /// is one rule rather than two.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    ProviderKeys,
    /// `zaru providers keys add <kind>` — store a provider's key.
    ///
    /// **The key is read from standard input and never from an argument.** An
    /// argument is in the shell's history file, in `/proc/<pid>/cmdline`, and
    /// in the output of `ps` for every user on the machine for as long as the
    /// process runs.
    ProviderKeysAdd {
        /// Which provider the key authenticates against.
        kind: ProviderKind,
    },
    /// `zaru notes tokens` — [ADR-0007] D7's listing.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesTokens,
    /// `zaru notes use <alias>` — [ADR-0007] D7's fifth surface, "move the
    /// composer role to another token".
    ///
    /// # It could only ever refuse, and now it moves
    ///
    /// That clause's own Status tracking recorded why this was the last of the
    /// five to be built: it "maps to `grant_composer_role`, which could only
    /// ever refuse, because nothing in this harness can put a token in the
    /// store for the role to move to". `add` arrived on 2026-09-06 and the
    /// second half of that sentence stopped being true.
    ///
    /// **And the first half was a trap.** `grant_composer_role` refuses
    /// whenever any token holds the role, so a `use` built on it succeeds at
    /// most once on a machine, ever. D7's word is "move", and this is a move:
    /// `CredentialStore::move_composer_role`, one store write, with every
    /// refusal decided before the first field changes so a refused move leaves
    /// the incumbent holding the role.
    ///
    /// **The first half is still true of every token that exists**, and that
    /// is the honest surface rather than a defect: the store refuses the role
    /// to a token whose cached scope reaches outside [ADR-0006] D4's set, and
    /// every Nuclear Notes token measured on 2026-09-14 grants 94 tools. So
    /// what a person running this sees today is a refusal **naming the tool**
    /// that put the token outside the set. That is the store holding D4
    /// correctly, and the command exists so that the refusal is something a
    /// person can read rather than a code path nothing reaches.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesUse {
        /// Which stored token is to carry the role.
        alias: Alias,
    },
    /// `zaru notes tokens add <alias> <host> [apex]` — [ADR-0007] D7's
    /// `add`, the second of that clause's five surfaces to exist.
    ///
    /// **The token is read from standard input**, for the reason
    /// [`Request::ProviderKeysAdd`] gives: an argument is in the shell's
    /// history file, in `/proc/<pid>/cmdline`, and in `ps` for every user on
    /// the machine for as long as the process runs.
    ///
    /// `host` is where the instance serves and is **not** the same question as
    /// `apex`. The host says where to connect; [ADR-0007] D8's reach says what
    /// the credential may cross once connected, and it is instance-locked
    /// "unless the user explicitly chooses otherwise" — so the choice is a word
    /// the user types rather than a default anything infers.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesTokensAdd {
        /// The local name this credential is known by. [ADR-0007] D2's
        /// `alias`, already validated.
        alias: Alias,
        /// The instance host to authenticate against.
        host: String,
        /// Whether the user declared this credential to have no instance
        /// boundary. [ADR-0007] D8.
        apex: bool,
    },
    /// `zaru notes tokens describe <alias> <text…>` — [ADR-0007] D7's
    /// `describe`, "set or edit the description".
    ///
    /// # The text is every remaining word, joined with one space
    ///
    /// A description is prose and every other argument on this surface is a
    /// token, so this is the one command whose last argument is a sentence.
    /// Out of session the words arrive as argv, already split by the shell;
    /// inside one they arrive as a typed line split on whitespace. Both are
    /// joined the same way, so `describe play my work token` means the same
    /// thing typed at either place and quoting is optional out of session.
    ///
    /// **What the two cannot agree on is a quoted run of interior whitespace**
    /// — `zaru … describe play "a  b"` keeps both spaces and no terminal line
    /// can express that at all. Said here rather than normalised away, because
    /// normalising would be a rule nobody decided.
    ///
    /// The text is not validated here. [`crate::credentials::Description`] is
    /// what refuses a control character, at the store's door, where the same
    /// rule already governs what `add` writes.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesTokensDescribe {
        /// Which stored token is being described.
        alias: Alias,
        /// D2's `description`, as the user typed it.
        text: String,
    },
    /// `zaru notes tokens rm <alias>` — [ADR-0007] D7's `rm`.
    ///
    /// It removes the credential and its sealed secret in one store write,
    /// and it removes the token carrying D4's composer role if that is the one
    /// named: revoking a credential is the person's to do. The outcome says
    /// when the role has gone with it, because the listing afterwards cannot
    /// — the row that would have shown it is the row that was removed.
    ///
    /// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
    NotesTokensRemove {
        /// Which stored token is to go.
        alias: Alias,
    },
    /// `zaru providers keys rm <kind>` — the provider half of D7's `rm`.
    ///
    /// The kind rather than an alias, because a provider key's alias is
    /// `provider.<kind>` and is composed rather than chosen — one key per
    /// kind, which is what the 2026-09-05 accepted Update settled. `zaru
    /// providers keys` is the listing it answers to.
    ProviderKeysRemove {
        /// Which provider's key is to go.
        kind: ProviderKind,
    },
    /// `zaru --resume <id>` — [ADR-0010] D4.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    Resume {
        /// The id, already validated.
        id: SessionId,
    },
    /// `zaru --continue` — D4's most recent session in this directory.
    Continue,
    /// Anything else: what the user wants done, which needs a provider.
    Task {
        /// The **task words**, as the user typed them, in order.
        ///
        /// Held rather than discarded so the refusal can quote what it could
        /// not run, and so that the day a provider exists this is the value
        /// the loop is handed. Never re-parsed as a command: whether a line is
        /// a command or a task was decided by the parser and is not decided
        /// twice.
        words: Vec<String>,
    },
}

/// One whole command line, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    /// What layer 5 was told.
    pub overrides: Overrides,
    /// What to do.
    pub request: Request,
}
