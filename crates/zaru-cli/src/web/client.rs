// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one HTTP client this workspace builds, and the retrieval `web.fetch`
//! makes with it.
//!
//! # `build` is shared with the provider client, and that is the point
//!
//! [`providers::gemini`](crate::providers::gemini) calls it too. Two builders
//! would be two answers to "what does an HTTP client in this workspace do
//! about redirects, timeouts and cookies", and a pair of answers is the
//! rule-in-two-places that made [`Layer`](crate::config::Layer) drift while it
//! was declared twice. What the two callers differ on is passed **as an
//! argument** — the timeout and the redirect policy — so the difference is at
//! the call site where a reader can see it rather than inside two constructors
//! that have to be compared.
//!
//! # No cookie jar, and there is no method to call
//!
//! `reqwest`'s `ClientBuilder::cookie_store` and `cookie_provider` are both
//! `#[cfg(feature = "cookies")]`, and that feature has no caller in this
//! workspace — the root manifest's `reqwest` row names it among the features
//! deliberately not taken. So "`web.fetch` keeps no cookies" is a property of
//! the dependency graph rather than a line of code somebody has to keep
//! writing, which is the same form `scripts/check-crate-boundaries.py` gives
//! ADR-0005 D3's offline claim for `zaru-tui`.
//!
//! # No header is added, and there is nowhere to put one
//!
//! `WebClient::retrieve` issues a `GET` and sets nothing.
//! [`Fetch::retrieve`](crate::tools::Fetch::retrieve) takes a URL and nothing
//! else, [ADR-0011] D1's argument contract gives `web.fetch` the single field
//! `url`, and no type on this path has a field a credential could occupy. **A
//! credential cannot be attached to a request made here** because there is no
//! parameter it could arrive through — the structural form, not a rule
//! somebody remembers.
//!
//! # The redirect policy is stateless, so one client serves every call
//!
//! `reqwest` sets a redirect policy on the **client**, not on the request, and
//! [`providers::gemini`](crate::providers::gemini) already records why a
//! client is built once and reused: a fresh client per request is a fresh
//! connection pool and a fresh TLS handshake per request. So the policy reads
//! only its [`Attempt`](reqwest::redirect::Attempt) and closes over two
//! `Copy` values, and there is no per-call state for two concurrent
//! retrievals to share.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface

use crate::tools::output::Captured;
use crate::tools::port::Retrieved;
use crate::web::bounds::FetchBounds;
use crate::web::url::{Destinations, RETRIEVABLE_SCHEMES, RefusedDestination, RequestedUrl};
use core::fmt;
use core::time::Duration;
use reqwest::redirect;

/// The HTTP client could not be built at all.
///
/// A machine with no usable TLS backend, which is environmental and not the
/// user's. It carries the builder's own wording for the reason
/// [`OverflowFailure`](crate::tools::OverflowFailure) does: there is no error
/// type this and a future second implementation would share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientUnavailable {
    detail: String,
}

impl ClientUnavailable {
    /// Report a failure in the builder's own words.
    #[must_use]
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// What went wrong.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ClientUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an HTTP client could not be built: {}", self.detail)
    }
}

impl std::error::Error for ClientUnavailable {}

/// Build the one shape of HTTP client this workspace uses.
///
/// The timeout and the redirect policy are the caller's because they are the
/// only two things its two callers disagree about; everything else — the TLS
/// backend, the absent cookie jar, the absent default headers — is the same
/// for both and is settled by the feature set rather than here.
///
/// # The timeout is set here and **only** here
///
/// [`WebClient::retrieve`] set it a second time, per request, until a
/// red-watch found that removing either one left the check green: `reqwest`
/// applies a client timeout to the whole request including reading the body,
/// so the two were one rule written twice with the same value. The
/// redundancy is gone rather than recorded, because a `WebClient` is built for
/// exactly one set of bounds and there is no caller for which the two could
/// differ — and a rule in two places is what the paragraph above refuses for
/// the builder itself.
///
/// # Errors
///
/// [`ClientUnavailable`] when `reqwest` cannot build a client, which in
/// practice is a machine with no usable TLS backend.
pub(crate) fn build(
    timeout: Duration,
    redirects: redirect::Policy,
) -> Result<reqwest::Client, ClientUnavailable> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(redirects)
        .build()
        .map_err(|error| ClientUnavailable::new(error.to_string()))
}

