// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A request that outgrows the model's context window, for every provider
//! client: refused before it is sent where the harness can know, and read as
//! the reader's to fix where the provider refused it.
//!
//! # [ADR-0036] is written about providers, not about one of them
//!
//! D1 is "**Each provider** preflights the request it will send", and D2's
//! remedy is "the configured `provider.<kind>.context_tokens` key". Until
//! 2026-09-27 only the `gemini` client did either: the `ollama` and
//! `openai-compatible` clients sent a request of any size, and a capacity
//! refusal from either was classified as a defect of this harness — exit 70,
//! "this is a bug in Zaru, not something you can configure" — where the
//! window was a number the reader sets. Each client had carried its own copy
//! of the recognition, or none, so **the root cause was the missing seam
//! rather than two missing arms**, and this module is the seam.
//!
//! # What is shared and what each kind supplies
//!
//! Shared, here: the two failures ([`Refused`] and [`Exceeded`]) with one
//! sentence each, the prose marker [`names_a_capacity`], the estimate
//! [`Calibration`] and the preflight [`preflight`] made with it. Each client
//! supplies what is its own: how its error body is read, which of its
//! structured fields name a capacity, and which kind's key the remedy names —
//! the classifier takes the kind, never a spelling.
//!
//! # Tokens are estimated, and the provider's count is the truth
//!
//! No tokeniser is in [ADR-0003] D2's table, and a model's tokeniser is the
//! provider's. So a request is estimated: its bytes as serialised, divided by
//! a bytes-per-token ratio. Until 2026-09-28 the ratio was one — bytes were
//! compared with a window stated in tokens — which was sound and used about a
//! quarter of every window: at the built-in `ollama` window of 4,096, one
//! typed line of 620 bytes filled it, because the tool surface alone is 2,531
//! bytes. Now the ratio starts at [`STARTING_BYTES_PER_TOKEN`] and, after
//! each answer, is what the provider counted for the requests it answered.
//! An estimate can be low; a request is therefore refused while it still
//! leaves [`answer_room`], the `ollama` client tells its server not to cut a
//! prompt silently, and a provider's own capacity refusal is read as one.
//!
//! # An unrecognised refusal stays what it was
//!
//! Recognition is deliberately narrow. A refusal whose body does not say, in
//! a structured field or in prose, that a context or token capacity was
//! exceeded keeps the class it had — for a 4xx, the harness's defect — rather
//! than being guessed into this one. D2 says so in as many words: "Other
//! remote request refusals remain defects rather than being guessed into a
//! capacity category". A wrong remedy is worse than none: a reader told to
//! raise a window when the request was malformed raises it and is refused
//! again.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets

use core::fmt;

/// A provider refused a request for exceeding the model's context or token
/// capacity, in words or in a field that say so.
///
/// **The reader's to fix** — [ADR-0036] D2 — and its sentence says what
/// happened without claiming the harness built a malformed request, which is
/// what a `RequestRefused` says and why this is not one read differently.
///
/// Built only by a client's own reading of a 4xx whose detail was already
/// checked free of any key the client holds. A detail withheld for carrying
/// the key names nothing, so that refusal is never built as this one.
///
/// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The HTTP status.
    pub code: u16,
    /// The provider's own classification of the error, where its body
    /// carried one: AIP-193's status name for `gemini`, the `type` of an
    /// OpenAI-shaped body. `None` for a body that carries none, which is
    /// every `ollama` body.
    pub status: Option<String>,
    /// What the provider said, **checked free of the key** before it got
    /// here where the client holds one.
    pub detail: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            code,
            status,
            detail,
        } = self;
        let status = match status.as_deref() {
            Some(status) if !status.is_empty() => format!(", {status}"),
            _ => String::new(),
        };
        write!(
            f,
            "the provider refused this request for exceeding the model's context or token \
             capacity (HTTP {code}{status}): {detail}. The turn's conversation and tool results \
             have outgrown what the provider accepts in one request",
        )
    }
}

impl std::error::Error for Refused {}

