// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a transport failure said, for every provider client that gets one.
//!
//! It turns an error chain into a sentence and knows nothing about any
//! provider: it does not know which kind raised it, what the endpoint was, or
//! which failure variant will carry the result. Each client's own arm decides
//! that, and keeping the two apart is what lets the walk be checked over
//! chains that never came from a socket.
//!
//! # It lives here because three clients fail the same way
//!
//! **It was `providers::openai_compatible::failure` until 2026-09-14**, where
//! it was written when that client was the only one measured against a closed
//! port. The `gemini` and `ollama` clients call `reqwest` at the same three
//! points — the send, the body read on a non-success status, and each chunk of
//! the stream — and each raised the same useless sentence. Moving it is not
//! generalisation in advance: the two further callers were measured from the
//! release binary before the module moved, and both printed `error sending
//! request for url (…)` and nothing else for a refused connection *and* for a
//! host that does not resolve, the two differing only in the URL.
//!
//! [`super::openai_compatible::failure`] re-exports both items so that
//! client's call sites and its checks are unchanged, and the move carries no
//! behaviour with it. **The sentences below are that arc's, unedited**, so
//! "this endpoint" in the first one is the OpenAI-compatible endpoint it was
//! measured against — `llama-server` on 18080 — rather than anything this
//! module knows about.
//!
//! # The bound is the whole of the safety argument
//!
//! A `source` chain is arbitrary-length data from a dependency, so the walk is
//! bounded at [`CHAIN_DEPTH`] rather than run to exhaustion. Nothing else here
//! is a guard: the text joined is composed by `reqwest` from the request URL
//! and the operating system, never from a server's response, which is why no
//! redaction runs over it. A client that puts its key in a URL would publish
//! it in the top-level message before this walk ever ran; none of the three
//! does, and each says so at its own call site.

/// What a transport error said, including the part that says what went wrong.
///
/// # `reqwest::Error`'s own `Display` is not the sentence a reader needs
///
/// Measured 2026-09-14 against this endpoint. `to_string()` on a failed send
/// gives exactly `error sending request for url
/// (http://127.0.0.1:18080/v1/chat/completions)` — a grammatical sentence
/// carrying no information at all. The cause is **three levels down the
/// `source` chain** and it is the only part a reader can act on:
///
/// ```text
/// top:      error sending request for url (http://127.0.0.1:11999/…)
///   source 1: client error (Connect)
///   source 2: tcp connect error
///   source 3: Connection refused (os error 111)
/// ```
///
/// and for a name that does not resolve, `dns error` then `failed to lookup
/// address information: Name or service not known`. "Connection refused" and
/// "the name does not resolve" are different problems with different remedies,
/// and the top-level message distinguishes them not at all.
///
/// [ADR-0016] D2 says an error whose reader cannot act "is a stack trace with
/// better grammar"; a message with better grammar and no stack trace is the
/// same failure with less to go on. So the chain is walked and joined.
///
/// **Bounded, because a chain is arbitrary-length data from a dependency.**
/// At most [`CHAIN_DEPTH`] links are read, so a cyclic or pathological chain
/// cannot make a refusal unbounded.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[must_use]
pub fn transport_detail(error: &dyn std::error::Error) -> String {
    let mut said = error.to_string();
    let mut source = error.source();
    let mut depth = 0;
    while let Some(link) = source {
        if depth == CHAIN_DEPTH {
            break;
        }
        let text = link.to_string();
        // A link that only repeats its parent adds nothing and costs a reader
        // a clause. `reqwest` does this at least once, where the wrapper's
        // Display is its source's.
        if !said.contains(&text) {
            said.push_str(": ");
            said.push_str(&text);
        }
        source = link.source();
        depth += 1;
    }
    said
}

