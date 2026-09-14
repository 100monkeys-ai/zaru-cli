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
use crate::credentials::notes::bearer_for_dispatch;
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
    let secret = Secret::notes(value.clone()).expect("an nn_mcp_ value names a kind");

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
/// **Three arms, because each of the first two is blind to what the next one
/// catches, and each was added because a mutation survived the ones before
/// it.**
///
/// The first arm is the value as typed. `{:?}` on a `String` escapes a
/// combining mark to `\u{301}`, so a rendering that published every byte of a
/// nonce does not `contain` that nonce — the mutation that put a bearer value
/// into a refusal through `{:?}` survived a check with only that arm, on
/// 2026-09-04, and it is recorded on ADR-0007's Status tracking.
///
/// The second arm is the **ASCII core**, which no escaping scheme alters.
///
/// The third arm is the value **hexadecimal-encoded**, and it is this arc's
/// own surviving mutation, 2026-09-05. A cipher mutated to copy its plaintext
/// into the blob left both arms above green, because the blob is rendered as
/// hexadecimal and `nn_mcp_…` reaches the file as `6e6e5f6d63705f…`. That is
/// the 2026-09-04 finding exactly — an *encoding* hides a published value from
/// an assertion written against the value — arriving through a different
/// encoding a year's worth of reading would not have predicted. Whenever the
/// store's own on-disk representation gains another encoding, this function
/// gains another arm.
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
    let hexadecimal = crate::credentials::sealing::hex_for_checks(core.as_bytes());
    assert!(
        !rendered.contains(&hexadecimal),
        "{what} published the bearer value hexadecimal-encoded, which is how the sealed blob is \
         written; its ASCII core renders as {hexadecimal} and that is in {rendered}"
    );
}

#[test]
fn a_kind_is_read_off_the_value_and_never_stored_beside_it() {
    let personal = Secret::notes(personal_secret_nonce()).expect("nn_mcp_ names a kind");
    let app = Secret::notes(app_secret_nonce()).expect("nn_app_ names a kind");

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
            Secret::notes(&value).is_err(),
            "a value beginning {prefix:?} was taken, and ADR-0007 D2 admits only nn_mcp_ and nn_app_"
        );
    }
}

// A refusal is exactly the text that gets pasted into a report. This is the
// corpus case applied to the one refusal that is handed a bearer value.
#[test]
fn the_refusal_for_an_unknown_prefix_does_not_quote_the_value() {
    let value = format!("sk-{}", nonce("provider"));
    let refusal = Secret::notes(&value).expect_err("sk- names no kind");

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

// The conversion ADR-0007 D3 makes the composition root's job: a stored
// secret becomes the value a `zaru-notes` session authenticates with, and
// neither type will show it.
//
// The first assertion is the discriminating one and it comes first on
// purpose. Without it, a `bearer_for_dispatch` that returned
// `Bearer::new("")` would satisfy every absence assertion below perfectly --
// the check would be green about a conversion that dropped the credential.
//
// The mutants: replacing `Bearer`'s hand-written `Debug` with a derive, and
// replacing `Secret`'s.
#[test]
fn a_stored_secret_crosses_into_a_bearer_intact_and_neither_type_will_show_it() {
    let value = personal_secret_nonce();
    let secret = Secret::notes(value.clone()).expect("an nn_mcp_ value names a kind");

    let bearer = bearer_for_dispatch(&secret);
    assert_eq!(
        bearer.expose_for_dispatch(),
        value,
        "the bearer value did not cross intact, so every redaction assertion below is about a \
         value that is not there"
    );

    // Each side asserted against its own crate's constant rather than one
    // shared literal, because the two are separate declarations that could
    // diverge and a check reading only one would not notice.
    let secret_rendered = format!("{secret:?}");
    let bearer_rendered = format!("{bearer:?}");
    assert!(
        secret_rendered.contains(REDACTED),
        "a secret's Debug printed {secret_rendered:?}, which carries no redaction marker at all"
    );
    assert!(
        bearer_rendered.contains(zaru_notes::session::REDACTED),
        "a bearer's Debug printed {bearer_rendered:?}, which carries no redaction marker at all"
    );
    assert_absent(&secret_rendered, &value, "a secret's Debug");
    assert_absent(&bearer_rendered, &value, "a bearer's Debug");
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
use crate::credentials::family::Family;
use crate::credentials::fixtures::{ScratchRoot, StagedConfirmer};
use crate::credentials::sealing::blob::Sealed;
use crate::credentials::sealing::fixtures::{NoKeyAnywhere, StagedKey};
use crate::credentials::sealing::key::CREDENTIAL_KEY_VARIABLE;
use crate::credentials::sealing::key::SealingKey;
use crate::credentials::store::{CredentialStore, DIRECTORY_MODE, FILE_MODE, STORE_FILE};
use std::os::unix::fs::PermissionsExt;

/// An ordinary instance-locked entry carrying a fresh nonce for a secret.
fn staged_entry(label: &str) -> (Entry, String) {
    let secret_value = personal_secret_nonce();
    let entry = Entry::notes(
        Alias::new(&nonce(label)).expect("a nonce is a legal alias"),
        Description::new(format!("{label}, {}", nonce("purpose"))).expect("one line"),
        Secret::notes(secret_value.clone()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
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
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("modes");
    store.add(entry, &keys, None).expect("an entry is added");

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
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, secret_value) = staged_entry("ondisk");
    let alias = entry.alias().clone();
    store.add(entry, &keys, None).expect("an entry is added");

    let raw = std::fs::read(store.path()).expect("the store wrote a file");
    let text = String::from_utf8(raw).expect("the store wrote UTF-8");

    assert!(
        text.contains(alias.as_str()),
        "the file does not carry the alias that was just added, so this check asserted nothing \
         about a store that had written anything: {text}"
    );
    assert_absent(&text, &secret_value, "the file on disk");
}

// ADR-0007 clause 4's at-rest half, at the unit level: the file carries a
// sealed value, and it is the value that was put in.
//
// **Both arms matter and the second is the one that is new.** Absence alone was
// all this check could assert while sealing was a port with no implementation:
// a store that wrote nothing satisfied it perfectly. Now the ciphertext is
// there to be opened, so the check opens it -- with the key it staged, through
// `Sealed::open` rather than through `CredentialStore::secret`, so the arm that
// says "the right value is in there" does not travel back through the store
// whose file is under test (Verification lessons §11).
//
// The whole-file evidence, read by a caller that is not the store at all, is
// `tests/sealing_from_outside.rs`.
#[test]
fn what_the_file_carries_is_the_sealed_value_and_it_opens_to_what_was_put_in() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, secret_value) = staged_entry("sealed");
    let alias = entry.alias().clone();
    store.add(entry, &keys, None).expect("an entry is added");

    let record = store.record(&alias).expect("the record is there");
    let opened = record
        .sealed
        .open(keys.key(), &alias, Family::Notes)
        .expect("the blob opens under the key it was sealed with");
    assert_eq!(
        opened.expose_for_dispatch(),
        secret_value,
        "the blob on disk does not open to the value that was stored"
    );

    // And the file itself carries that same blob rather than a second copy of
    // it kept only in memory.
    let text = std::fs::read_to_string(store.path()).expect("the store wrote a file");
    assert!(
        text.contains(&record.sealed.as_hex()),
        "the sealed value the store holds is not in the file it wrote: {text}"
    );
}

// A store file written before `sealed` existed does not parse, and says which
// file.
//
// The harness is pre-alpha and carries no migration, so this is the behaviour
// rather than a gap in it: a record with no sealed value is a record with no
// secret, and reading one as though it were complete would be worse than
// refusing. `deny_unknown_fields` already refuses the other direction.
#[test]
fn a_store_file_from_before_sealing_is_refused_naming_the_file() {
    let scratch = ScratchRoot::new();
    let root = scratch.store_root();
    std::fs::create_dir_all(&root).expect("the root is made");
    let path = root.join(STORE_FILE);
    std::fs::write(
        &path,
        r#"{"entries":{"work":{"description":"a token","kind":"personal",
           "reach":{"instance_locked":"cortex.page"},"role":null,"tools":[],"workspace":null}}}"#,
    )
    .expect("the staged file is written");

    let refusal = CredentialStore::open(&root)
        .expect_err("a record with no sealed value was read as though it had one");
    let said = refusal.to_string();
    assert!(
        said.contains(&path.display().to_string()),
        "the refusal does not name the file, so the reader cannot find it: {said}"
    );
    assert!(
        said.contains("sealed"),
        "the refusal does not name the missing field: {said}"
    );
}

