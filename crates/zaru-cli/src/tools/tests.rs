// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the tool surface refuses, and what it may never let through.
//!
//! ADR-0011 decides what a model-driven action may reach, so [Testing]'s rule
//! governs this file: "Every escape found at a security boundary — a sandbox,
//! a permission model, a credential store, anything deciding what a
//! model-driven action may reach — joins a permanent hostile-input corpus as
//! its reproduction... **the corpus never shrinks**: a case removed for
//! looking redundant is a case nobody will notice the day it stops being
//! redundant."
//!
//! Every check here names the mutant it catches, because [Verification
//! lessons] §12 asks for the mutant to be written down before the comparison
//! is: "A named mutant nobody can construct is itself the finding."
//!
//! [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
//! [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons

use crate::tools::fixtures::nonce;
use crate::tools::mode::{Layer, Mode, ModeRefused, Tier};
use crate::tools::name::{Effect, ToolName};
use crate::tools::notice::SessionNotice;

/// **Corpus case 1, the representational arm — a tool reaching outside its
/// declared scope has nothing to call.**
///
/// ADR-0011 D1 names seven built-ins and its Alternative 3 says "Small is the
/// security posture, not an ergonomic compromise". The check is that the
/// forbidden reach has nothing to call, not that a call to it is denied — a
/// denial is a code path and a code path can be wrong ([Testing]).
///
/// The mutant this catches is adding an eighth variant: the match below is
/// exhaustive and stops compiling, which is louder than an assertion, and the
/// length assertion catches a variant added alongside a widened `ALL`.
///
/// The expected spellings are literals written here, not values read back out
/// of `as_str`. [Verification lessons] §11: at least one arm of a comparison
/// must not travel through the thing being checked.
///
/// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_built_in_set_is_the_seven_adr_0011_d1_names() {
    let mut named = Vec::new();
    for tool in ToolName::ALL {
        // Exhaustive on purpose. An eighth variant fails to compile here.
        let spelled = match tool {
            ToolName::FsRead => "fs.read",
            ToolName::FsWrite => "fs.write",
            ToolName::FsEdit => "fs.edit",
            ToolName::FsList => "fs.list",
            ToolName::FsSearch => "fs.search",
            ToolName::CmdRun => "cmd.run",
            ToolName::WebFetch => "web.fetch",
        };
        assert_eq!(
            tool.as_str(),
            spelled,
            "{tool:?} does not spell itself as ADR-0011 D1's table does"
        );
        named.push(spelled);
    }

    assert_eq!(
        named,
        vec![
            "fs.read",
            "fs.write",
            "fs.edit",
            "fs.list",
            "fs.search",
            "cmd.run",
            "web.fetch",
        ],
        "ADR-0011 D1's table names these seven built-ins and no others; the set this crate can \
         construct is {named:?}. Everything beyond the seven is an MCP server, and each built-in \
         is a capability with no membrane behind it at bare tier"
    );
    assert_eq!(
        ToolName::ALL.len(),
        7,
        "a built-in was added to ToolName::ALL without ADR-0011 D1 being changed"
    );
}

/// **ADR-0011 D3's `ask` rule, read literally, including the consequence that
/// is a finding.**
///
/// D3: "`ask` — Prompts before any write or command." So `fs.write`,
/// `fs.edit` and `cmd.run` prompt, and `fs.read`, `fs.list`, `fs.search` and
/// `web.fetch` do not.
///
/// `web.fetch` is the finding: at the **default** mode a URL the model chose
/// is retrieved with no prompt, because it is neither a write nor a command.
/// That is recorded as an open question on the record rather than fixed here,
/// and this check is what makes the built behaviour visible rather than
/// incidental.
///
/// The mutant: folding `Retrieve` in with `Write | Command`, which would make
/// the harness prompt on `web.fetch` and quietly answer a question the record
/// left open. Both arms are asserted, so "prompt on everything" and "prompt
/// on nothing" each fail.
#[test]
fn only_a_write_or_a_command_prompts_in_ask_mode() {
    let mut prompting = Vec::new();
    let mut silent = Vec::new();
    for tool in ToolName::ALL {
        if tool.effect().prompts_in_ask() {
            prompting.push(tool.as_str());
        } else {
            silent.push(tool.as_str());
        }
    }

    assert_eq!(
        prompting,
        vec!["fs.write", "fs.edit", "cmd.run"],
        "ADR-0011 D3's `ask` mode \"prompts before any write or command\"; the tools that prompt \
         are {prompting:?}"
    );
    assert_eq!(
        silent,
        vec!["fs.read", "fs.list", "fs.search", "web.fetch"],
        "these are the tools D3's `ask` sentence does not name, so none of them prompts on its \
         effect alone; `web.fetch` among them is the open question recorded on ADR-0011. Note \
         that D4 still prompts for any of them outside the working directory"
    );

    // The effect classification is the thing the rule reads, so it is asserted
    // per tool rather than only in aggregate.
    assert_eq!(ToolName::FsRead.effect(), Effect::Read);
    assert_eq!(ToolName::FsWrite.effect(), Effect::Write);
    assert_eq!(ToolName::CmdRun.effect(), Effect::Command);
    assert_eq!(ToolName::WebFetch.effect(), Effect::Retrieve);
}

