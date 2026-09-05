// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Child processes: the one implementation of running a command on the user's
//! machine, and the two ports it answers.
//!
//! # This is the first thing in the workspace that acts on the world outside
//! a file
//!
//! Until this module, `Command::new` appeared in no product tree at all. Two
//! ports had been waiting for it since 2026-09-04 and each was declared with
//! the reason its implementation did not exist yet:
//! [`ValidatorRunner`](zaru_core::iteration::validator::ValidatorRunner), whose
//! own documentation says "its product implementation is a subprocess and
//! belongs in `zaru-cli`", and [`Subprocess`](crate::tools::Subprocess), which
//! is [ADR-0011] D1's `cmd.run`.
//!
//! What made it possible is that the records now say what running a command
//! here is allowed to be. [ADR-0011] D2 says `bare` is advisory and **not a
//! sandbox**; [ADR-0001] D1 gives `bare` no membrane and `contained` local
//! containers; [ADR-0004] puts the membrane behind
//! [`Verdicts`](crate::tools::Verdicts), which
//! [`NoMembrane`](crate::tools::NoMembrane) already answers honestly. So a
//! child process here is exactly what those records describe and no more, and
//! the module says so rather than implying containment it does not have.
//!
//! # Where it lives, and why here
//!
//! `zaru-cli`, with the tool surface, because everything a spawn needs is this
//! crate's: [ADR-0011] D4's working directory, D5's budget and session
//! directory, and the permission decision that runs before it.
//! **`zaru-core` gains nothing** — it declares both ports and implements
//! neither, which is the same dependency inversion the composer and the tool
//! surface already use — and no [ADR-0003] D8 edge moves.
//!
//! # No dependency
//!
//! `std::process`, `std::thread` and `std::time`, plus
//! `std::os::unix::process::ExitStatusExt` for a signal number, which is safe
//! and leaves the workspace's `unsafe_code = "deny"` untouched. A timeout
//! crate, `libc`, and any shell-word crate are each outside [ADR-0003] D2's
//! table and none is taken; the ceiling is a poll and two threads, and the
//! splitter is [`mod@line`].
//!
//! # The four pieces
//!
//! | Module | Holds |
//! | --- | --- |
//! | [`mod@line`] | a command as a program and arguments, and the one door text comes through |
//! | [`environment`] | exactly what a child is given, with [ADR-0014] D1's layer 4 unable to be in it |
//! | [`ceiling`] | how long a child may run, refused at zero, no number chosen here |
//! | [`spawn`] | the child itself, at the boundary's root, under the ceiling |
//!
//! [`ports`] holds the two trait implementations, so that "two ports, one
//! process" is legible in the file list.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0004]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy

pub mod ceiling;
pub mod environment;
pub mod line;
pub mod ports;
pub mod spawn;

pub use ceiling::{CeilingIsZero, ProcessCeiling};
pub use environment::{Environment, HARNESS_PREFIX, MINIMUM, NotForAChild};
pub use line::{CommandLine, NotACommandLine, REFUSED_CONSTRUCTS};
pub use spawn::{Ended, Outcome, SIGNALLED_EXIT_BASE, Spawn, SpawnFailure};

#[cfg(test)]
mod tests;