// The store's file is replaced whole or not at all.
//
// This matters more here than it did for the checkpoint this discipline was
// lifted from. A torn checkpoint costs a turn; a torn credential store is every
// credential the user has, because the file is now the only copy of every
// sealed value. Until 2026-09-05 `save` truncated the live file and then filled
// it, so a reader landing between the two saw an empty file.
//
// The shape is the session arc's `no_reader_ever_sees_a_partly_rewritten_
// checkpoint`, deliberately: one reader thread, many rewrites, and the reader's
// own count asserted first so the check cannot pass vacuously (Verification
// lessons §4).
#[test]
fn no_reader_ever_sees_a_partly_written_credential_store() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    // Long enough that a truncate-then-fill has a window a reader can land in.
    for index in 0..40 {
        let (entry, _) = staged_entry(&format!("bulk-{index}"));
        let entry = entry.with_tools(ToolScope::new(
            (0..40).map(|tool| format!("pages.read.{index}.{tool}")),
        ));
        store.add(entry, &keys, None).expect("an entry is added");
    }
    let path = store.path();
    let staged = std::fs::metadata(&path)
        .expect("the store wrote a file")
        .len();
    assert!(
        staged > 40_000,
        "the staged store is only {staged} bytes, which may be one write on this filesystem, so \
         a reader could not land inside a rewrite even if one were torn"
    );

    const REWRITES: u32 = 200;
    let reading_path = path.clone();
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let readers_flag = std::sync::Arc::clone(&done);
    let reader = std::thread::spawn(move || {
        let mut reads = 0u64;
        let mut torn = Vec::new();
        while !readers_flag.load(std::sync::atomic::Ordering::Relaxed) {
            match std::fs::read(&reading_path) {
                Ok(bytes) => {
                    reads += 1;
                    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
                        torn.push(bytes.len());
                        if torn.len() > 8 {
                            break;
                        }
                    }
                }
                Err(error) => torn.push(usize::MAX - error.raw_os_error().unwrap_or(0) as usize),
            }
        }
        (reads, torn)
    });

    for _ in 0..REWRITES {
        store.save().expect("the store rewrites");
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let (reads, torn) = reader.join().expect("the reader thread panicked");

    assert!(
        reads > 10,
        "the reader only completed {reads} reads, so this check asserted nothing about the \
         {REWRITES} rewrites beside it"
    );
    assert!(
        torn.is_empty(),
        "a reader saw {} credential store(s) that were not whole documents, at these byte \
         lengths: {torn:?}. The file is replaced through a renamed sibling precisely so that a \
         reader sees the whole previous store or the whole new one",
        torn.len()
    );
    assert!(
        !crate::atomic::temporary_path(&path).exists(),
        "the sibling temporary was left behind, so a later reader could mistake it for state"
    );
}

