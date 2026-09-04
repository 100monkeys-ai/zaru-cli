// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The two numbers this module takes from its caller, and the boundary that
//! refuses a useless pair.
//!
//! Neither number is decided here. ADR-0013's Neutral consequence is the
//! whole of it: "Nothing here sets a threshold. It is provider-dependent
//! configuration." The window belongs to whichever provider ADR-0012
//! resolved, and `zaru-cli` owns where both come from.
//!
//! Absolute token counts rather than a fraction of the window. A fraction
//! invites a `0.8` nobody chose to appear in this file, and the whole reason
//! these are parameters is that no record carries a number for them.
//!
//! This is a boundary in the sense [Operating Principles] means: values
//! crossing in from outside are validated here and nowhere else.
//!
//! [Operating Principles]: https://100monkeys-ai.cortex.page/zaru/p/operations/operating-principles

use core::fmt;

/// A pair of numbers the caller passed that cannot describe a real window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsRefused {
    /// A context window of zero was passed.
    ///
    /// A window of zero admits no context at all, so every assembly would
    /// refuse and ADR-0013 D7's exhaustion route would be the only outcome
    /// the loop could ever reach.
    WindowIsZero,
    /// A pressure threshold of zero was passed.
    ///
    /// Compaction runs when usage exceeds the threshold, so a threshold of
    /// zero compacts on every turn — including the ones with nothing to
    /// compact — and spends a model call each time.
    ThresholdIsZero,
    /// The threshold sits above the window it is meant to give warning of.
    ThresholdAboveWindow {
        /// The threshold that was passed.
        threshold: u64,
        /// The window that was passed.
        window: u64,
    },
}

impl fmt::Display for LimitsRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowIsZero => f.write_str(
                "context window is 0; a window that admits no context makes every assembly refuse",
            ),
            Self::ThresholdIsZero => f.write_str(
                "pressure threshold is 0; compaction would run on every turn, including the ones \
                 with nothing to compact",
            ),
            Self::ThresholdAboveWindow { threshold, window } => write!(
                f,
                "pressure threshold {threshold} is above the context window {window}; a threshold \
                 the window is reached before is a threshold that never fires"
            ),
        }
    }
}

impl std::error::Error for LimitsRefused {}

/// How many tokens the provider's context window admits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextWindow(u64);

impl ContextWindow {
    /// Take a window from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`LimitsRefused::WindowIsZero`] when `tokens` is zero.
    pub const fn new(tokens: u64) -> Result<Self, LimitsRefused> {
        if tokens == 0 {
            return Err(LimitsRefused::WindowIsZero);
        }
        Ok(Self(tokens))
    }

    /// The window in tokens.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The usage at which ADR-0013 D2's compaction runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PressureThreshold(u64);

impl PressureThreshold {
    /// Take a threshold from the caller, refusing zero.
    ///
    /// # Errors
    ///
    /// [`LimitsRefused::ThresholdIsZero`] when `tokens` is zero.
    pub const fn new(tokens: u64) -> Result<Self, LimitsRefused> {
        if tokens == 0 {
            return Err(LimitsRefused::ThresholdIsZero);
        }
        Ok(Self(tokens))
    }

    /// The threshold in tokens.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Everything the caller bounds the context with.
///
/// The two are validated **together** rather than one at a time, because
/// neither is wrong on its own and the pair can still be nonsense: a
/// threshold above the window is a warning that arrives after the failure it
/// warns about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLimits {
    window: ContextWindow,
    threshold: PressureThreshold,
}

impl ContextLimits {
    /// Take both, refusing a threshold above the window.
    ///
    /// # Errors
    ///
    /// [`LimitsRefused::ThresholdAboveWindow`] naming both values.
    pub const fn new(
        window: ContextWindow,
        threshold: PressureThreshold,
    ) -> Result<Self, LimitsRefused> {
        if threshold.get() > window.get() {
            return Err(LimitsRefused::ThresholdAboveWindow {
                threshold: threshold.get(),
                window: window.get(),
            });
        }
        Ok(Self { window, threshold })
    }

    /// The window.
    #[must_use]
    pub const fn window(self) -> ContextWindow {
        self.window
    }

    /// The threshold.
    #[must_use]
    pub const fn threshold(self) -> PressureThreshold {
        self.threshold
    }
}
