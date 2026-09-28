// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one port this module answers, in its own file for the reason
//! [`crate::process::ports`] is: "one client, one port" is then legible in the
//! file list rather than only in a signature.

use crate::tools::port::{Fetch, Retrieved};
use crate::web::client::WebClient;
use crate::web::url::RequestedUrl;
use zaru_core::iteration::PortFailure;

impl Fetch for WebClient {
    /// # This never returns `Err`, and that is worth saying rather than
    /// hiding behind the signature
    ///
    /// Every outcome of a retrieval is a
    /// [`Captured`](crate::tools::output::Captured), or a redirect to another
    /// host handed back as [`Retrieved::Elsewhere`]: a refused destination,
    /// a refused scheme on a redirect, a body over the ceiling, a timeout, a
    /// connection that failed, and a `404` are all **the work's failure**,
    /// carried as a non-zero exit code with the reason on standard error —
    /// which is the rule [`files`](crate::tools::files) states for the five
    /// filesystem built-ins and which this one has no reason to differ from.
    ///
    /// A [`PortFailure`] is what
    /// [`Executor`](crate::tools::Executor) raises when **the harness** built
    /// the wrong thing, and nothing on this path can. The arm exists because
    /// the trait's signature has one, in the shape
    /// [`crate::cli::layers`] already records for `LayerSource::read`.
    async fn retrieve(
        &self,
        url: &RequestedUrl,
        followed: usize,
    ) -> Result<Retrieved, PortFailure> {
        Ok(WebClient::retrieve(self, url, followed).await)
    }
}
