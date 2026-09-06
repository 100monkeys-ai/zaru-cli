// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The checks that need a real Nuclear Notes instance, and a real credential.
//!
//! # Every check here is `#[ignore]`d, and that is the honest shape
//!
//! The gate this workspace runs on has **no network**, and the fleet's standing
//! ruling forbids a loopback listener standing in for a server. Neither is a
//! reason to have no checks against the real substrate — it is a reason for
//! them not to run on the gate. So they are here, ignored by default, with the
//! environment they need named in one place, and `cargo test -p zaru-notes
//! --test notes_live -- --ignored` runs them wherever a token exists.
//!
//! **A check nobody can re-run is a claim rather than evidence.** The shape
//! [ADR-0007] clause 4's keyring arm already uses is the shape here: built,
//! exercised by hand against the real thing, and explicitly not claimed as
//! satisfied by the suite. What this file adds over a session transcript
//! somebody pasted into a report is that the next contributor can run it.
//!
//! # The credential is read from a file, never from an argument or a variable
//!
//! `ZARU_NOTES_LIVE_TOKEN_FILE` names a path; the token is its contents. An
//! argument is in `/proc/<pid>/cmdline` and in `ps` for every user on the
//! machine; an environment variable is in `/proc/<pid>/environ` for the length
//! of the run. A file at `0600` is readable by its owner, and the process
//! library's [Credentials] rules say what to do with it afterwards in as many
//! words: "remove it when the operation completes, and then verify the
//! removal", with a control that discriminates.
//!
//! **Nothing here prints the token, and one check asserts that of every
//! failure this crate can raise against a live server.**
//!
//! # What each variable is
//!
//! | Variable | What it names |
//! | --- | --- |
//! | `ZARU_NOTES_LIVE_TOKEN_FILE` | a file whose contents are one bearer value |
//! | `ZARU_NOTES_LIVE_HOST` | the instance host, as `Instance` takes it |
//! | `ZARU_NOTES_LIVE_WORKSPACE` | a workspace id the token can read pages from |
//! | `ZARU_NOTES_LIVE_MEMBER_WORKSPACE` | a workspace id the token is a **member** of |
//! | `ZARU_NOTES_LIVE_PAGE` | a page path inside that workspace |
//! | `ZARU_NOTES_LIVE_OTHER_WORKSPACE` | a second workspace id, for the cross-workspace measurement |
//!
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
//! [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials

use zaru_notes::session::{Bearer, HttpEndpoint, Instance, NotesError, Session, WorkspaceId};

/// The name of a variable this file reads, so a missing one names itself.
fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "{name} is not set. This check needs a real Nuclear Notes instance; see the module \
             documentation for the five variables and run it with `--ignored`."
        )
    })
}

/// The bearer, from the file named by `ZARU_NOTES_LIVE_TOKEN_FILE`.
///
/// Trimmed of trailing whitespace exactly as `zaru notes tokens add` trims what
/// arrives on standard input, so a file written with a trailing newline works
/// and a value with one does not silently differ from the same value typed.
fn bearer() -> Bearer {
    let path = required("ZARU_NOTES_LIVE_TOKEN_FILE");
    let value = std::fs::read_to_string(&path)
        .unwrap_or_else(|failure| panic!("{path} could not be read: {failure}"));
    Bearer::new(value.trim_end().to_owned())
}

/// A session against the live instance.
async fn attach() -> Session {
    let endpoint = HttpEndpoint::new().expect("a TLS backend");
    Session::attach(
        &endpoint,
        Instance::new(required("ZARU_NOTES_LIVE_HOST")),
        bearer(),
    )
    .await
    .expect("the live instance completes a session")
}

/// A session attaches over the real transport, and says what it negotiated.
///
/// The first thing this crate has ever done against a server that is not in
/// this process.
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn a_session_attaches_over_streamable_http_and_reports_what_it_negotiated() {
    let session = attach().await;
    let negotiated = session.negotiated();

    assert!(
        !negotiated.protocol_version.is_empty(),
        "the handshake completed without a protocol version"
    );
    assert!(
        negotiated.serves_tools,
        "a server that serves no tools cannot answer any call this crate makes"
    );
    println!(
        "negotiated: protocol {} with {} {}",
        negotiated.protocol_version, negotiated.server_name, negotiated.server_version
    );

    let tools = session.tools().await.expect("tools/list answers");
    assert!(!tools.is_empty(), "the token grants no tools at all");
    println!("tools/list: {} tool(s)", tools.len());
}

