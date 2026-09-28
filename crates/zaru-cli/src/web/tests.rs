// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `web.fetch` against a real socket.
//!
//! # Why these are in-crate and the refusals are outside
//!
//! Every check here needs a server, the CI runner has no network, so the
//! server is a `std::net::TcpListener` on `127.0.0.1` serving canned bytes —
//! which [`Destinations::public`](super::url::Destinations::public) refuses.
//! Reaching it needs
//! [`WebClient::reaching_this_machine`](super::client::WebClient), which is
//! `#[cfg(test)]` and therefore **absent from the rlib** the binary and every
//! integration test link. So the successful-retrieval path can only be driven
//! from here, on the precedent of `crate::process`'s own checks, which drive
//! **real child processes** the same way.
//!
//! Every **refusal** is driven from outside the crate over the real client, in
//! `tests/fetch_from_outside.rs`, because a refusal needs no server at all.
//! The division is the cost of the destination rule and is stated rather than
//! hidden.
//!
//! Nothing here reaches a network. The only real effect is a listener on an
//! ephemeral loopback port, torn down when the check ends.

use crate::tools::output::Captured;
use crate::tools::port::Retrieved;
use crate::web::bounds::{BodyCeiling, FetchBounds, FetchTimeout, RedirectLimit};
use crate::web::client::WebClient;
use crate::web::url::{Destinations, RequestedUrl, UrlRefused};
use core::time::Duration;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};

// ------------------------------------------------------------------ staging

/// A server the check owns, serving canned responses in order.
///
/// It records the **whole request** of every connection, headers and all, so
/// that an assertion about what the harness sent is made against what a
/// server received rather than against what the client meant to send —
/// verification lessons §10, "assert the consequence, never a proxy the code
/// already computes".
struct Listener {
    address: SocketAddr,
    received: Arc<Mutex<Vec<String>>>,
}

impl Listener {
    /// Serve `responses` in order, one per connection, then stop.
    fn serving(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("staging: a loopback port");
        let address = listener.local_addr().expect("staging: the bound address");
        let received = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&received);
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                // Read the request head. A `GET` has no body, so the blank
                // line ends it; reading to end-of-stream would block until
                // the client hung up.
                let mut head = String::new();
                let mut reader = BufReader::new(stream.try_clone().expect("staging: a clone"));
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let blank = line == "\r\n" || line == "\n";
                            head.push_str(&line);
                            if blank {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                recorder
                    .lock()
                    .expect("staging: the recorder is not poisoned")
                    .push(head);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self { address, received }
    }

    /// A server that accepts a connection, reads nothing and never answers.
    fn silent() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("staging: a loopback port");
        let address = listener.local_addr().expect("staging: the bound address");
        let received = Arc::new(Mutex::new(Vec::new()));
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                // Held open, answering nothing, until the check ends.
                std::thread::sleep(Duration::from_secs(30));
                drop(stream);
            }
        });
        Self { address, received }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    /// The same server addressed by a **different host string**.
    ///
    /// `localhost` and `127.0.0.1` are two hosts to the rule and one machine
    /// to the operating system, which is what lets a cross-host redirect be
    /// staged entirely on loopback with no second interface.
    fn url_as_localhost(&self, path: &str) -> String {
        format!("http://localhost:{}{path}", self.address.port())
    }

    fn requests(&self) -> Vec<String> {
        self.received
            .lock()
            .expect("the recorder is not poisoned")
            .clone()
    }
}

/// One HTTP response, built here rather than by a crate.
fn response(status: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    out.push_str("Connection: close\r\n\r\n");
    out.push_str(body);
    out
}

/// One HTTP response with **no `Content-Length`**, delimited by the
/// connection closing.
///
/// This is the framing under which a body's size is unknown until it has been
/// read, which is the only shape that reaches the read loop's bound.
fn unframed(status: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    out.push_str(body);
    out
}

fn ok(body: &str) -> String {
    response(
        "200 OK",
        &[("Content-Type", "text/plain; charset=utf-8")],
        body,
    )
}

fn redirect_to(target: &str) -> String {
    response("302 Found", &[("Location", target)], "")
}

fn bounds(body: u64, timeout: Duration, redirects: usize) -> FetchBounds {
    FetchBounds {
        body: BodyCeiling::new(body).expect("staging: not zero"),
        timeout: FetchTimeout::new(timeout).expect("staging: not zero"),
        redirects: RedirectLimit::new(redirects),
    }
}

fn generous() -> FetchBounds {
    bounds(1 << 20, Duration::from_secs(10), 3)
}

fn client(bounds: FetchBounds) -> WebClient {
    WebClient::reaching_this_machine(bounds).expect("staging: a client builds")
}

