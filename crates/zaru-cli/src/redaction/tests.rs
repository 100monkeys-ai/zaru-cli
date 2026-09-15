// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the one product redactor does, and what it refuses to say.
//!
//! Every credential here is a generated nonce from `credentials::fixtures`,
//! carrying a decomposed grapheme cluster, a precomposed one and an
//! astral-plane character. A nonce is a uniqueness device rather than a
//! secret, and the awkwardness is what makes an absence assertion mean
//! something: see that module for the mutation it answers.

use crate::credentials::alias::Alias;
use crate::credentials::entry::{Description, Entry, Instance, Reach, ToolScope};
use crate::credentials::fixtures::{
    ScratchRoot, app_secret_nonce, ascii_core as fixture_ascii_core, nonce, personal_secret_nonce,
};
use crate::credentials::sealing::fixtures::StagedKey;
use crate::credentials::secret::Secret;
use crate::credentials::store::CredentialStore;
use crate::redaction::{HeldSecrets, ascii_core, held_secrets_for_redaction, marker};
use crate::tools::{Captured, OutputBudget};
use std::borrow::Cow;
use zaru_core::redaction::{Redacted, Redactor};

/// A store on its own scratch root holding one entry per supplied value.
///
/// Returns the store, the key store it was sealed under, and the aliases in
/// the order the values were given.
fn store_holding(
    scratch: &ScratchRoot,
    values: &[String],
) -> (CredentialStore, StagedKey, Vec<Alias>) {
    let keys = StagedKey::minted();
    let mut store = CredentialStore::open(scratch.store_root()).expect("a fresh root opens");
    let mut aliases = Vec::new();
    for (index, value) in values.iter().enumerate() {
        // Deliberately **not** a nonce. An alias from the same generator as
        // the secret shares its pid and its timestamp, so a marker naming it
        // legitimately carries bytes that also appear in the value -- which
        // made the marker check below fail on its own staging rather than on
        // the product. Each check owns its scratch root, so a short fixed
        // name is unique where it has to be.
        let alias = Alias::new(&format!("held{index}")).expect("a plain name is a legal alias");
        let entry = Entry::notes(
            alias.clone(),
            Description::new(format!("held {index}, {}", nonce("purpose"))).expect("one line"),
            Secret::notes(value.clone()).expect("the fixture prefixes name a kind"),
            Reach::InstanceLocked(Instance::new("100monkeys-ai.cortex.page")),
        )
        .expect("an nn_ value builds a Nuclear Notes entry")
        .with_tools(ToolScope::of_names(["pages.read"]));
        store.add(entry, &keys, None).expect("an entry is added");
        aliases.push(alias);
    }
    (store, keys, aliases)
}

#[test]
fn a_held_value_and_its_ascii_core_are_replaced_by_a_marker_naming_the_alias() {
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let core = ascii_core(&value);
    assert!(
        !core.is_empty() && core != value,
        "the fixture must produce an ASCII core distinct from the value, or \
         the escaped-form arm of this check asserts nothing: {value:?}"
    );

    let (store, keys, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "one entry is one held value");

    // The expected marker is composed here from the alias, not read back out
    // of the redactor. An assertion phrased in the quantity under test cannot
    // survive its own mutation -- Verification lessons §11, and the shape a
    // mutation actually exploited in `zaru-core`'s own redaction checks.
    let expected = format!("<redacted: {}>", aliases[0]);
    assert_eq!(marker(&aliases[0]), expected);

    // The second text is the **real** escaped rendering rather than one
    // typed by hand: `{:?}` is what a Debug in a refusal, a panic or an
    // assertion failure would produce, and it is the exact form the mutation
    // ADR-0007's Status tracking records survived through.
    for text in [
        format!("cmd.run failed: Authorization: Bearer {value}"),
        format!("cmd.run failed: Authorization: Bearer {value:?}"),
    ] {
        let redacted = Redacted::by(&held, &text);
        assert!(
            !redacted.as_str().contains(&value),
            "the bearer value reached the model: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(core),
            "the bearer value's ASCII core reached the model, so an escaping \
             renderer would publish it: {:?}",
            redacted.as_str()
        );
        assert!(
            !redacted.as_str().contains(fixture_ascii_core(&value)),
            "the credential fixtures' own notion of an ASCII core -- the \
             value with its awkward tail stripped -- reached the model. The \
             two definitions differ and both must be absent: {:?}",
            redacted.as_str()
        );
        assert!(
            redacted.as_str().contains(&expected),
            "nothing marks where the value was, and a redactor that erased \
             its whole input would satisfy both assertions above on its own: \
             {:?}",
            redacted.as_str()
        );
    }
}

