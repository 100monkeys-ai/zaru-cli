// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The vocabulary's checks, and the first two cases of the hostile corpus.
//!
//! [Testing]'s rule for this surface: "Every escape found at a security
//! boundary — a sandbox, a permission model, a credential store, anything
//! deciding what a model-driven action may reach — joins a permanent
//! hostile-input corpus as its reproduction... **the corpus never shrinks**."
//! The cases in [`ALIAS_CORPUS`] are that corpus's first entries and are
//! added to, never removed from.
//!
//! [Testing]: https://100monkeys-ai.cortex.page/project-management/p/process/testing

use crate::credentials::alias::{Alias, AliasRefused};
use crate::credentials::entry::{Description, Ttl};
use crate::credentials::fixtures::{app_secret_nonce, ascii_core, nonce, personal_secret_nonce};
use crate::credentials::secret::{Kind, REDACTED, Secret};
use core::time::Duration;

/// Aliases that must never reach the store, with what each one is trying.
///
/// A table rather than a run of `assert!`s, so that the count is visible and
/// a case removed is a case a reader can see was removed.
const ALIAS_CORPUS: &[(&str, &str)] = &[
    ("", "the empty name"),
    (".", "the current directory"),
    ("..", "the parent directory"),
    ("../../etc/passwd", "a relative traversal out of the store"),
    ("/etc/passwd", "an absolute path"),
    ("a/b", "a nested path"),
    ("a\\b", "a windows-style path"),
    ("..\\..\\windows", "a windows-style traversal"),
    ("work\0hidden", "a NUL, which truncates a C string"),
    (
        "work\nagent",
        "a newline, which forges a second listing row",
    ),
    (
        "work\u{1b}[2K",
        "an ANSI erase-line sequence, which hides a neighbouring row",
    ),
    (
        "notes:work",
        "a colon, which forges an ADR-0007 D5 namespace",
    ),
    (" work", "leading whitespace, invisible in a listing"),
    ("work ", "trailing whitespace, invisible in a listing"),
];

// The corpus case: a path that escapes the store's directory.
//
// The primary defence is that there is nothing to escape with — the store is
// one file and an alias is a key inside it, never a path segment. This is the
// second line, and it is what stops an alias that *reads* as a traversal from
// reaching a listing, a namespace, or a future caller who does build a path.
//
// The mutant: return `Ok(Self(offered.to_owned()))` from `Alias::new` before
// any check. Every row below reddens, and the failure names the row.
#[test]
fn an_alias_that_would_escape_the_store_is_refused_at_construction() {
    for (offered, what_it_tries) in ALIAS_CORPUS {
        let outcome = Alias::new(offered);
        assert!(
            outcome.is_err(),
            "the alias {offered:?} was taken, and it is {what_it_tries}"
        );
    }
    assert_eq!(
        ALIAS_CORPUS.len(),
        14,
        "the hostile-input corpus only grows; a case was removed rather than added"
    );
}

// The counterpart, and it is not decoration. Verification lessons §8: an
// instrument that has only ever printed one answer has not been shown able to
// print another. A refusal that refuses everything is not a boundary.
#[test]
fn an_alias_carrying_a_multibyte_grapheme_is_taken() {
    let offered = nonce("alias");
    let alias = Alias::new(&offered).unwrap_or_else(|refusal| {
        panic!(
            "the alias {offered:?} was refused, and it carries nothing the corpus names: {refusal}"
        )
    });
    assert_eq!(alias.as_str(), offered);
}

#[test]
fn each_refusal_names_the_shape_it_refused_rather_than_a_generic_reason() {
    assert_eq!(Alias::new(""), Err(AliasRefused::Empty));
    assert_eq!(Alias::new(".."), Err(AliasRefused::DotOrDotDot));
    assert!(matches!(
        Alias::new("a/b"),
        Err(AliasRefused::Separator { found: '/', .. })
    ));
    assert!(matches!(
        Alias::new("notes:work"),
        Err(AliasRefused::NamespaceSeparator { .. })
    ));
    assert!(matches!(
        Alias::new("work\u{1b}[2K"),
        Err(AliasRefused::Control { .. })
    ));
    assert!(matches!(
        Alias::new(" work"),
        Err(AliasRefused::SurroundingWhitespace { .. })
    ));
}

