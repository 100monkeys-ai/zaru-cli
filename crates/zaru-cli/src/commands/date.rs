// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The civil date an admission is stamped with.
//!
//! # No dependency arrives for this
//!
//! [ADR-0015] D6's attribution line carries a date — `◈ /deploy-check
//! (project · admitted 2026-08-19)` — so an admission has to record one. A
//! date crate would be a row in [ADR-0003] D2's table taken for **one line of
//! arithmetic**, and that table's own rule is that a dependency arrives in
//! the arc that has a caller worth the carry. The clock is
//! [`std::time::SystemTime`], which [`crate::session::id`] already reads, and
//! the conversion from a day count to a year, month and day is the
//! civil-from-days algorithm below.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use std::time::{SystemTime, UNIX_EPOCH};

/// Today, as `YYYY-MM-DD`, from this machine's clock.
///
/// A clock before the epoch reads as the epoch. It is not an error: a machine
/// whose clock is wrong is not a reason to refuse an admission the user asked
/// for, and the date is a record of what the user was told rather than an
/// ordering key.
#[must_use]
pub fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / 86_400);
    civil(i64::try_from(days).unwrap_or(0))
}

/// `YYYY-MM-DD` for a count of days since 1970-01-01.
///
/// Howard Hinnant's `civil_from_days`, which is exact for every day in the
/// proleptic Gregorian calendar and is arithmetic rather than a table. It is
/// written here rather than taken from a crate for the reason in this
/// module's own documentation.
#[must_use]
pub fn civil(days: i64) -> String {
    // Shift the epoch to 0000-03-01, which puts the leap day at the end of the
    // year and makes the month arithmetic a single expression.
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}
