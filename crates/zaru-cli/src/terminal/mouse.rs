// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `terminal.mouse`: whether the shell holds the mouse, an [ADR-0014] key.
//!
//! # What holding the mouse buys, and what it costs
//!
//! The shell asks the terminal for its buttons (`driver::arm`) so that the
//! wheel scrolls the pane. Without that, Windows Terminal and the VS Code
//! terminal turn a wheel notch on the alternate screen into `Up` or `Down`,
//! and those keys walk the history on an empty prompt. That was pull request
//! #4's reason, and it is the reason this defaults to `true`.
//!
//! The cost is the terminal's plain click-and-drag selection. A terminal
//! that reports its buttons stops selecting with them, and gives the
//! selection back only while its bypass modifier is held: Shift in Windows
//! Terminal, in the VS Code terminal off macOS, and in most others.
//! `compose::tips::Tip::Selection` says so.
//!
//! **`terminal.mouse = false` makes the opposite trade**, as Claude Code's
//! `CLAUDE_CODE_DISABLE_MOUSE=1` does. No mouse mode is requested, selection
//! is the terminal's own, the selection tip is not offered, and the wheel
//! walks the history in the two terminals named above. That cost is written
//! here rather than discovered.
//!
//! # Why a project cannot set it
//!
//! How a person's own terminal behaves is theirs. A repository they cloned
//! should not decide whether their mouse selects, which is the argument
//! [`crate::tools::mode::PROJECT_REFUSAL`] makes for how much the harness
//! prompts.
//!
//! Decided 2026-09-27 as a delegated coordinator ruling on [ADR-0005]'s
//! amendments, open to Jeshua's veto.
//!
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

use crate::config::{Field, FieldKind, Key, Resolution, Schema, Value};

/// The key, spelled here and nowhere else.
pub const KEY: &str = "terminal.mouse";

/// What [ADR-0014] D1's layer 1 compiles in.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
pub const BUILT_IN: bool = true;

/// Why the project layer may not set [`KEY`]. It names where the key *does*
/// belong, because [ADR-0016] D2 wants an error whose reader can act.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
pub const PROJECT_REFUSAL: &str = "whether the harness holds the mouse is the user's own choice \
                                   about their own terminal, and a repository they cloned must \
                                   not make it; set it in ~/.zaru/config.toml or \
                                   ZARU_TERMINAL_MOUSE instead";

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("terminal.mouse is a well-formed key")
}

/// What [`KEY`] holds: a boolean the project layer may not set.
#[must_use]
pub fn field() -> Field {
    Field::refused_to_projects(FieldKind::Bool, PROJECT_REFUSAL)
}

/// Declare [`KEY`] into a caller's schema, in the shape
/// [`crate::tools::mode::declare`] already uses.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

/// Whether the shell holds the mouse, from a resolved configuration.
///
/// Layer 1 always supplies [`BUILT_IN`], so a resolution without the key is
/// one that did not come from `cli::layers`. It reads as the built-in value
/// rather than as a second default of its own.
#[must_use]
pub fn held(resolution: &Resolution) -> bool {
    match resolution.get(&key()) {
        Some(Value::Bool(held)) => *held,
        _ => BUILT_IN,
    }
}
