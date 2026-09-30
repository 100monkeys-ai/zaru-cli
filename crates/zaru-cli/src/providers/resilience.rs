// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D4's retry, obeyed: a provider that stalls or fails for a
//! reason a second attempt can change is asked again, visibly, a bounded
//! number of times.
//!
//! # What is retried, and what is not
//!
//! **Transient**, and retried: a connection that failed or broke, a response
//! that began and then went silent for [`Policy::stall`], and HTTP 408, 429
//! and every 5xx. Each is the provider's condition at a moment, and the same
//! request a few seconds later can meet a different one.
//!
//! **Not retried**: every other 4xx, and any refusal the provider states in
//! words. Those are answers about the request, and asking again gets the same
//! answer; the person is shown the provider's words, as ADR-0016's amendments
//! already require.
//!
//! # The numbers, and where they come from
//!
//! Three retries at most, waiting 1 s, 2 s and 4 s, each plus up to 1 s of
//! jitter, honouring a `Retry-After` the provider sends when it asks for
//! longer, and never more than 30 s in one wait. A response that has begun and
//! then sends nothing for 60 s is abandoned as stalled. **Each is a
//! configuration key with that default** ([`STALL_KEY`] and the four beside
//! it), declared in layer 1 so `zaru config explain` prints it and a project
//! may lower it and not raise it. The figures are the coordinator's delegated
//! ruling of 2026-09-30 under directive 58, open to Jeshua's veto, and are
//! recorded on ADR-0012's amendments volume 3.
//!
//! # The stall clock starts when the response does
//!
//! The clock is per byte received, so a slow stream that keeps flowing is
//! never a stall. **It runs between the bytes of a response that has begun,
//! and not before the first.** Two measurements recorded on ADR-0012 put a
//! working provider's first byte well past sixty seconds: Gemini's first SSE
//! byte at 46.0 s and at 92.7 s on reasoning turns, with no frame of any kind
//! before it, and a cold load through `llama-server` that took over four
//! minutes before a token. A no-byte clock over that wait would abandon a
//! working turn and then retry it into the same wall, which is the defect the
//! exchange ceiling closed on 2026-09-15. The wait for a response to begin is
//! bounded by that ceiling, [`super::transport::EXCHANGE_TIMEOUT`].
//!
//! # A retry never extends the exchange's ceiling
//!
//! One exchange, with every retry it makes and every wait between them, stays
//! inside [`Policy::budget`], which is the exchange ceiling. Each attempt is
//! handed what is left of it, and a wait that would end past it is not taken:
//! the last failure is reported instead. So a retry buys a second chance
//! inside the ten minutes and never a second ten minutes.
//!
//! # A retry is not an exchange
//!
//! The loop is below the model port: [`exchange`] runs inside one
//! `Model::respond`, so the tool-call loop counts one exchange however many
//! attempts it took, and ADR-0034's limit counts what the model was asked, not
//! how often the network had to be tried.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::config::{Field, Key, Resolution, Schema, Table, Value};
use crate::failure::{Backoff, RETRY_LABEL, RetryCeiling};
use core::fmt;
use core::future::Future;
use core::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use zaru_core::iteration::PortFailure;
use zaru_core::tool_call::{Capabilities, Model, ModelRequest, ModelResponse};

#[cfg(test)]
mod tests;

/// How long a response that has begun may send nothing before it is abandoned.
pub const STALL_KEY: &str = "provider.stall_seconds";
/// How many times a transient failure is retried.
pub const RETRIES_KEY: &str = "provider.retries";
/// The first wait before a retry; each later one doubles it.
pub const BACKOFF_KEY: &str = "provider.retry_backoff_seconds";
/// The most random time added to each wait.
pub const JITTER_KEY: &str = "provider.retry_jitter_seconds";
/// The longest any one wait may be, `Retry-After` included.
pub const WAIT_CEILING_KEY: &str = "provider.retry_wait_ceiling_seconds";

/// The five keys, their defaults, and the least value each accepts.
///
/// One table, so the declaration, layer 1's defaults and the reading cannot
/// come to disagree about a key.
pub const KEYS: [(&str, u32, u32); 5] = [
    (STALL_KEY, 60, 1),
    (RETRIES_KEY, 3, 1),
    (BACKOFF_KEY, 1, 1),
    (JITTER_KEY, 1, 0),
    (WAIT_CEILING_KEY, 30, 1),
];

