// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a request carries and what a response means, as functions with no
//! socket in them.
//!
//! # Why the transport is split here
//!
//! The gate that runs this workspace's checks has **no network**, and the
//! standing ruling of this fleet forbids a loopback listener standing in for a
//! server. Both together would leave a network transport with nothing asserted
//! about it at all — which is the shape [Testing] calls untested code with a
//! comment on it.
//!
//! So the two questions a transport actually gets wrong are answered here,
//! where neither needs a peer: **what does the request carry**, and **what does
//! a response mean**. Each is a function from values to values. What is left in
//! [`super::http`] is the `reqwest` call itself and the plumbing either side of
//! it, which is the part a check could only exercise by opening a socket.
//!
//! The complement is the in-process `rmcp` server the rest of this crate's
//! checks run against, over `tokio::io::duplex`: real protocol bytes, no
//! socket, and the half of the session that is not HTTP.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/project-management/p/process/testing

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{RequestBuilder, StatusCode};
use rmcp::transport::common::http_header::{
    EVENT_STREAM_MIME_TYPE, HEADER_LAST_EVENT_ID, HEADER_MCP_PROTOCOL_VERSION, HEADER_SESSION_ID,
    JSON_MIME_TYPE,
};
use rmcp::transport::streamable_http_client::{
    AuthRequiredError, StreamableHttpError, StreamableHttpPostResponse,
};
use std::borrow::Cow;
use std::collections::HashMap;

/// What this transport tells a server it will accept.
///
/// **Both, always, and the order is the specification's.** A streamable HTTP
/// server chooses between answering a POST with a single JSON body and
/// answering it with an event stream, and it chooses by reading this header. A
/// client that offered only `application/json` would be refusing every
/// streamed answer before it was sent; one that offered only
/// `text/event-stream` would be refusing every immediate one. Measured against
/// Nuclear Notes on 2026-09-05: with both offered, its POST answers
/// `application/json` and its `GET` answers `text/event-stream`, so this one
/// product uses each branch on a different method.
pub const ACCEPT_JSON_AND_EVENT_STREAM: &str = "application/json, text/event-stream";

/// Header names a caller may not supply, because this transport sets them.
///
/// Transcribed from `rmcp`'s own `RESERVED_HEADERS`, which is `pub(crate)` and
/// therefore not reachable from here, and kept in the same order with the same
/// exception: `MCP-Protocol-Version` is on the list and is **allowed through**,
/// because the SDK's worker injects it after initialisation and a client that
/// refused it would refuse the SDK's own request.
///
/// Transcribed rather than re-derived for the reason this workspace transcribes
/// [ADR-0006](https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces)
/// D4's tool list verbatim: deciding for oneself which headers a transport owns
/// is authoring a rule somebody else already wrote.
const RESERVED_HEADERS: &[&str] = &[
    "accept",
    HEADER_SESSION_ID,
    HEADER_MCP_PROTOCOL_VERSION,
    HEADER_LAST_EVENT_ID,
];

/// What a POST's status and content type mean.
///
/// Named rather than returned as a tuple of booleans, because the four are
/// exclusive and a caller that could hold two of them at once is a caller with
/// a branch nobody wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The server took the message and has nothing to say about it.
    Accepted,
    /// The body is one JSON-RPC message.
    Json,
    /// The body is an event stream.
    Sse,
    /// The status is not a success and the body is the server's own words.
    Refused,
}

/// Add a caller's headers, refusing one this transport owns.
///
/// # Errors
///
/// [`StreamableHttpError::ReservedHeaderConflict`] naming the header.
pub fn apply_custom<E>(
    mut builder: RequestBuilder,
    custom: HashMap<HeaderName, HeaderValue>,
) -> Result<RequestBuilder, StreamableHttpError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    for (name, value) in custom {
        reserved(&name).map_err(StreamableHttpError::ReservedHeaderConflict)?;
        builder = builder.header(name, value);
    }
    Ok(builder)
}

