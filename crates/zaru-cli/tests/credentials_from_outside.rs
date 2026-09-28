// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A caller outside this crate drives the credential store through its own
//! public door.
//!
//! # What this establishes, and what it does not
//!
//! [Verification lessons] §25: "For any capability a user interacts with, one
//! check drives the interaction end to end and reads the outcome... Mutation
//! testing cannot find this — it operates on the assertions that exist, and
//! there is no mutant for a call that was never written." The unit checks
//! reach the store's internals; this one reaches only what `zaru-cli` exports,
//! so a type or method that was never made public fails here and nowhere else.
//!
//! **It is not evidence about the `zaru` binary.** No binary reaches the
//! credential store, because ADR-0007 D7's surfaces are `/notes tokens ...` —
//! slash commands inside an interactive session that needs a terminal backend
//! outside ADR-0003 D2's table, ADR-0010's session lifecycle and ADR-0001 D2's
//! tier resolution, none of which exists. The frame this check prints is
//! evidence about the mechanism, and it must not be quoted as evidence about
//! the binary.
//!
//! Everything here is a generated nonce. No real credential is held.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use zaru_cli::credentials::{
    Alias, AliasRefused, Confirm, CredentialStore, Description, Entry, Instance, KeyStore, Reach,
    SealingError, SealingKey, Secret, ToolScope,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A value no other call produces. Written here rather than imported: the
/// crate's own fixtures are private, and a check about the public door that
/// borrowed the crate's internals would be reaching around the door.
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
/// That it can be implemented from out here is part of what this check
/// establishes: `KeyStore` is the seam a machine's own keyring sits behind, and
/// a trait that could only be implemented from inside would not be one. The key
/// is kept so this check can open what the store sealed **without going back
/// through the store**, which is the arm of the comparison that must not travel
/// through the code under test.
struct StagedKey(SealingKey);

impl StagedKey {
    fn minted() -> Self {
        Self(SealingKey::mint())
    }
}

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

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join(nonce("cs-outside"))
}

#[test]
fn a_caller_outside_this_crate_can_store_grant_and_project() {
    let base = scratch_root();
    let root = base.join("zaru");
    let keys = StagedKey::minted();

    let mut store = CredentialStore::open(&root).expect("a fresh root opens");

    // The composer's token: ADR-0006 D4's scope exactly.
    let composer_alias = Alias::new(&nonce("composer")).expect("a nonce is a legal alias");
    let composer_secret = format!("nn_mcp_{}", nonce("secret"));
    store
        .add(
            Entry::notes(
                composer_alias.clone(),
                Description::new("the composer's search").expect("one line"),
                Secret::notes(composer_secret).expect("nn_mcp_ names a kind"),
                Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::of_names(["pages.read", "search.global"]))
            .with_workspace("zaru"),
            &keys,
            None,
        )
        .expect("the composer's token is stored");

    // An agent token, apex, confirmed.
    let agent_alias = Alias::new(&nonce("agent")).expect("a nonce is a legal alias");
    let agent_secret = format!("nn_app_{}", nonce("secret"));
    store
        .add(
            Entry::notes(
                agent_alias.clone(),
                Description::new("the agent's research").expect("one line"),
                Secret::notes(agent_secret.clone()).expect("nn_app_ names a kind"),
                Reach::Apex,
            )
            .expect("an nn_ value builds a Nuclear Notes entry")
            .with_tools(ToolScope::of_names(["pages.read", "pages.apply_patch"])),
            &keys,
            Some(&AlwaysConfirms),
        )
        .expect("a confirmed apex token is stored");

    store
        .grant_composer_role(&composer_alias)
        .expect("a read-only scope may hold the role");

    // What the agent is shown, read through the public door.
    let namespaces = store.agent_namespaces();
    println!("--- the agent's namespaces, read from outside the crate ---");
    for namespace in &namespaces {
        println!(
            "{}\n  {}\n  {} tool(s): {:?}",
            namespace.name,
            namespace.description,
            namespace.tools.len(),
            namespace.tools
        );
    }
    println!("--- the file on disk ---");
    let raw = std::fs::read_to_string(store.path()).expect("the store wrote a file");
    println!("{raw}");

    assert_eq!(
        namespaces.len(),
        1,
        "the composer's token reached the agent"
    );
    assert!(
        namespaces[0].name.ends_with(agent_alias.as_str()),
        "the projected namespace is not the agent's: {}",
        namespaces[0].name
    );
    assert!(
        namespaces[0].description.contains(Reach::APEX_MARKING),
        "the apex token is unmarked in the description the agent reads"
    );
    assert!(
        !raw.contains(&agent_secret),
        "the file on disk carries a bearer value"
    );

    // The secret comes back only through the port.
    let recovered = store
        .secret(&agent_alias, &keys)
        .expect("the port holds it");
    assert_eq!(recovered.expose_for_dispatch(), agent_secret);

    // The scratch root goes, and a control beside it stays.
    let control = base.join("control");
    std::fs::create_dir_all(&control).expect("the control is creatable");
    std::fs::remove_dir_all(&root).expect("the root is removable");
    assert!(!root.exists(), "the store's root survived removal");
    assert!(control.exists(), "the control was removed too");
    std::fs::remove_dir_all(&base).expect("the scratch tree is removable");
    assert!(!base.exists(), "the scratch tree survived removal");
}

/// **Every refusal that carries the offered value carries it as offered.**
///
/// [ADR-0007] D7 refuses a control character in an alias because the alias is
/// rendered into a terminal listing. Until 2026-09-14 the `Control` variant
/// escaped the value at construction while its three siblings stored it raw,
/// and every one of the four is rendered through `{:?}` -- so the odd one out
/// was escaped twice and the reader was asked to remove a backslash they had
/// not typed. The inconsistency is the defect: one storage rule across the
/// variants is what makes one rendering rule correct.
///
/// This asserts the rule directly, over every variant that carries a value, so
/// a fifth variant cannot reintroduce the pre-escape quietly. `Empty` and
/// `DotOrDotDot` carry nothing and have nothing to be consistent about.
///
/// [ADR-0007]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store
#[test]
fn every_alias_refusal_carries_the_value_exactly_as_offered() {
    // One input per variant, each chosen to trip exactly the variant named --
    // `Alias::new` tests in order, so a value tripping two would never reach
    // the second.
    let offered: [(&str, &str); 4] = [
        ("a separator", "work/notes"),
        ("the namespace separator", "notes:work"),
        ("a control character", "work\nnotes"),
        ("surrounding whitespace", " work "),
    ];

    let mut carried = 0usize;
    for (what, value) in offered {
        let refusal = Alias::new(value).expect_err("each of these is refused");
        let held = match &refusal {
            AliasRefused::Separator { offered, .. }
            | AliasRefused::NamespaceSeparator { offered }
            | AliasRefused::Control { offered }
            | AliasRefused::SurroundingWhitespace { offered } => offered.clone(),
            // No wildcard: a variant added later with a value has to be given
            // an arm here, which is the point of the check.
            AliasRefused::Empty | AliasRefused::DotOrDotDot => {
                panic!("{what}: {value:?} tripped a refusal that carries no value")
            }
        };
        assert_eq!(
            held, value,
            "{what}: the refusal for {value:?} carries {held:?} instead. A value pre-escaped here \
             is escaped a second time by the `{{:?}}` every render of it uses.",
        );
        carried += 1;
    }

    assert_eq!(
        carried, 4,
        "four variants carry the offered value and this check reached {carried}",
    );
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
