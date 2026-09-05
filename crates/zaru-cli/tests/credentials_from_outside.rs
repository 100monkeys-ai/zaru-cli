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
    Alias, Confirm, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
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
            .with_tools(ToolScope::new(["pages.read", "search.global"]))
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
            .with_tools(ToolScope::new(["pages.read", "pages.apply_patch"])),
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