/// Why one redirect hop was not followed.
///
/// Rendered into the capture the model reads, so it names the host it declined
/// to follow to and never anything else from the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
enum HopRefused {
    /// The next hop leaves the host that was asked.
    AnotherHost {
        /// The host that was asked.
        from: String,
        /// The host the server pointed at.
        to: String,
    },
    /// The next hop carries a scheme this surface does not retrieve.
    Scheme {
        /// The scheme.
        scheme: String,
    },
    /// The next hop is a destination this surface does not reach.
    Destination(RefusedDestination),
}

impl fmt::Display for HopRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnotherHost { from, to } => write!(
                f,
                "the redirect from {from} to {to} was not followed, because it leaves the host \
                 that was asked. A redirect across a host turns the URL the model chose into one \
                 the server chose, and that is the whole of what this surface is allowed to \
                 decide"
            ),
            Self::Scheme { scheme } => write!(
                f,
                "the redirect was not followed, because it points at a {scheme:?} URL and \
                 web.fetch retrieves {}",
                RETRIEVABLE_SCHEMES.join(" and ")
            ),
            Self::Destination(refused) => write!(f, "the redirect was not followed: {refused}"),
        }
    }
}

/// Whether this hop is refused, on the three rules that are not a count.
///
/// **One predicate, called from two places**: the redirect policy, which sees
/// every hop as it is offered, and [`WebClient::retrieve`], which re-derives
/// why a stopped chain stopped from the response it got back. Writing the
/// rules twice is what would let the policy refuse a hop and the capture
/// report a different reason for it.
///
/// The **port is deliberately not compared**, only the host: an `http` to
/// `https` upgrade on one host changes the default port, so a rule that
/// compared ports would refuse the commonest redirect on the web.
fn refuse_hop(
    destinations: Destinations,
    current: &reqwest::Url,
    next: &reqwest::Url,
) -> Option<HopRefused> {
    if !RETRIEVABLE_SCHEMES.contains(&next.scheme()) {
        return Some(HopRefused::Scheme {
            scheme: next.scheme().to_string(),
        });
    }
    if let Some(refused) = destinations.refuses(next) {
        return Some(HopRefused::Destination(refused));
    }
    if next.host_str() != current.host_str() {
        return Some(HopRefused::AnotherHost {
            from: current
                .host_str()
                .unwrap_or_default()
                .escape_debug()
                .to_string(),
            to: next
                .host_str()
                .unwrap_or_default()
                .escape_debug()
                .to_string(),
        });
    }
    None
}

/// [ADR-0011] D1's `web.fetch`, over one `reqwest::Client`.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug)]
pub struct WebClient {
    http: reqwest::Client,
    bounds: FetchBounds,
    destinations: Destinations,
}

impl WebClient {
    /// A client that reaches everything except this machine and link-local.
    ///
    /// The only public constructor. See
    /// [`Destinations`] for what it refuses and
    /// why that is not a security vocabulary.
    ///
    /// # Errors
    ///
    /// [`ClientUnavailable`] when `reqwest` cannot build a client at all.
    pub fn new(bounds: FetchBounds) -> Result<Self, ClientUnavailable> {
        Self::reaching(bounds, Destinations::public())
    }

    /// A client that also reaches this machine, for a check with its own
    /// listener.
    ///
    /// Compiled only under `cfg(test)`, so it is absent from the rlib the
    /// binary and every integration test link. See
    /// [`Destinations::including_this_machine`](crate::web::url::Destinations).
    ///
    /// # Errors
    ///
    /// [`ClientUnavailable`] when `reqwest` cannot build a client at all.
    #[cfg(test)]
    pub(crate) fn reaching_this_machine(bounds: FetchBounds) -> Result<Self, ClientUnavailable> {
        Self::reaching(bounds, Destinations::including_this_machine())
    }