#[test]
fn the_marker_names_the_alias_and_carries_nothing_of_the_value() {
    // ADR-0007 D2 makes the alias "a local unique name. The handle
    // everywhere", already shown to the human and to the agent, so naming it
    // tells a reader which credential was in the text. The value is what may
    // never travel -- the same rule `SecretRefused` holds, and for the same
    // reason: a marker is exactly the text that gets pasted into a report.
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let (store, keys, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    let redacted = Redacted::by(&held, &format!("here it is: {value}"));
    let written = marker(&aliases[0]);
    assert!(redacted.as_str().contains(&written));
    assert!(
        written.contains(aliases[0].as_str()),
        "the marker does not name the alias, so a reader cannot tell which \
         credential was in the text: {written:?}"
    );
    // Every window of the value that is long enough to identify it. A marker
    // that carried a prefix, a suffix or a middle slice would fail here while
    // passing a whole-value assertion. The alias is a plain name rather than
    // a nonce for exactly this reason -- see `store_holding`.
    for window in 8..=value.len() {
        for start in 0..=value.len().saturating_sub(window) {
            let end = start + window;
            if !value.is_char_boundary(start) || !value.is_char_boundary(end) {
                continue;
            }
            assert!(
                !written.contains(&value[start..end]),
                "the marker carries {} bytes of the value: {written:?}",
                end - start
            );
        }
    }
}

#[test]
fn a_secret_that_is_a_prefix_of_another_cannot_leave_its_tail_behind() {
    // Longest first. Replacing the shorter value first would rewrite the
    // longer one's head and leave its remaining bytes in the text -- a
    // partial bearer, published by a code path that ran the redactor.
    let scratch = ScratchRoot::new();
    let short = personal_secret_nonce();
    let long = format!("{short}-and-more");
    let (store, keys, _) = store_holding(&scratch, &[short.clone(), long.clone()]);
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secrets");
    assert_eq!(held.len(), 2);

    let redacted = Redacted::by(&held, &format!("the token is {long} exactly"));
    assert!(
        !redacted.as_str().contains("-and-more"),
        "the longer secret's tail survived, so the shorter one was replaced \
         first and cut it in half: {:?}",
        redacted.as_str()
    );
    assert!(!redacted.as_str().contains(&short));
    assert!(!redacted.as_str().contains(&long));
}

#[test]
fn a_harness_holding_nothing_carries_every_byte_through() {
    // The arm that discriminates every absence assertion in this crate and in
    // `zaru-core`. Without it a redactor that erased its input passes them all.
    let scratch = ScratchRoot::new();
    let (store, keys, _) = store_holding(&scratch, &[]);
    let held = held_secrets_for_redaction(&store, &keys).expect("an empty store yields nothing");
    assert!(held.is_empty());

    let text = format!(
        "stdout: {}\nstderr: {}\n",
        app_secret_nonce(),
        nonce("other")
    );
    assert!(matches!(held.redact(&text), Cow::Borrowed(_)));
    assert_eq!(Redacted::by(&held, &text).as_str(), text);
    assert_eq!(Redacted::by(&HeldSecrets::none(), &text).as_str(), text);
}

#[test]
fn the_debug_of_held_secrets_carries_a_count_and_never_a_value() {
    // The mutant is one word: `#[derive(Debug)]` here puts every held bearer
    // into every `{:?}`, every `assert_eq!` failure and every panic message
    // in the program. `Secret`'s own `Debug` carries the same sentence.
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let (store, keys, _) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    let rendered = format!("{held:?}");
    assert!(
        !rendered.contains(&value),
        "a HeldSecrets Debug published a bearer value: {rendered}"
    );
    assert!(
        !rendered.contains(ascii_core(&value)),
        "a HeldSecrets Debug published a bearer value in an escaped form: \
         {rendered}"
    );
    assert!(
        rendered.contains("HeldSecrets(1 held)"),
        "a Debug that rendered nothing at all would satisfy both assertions \
         above on its own: {rendered}"
    );
}

#[test]
fn nothing_here_matches_a_pattern_and_an_unheld_secret_is_carried_through() {
    // The decision's own out-of-scope sentence, as a check rather than as a
    // comment. A value that is shaped exactly like a bearer -- right prefix,
    // right length -- but that the harness does not hold reaches the model
    // unaltered, because the harness redacts what it holds and looks for
    // nothing else. A future pattern matcher reddens here, which is the point.
    let scratch = ScratchRoot::new();
    let held_value = personal_secret_nonce();
    let unheld = app_secret_nonce();
    let (store, keys, _) = store_holding(&scratch, std::slice::from_ref(&held_value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    let text = format!("held {held_value} and unheld {unheld}");
    let redacted = Redacted::by(&held, &text);
    assert!(
        !redacted.as_str().contains(&held_value),
        "the held value was not redacted: {:?}",
        redacted.as_str()
    );
    assert!(
        redacted.as_str().contains(&unheld),
        "a value the harness does not hold was redacted, which means \
         something here is matching a pattern. ADR-0008's decision of \
         2026-09-05 names unknown secrets in command output as out of scope: \
         {:?}",
        redacted.as_str()
    );
}

#[test]
fn a_held_value_across_a_tools_elision_boundary_leaves_no_fragment() {
    // The defect the staged outside-caller evidence caught, kept as a check.
    // `Captured::present` truncates head-and-tail under ADR-0011 D5; if the
    // port ran after that, a held value straddling the cut would be halved
    // and its head would survive in what the model reads, where no later
    // redaction recognises it. `zaru-core`'s refinement construction had this
    // rule written down and this path did not.
    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let core = ascii_core(&value);
    let (store, keys, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");

    // A budget whose kept head ends **inside** the value: the head keeps
    // `budget / 2` rounded up, so the value must start before that and end
    // after it.
    // Chosen so that the boundary falls inside the value in the *raw* stream
    // and the whole marker still fits in the kept head of the *redacted* one
    // -- otherwise this check reddens on a truncated marker rather than on a
    // surviving fragment, which is a different failure wearing the same red.
    let budget: usize = 48;
    let head_kept = budget.div_ceil(2);
    let lead = "x: ";
    assert!(
        lead.len() < head_kept && head_kept < lead.len() + core.len(),
        "the staging must put the elision boundary inside the value, or a \
         truncate-first implementation leaves nothing for this check to see"
    );
    assert!(
        lead.len() + marker(&aliases[0]).len() <= head_kept,
        "the whole marker must fit in the kept head, or the presence \
         assertion below fails on a truncated marker rather than on a \
         surviving fragment"
    );
    let captured = Captured {
        exit_code: 0,
        stdout: format!("{lead}{value}{}", "T".repeat(400)),
        stderr: String::new(),
    };

    let mut sink = Preserving::default();
    let shown = captured
        .present(
            OutputBudget::new(budget).expect("a non-zero budget"),
            &held,
            Some(&mut sink),
        )
        .expect("a sink was supplied");

    let stdout = shown.stdout.as_str();
    assert!(
        stdout.contains("bytes elided"),
        "the staging must actually truncate: {stdout:?}"
    );
    assert!(
        !stdout.contains(&value),
        "the whole held value survived truncation: {stdout:?}"
    );
    // The head of the core is what a truncate-first implementation leaves.
    let head_of_the_core = &core[..head_kept - lead.len()];
    assert!(
        !stdout.contains(head_of_the_core),
        "the first bytes of a held value survived into what a caller is \
         shown, which is what truncating before redacting leaves behind: \
         {stdout:?}"
    );
    assert!(
        stdout.contains(&marker(&aliases[0])),
        "nothing marks where the value was: {stdout:?}"
    );

    // And the record still has all of it. ADR-0011 D5 promises the whole
    // output survives where the user can read it; a redaction that reached
    // the preserved file would have taken the evidence with it.
    assert!(
        sink.preserved.contains(&value),
        "the overflow sink preserved a redacted capture rather than the whole \
         one: {:?}",
        sink.preserved
    );
}

/// An overflow sink that keeps what it was handed, so a check can assert the
/// record is raw as well as asserting the excerpt is not.
#[derive(Default)]
struct Preserving {
    preserved: String,
}

impl crate::tools::Overflow for Preserving {
    fn preserve(
        &mut self,
        captured: &Captured,
    ) -> Result<std::path::PathBuf, crate::tools::OverflowFailure> {
        self.preserved = format!("{}{}", captured.stdout, captured.stderr);
        Ok(std::path::PathBuf::from("/staged/output-0001.txt"))
    }
}

/// [ADR-0010] D2's two conversation records pass this port, and this is the
/// only place on that file where anything does.
///
/// # Why a record on a file the record itself calls raw is redacted
///
/// ADR-0010's Negative section says the transcript "contains whatever the
/// session contained, **including secrets that appeared in command output**",
/// and every other record on it stays verbatim. This port is a different
/// obligation: it is over values the harness itself **holds**. The rule the
/// accepted Update of 2026-09-06 states is that the person's words and the
/// harness's answer are raw except for a credential this harness put in its
/// own sealed store.
///
/// # What discriminates
///
/// **The control is the arm that makes the absence mean anything.** A
/// constructor that returned an empty string, or that dropped the text
/// entirely, would satisfy an absence assertion on its own; so the same
/// record must carry the control byte for byte, and the marker must name the
/// alias. Both voices are held, because the two are built by two public
/// functions and a check over one of them would pass against the other being
/// raw.
///
/// **The mutant:** `utterance` building `text` from its argument rather than
/// from `Redacted::by(redactor, text)` — which is what a transcript record
/// looked like everywhere else on this file, and is exactly the reading this
/// arc started from and measured to be wrong.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn adr_0010_d2s_conversation_records_are_built_through_the_port() {
    use crate::compose::boundary::{spoken_by_the_user, spoken_by_zaru};
    use crate::session::{Record, Voice};

    let scratch = ScratchRoot::new();
    let value = personal_secret_nonce();
    let core = ascii_core(&value);
    let control = format!("control-{}", nonce("conversation"));

    let (store, keys, aliases) = store_holding(&scratch, std::slice::from_ref(&value));
    let held = held_secrets_for_redaction(&store, &keys).expect("the store yields its secret");
    assert_eq!(held.len(), 1, "one entry is one held value");

    let said = format!("echo {value} and {control}");
    for (voice, record) in [
        (Voice::User, spoken_by_the_user(&held, 7, &said)),
        (Voice::Zaru, spoken_by_zaru(&held, 7, &said)),
    ] {
        let Record::Conversation(utterance) = record else {
            panic!("both constructors build ADR-0010 D2's seventh producer and nothing else");
        };
        assert_eq!(utterance.n, 7, "the turn number is the caller's");
        assert_eq!(utterance.voice, voice, "the voice is the constructor's");

        let text = &utterance.text;
        assert!(
            text.contains(&control),
            "the {} half carries no control, so the two absences below are about an empty \
             record rather than about redaction: {text:?}",
            voice.spoken_as()
        );
        for (what, needle) in [("by value", value.as_str()), ("by its ASCII core", core)] {
            assert!(
                !text.contains(needle),
                "a held provider key reached ADR-0010 D2's {} record {what}, and that file is \
                 one of the files `absent_everywhere` walks: {text:?}",
                voice.spoken_as()
            );
        }
        assert!(
            text.contains(&marker(&aliases[0])),
            "nothing marks where the value was, so a reader cannot tell a redaction from a \
             thing the person never typed: {text:?}"
        );
    }

    // The accepting sibling from the product: an empty store redacts nothing,
    // so a record built through the same door carries the whole line. Without
    // it the absences above are satisfied by a port that removes everything.
    let Record::Conversation(carried) = spoken_by_the_user(&HeldSecrets::none(), 7, &said) else {
        panic!("the constructor builds one variant");
    };
    assert_eq!(
        carried.text, said,
        "a harness holding nothing altered a line anyway, so this port is doing something \
         other than removing values the harness holds"
    );
}

// --- The enumeration ADR-0008's decision asks for --------------------------

/// Every product source file the redaction port is called from, and what path
/// each one is.
///
/// **This list is the authority on how many paths there are**, and the
/// decision's own "three today" is the number that was known when it was
/// written. **It is eight**, and the decision's own count has been overtaken
/// four times: the `redaction-seam` arc found a fourth by reading the code, a
/// coordinator ruling of 2026-09-05 put it in, the `shell-task-turns` arc's
/// finished turn became the seventh, and the eighth arrived the same day with
/// something no reading could have found because the thing at the end of it
/// did not exist. That is why ADR-0008 was amended to point at this check
/// rather than at a number.
///
/// **The eighth row was named on ADR-0008 before it was added here**, which is
/// what the assertion below demands in its own words. It reddened on the first
/// compile of `compose/summarise.rs`, which is the whole purpose of
/// enumerating rather than counting.
const PATHS: [(&str, &str); 9] = [
    (
        "zaru-core/src/iteration/refinement.rs",
        "the refinement prompt's four variable-length parts (ADR-0008 D4)",
    ),
    (
        "zaru-core/src/context/assembly.rs",
        "the assembled context (ADR-0013 D1 and D5). **Its own description \
         said \"covering layers 6 and 7\" until 2026-09-15, and that was \
         narrower than what the call does**: `Context::render` begins with the \
         stable prefix and `Redacted::by` takes the whole render, so layers 1 \
         to 4 pass the port too -- which is what lets ADR-0027's served page \
         carry a held bearer without one reaching a model. Measured from the \
         release binary at `15d31f1`, where a persona planted with a stored \
         bearer in it assembled with the marker in its place",
    ),
    (
        "zaru-core/src/tool_call/port.rs",
        "a refusal's sentence becoming the next turn's content (ADR-0011 D6)",
    ),
    (
        "zaru-cli/src/tools/output.rs",
        "a tool's two captured streams, before D5's truncation (ADR-0011 D5)",
    ),
    (
        "zaru-cli/src/tools/execute.rs",
        "the assembled tool result, which is what a `ToolResult` is built from",
    ),
    (
        "zaru-cli/src/session/resume.rs",
        "a resumed session's interrupted call (ADR-0010 D4)",
    ),
    (
        "zaru-cli/src/compose/boundary.rs",
        "a finished turn becoming the next turn's layer 6, in a session that \
         holds a conversation (ADR-0013 D1); and, since 2026-09-06, the same \
         two strings becoming ADR-0010 D2's conversation records, from the \
         same call site so that this list does not gain a ninth row for a \
         path that is not a model prompt",
    ),
    (
        "zaru-cli/src/compose/summarise.rs",
        "the span a compaction sends to a model as its own request (ADR-0013 D2)",
    ),
    (
        "zaru-cli/src/compose/persona.rs",
        "ADR-0027 D1's served page, becoming ADR-0013 D1's layer 1 **and** the          line `~/.zaru/persona.jsonl` holds. **The ninth row is the first that          is not a prompt**, and it is deliberate: the prompt seam alone would          have covered the model and not a person reading the cache with `cat`,          which ADR-0010 D5 invites them to do, so the body is redacted once on          the way into the file and the same redaction is what layer 1 takes.          Ruled 2026-09-15 under directive 20, open to Jeshua's veto",
    ),
];

#[test]
fn no_captured_bytes_reach_a_prompt_except_through_the_port() {
    // ADR-0008's decision of 2026-09-05 asks for the paths to be "enumerated
    // by a check", so that a fifth added later reddens rather than arriving
    // unnoticed. The type system holds the other half: `Prompt` and
    // `ToolResult::content` can only be built from a `Redacted`, and
    // `Redacted` has one constructor which takes a `Redactor`. This walk is
    // what notices a *new* door being opened or a *new* path appearing.
    //
    // Agent lessons §44: when a rule is enforced by matching source text, the
    // matching is part of the rule, so a walk that found too little must fail
    // rather than pass. Comment lines are stripped, so a doc comment naming
    // the call does not count as one.
    let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let core = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("zaru-core")
        .join("src");
    let sources: Vec<(std::path::PathBuf, String)> = product_sources(&cli)
        .into_iter()
        .chain(product_sources(&core))
        .collect();
    let lines: usize = sources.iter().map(|(_, body)| body.lines().count()).sum();
    println!(
        "the port's enumeration scanned {} product source file(s) and {lines} line(s) across \
         zaru-cli and zaru-core",
        sources.len()
    );
    assert!(
        sources.len() >= 60 && lines >= 8_000,
        "scanned {} product source file(s) and {lines} line(s) across two crates, which is less \
         than they hold; the walk is broken rather than the tree clean",
        sources.len()
    );

    let mut found: Vec<String> = Vec::new();
    let mut second_doors: Vec<String> = Vec::new();
    for (path, body) in &sources {
        let code: String = body
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let shown = shown_path(path);
        if code.contains("Redacted::by(") {
            found.push(shown.clone());
        }
        // The tuple constructor. `Redacted::by` is the only door and the only
        // place that may build one is the module that declares the type.
        if code.contains("Redacted(") && !shown.ends_with("zaru-core/src/redaction.rs") {
            second_doors.push(shown);
        }
    }
    found.sort();
    found.dedup();

    assert!(
        !found.is_empty(),
        "no product source calls the redaction port at all, so ADR-0008 clause 6's decision is \
         declared and not applied; {} file(s) and {lines} line(s) were scanned",
        sources.len()
    );
    assert!(
        second_doors.is_empty(),
        "a second way to build a `Redacted` was added outside the module that declares it, which \
         is the bypass the absent constructor exists to prevent: {second_doors:?}"
    );

    let mut expected: Vec<String> = PATHS.iter().map(|(path, _)| (*path).to_owned()).collect();
    expected.sort();
    assert_eq!(
        found,
        expected,
        "the set of product files calling ADR-0008 clause 6's port changed. A path added here is \
         a path from captured bytes into a model prompt, and the decision of 2026-09-05 requires \
         it to be named on the record before it is added; a path removed here is a path that has \
         stopped being redacted. What each known one is:\n{}",
        PATHS
            .iter()
            .map(|(path, what)| format!("  {path} -- {what}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A path as this check names it: the crate directory and everything under
/// it, with the platform's separator normalised.
///
/// Not the absolute path, which carries the worktree's own name -- the
/// `manifest-validators` arc reported a false offender because a substring
/// test matched the directory it was working in.
fn shown_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    for crate_name in ["zaru-cli/src/", "zaru-core/src/"] {
        if let Some(at) = text.rfind(crate_name) {
            return text[at..].to_owned();
        }
    }
    text
}

/// Every `.rs` file under `root` that is not part of a module's test tree.
fn product_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut found = Vec::new();
    let mut frontier = vec![root.to_path_buf()];
    while let Some(here) = frontier.pop() {
        let entries = std::fs::read_dir(&here)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", here.display()));
        for entry in entries {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                frontier.push(path);
                continue;
            }
            if path.extension().is_some_and(|extension| extension == "rs")
                && !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("fixtures.rs" | "tests.rs")
                )
            {
                let body = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
                found.push((path, body));
            }
        }
    }
    found
}
