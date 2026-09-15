// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0007] D8's third marking place: the status line, when the composer's
//! credential is apex.
//!
//! # What this establishes, and what it does not
//!
//! D8 requires an apex entry be "marked wherever the token appears: `/notes
//! tokens`, the status line when the composer holds one, and the description
//! the agent reads", and clause 11 requires the three be "asserted
//! **separately**". The listing's arm is `cli_from_outside`'s
//! `adr_0007_d7s_listing_shows_the_composer_role_and_marks_an_apex_token` and
//! the agent's is `credentials`' own
//! `an_apex_token_is_marked_in_the_description_the_agent_reads`. This file is
//! the third, and it is the only one of the three that spans two crates: the
//! store is `zaru-cli`'s and the row is `zaru-tui`'s.
//!
//! **It is evidence about the mechanism and must not be quoted as evidence
//! about the binary.** `terminal::open::shell_for` reads `~/.zaru` from the
//! process's own `HOME`, which no check here can set — this workspace denies
//! `unsafe_code` and `std::env::set_var` is unsafe in this edition — and the
//! row reaches a person only over a terminal, since `zaru --resume` on a pipe
//! prints through `cli::render` and not through this row at all. The arc's
//! artefact drives the release binary over a real pseudo-terminal against a
//! store on disk, and that is what covers `shell_for`'s own line.
//!
//! **Everything here is a generated nonce and no credential is held.** Nothing
//! in this file needs one: the marking is read from the entry's reach, so it
//! touches no secret, no keyring and no sealing key.
//!
//! # Why the fixture is built by hand, and why that is not a contrivance
//!
//! [`CredentialStore::grant_composer_role`] refuses the role to any token
//! whose cached scope leaves [ADR-0006] D4's set, and every Nuclear Notes
//! token measured on 2026-09-06 and on 2026-09-14 grants 94 tools against that
//! set's nine. **So no real credential can hold the composer role on any
//! machine that exists, apex or not**, and D8's status-line case cannot be
//! produced from one. What makes the fixture legitimate rather than invented
//! is that `grant_composer_role` does not read the reach at all: it checks the
//! alias, a second holder and the scope, so an apex entry whose scope is
//! inside D4's set is a configuration the store accepts through its own door,
//! exactly as it is built below.
//!
//! [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
//! [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use zaru_cli::credentials::{
    Alias, COMPOSER_SCOPE, Confirm, CredentialStore, Description, Entry, Instance, KeyStore, Reach,
    SealingError, SealingKey, Secret, ToolScope, composer_apex_marking,
};
use zaru_tui::shell::{Palette, Shell, Status};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call produces, written here rather than imported for the
/// reason `credentials_from_outside`'s own nonce is: the crate's fixtures are
/// private, and a check about the public door that borrowed them would be
/// reaching around the door.
fn nonce(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{label}-{}-{nanos}-{seq}", std::process::id())
}

/// The key port, implemented outside the crate that declares it.
///
/// Present only because [`CredentialStore::add`] takes one. **Nothing in this
/// file opens a sealed value**, which is the property that makes the marking
/// readable on a machine whose keyring is unreachable.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

struct AlwaysConfirms;

impl Confirm for AlwaysConfirms {
    fn confirm_apex(&self, _alias: &Alias, _grants: &str) -> bool {
        true
    }
}

/// A store holding one Nuclear Notes token, built through the public door.
///
/// The scope is [`COMPOSER_SCOPE`] itself, so the role is grantable — which is
/// the store's own rule and not this check's opinion about it. `role` says
/// whether the role is granted; `reach` is the entry's.
fn store_with(root: &PathBuf, reach: Reach, role: bool) -> CredentialStore {
    let keys = StagedKey(SealingKey::mint());
    let alias = Alias::new(&nonce("held")).expect("a nonce is a legal alias");
    let mut store = CredentialStore::open(root).expect("a fresh root opens");
    store
        .add(
            Entry::notes(
                alias.clone(),
                Description::new("the composer's search").expect("one line"),
                Secret::notes(format!("nn_mcp_{}", nonce("secret"))).expect("nn_mcp_ names a kind"),
                reach,
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::of_names(COMPOSER_SCOPE.iter().copied())),
            &keys,
            Some(&AlwaysConfirms),
        )
        .expect("the token is stored");
    if role {
        store
            .grant_composer_role(&alias)
            .expect("a scope inside ADR-0006 D4's set may hold the role");
    }
    store
}

