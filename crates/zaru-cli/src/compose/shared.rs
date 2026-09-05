// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! One tool surface, reached by both loops.
//!
//! # Why this exists, and it is a borrow rather than a decision
//!
//! [`tool_call::run`](zaru_core::tool_call::run) takes
//! [`Ports::tools`](zaru_core::tool_call::Ports) as `&mut X` **and**
//! [ADR-0009] D4's `inner: Option<&I>` in the same call. It takes the inner
//! branch before it touches `ports.tools`, so the two are never both used —
//! but the borrow checker cannot see that, and an inner loop that also held
//! the executor mutably would be a second `&mut` to one value.
//!
//! So there is exactly one [`Executor`] per session, behind a lock, and this
//! is the handle. It is [`Copy`], so the outer loop's `&mut Shared` and the
//! inner loop's own copy are two handles to one surface rather than two
//! surfaces.
//!
//! # This is what makes "a candidate cannot do what a turn cannot" structural
//!
//! [ADR-0008]'s reserved question — "what an execution *is*" — was decided on
//! 2026-09-05 under directive 20 as *a candidate applied through the same
//! tool surface a turn uses*. Sharing the value is what turns that sentence
//! into a property: there is one [ADR-0011] D4 boundary, one allowlist, one
//! [`Confirm`](crate::tools::Confirm), one
//! [`Verdicts`](crate::tools::Verdicts), one overflow sink and one
//! transcript, so a candidate's `fs.write` is classified by the same
//! `WorkingDirectory::classify` and asked by the same prompt a turn's is.
//! **A second executor built for the inner loop would be a second policy**,
//! and nothing would say when the two drifted.
//!
//! # Why a `tokio` mutex rather than a `RefCell` or a `std` one
//!
//! Both port methods declare `impl Future<Output = …> + Send`, and
//! `Executor::execute` awaits — `web.fetch` is `reqwest`'s future and
//! `cmd.run` is a child process — so the guard is held across an await point.
//! A `RefCell` borrow is not `Send` at all and `std::sync::MutexGuard` is
//! `!Send` by construction; `tokio::sync::MutexGuard` is `Send` where its
//! contents are, which is the whole of why this is the type.
//!
//! `sync` is a **feature** of a crate [ADR-0003] D2's table already names,
//! arriving with a caller exactly as `net` and `time` did on 2026-09-05 —
//! that record's clause 7's own distinction. Measured with both instruments
//! on 2026-09-05 against the 314-package lock and the 242-package `zaru-cli`
//! closure: **zero and zero**, because `reqwest` already enables it on this
//! same crate. Declared anyway rather than inherited, for the reason the
//! manifest gives: a rule that holds by circumstance breaks in a confusing
//! place the day a dependency's feature set moves.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::tools::{Executor, Fetch, Subprocess};
use tokio::sync::Mutex;
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{ToolDescriptor, ToolExecutor, ToolOutcome, ToolRequest};

/// The one tool surface a session has, as a handle either loop can hold.
///
/// See the module documentation. `'m` is the lock's lifetime and `'e` the
/// executor's own borrows; they are separate because
/// [`Mutex`] is invariant in its contents, and tying them together would make
/// the composition's declaration order a thing to get right rather than a
/// thing that compiles.
pub struct Shared<'m, 'e, C, F> {
    cell: &'m Mutex<Executor<'e, C, F>>,
}

impl<C, F> Clone for Shared<'_, '_, C, F> {
    fn clone(&self) -> Self {
        *self
    }
}

/// Two handles to one surface, which is the point of the type.
impl<C, F> Copy for Shared<'_, '_, C, F> {}

impl<C, F> core::fmt::Debug for Shared<'_, '_, C, F> {
    /// Names what it is and renders nothing it holds.
    ///
    /// [`Executor`] writes its own `Debug` by hand because what it holds
    /// includes a working directory and a transcript; reaching through the
    /// lock to render it would undo that, and would also block on a lock in
    /// a formatter.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Shared").finish_non_exhaustive()
    }
}

impl<'m, 'e, C, F> Shared<'m, 'e, C, F> {
    /// Take a handle to the session's one tool surface.
    #[must_use]
    pub const fn over(cell: &'m Mutex<Executor<'e, C, F>>) -> Self {
        Self { cell }
    }
}

impl<C, F> ToolExecutor for Shared<'_, '_, C, F>
where
    C: Subprocess + Sync,
    F: Fetch + Sync,
{
    /// The seven, from the one list both implementations return.
    ///
    /// It cannot come from the executor: that method borrows from `&self`,
    /// and a slice borrowed from the guard would not outlive this call. See
    /// [`descriptor_set`](crate::tools::descriptor_set).
    fn descriptors(&self) -> &[ToolDescriptor] {
        crate::tools::descriptor_set()
    }

    /// Execute one call on the shared surface.
    ///
    /// The lock is taken for the length of one call and released before the
    /// next, so an inner loop applying a candidate and an outer loop running
    /// a turn interleave at call granularity rather than at loop granularity
    /// — which is the honest ordering, since ADR-0009 D4's branch means only
    /// one of them is ever running.
    async fn execute(&mut self, request: &ToolRequest) -> Result<ToolOutcome, PortFailure> {
        self.cell.lock().await.execute(request).await
    }
}