/// **Corpus case 2 — a permission that widens without the user's act.**
///
/// ADR-0014 D6: "A repository the user cloned must not be able to configure
/// its way to more privilege than the user granted." Its own trigger clause 5
/// asks for each escalation asserted separately rather than one test covering
/// "escalation is rejected", which is the adjacent-coverage disguise of
/// [Verification lessons] §28.
///
/// **Two arms, and the second is what discriminates.** The same key and the
/// same value are refused from the project layer and accepted from the user
/// layer. A gate that refused every layer would pass a one-sided assertion
/// and be wrong — that is [Verification lessons] §9 in the direction the
/// fixture usually hides.
///
/// The mutant: dropping the `is_written_by_a_cloned_repository` guard, after
/// which a cloned repository sets `yolo` and nothing says so.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_cloned_repository_cannot_set_the_permission_mode_and_the_user_can() {
    let key = nonce("permission-key");

    for mode in Mode::ALL {
        let refusal = Mode::from_layer(Layer::Project, &key, mode.as_str())
            .expect_err("the project layer must not set the permission mode");
        let rendered = refusal.to_string();

        assert!(
            matches!(refusal, ModeRefused::FromAClonedRepository { .. }),
            "a project-layer {mode} was refused for the wrong reason: {refusal:?}"
        );
        assert!(
            rendered.contains(&key),
            "ADR-0014 D6 requires the error name the key; it named none of {key:?} in {rendered:?}"
        );
        assert!(
            rendered.contains("must not be able to configure its way to more privilege"),
            "the refusal does not give D6's reason: {rendered:?}"
        );

        // The arm that discriminates: the same key and the same value, from a
        // layer the user writes, is accepted.
        assert_eq!(
            Mode::from_layer(Layer::User, &key, mode.as_str()),
            Ok(mode),
            "the user's own configuration must be able to set {mode}, or the refusal above is \
             \"refuse everything\" rather than a boundary"
        );
    }

    // Exactly one layer is a cloned repository's, and the other four are not.
    let cloned: Vec<&str> = Layer::ALL
        .into_iter()
        .filter(|layer| layer.is_written_by_a_cloned_repository())
        .map(Layer::as_str)
        .collect();
    assert_eq!(
        cloned,
        vec!["project config"],
        "ADR-0014 D1's layer 3 is the one a cloned repository writes; this crate believes it is \
         {cloned:?}"
    );
}

/// A project layer offering a misspelled mode is refused for the reason that
/// matters.
///
/// The ordering inside `from_layer` is load-bearing: checking the value first
/// would report a typo to a repository that was not allowed to set the key at
/// all, which tells an attacker which spellings are real and tells the user
/// nothing about what was attempted.
///
/// The mutant: swapping the two checks.
#[test]
fn a_cloned_repository_is_refused_the_layer_before_the_value_is_parsed() {
    let key = nonce("permission-key");
    let refusal = Mode::from_layer(Layer::Project, &key, "yolo-but-misspelled")
        .expect_err("the project layer must not set the permission mode");
    assert!(
        matches!(refusal, ModeRefused::FromAClonedRepository { .. }),
        "a misspelled mode from a cloned repository was reported as a typo rather than as an \
         escalation: {refusal:?}"
    );
}