    /// Build a client over a reach.
    fn reaching(
        bounds: FetchBounds,
        destinations: Destinations,
    ) -> Result<Self, ClientUnavailable> {
        let limit = bounds.redirects.get();
        let policy = redirect::Policy::custom(move |attempt| {
            let Some(current) = attempt.previous().last() else {
                // Unreachable: `previous` carries the URL that was requested.
                // Stopping is the safe arm of an impossible one.
                return attempt.stop();
            };
            if refuse_hop(destinations, current, attempt.url()).is_some() {
                return attempt.stop();
            }
            // `previous` includes the URL first requested, which is not a
            // redirection, so this is `reqwest`'s own arithmetic for its
            // `limited` policy — transcribed rather than re-derived.
            if attempt.previous().len() > limit {
                return attempt.stop();
            }
            attempt.follow()
        });
        Ok(Self {
            http: build(bounds.timeout.get(), policy)?,
            bounds,
            destinations,
        })
    }

    /// The bounds this client was built with.
    #[must_use]
    pub const fn bounds(&self) -> FetchBounds {
        self.bounds
    }

    /// Retrieve the URL and capture what came back.
    ///
    /// Every outcome is a [`Captured`] and none is a
    /// [`PortFailure`](zaru_core::iteration::PortFailure): a retrieval that
    /// failed is a non-zero exit code with the reason on standard error,
    /// exactly as a command that failed is, which is the rule
    /// [`files`](crate::tools::files) already states — "a failure of the act
    /// is the work's, never the harness's". No [ADR-0016] class is claimed
    /// here for the same reason that module gives.
    ///
    /// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
    ///
    /// # A redirect to another host is handed back, not followed
    ///
    /// Until 2026-09-28 such a redirect was refused outright. Now it is
    /// returned as [`Retrieved::Elsewhere`], so the caller can ask the person
    /// about the new host as it asks about any call, and refuse it where
    /// there is nobody to ask. `followed` counts the ones already followed
    /// for this call, and the redirect limit holds across hosts as it holds
    /// within one.
    pub(crate) async fn retrieve(&self, url: &RequestedUrl, followed: usize) -> Retrieved {
        Retrieved::Captured(match self.retrieve_one(url, followed).await {
            Ok(captured) => captured,
            Err(to) => return Retrieved::Elsewhere { to },
        })
    }

