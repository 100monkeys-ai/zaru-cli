// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The HTTP half of the streamable transport, over this workspace's `reqwest`.
//!
//! # What this implements and why it is written rather than taken
//!
//! `rmcp` offers `StreamableHttpClient` for `reqwest::Client` behind its
//! `transport-streamable-http-client-reqwest` feature, and that impl is not
//! taken. The reason is a version, measured rather than assumed: `rmcp` 3.2.0
//! declares `reqwest = "0.13.2"` and this workspace's [ADR-0003] D2 row is
//! `reqwest` 0.12.28, so taking the SDK's client puts **two `reqwest` in one
//! tree** — printed by `cargo tree -e normal -d` — which is the duplication
//! `ratatui` is pinned to 0.29 and `aes-gcm` to 0.11 to avoid.
//!
//! See [`super`] for the three shapes and their numbers.
//!
//! # What is deliberately narrower than the SDK's own client
//!
//! Two things, and each is named rather than silently absent.
//!
//! **No legacy discovery.** The SDK's client calls `legacy_discover_response`
//! on a non-success POST, which is `pub(super)` and cannot be reached from
//! here. It exists so that a 4xx from an old SSE-transport server can be read
//! as a discovery hint. Nuclear Notes is not such a server — measured
//! 2026-09-05: its POST answers `200` with `application/json` and its `GET`
//! answers `200` with `text/event-stream`, which is the streamable transport
//! [ADR-0103] mounts — so what is lost is a fallback to a shape this product
//! never talks to.
//!
//! **A coarser SSE event bound.** See [`super::bounded`].
//!
//! # The bearer is a `String` here and that is `rmcp`'s shape, not a choice
//!
//! `StreamableHttpClient` takes `auth_header: Option<String>` on every method.
//! Nothing in this module constructs one: the value arrives already formatted
//! from [`Endpoint::open`](crate::session::Endpoint::open), which is this
//! crate's one call site of
//! [`Bearer::expose_for_dispatch`](crate::session::Bearer::expose_for_dispatch),
//! and this module passes it to `reqwest` and to nothing else. **No error this
//! module raises is built from it** — the failures below carry a status, a
//! content type, or `reqwest`'s own sentence.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0103]: https://cortex.page/adrs/p/0103-mcp-server-transport-mount

use crate::session::transport::bounded::{self, Budget, DEFAULT_MAX_SSE_EVENT_SIZE};
use crate::session::transport::request::{self, Outcome};
use futures::StreamExt;
use reqwest::header::{HeaderName, HeaderValue};
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::common::client_side_sse::BoxedSseResponse;
use rmcp::transport::common::http_header::{HEADER_LAST_EVENT_ID, HEADER_SESSION_ID};
use rmcp::transport::streamable_http_client::{
    StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use std::collections::HashMap;
use std::sync::Arc;

/// The `reqwest` client the transport runs over.
///
/// A newtype rather than an impl on `reqwest::Client` itself: the SDK already
/// writes that impl behind a feature this crate does not take, and a second one
/// on the same foreign type would be a coherence error the day anything in this
/// workspace did take it. The newtype is also what makes the client's own
/// configuration — no idle pooling, no redirects — a property of this type
/// rather than of whichever `Client` a caller happened to have.
#[derive(Debug, Clone)]
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    /// Build the client this transport uses.
    ///
    /// **Redirects are refused rather than followed**, which is a credential
    /// decision rather than an HTTP one: `reqwest` replays request headers to a
    /// redirect target, and the header this transport carries is the bearer.
    /// A server — or anything that can answer as one — that replied `307` to a
    /// different host would otherwise be handed the token. The SDK's own client
    /// disables them for the same reason, in its own words: "Automatic
    /// redirects are disabled so caller-supplied custom headers cannot be
    /// replayed to a redirect target."
    ///
    /// Idle connection pooling is disabled for the reason the SDK gives: a
    /// response body that was not fully consumed before the pool reuses its
    /// connection stalls on Linux TCP delayed ACK, and an SSE body is never
    /// fully consumed.
    ///
    /// # Errors
    ///
    /// [`reqwest::Error`] when the TLS backend cannot be initialised, which is
    /// the only way this builder fails.
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .pool_max_idle_per_host(0)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
}

