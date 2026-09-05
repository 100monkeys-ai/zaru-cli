// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0010] D1's `meta.toml`, written and read.
//!
//! # The file that had no writer, and why it has one now
//!
//! D1's directory holds three files and two were written. [`MetaStore`] has
//! been a port with no implementation since the `session-lifecycle` arc landed,
//! for one reason: [ADR-0003] D2's table named no TOML crate. It does now, and
//! this module is the implementation. The bytes go through
//! [`crate::config::file::TomlFile`] on the way in and through the `toml`
//! crate's own rendering on the way out — a `std`-only emitter for five flat
//! keys was considered and refused when this record landed, because `workspace`
//! and `provider` arrive from outside and getting basic-string escaping
//! *nearly* right and calling the file `meta.toml` is the "for now" the harness
//! forbids.
//!
//! # It records a sixth thing D1 does not name, and it has to
//!
//! D1 says `meta.toml` records "tier, workspace, provider, started, ended".
//! [`Meta`] holds the tier as a [`ResolvedTier`], which **cannot be constructed
//! without naming the configuration layer the tier came from** — that is
//! [ADR-0014] D3's argument made structural by the `runtime-tiers` arc. So a
//! writer that recorded the tier alone would force the reader to invent a
//! layer, and a session's tier would come back as a value the file never
//! carried.
//!
//! The file therefore carries `tier_from`, whose value is
//! [`Layer::label`](crate::config::Layer::label) — **the same word ADR-0014
//! D3's block prints in its supplier column**, so the file and the block are
//! one vocabulary rather than two. Written as an accepted Update on ADR-0010 D1
//! under a delegated coordinator ruling of 2026-09-05, open to Jeshua's veto.
//!
//! # A malformed `meta.toml` is a defect, and that is the opposite of a config
//! file
//!
//! [`crate::cli::classify`] reads a malformed `~/.zaru/config.toml` or
//! `./zaru.toml` as the user's, because this harness never writes either.
//! **This file's only writer is this harness**, so one that does not parse is
//! one we produced or one somebody hand-edited, and [ADR-0016] D1 puts the
//! first of those in the defect row. The two are told apart by which port
//! failed rather than by anything on the value.
//!
//! # Nothing in the product calls this yet, and that is stated rather than
//! implied
//!
//! **The binary starts no session**, so no product path writes a `meta.toml`
//! and none reads one. `resume` and `sessions list` are deliberately not wired
//! to it: every session directory on every machine predates this writer, and a
//! read wired into resume would report a defect for each of them. The arc that
//! starts a session is the one that wires both halves.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::config::file::{SizeCeiling, TomlFile};
use crate::config::{Layer, Value};
use crate::runtime::{ResolvedTier, Tier};
use crate::session::id::Millis;
use crate::session::meta::{Meta, MetaFailure, MetaStore};
use crate::session::store::FILE_MODE;
use std::path::{Path, PathBuf};

/// D1's `tier`.
pub const TIER_KEY: &str = "tier";

/// The configuration layer that supplied the tier. See the module
/// documentation for why D1's five fields are six here.
pub const TIER_FROM_KEY: &str = "tier_from";

/// D1's `workspace`.
pub const WORKSPACE_KEY: &str = "workspace";

/// D1's `provider`.
pub const PROVIDER_KEY: &str = "provider";

/// D1's `started`, in milliseconds.
pub const STARTED_KEY: &str = "started";

/// D1's `ended`, in milliseconds, absent while the session is running.
pub const ENDED_KEY: &str = "ended";

/// How large a `meta.toml` this harness will parse, in bytes.
///
/// Six flat keys, so a file anywhere near this is not one this harness wrote.
/// A separate number from [`crate::cli::FILE_CEILING_BYTES`] because they bound
/// different things: that one bounds a file a person hand-writes, and this one
/// bounds a file only this harness writes.
pub const CEILING_BYTES: u64 = 64 * 1024;

/// [ADR-0010] D1's `meta.toml`, at a session's own path.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaFile {
    path: PathBuf,
}

impl MetaFile {
    /// The metadata file at a path.
    ///
    /// A path rather than a [`Session`](crate::session::Session), so that a
    /// caller holding only a directory can reach it and so that this module
    /// does not have to know how a session directory is named.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What a session recorded about itself, or `None` where nothing has.
    ///
    /// **This is the honest reader** and [`MetaStore::read`] is it with the
    /// absence turned into a failure, because that trait's signature has no
    /// room for one. A session directory written before this file had a writer
    /// has no `meta.toml`, and that is a datum rather than a fault — see the
    /// module documentation.
    ///
    /// # Errors
    ///
    /// [`MetaFailure`] when the file is there and cannot be read, does not
    /// parse, or does not carry what D1 says it carries.
    pub fn read_if_present(&self) -> Result<Option<Meta>, MetaFailure> {
        let ceiling = SizeCeiling::new(CEILING_BYTES).expect("64 KiB is not zero");
        let Some(document) = TomlFile::at(&self.path, ceiling)
            .read()
            .map_err(|refusal| MetaFailure::new(refusal.to_string()))?
        else {
            return Ok(None);
        };

        let tier = self.text(&document, TIER_KEY)?;
        let tier = Tier::named(tier).ok_or_else(|| {
            self.wrong(format_args!(
                "`{TIER_KEY}` names no runtime tier; ADR-0001 D1 defines exactly three"
            ))
        })?;
        let supplied_by = self.text(&document, TIER_FROM_KEY)?;
        let supplied_by = Layer::named(supplied_by).ok_or_else(|| {
            self.wrong(format_args!(
                "`{TIER_FROM_KEY}` names no configuration layer; ADR-0014 D1 has five and this \
                 file records the label its own explain block prints"
            ))
        })?;

        let mut meta = Meta::new(
            ResolvedTier::supplied(tier, supplied_by),
            self.optional_text(&document, WORKSPACE_KEY)?,
            self.optional_text(&document, PROVIDER_KEY)?,
            Millis::new(self.millis(&document, STARTED_KEY)?.ok_or_else(|| {
                self.wrong(format_args!(
                    "`{STARTED_KEY}` is absent, and a session started"
                ))
            })?),
        );
        meta.ended = self.millis(&document, ENDED_KEY)?.map(Millis::new);
        Ok(Some(meta))
    }