// The corpus case: a value that appears in an error.
//
// Two arms, deliberately. Asserting only that the value is absent would be
// satisfied by a Debug that printed nothing at all, which is a different
// defect wearing the same green. So the marker must be present *and* the
// value absent.
//
// The two arms are also two readers: the left-hand side is the nonce this
// check generated, and the right-hand side is the string the formatter
// produced. Neither travels through the other.
#[test]
fn a_secrets_debug_carries_the_redaction_and_not_the_value() {
    let value = personal_secret_nonce();
    let secret = Secret::new(value.clone()).expect("an nn_mcp_ value names a kind");

    let rendered = format!("{secret:?}");
    assert!(
        rendered.contains(REDACTED),
        "a secret's Debug printed {rendered:?}, which carries no redaction marker at all"
    );
    assert_absent(&rendered, &value, "a secret's Debug");
}

/// Asserts that a rendering published neither a value nor anything that
/// identifies it, and says which arm caught it.
///
/// Two arms, because one of them is blind. `{:?}` on a `String` escapes a
/// combining mark to `\u{301}`, so a rendering that published every byte of a
/// nonce does not `contain` that nonce — the mutation that put a bearer value
/// into a refusal through `{:?}` survived a check with only the first arm.
/// The ASCII core survives every escaping scheme and identifies the value on
/// its own.
fn assert_absent(rendered: &str, value: &str, what: &str) {
    assert!(
        !rendered.contains(value),
        "{what} published the bearer value verbatim: {rendered}"
    );
    let core = ascii_core(value);
    assert!(
        !core.is_empty(),
        "the fixture produced no ASCII core, so this check asserted nothing"
    );
    assert!(
        !rendered.contains(core),
        "{what} published the bearer value in an escaped form; its ASCII core {core:?} is in \
         {rendered}"
    );
}

#[test]
fn a_kind_is_read_off_the_value_and_never_stored_beside_it() {
    let personal = Secret::new(personal_secret_nonce()).expect("nn_mcp_ names a kind");
    let app = Secret::new(app_secret_nonce()).expect("nn_app_ names a kind");

    assert_eq!(personal.kind(), Kind::Personal);
    assert_eq!(app.kind(), Kind::App);
    assert_eq!(Kind::Personal.as_str(), "personal");
    assert_eq!(Kind::App.as_str(), "app");
}

#[test]
fn a_secret_whose_prefix_names_no_kind_is_refused() {
    for prefix in ["", "nn_", "nn_mcp", "sk-", "aegis_", "NN_MCP_"] {
        let value = format!("{prefix}{}", nonce("wrong"));
        assert!(
            Secret::new(&value).is_err(),
            "a value beginning {prefix:?} was taken, and ADR-0007 D2 admits only nn_mcp_ and nn_app_"
        );
    }
}

// A refusal is exactly the text that gets pasted into a report. This is the
// corpus case applied to the one refusal that is handed a bearer value.
#[test]
fn the_refusal_for_an_unknown_prefix_does_not_quote_the_value() {
    let value = format!("sk-{}", nonce("provider"));
    let refusal = Secret::new(&value).expect_err("sk- names no kind");

    let displayed = refusal.to_string();
    let debugged = format!("{refusal:?}");
    assert_absent(&displayed, &value, "a refusal's Display");
    assert_absent(&debugged, &value, "a refusal's Debug");
    // And it still has to be useful to the person reading it.
    assert!(
        displayed.contains("nn_mcp_") && displayed.contains("nn_app_"),
        "the refusal does not say what a bearer value must begin with: {displayed}"
    );
}

#[test]
fn a_ttl_of_zero_is_refused_and_any_other_is_taken() {
    assert!(Ttl::new(Duration::ZERO).is_err());
    let window = Duration::from_secs(300);
    assert_eq!(Ttl::new(window).expect("a non-zero ttl").get(), window);
}

#[test]
fn a_description_that_is_not_one_renderable_line_is_refused() {
    assert!(Description::new("a token for work\nand a forged row").is_err());
    assert!(Description::new("a token\u{1b}[2Kfor work").is_err());

    let text = format!("read-only, {}", nonce("description"));
    let description = Description::new(&text).expect("one line of ordinary text");
    assert_eq!(description.as_str(), text);
}

// ---------------------------------------------------------------------------
// The store on disk.
// ---------------------------------------------------------------------------