// A machine with no key anywhere refuses the add, and writes nothing.
//
// The refusal is the whole of what ADR-0007 D3 offers such a machine, so it has
// to name both sources -- and the store must not be left with an entry whose
// secret was never sealed.
#[test]
fn an_add_with_no_key_anywhere_is_refused_and_writes_no_entry() {
    let scratch = ScratchRoot::new();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("nokey");
    let alias = entry.alias().clone();

    let refusal = store
        .add(entry, &NoKeyAnywhere, None)
        .expect_err("a token was stored on a machine with no sealing key");
    let said = refusal.to_string();
    assert!(
        said.contains(CREDENTIAL_KEY_VARIABLE),
        "the refusal does not name the environment variable: {said}"
    );
    assert!(
        said.contains("keyring"),
        "the refusal does not name the OS keyring: {said}"
    );
    assert!(
        store.record(&alias).is_none(),
        "the entry was kept even though its secret was never sealed"
    );
    assert!(
        !store.path().exists(),
        "the store wrote a file for an entry it refused"
    );
}

// The scratch root, and the control that makes its absence mean something.
//
// Three readers, and a sibling that must survive all three. A checker that
// answers "gone" for everything passes the first three and fails the fourth,
// which is the reading the library's Credentials page calls discriminating.
#[test]
fn a_scratch_root_is_gone_after_removal_and_a_control_beside_it_survives() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("removal");
    store.add(entry, &keys, None).expect("an entry is added");

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
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (entry, _) = staged_entry("apex");
    let entry = Entry::notes(
        entry.alias().clone(),
        entry.description().clone(),
        entry.secret().clone(),
        Reach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry");

    let refusal = store
        .add(entry, &keys, None)
        .expect_err("an apex token with no confirmer is refused");
    let message = refusal.to_string();
    assert!(
        message.contains("no instance boundary"),
        "the refusal does not state what the token grants: {message}"
    );
    assert!(store.is_empty(), "the apex token was stored anyway");
}

/// The defect the order in `notes tokens add` prevents, made visible.
///
/// ADR-0007 D8 requires the apex confirmation to state "what it grants", and
/// `apex_grants` composes that sentence out of the entry's own tool scope. An
/// entry built and stored **before** `tools/list` was read carries
/// `ToolScope::default()`, so the sentence the user is asked to accept says the
/// credential grants nothing — which is the least alarming possible wording for
/// the most dangerous possible credential.
///
/// Two entries, identical but for `with_tools`, and the two sentences differ.
/// That is why `cli::run::notes_tokens_add` reaches the instance and reads its
/// scope before an entry exists at all.
#[test]
fn an_apex_confirmation_states_the_scope_the_entry_carries_and_zero_when_it_carries_none() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("unread");
    let unread = Entry::notes(
        base.alias().clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry");
    let before = store
        .add(unread, &keys, None)
        .expect_err("an apex token with no confirmer is refused")
        .to_string();

    let (base, _) = staged_entry("read");
    let read = Entry::notes(
        base.alias().clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::new(vec![
        "pages.read".to_owned(),
        "pages.apply_patch".to_owned(),
        "workspaces.create".to_owned(),
    ]));
    let after = store
        .add(read, &keys, None)
        .expect_err("an apex token with no confirmer is refused")
        .to_string();

    assert!(
        before.contains("grants 0 tool(s)"),
        "an entry stored before its scope was read says it grants nothing: {before}"
    );
    assert!(
        after.contains("grants 3 tool(s)"),
        "an entry stored after its scope was read says what it really grants: {after}"
    );
    assert!(store.is_empty(), "neither apex token was stored");
}