/// Whether a header name is one this transport sets for itself.
///
/// # Errors
///
/// The name, as the caller spelled it, when it is reserved.
pub fn reserved(name: &HeaderName) -> Result<(), String> {
    if name
        .as_str()
        .eq_ignore_ascii_case(HEADER_MCP_PROTOCOL_VERSION)
    {
        return Ok(());
    }
    if RESERVED_HEADERS
        .iter()
        .any(|owned| name.as_str().eq_ignore_ascii_case(owned))
    {
        return Err(name.to_string());
    }
    Ok(())
}

/// The response's content type, lowercased to its media type alone.
///
/// A server is free to send `application/json; charset=utf-8`, so a comparison
/// against the whole header value is a comparison that fails on a correct
/// server. The parameters are dropped and the media type is compared.
#[must_use]
pub fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        })
}

/// What to do with a POST's response, before its body is read.
///
/// `carried_session` is whether the request named a session, and it is what
/// makes a `404` mean "the session the server had is gone" rather than "there
/// is nothing at this URL". `is_request` is whether a reply was expected at
/// all: a notification answered with an empty success is accepted rather than
/// read as a message that failed to parse.
///
/// # Errors
///
/// [`StreamableHttpError::AuthRequired`] when the server challenges,
/// [`StreamableHttpError::SessionExpired`] when a named session is gone, and
/// [`StreamableHttpError::UnexpectedContentType`] when a success carries
/// neither media type this transport offered.
pub fn read_post<E>(
    status: StatusCode,
    content_type: Option<&str>,
    carried_session: bool,
    is_request: bool,
) -> Result<Outcome, StreamableHttpError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    if status == StatusCode::ACCEPTED || status == StatusCode::NO_CONTENT {
        return Ok(Outcome::Accepted);
    }
    if status == StatusCode::NOT_FOUND && carried_session {
        return Err(StreamableHttpError::SessionExpired);
    }
    if !status.is_success() {
        return Ok(Outcome::Refused);
    }
    match content_type {
        Some(JSON_MIME_TYPE) => Ok(Outcome::Json),
        Some(EVENT_STREAM_MIME_TYPE) => Ok(Outcome::Sse),
        // A success with no body and nothing expected. The specification wants
        // 202 for this and some servers send an empty 200; both mean the same
        // thing and reading the second as a parse failure would turn a correct
        // server into an error.
        None if !is_request => Ok(Outcome::Accepted),
        other => Err(StreamableHttpError::UnexpectedContentType(
            other.map(str::to_owned),
        )),
    }
}

/// What a `GET` for the notification stream means.
///
/// # Errors
///
/// [`StreamableHttpError::ServerDoesNotSupportSse`] on `405`,
/// [`StreamableHttpError::UnexpectedServerResponse`] on any other non-success,
/// and [`StreamableHttpError::UnexpectedContentType`] when a success is not an
/// event stream.
pub fn read_stream<E>(
    status: StatusCode,
    content_type: Option<&str>,
) -> Result<(), StreamableHttpError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    if status == StatusCode::METHOD_NOT_ALLOWED {
        return Err(StreamableHttpError::ServerDoesNotSupportSse);
    }
    if !status.is_success() {
        return Err(StreamableHttpError::UnexpectedServerResponse(Cow::Owned(
            format!("HTTP {status} opening the notification stream"),
        )));
    }
    match content_type {
        Some(EVENT_STREAM_MIME_TYPE) => Ok(()),
        other => Err(StreamableHttpError::UnexpectedContentType(
            other.map(str::to_owned),
        )),
    }
}