    /// A refusal naming this file, in this module's own words.
    ///
    /// **It carries no value out of the document.** `workspace` and `provider`
    /// arrive from outside, and [`MetaFailure`]'s own documentation says an
    /// implementation must not put either in one.
    fn wrong(&self, detail: std::fmt::Arguments<'_>) -> MetaFailure {
        MetaFailure::new(format!("{}: {detail}", self.path.display()))
    }

    fn text<'a>(
        &self,
        document: &'a crate::config::Table,
        key: &'static str,
    ) -> Result<&'a str, MetaFailure> {
        match document.get(key) {
            Some(Value::Text(text)) => Ok(text),
            Some(other) => Err(self.wrong(format_args!(
                "`{key}` is {}, and this file records it as text",
                other.shape()
            ))),
            None => Err(self.wrong(format_args!(
                "`{key}` is absent, and ADR-0010 D1 says this file records it"
            ))),
        }
    }

    fn optional_text(
        &self,
        document: &crate::config::Table,
        key: &'static str,
    ) -> Result<Option<String>, MetaFailure> {
        match document.get(key) {
            None => Ok(None),
            Some(Value::Text(text)) => Ok(Some(text.clone())),
            Some(other) => Err(self.wrong(format_args!(
                "`{key}` is {}, and this file records it as text",
                other.shape()
            ))),
        }
    }

    fn millis(
        &self,
        document: &crate::config::Table,
        key: &'static str,
    ) -> Result<Option<u64>, MetaFailure> {
        match document.get(key) {
            None => Ok(None),
            Some(Value::Integer(number)) => u64::try_from(*number).map(Some).map_err(|_| {
                self.wrong(format_args!(
                    "`{key}` is negative, and a reading of a wall clock is not"
                ))
            }),
            Some(other) => Err(self.wrong(format_args!(
                "`{key}` is {}, and this file records it as a whole number of milliseconds",
                other.shape()
            ))),
        }
    }

    /// What a [`Meta`] is written as.
    ///
    /// A `toml::Table` rendered by the crate that defines the format, rather
    /// than a `format!` this module wrote: `workspace` and `provider` arrive
    /// from outside and may carry a quote, a newline or a backslash, and a
    /// nearly-correct escaping is the failure ADR-0010's own Update named.
    ///
    /// The keys come out sorted, because the map is ordered, so the bytes are a
    /// function of the contents rather than of the order this method happened
    /// to insert them in.
    fn rendered(&self, meta: &Meta) -> Result<String, MetaFailure> {
        let mut document = toml::Table::new();
        document.insert(
            TIER_KEY.to_owned(),
            toml::Value::String(meta.tier().as_str().to_owned()),
        );
        document.insert(
            TIER_FROM_KEY.to_owned(),
            toml::Value::String(meta.resolved_tier().supplied_by().label().to_owned()),
        );
        if let Some(workspace) = &meta.workspace {
            document.insert(
                WORKSPACE_KEY.to_owned(),
                toml::Value::String(workspace.clone()),
            );
        }
        if let Some(provider) = &meta.provider {
            document.insert(
                PROVIDER_KEY.to_owned(),
                toml::Value::String(provider.clone()),
            );
        }
        document.insert(STARTED_KEY.to_owned(), self.reading(meta.started)?);
        if let Some(ended) = meta.ended {
            document.insert(ENDED_KEY.to_owned(), self.reading(ended)?);
        }
        Ok(document.to_string())
    }

    /// A clock reading as TOML holds a whole number.
    ///
    /// TOML's integer is 64-bit and signed; a [`Millis`] is unsigned. The
    /// conversion is checked rather than cast, because a cast would write a
    /// negative timestamp rather than saying it could not.
    fn reading(&self, reading: Millis) -> Result<toml::Value, MetaFailure> {
        i64::try_from(reading.get())
            .map(toml::Value::Integer)
            .map_err(|_| {
                self.wrong(format_args!(
                    "a clock reading is past what this file can hold; TOML's whole number is \
                     signed and 64-bit"
                ))
            })
    }
}

impl MetaStore for MetaFile {
    /// Record what this session is, atomically.
    ///
    /// Through [`crate::atomic::write`], which is the one atomic replace in this
    /// crate and which the checkpoint and the credential store also go through:
    /// a reader sees the whole previous file or the whole new one, and a
    /// process that dies inside this call leaves the previous one intact. The mode is [`FILE_MODE`], which
    /// [ADR-0010] D5 makes the only protection a session's files have.
    ///
    /// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
    fn write(&mut self, meta: &Meta) -> Result<(), MetaFailure> {
        let rendered = self.rendered(meta)?;
        crate::atomic::write(&self.path, rendered.as_bytes(), FILE_MODE).map_err(|failed| {
            MetaFailure::new(format!(
                "could not {} {}: {}",
                failed.action,
                failed.path.display(),
                failed.source
            ))
        })
    }

    fn read(&self) -> Result<Meta, MetaFailure> {
        self.read_if_present()?.ok_or_else(|| {
            MetaFailure::new(format!(
                "{} does not exist, so this session recorded nothing about itself",
                self.path.display()
            ))
        })
    }
}