impl StreamableHttpClient for ReqwestHttp {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        self.post_message_with_max_sse_event_size(
            uri,
            message,
            session_id,
            auth_header,
            custom_headers,
            DEFAULT_MAX_SSE_EVENT_SIZE,
        )
        .await
    }

    async fn post_message_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let carried_session = session_id.is_some();
        let is_request = matches!(message, ClientJsonRpcMessage::Request(_));

        let mut builder = self.client.post(uri.as_ref()).header(
            reqwest::header::ACCEPT,
            request::ACCEPT_JSON_AND_EVENT_STREAM,
        );
        if let Some(header) = auth_header {
            builder = builder.header(reqwest::header::AUTHORIZATION, header);
        }
        if let Some(session) = session_id {
            builder = builder.header(HEADER_SESSION_ID, session.as_ref());
        }
        builder = request::apply_custom(builder, custom_headers)?;

        let response = builder
            .json(&message)
            .send()
            .await
            .map_err(StreamableHttpError::Client)?;

        let status = response.status();
        let content_type = request::content_type(response.headers());
        let session = response
            .headers()
            .get(HEADER_SESSION_ID)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        match request::read_post(status, content_type.as_deref(), carried_session, is_request)? {
            Outcome::Accepted => Ok(StreamableHttpPostResponse::Accepted),
            Outcome::Json => {
                let body = response.text().await.map_err(StreamableHttpError::Client)?;
                Ok(StreamableHttpPostResponse::Json(
                    serde_json::from_str(&body)?,
                    session,
                ))
            }
            Outcome::Sse => Ok(StreamableHttpPostResponse::Sse(
                sse(response, max_sse_event_size),
                session,
            )),
            Outcome::Refused => {
                let body = response.text().await.unwrap_or_default();
                Err(request::refusal(status, content_type.as_deref(), &body))
            }
        }
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxedSseResponse, StreamableHttpError<Self::Error>> {
        self.get_stream_with_max_sse_event_size(
            uri,
            session_id,
            last_event_id,
            auth_header,
            custom_headers,
            DEFAULT_MAX_SSE_EVENT_SIZE,
        )
        .await
    }

    async fn get_stream_with_max_sse_event_size(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
        max_sse_event_size: usize,
    ) -> Result<BoxedSseResponse, StreamableHttpError<Self::Error>> {
        let mut builder = self.client.get(uri.as_ref()).header(
            reqwest::header::ACCEPT,
            request::ACCEPT_JSON_AND_EVENT_STREAM,
        );
        if let Some(header) = auth_header {
            builder = builder.header(reqwest::header::AUTHORIZATION, header);
        }
        if let Some(session) = session_id {
            builder = builder.header(HEADER_SESSION_ID, session.as_ref());
        }
        if let Some(event) = last_event_id {
            builder = builder.header(HEADER_LAST_EVENT_ID, event);
        }
        builder = request::apply_custom(builder, custom_headers)?;

        let response = builder.send().await.map_err(StreamableHttpError::Client)?;
        let status = response.status();
        let content_type = request::content_type(response.headers());
        request::read_stream(status, content_type.as_deref())?;
        Ok(sse(response, max_sse_event_size))
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let mut builder = self
            .client
            .delete(uri.as_ref())
            .header(HEADER_SESSION_ID, session.as_ref());
        if let Some(header) = auth_header {
            builder = builder.header(reqwest::header::AUTHORIZATION, header);
        }
        builder = request::apply_custom(builder, custom_headers)?;

        let response = builder.send().await.map_err(StreamableHttpError::Client)?;
        // A server that does not implement session deletion answers 405, and
        // that is not a failure of anything: the session ends when the process
        // stops holding it. The SDK's own client reads it the same way.
        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(());
        }
        response
            .error_for_status()
            .map_err(StreamableHttpError::Client)?;
        Ok(())
    }
}

/// A response body, parsed as server-sent events under a byte ceiling.
///
/// The budget is threaded through a `map` rather than a `Stream` impl of its
/// own, so the counting is an ordinary function this crate can check with no
/// runtime — see [`super::bounded`].
fn sse(response: reqwest::Response, ceiling: usize) -> BoxedSseResponse {
    let mut budget = Budget::new(ceiling);
    let bytes = response
        .bytes_stream()
        .map(move |chunk| bounded::account(&mut budget, chunk));
    sse_stream::SseStream::from_bytes_stream(bytes).boxed()
}