/// The complete request for the next exchange would not leave the model room
/// to answer inside its window, so it was not sent.
///
/// Checked before I/O by [`preflight`]. `needed` is an **estimate** in tokens:
/// the request's bytes as serialised, divided by the bytes-per-token ratio
/// [`Calibration`] holds. The request is refused when that estimate passes
/// the window less [`answer_room`], because a request that fills the window
/// leaves the model nothing to answer with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exceeded {
    /// Tokens the next request needs, estimated.
    pub needed: u64,
    /// The request's size in bytes as it would have been sent.
    pub bytes: u64,
    /// The model's context window, in tokens.
    pub window: u64,
    /// Tokens of the window kept for the model's answer.
    pub room: u64,
    /// The largest tool result in the request, where it carried one: the tool
    /// that returned it and its estimated tokens.
    pub largest: Option<(String, u64)>,
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            needed,
            bytes,
            window,
            room,
            largest,
        } = self;
        let allowed = window.saturating_sub(*room);
        write!(
            f,
            "the next request to the model needs an estimated {needed} tokens ({bytes} bytes), \
             and this model's window of {window} tokens holds {allowed} once {room} are kept \
             for its answer; it was not sent"
        )?;
        if let Some((tool, tokens)) = largest {
            write!(
                f,
                ". The largest part of it is what `{tool}` returned, an estimated {tokens} tokens"
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for Exceeded {}

/// What the harness assumes a token costs in bytes before any provider has
/// counted one in this session.
///
/// # Three, because the estimate must err toward a fuller window
///
/// An estimate that is too low sends a request the provider cuts or refuses;
/// one that is too high compacts a little early and says so. So the starting
/// figure sits below every ratio measured: whole requests against a local
/// Ollama came to **4.05** and **3.87** bytes a token on 2026-09-14, and code,
/// JSON and non-English text run lower than English prose. At three, the
/// first request of a session is estimated at a third to a quarter more tokens
/// than it will be counted at. One answer later [`Calibration`] uses what the
/// provider counted instead.
pub const STARTING_BYTES_PER_TOKEN: u64 = 3;

/// The fewest bytes a token is taken to cost when a provider's count is read.
///
/// A count implying fewer than one byte a token is not a count of the text
/// that was sent, and learning from it would make every later estimate
/// larger than anything real.
pub const FEWEST_BYTES_PER_TOKEN: u64 = 1;

/// The most bytes a token is taken to cost when a provider's count is read.
///
/// A count implying more is not a count of the whole request. A local server
/// that reused a cached prompt reports only the tokens it evaluated, which can
/// be a handful for a request of kilobytes, and learning from that would make
/// every later estimate far too small: the direction that overflows a window.
pub const MOST_BYTES_PER_TOKEN: u64 = 8;

/// The share of the window kept for the model's answer: one eighth.
///
/// A request is sent only when its estimate is within the other seven eighths.
/// Compaction starts earlier, at three quarters
/// ([`crate::cli::layers::context_limits`]), so a turn that grows by its own
/// tool results has an eighth of the window to grow into before a request is
/// refused, and the answer has an eighth after that.
pub const ANSWER_SHARE_DIVISOR: u64 = 8;

/// Tokens of `window` kept for the model's answer.
#[must_use]
pub const fn answer_room(window: u64) -> u64 {
    window / ANSWER_SHARE_DIVISOR
}

/// What a provider counted for the requests of one session, and the estimate
/// made from it.
///
/// # Shared, so the context and the client learn together
///
/// The client that sends a request is the one that reads the provider's count
/// for it, and the session's context is what the status line and compaction
/// measure. They hold clones of one value, so what the client learns from an
/// answer is what the next context measurement uses. A session has one client
/// and one model, so this is the ratio for that model in this session; a new
/// session starts again from [`STARTING_BYTES_PER_TOKEN`].
///
/// # The ratio is every accepted answer's, pooled
///
/// Total request bytes over total prompt tokens, across every answer whose
/// count was accepted. Pooling weighs a large request more than a small one,
/// which is right: a large request is the one near the window.
#[derive(Debug, Clone, Default)]
pub struct Calibration {
    learned: std::sync::Arc<std::sync::Mutex<Learned>>,
}

/// The pooled counts, or none yet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Learned {
    bytes: u64,
    tokens: u64,
}

/// The ratio an estimate is made at, and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ratio {
    /// No provider count yet: [`STARTING_BYTES_PER_TOKEN`].
    Starting,
    /// Learned from the provider's counts in this session.
    Learned {
        /// Request bytes of every accepted answer.
        bytes: u64,
        /// Prompt tokens the provider counted for them.
        tokens: u64,
    },
}