#[test]
fn an_apex_token_the_user_declines_is_not_stored_and_one_they_accept_is() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("declined");
    let declining = StagedConfirmer::declining();
    let entry = Entry::notes(
        base.alias().clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry");
    store
        .add(entry, &keys, Some(&declining))
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
    let entry = Entry::notes(
        alias.clone(),
        base.description().clone(),
        base.secret().clone(),
        Reach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry");
    store
        .add(entry, &keys, Some(&accepting))
        .expect("a confirmed apex token is stored");
    assert_eq!(store.len(), 1);
    assert_eq!(
        store.record(&alias).expect("it is there").reach(),
        Some(&crate::credentials::store::StoredReach::Apex)
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
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let (first, _) = staged_entry("duplicate");
    let alias = first.alias().clone();
    let second = Entry::notes(
        alias.clone(),
        first.description().clone(),
        first.secret().clone(),
        first.reach().expect("a Notes entry has a reach").clone(),
    )
    .expect("an nn_ value builds a Nuclear Notes entry");
    store.add(first, &keys, None).expect("the first is added");
    store
        .add(second, &keys, None)
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
        // The smuggled key is written **before** `held`, because
        // `deny_unknown_fields` refuses at the key it does not recognise and
        // this check is about that refusal rather than about the rest of the
        // record parsing. A plaintext `secret` beside a sealed one is the
        // exact shape ADR-0007 D3 must never accept quietly.
        r#"{"entries":{"work":{"description":"d","secret":"nn_mcp_smuggled","held":{"notes":{"kind":"personal","reach":"apex","role":null,"tools":[],"workspace":null}}}}}"#,
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
    let keys = StagedKey::minted();
    let (entry, secret_value) = staged_entry("roundtrip");
    let alias = entry.alias().clone();
    let description = entry.description().as_str().to_owned();

    {
        let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
        store.add(entry, &keys, None).expect("an entry is added");
    }

    let store = CredentialStore::open(scratch.store_root()).expect("the written root reopens");
    let record = store
        .record(&alias)
        .expect("the entry survived the round trip");
    assert_eq!(record.description, description);
    assert_eq!(record.kind(), "personal");
    assert_eq!(record.tools(), vec!["pages.read", "search.global"]);
    assert_eq!(record.workspace(), Some("zaru"));
    assert_eq!(record.role(), None);

    // The secret came back through the port, which is the only path it has.
    let recovered = store.secret(&alias, &keys).expect("the port holds it");
    assert_eq!(recovered.expose_for_dispatch(), secret_value);
}

// ---------------------------------------------------------------------------
// The composer role, and what the agent may see.
// ---------------------------------------------------------------------------

use crate::credentials::entry::Reach as EntryReach;
use crate::credentials::projection::{NAMESPACE_PREFIX, Namespace};
use crate::credentials::store::StoreError;

/// Stages a store holding one composer-scoped token and one agent token.
fn staged_pair() -> (ScratchRoot, StagedKey, CredentialStore, Alias, Alias) {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (composer, _) = staged_entry("composer");
    let composer_alias = composer.alias().clone();
    let composer = Entry::notes(
        composer_alias.clone(),
        composer.description().clone(),
        composer.secret().clone(),
        composer.reach().expect("a Notes entry has a reach").clone(),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::new(["pages.read", "search.global"]));

    let (agent, _) = staged_entry("agent");
    let agent_alias = agent.alias().clone();

    store.add(composer, &keys, None).expect("composer added");
    store.add(agent, &keys, None).expect("agent added");
    store
        .grant_composer_role(&composer_alias)
        .expect("a read-only scope may hold the role");

    (scratch, keys, store, composer_alias, agent_alias)
}

// The corpus case: a token that escapes its context.
//
// ADR-0007 D4: "The agent may use any token not flagged composer, and never
// the composer's." The composer's entry never enters the projection, so this
// asserts an absence from a list the check reads rather than a filter the
// store reports about itself.
//
// The mutant: drop the `filter` in `agent_namespaces`.
#[test]
fn a_composer_role_token_never_appears_in_the_agents_namespace_list() {
    let (_scratch, _keys, store, composer_alias, agent_alias) = staged_pair();

    let namespaces = store.agent_namespaces();
    let names: Vec<&str> = namespaces.iter().map(|ns| ns.name.as_str()).collect();

    assert_eq!(
        names,
        vec![format!("{NAMESPACE_PREFIX}:{agent_alias}")],
        "the agent's namespace list is not exactly the non-composer tokens"
    );
    assert!(
        !names
            .iter()
            .any(|name| name.contains(composer_alias.as_str())),
        "the composer's alias {composer_alias:?} reached the agent: {names:?}"
    );
    // The staging: a projection of nothing would satisfy the absence above.
    assert_eq!(
        store.len(),
        2,
        "the store does not hold both tokens, so this check asserted nothing"
    );
}

// ADR-0007 D3: "The agent sees aliases, descriptions, and tool lists. It never
// sees a secret value."
//
// The destructure is the mechanism. A fourth field on `Namespace` -- a secret
// among them -- stops this check compiling rather than travelling unnoticed,
// which is the same signal the composer's exhaustive `Scope` match uses.
#[test]
fn what_the_agent_sees_is_three_fields_and_a_fourth_would_not_compile() {
    let (_scratch, _keys, store, _composer_alias, _agent_alias) = staged_pair();
    let namespaces = store.agent_namespaces();
    let projected = namespaces.first().expect("one agent token is projected");

    let Namespace {
        name,
        description,
        tools,
    } = projected;

    assert!(name.starts_with(&format!("{NAMESPACE_PREFIX}:")));
    assert!(!description.is_empty());
    assert_eq!(
        tools,
        &vec!["pages.read".to_owned(), "search.global".to_owned()]
    );
}

// ADR-0007 D4: "Exactly one token is flagged composer... A token cannot be
// both. The store refuses the configuration."
//
// The mutant: drop the `composer()` lookup from `grant_composer_role`.
#[test]
fn a_second_composer_role_is_refused_naming_both_aliases() {
    let (_scratch, _keys, mut store, composer_alias, agent_alias) = staged_pair();

    let refusal = store
        .grant_composer_role(&agent_alias)
        .expect_err("a second composer role is refused");

    match &refusal {
        StoreError::SecondComposerRole { existing, offered } => {
            assert_eq!(existing, &composer_alias);
            assert_eq!(offered, &agent_alias);
        }
        other => panic!("a second composer role was refused for the wrong reason: {other}"),
    }
    let message = refusal.to_string();
    assert!(
        message.contains(composer_alias.as_str()) && message.contains(agent_alias.as_str()),
        "the refusal names only one of the two aliases: {message}"
    );
    // And the first token still holds it -- a refusal that also demoted the
    // incumbent would leave the store with none.
    assert_eq!(
        store.composer().expect("the role is still held").0,
        &composer_alias
    );
}

// ADR-0005's trigger clause 9, in the local form this arc can hold: "The
// composer's own credential cannot write, asserted against the credential
// store rather than against the code that uses it."
//
// The permitted set is ADR-0006 D4's, transcribed. Deciding for oneself which
// tool names are writes would be authoring a security vocabulary, which is on
// the human side of the boundary; copying a record's list is not.
//
// The mutant: drop the `outside_composer_scope` check from
// `grant_composer_role`.
#[test]
fn the_composer_role_is_refused_when_the_cached_scope_leaves_adr_0006_d4s_set() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("writer");
    let alias = base.alias().clone();
    let entry = Entry::notes(
        alias.clone(),
        base.description().clone(),
        base.secret().clone(),
        base.reach().expect("a Notes entry has a reach").clone(),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::new([
        "pages.read",
        "search.global",
        "pages.apply_patch",
    ]));
    store.add(entry, &keys, None).expect("it is stored");

    let refusal = store
        .grant_composer_role(&alias)
        .expect_err("a scope reaching outside D4's set cannot hold the role");
    match &refusal {
        StoreError::ComposerScopeExceeded { alias: named, tool } => {
            assert_eq!(named, &alias);
            assert_eq!(
                tool, "pages.apply_patch",
                "the refusal names the wrong tool"
            );
        }
        other => panic!("the role was refused for the wrong reason: {other}"),
    }
    assert!(
        store.composer().is_none(),
        "the role was granted despite the refusal"
    );
    assert!(
        refusal.to_string().contains("pages.apply_patch"),
        "the refusal does not name the tool that caused it: {refusal}"
    );
}

// The counterpart, so that the refusal above is not simply "refuse always".
#[test]
fn every_tool_adr_0006_d4_names_may_hold_the_composer_role() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("readonly");
    let alias = base.alias().clone();
    let entry = Entry::notes(
        alias.clone(),
        base.description().clone(),
        base.secret().clone(),
        base.reach().expect("a Notes entry has a reach").clone(),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::new(
        crate::credentials::entry::COMPOSER_SCOPE.iter().copied(),
    ));
    store.add(entry, &keys, None).expect("it is stored");
    store
        .grant_composer_role(&alias)
        .expect("D4's own set may hold the role");
    assert_eq!(store.composer().expect("granted").0, &alias);
}