fn key(name: &str) -> Key {
    Key::new(name).expect("each resilience key is a well-formed key")
}

/// Declare the five keys into a caller's schema.
///
/// **`LowerOnly`, as every count-shaped key here is.** A project that lowers
/// one makes the harness give up sooner, which is its own business; one that
/// raised them could make the reader's machine wait on a provider for longer
/// than the reader said it should.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    KEYS.iter().fold(schema, |schema, (name, _, _)| {
        schema.with(key(name), Field::ceiling())
    })
}

/// Write the five defaults into layer 1's document.
///
/// Here rather than where they are read, for the reason the context window's
/// default is: a project may lower what the layers below granted, and a
/// default nobody granted is one no project value is measured against.
pub fn insert_defaults(document: &mut Table) {
    for (name, default, _) in KEYS {
        document.insert_path(&key(name), Value::Integer(i64::from(default)));
    }
}

/// Why a resilience key could not be taken as what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyRefused {
    /// A whole number outside what the key accepts.
    OutOfRange {
        /// The key.
        key: Key,
        /// What it held.
        found: i64,
        /// The least it accepts.
        least: u32,
    },
    /// Something that is not a whole number.
    WrongShape {
        /// The key.
        key: Key,
        /// The shape it held, never its value.
        found: &'static str,
    },
}

impl PolicyRefused {
    /// The key a remedy names.
    #[must_use]
    pub const fn key(&self) -> &Key {
        match self {
            Self::OutOfRange { key, .. } | Self::WrongShape { key, .. } => key,
        }
    }
}

impl fmt::Display for PolicyRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { key, found, least } => write!(
                f,
                "`{key}` is {found}, and it takes a whole number from {least} to {}",
                u32::MAX
            ),
            Self::WrongShape { key, found } => write!(
                f,
                "`{key}` holds {found}, and it takes a whole number of at least 1"
            ),
        }
    }
}

impl std::error::Error for PolicyRefused {}

/// What one exchange may do when its provider fails transiently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    stall: Duration,
    retries: RetryCeiling,
    backoff: Backoff,
    jitter: Duration,
    wait_ceiling: Duration,
    budget: Duration,
}

impl Policy {
    /// A policy from its parts.
    #[must_use]
    pub const fn new(
        stall: Duration,
        retries: RetryCeiling,
        backoff: Backoff,
        jitter: Duration,
        wait_ceiling: Duration,
        budget: Duration,
    ) -> Self {
        Self {
            stall,
            retries,
            backoff,
            jitter,
            wait_ceiling,
            budget,
        }
    }

    /// The ruled numbers, as layer 1 declares them.
    #[must_use]
    pub fn built_in() -> Self {
        Self::from_counts(KEYS.map(|(_, default, _)| default))
    }

    /// Read the five keys out of a resolved configuration.
    ///
    /// A key no layer sets takes its default, so a resolution built without
    /// layer 1 still answers the ruled numbers.
    ///
    /// # Errors
    ///
    /// [`PolicyRefused`], naming the key.
    pub fn from_configuration(resolution: &Resolution) -> Result<Self, PolicyRefused> {
        let mut read = [0u32; KEYS.len()];
        for (slot, (name, default, least)) in read.iter_mut().zip(KEYS) {
            let key = key(name);
            *slot = match resolution.get(&key) {
                None => default,
                Some(value) => {
                    let Some(found) = value.as_integer() else {
                        return Err(PolicyRefused::WrongShape {
                            key,
                            found: value.shape(),
                        });
                    };
                    u32::try_from(found)
                        .ok()
                        .filter(|count| *count >= least)
                        .ok_or(PolicyRefused::OutOfRange { key, found, least })?
                }
            };
        }
        Ok(Self::from_counts(read))
    }