impl Calibration {
    /// A calibration that has learned nothing: estimates are made at
    /// [`STARTING_BYTES_PER_TOKEN`].
    #[must_use]
    pub fn starting() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> Learned {
        match self.learned.lock() {
            Ok(learned) => *learned,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// The ratio estimates are made at now.
    #[must_use]
    pub fn ratio(&self) -> Ratio {
        let learned = self.snapshot();
        if learned.tokens == 0 {
            Ratio::Starting
        } else {
            Ratio::Learned {
                bytes: learned.bytes,
                tokens: learned.tokens,
            }
        }
    }

    /// The estimated tokens `bytes` bytes of request cost, rounded up.
    #[must_use]
    pub fn tokens_in(&self, bytes: u64) -> u64 {
        let (per_bytes, per_tokens) = match self.ratio() {
            Ratio::Starting => (STARTING_BYTES_PER_TOKEN, 1),
            Ratio::Learned { bytes, tokens } => (bytes, tokens),
        };
        let scaled = u128::from(bytes) * u128::from(per_tokens);
        let divisor = u128::from(per_bytes.max(1));
        u64::try_from(scaled.div_ceil(divisor)).unwrap_or(u64::MAX)
    }

    /// The bytes `tokens` estimated tokens stand for, rounded down: the
    /// inverse of [`Self::tokens_in`], for a budget stated in tokens that is
    /// spent in bytes.
    #[must_use]
    pub fn bytes_in(&self, tokens: u64) -> u64 {
        let (per_bytes, per_tokens) = match self.ratio() {
            Ratio::Starting => (STARTING_BYTES_PER_TOKEN, 1),
            Ratio::Learned { bytes, tokens } => (bytes, tokens),
        };
        let scaled = u128::from(tokens) * u128::from(per_bytes);
        u64::try_from(scaled / u128::from(per_tokens.max(1))).unwrap_or(u64::MAX)
    }

    /// Learn from one answer: the request was `bytes` bytes and the provider
    /// counted `prompt_tokens` tokens of prompt for it.
    ///
    /// Returns whether the count was taken. A count of zero, or one implying
    /// fewer than [`FEWEST_BYTES_PER_TOKEN`] or more than
    /// [`MOST_BYTES_PER_TOKEN`] bytes a token, is not a count of the whole
    /// request and is left out.
    pub fn learn(&self, bytes: u64, prompt_tokens: u64) -> bool {
        if prompt_tokens == 0
            || bytes < prompt_tokens.saturating_mul(FEWEST_BYTES_PER_TOKEN)
            || bytes > prompt_tokens.saturating_mul(MOST_BYTES_PER_TOKEN)
        {
            return false;
        }
        let mut learned = match self.learned.lock() {
            Ok(learned) => learned,
            Err(poisoned) => poisoned.into_inner(),
        };
        learned.bytes = learned.bytes.saturating_add(bytes);
        learned.tokens = learned.tokens.saturating_add(prompt_tokens);
        true
    }
}

impl zaru_core::context::TokenCounter for Calibration {
    /// The text's bytes, estimated in tokens.
    fn count(&self, text: &str) -> u64 {
        self.tokens_in(text.len() as u64)
    }

    fn count_bytes(&self, bytes: u64) -> u64 {
        self.tokens_in(bytes)
    }

    /// The message as JSON, estimated in tokens.
    ///
    /// The ratio is learned from whole requests as a provider is sent them,
    /// and a provider is sent each message wrapped in its role, its call ids
    /// and its structure. So the estimate is made over the message's JSON,
    /// which carries the same wrapping, rather than over its text alone.
    /// Measured with the scripted server on 2026-09-28: after a turn of fifty
    /// short tool calls, the text alone estimated 1.5k tokens where the
    /// server counted 3,279.
    fn count_message(&self, message: &zaru_core::conversation::Message) -> u64 {
        let bytes = serde_json::to_string(message)
            .map_or_else(|_| message.rendered().len(), |json| json.len());
        self.tokens_in(bytes as u64)
    }
}

/// Whether a refusal's already redacted detail explicitly says the request
/// exceeded a context or token capacity.
///
/// **A prose marker, and deliberately narrow.** A capacity word — "context"
/// or "token" — beside an exceeding word — "exceed", "maximum", "limit",
/// "too large" or "overflow", the last for LM Studio's "Trying to keep the
/// first N tokens when context the overflows". It reads every capacity sentence found in the providers'
/// own sources and published errors on 2026-09-27 (listed where each client
/// calls it) and reads none of the malformed-request sentences those clients
/// have recorded. A refusal it does not read keeps its old class.
#[must_use]
pub fn names_a_capacity(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    (detail.contains("context") || detail.contains("token"))
        && (detail.contains("exceed")
            || detail.contains("maximum")
            || detail.contains("limit")
            || detail.contains("too large")
            || detail.contains("overflow"))
}

/// A native request's size in bytes, as serialised.
///
/// The request as serialised, which is the request as sent: `reqwest`'s
/// `.json` serialises the same value with the same serializer.
#[must_use]
pub fn request_bytes(request: &impl serde::Serialize) -> u64 {
    serde_json::to_string(request).map_or(0, |rendered| rendered.len() as u64)
}

/// The largest tool result in a turn, by its bytes: the tool's name and the
/// result's estimated tokens.
fn largest_result(
    turn: &[zaru_core::conversation::Message],
    calibration: &Calibration,
) -> Option<(String, u64)> {
    turn.iter()
        .filter_map(|message| match message {
            zaru_core::conversation::Message::Tool { name, content, .. } => {
                Some((name.clone(), content.len() as u64))
            }
            _ => None,
        })
        .max_by_key(|(_, bytes)| *bytes)
        .map(|(name, bytes)| (name, calibration.tokens_in(bytes)))
}

/// [ADR-0036] D1: refuse, before any network I/O, a request that cannot fit.
///
/// Every client calls this on the body it is about to send and nowhere else,
/// so the measurement is of the provider-native request — assembled prompt,
/// the model's own prior turns, the turn's tool results and the declared tool
/// surface — and never of a request nobody sends.
///
/// The request is estimated in tokens through `calibration` and compared with
/// the window less [`answer_room`]. On success it returns the request's bytes,
/// which the client hands back to [`Calibration::learn`] with the provider's
/// count once the answer arrives.
///
/// # Errors
///
/// [`Exceeded`], carrying the estimate, the bytes, the window, the room kept
/// for the answer and the largest tool result in `turn`.
///
/// [ADR-0036]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0036-in-turn-provider-request-budgets
pub fn preflight(
    request: &impl serde::Serialize,
    window: u64,
    calibration: &Calibration,
    turn: &[zaru_core::conversation::Message],
) -> Result<u64, Exceeded> {
    let bytes = request_bytes(request);
    let needed = calibration.tokens_in(bytes);
    let room = answer_room(window);
    if needed > window.saturating_sub(room) {
        return Err(Exceeded {
            needed,
            bytes,
            window,
            room,
            largest: largest_result(turn, calibration),
        });
    }
    Ok(bytes)
}

/// What a check of a capacity refusal reads, shared by the three clients'
/// checks so that the three kinds are held to one list of clauses.
#[cfg(test)]
pub mod fixtures {
    use crate::providers::ProviderKind;

    /// A calibration that has learned one token a byte, for a check whose
    /// numbers are written in bytes: every count it makes is the text's
    /// length, exactly.
    pub fn one_token_a_byte() -> super::Calibration {
        let calibration = super::Calibration::starting();
        assert!(
            calibration.learn(1_000, 1_000),
            "one token a byte is a count"
        );
        calibration
    }

    /// Every clause of a capacity refusal as a person reads it, each miss
    /// reported, for one rendering.
    pub fn misses(
        kind: ProviderKind,
        presented: &crate::failure::Presentation,
        theirs: &str,
    ) -> Vec<String> {
        use crate::failure::Class;
        let said = presented.to_string();
        let mut misses = Vec::new();
        if presented.class != Class::UserCorrectable {
            misses.push(format!(
                "a capacity refusal is the reader's to fix, and this one is {:?} at exit {}",
                presented.class,
                presented.class.exit_code()
            ));
        }
        if said.contains("malformed")
            || said.contains("this harness built")
            || said.contains("bug in Zaru")
            || said.contains("defect in Zaru")
        {
            misses.push("it claims the harness malfunctioned".to_owned());
        }
        if !(presented.headline.contains("capacity") && presented.headline.contains(theirs)) {
            misses.push(
                "the statement does not name the capacity beside the provider's words".to_owned(),
            );
        }
        if !said.contains(kind.context_tokens_key().as_str()) {
            misses.push(format!(
                "the remedy does not name `{}`",
                kind.context_tokens_key().as_str()
            ));
        }
        if !misses.is_empty() {
            misses.push(format!("rendered: {said}"));
        }
        misses
    }
}

#[cfg(test)]
mod tests {
    use super::{Calibration, Ratio, STARTING_BYTES_PER_TOKEN, answer_room, preflight};
    use zaru_core::context::TokenCounter as _;

    /// Before any answer the estimate errs toward a fuller window: three bytes
    /// a token, which is under both ratios measured on a real server (4.05
    /// and 3.87), so the first request of a session is estimated at more
    /// tokens than it will be counted at.
    ///
    /// Watched red with the starting ratio set to one, the byte count this
    /// replaced: "the starting ratio must sit under every measured one and
    /// above one byte a token: 2715 bytes counted as 671 tokens were estimated
    /// at 2715 with 1 bytes a token".
    #[test]
    fn the_starting_ratio_errs_toward_a_fuller_window() {
        let measured = [(2_715_u64, 671_u64), (3_035, 785)];
        let starting = Calibration::starting();
        assert_eq!(starting.ratio(), Ratio::Starting);
        for (bytes, tokens) in measured {
            let estimated = starting.tokens_in(bytes);
            assert!(
                estimated > tokens && estimated < bytes,
                "the starting ratio must sit under every measured one and above one byte a \
                 token: {bytes} bytes counted as {tokens} tokens were estimated at {estimated} \
                 with {STARTING_BYTES_PER_TOKEN} bytes a token"
            );
        }
    }

    /// One answer teaches the ratio, and a count that cannot be of the whole
    /// request teaches nothing.
    ///
    /// A local server reusing a cached prompt can report the handful of
    /// tokens it evaluated for a request of kilobytes; learning that would
    /// make every later estimate far too small, which is the direction that
    /// overflows a window.
    ///
    /// Watched red with the bounds removed from `learn`: "a count of 40
    /// tokens for 4,000 bytes is a cached prompt, not the whole request".
    #[test]
    fn a_count_teaches_the_ratio_and_a_cached_count_does_not() {
        let calibration = Calibration::starting();
        assert!(
            !calibration.learn(4_000, 40),
            "a count of 40 tokens for 4,000 bytes is a cached prompt, not the whole request"
        );
        assert!(!calibration.learn(4_000, 0), "a count of zero is no count");
        assert!(
            !calibration.learn(100, 400),
            "more tokens than bytes is not a count of this text"
        );
        assert_eq!(calibration.ratio(), Ratio::Starting);

        assert!(calibration.learn(4_000, 1_000));
        assert_eq!(calibration.tokens_in(4_000), 1_000);
        assert!(calibration.learn(2_000, 1_000));
        assert_eq!(
            calibration.ratio(),
            Ratio::Learned {
                bytes: 6_000,
                tokens: 2_000
            },
            "the ratio is every accepted answer's, pooled"
        );
        assert_eq!(calibration.tokens_in(3_000), 1_000);
        assert_eq!(calibration.bytes_in(1_000), 3_000);
        assert_eq!(
            calibration.count("abc"),
            1,
            "text is counted by its bytes at the ratio"
        );
        assert_eq!(calibration.count_bytes(3_001), 1_001, "rounded up");
    }

    /// A clone is the same calibration: what the client learns, the context
    /// measures with.
    #[test]
    fn a_clone_learns_with_the_original() {
        let client = Calibration::starting();
        let context = client.clone();
        assert!(client.learn(4_000, 1_000));
        assert_eq!(
            context.tokens_in(4_000),
            1_000,
            "the context's estimate moved with the client's answer"
        );
    }

    /// A request is sent only when its estimate is within seven eighths of the
    /// window; the last eighth is kept for the answer.
    #[test]
    fn a_request_is_refused_when_it_leaves_no_room_to_answer() {
        let calibration = Calibration::starting();
        assert!(calibration.learn(1_000, 1_000));
        let window = 800;
        assert_eq!(answer_room(window), 100);
        let fits = "x".repeat(700 - 2);
        preflight(&fits, window, &calibration, &[])
            .expect("700 tokens is seven eighths of 800 and fits");
        let over = "x".repeat(701 - 2);
        let refused = preflight(&over, window, &calibration, &[])
            .expect_err("701 tokens leaves less than an eighth for the answer");
        assert_eq!(
            (refused.needed, refused.window, refused.room),
            (701, 800, 100)
        );
    }
}
