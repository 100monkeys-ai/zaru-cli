// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The seven built-in tools ADR-0011 D1 names, and what each one does to the
//! world.
//!
//! # The set is closed, and that is the security posture
//!
//! D1: "Everything beyond this is an MCP server. The built-in set stays small
//! because each entry is a capability with no membrane behind it at `bare`."
//! Alternative 3 on that record puts it plainly: "Small is the security
//! posture, not an ergonomic compromise."
//!
//! So the set is an enum with seven variants and no way to build an eighth.
//! [Testing]'s form of this is that **the forbidden reach has nothing to
//! call** — absence rather than refusal. It is the same shape `zaru-tui`'s
//! `Scope` uses for ADR-0005 D6's exclusion of `all_public`: an eighth
//! variant stops the exhaustive match in
//! `the_built_in_set_is_the_seven_adr_0011_d1_names` compiling, which is a
//! louder signal than any assertion.
//!
//! # One of the seven names is not what D1 says it is
//!
//! D1: "Names match the AEGIS built-in dispatchers deliberately. An engineer
//! who learns `cmd.run` locally already knows what it is called in a
//! manifest." Six of the seven do. **`fs.search` does not exist on the
//! platform.** [ADR-048] maps `search.grep` to `fs.grep` and `search.glob` to
//! `fs.glob`, two tools; `grep` over `aegis-orchestrator` on 2026-09-04
//! returned exactly twelve built-in names — `cmd.run`, `fs.create_dir`,
//! `fs.delete`, `fs.edit`, `fs.glob`, `fs.grep`, `fs.list`, `fs.multi_edit`,
//! `fs.read`, `fs.write`, `web.fetch`, `web.search` — and `fs.search` is not
//! among them. D1's own row for it, "Content and filename search", is those
//! two platform tools merged.
//!
//! **The record's spellings are kept here regardless**, because the record is
//! what this code holds, and whether `fs.search` should be renamed, split, or
//! left alone is a question for the record's author with three different
//! answers. It is recorded as a proposed Update on ADR-0011 rather than
//! settled by whichever spelling an implementer preferred.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [ADR-048]: https://100monkeys-ai.cortex.page/aegis-architecture/p/adrs/048-core-mcp-tools-implementation

use core::fmt;

/// What a tool does to the world.
///
/// The first three words are ADR-0011's own: D1's table says a tool reads,
/// creates or overwrites, replaces within, lists, searches, executes, or
/// retrieves, and D3's `ask` mode "prompts before any **write or command**".
/// [`Effect::prompts_in_ask`] is where that sentence is held, and it is the
/// only place the distinction is made.
///
/// `Retrieve` is a fourth category rather than a write or a command because
/// D3's sentence admits only two and `web.fetch` is neither. What follows
/// from that is surprising enough to be recorded on the record rather than
/// left for a reader to discover from behaviour — see
/// [`Effect::prompts_in_ask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads the filesystem and changes nothing.
    Read,
    /// Creates, overwrites, or alters a file.
    Write,
    /// Executes a shell command.
    Command,
    /// Retrieves a URL.
    Retrieve,
}

impl Effect {
    /// Whether ADR-0011 D3's `ask` mode prompts before this effect.
    ///
    /// D3: "`ask` — Prompts before any write or command. Default."
    ///
    /// # This is a literal reading, and one consequence of it is a finding
    ///
    /// `web.fetch` is neither a write nor a command, so at the **default**
    /// mode a URL the model chose is retrieved with no prompt. That may be
    /// what D3 means — the built-in set is small and `web.fetch` reaches
    /// nothing on the user's disk — or "write or command" may be shorthand
    /// for "anything with a side effect or an outbound reach". The record
    /// does not say, so the literal reading is built and the question is
    /// recorded as open rather than answered by an implementer's instinct.
    ///
    /// Note that this is not the whole prompting rule: ADR-0011 D4 makes
    /// out-of-tree access prompt in `ask` and `allow` whatever the effect is,
    /// so a *read* outside the working directory still prompts.
    #[must_use]
    pub const fn prompts_in_ask(self) -> bool {
        matches!(self, Self::Write | Self::Command)
    }
}

/// One of the seven built-in tools ADR-0011 D1 names.
///
/// There is no eighth and no way to make one. See the module documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolName {
    /// Read a file.
    FsRead,
    /// Create or overwrite a file.
    FsWrite,
    /// Replace an exact string within a file.
    FsEdit,
    /// List a directory.
    FsList,
    /// Content and filename search.
    FsSearch,
    /// Execute a shell command.
    CmdRun,
    /// Retrieve a URL.
    WebFetch,
}

impl ToolName {
    /// Every built-in ADR-0011 D1 names, in the order the record's table
    /// gives them.
    ///
    /// A hand-written list, guarded by the exhaustive match in
    /// `the_built_in_set_is_the_seven_adr_0011_d1_names`: adding a variant
    /// fails to compile there, which is the signal that D1 is being changed
    /// rather than extended, and the length assertion beside it catches a
    /// variant added alongside a widened `ALL`.
    pub const ALL: [Self; 7] = [
        Self::FsRead,
        Self::FsWrite,
        Self::FsEdit,
        Self::FsList,
        Self::FsSearch,
        Self::CmdRun,
        Self::WebFetch,
    ];

    /// The tool's name as ADR-0011 D1 spells it.
    ///
    /// Transcribed from the record rather than derived from the variant, so
    /// that a renamed variant cannot silently rename the wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::FsEdit => "fs.edit",
            Self::FsList => "fs.list",
            Self::FsSearch => "fs.search",
            Self::CmdRun => "cmd.run",
            Self::WebFetch => "web.fetch",
        }
    }

    /// What ADR-0011 D1's second column says this tool does.
    ///
    /// Transcribed from the record's table rather than written here, for the
    /// reason [`ToolName::as_str`] is: it is what a model is told the tool is
    /// for, and a description invented beside the code is a contract nobody
    /// decided.
    #[must_use]
    pub const fn purpose(self) -> &'static str {
        match self {
            Self::FsRead => "Read a file",
            Self::FsWrite => "Create or overwrite a file",
            Self::FsEdit => "Replace an exact string within a file",
            Self::FsList => "List a directory",
            Self::FsSearch => "Content and filename search",
            Self::CmdRun => "Execute a shell command",
            Self::WebFetch => "Retrieve a URL",
        }
    }

    /// What this tool does to the world.
    #[must_use]
    pub const fn effect(self) -> Effect {
        match self {
            Self::FsRead | Self::FsList | Self::FsSearch => Effect::Read,
            Self::FsWrite | Self::FsEdit => Effect::Write,
            Self::CmdRun => Effect::Command,
            Self::WebFetch => Effect::Retrieve,
        }
    }

    /// Whether this tool addresses something in the filesystem.
    ///
    /// ADR-0011 D4's working-directory boundary is about paths, and
    /// `web.fetch` addresses a URL rather than a path. A URL allowlist is a
    /// separate question that D4 does not raise and no record answers, so
    /// nothing here classifies one.
    #[must_use]
    pub const fn addresses_a_path(self) -> bool {
        !matches!(self, Self::WebFetch)
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