    /// The five counts in [`KEYS`]' order, each already at least its least.
    fn from_counts(counts: [u32; KEYS.len()]) -> Self {
        let [stall, retries, backoff, jitter, wait_ceiling] = counts;
        let seconds = |count: u32| Duration::from_secs(u64::from(count));
        Self {
            stall: seconds(stall),
            retries: RetryCeiling::new(retries)
                .expect("a retry count below 1 is refused before this"),
            backoff: Backoff::new(seconds(backoff))
                .expect("a backoff below 1 second is refused before this"),
            jitter: seconds(jitter),
            wait_ceiling: seconds(wait_ceiling),
            budget: super::transport::EXCHANGE_TIMEOUT,
        }
    }

    /// How long a response that has begun may send nothing.
    #[must_use]
    pub const fn stall(&self) -> Duration {
        self.stall
    }

    /// How many retries one exchange may make.
    #[must_use]
    pub const fn retries(&self) -> u32 {
        self.retries.get()
    }

    /// The whole exchange's bound, every attempt and every wait inside it.
    #[must_use]
    pub const fn budget(&self) -> Duration {
        self.budget
    }

    /// The most jitter one wait may carry.
    #[must_use]
    pub const fn jitter(&self) -> Duration {
        self.jitter
    }

    /// The wait before retry number `retry`, counted from 1.
    ///
    /// The backoff doubled for each retry before this one, plus `jitter` (held
    /// to [`Self::jitter`]); a `Retry-After` longer than that replaces it; and
    /// nothing longer than the wait ceiling.
    #[must_use]
    pub fn wait_before(
        &self,
        retry: u32,
        retry_after: Option<Duration>,
        jitter: Duration,
    ) -> Duration {
        let doubling = 2u32.saturating_pow(retry.saturating_sub(1));
        let backoff = self
            .backoff
            .get()
            .saturating_mul(doubling)
            .saturating_add(jitter.min(self.jitter));
        let chosen = match retry_after {
            Some(asked) if asked > backoff => asked,
            _ => backoff,
        };
        chosen.min(self.wait_ceiling)
    }
}

/// Whether a status is the provider's condition at a moment rather than an
/// answer about the request: 408, 429 and every 5xx.
#[must_use]
pub const fn is_transient_status(code: u16) -> bool {
    matches!(code, 408 | 429 | 500..=599)
}

/// Why a failure is worth one more attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The provider answered with a transient status.
    Status(u16),
    /// A response began and then sent nothing for this long.
    Stalled(Duration),
    /// The connection failed, or broke part-way through the answer.
    Connection,
}

/// A failure a retry can change, and how long the provider asked to be left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transient {
    /// Why.
    pub cause: Cause,
    /// The provider's `Retry-After`, where it sent one.
    pub retry_after: Option<Duration>,
}

/// A failure that can say whether a retry could change it.
pub trait Transience {
    /// `Some` when a second attempt could meet a different outcome.
    fn transient(&self) -> Option<Transient>;
}

/// One retry, as the person is shown it.
///
/// Labelled `retry`, counted against its bound, and saying why and how long
/// the wait is: ADR-0016 D4's "visible, counted, and bounded", in one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retrying {
    /// Why the attempt before it failed.
    pub cause: Cause,
    /// Which retry this is, from 1.
    pub retry: u32,
    /// How many the policy allows.
    pub of: u32,
    /// How long before it is made.
    pub wait: Duration,
}

impl fmt::Display for Retrying {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{RETRY_LABEL} {} of {} in {}",
            self.retry,
            self.of,
            spoken(self.wait)
        )?;
        match self.cause {
            Cause::Status(code) => write!(f, " after HTTP {code}"),
            Cause::Stalled(silent) => {
                write!(
                    f,
                    ": the provider stalled, sending nothing for {}",
                    spoken(silent)
                )
            }
            Cause::Connection => f.write_str(": the connection to the provider failed"),
        }
    }
}

/// A duration as a person reads it: whole seconds, or tenths where it has a
/// fraction.
#[must_use]
pub fn spoken(wait: Duration) -> String {
    if wait.subsec_nanos() == 0 {
        format!("{} s", wait.as_secs())
    } else {
        format!("{:.1} s", wait.as_secs_f64())
    }
}

/// What one attempt is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    /// How long a begun response may send nothing.
    pub stall: Duration,
    /// What is left of the exchange's ceiling.
    pub budget: Duration,
}

