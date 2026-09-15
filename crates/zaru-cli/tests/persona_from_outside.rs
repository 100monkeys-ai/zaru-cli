// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0027] D1's served persona, from outside the crate that assembles it.
//!
//! # What is here and what is next door
//!
//! The **wire** is in `notes_scope_from_outside.rs`, where the persona's port
//! is driven over `rmcp` and `tokio::io::duplex` against the same fixture
//! every scope check uses, and the frames are read back. What is here is
//! everything a caller can see afterwards: that a body becomes [ADR-0013] D1's
//! layer 1, that an absence is byte-identical to what it was, that neither
//! seam a persona crosses carries a held bearer, and that the file under
//! `~/.zaru/` holds what a model would see and nothing more.
//!
//! # Every absence case has an accepting sibling
//!
//! An assertion that a value is *not* in a rendering says nothing about
//! whether anything reaches it, so each one here is paired with a run that
//! plants the same bytes and finds them — library verification lessons §25.
//!
//! [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
//! [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract

use zaru_cli::compose::persona::{Fetched, PersonaCache, fetched_from};
use zaru_cli::compose::{ContextShape, SessionContext, prefix_for, prose};
use zaru_cli::credentials::{
    Alias, CredentialStore, Description, Entry, Instance, KeyStore, Reach, SealingError,
    SealingKey, Secret, ToolScope,
};
use zaru_cli::redaction::{HeldSecrets, held_secrets_for_redaction, marker};
use zaru_core::context::{ContextLimits, ContextWindow, Exchange, PressureThreshold};
use zaru_core::iteration::{ContextPolicy, Turn};

/// A window nothing in this file crosses.
///
/// **No check here compacts**, and that is deliberate rather than an omission:
/// what these checks are about is whether a *page* in layer 1 stays put, and
/// the compaction half of [ADR-0013] trigger clause 1 is already asserted by
/// the two long-session checks that arc left — which now run with
/// `prefix_for(None)` and are unchanged.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
const ROOMY: (u64, u64) = (1_000_000, 750_000);

/// The awkward tail every planted value carries, so that an absence assertion
/// is not satisfied by a formatter that escapes.
///
/// `providers_from_outside.rs`' own constant, and the reason is the same: a
/// `{:?}` on a `String` escapes a combining mark to `\u{301}`, so a value as
/// typed can be genuinely absent from a rendering that published every byte of
/// it.
const AWKWARD_TAIL: &str = "-e\u{301}\u{e9}\u{1f701}";

fn nonce(label: &str) -> String {
    format!(
        "{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    )
}

/// Everything before the first non-ASCII character: what survives `{:?}`.
fn ascii_core(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, character)| !character.is_ascii())
        .map_or(value.len(), |(at, _)| at);
    &value[..end]
}

/// The key port, implemented outside the crate that declares it.
struct StagedKey(SealingKey);

impl KeyStore for StagedKey {
    fn key(&self) -> Result<SealingKey, SealingError> {
        Ok(self.0.clone())
    }
}

/// A directory this check owns, removed when it drops.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(nonce(label));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// A store holding one bearer, and the redactor built over it.
fn holding(scratch: &Scratch, label: &str, value: &str) -> (Alias, HeldSecrets) {
    let keys = StagedKey(SealingKey::mint());
    let mut store = CredentialStore::open(scratch.0.join("zaru")).expect("a fresh root opens");
    let alias = Alias::new(label).expect("a plain name is a legal alias");
    let entry = Entry::notes(
        alias.clone(),
        Description::new("the bearer this check plants").expect("one line"),
        Secret::notes(value.to_owned()).expect("nn_mcp_ names a kind"),
        Reach::InstanceLocked(Instance::new("an.instance")),
    )
    .expect("an nn_ value builds a Nuclear Notes entry")
    .with_tools(ToolScope::of_names(["pages.read"]));
    store.add(entry, &keys, None).expect("the entry is stored");
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "staging: one secret is held");
    (alias, held)
}