/// The failure a non-success POST becomes, once its body has been read.
///
/// **The body is carried and the request is not.** What the server said about
/// what went wrong is the only thing a reader can act on; the request carried
/// the bearer, and a failure that quoted its own request would publish it.
#[must_use]
pub fn refusal<E>(
    status: StatusCode,
    content_type: Option<&str>,
    body: &str,
) -> StreamableHttpError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    if status == StatusCode::UNAUTHORIZED {
        return StreamableHttpError::AuthRequired(AuthRequiredError::new(body.to_owned()));
    }
    // A JSON-RPC error body on a non-success status is still a JSON-RPC error,
    // and reading it as one is what puts the server's own code and message in
    // front of the caller instead of an HTTP status.
    if content_type == Some(JSON_MIME_TYPE)
        && let Ok(message) = serde_json::from_str::<rmcp::model::ServerJsonRpcMessage>(body)
        && matches!(message, rmcp::model::JsonRpcMessage::Error(_))
    {
        return StreamableHttpError::UnexpectedServerResponse(Cow::Owned(format!(
            "HTTP {status}: {body}"
        )));
    }
    StreamableHttpError::UnexpectedServerResponse(Cow::Owned(format!("HTTP {status}: {body}")))
}

/// Whether a post response is one this transport reads as a stream.
///
/// Exists so a check can name the variant without constructing one, which
/// [`StreamableHttpPostResponse`] does not otherwise allow: its `Sse` arm holds
/// a boxed stream.
#[must_use]
pub const fn is_stream(response: &StreamableHttpPostResponse) -> bool {
    matches!(response, StreamableHttpPostResponse::Sse(_, _))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Failure = StreamableHttpError<reqwest::Error>;

    #[test]
    fn the_accept_header_offers_both_media_types() {
        assert!(ACCEPT_JSON_AND_EVENT_STREAM.contains(JSON_MIME_TYPE));
        assert!(ACCEPT_JSON_AND_EVENT_STREAM.contains(EVENT_STREAM_MIME_TYPE));
    }

    #[test]
    fn a_json_success_is_read_as_a_message() {
        let outcome: Outcome =
            read_post::<reqwest::Error>(StatusCode::OK, Some("application/json"), false, true)
                .expect("a 200 with a JSON body is a message");
        assert_eq!(outcome, Outcome::Json);
    }

    #[test]
    fn an_event_stream_success_is_read_as_a_stream() {
        let outcome: Outcome =
            read_post::<reqwest::Error>(StatusCode::OK, Some("text/event-stream"), false, true)
                .expect("a 200 with an event stream is a stream");
        assert_eq!(outcome, Outcome::Sse);
    }

    #[test]
    fn a_content_type_with_parameters_still_matches() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        assert_eq!(content_type(&headers).as_deref(), Some("application/json"));
    }

    #[test]
    fn a_content_type_in_another_case_still_matches() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("Application/JSON"),
        );
        assert_eq!(content_type(&headers).as_deref(), Some("application/json"));
    }

    #[test]
    fn a_success_in_a_third_media_type_is_refused_naming_it() {
        let failure: Failure =
            read_post::<reqwest::Error>(StatusCode::OK, Some("text/html"), false, true)
                .expect_err("html is neither media type this transport offered");
        assert!(
            matches!(failure, StreamableHttpError::UnexpectedContentType(Some(ref found)) if found == "text/html"),
            "the refusal does not name what arrived: {failure:?}"
        );
    }

    #[test]
    fn an_accepted_status_needs_no_body() {
        for status in [StatusCode::ACCEPTED, StatusCode::NO_CONTENT] {
            let outcome: Outcome = read_post::<reqwest::Error>(status, None, false, false)
                .expect("202 and 204 are acceptance");
            assert_eq!(outcome, Outcome::Accepted);
        }
    }

    #[test]
    fn an_empty_success_for_a_notification_is_acceptance_and_for_a_request_is_not() {
        let notification: Outcome = read_post::<reqwest::Error>(StatusCode::OK, None, false, false)
            .expect("an empty 200 answering a notification is acceptance");
        assert_eq!(notification, Outcome::Accepted);
        let request: Failure = read_post::<reqwest::Error>(StatusCode::OK, None, false, true)
            .expect_err("an empty 200 answering a request answered nothing");
        assert!(matches!(
            request,
            StreamableHttpError::UnexpectedContentType(None)
        ));
    }

    #[test]
    fn a_404_is_an_expired_session_only_when_a_session_was_named() {
        let named: Failure = read_post::<reqwest::Error>(StatusCode::NOT_FOUND, None, true, true)
            .expect_err("a 404 on a named session is that session ending");
        assert!(matches!(named, StreamableHttpError::SessionExpired));
        let unnamed: Outcome =
            read_post::<reqwest::Error>(StatusCode::NOT_FOUND, None, false, true)
                .expect("a 404 with no session named is an ordinary refusal to read");
        assert_eq!(unnamed, Outcome::Refused);
    }

    #[test]
    fn a_401_becomes_the_challenge_and_carries_the_servers_own_words() {
        let failure: Failure = refusal(
            StatusCode::UNAUTHORIZED,
            Some("application/json"),
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32001,"message":"Missing bearer token."}}"#,
        );
        let StreamableHttpError::AuthRequired(challenge) = failure else {
            panic!("a 401 is an authentication challenge, not {failure:?}");
        };
        assert!(
            challenge
                .www_authenticate_header
                .contains("Missing bearer token."),
            "the server's own sentence was dropped: {challenge:?}"
        );
    }

    #[test]
    fn a_refusal_carries_the_status_and_the_body_and_nothing_else() {
        let failure: Failure = refusal(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some("text/plain"),
            "the cortex is unwell",
        );
        let rendered = failure.to_string();
        assert!(
            rendered.contains("500") && rendered.contains("the cortex is unwell"),
            "the refusal lost the status or the body: {rendered}"
        );
    }

    #[test]
    fn a_405_on_the_stream_says_the_server_serves_none() {
        let failure: Failure = read_stream::<reqwest::Error>(StatusCode::METHOD_NOT_ALLOWED, None)
            .expect_err("405 on the stream is a server that does not serve one");
        assert!(matches!(
            failure,
            StreamableHttpError::ServerDoesNotSupportSse
        ));
    }

    #[test]
    fn an_event_stream_is_the_only_body_the_stream_accepts() {
        read_stream::<reqwest::Error>(StatusCode::OK, Some("text/event-stream"))
            .expect("an event stream is what a notification stream is");
        let failure: Failure =
            read_stream::<reqwest::Error>(StatusCode::OK, Some("application/json"))
                .expect_err("a JSON body is not a notification stream");
        assert!(matches!(
            failure,
            StreamableHttpError::UnexpectedContentType(Some(_))
        ));
    }

    #[test]
    fn the_headers_this_transport_sets_may_not_be_supplied_by_a_caller() {
        for owned in ["accept", "Mcp-Session-Id", "Last-Event-Id"] {
            let name = HeaderName::from_bytes(owned.as_bytes()).expect("a valid header name");
            assert!(
                reserved(&name).is_err(),
                "{owned} is this transport's to set and was allowed through"
            );
        }
    }

    #[test]
    fn the_protocol_version_header_is_reserved_and_allowed_through() {
        // The SDK's worker injects it after initialisation, so a client that
        // refused it would refuse the SDK's own request.
        let name = HeaderName::from_static("mcp-protocol-version");
        assert!(reserved(&name).is_ok());
    }

    #[test]
    fn an_ordinary_header_is_allowed_through() {
        let name = HeaderName::from_static("x-request-id");
        assert!(
            reserved(&name).is_ok(),
            "the reserved check refuses everything, which is not a check"
        );
    }

    #[test]
    fn a_reserved_header_is_refused_whatever_case_it_is_written_in() {
        let name = HeaderName::from_bytes(b"MCP-SESSION-ID").expect("a valid header name");
        assert!(reserved(&name).is_err());
    }
}
