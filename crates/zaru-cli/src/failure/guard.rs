// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0016] D3's boundary: where a panic stops being a Rust panic and starts
//! being a defect report.
//!
//! D3: "Panics and internal invariant violations are caught at the session
//! boundary and presented as what they are."
//!
//! # The boundary is one call, and it is narrow because there is nowhere to
//! continue to
//!
//! ADR-0016's own Negative consequence is the constraint: "Catching panics at
//! the session boundary risks masking a corrupted state that should have
//! terminated the process. **The boundary must be narrow.**"
//!
//! [`guard`] wraps exactly one call and nothing else, and [`Guarded`] has no
//! arm that both reports a defect and hands back a value. So a caller cannot
//! carry on with a half-built result: the only thing it can do with a
//! [`Caught`] is report it and exit. That is narrowness held by the type
//! rather than by a rule somebody remembers — the widening that would break it
//! is guarding each statement instead of the body, and the check
//! `nothing_after_a_caught_panic_runs` is what that widening reddens.
//!
//! # The default panic banner has to be replaced, not merely caught
//!
//! `catch_unwind` alone does not stop the default hook printing Rust's own
//! `thread 'main' panicked at ...` banner first, and a defect that arrives as
//! a raw panic is exactly the failure this record's Context names: "a product
//! whose deliberate failures render beautifully while its infrastructure
//! failures render as a Rust panic has undermined its own thesis at the worst
//! moment". So [`guard`] installs its own hook for the duration and restores
//! the previous one afterwards. Measured in a scratch probe on 2026-09-04:
//! with the hook replaced, nothing is printed by the runtime and the message
//! and location are captured instead.
//!
//! # `panic::set_hook` is process-wide, so this is called once
//!
//! The hook is global state and `take_hook`/`set_hook` is not atomic, so two
//! concurrent guards could interleave. The product calls [`guard`] exactly
//! once, from the binary's `main`, and the checks on it hold a lock for the
//! duration so the question never arises. Two probes on 2026-09-04 failed to
//! reproduce interference, and that is not evidence that it cannot happen.
//!
//! # The panic's own words are not in the report
//!
//! Under a **delegated coordinator ruling of 2026-09-04**, open to Jeshua's
//! veto: the message travels beside the report in [`OwnWords`] rather than
//! inside it, so no [`Presentation`] can show it
//! — it is not in the value a presentation is built from. It is the one field
//! on this path that can carry arbitrary captured text, and it belongs to
//! ADR-0010's transcript when that exists. **Nothing here redacts, filters or
//! inspects it**, and [ADR-0008]'s trigger clause 6 — decided on 2026-09-05 —
//! does not ask it to: that decision covers the paths into a **model
//! prompt**, and this one ends at a person.
//!
//! # A panic on any thread the harness started, not only this one
//!
//! Until 2026-09-28 only the calling thread's unwind was caught. A panic on
//! the terminal reader's thread ended that thread, closed its channel, and the
//! pump read the source as ended and left as a person leaving does: the
//! session ended at exit 0 and printed nothing, with the panic sitting in this
//! hook's capture where nothing read it. Every thread the harness starts is
//! started through [`thread`] and named for it, and a panic on one of those is
//! reported here exactly as a panic on this one is — once the body has
//! returned, which for a session is once the terminal has been given back.
//! A panic on a thread the harness did not start is not the harness's.
//!
//! # The session it is inside is told, not passed
//!
//! D3's report names "the session id" and says "the transcript is already on
//! disk". Until 2026-09-28 the one call in `main` passed
//! [`SessionEvidence::NoSessionExists`] and nothing ever told the boundary
//! otherwise, so a defect inside a live session reported "there is no session
//! and no transcript was written" about a session whose transcript was on disk
//! — and a person told that does not look for the file that holds their work.
//! `main` runs before any session exists and cannot know one; the code that
//! opens a session does, and it is many calls down.
//!
//! So whatever opens a session calls [`inside`] as it opens it, and the
//! boundary remembers the last session it was told of **on the thread its body
//! runs on**, for as long as the body runs. The report is built there, after
//! the body has returned or unwound — never inside the hook, whatever thread
//! panicked — so a value is what is held, not a reference into a session that
//! is unwinding. It is per boundary rather than per process because the checks
//! run many sessions at once in one process, and a boundary that could be told
//! about a session a neighbouring check opened would name the wrong one; a
//! call to [`inside`] on a thread no boundary is running on is a session
//! nothing is guarding, and tells nothing.
//!
//! **The transcript is claimed only if it is there when the report is
//! built.** See [`SessionEvidence::of`]. Nothing is flushed first, because
//! nothing is held back: ADR-0010 D2's writer writes, flushes and syncs each
//! record before `record` returns, so every record a session reported written
//! is on disk already, and one being written as the panic struck is the event
//! in flight that D2 says a crash may lose.
//!
//! # A panic that leaves the session running is not left running
//!
//! A task on the session's runtime is polled on the thread the body runs on,
//! so a panic in one reaches this hook as the harness's own — and tokio then
//! swallows it, and the session carries on with a part of it gone until the
//! person leaves, which is when the report appeared. [`a_part_of_the_harness_has_died`]
//! is what the session's pump races, so the session ends as soon as the hook
//! has kept a panic, and the report follows the terminal being given back.
//!
//! # A structural guard this arc did not have to write
//!
//! `catch_unwind` does nothing under `panic = "abort"`, and the root
//! `Cargo.toml` carries no `[profile]` section, so unwinding is in force. A
//! later profile that changed it would disable this boundary silently — except
//! that the checks below would abort the test process rather than fail it, so
//! the disabling is loud without a separate gate.
//!
//! [ADR-0008]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0008-the-agent-loop
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::failure::classified::Classified;
use crate::failure::defect::{DefectReport, Location, SessionEvidence, SessionId};
use crate::failure::present::Presentation;
use core::cell::RefCell;
use core::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