/// [ADR-0013] D1's layer 2, read from the live instance.
///
/// # It takes its own workspace, and the reason is a measurement
///
/// Reading a page and reading a grounding are **not** gated the same way, which
/// was found by running this on 2026-09-06 rather than by reading anything.
/// Against a public workspace the token is not a member of, `pages.read`
/// answered 3,859 bytes and `cortex.ground` refused:
///
/// ```text
/// CallRefused { tool: "cortex.ground", code: -32002, detail: "Workspace not reachable." }
/// ```
///
/// So a grounding needs a workspace the credential is a **member** of, and
/// `ZARU_NOTES_LIVE_MEMBER_WORKSPACE` names one. Two variables rather than one,
/// because collapsing them would hide the asymmetry rather than record it.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn the_grounding_is_read_from_the_live_instance_with_the_workspace_named() {
    let session = attach().await;
    let workspace = WorkspaceId::new(required("ZARU_NOTES_LIVE_MEMBER_WORKSPACE"));

    let grounding = session
        .ground(&workspace)
        .await
        .expect("cortex.ground answers for a workspace this credential is a member of");
    assert!(
        !grounding.is_empty(),
        "the grounding came back empty, which is not a grounding"
    );
    println!("cortex.ground: {} byte(s)", grounding.len());
}

/// The other half of the measurement above, asserted rather than described.
///
/// A public workspace this credential can read pages from is a workspace whose
/// grounding it cannot read. Both calls, one after the other, so neither
/// verdict is a claim about the other.
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn a_readable_workspace_is_not_necessarily_a_groundable_one() {
    let session = attach().await;
    let readable = WorkspaceId::new(required("ZARU_NOTES_LIVE_WORKSPACE"));
    let path = required("ZARU_NOTES_LIVE_PAGE");

    session
        .read_page(&path, &readable)
        .await
        .expect("the page is readable");
    let refused = session.ground(&readable).await;
    println!("cortex.ground on a workspace read from but not joined: {refused:?}");
    assert!(
        refused.is_err(),
        "reading a page and reading a grounding turned out to be gated the same way, which this \
         check was written because they are not: {refused:?}"
    );
}

/// A page read by path, with the workspace named on the call.
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn a_page_is_read_by_path_with_its_workspace_named() {
    let session = attach().await;
    let workspace = WorkspaceId::new(required("ZARU_NOTES_LIVE_WORKSPACE"));
    let path = required("ZARU_NOTES_LIVE_PAGE");

    let body = session
        .read_page(&path, &workspace)
        .await
        .expect("the page is readable");
    assert!(!body.is_empty(), "the page came back empty");
    println!("pages.read {path}: {} byte(s)", body.len());
}

/// A search, with the workspace named.
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn a_search_names_its_workspace_and_answers_in_the_shape_this_crate_expects() {
    let session = attach().await;
    let workspace = WorkspaceId::new(required("ZARU_NOTES_LIVE_WORKSPACE"));

    // **The value of this check is the shape, not the hits.** `listing::read`
    // has always documented its expectation as a guess -- "no Nuclear Notes
    // token exists in this workspace, so what the live tools put in a result
    // has not been read off the wire" -- and a search that parses here is that
    // guess measured. A search that matched nothing would still measure it, so
    // the assertion is on the call succeeding rather than on a count.
    let found = session
        .search("workspace", &workspace)
        .await
        .expect("search.global answers in a shape this crate can read");
    println!("search.global: {} row(s)", found.len());
    for row in found.iter().take(3) {
        println!("  {} -- {}", row.path, row.title);
    }
}