async fn fetch(client: &WebClient, url: &str) -> Captured {
    let requested = RequestedUrl::parse(url).expect("staging: the URL parses");
    match client.retrieve(&requested, 0).await {
        Retrieved::Captured(captured) => captured,
        Retrieved::Elsewhere { to } => panic!("the retrieval was pointed at {}", to.as_str()),
    }
}

/// A retrieval that may hand back where another host was pointed at.
async fn fetch_or_elsewhere(client: &WebClient, url: &str, followed: usize) -> Retrieved {
    let requested = RequestedUrl::parse(url).expect("staging: the URL parses");
    client.retrieve(&requested, followed).await
}

// ------------------------------------------------------- the parse and scheme

#[test]
fn the_two_retrievable_schemes_are_accepted_and_every_other_one_is_refused_by_name() {
    // The accepting arm first: a table with no accepting arm is satisfied by
    // a parser that refuses everything.
    for accepted in [
        "http://example.invalid/",
        "https://example.invalid/",
        "HTTPS://example.invalid/",
    ] {
        RequestedUrl::parse(accepted).unwrap_or_else(|refused| {
            panic!("{accepted} should be retrievable, and was {refused}")
        });
    }

    let mut wrong = Vec::new();
    for (offered, scheme) in [
        ("file:///etc/passwd", "file"),
        ("data:text/plain,hello", "data"),
        ("ftp://example.invalid/x", "ftp"),
        ("gopher://example.invalid/x", "gopher"),
        ("ws://example.invalid/x", "ws"),
    ] {
        match RequestedUrl::parse(offered) {
            Err(UrlRefused::SchemeNotRetrievable { scheme: named }) if named == scheme => {}
            other => wrong.push(format!(
                "{offered} gave {other:?}, wanted the scheme {scheme:?}"
            )),
        }
    }
    assert!(
        wrong.is_empty(),
        "every scheme but http and https is refused naming itself: {wrong:#?}"
    );
}

#[test]
fn a_refusal_never_renders_the_url_it_refused() {
    // The url field is where a token pasted into a query string would be, and
    // a refusal is exactly the text that gets copied into a bug report. The
    // needle carries a combining mark and a multi-byte grapheme, so an
    // escaping writer publishes every byte of it while the value as typed is
    // absent -- verification lessons §50 and §63.
    // The combining mark sits at the END of the ASCII run, not after the
    // first character: `ascii_core` stops at the first non-ASCII byte, so a
    // needle spelled `s\u{0301}ecret` has the core `"s"` -- one byte, present
    // in almost every sentence, and an assertion that can never fail. That is
    // verification lessons §51 from the other side: the fixture was awkward
    // on one axis and useless on the axis the assertion moves. The staging
    // assertion below is what stops it happening again.
    let needle = "planted-secret-value\u{0301}-Ω";
    let core = crate::redaction::ascii_core(needle);
    assert!(
        core.len() > 8,
        "staging: the needle's ASCII core is what survives escaping, and a short one matches \
         everything; it was {core:?}"
    );
    let mut leaked = Vec::new();
    for offered in [
        format!("file:///tmp/{needle}"),
        format!("gopher://host.invalid/{needle}"),
        format!("not a url at all {needle}"),
        format!("https:///{needle}"),
    ] {
        let Err(refused) = RequestedUrl::parse(&offered) else {
            continue;
        };
        let rendered = format!("{refused} {refused:?}");
        if rendered.contains(needle) || rendered.contains(core) {
            leaked.push(format!("{offered} rendered as {rendered}"));
        }
    }
    assert!(
        leaked.is_empty(),
        "a refusal on the URL path renders the scheme and the parser's position, never the URL \
         itself: {leaked:#?}"
    );
}

// ------------------------------------------------------- the destination rule