impl Attempt {
    /// One attempt under the built-in figures and the whole exchange ceiling,
    /// for a client driven on its own rather than through [`Resilient`].
    #[must_use]
    pub fn built_in() -> Self {
        let policy = Policy::built_in();
        Self {
            stall: policy.stall,
            budget: policy.budget,
        }
    }
}

/// Time, as the retry loop needs it: a reading and a wait.
pub trait Clock: Sync {
    /// Now.
    fn now(&self) -> Instant;
    /// Wait this long.
    fn pause(&self, wait: Duration) -> impl Future<Output = ()> + Send;
}

/// The runtime's own clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tokio;

impl Clock for Tokio {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn pause(&self, wait: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(wait)
    }
}

impl<T: Clock> Clock for &T {
    fn now(&self) -> Instant {
        (**self).now()
    }

    fn pause(&self, wait: Duration) -> impl Future<Output = ()> + Send {
        (**self).pause(wait)
    }
}

/// Where a wait's jitter comes from.
pub trait Jitter: Sync {
    /// A duration from zero to `most`, inclusive.
    fn up_to(&self, most: Duration) -> Duration;
}

/// Jitter from the standard library's per-process random hash keys.
///
/// **No dependency is added for it.** `RandomState` is seeded from the
/// operating system once per thread and moves on at each construction, which
/// is spread enough to keep two clients that failed together from retrying
/// together, and that is all jitter is for.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scattered;

impl Jitter for Scattered {
    fn up_to(&self, most: Duration) -> Duration {
        use std::hash::{BuildHasher, Hasher};
        let span = u64::try_from(most.as_nanos()).unwrap_or(u64::MAX);
        if span == 0 {
            return Duration::ZERO;
        }
        let drawn = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        Duration::from_nanos(drawn % span.saturating_add(1))
    }
}

/// Run `attempt` until it answers, fails for good, or the policy is spent.
///
/// `told` hears of each retry before its wait begins, so a person watching
/// sees why nothing is happening. See the module documentation for what is
/// retried and for the ceiling no retry extends.
///
/// # Errors
///
/// The last attempt's failure: one [`Transience`] says a retry cannot change,
/// one met with no retry left, or one whose next wait would end past the
/// exchange's ceiling.
pub async fn exchange<T, E, A, F>(
    policy: &Policy,
    clock: &impl Clock,
    jitter: &impl Jitter,
    mut attempt: A,
    mut told: impl FnMut(&Retrying),
) -> Result<T, E>
where
    E: Transience,
    A: FnMut(Attempt) -> F,
    F: Future<Output = Result<T, E>>,
{
    let started = clock.now();
    let mut made = 0u32;
    loop {
        let spent = clock.now().saturating_duration_since(started);
        let failure = match attempt(Attempt {
            stall: policy.stall,
            budget: policy.budget.saturating_sub(spent),
        })
        .await
        {
            Ok(answer) => return Ok(answer),
            Err(failure) => failure,
        };
        let Some(transient) = failure.transient() else {
            return Err(failure);
        };
        if made >= policy.retries() {
            return Err(failure);
        }
        let retry = made + 1;
        let wait = policy.wait_before(retry, transient.retry_after, jitter.up_to(policy.jitter));
        let spent = clock.now().saturating_duration_since(started);
        if spent.saturating_add(wait) >= policy.budget {
            return Err(failure);
        }
        told(&Retrying {
            cause: transient.cause,
            retry,
            of: policy.retries(),
            wait,
        });
        clock.pause(wait).await;
        made = retry;
    }
}

/// Something that makes one attempt at an exchange.
///
/// [`super::ProviderClient`] is the product's; a check stages its own, so the
/// retry loop a turn runs is the one a check drives.
pub trait Exchanging: Sync {
    /// What one attempt fails with.
    type Failure: Transience + fmt::Display + Send;

    /// What the model behind it can do.
    fn capabilities(&self) -> Capabilities;

    /// One attempt, inside what `attempt` allows.
    fn attempt(
        &self,
        request: &ModelRequest<'_>,
        attempt: Attempt,
    ) -> impl Future<Output = Result<ModelResponse, Self::Failure>> + Send;
}

