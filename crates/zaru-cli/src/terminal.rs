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
//! # The backend arrives through `ratatui`'s own backend crate
//!
//! Everything crossterm is reached as `ratatui_crossterm::crossterm`, the
//! backend crate `ratatui` 0.30 split out of itself, so no `crossterm` line
//! appears in any manifest. Until 2026-09-28 it was `ratatui::crossterm`,
//! through `ratatui`'s `crossterm` feature; that feature turns the backend's
//! `underline-color` on, so the backend crate is taken directly with its
//! defaults off, recorded on [ADR-0003]'s amendment for the move. What it
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
//! the restorer and gives it back on drop, so every path that runs `Drop`
//! restores: an ordinary exit, an early return, and an unwind. `ratatui::init`
//! installs a panic hook that restores as well. Both are checkable without a
//! terminal: [`Restore`] is a port and
//! `the_terminal_is_restored_when_the_shell_panics` catches an unwind and
//! counts.
//!
//! **A signal runs no `Drop`**, so that sentence was false of a session a
//! signal ended until 2026-09-27. `open` now takes `SIGTERM`, `SIGINT` and
//! `SIGHUP` and gives the terminal back before exiting with `128 + n`, and
//! `a_session_ended_by_a_signal_gives_the_terminal_back` holds it on the real
//! binary in a pseudo-terminal. **`SIGKILL` is the one ending that still
//! leaves the terminal as it was**, because no process can catch it.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Bounded Contexts]: https://100monkeys-ai.cortex.page/zaru/p/architecture/bounded-contexts

pub mod corpus;
pub mod driver;
pub mod mouse;
pub mod open;
pub mod paths;
pub mod source;
pub mod trie;
pub mod vocabulary;

pub use corpus::{CORPUS_FILE, CachedCorpus, CorpusCache, CorpusError};
pub use driver::{
    Added, AfterTurn, Asked, Asking, Guard, KEY_IS_STORED, NOTES_STRIP_HAS_NO_SINGLE_TOKEN,
    NOTES_TOKEN_IS_STORED, Pump, Pumped, Restore, SECRET_DECLINED, SECRET_GUIDANCE, Surface,
    add_a_notes_token, after, apex_statement, ask_for_a_secret, notes_looking,
    question_for_the_shell, run, secret_for, secret_statement, switch_for,
};
pub use open::{
    Opening, Populating, Refresh, refresh_from, resolve, restored_context, shell_for, take_over,
};
pub use paths::{NOTHING_TO_OFFER, ProjectPaths, WALK_CEILING};
pub use source::{Beat, POLL, Pace, Source, TICK, Taken};
pub use trie::{FROM_CACHE, LOOKING, NOTHING_CACHED, NotesTrie, UNREACHABLE};
pub use vocabulary::{Transcript, Vocabulary};

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod tests;
