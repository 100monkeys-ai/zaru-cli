// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The terminal `zaru_tui::shell` renders into, and the adapters it reaches
//! this crate through.
//!
//! # Why the driver is here and the shell is there
//!
//! [ADR-0003] D8 gives `zaru-tui` exactly one sibling edge, `zaru-core`, so
//! everything the shell must dispatch or render — [ADR-0015] D2's closed
//! namespace set, [ADR-0014] D5's nearest match, [ADR-0010] D2's transcript
//! shapes, [ADR-0011] D3's confirmation — is unreachable from there. The shell
//! declares three ports; this module implements all three and owns the
//! terminal itself. That is [Bounded Contexts]' own sentence: "The in-session
//! half is `zaru-tui`'s when that crate has a terminal."
//!
//! # The backend arrives through `ratatui`'s own feature
//!
//! Everything crossterm is reached as `ratatui::crossterm`, so no `crossterm`
//! line appears in any manifest and [ADR-0003] D2 needs no new row — the shape
//! that record already blessed for `rmcp`'s streamable-HTTP transport. What it
//! did need was `scripts/check-crate-boundaries.py`, which refused `mio` and
//! `rustix` in `zaru-tui`'s closure by any route and now refuses them by every
//! route except this one.
//!
//! # Restoring the terminal is a `Drop`, not a call at the end
//!
//! A restore written at the end of the loop is a restore that does not happen
//! when the loop returns early, and a panic leaves the user's terminal in raw
//! mode with no echo — which is [ADR-0016] D3's defect arriving in the worst
//! possible register, because the user cannot read the report. [`Guard`] holds
//! the restorer and gives it back on drop, so every exit path restores, and
//! `ratatui::init` installs a panic hook that restores as well. Both are
//! checkable without a terminal: [`Restore`] is a port and
//! `the_terminal_is_restored_when_the_shell_panics` catches an unwind and
//! counts.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

pub mod driver;
pub mod open;
pub mod source;
pub mod trie;
pub mod vocabulary;

pub use driver::{
    AfterTurn, Asked, Guard, KEY_IS_STORED, Pump, Pumped, Restore, SECRET_DECLINED,
    SECRET_GUIDANCE, Surface, after, ask_for_a_secret, question_for_the_shell, run, secret_for,
    secret_statement, switch_for,
};
pub use open::{Opening, resolve_in, restored_context, shell_for, take_over};
pub use source::{Beat, POLL, Pace, Source, TICK, Taken};
pub use trie::{NOTHING_CACHED, NotesTrie};
pub use vocabulary::{Transcript, Vocabulary};

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod tests;