/// What the name of every thread the harness starts begins with.
///
/// See [`thread`].
pub const HARNESS_THREAD: &str = "zaru-";

/// Start a thread of the harness's own, named `zaru-<name>`.
///
/// **Every thread the harness starts is started here**, so that [`guard`] can
/// tell a panic on one of them from a panic on a thread that is not the
/// harness's. `corpus_every_thread_the_harness_starts_is_its_own` in
/// `tests/files_from_outside.rs` walks the product for a thread started any
/// other way.
///
/// # Errors
///
/// When the operating system will not start a thread, as
/// [`std::thread::Builder::spawn`] reports it.
pub fn thread<T: Send + 'static>(
    name: &str,
    body: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<T>> {
    std::thread::Builder::new()
        .name(format!("{HARNESS_THREAD}{name}"))
        .spawn(body)
}

/// What the boundary running on a thread knows, for as long as its body runs.
///
/// See the module documentation for why this is per boundary and per thread.
struct Boundary {
    /// The session the body is inside, as the code that opened it said.
    told: Option<(SessionId, PathBuf)>,
    /// What the hook kept, shared with the hook.
    kept: Arc<Kept>,
}

/// The first panic of the harness's own, and the signal that there is one.
struct Kept {
    first: Mutex<Option<(Location, String)>>,
    raised: tokio::sync::Notify,
}

impl Kept {
    fn holds_a_panic(&self) -> bool {
        self.first
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }
}

thread_local! {
    static BOUNDARY: RefCell<Option<Boundary>> = const { RefCell::new(None) };
}

/// Tell the boundary this thread's body is running under which session it is
/// now inside. ADR-0016 D3.
///
/// Called by whatever opens a session, as it opens it: `compose::turn::start`
/// for a session it mints and `terminal::open` for one it resumes, continues
/// or switches to. The last one told is the one a report names. On a thread
/// no boundary is running on it tells nothing — see the module documentation.
pub fn inside(id: SessionId, transcript: PathBuf) {
    BOUNDARY.with(|boundary| {
        if let Some(boundary) = boundary.borrow_mut().as_mut() {
            boundary.told = Some((id, transcript));
        }
    });
}

/// Resolves once a thread or a task of the harness's own has panicked under
/// the boundary this thread's body is running under.
///
/// The session's pump races this, so a panic that would otherwise leave the
/// session running — a task on its runtime, whose panic tokio swallows — ends
/// it at once, and `main` reports the defect. On a thread no boundary is
/// running on it never resolves: nothing is guarding, so there is nothing to
/// report and no reason to end anything.
pub async fn a_part_of_the_harness_has_died() {
    let kept = BOUNDARY.with(|boundary| {
        boundary
            .borrow()
            .as_ref()
            .map(|boundary| Arc::clone(&boundary.kept))
    });
    let Some(kept) = kept else {
        return core::future::pending().await;
    };
    loop {
        // Armed before the check, so a panic kept between the two still wakes
        // it: `notify_one` leaves a permit when nothing is waiting yet.
        let raised = kept.raised.notified();
        if kept.holds_a_panic() {
            return;
        }
        raised.await;
    }
}

/// What the panic itself said.
///
/// **Never presented.** See the module documentation: it is held beside the
/// report rather than inside it, so no rendering can reach it, and it is
/// ADR-0010's transcript that will take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnWords(String);

impl OwnWords {
    /// What the panic said, for whatever writes the transcript.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A defect the boundary caught.
///
/// Carries D3's report and, separately, the panic's own words. There is no
/// constructor that puts the second inside the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caught {
    report: DefectReport,
    own_words: OwnWords,
}

impl Caught {
    /// D3's report.
    #[must_use]
    pub const fn report(&self) -> &DefectReport {
        &self.report
    }

    /// What the panic said. Not part of the report and not presented.
    #[must_use]
    pub const fn own_words(&self) -> &OwnWords {
        &self.own_words
    }

