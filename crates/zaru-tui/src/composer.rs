// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The composer: the input widget and its hint strip.
//!
//! ADR-0005 is the record. One strip renders below the input, its content is a
//! pure function of composer state, and the mode is selected by whether the
//! prompt is empty.
//!
//! # Synchronous, and it holds no clock
//!
//! Nothing here is asynchronous and nothing here reads the machine's clock.
//! Every input carries `now: Duration`, so the debounce D3 fixes at 250 ms is
//! measured in whatever clock the caller is spending — `zaru-cli` passes the
//! system one, a test passes exact values. There is no clock object to read
//! the wrong clock through, which is the testing contract's prohibition on
//! asserting wall-clock time made structural rather than remembered.
//!
//! # What it needs from `zaru-core`: nothing
//!
//! D1 makes the strip's content a pure function of composer state, so the
//! composer subscribes to no event. `zaru-core`'s `EventSink` is how
//! [ADR-0028]'s execution-narrative renderer subscribes — a second surface in
//! this crate, and not this one. The `zaru-tui` to `zaru-core` edge stays as
//! the skeleton left it and the composer does not use it.
//!
//! # The two tiers
//!
//! Tier one is [`Entries`], a trait this crate declares and nothing in this
//! crate's product tree implements. Tier two is not a port at all: the
//! composer emits a [`SearchRequest`] and is handed a [`SearchResponse`].
//! [`entries`] says why the port is declared here rather than in the crate
//! that will own the trie.
//!
//! [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative

pub mod entries;
pub mod search;
pub mod strip;

pub use entries::{Entries, Entry, EntryKind};
pub use search::{
    DEBOUNCE, MIN_QUERY_CHARS, RequestRefused, Scope, SearchRequest, SearchResponse, SearchState,
};
pub use strip::{PickerKind, StripContent, StripMode};
