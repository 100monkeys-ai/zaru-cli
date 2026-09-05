// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0011] D1's seventh built-in, `web.fetch`, and the one place this
//! workspace builds an HTTP client.
//!
//! # Two claims, both true, one module
//!
//! **This is the one place a model-chosen URL is retrieved**, which is what
//! [`WebClient`] is, and **the one place a `reqwest::Client` is built**, which
//! is what `client::build` is — private to the crate, so it is named here
//! rather than linked. The second is why
//! [`providers::gemini`](crate::providers::gemini) reaches in here for its own
//! client: two builders would be two answers to "what does an HTTP client in
//! this workspace do about redirects, timeouts and cookies", and the pair
//! would drift the way [`Layer`](crate::config::Layer) drifted while it was
//! declared twice. It is the shape [`crate::atomic`] and
//! [`crate::process::line`] already have.
//!
//! It sits at the crate root beside [`crate::process`] rather than under
//! `tools/`, and for the same reason that one does: a machine's capability is
//! the program's rather than the tool surface's, and `providers` needs this
//! one too.
//!
//! # What a retrieval is allowed to be
//!
//! `GET`, over `http` or `https`, to a destination that is not this machine
//! and not the link-local range, following no redirect across a host, with
//! **no header this crate adds** and no cookie jar. Each of those is a
//! decision and each is written where it is made: the scheme and the
//! destination in [`mod@url`], the redirects and the body in [`client`], the
//! bounds in [`bounds`].
//!
//! **A credential cannot be attached to a request made here**, and that is a
//! property of the signature rather than of a code path:
//! [`Fetch::retrieve`](crate::tools::Fetch::retrieve) takes a URL and nothing
//! else, and nothing in this module has a parameter a header could arrive
//! through. [ADR-0011] D1's argument contract gives `web.fetch` one field,
//! `url`, so there is no route from the model either.
//!
//! # This module never redacts, and that is deliberate
//!
//! [ADR-0008] clause 6's port is applied by
//! [`Captured::present`](crate::tools::Captured::present), one layer up,
//! before [ADR-0011] D5's truncation. A [`Captured`](crate::tools::Captured)
//! produced here is **raw**, exactly as
//! [`Spawn`](crate::process::Spawn)'s and [`files`](crate::tools::files)' are,
//! so the set of product files calling that port is unchanged and
//! `no_captured_bytes_reach_a_prompt_except_through_the_port` — which asserts
//! the **exact set** — stays green. A second redaction here would be the
//! scattering that decision exists to prevent.
//!
//! # What is not claimed
//!
//! **The harness is not a sandbox and this module does not make it one.**
//! [ADR-0011] D2 says so at `bare` and [ADR-0001] D1 gives `bare` no membrane:
//! `cmd.run` can reach every destination [`mod@url`] refuses, and nothing here
//! changes that. What the destination rule buys is narrower and is stated
//! where it is made — see [`url::Destinations`].
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

pub mod bounds;
pub mod client;
pub mod ports;
pub mod url;

pub use bounds::{BodyCeiling, BoundIsZero, FetchBounds, FetchTimeout, RedirectLimit};
pub use client::{ClientUnavailable, WebClient};
pub use url::{Destinations, RequestedUrl, UrlRefused};

#[cfg(test)]
mod tests;