/// The two names the record had spelled differently from the substrate.
///
/// **The check above cannot hold this and it is worth saying why**: it builds
/// its scope *from* `COMPOSER_SCOPE`, so it passes whatever the nine strings
/// are. This one names the two corrected spellings as literals, so reverting
/// the constant to the record's original `kg.related` and
/// `discovery.entities` reddens here rather than passing silently.
///
/// Both arms matter. Asserting only the presence of the new spellings would
/// pass a constant carrying **both**, which would widen the permitted set by
/// two names nobody decided on.
#[test]
fn the_two_names_adr_0006_d4_had_wrong_are_the_substrates_spellings() {
    use crate::credentials::entry::COMPOSER_SCOPE;

    for corrected in ["kg.get_related", "discovery.list_entities"] {
        assert!(
            COMPOSER_SCOPE.contains(&corrected),
            "the composer scope does not carry {corrected:?}, which is how the live tool surface \
             spells it -- read from tools/list on 2026-09-06 and again on 2026-09-14"
        );
    }
    for superseded in ["kg.related", "discovery.entities"] {
        assert!(
            !COMPOSER_SCOPE.contains(&superseded),
            "the composer scope still carries {superseded:?}, which no tool on the surface is \
             called; carrying both spellings would widen the permitted set by a name nobody decided"
        );
    }
    assert_eq!(
        COMPOSER_SCOPE.len(),
        9,
        "the correction changed two spellings and must not have changed the count"
    );
}

// ADR-0007 D8: an apex token is "marked wherever the token appears", and one
// of the three places is "the description the agent reads".
#[test]
fn an_apex_token_is_marked_in_the_description_the_agent_reads() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");

    let (base, _) = staged_entry("apexmark");
    let entry = Entry::notes(
        base.alias().clone(),
        base.description().clone(),
        base.secret().clone(),
        EntryReach::Apex,
    )
    .expect("an nn_ value builds a Nuclear Notes entry");
    let confirmer = StagedConfirmer::accepting();
    store
        .add(entry, &keys, Some(&confirmer))
        .expect("a confirmed apex token is stored");

    let namespaces = store.agent_namespaces();
    let projected = namespaces.first().expect("it is projected");
    assert!(
        projected.description.contains(EntryReach::APEX_MARKING),
        "the agent's description does not mark an apex token: {}",
        projected.description
    );
    // And an instance-locked one is not marked, so the marking means something.
    let (locked, _) = staged_entry("lockedmark");
    store.add(locked, &keys, None).expect("it is stored");
    let unmarked = store
        .agent_namespaces()
        .into_iter()
        .find(|ns| !ns.description.contains(EntryReach::APEX_MARKING));
    assert!(
        unmarked.is_some(),
        "every token is marked apex, so the marking distinguishes nothing"
    );
}

// ---------------------------------------------------------------------------
// ADR-0007 D6's cache: the half `zaru-cli` owns.
//
// The three signals are `zaru-notes`' to produce and each is asserted there.
// What is asserted here is everything about the cache that needs no session;
// the consumption of each signal is driven end to end in
// `tests/notes_scope_from_outside.rs`, because constructing a `Session` at all
// means implementing `Endpoint`, whose associated types are `rmcp`'s.
// ---------------------------------------------------------------------------

use crate::credentials::notes::Cached;
use crate::credentials::store::{Record, StoredHeld, StoredReach};

// D6 gives the TTL no number and this workspace refuses to invent one: it
// arrives as a caller-validated `Ttl` and `Cached::expired` is the only place
// it is unwrapped.
//
// The mutant: substitute a literal for `ttl.get()`. The check owns the window
// and the readings, so no literal can equal both the boundary below and the
// window the assertion reads back.
#[test]
fn the_ttl_backstop_uses_the_window_the_store_validated_and_nothing_else() {
    let cached = Cached {
        scope: ToolScope::new(["pages.read"]),
        at: Duration::from_secs(100),
    };
    let ttl = Ttl::new(Duration::from_secs(300)).expect("a non-zero window");

    assert_eq!(
        cached.expired(Duration::from_secs(399), ttl),
        None,
        "one second inside the window is inside the window"
    );

    let fired = cached
        .expired(Duration::from_secs(400), ttl)
        .expect("the window has elapsed exactly");
    assert_eq!(
        fired,
        zaru_notes::session::Invalidation::Expired {
            window: Duration::from_secs(300),
            elapsed: Duration::from_secs(300),
        },
        "the signal must carry the window the caller validated and the elapsed time measured \
         against it"
    );

    // A caller's clock is a caller's business, and a reading before the cache
    // was taken is not a reason to abort a program.
    assert_eq!(cached.expired(Duration::from_secs(50), ttl), None);
}