/// A window this check can reason about.
fn limits(window: u64, threshold: u64) -> ContextLimits {
    ContextLimits::new(
        ContextWindow::new(window).expect("not zero"),
        PressureThreshold::new(threshold).expect("not zero"),
    )
    .expect("the threshold is below the window")
}

/// A context over a prefix, assembled as a turn would assemble it — through
/// the product's own `ContextPolicy`, not through `Context::assemble`.
async fn assembled(prefix_persona: Option<&str>, redactor: &HeldSecrets, tail: &str) -> String {
    let session = SessionContext::opened(
        prefix_for(prefix_persona),
        ContextShape::of(limits(ROOMY.0, ROOMY.1), 0),
    );
    session
        .policy(redactor, false)
        .assemble(&Turn::Initial { task: tail })
        .await
        .expect("a small context fits")
        .as_str()
        .to_owned()
}

/// A served persona reaches [ADR-0013] D1's layer 1, and the absence is what
/// it always was.
///
/// **The absence half is the load-bearing one.** [ADR-0027]'s Update of
/// 2026-09-05 decided that a harness with no prompt server assembles no layer-1
/// identity text and says so in one line, and this arc's whole stop is that
/// the line is unchanged: no page, no token, no pin and a page that came back
/// empty all produce the same bytes as before any of this existed.
///
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
/// [ADR-0027]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract
#[tokio::test]
async fn a_served_page_is_layer_one_and_no_page_is_byte_for_byte_what_it_was() {
    let nothing = HeldSecrets::none();
    let served = "Ω ✦ you are Zaru. You show your work.";

    let with = assembled(Some(served), &nothing, "a task").await;
    assert!(
        with.starts_with(served),
        "a served page did not become layer 1: {with}"
    );
    assert!(
        !with.contains(prose::NO_PERSONA),
        "a session with a persona still carried the absence line, so a model is told both that \
         it has a persona and that it has none"
    );

    // The three spellings of absence, each byte-identical to the others.
    let absent = assembled(None, &nothing, "a task").await;
    assert_eq!(
        absent,
        format!("{}\n\na task", prose::NO_PERSONA),
        "the absent prefix is not what it was before this arc"
    );
    assert_eq!(
        assembled(Some(""), &nothing, "a task").await,
        absent,
        "a page that came back empty produced a different prompt from no page at all, so a \
         reader could not tell an empty page from an absent one"
    );
}