    /// [`Self::retrieve`], with a redirect to another host as the error arm.
    async fn retrieve_one(
        &self,
        url: &RequestedUrl,
        followed: usize,
    ) -> Result<Captured, RequestedUrl> {
        if let Some(refused) = self.destinations.refuses(url.inner()) {
            return Ok(refusal(refused.to_string()));
        }

        let mut response = match self.http.get(url.inner().clone()).send().await {
            Ok(response) => response,
            Err(error) => return Ok(refusal(transport(url, &error))),
        };

        let status = response.status();
        let final_url = response.url().clone();

        if status.is_redirection() {
            if let Some(to) = self.another_host(&final_url, &response) {
                if followed >= self.bounds.redirects.get() {
                    return Ok(refusal(format!(
                        "the redirect from {} to {} was not followed, because the limit of {} was \
                         reached",
                        final_url.host_str().unwrap_or_default().escape_debug(),
                        to.host().escape_debug(),
                        self.bounds.redirects
                    )));
                }
                return Err(to);
            }
            return Ok(refusal(self.why_the_chain_stopped(&final_url, &response)));
        }

        let ceiling = self.bounds.body.get();
        // The server's own claim, tested before a byte is read -- the same
        // "skipped on its metadata, so it is never opened" move `fs.search`
        // makes for an oversized file. It is the cheap arm and not the one
        // that holds: a `Content-Length` can lie or be absent, and the loop
        // below is what actually bounds the read.
        if let Some(claimed) = response.content_length()
            && claimed > ceiling
        {
            return Ok(refusal(over_ceiling(self.bounds.body, Some(claimed))));
        }

        let mut body: Vec<u8> = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    body.extend_from_slice(&chunk);
                    // Reading one byte past the ceiling is what makes "over
                    // it" observable without holding the whole of an
                    // unbounded body: the moment the length exceeds the
                    // ceiling the read stops and the retrieval is refused.
                    if body.len() as u64 > ceiling {
                        return Ok(refusal(over_ceiling(self.bounds.body, None)));
                    }
                }
                Ok(None) => break,
                Err(error) => return Ok(refusal(transport(url, &error))),
            }
        }

        // Lossily, as `fs.read` decodes a file: what is retrieved is a
        // document being shown to a model, and refusing a page for one
        // invalid sequence would be a stricter rule than the one this crate
        // already applies to a file it reads.
        let text = String::from_utf8_lossy(&body).into_owned();
        let heading = heading(&final_url, status, &response);
        let mut stdout = heading.clone();
        stdout.push('\n');
        stdout.push_str(&text);

        Ok(Captured {
            exit_code: i32::from(!status.is_success()),
            stdout,
            stderr: if status.is_success() {
                String::new()
            } else {
                format!("{heading}\nthe server did not return a success status")
            },
        })
    }

    /// Where a redirect points, when it points at another host this surface
    /// could retrieve from, and nowhere this surface refuses to reach.
    fn another_host(
        &self,
        final_url: &reqwest::Url,
        response: &reqwest::Response,
    ) -> Option<RequestedUrl> {
        let next = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| final_url.join(value).ok())?;
        match refuse_hop(self.destinations, final_url, &next) {
            Some(HopRefused::AnotherHost { .. }) => RequestedUrl::parse(next.as_str()).ok(),
            _ => None,
        }
    }

    /// Why a chain that came back as a 3xx was not followed further.
    ///
    /// Re-derived from the response rather than recorded by the policy,
    /// because the policy is stateless and shared by every concurrent call.
    /// The three real rules are asked through the same [`refuse_hop`] the
    /// policy uses; if none of them fires, the chain stopped on the count.
    fn why_the_chain_stopped(
        &self,
        final_url: &reqwest::Url,
        response: &reqwest::Response,
    ) -> String {
        let next = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| final_url.join(value).ok());
        match next {
            Some(next) => match refuse_hop(self.destinations, final_url, &next) {
                Some(refused) => refused.to_string(),
                None => format!(
                    "the redirect from {} was not followed, because the limit of {} was reached",
                    final_url.host_str().unwrap_or_default().escape_debug(),
                    self.bounds.redirects
                ),
            },
            None => format!(
                "{} answered with a redirect carrying no usable Location, so there was nothing \
                 to follow",
                final_url.host_str().unwrap_or_default().escape_debug()
            ),
        }
    }
}

/// The one line a capture opens with.
///
/// **Every value in it is escaped**, and the content type is why: it is text
/// the server chose, going into a line a model reads, and a header carrying a
/// newline could otherwise forge a second line of the harness's own output.
fn heading(
    url: &reqwest::Url,
    status: reqwest::StatusCode,
    response: &reqwest::Response,
) -> String {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("(none)");
    format!(
        "web.fetch {} — {} — content-type: {}",
        url.as_str().escape_debug(),
        status.as_u16(),
        content_type.escape_debug()
    )
}

/// A capture that says the retrieval did not happen, in the harness's words.
fn refusal(detail: String) -> Captured {
    Captured {
        exit_code: 1,
        stdout: String::new(),
        stderr: detail,
    }
}

/// What a caller is told about a body that exceeded the ceiling.
///
/// `claimed` is present when the server's own `Content-Length` was what
/// refused it, so a reader can tell the cheap arm from the one that read.
fn over_ceiling(ceiling: crate::web::bounds::BodyCeiling, claimed: Option<u64>) -> String {
    let observed = match claimed {
        Some(bytes) => format!("the server declared {bytes} byte(s)"),
        None => "the body passed it while being read".to_owned(),
    };
    format!(
        "web.fetch does not retrieve a body over {ceiling} and {observed}, so the retrieval was \
         refused rather than cut short. When output is truncated the full text must reach the \
         session directory, and a body this surface stopped reading is one \
         no session copy could be complete for -- so nothing is captured rather than a prefix \
         being passed off as the document"
    )
}

/// What a caller is told about a request that did not complete.
///
/// The error's own wording, and the requested URL's host — never the whole
/// URL, which can carry a query string somebody pasted a token into.
fn transport(url: &RequestedUrl, error: &reqwest::Error) -> String {
    format!(
        "web.fetch could not retrieve from {}: {error}",
        url.inner().host_str().unwrap_or_default().escape_debug()
    )
}