/// An exchange made under a [`Policy`], telling someone of each retry.
///
/// **This is the one place a turn's exchanges are retried**, and it is a model
/// port of its own: the tool-call loop is handed it, calls `respond` once per
/// exchange, and never sees an attempt.
pub struct Resilient<'a, X, C = Tokio, J = Scattered> {
    inner: &'a X,
    policy: Policy,
    clock: C,
    jitter: J,
    told: &'a (dyn Fn(&Retrying) + Sync),
}

impl<X> fmt::Debug for Resilient<'_, X> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resilient")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl<'a, X: Exchanging> Resilient<'a, X> {
    /// Over `inner`, on the runtime's clock and the standard jitter.
    #[must_use]
    pub const fn over(inner: &'a X, policy: Policy, told: &'a (dyn Fn(&Retrying) + Sync)) -> Self {
        Self {
            inner,
            policy,
            clock: Tokio,
            jitter: Scattered,
            told,
        }
    }
}

impl<'a, X: Exchanging, C: Clock, J: Jitter> Resilient<'a, X, C, J> {
    /// Over `inner`, with a clock and a jitter of the caller's.
    #[must_use]
    pub const fn timed(
        inner: &'a X,
        policy: Policy,
        clock: C,
        jitter: J,
        told: &'a (dyn Fn(&Retrying) + Sync),
    ) -> Self {
        Self {
            inner,
            policy,
            clock,
            jitter,
            told,
        }
    }

    /// What it wraps.
    #[must_use]
    pub const fn inner(&self) -> &'a X {
        self.inner
    }

    /// One exchange, retried under the policy; the typed failure if it fails.
    ///
    /// # Errors
    ///
    /// The last attempt's failure, as [`exchange`] says.
    pub async fn exchange(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, X::Failure> {
        exchange(
            &self.policy,
            &self.clock,
            &self.jitter,
            |attempt| self.inner.attempt(request, attempt),
            |retrying| (self.told)(retrying),
        )
        .await
    }
}

impl<X: Exchanging, C: Clock, J: Jitter> Model for Resilient<'_, X, C, J> {
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn respond(&self, request: &ModelRequest<'_>) -> Result<ModelResponse, PortFailure> {
        self.exchange(request)
            .await
            .map_err(|failure| PortFailure::new(failure.to_string()))
    }
}

/// A response went silent for the stall bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stalled {
    /// How long nothing arrived for.
    pub silent_for: Duration,
}

/// Read one piece of a response that has begun, or say it stalled.
///
/// **Every client reads every chunk of its stream through this**, so the stall
/// clock restarts at each byte that arrives and a slow stream that keeps
/// flowing is never abandoned.
///
/// # Errors
///
/// [`Stalled`] when nothing arrived within `stall`.
pub async fn within_stall<F: Future>(stall: Duration, read: F) -> Result<F::Output, Stalled> {
    tokio::time::timeout(stall, read)
        .await
        .map_err(|_| Stalled { silent_for: stall })
}

/// The provider's `Retry-After`, from a response's headers.
#[must_use]
pub fn retry_after_of(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?;
    retry_after(value, SystemTime::now())
}

/// A `Retry-After` value as a wait from `now`.
///
/// RFC 9110 §10.2.3 allows two forms: a count of seconds, and an HTTP-date,
/// which a sender generates as an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37
/// GMT`). Both are read. A date already past is a wait of zero, and anything
/// else is not a `Retry-After` and is ignored.
#[must_use]
pub fn retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = imf_fixdate(value)?;
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

/// An IMF-fixdate, `Sun, 06 Nov 1994 08:49:37 GMT`, as a time.
fn imf_fixdate(value: &str) -> Option<SystemTime> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?.strip_suffix(',')?;
    let day: u32 = parts.next()?.parse().ok()?;
    let month = parts.next()?;
    let month = u32::try_from(MONTHS.iter().position(|name| *name == month)? + 1).ok()?;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    if parts.next()? != "GMT" || parts.next().is_some() || clock.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let days = u64::try_from(days_from_civil(year, month, day)).ok()?;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds))
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let month = i64::from(month);
    let shifted = if month > 2 { month - 3 } else { month + 9 };
    let of_year = (153 * shifted + 2) / 5 + i64::from(day) - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_era_days - 719_468
}