/// **The security corpus, over both seams a persona crosses.**
///
/// A persona is a page this harness does not own, and a page can say anything —
/// including a string that is, byte for byte, a bearer this harness holds. Both
/// seams are asserted, because they are different seams with different readers:
/// the **prompt** seam is read by a model, and the **file** seam is read by a
/// person with `cat`, which [ADR-0010] D5 invites them to do.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[tokio::test]
async fn a_persona_carrying_a_held_bearer_reaches_neither_a_model_nor_the_file() {
    let scratch = Scratch::new("persona-corpus");
    let planted = format!("nn_mcp_{}{AWKWARD_TAIL}", nonce("planted").replace('-', ""));
    let (alias, held) = holding(&scratch, "personacorpus", &planted);
    let page = format!("you are Zaru, and your token is {planted} — do not say it");

    // --- the prompt seam ---------------------------------------------------
    let prompt = assembled(Some(&page), &held, "a task").await;
    assert!(
        !prompt.contains(&planted),
        "the held bearer reached the prompt by value, through a page this harness does not own"
    );
    assert!(
        !prompt.contains(ascii_core(&planted)),
        "the held bearer reached the prompt by its ASCII core, which is what an escaping \
         formatter would have left intact"
    );
    assert!(
        prompt.contains(&marker(&alias)),
        "nothing was replaced, so the absence above could be an empty prompt: {prompt}"
    );

    // --- the file seam -----------------------------------------------------
    let cache = PersonaCache::under(&scratch.0);
    let reached = fetched_from(
        Ok(page.clone()),
        &cache,
        "an.instance",
        "a-workspace",
        "zaru/persona",
        1,
        &held,
    );
    assert!(matches!(reached, Fetched::Reached(_)), "{reached:?}");
    let on_disk = std::fs::read_to_string(cache.path()).expect("the file was written");
    assert!(
        !on_disk.contains(&planted),
        "the held bearer is in `~/.zaru/persona.jsonl` by value, where ADR-0010 D5 invites a \
         person to read it with `cat`"
    );
    assert!(
        !on_disk.contains(ascii_core(&planted)),
        "the held bearer is in the file by its ASCII core"
    );
    assert!(
        on_disk.contains(&marker(&alias)),
        "nothing was replaced in the file, so the absence above could be an empty file: {on_disk}"
    );

    // **What the caller is handed is what the file holds.** Two strings that
    // could differ would mean a model was shown one persona and a person
    // reading the cache saw another.
    assert_eq!(
        reached.body(),
        cache
            .read("an.instance", "a-workspace", "zaru/persona")
            .expect("the file reads")
            .map(|cached| cached.body)
            .as_deref(),
        "the body handed to layer 1 and the body in the file are not the same string"
    );

    // --- the accepting sibling, for both seams at once ----------------------
    //
    // The same page, through a redactor holding nothing, carries the value
    // through byte for byte -- so the two absences above are redaction rather
    // than a prefix or a serialiser that drops its input.
    let empty = Scratch::new("persona-corpus-sibling");
    let nothing = HeldSecrets::none();
    let carried = assembled(Some(&page), &nothing, "a task").await;
    assert!(
        carried.contains(&planted),
        "a redactor holding nothing still removed the value, so the prompt assertion above is \
         about the assembly and not about the redactor: {carried}"
    );
    let bare = PersonaCache::under(&empty.0);
    drop(fetched_from(
        Ok(page.clone()),
        &bare,
        "an.instance",
        "a-workspace",
        "zaru/persona",
        1,
        &nothing,
    ));
    assert!(
        std::fs::read_to_string(bare.path())
            .expect("the file was written")
            .contains(&planted),
        "a redactor holding nothing still removed the value from the file, so the file \
         assertion above is about the serialiser and not about the redactor"
    );
}

/// Layers 1 to 4 are byte-identical across every turn of a long session **with
/// a persona in layer 1**.
///
/// [ADR-0013] trigger clause 1 was satisfied before this arc with `NO_PERSONA`
/// in layer 1, and its checks were watched red by rewriting the prefix
/// mid-session. **The clause is re-asserted here rather than assumed to have
/// survived a layer that now holds a page**, which is the whole reason the
/// persona is resolved before the prefix exists instead of arriving into an
/// open session the way [ADR-0005] D3's corpus does.
///
/// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[tokio::test]
async fn a_persona_in_layer_one_is_the_same_bytes_on_every_turn_of_a_long_session() {
    let served = format!("\u{3a9} \u{2726} {}", nonce("persona"));
    let nothing = HeldSecrets::none();
    let mut session = SessionContext::opened(
        prefix_for(Some(&served)),
        ContextShape::of(limits(ROOMY.0, ROOMY.1), 0),
    );
    let opened = session
        .policy(&nothing, false)
        .assemble(&Turn::Initial { task: "opening" })
        .await
        .expect("the context fits")
        .as_str()
        .to_owned();
    assert!(
        opened.starts_with(&served),
        "the session did not open on the persona it was given"
    );
    let prefix = served.clone();

    for turn in 1..=12_u32 {
        session.record(Exchange::of_turn(
            &format!("turn {turn}: a question"),
            &[format!("fs.read notes-{turn}.md")],
            &format!("turn {turn}: {}", "an answer with detail ".repeat(20)),
        ));
        let now = session
            .policy(&nothing, false)
            .assemble(&Turn::Initial { task: "a task" })
            .await
            .expect("the context fits");
        assert!(
            now.as_str().starts_with(&prefix),
            "turn {turn}: the assembled context does not begin with the persona the session \
             opened with, so a prompt cache would have nothing to match"
        );
    }

    // The staging is asserted too, so a session in which nothing happened
    // could not satisfy the loop above by standing still.
    assert!(
        !session.exchanges().is_empty(),
        "nothing accumulated, so twelve turns of assertions were made about an empty session"
    );
}