// The pin: a `Cached` reading is a monotonic offset from wherever the caller's
// clock started, so it is meaningless in any other process and must never be
// written to a file that outlives the run that took it.
//
// This destructures exhaustively rather than counting, so an eighth field on
// `Record` -- an `at`, a `cached_at`, an `age` -- stops this check compiling
// rather than travelling to disk. It did exactly that when the sealing arc
// added the seventh: "error[E0027]: pattern does not mention field `sealed`", and it did it
// again on 2026-09-05 when six of those seven moved into `StoredHeld::Notes`. Same signal as
// `what_the_agent_sees_is_three_fields_and_a_fourth_would_not_compile`.
#[test]
fn a_records_fields_are_adr_0007_d2s_and_a_clock_reading_is_not_among_them() {
    let record = Record {
        description: nonce("description"),
        held: StoredHeld::Notes {
            kind: "personal".to_owned(),
            reach: StoredReach::Apex,
            role: None,
            tools: vec!["pages.read".to_owned()],
            workspace: None,
        },
        sealed: Sealed::seal(
            &SealingKey::mint(),
            &Alias::new("fields").expect("a well-formed alias"),
            &Secret::notes(personal_secret_nonce()).expect("the fixture value has a kind"),
        )
        .expect("a bearer value seals"),
    };

    let Record {
        description,
        held,
        sealed,
    } = record;

    assert!(!description.is_empty());
    // The Notes half is destructured exhaustively too, so a sixth field on
    // that variant is caught by the same mechanism as a fourth field on the
    // record itself.
    let StoredHeld::Notes {
        kind,
        reach,
        role,
        tools,
        workspace,
    } = held
    else {
        panic!("the record above was built as a Notes record");
    };
    assert_eq!(kind, "personal");
    assert_eq!(reach, StoredReach::Apex);
    assert_eq!(role, None);
    assert_eq!(tools, vec!["pages.read".to_owned()]);
    assert_eq!(workspace, None);
    // The seventh field is the one this arc added, and it is not optional: a
    // record without a sealed value does not exist as a type.
    assert!(
        sealed.len() > 28,
        "a sealed value shorter than a version, a nonce and a tag is not one"
    );
}

// D6's write-through is what makes D5 and D6 one read rather than two: the
// projection to the agent is built from the same field the refresh replaces.
//
// The before-arm is what makes this discriminate. Without it, a projection
// that had always carried the new tool would pass.
//
// The mutant: make `replace_tools` return `self.save()` without assigning.
#[test]
fn a_replaced_scope_reaches_the_agents_namespace_projection_in_the_same_read() {
    let (scratch, _keys, mut store, _composer_alias, agent_alias) = staged_pair();

    let before: Vec<String> = store
        .record(&agent_alias)
        .expect("the agent token is stored")
        .tools()
        .to_vec();
    assert!(
        !before.contains(&"kg.list_cross_links".to_owned()),
        "the tool this check is about was already cached, so the assertion below asserts nothing: \
         {before:?}"
    );

    let refreshed = ToolScope::new(["pages.read", "search.global", "kg.list_cross_links"]);
    store
        .replace_tools(&agent_alias, &refreshed)
        .expect("a stored alias takes a scope");

    // Read back through the projection, which is the consumer D5 names, and
    // through a store reopened from the bytes on disk, which is a second
    // reader that does not share this store's in-memory map.
    let projected: Vec<String> = store
        .agent_namespaces()
        .into_iter()
        .find(|ns| ns.name == format!("{NAMESPACE_PREFIX}:{agent_alias}"))
        .expect("the agent token projects a namespace")
        .tools;
    assert_eq!(
        projected,
        vec![
            "pages.read".to_owned(),
            "search.global".to_owned(),
            "kg.list_cross_links".to_owned(),
        ],
        "the refreshed scope did not reach the agent's namespace"
    );

    let reopened = CredentialStore::open(scratch.store_root()).expect("the written root reopens");
    assert_eq!(
        reopened
            .record(&agent_alias)
            .expect("the entry survived")
            .tools(),
        projected,
        "the refreshed scope was not written through to the file"
    );
}

#[test]
fn a_scope_offered_for_an_unknown_alias_is_refused_rather_than_creating_one() {
    let (_scratch, _keys, mut store, _composer_alias, _agent_alias) = staged_pair();
    let stranger = Alias::new("stranger").expect("an ordinary alias");
    let before = store.len();

    let refusal = store
        .replace_tools(&stranger, &ToolScope::new(["pages.read"]))
        .expect_err("nothing answers to that alias");
    assert!(
        matches!(refusal, StoreError::UnknownAlias { .. }),
        "an unknown alias must be refused rather than silently created: {refusal:?}"
    );
    assert_eq!(
        store.len(),
        before,
        "the refusal added an entry, which is the failure it exists to prevent"
    );
}

// ---------------------------------------------------------------------------
// ADR-0007 D2's second family, added 2026-09-05 when the open question
// "whether the credential store holds provider credentials, or only Nuclear
// Notes tokens" closed in favour of one store.
// ---------------------------------------------------------------------------

use crate::credentials::fixtures::provider_secret_nonce;
use crate::credentials::secret::SecretRefused;
use crate::providers::ProviderKind;

/// A provider entry on a scratch store, and the value it holds.
fn staged_provider_entry(kind: ProviderKind) -> (Entry, String) {
    let value = provider_secret_nonce();
    let entry = Entry::provider(
        Alias::new(&format!("provider.{}", kind.key_segment())).expect("a well-formed alias"),
        Description::new(format!("the {kind} test key")).expect("a one-line description"),
        Secret::provider(kind, value.clone()).expect("a well-formed provider key"),
    )
    .expect("a provider secret builds a provider entry");
    (entry, value)
}