use crate::credentials::entry::{Entry, Instance, Reach, ToolScope};
use crate::credentials::fixtures::{InMemorySecrets, ScratchRoot, StagedConfirmer};
use crate::credentials::store::{CredentialStore, DIRECTORY_MODE, FILE_MODE, STORE_FILE};
use std::os::unix::fs::PermissionsExt;

/// An ordinary instance-locked entry carrying a fresh nonce for a secret.
fn staged_entry(label: &str) -> (Entry, String) {
    let secret_value = personal_secret_nonce();
    let entry = Entry::new(
        Alias::new(&nonce(label)).expect("a nonce is a legal alias"),
        Description::new(format!("{label}, {}", nonce("purpose"))).expect("one line"),
        Secret::new(secret_value.clone()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .with_tools(ToolScope::new(["pages.read", "search.global"]))
    .with_workspace("zaru");
    (entry, secret_value)
}

// The corpus case, stated as a mode rather than as an intention. Both readings
// come off the filesystem after the fact, never from what the code asked for.
//
// The mutant: drop `.mode(FILE_MODE)` and the `set_permissions` call from
// `save`, and the file arrives at whatever the umask says -- 0644 here.
#[test]
fn the_file_on_disk_carries_0600_and_its_directory_0700() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("modes");
    store
        .add(entry, &mut sealer, None)
        .expect("an entry is added");

    let directory = std::fs::metadata(store.root())
        .expect("the root exists")
        .permissions()
        .mode()
        & 0o777;
    let file = std::fs::metadata(store.path())
        .expect("the file exists")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(
        directory, DIRECTORY_MODE,
        "the credential store's directory is mode {directory:o}, not {DIRECTORY_MODE:o}"
    );
    assert_eq!(
        file, FILE_MODE,
        "the credential store's file is mode {file:o}, not {FILE_MODE:o}"
    );
}

// The corpus case: a value that reaches a place it must not.
//
// One arm is the nonce this check generated; the other is the raw bytes on
// disk, read with `std::fs::read` and never through the store. Neither travels
// through the other.
//
// The staging is asserted too. A store that wrote nothing at all would satisfy
// "the secret is absent" perfectly, and that is a different defect wearing the
// same green -- Verification lessons §4.
#[test]
fn a_stored_secret_is_absent_from_the_bytes_the_store_wrote() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, secret_value) = staged_entry("ondisk");
    let alias = entry.alias().clone();
    store
        .add(entry, &mut sealer, None)
        .expect("an entry is added");

    let raw = std::fs::read(store.path()).expect("the store wrote a file");
    let text = String::from_utf8(raw).expect("the store wrote UTF-8");

    assert!(
        text.contains(alias.as_str()),
        "the file does not carry the alias that was just added, so this check asserted nothing \
         about a store that had written anything: {text}"
    );
    assert_absent(&text, &secret_value, "the file on disk");
}

// The scratch root, and the control that makes its absence mean something.
//
// Three readers, and a sibling that must survive all three. A checker that
// answers "gone" for everything passes the first three and fails the fourth,
// which is the reading the library's Credentials page calls discriminating.
#[test]
fn a_scratch_root_is_gone_after_removal_and_a_control_beside_it_survives() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("removal");
    store
        .add(entry, &mut sealer, None)
        .expect("an entry is added");

    let root = store.root().to_path_buf();
    let file = store.path();
    let control = scratch.control();
    assert!(file.exists(), "the store never wrote a file to remove");

    std::fs::remove_dir_all(&root).expect("the root is removable");

    // Reader one: the path predicate.
    assert!(!root.exists(), "the store's root is still there");
    // Reader two: enumerate the parent, which is a different question.
    let siblings: Vec<String> = std::fs::read_dir(scratch.base())
        .expect("the parent is readable")
        .map(|entry| {
            entry
                .expect("a readable directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        !siblings.iter().any(|name| name == "zaru"),
        "the parent still lists the store's root: {siblings:?}"
    );
    // Reader three: the error kind separates "gone" from "refused".
    let refused = std::fs::read(&file).expect_err("the file is gone");
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::NotFound,
        "reading the removed file failed for a reason other than its absence: {refused}"
    );
    // The control: a checker that says "gone" about everything fails here.
    assert!(
        control.exists(),
        "the control directory was removed too, so the three readings above are not about the \
         store's root in particular"
    );
    assert!(
        siblings.iter().any(|name| name == "control"),
        "the parent listing found nothing at all, so it could not have found the root either: \
         {siblings:?}"
    );
}

