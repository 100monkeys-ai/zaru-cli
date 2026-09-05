// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one built-in of [ADR-0011] D1's seven that cannot act, answering as a
//! tool that failed rather than as a port that broke.
//!
//! # The model is offered seven and must be answered for seven
//!
//! [`crate::tools::descriptors`] builds the offered set from
//! [`ToolName::ALL`](crate::tools::ToolName) rather than from a list retyped
//! beside it, so "the seven cannot drift from what the executor will accept —
//! the two lists are one walk". That is the right rule and it means a
//! composition cannot quietly offer six: `web.fetch` is on the list, a model
//! may ask for it, and something has to answer.
//!
//! # It is the work's failure, not a port failure, and the difference is a
//! whole turn
//!
//! [`Fetch::retrieve`](crate::tools::Fetch::retrieve) may return a
//! `PortFailure`, and one would end the **entire turn**:
//! `tool_call::run` maps it to `ToolCallError::Port` and returns, so a model
//! that tried `web.fetch` once would take everything else it had done with it.
//!
//! [ADR-0016] D1 row 1 puts a tool that ran and reported its own bad news in
//! the expected register, and `zaru-core`'s own `ToolResult::failed` says the
//! loop "carries it on rather than stopping". So this answers with a
//! [`Captured`] carrying a non-zero exit code and a sentence on standard
//! error, which is **exactly the shape [`crate::tools::files`] already uses**
//! for an I/O failure — the first thing in this workspace to raise one from a
//! model-driven action, and the precedent that record's Status tracking
//! records: "Every failure of an act is reported as the work's".
//!
//! # It is deleted rather than edited
//!
//! The `web-fetch` arc gives D1's seventh built-in a real implementation, and
//! on the day it lands this module goes. It is a stand-in for a capability
//! nobody has built, not a policy about URLs: **no URL is inspected, refused,
//! or allowed here**, because a URL allowlist would be a security vocabulary
//! this crate has no record to transcribe — which is
//! [`crate::tools::port`]'s own sentence about the port.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::tools::output::Captured;
use crate::tools::port::Fetch;
use zaru_core::iteration::PortFailure;

/// What a model is told when it asks for the built-in this build has not
/// written.
///
/// A named constant because a check asserts it reaches the model: an
/// implementation that answered with an empty stream would satisfy "the turn
/// carried on" without telling the model anything it could act on.
pub const NOT_BUILT: &str = "web.fetch is one of ADR-0011 D1's seven built-in tools and this build \
                             has no implementation of it: the harness cannot retrieve a URL. \
                             Nothing was fetched. Every other built-in works; use one of those, or \
                             ask the person running this harness to fetch the page for you.";

/// [ADR-0011] D1's `web.fetch`, with nothing behind it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoFetch;

impl Fetch for NoFetch {
    /// Report the work's failure, and never a port failure.
    ///
    /// **The URL does not appear in the answer.** It came from the model,
    /// which already has it, so echoing it buys nothing — and a capture is
    /// text that travels into a prompt and into a transcript, where the rule
    /// this workspace holds is that a refusal carries what a reader needs and
    /// no more.
    async fn retrieve(&self, _url: &str) -> Result<Captured, PortFailure> {
        Ok(Captured {
            exit_code: 1,
            stdout: String::new(),
            stderr: NOT_BUILT.to_owned(),
        })
    }
}