#[test]
fn this_machine_and_link_local_are_refused_by_name_and_everything_else_is_reached() {
    let public = Destinations::public();
    let mut wrong = Vec::new();

    // Refusing arms. `2130706433` and `0177.0.0.1` are 127.0.0.1 in decimal
    // and octal; `::ffff:127.0.0.1` is it wearing an IPv6 spelling; `0.0.0.0`
    // connects to this machine on Linux. Each is a spelling, not a second
    // rule, and each is here because it is where a spelling would slip
    // through.
    for refused in [
        "http://127.0.0.1/",
        "http://127.0.0.53/",
        "http://localhost/",
        "http://LOCALHOST/",
        "http://api.localhost/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://0.0.0.0/",
        "http://[::]/",
        "http://2130706433/",
        "http://0177.0.0.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://169.254.1.1/",
        "http://[fe80::1]/",
    ] {
        let url = reqwest::Url::parse(refused).expect("staging: the URL parses");
        if public.refuses(&url).is_none() {
            wrong.push(format!("{refused} was reached and must not be"));
        }
    }

    // Accepting arms, including the private ranges that are deliberately NOT
    // refused: refusing them would be a judgement about a user's own network.
    for reached in [
        "http://example.invalid/",
        "https://93.184.216.34/",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "http://172.16.0.1/",
        "http://[2001:db8::1]/",
        "http://localhostage.invalid/",
        "http://notlocalhost/",
    ] {
        let url = reqwest::Url::parse(reached).expect("staging: the URL parses");
        if let Some(refusal) = public.refuses(&url) {
            wrong.push(format!(
                "{reached} was refused as {refusal} and must not be"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "the destination rule disagreed: {wrong:#?}"
    );
}

#[test]
fn the_check_s_own_reach_widens_this_machine_and_never_link_local() {
    // The permissive constructor exists so a check can serve its own
    // response. It must not become an exemption from the rule the corpus is
    // about: a metadata endpoint stays refused even here, so no check can
    // pass by reaching one.
    let permissive = Destinations::including_this_machine();
    let loopback = reqwest::Url::parse("http://127.0.0.1:8080/").expect("staging");
    let metadata = reqwest::Url::parse("http://169.254.169.254/").expect("staging");
    assert!(
        permissive.refuses(&loopback).is_none(),
        "the check's own reach includes this machine, or it could not serve itself a response"
    );
    assert!(
        permissive.refuses(&metadata).is_some(),
        "no reach admits link-local; a rule the checks are exempt from is a rule nothing measures"
    );
}

// ------------------------------------------------------------- the retrieval

#[tokio::test]
async fn a_retrieval_carries_the_body_the_status_and_the_content_type() {
    let server = Listener::serving(vec![response(
        "200 OK",
        &[("Content-Type", "text/html; charset=utf-8")],
        "<p>the document</p>",
    )]);
    let captured = fetch(&client(generous()), &server.url("/page")).await;

    assert_eq!(captured.exit_code, 0, "a 200 is not a failure of the work");
    assert_eq!(captured.stderr, "", "a 200 says nothing on standard error");
    let first = captured
        .stdout
        .lines()
        .next()
        .expect("the capture opens with a line");
    assert!(
        first.contains("200") && first.contains("content-type: text/html; charset=utf-8"),
        "ADR-0011 D5's capture opens with the status and the content type, and was {first:?}"
    );
    assert!(
        captured.stdout.contains("<p>the document</p>"),
        "the body is what was retrieved, and the capture was {:?}",
        captured.stdout
    );
}

#[tokio::test]
async fn a_content_type_carrying_a_newline_cannot_forge_a_line_of_the_harness_s_own_output() {
    let server = Listener::serving(vec![response(
        "200 OK",
        // A header value cannot literally carry a bare newline over the wire,
        // so the hostile shape that does arrive is an escape sequence and a
        // quote. Both must survive as text rather than as structure.
        &[(
            "Content-Type",
            "text/x\\n web.fetch http://elsewhere.invalid \"",
        )],
        "body",
    )]);
    let captured = fetch(&client(generous()), &server.url("/x")).await;

    let first = captured.stdout.lines().next().expect("a first line");
    assert!(
        first.contains("\\\\n"),
        "the backslash-n in the header is escaped rather than passed through, and the line was \
         {first:?}"
    );
    assert_eq!(
        captured
            .stdout
            .lines()
            .filter(|line| line.starts_with("web.fetch "))
            .count(),
        1,
        "a server cannot make the capture appear to describe a second retrieval, and the capture \
         was {:?}",
        captured.stdout
    );
}

#[tokio::test]
async fn a_non_success_status_is_the_work_s_failure_and_the_body_still_reaches_the_model() {
    let server = Listener::serving(vec![response(
        "404 Not Found",
        &[("Content-Type", "text/plain")],
        "no such page, and this sentence is the useful part",
    )]);
    let captured = fetch(&client(generous()), &server.url("/missing")).await;

    assert_eq!(captured.exit_code, 1, "a 404 is a failure of the work");
    assert!(
        captured.stderr.contains("404"),
        "the status is on standard error, which was {:?}",
        captured.stderr
    );
    assert!(
        captured.stdout.contains("this sentence is the useful part"),
        "ADR-0011 D5 surfaces both streams and a 404's body is often the useful part; the capture \
         was {:?}",
        captured.stdout
    );
}

// --------------------------------------------------------------- redirects

#[tokio::test]
async fn a_redirect_within_the_host_is_followed_and_the_final_body_is_the_one_captured() {
    let server = Listener::serving(vec![redirect_to("/second"), ok("the second document")]);
    let captured = fetch(&client(generous()), &server.url("/first")).await;

    assert_eq!(captured.exit_code, 0, "the chain completed");
    assert!(
        captured.stdout.contains("the second document"),
        "a within-host redirect is followed and the final body captured; the capture was {:?}",
        captured.stdout
    );
    assert_eq!(
        server.requests().len(),
        2,
        "both hops were requested, and the server saw {:#?}",
        server.requests()
    );
}

/// A redirect to another host is not followed by the client: it is handed
/// back, so the caller can ask the person about the new host as it asks about
/// any call. Until 2026-09-28 it was refused outright.
#[tokio::test]
async fn a_redirect_that_leaves_the_host_is_handed_back_and_the_other_host_is_never_reached() {
    let elsewhere = Listener::serving(vec![ok("the document behind the redirect")]);
    let target = elsewhere.url_as_localhost("/taken");
    let asked = Listener::serving(vec![redirect_to(&target)]);

    let retrieved = fetch_or_elsewhere(&client(generous()), &asked.url("/start"), 0).await;

    match retrieved {
        Retrieved::Elsewhere { to } => assert_eq!(
            to.as_str(),
            target,
            "the client handed back a URL other than the one the server pointed at"
        ),
        Retrieved::Captured(captured) => panic!(
            "a redirect to another host was not handed back to the caller to decide: {captured:?}"
        ),
    }
    // The assertion that matters: the other host was never connected to.
    assert!(
        elsewhere.requests().is_empty(),
        "the host the redirect pointed at received {:#?}, and must have received nothing",
        elsewhere.requests()
    );
}

/// The redirect limit holds across hosts as it holds within one: a caller
/// that has already followed as many as the limit is refused the next.
#[tokio::test]
async fn a_redirect_to_another_host_past_the_limit_is_refused() {
    let elsewhere = Listener::serving(vec![ok("the document behind the redirect")]);
    let asked = Listener::serving(vec![redirect_to(&elsewhere.url_as_localhost("/taken"))]);

    let retrieved = fetch_or_elsewhere(&client(generous()), &asked.url("/start"), 3).await;

    let Retrieved::Captured(captured) = retrieved else {
        panic!("a redirect past the limit was handed back to be followed");
    };
    assert_eq!(captured.exit_code, 1, "a refused chain is not a success");
    assert!(
        captured
            .stderr
            .contains("the limit of 3 redirect(s) within one host was reached"),
        "the capture names the limit, and was {:?}",
        captured.stderr
    );
    assert!(elsewhere.requests().is_empty());
}

/// Each hop is checked, and the one that leaves the host is the second of
/// three here, so "checks only the first" and "checks only the last" both
/// redden.
#[tokio::test]
async fn the_hop_predicate_holds_at_every_hop_and_not_only_the_first() {
    let elsewhere = Listener::serving(vec![ok("the document behind the second hop")]);
    let asked = Listener::serving(vec![
        redirect_to("/two"),
        redirect_to(&elsewhere.url_as_localhost("/taken")),
        ok("a third hop nobody should reach"),
    ]);

    let retrieved = fetch_or_elsewhere(&client(generous()), &asked.url("/one"), 0).await;

    assert!(
        matches!(retrieved, Retrieved::Elsewhere { .. }),
        "the second hop left the host and was not handed back: {retrieved:?}"
    );
    assert_eq!(
        asked.requests().len(),
        2,
        "the first hop was followed and the second was not, so the asked host saw exactly two \
         requests and saw {:#?}",
        asked.requests()
    );
    assert!(
        elsewhere.requests().is_empty(),
        "the second hop's host received {:#?}, and must have received nothing",
        elsewhere.requests()
    );
}

#[tokio::test]
async fn the_within_host_limit_stops_a_chain_and_the_limit_is_the_caller_s() {
    let server = Listener::serving(vec![
        redirect_to("/two"),
        redirect_to("/three"),
        ok("a document behind two redirects"),
    ]);
    let captured = fetch(
        &client(bounds(1 << 20, Duration::from_secs(10), 1)),
        &server.url("/one"),
    )
    .await;

    assert_eq!(
        server.requests().len(),
        2,
        "a limit of one follows one redirect and no more, and the server saw {:#?}",
        server.requests()
    );
    assert!(
        captured.stderr.contains("limit"),
        "the capture says the chain stopped on the count rather than on a rule, and was {:?}",
        captured.stderr
    );

    // The accepting sibling: the same chain one hop shorter completes, so a
    // client that refused every redirect cannot pass.
    let shorter = Listener::serving(vec![
        redirect_to("/two"),
        ok("a document behind one redirect"),
    ]);
    let completed = fetch(
        &client(bounds(1 << 20, Duration::from_secs(10), 1)),
        &shorter.url("/one"),
    )
    .await;
    assert!(
        completed.stdout.contains("a document behind one redirect"),
        "a chain within the limit completes, and the capture was {:?}",
        completed.stdout
    );
}

#[tokio::test]
async fn a_limit_of_zero_follows_nothing_and_is_a_limit_rather_than_a_refusal() {
    let server = Listener::serving(vec![redirect_to("/two"), ok("unreachable at this limit")]);
    let captured = fetch(
        &client(bounds(1 << 20, Duration::from_secs(10), 0)),
        &server.url("/one"),
    )
    .await;

    assert_eq!(
        server.requests().len(),
        1,
        "zero redirects follows none, and the server saw {:#?}",
        server.requests()
    );
    assert!(
        !captured.stdout.contains("unreachable at this limit"),
        "nothing behind the redirect was captured, and the capture was {:?}",
        captured.stdout
    );
}

// ------------------------------------------------------------- the ceiling

#[tokio::test]
async fn a_body_over_the_ceiling_is_refused_whole_and_never_captured_as_a_prefix() {
    let ceiling = 64;
    let body = "x".repeat(usize::try_from(ceiling).expect("small") + 1);
    // The honest arm: the server declares its length and the retrieval is
    // refused before a byte is read.
    let honest = Listener::serving(vec![ok(&body)]);
    let refused = fetch(
        &client(bounds(ceiling, Duration::from_secs(10), 3)),
        &honest.url("/big"),
    )
    .await;

    assert_eq!(refused.exit_code, 1, "an oversized body is not a success");
    assert!(
        refused.stdout.is_empty(),
        "nothing is captured, because a prefix passed off as the document is exactly what D5 \
         forbids; the capture was {:?}",
        refused.stdout
    );
    assert!(
        refused.stderr.contains("refused rather than cut short")
            && refused.stderr.contains("declared"),
        "the refusal names the ceiling and says the server declared the size, and was {:?}",
        refused.stderr
    );

    // The arm that holds, and the reason it is this one rather than a server
    // that lies about its length. A lying `Content-Length` buys nothing: hyper
    // honours the declared length and hands back exactly that many bytes, so
    // a check staged that way measures HTTP's own framing and never reaches
    // the loop. **A response with no `Content-Length` at all** is the case the
    // cheap arm cannot see -- the body is delimited by the connection closing,
    // `content_length()` is `None`, and the read is the only thing bounding
    // it. Found by running the first staging rather than by reading it.
    let unframed = Listener::serving(vec![unframed(
        "200 OK",
        &[("Content-Type", "text/plain")],
        &body,
    )]);
    let caught = fetch(
        &client(bounds(ceiling, Duration::from_secs(10), 3)),
        &unframed.url("/big"),
    )
    .await;
    assert!(
        caught.stdout.is_empty(),
        "a body the server never declared a length for is bounded by the read, and the capture \
         was {:?}",
        caught.stdout
    );
    assert!(
        caught.stderr.contains("while being read"),
        "the refusal says which arm caught it, so the two are told apart rather than counted \
         together; it was {:?}",
        caught.stderr
    );

    // The accepting sibling: one byte under the ceiling is captured whole.
    let small = "y".repeat(usize::try_from(ceiling).expect("small") - 1);
    let fits = Listener::serving(vec![ok(&small)]);
    let taken = fetch(
        &client(bounds(ceiling, Duration::from_secs(10), 3)),
        &fits.url("/small"),
    )
    .await;
    assert!(
        taken.stdout.contains(&small),
        "a body under the ceiling is captured whole, and the capture was {:?}",
        taken.stdout
    );
}

// -------------------------------------------------------------- the timeout

#[tokio::test]
async fn a_server_that_never_answers_is_ended_by_the_caller_s_timeout() {
    let silent = Listener::silent();
    let started = std::time::Instant::now();
    let captured = fetch(
        &client(bounds(1 << 20, Duration::from_millis(300), 3)),
        &silent.url("/hang"),
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(
        captured.exit_code, 1,
        "a retrieval that never answered is not a success"
    );
    assert!(
        captured.stderr.contains("could not retrieve"),
        "the capture says the retrieval did not complete, and was {:?}",
        captured.stderr
    );
    // Bounded by the check's own clock rather than by a sleep: a mutation
    // that deletes the timeout makes this wait for the listener's thirty
    // seconds, which is a deterministic failure rather than a widened race
    // -- verification lessons §57.
    assert!(
        elapsed < Duration::from_secs(10),
        "the timeout ended it, and it took {elapsed:?}"
    );
}

// -------------------------------------------------- what the request carries

#[tokio::test]
async fn a_request_carries_the_url_the_model_asked_for_and_no_credential_of_the_harness_s() {
    // The nonce carries a combining mark and a multi-byte grapheme, so an
    // encoder that escaped it would publish every byte while the value as
    // typed was absent -- verification lessons §50 and §63. Its ASCII core is
    // asserted beside it.
    let planted = "nn_mcp_pla\u{0301}nted-Ω-bearer";
    let core = crate::redaction::ascii_core(planted);
    assert!(
        core.len() > 8,
        "staging: the planted value has an ASCII core"
    );

    let server = Listener::serving(vec![ok("a document")]);
    // The accepting arm is in the same run: a nonce the MODEL put in the
    // query string must reach the server, or a listener that received nothing
    // would satisfy the absence assertion on its own.
    let asked = format!("{}?marker=reached-the-server", server.url("/page"));
    let captured = fetch(&client(generous()), &asked).await;
    assert_eq!(captured.exit_code, 0, "the retrieval succeeded");

    let requests = server.requests();
    let request = requests.first().expect("the server received a request");
    assert!(
        request.contains("reached-the-server"),
        "what the model asked for reached the server, or this check asserts nothing; the request \
         was {request:?}"
    );
    assert!(
        request.starts_with("GET "),
        "web.fetch issues a GET and nothing else; the request was {request:?}"
    );
    let lowered = request.to_ascii_lowercase();
    for header in [
        "authorization:",
        "cookie:",
        "x-goog-api-key:",
        "proxy-authorization:",
    ] {
        assert!(
            !lowered.contains(header),
            "web.fetch adds no credential header; the request carried {header} and was {request:?}"
        );
    }
    assert!(
        !request.contains(planted) && !request.contains(core),
        "a value the harness holds cannot travel on a request, because there is no parameter it \
         could arrive through; the request was {request:?}"
    );
}

#[tokio::test]
async fn no_cookie_is_ever_sent_back_after_a_server_sets_one() {
    let server = Listener::serving(vec![
        response(
            "200 OK",
            &[
                ("Content-Type", "text/plain"),
                ("Set-Cookie", "session=planted; Path=/"),
            ],
            "first",
        ),
        ok("second"),
    ]);
    let http = client(generous());
    let first = fetch(&http, &server.url("/one")).await;
    assert_eq!(first.exit_code, 0, "the first retrieval succeeded");
    let second = fetch(&http, &server.url("/two")).await;
    assert_eq!(second.exit_code, 0, "the second retrieval succeeded");

    let requests = server.requests();
    assert_eq!(
        requests.len(),
        2,
        "the second request was made at all, or the absence below is about nothing; the server \
         saw {requests:#?}"
    );
    assert!(
        !requests[1].to_ascii_lowercase().contains("cookie:"),
        "the second request carries no cookie, because `reqwest`'s cookie feature has no caller \
         in this workspace and there is no jar to keep one; the request was {:?}",
        requests[1]
    );
}

// ------------------------------------------ asking about a URL, and its host

/// A confirmer that gives its answers in order, and records every question.
struct Answers {
    given: Mutex<std::collections::VecDeque<crate::tools::port::Answer>>,
    asked: Mutex<Vec<String>>,
}

impl Answers {
    fn in_order(answers: &[crate::tools::port::Answer]) -> Self {
        Self {
            given: Mutex::new(answers.iter().copied().collect()),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().expect("not poisoned").clone()
    }
}

impl crate::tools::port::Confirm for Answers {
    fn confirm(
        &self,
        question: &crate::tools::port::Question,
    ) -> Result<crate::tools::port::Answer, crate::tools::port::ConfirmFailure> {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(question.statement.clone());
        Ok(self
            .given
            .lock()
            .expect("not poisoned")
            .pop_front()
            .unwrap_or(crate::tools::port::Answer::No))
    }
}

/// An allowlist of `tools.allowlist` lines, as a person writes them.
struct Lines(Vec<crate::tools::Entry>);

impl crate::tools::port::Allowlist for Lines {
    fn approves(&self, invocation: &crate::tools::Invocation<'_>) -> bool {
        self.0.iter().any(|entry| entry.approves(invocation))
    }
}

struct NoCommands;

impl crate::tools::port::Subprocess for NoCommands {
    async fn run(
        &self,
        _line: &crate::process::line::CommandLine,
    ) -> Result<Captured, zaru_core::iteration::PortFailure> {
        Err(zaru_core::iteration::PortFailure::new(
            "these checks run no command",
        ))
    }
}

/// What one `web.fetch` produced through the executor, and what was asked.
struct Fetched {
    outcome: zaru_core::tool_call::ToolOutcome,
    transcript: String,
}

/// Run `web.fetch` calls through the real executor over a client that may
/// reach this machine, and hand back each outcome.
async fn through_the_executor(
    mode: crate::tools::Mode,
    allowlist: &Lines,
    confirmer: Option<&(dyn crate::tools::port::Confirm + Sync)>,
    grants: &crate::tools::grants::SessionGrants,
    urls: &[&str],
) -> Vec<Fetched> {
    use zaru_core::tool_call::ToolExecutor as _;
    let base = std::env::temp_dir().join(format!(
        "web-exec-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    std::fs::create_dir_all(&base).expect("staging: a scratch directory");
    let working = crate::tools::WorkingDirectory::at(&base).expect("resolves");
    let transcript_path = base.join("transcript.jsonl");
    let mut transcript =
        crate::session::Transcript::append_to(&transcript_path).expect("the transcript opens");
    let mut overflow = crate::tools::SessionOverflow::in_session(&base);
    let web = client(generous());
    let mut fetched = Vec::new();
    {
        let mut executor = crate::tools::Executor {
            working_directory: &working,
            mode,
            allowlist,
            destructive: &crate::tools::Shapes,
            session_grants: grants,
            confirmer,
            verdicts: &crate::tools::NoMembrane,
            budget: crate::tools::OutputBudget::new(4096).expect("a usable budget"),
            preview_budget: crate::tools::OutputBudget::new(4096).expect("a usable budget"),
            search_ceiling: crate::cli::layers::search_ceiling(),
            overflow: &mut overflow,
            transcript: &mut transcript,
            redactor: &crate::redaction::HeldSecrets::none(),
            subprocess: &NoCommands,
            fetch: &web,
            projected: &crate::tools::NoProjection,
            declared: crate::tools::descriptor_set(),
        };
        for (at, url) in urls.iter().enumerate() {
            let outcome = executor
                .execute(&zaru_core::tool_call::ToolRequest {
                    id: format!("f{at}"),
                    name: String::from("web.fetch"),
                    arguments: serde_json::json!({ "url": url }).to_string(),
                })
                .await
                .expect("a retrieval is never a port failure");
            fetched.push(Fetched {
                outcome,
                transcript: String::new(),
            });
        }
    }
    let written = std::fs::read_to_string(&transcript_path).unwrap_or_default();
    for one in &mut fetched {
        one.transcript.clone_from(&written);
    }
    let _ = std::fs::remove_dir_all(&base);
    fetched
}

/// What the model was given for a completed call, or why it was refused.
fn given(outcome: &zaru_core::tool_call::ToolOutcome) -> String {
    match outcome {
        zaru_core::tool_call::ToolOutcome::Completed { result, .. } => {
            result.content.as_str().to_owned()
        }
        zaru_core::tool_call::ToolOutcome::Refused { because, .. } => because.clone(),
    }
}

/// A redirect to another host is asked about again, showing the new URL; a no
/// stops there and the other host is never reached; a yes follows it.
#[tokio::test]
async fn a_redirect_to_another_host_is_asked_about_and_followed_only_on_a_yes() {
    use crate::tools::port::Answer;

    // Declined at the second host.
    let elsewhere = Listener::serving(vec![ok("the document behind the redirect")]);
    let target = elsewhere.url_as_localhost("/taken");
    let asked_host = Listener::serving(vec![redirect_to(&target)]);
    let confirmer = Answers::in_order(&[Answer::Once, Answer::No]);
    let fetched = through_the_executor(
        crate::tools::Mode::Ask,
        &Lines(Vec::new()),
        Some(&confirmer),
        &crate::tools::grants::SessionGrants::none(),
        &[&asked_host.url("/start")],
    )
    .await;
    let asked = confirmer.asked();
    assert_eq!(
        asked.len(),
        2,
        "a redirect to another host must be asked about as a call of its own: {asked:?}"
    );
    assert!(
        asked[1].contains(&target),
        "the second question shows the new URL: {asked:?}"
    );
    assert!(
        given(&fetched[0].outcome).contains("was not followed"),
        "the model is told the redirect was not followed: {}",
        given(&fetched[0].outcome)
    );
    assert!(
        elsewhere.requests().is_empty(),
        "the host the person declined received {:#?}",
        elsewhere.requests()
    );
    assert!(
        fetched[0].transcript.contains(&target),
        "the hop is on the transcript as a call of its own: {}",
        fetched[0].transcript
    );

    // Allowed at the second host.
    let elsewhere = Listener::serving(vec![ok("the document behind the redirect")]);
    let asked_host = Listener::serving(vec![redirect_to(&elsewhere.url_as_localhost("/taken"))]);
    let confirmer = Answers::in_order(&[Answer::Once, Answer::Once]);
    let fetched = through_the_executor(
        crate::tools::Mode::Ask,
        &Lines(Vec::new()),
        Some(&confirmer),
        &crate::tools::grants::SessionGrants::none(),
        &[&asked_host.url("/start")],
    )
    .await;
    assert!(
        given(&fetched[0].outcome).contains("the document behind the redirect"),
        "a redirect the person allowed was not followed: {}",
        given(&fetched[0].outcome)
    );
    assert_eq!(elsewhere.requests().len(), 1);
}

/// With nobody to ask, a redirect to another host is refused, even when the
/// first host was on the allowlist; and the allowlist may name a host.
#[tokio::test]
async fn with_nobody_to_ask_a_redirect_to_another_host_is_refused() {
    let elsewhere = Listener::serving(vec![ok("the document behind the redirect")]);
    let asked_host = Listener::serving(vec![redirect_to(&elsewhere.url_as_localhost("/taken"))]);
    let allowlist = Lines(vec![
        crate::tools::Entry::parse(1, "web.fetch 127.0.0.1").expect("a host entry parses"),
    ]);
    let fetched = through_the_executor(
        crate::tools::Mode::Allow,
        &allowlist,
        None,
        &crate::tools::grants::SessionGrants::none(),
        &[&asked_host.url("/start")],
    )
    .await;
    let said = given(&fetched[0].outcome);
    assert!(
        matches!(
            fetched[0].outcome,
            zaru_core::tool_call::ToolOutcome::Completed { .. }
        ),
        "the first host is on the allowlist, so the call itself ran: {said}"
    );
    assert!(
        said.contains("was not followed") && said.contains("did not reach them"),
        "with nobody to ask the redirect must be refused, saying why: {said}"
    );
    assert!(
        elsewhere.requests().is_empty(),
        "the other host was reached with nobody to ask: {:#?}",
        elsewhere.requests()
    );
    assert_eq!(asked_host.requests().len(), 1);
}

/// `h` allows every URL on the asked host for the rest of the session, and
/// no other host.
#[tokio::test]
async fn a_host_allowed_for_the_session_is_not_asked_about_again_and_no_other_host_is() {
    use crate::tools::port::Answer;
    let server = Listener::serving(vec![ok("one"), ok("two"), ok("three")]);
    let confirmer = Answers::in_order(&[Answer::ForThisHost, Answer::No]);
    let grants = crate::tools::grants::SessionGrants::none();
    let fetched = through_the_executor(
        crate::tools::Mode::Ask,
        &Lines(Vec::new()),
        Some(&confirmer),
        &grants,
        &[
            &server.url("/one"),
            &server.url("/two?other=page"),
            &server.url_as_localhost("/three"),
        ],
    )
    .await;
    let asked = confirmer.asked();
    assert_eq!(
        asked.len(),
        2,
        "the second URL on the allowed host was asked about, or the other host was not: {asked:?}"
    );
    assert!(asked[1].contains("localhost"), "{asked:?}");
    assert!(given(&fetched[1].outcome).contains("two"));
    assert!(matches!(
        fetched[2].outcome,
        zaru_core::tool_call::ToolOutcome::Refused { .. }
    ));
}

/// An allowlist host entry matches that host exactly, and nothing that merely
/// starts or ends with it.
#[test]
fn an_allowlist_host_entry_matches_that_host_and_no_other() {
    let entry = crate::tools::Entry::parse(1, "web.fetch docs.rs").expect("parses");
    for (url, expected) in [
        ("https://docs.rs/serde", true),
        ("http://DOCS.RS/x?y=z", true),
        ("https://docs.rs:8443/x", true),
        ("https://docs.rs.evil.example/x", false),
        ("https://evil-docs.rs/x", false),
        ("https://sub.docs.rs/x", false),
    ] {
        let requested = RequestedUrl::parse(url).expect("parses");
        let invocation = crate::tools::Invocation::fetching(&requested);
        assert_eq!(entry.approves(&invocation), expected, "{url}");
    }
    // A whole URL is still matched byte for byte.
    let exact = crate::tools::Entry::parse(1, "web.fetch https://docs.rs/serde").expect("parses");
    let other = RequestedUrl::parse("https://docs.rs/tokio").expect("parses");
    assert!(!exact.approves(&crate::tools::Invocation::fetching(&other)));
}