#[test]
fn an_apex_token_offered_with_no_confirmer_is_refused() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("apex");
    let entry = Entry::new(
        entry.alias().clone(),
        entry.description().clone(),
        entry.secret().clone(),
        Reach::Apex,
    );

    let refusal = store
        .add(entry, &mut sealer, None)
        .expect_err("an apex token with no confirmer is refused");
    let message = refusal.to_string();
    assert!(
        message.contains("no instance boundary"),
        "the refusal does not state what the token grants: {message}"
    );
    assert!(store.is_empty(), "the apex token was stored anyway");
}

#[test]
fn an_apex_token_the_user_declines_is_not_stored_and_one_they_accept_is() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("declined");
    let declining = StagedConfirmer::declining();
    let entry = Entry::new(
        base.alias().clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    );
    store
        .add(entry, &mut sealer, Some(&declining))
        .expect_err("a declined apex token is not stored");
    assert!(store.is_empty(), "a declined apex token was stored");
    assert_eq!(
        declining.told().len(),
        1,
        "the confirmer was never asked, so the decline was not the user's"
    );

    let (base, _) = staged_entry("accepted");
    let accepting = StagedConfirmer::accepting();
    let alias = base.alias().clone();
    let entry = Entry::new(
        alias.clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    );
    store
        .add(entry, &mut sealer, Some(&accepting))
        .expect("a confirmed apex token is stored");
    assert_eq!(store.len(), 1);
    assert_eq!(
        store.record(&alias).expect("it is there").reach,
        crate::credentials::store::StoredReach::Apex
    );
    // D8: the sentence the user was told is the sentence the store composed.
    assert_eq!(accepting.told().len(), 1);
    assert!(
        accepting.told()[0].contains("no instance boundary"),
        "the user was not told what an apex token grants: {:?}",
        accepting.told()
    );
}

#[test]
fn a_duplicate_alias_is_refused() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (first, _) = staged_entry("duplicate");
    let alias = first.alias().clone();
    let second = Entry::new(
        alias.clone(),
        first.description().clone(),
        first.secret().clone(),
        first.reach().clone(),
    );
    store
        .add(first, &mut sealer, None)
        .expect("the first is added");
    store
        .add(second, &mut sealer, None)
        .expect_err("the second is refused");
    assert_eq!(store.len(), 1);
}

// ADR-0014 D5's argument, one layer down: a key nothing reads might have been
// a restriction.
#[test]
fn a_key_nothing_reads_is_refused_at_load_rather_than_ignored() {
    let scratch = ScratchRoot::new();
    let root = scratch.store_root();
    std::fs::create_dir_all(&root).expect("the root is creatable");
    std::fs::write(
        root.join(STORE_FILE),
        r#"{"entries":{"work":{"description":"d","kind":"personal","reach":"apex","role":null,"tools":[],"workspace":null,"secret":"nn_mcp_smuggled"}}}"#,
    )
    .expect("the file is writable");

    let refusal = CredentialStore::open(&root).expect_err("an unknown key is refused");
    let message = refusal.to_string();
    assert!(
        message.contains("secret"),
        "the refusal does not name the key nothing reads: {message}"
    );
}

#[test]
fn what_the_store_wrote_is_what_it_reads_back() {
    let scratch = ScratchRoot::new();
    let mut sealer = InMemorySecrets::default();
    let (entry, secret_value) = staged_entry("roundtrip");
    let alias = entry.alias().clone();
    let description = entry.description().as_str().to_owned();

    {
        let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
        store
            .add(entry, &mut sealer, None)
            .expect("an entry is added");
    }

    let store = CredentialStore::open(scratch.store_root()).expect("the written root reopens");
    let record = store
        .record(&alias)
        .expect("the entry survived the round trip");
    assert_eq!(record.description, description);
    assert_eq!(record.kind, "personal");
    assert_eq!(record.tools, vec!["pages.read", "search.global"]);
    assert_eq!(record.workspace.as_deref(), Some("zaru"));
    assert_eq!(record.role, None);

    // The secret came back through the port, which is the only path it has.
    let recovered = store.secret(&alias, &sealer).expect("the port holds it");
    assert_eq!(recovered.expose_for_dispatch(), secret_value);
}