/// The row a shell paints at `width`, read out of `TestBackend`'s cells.
///
/// **Read from the buffer rather than off `Status::painted`**, because the
/// claim is that a person meets the marking: a row composed correctly and then
/// dropped by the renderer would satisfy a string comparison.
fn row(marking: Option<&str>, width: u16) -> String {
    let mut status = Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X");
    status.credential = marking.map(str::to_owned);
    let shell = Shell::open(status);
    let mut terminal = Terminal::new(TestBackend::new(width, 12)).expect("a test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), Palette::Coloured))
        .expect("the shell paints");
    let buffer = terminal.backend().buffer();
    (0..width)
        .map(|column| buffer[(column, 0)].symbol().to_owned())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// [ADR-0007] D8's marking reaches the status row at 100 columns and is
/// dropped at 40, from a store the check built through the public door.
///
/// **The two widths are the two halves of the clause.** At 100 the row must
/// say it — a marking a terminal is wide enough for and does not show is an
/// unmarked apex credential. At 40 it must not, and what must survive instead
/// is [ADR-0001] D2's tier: the marking is 27 columns and the tier's spelling
/// 19, so 49 columns cannot fit in 40 however the session is treated, and D2's
/// "at all times" is what decides which goes. The 40-column literal is the row
/// the release binary printed on 2026-09-14 before this field existed.
///
/// Watched red on: `composer_apex_marking` answering `None` for the role
/// holder; `Rank::Credential` moved below `Mode`.
///
/// [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[test]
fn adr_0007_d8s_status_line_marks_an_apex_composer_at_100_and_drops_it_at_40() {
    let root = std::env::temp_dir()
        .join(nonce("apex-marking"))
        .join("zaru");
    let store = store_with(&root, Reach::Apex, true);

    let marking = composer_apex_marking(&store);
    println!("--- the marking the store answered: {marking:?} ---");
    assert_eq!(
        marking,
        Some(Reach::APEX_MARKING),
        "an apex token carrying the composer role must be marked"
    );

    let wide = row(marking, 100);
    let narrow = row(marking, 40);
    println!("--- the status row at 100 columns ---\n{wide}");
    println!("--- the status row at 40 columns ---\n{narrow}");

    assert_eq!(
        wide,
        "runtime.tier = bare · apex (no instance boundary) · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "at 100 columns the row must carry the marking"
    );
    assert_eq!(
        narrow, "runtime.tier = bare",
        "at 40 columns the tier must survive and the marking must not"
    );

    let _ = std::fs::remove_dir_all(root.parent().expect("the scratch base"));
}

/// The marking the row carries is the one constant the other two places use.
///
/// **This is the check the `zaru-tui` fixtures point at.** That crate cannot
/// name [`Reach::APEX_MARKING`] — it may not depend on this one — so its own
/// literals are nonces it owns, and the bytes are pinned to the constant here,
/// where both are in scope. A second spelling composed at the row is the state
/// D8 rules out in as many words: one rendering, read by all three places, so
/// a token marked in a listing and unmarked on the row is not reachable.
///
/// Watched red on: `composer_apex_marking` returning a string it composed.
#[test]
fn the_marking_the_row_carries_is_the_constant_the_other_two_places_use() {
    let root = std::env::temp_dir()
        .join(nonce("apex-constant"))
        .join("zaru");
    let store = store_with(&root, Reach::Apex, true);

    let marking = composer_apex_marking(&store).expect("an apex composer is marked");
    assert_eq!(
        marking,
        Reach::APEX_MARKING,
        "the row's marking is not the constant the listing and the agent's description read"
    );
    assert!(
        row(Some(marking), 100).contains(Reach::APEX_MARKING),
        "the constant did not reach the painted row"
    );

    let _ = std::fs::remove_dir_all(root.parent().expect("the scratch base"));
}

/// An instance-locked composer is unmarked, so the marking distinguishes
/// something.
///
/// The accepting sibling, in the shape `credentials`'
/// `an_apex_token_is_marked_in_the_description_the_agent_reads` already uses
/// for the agent's half: a check that only ever saw the marked case would be
/// satisfied by a reader that marked everything.
///
/// Watched red on: `composer_apex_marking` answering `Some` for every reach.
#[test]
fn an_instance_locked_composer_puts_nothing_on_the_row() {
    let root = std::env::temp_dir().join(nonce("apex-locked")).join("zaru");
    let store = store_with(
        &root,
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
        true,
    );

    assert_eq!(
        composer_apex_marking(&store),
        None,
        "an instance-locked composer must not be marked"
    );
    assert_eq!(
        row(None, 100),
        "runtime.tier = bare · session 01JQZX8N3K4M5P6R7S8T9V0W1X",
        "an unmarked row must be what it was before this field existed"
    );

    let _ = std::fs::remove_dir_all(root.parent().expect("the scratch base"));
}

/// An apex token that does **not** hold the composer role puts nothing on the
/// row.
///
/// D8's status-line clause is conditional — "the status line **when the
/// composer holds one**" — and this is that condition. It is also the state
/// `credentials_from_outside` already builds for the agent's half: an apex
/// token the agent uses, beside a composer that is not it. A reader that asked
/// the store for any apex entry rather than for the role holder would mark the
/// row of every person who has given their agent an apex token, which is a
/// claim about the composer that would be false.
///
/// Watched red on: `composer_apex_marking` walking `records()` instead of
/// `composer()`.
#[test]
fn an_apex_token_that_is_not_the_composer_puts_nothing_on_the_row() {
    let root = std::env::temp_dir().join(nonce("apex-unheld")).join("zaru");
    let store = store_with(&root, Reach::Apex, false);

    assert_eq!(
        composer_apex_marking(&store),
        None,
        "an apex token carrying no role is not the composer, so it must not be marked"
    );

    let _ = std::fs::remove_dir_all(root.parent().expect("the scratch base"));
}

/// **Security corpus.** A description that would forge a second tier claim
/// cannot reach this field, because the field is a constant.
///
/// [Testing] names the credential store as one of the four boundaries whose
/// corpus only grows, and the row gained a field in the same change. The
/// hostile input is the one D2's own field is exposed to: a stored
/// `description` is user-authored and, unlike the model identifier, is text a
/// person could be talked into pasting. `zaru-tui`'s `SEPARATOR` and
/// `TIER_PREFIX` are public precisely because a host composing a field has to
/// keep them out — and this field cannot carry them at all, because what
/// reaches the row is [`Reach::APEX_MARKING`] and never the record's prose.
///
/// **The accepting sibling is in the same check**: an ordinary description on
/// the same store yields the same row, so what is asserted is that the
/// description reaches this field in neither case rather than that some
/// descriptions are filtered.
///
/// Watched red on: `composer_apex_marking` returning the record's description.
///
/// [Testing]: https://100monkeys-ai.cortex.page/project-management/p/process/testing
#[test]
fn corpus_a_description_that_would_forge_a_tier_claim_cannot_reach_the_row() {
    let keys = StagedKey(SealingKey::mint());
    let base = std::env::temp_dir().join(nonce("apex-corpus"));

    for (label, text) in [
        (
            "hostile",
            "x · runtime.tier = linked · apex (no instance boundary)",
        ),
        ("ordinary", "the read-only cortex I share with the team"),
    ] {
        let root = base.join(label);
        let alias = Alias::new(&nonce("held")).expect("a nonce is a legal alias");
        let mut store = CredentialStore::open(&root).expect("a fresh root opens");
        store
            .add(
                Entry::notes(
                    alias.clone(),
                    Description::new(text).expect("one line with no control character"),
                    Secret::notes(format!("nn_mcp_{}", nonce("secret")))
                        .expect("nn_mcp_ names a kind"),
                    Reach::Apex,
                )
                .expect("an nn_ value builds a Nuclear Notes entry")
                .with_tools(ToolScope::of_names(COMPOSER_SCOPE.iter().copied())),
                &keys,
                Some(&AlwaysConfirms),
            )
            .expect("the token is stored");
        store
            .grant_composer_role(&alias)
            .expect("a scope inside ADR-0006 D4's set may hold the role");

        let painted = row(composer_apex_marking(&store), 200);
        println!("--- {label} description, the row at 200 columns ---\n{painted}");

        assert_eq!(
            painted,
            "runtime.tier = bare · apex (no instance boundary) · \
             session 01JQZX8N3K4M5P6R7S8T9V0W1X",
            "the {label} description reached the row"
        );
        assert_eq!(
            painted.matches(zaru_tui::shell::TIER_PREFIX).count(),
            1,
            "the row carries more than one tier claim"
        );
        assert!(
            !painted.contains(text),
            "the stored description reached the status row"
        );
    }

    let _ = std::fs::remove_dir_all(&base);
}