    /// What a renderer shows for this defect.
    ///
    /// Built from the report alone, so the panic's own words cannot reach it.
    #[must_use]
    pub fn presentation(&self) -> Presentation {
        Presentation::of(&Classified::Defect(self.report.clone()))
    }
}

impl fmt::Display for Caught {
    /// The plain-text projection of the report. Colour and layout are
    /// `zaru-tui`'s.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.presentation())
    }
}

/// What came back from a guarded call.
///
/// **There is no arm carrying both a defect and a value.** That is what makes
/// the boundary narrow: a caller holding a [`Caught`] has nothing to carry on
/// with, so a corrupted state cannot be resumed past.
#[derive(Debug)]
pub enum Guarded<T> {
    /// The body ran to completion and this is what it returned.
    Ran(T),
    /// The body panicked. ADR-0016 D3.
    Defected(Caught),
}

/// Run `body` behind ADR-0016 D3's boundary.
///
/// `version` and `report_at` come from the caller's own package metadata
/// rather than being retyped here — a list retyped beside the binary is a list
/// that drifts. The session the report names is the one the body was last
/// inside, told through [`inside`]; a body that opened none is reported as
/// having none.
///
/// Call this **once**, at the process boundary. See the module documentation
/// for why.
pub fn guard<T>(version: &str, report_at: &str, body: impl FnOnce() -> T) -> Guarded<T> {
    let kept = Arc::new(Kept {
        first: Mutex::new(None),
        raised: tokio::sync::Notify::new(),
    });
    let sink = Arc::clone(&kept);
    let boundary = std::thread::current().id();
    BOUNDARY.with(|state| {
        *state.borrow_mut() = Some(Boundary {
            told: None,
            kept: Arc::clone(&kept),
        });
    });

    let previous = Arc::new(panic::take_hook());
    let forward = Arc::clone(&previous);
    panic::set_hook(Box::new(move |info| {
        // The harness's own panics are this thread's and those of the threads
        // it started through [`thread`]. The hook is the process's, so a
        // panic on anybody else's thread reaches it too, and is not ours to
        // report -- so it goes to whatever hook was there before, as if this
        // boundary were not. Until 2026-09-28 it was dropped: measured when a
        // check's failing assertion printed no sentence at all, because a
        // neighbouring check's boundary was installed at the moment it failed.
        let on = std::thread::current();
        let ours = on.id() == boundary
            || on
                .name()
                .is_some_and(|name| name.starts_with(HARNESS_THREAD));
        if !ours {
            forward(info);
            return;
        }
        let location = info
            .location()
            .map_or_else(Location::unknown, |at| Location {
                file: at.file().to_owned(),
                line: at.line(),
                column: at.column(),
            });
        let said = info.payload().downcast_ref::<&str>().map_or_else(
            || {
                info.payload()
                    .downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_default()
            },
            |said| (*said).to_owned(),
        );
        // The first is kept: a panic that follows another is usually its
        // consequence, and the report names where the defect surfaced.
        let mut held = sink.first.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            *held = Some((location, said));
            drop(held);
            sink.raised.notify_one();
        }
    }));

    let outcome = panic::catch_unwind(AssertUnwindSafe(body));
    // This boundary's hook is dropped first, and with it the second handle on
    // the one before it, so the one before it goes back exactly as it was.
    drop(panic::take_hook());
    match Arc::try_unwrap(previous) {
        Ok(previous) => panic::set_hook(previous),
        Err(shared) => panic::set_hook(Box::new(move |info| shared(info))),
    }
    let told = BOUNDARY
        .with(|state| state.borrow_mut().take())
        .and_then(|boundary| boundary.told);
    let panicked = kept
        .first
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    // Read now, after the body, so the transcript is claimed only if it is on
    // disk as the report is written. See `SessionEvidence::of`.
    let session = || {
        told.map_or(SessionEvidence::NoSessionExists, |(id, transcript)| {
            SessionEvidence::of(id, transcript)
        })
    };

    match (outcome, panicked) {
        // A body that returned, with no panic on any thread of the harness's.
        (Ok(value), None) => Guarded::Ran(value),
        // A body that returned **after a thread of the harness's panicked** is
        // a defect all the same: the value it returned is what the rest of the
        // harness made of that thread's disappearance, and until 2026-09-28 a
        // terminal reader that panicked was read as a person leaving, at exit
        // 0 with nothing printed. The value is dropped here, which is the
        // boundary staying narrow: there is no arm that carries it on.
        (Ok(_), Some((location, said))) | (Err(_), Some((location, said))) => {
            Guarded::Defected(Caught {
                report: DefectReport::new(version, report_at, location, session()),
                own_words: OwnWords(said),
            })
        }
        (Err(_), None) => Guarded::Defected(Caught {
            report: DefectReport::new(version, report_at, Location::unknown(), session()),
            own_words: OwnWords(String::new()),
        }),
    }
}