// The whole of the amended D2's prefix clause, as a check rather than as
// prose: the kind is what the caller declared, and no byte of the value
// contributed to it. A `Secret::provider` that fell back to sniffing a prefix
// would have to answer `Gemini` for a value with no prefix at all, which is
// what the second half asserts.
#[test]
fn a_provider_keys_kind_is_declared_and_no_prefix_rule_is_consulted() {
    for kind in ProviderKind::ALL {
        let value = provider_secret_nonce();
        let secret = Secret::provider(kind, value.clone()).expect("a well-formed provider key");
        assert_eq!(
            secret.kind(),
            Kind::Provider(kind),
            "the declared kind did not survive construction"
        );
        assert_eq!(secret.kind().as_str(), kind.as_str());
        assert!(
            !secret.kind().is_notes(),
            "a provider key answered that it is a Nuclear Notes token"
        );
        assert_eq!(secret.expose_for_dispatch(), value);
    }

    // The same bytes, under two different declared kinds, are two different
    // kinds. Nothing about the value decides.
    let shared = provider_secret_nonce();
    let one = Secret::provider(ProviderKind::Gemini, shared.clone()).expect("well-formed");
    let two = Secret::provider(ProviderKind::Anthropic, shared).expect("well-formed");
    assert_ne!(one.kind(), two.kind());

    // And a Nuclear Notes value offered as a provider key is taken as one:
    // there is no prefix check on this path at all, which is the point. What
    // stops the two blurring is `Entry`, checked below.
    let notes_shaped = personal_secret_nonce();
    assert_eq!(
        Secret::provider(ProviderKind::Gemini, notes_shaped)
            .expect("a provider key is not prefix-checked")
            .kind(),
        Kind::Provider(ProviderKind::Gemini)
    );
}

// The security corpus grows here. A provider key is refused exactly the
// shapes a header and a listing cannot carry, and **the refusal carries no
// part of the value** -- not verbatim, not escaped, not by length.
//
// The second clause turned out to be held by the compiler rather than by the
// assertions, and that is worth recording because it is stronger. The
// mutation written for it -- give `SecretRefused::Control` an `offered:
// String` rendered through `{:?}`, which is the exact shape ADR-0007's Status
// tracking records surviving once already -- **does not build**:
// "error[E0204]: the trait `Copy` cannot be implemented for this type". A
// `Copy` enum cannot carry a `String`, so no variant of this type can hold a
// value at all. `secret_refusals_cannot_carry_a_value` below is what keeps
// that mechanism from being removed silently, since deleting the derive would
// make the mutation compile again and nothing else would notice.
//
// So the mutation actually watched here is the one that does build: deleting
// the control-character arm, which reddens this check at "the shape is
// refused".
#[test]
fn a_provider_key_is_refused_what_a_header_cannot_carry_and_the_refusal_quotes_nothing() {
    let core = ascii_core(&provider_secret_nonce()).to_owned();
    let cases: [(String, SecretRefused); 4] = [
        (String::new(), SecretRefused::Empty),
        (format!("{core}\u{1b}[2K"), SecretRefused::Control),
        (format!("{core}\nX-Injected: 1"), SecretRefused::Control),
        (format!(" {core} "), SecretRefused::SurroundingWhitespace),
    ];

    for (offered, expected) in cases {
        let refusal = Secret::provider(ProviderKind::Gemini, offered.clone())
            .expect_err("the shape is refused");
        assert_eq!(refusal, expected, "the wrong reason was given");

        let rendered = format!("{refusal} {refusal:?}");
        assert!(
            !rendered.contains(&offered) || offered.is_empty(),
            "the refusal published the value it was handed"
        );
        assert!(
            !rendered.contains(&core),
            "the refusal published the value's ASCII core, which no escaping scheme alters"
        );
    }

    // And a well-formed one is taken, so the gate above is not refusing
    // everything -- the discriminating arm.
    assert!(Secret::provider(ProviderKind::Gemini, provider_secret_nonce()).is_ok());
}

// The two halves are not interchangeable, and the constructor is what says
// so. Both directions, because a gate that refused everything would pass a
// one-directional check.
#[test]
fn an_entry_is_refused_when_its_secret_belongs_to_the_other_family() {
    let alias = Alias::new("mismatch").expect("a well-formed alias");
    let description = Description::new("either way").expect("a one-line description");

    let refusal = Entry::provider(
        alias.clone(),
        description.clone(),
        Secret::notes(personal_secret_nonce()).expect("nn_mcp_ names a kind"),
    )
    .expect_err("a Notes token is not a provider key");
    assert_eq!(refusal.wanted, "provider");
    assert_eq!(refusal.found, "Nuclear Notes");
    assert!(refusal.to_string().contains("mismatch"));

    let refusal = Entry::notes(
        alias.clone(),
        description.clone(),
        Secret::provider(ProviderKind::Gemini, provider_secret_nonce()).expect("well-formed"),
        Reach::Apex,
    )
    .expect_err("a provider key is not a Notes token");
    assert_eq!(refusal.wanted, "Nuclear Notes");
    assert_eq!(refusal.found, "provider");

    // Both constructors succeed on their own family, so neither gate is
    // refusing everything.
    assert!(
        Entry::notes(
            alias.clone(),
            description.clone(),
            Secret::notes(personal_secret_nonce()).expect("nn_mcp_ names a kind"),
            Reach::Apex,
        )
        .is_ok()
    );
    assert!(
        Entry::provider(
            alias,
            description,
            Secret::provider(ProviderKind::Gemini, provider_secret_nonce()).expect("well-formed"),
        )
        .is_ok()
    );
}