/// [ADR-0006]'s Context, re-measured against the live substrate.
///
/// That record's clause 8 asks for exactly this and says why: "D6's whole shape
/// follows from it, and a substrate that had changed would make D6 wrong rather
/// than satisfied". The measurement it rests on is from 2026-08-22.
///
/// **Both halves, because one alone measures nothing.** The same path is read
/// naming its own workspace and naming another; the first answers and the
/// second does not. A check that only asserted the refusal would pass against a
/// server that refused everything.
///
/// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance and a real token; see the module documentation"]
async fn a_path_resolves_in_its_own_workspace_and_not_in_another() {
    let session = attach().await;
    let own = WorkspaceId::new(required("ZARU_NOTES_LIVE_WORKSPACE"));
    let other = WorkspaceId::new(required("ZARU_NOTES_LIVE_OTHER_WORKSPACE"));
    let path = required("ZARU_NOTES_LIVE_PAGE");

    let here = session
        .read_page(&path, &own)
        .await
        .expect("the page is readable in its own workspace");
    assert!(!here.is_empty());

    let there = session.read_page(&path, &other).await;
    println!("pages.read {path} with another workspace named: {there:?}");
    assert!(
        there.is_err(),
        "the same path resolved in a workspace it does not live in, which would make ADR-0006 \
         D6's whole shape unnecessary: {there:?}"
    );
}

/// A token the server rejects, and what reaches the caller.
///
/// The corpus entry [ADR-0016] wants a class for. **Nothing here classifies**;
/// it records what the failure carries, and asserts the one thing that is not
/// negotiable: the value is not in it.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[tokio::test]
#[ignore = "needs a real Nuclear Notes instance; the token it uses is deliberately not one"]
async fn a_token_the_server_rejects_reaches_the_caller_without_the_value_in_it() {
    let endpoint = HttpEndpoint::new().expect("a TLS backend");
    // Shaped like a personal token, and authenticating nothing. A uniqueness
    // device rather than a secret, so a check can look for it in what comes
    // back.
    let planted = format!(
        "nn_mcp_rejected-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos()
    );

    let attached = Session::attach(
        &endpoint,
        Instance::new(required("ZARU_NOTES_LIVE_HOST")),
        Bearer::new(planted.clone()),
    )
    .await;

    let failure = match attached {
        Ok(session) => {
            // Attaching may not be where the server refuses, so the first call
            // is. Either way the refusal is what is asserted on.
            session
                .tools()
                .await
                .expect_err("a token the server never issued cannot list tools")
        }
        Err(failure) => failure,
    };

    println!("rejected token: {failure}");
    println!("rejected token, debug: {failure:?}");
    for rendered in [failure.to_string(), format!("{failure:?}")] {
        assert!(
            !rendered.contains(&planted),
            "a refusal published the bearer value: {rendered}"
        );
    }
    // The discriminating half: the planted value really is findable when it is
    // there, so the two assertions above are not vacuous.
    assert!(
        format!("a refusal carrying {planted}").contains(&planted),
        "the search itself does not work, so its absences prove nothing"
    );
    assert!(
        matches!(
            failure,
            NotesError::Attach { .. } | NotesError::Transport { .. } | NotesError::Call(_)
        ),
        "a rejected token reached the caller as something else: {failure:?}"
    );
}

/// An instance that does not resolve.
///
/// The second corpus entry. `.invalid` is reserved by RFC 2606 and resolves
/// nowhere by construction, so this needs no network to be wrong about — but it
/// is here rather than in the gate's suite because a machine with no resolver
/// at all fails differently, and a check whose verdict depends on that is a
/// check that tracks the environment rather than the code.
#[tokio::test]
#[ignore = "needs a real network stack; see the module documentation"]
async fn an_instance_that_does_not_resolve_reaches_the_caller_as_the_transports_own_words() {
    let endpoint = HttpEndpoint::new().expect("a TLS backend");
    let failure = Session::attach(
        &endpoint,
        Instance::new("nuclear-notes.invalid"),
        Bearer::new("nn_mcp_unused"),
    )
    .await
    .expect_err("a host reserved by RFC 2606 resolves nowhere");

    println!("unresolvable instance: {failure}");
    assert!(
        matches!(
            failure,
            NotesError::Attach { .. } | NotesError::Transport { .. }
        ),
        "an unreachable host was reported as something other than a failure to attach: {failure:?}"
    );
}