/// An unknown mode names the three that exist and does not guess.
///
/// ADR-0014 D5's nearest-match suggestion is about unknown *keys* and is that
/// record's to build; this refusal names D3's three values and suggests
/// nothing, so no behaviour of ADR-0014 is settled here.
#[test]
fn a_value_that_names_no_mode_is_refused_naming_the_three_that_exist() {
    let key = nonce("permission-key");
    for offered in ["", "ASK", "ask ", "allowlist", "yolo\n", "sudo"] {
        let refusal = Mode::from_layer(Layer::User, &key, offered)
            .expect_err("{offered:?} names no ADR-0011 D3 mode");
        let rendered = refusal.to_string();
        assert!(
            matches!(refusal, ModeRefused::NoSuchMode { .. }),
            "{offered:?} was refused for the wrong reason: {refusal:?}"
        );
        for named in ["\"ask\"", "\"allow\"", "\"yolo\""] {
            assert!(
                rendered.contains(named),
                "the refusal of {offered:?} does not name {named}: {rendered:?}"
            );
        }
    }

    // And the three that do exist are taken, so the refusal above is not
    // "refuse everything".
    for mode in Mode::ALL {
        assert_eq!(Mode::from_layer(Layer::User, &key, mode.as_str()), Ok(mode));
    }
}

/// ADR-0011 D3: the default is the safe one.
///
/// The mutant: `#[default]` moved to another variant, which would ship a
/// harness that prompts less than the record says without any diff naming a
/// mode.
#[test]
fn the_default_mode_is_ask() {
    assert_eq!(
        Mode::default(),
        Mode::Ask,
        "ADR-0011 D3 makes `ask` the default: \"Three permission modes, chosen by the user, \
         defaulting to the safe one\""
    );
    assert_eq!(Mode::ALL.len(), 3, "ADR-0011 D3 names exactly three modes");
}

/// **ADR-0011 D2's not-a-sandbox line: once, at bare tier, and nowhere else.**
///
/// D2: "At bare tier the harness states plainly, once at session start, that
/// it is not a sandbox." The sentence is the caller's — see
/// [`SessionNotice`] — so this asserts the mechanism and not the wording.
///
/// Two mutants: making `state_once` clone rather than take, which states it
/// on every call; and dropping the `has_membrane` guard, which states at
/// `contained` and `linked` a sentence that would be false there.
#[test]
fn the_session_notice_is_stated_once_and_only_where_there_is_no_membrane() {
    let sentence = nonce("not-a-sandbox");

    let mut bare = SessionNotice::for_tier(Tier::Bare, sentence.clone())
        .expect("bare tier has no membrane, so it owes the user the line");
    assert!(bare.is_owed(), "the line is owed before it is stated");
    assert_eq!(
        bare.state_once(),
        Some(sentence.clone()),
        "the sentence the caller supplied is not the sentence stated"
    );
    assert!(!bare.is_owed(), "the line is still owed after being stated");
    for again in 0..3 {
        assert_eq!(
            bare.state_once(),
            None,
            "the line was stated a second time (call {again}); D2 says once at session start"
        );
    }

    let membraned: Vec<&str> = Tier::ALL
        .into_iter()
        .filter(|tier| SessionNotice::for_tier(*tier, sentence.clone()).is_none())
        .map(Tier::as_str)
        .collect();
    assert_eq!(
        membraned,
        vec!["contained", "linked"],
        "ADR-0011 D2 gives `bare` no enforcement and the other two a membrane, so only `bare` \
         owes this line; this crate withholds it from {membraned:?}"
    );
}

/// The three tiers are ADR-0001 D1's, spelled as that record spells them.
///
/// The mutant: a fourth variant, which stops the exhaustive match compiling.
#[test]
fn the_tiers_are_the_three_adr_0001_d1_names() {
    let mut named = Vec::new();
    for tier in Tier::ALL {
        // Exhaustive on purpose.
        let spelled = match tier {
            Tier::Bare => "bare",
            Tier::Contained => "contained",
            Tier::Linked => "linked",
        };
        assert_eq!(
            tier.as_str(),
            spelled,
            "{tier:?} does not spell itself as ADR-0001 D1 spells it"
        );
        named.push(spelled);
    }
    assert_eq!(named, vec!["bare", "contained", "linked"]);
    assert!(
        !Tier::Bare.has_membrane(),
        "ADR-0011 D2's table gives `bare` no enforcement at all"
    );
    assert!(Tier::Contained.has_membrane() && Tier::Linked.has_membrane());
}