// D3: "The agent sees aliases, descriptions, and tool lists. It never sees a
// secret value." A provider key is not projected at all -- not as an empty
// namespace, not under a `notes:` name. The Notes token beside it is the
// discriminating arm: a projection that returned nothing would pass the
// absence assertion on its own.
#[test]
fn a_provider_key_is_never_projected_to_the_agent() {
    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("the store opens");

    let (notes, _) = staged_entry("agent");
    let notes_alias = notes.alias().clone();
    store
        .add(notes, &keys, None)
        .expect("the Notes token is added");

    let (provider, provider_value) = staged_provider_entry(ProviderKind::Gemini);
    let provider_alias = provider.alias().clone();
    store
        .add(provider, &keys, None)
        .expect("the provider key is added");

    let namespaces = store.agent_namespaces();
    assert_eq!(
        namespaces.len(),
        1,
        "the agent was offered {} namespaces over a store of two credentials",
        namespaces.len()
    );
    assert_eq!(namespaces[0].name, format!("notes:{notes_alias}"));

    let rendered = format!("{namespaces:?}");
    assert!(
        !rendered.contains(provider_alias.as_str()),
        "the provider key's alias reached the agent's namespace list"
    );
    assert!(
        !rendered.contains(&provider_value),
        "a provider key's value reached the agent"
    );
    assert!(
        !rendered.contains(ascii_core(&provider_value)),
        "a provider key's ASCII core reached the agent"
    );
}

// The seal binds the family as well as the alias. A record claiming to hold a
// Notes token over a provider key's bytes does not open, which is what stops
// a hand-edited `held` turning a provider key into a Notes token that the
// composer role could then be granted to.
#[test]
fn a_sealed_value_does_not_open_under_the_wrong_family() {
    let key = SealingKey::mint();
    let alias = Alias::new("family").expect("a well-formed alias");
    let value = provider_secret_nonce();
    let sealed = Sealed::seal(
        &key,
        &alias,
        &Secret::provider(ProviderKind::Gemini, value.clone()).expect("well-formed"),
    )
    .expect("a provider key seals");

    // Under its own family it opens and the kind comes back.
    let opened = sealed
        .open(&key, &alias, Family::Provider(ProviderKind::Gemini))
        .expect("it opens under the family it was sealed for");
    assert_eq!(opened.kind(), Kind::Provider(ProviderKind::Gemini));
    assert_eq!(opened.expose_for_dispatch(), value);

    // Claimed as Nuclear Notes, the prefix rule refuses it and no `Secret`
    // exists. The refusal carries nothing.
    let refusal = sealed
        .open(&key, &alias, Family::Notes)
        .expect_err("a provider key is not a Notes token however the record is edited");
    let rendered = format!("{refusal} {refusal:?}");
    assert!(!rendered.contains(&value));
    assert!(!rendered.contains(ascii_core(&value)));

    // And the kind is carried, not guessed: opened under a *different*
    // provider kind the same bytes come back under that kind, because there
    // is nothing in them to contradict it. That is the honest consequence of
    // not inventing a prefix rule, and it is recorded rather than hidden --
    // what stops a key being used against the wrong provider is that the
    // alias is `provider.<kind>` and the record's kind is what the user
    // declared.
    assert_eq!(
        sealed
            .open(&key, &alias, Family::Provider(ProviderKind::Anthropic))
            .expect("nothing in the bytes names a provider")
            .kind(),
        Kind::Provider(ProviderKind::Anthropic)
    );
}

// The whole reason the one-store shape was chosen: `HeldSecrets` is built
// from what the store holds, so a provider key is covered by ADR-0008 clause
// 6 without a second seam. The discriminating arm is a redactor built from a
// store that does *not* hold the key.
#[test]
fn a_provider_key_is_redacted_because_the_store_holds_it() {
    use crate::redaction::held_secrets_for_redaction;
    use zaru_core::redaction::Redactor;

    let scratch = ScratchRoot::new();
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("the store opens");
    let (provider, value) = staged_provider_entry(ProviderKind::Gemini);
    let alias = provider.alias().clone();
    store.add(provider, &keys, None).expect("it is added");

    let held = held_secrets_for_redaction(&store, &keys).expect("the store opens its own seals");
    assert_eq!(held.len(), 1);

    let prompt = format!("the request failed with key {value} attached");
    let redacted = held.redact(&prompt);
    assert!(
        !redacted.contains(&value),
        "a provider key survived redaction"
    );
    assert!(
        !redacted.contains(ascii_core(&value)),
        "a provider key's ASCII core survived redaction"
    );
    assert!(
        redacted.contains(&crate::redaction::marker(&alias)),
        "the marker naming the alias is absent, so this could be an erasure rather than a redaction"
    );

    // The discriminating arm: the same text against a redactor holding
    // nothing comes through unaltered, so the assertion above is about the
    // store's contents rather than about the text.
    let empty = CredentialStore::open(scratch.control()).expect("an empty store opens");
    let nothing = held_secrets_for_redaction(&empty, &keys).expect("an empty store holds nothing");
    assert_eq!(nothing.redact(&prompt), prompt);
}

// The mechanism the check above discovered, pinned so it cannot be removed
// without a compile error. `SecretRefused` is `Copy`, and a `Copy` type
// cannot own a `String`, so "a refusal carries no part of the value" is a
// property of the type rather than of the four `Display` arms somebody has to
// keep writing correctly. Dropping the derive to add an `offered` field
// stops this function compiling: "the trait bound `SecretRefused: Copy` is
// not satisfied".
#[test]
fn secret_refusals_cannot_carry_a_value() {
    const fn only_for_copy_types<T: Copy>() {}
    only_for_copy_types::<SecretRefused>();

    // And the same for the entry-level refusal's two family names, which are
    // `&'static str` rather than `String` for the same reason: neither can be
    // made to carry a runtime value.
    let refusal = Entry::provider(
        Alias::new("copy").expect("a well-formed alias"),
        Description::new("d").expect("a one-line description"),
        Secret::notes(personal_secret_nonce()).expect("nn_mcp_ names a kind"),
    )
    .expect_err("a Notes token is not a provider key");
    let _: &'static str = refusal.wanted;
    let _: &'static str = refusal.found;
}