/// The same sentence, and the bound it was refused at when it timed out.
///
/// **A timed-out exchange said nothing about the ceiling it hit.** Measured
/// 2026-09-15 from the release binary: a reasoning turn against
/// `gemini-3.6-flash` painted nothing for sixty seconds and then printed *"the
/// provider could not be reached: error sending request for url (...):
/// operation timed out"*, with the remedy beside it saying only that waiting
/// would not help. Nothing on either line told the reader a bound existed, so
/// a turn lost to a ceiling and a turn lost to a dead socket read identically
/// -- and the first has a cause the second does not.
///
/// So when, and only when, `reqwest` reports the failure as a timeout, the
/// figure joins the sentence it already prints: *"... operation timed out
/// after 600s"*. **The figure is composed from the constant and never typed**,
/// so a ceiling that changes changes the sentence, and a refusal that is not a
/// timeout is untouched -- a refused connection still says exactly what it
/// said.
///
/// **The ceiling arrives as an argument rather than being read here**, which
/// is what lets a check drive a real timeout against a listener that never
/// answers in eighty milliseconds instead of ten minutes.
///
/// [`transport_detail`] keeps its `&dyn Error` signature and is unchanged: the
/// chain-walk checks run over synthetic chains that never came from a socket,
/// and `reqwest::Error` cannot be constructed to make one.
#[must_use]
pub fn transport_detail_within(error: &reqwest::Error, ceiling: core::time::Duration) -> String {
    let said = transport_detail(error);
    if error.is_timeout() {
        format!("{said} after {ceiling:?}")
    } else {
        said
    }
}

/// How many `source` links a transport failure's sentence may carry.
///
/// Four is one more than the deepest chain measured (`Connect` → `tcp connect
/// error` → `Connection refused`), so the measured cases are whole and an
/// unmeasured one cannot run away.
pub const CHAIN_DEPTH: usize = 4;

/// How long one exchange may take before a client gives up, for every kind.
///
/// **Ten minutes, and one figure rather than three.** Until 2026-09-15 each
/// client declared its own: `ollama` and `openai-compatible` at 600 seconds
/// and `gemini` at 60, so which bound a turn ran under depended on which kind
/// served the alias, and a fourth kind would have picked a fourth number. It
/// is declared here, at the seam the three already share for what a transport
/// failure says, so a client chooses nothing.
///
/// **It bounds the whole streamed exchange, first byte to last.** That is
/// `reqwest`'s own reading of the value `crate::web::client::build` passes
/// to `ClientBuilder::timeout`, whose documentation in 0.12.28 is "a total
/// request timeout … applied from when the request starts connecting until
/// the response body has finished. Also considered a total deadline" — and
/// not `read_timeout`, which resets after each successful read. So the budget
/// is spent by generation rather than by latency, and **an answer that
/// generates for longer than ten minutes is refused**.
///
/// **The figure is measured rather than preferred, on both ends of the
/// range.** A cold load of `llama3.2:3b` through `llama-server` on the
/// development machine took over four minutes before a token on 2026-09-14,
/// so a 60-second ceiling reported a working server as unreachable. At the
/// other end, two timestamped probes of `streamGenerateContent` against
/// `gemini-3.6-flash` on 2026-09-15 found the first SSE byte at **46.0 s** and
/// at **92.7 s**, the second confirmed by Google's own `server-timing:
/// gfet4t7; dur=92545` — so 60 seconds killed a reasoning turn that was well
/// inside the model's ordinary range, which is the defect this figure closes.
/// A hosted gateway answering in seconds never approaches it, so the ceiling
/// costs that reader nothing.
///
/// There is deliberately **no retry and no backoff in a client**. A retry
/// policy decides whether a request that may have had an effect is repeated,
/// and a client that retried on its own would be answering that silently.
/// **Since 2026-09-30 the composition answers it**, in
/// `providers::resilience`, and every retry of one exchange stays inside this
/// ceiling: each attempt is handed what is left of it, so a retry never buys a
/// second ten minutes.
///
/// **It is a constant until a record says otherwise.** Whether
/// `provider.<kind>` should carry a ceiling key is [ADR-0012]'s author's and
/// is not settled here.
///
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
pub const EXCHANGE_TIMEOUT: core::time::Duration = core::time::Duration::from_secs(600);
