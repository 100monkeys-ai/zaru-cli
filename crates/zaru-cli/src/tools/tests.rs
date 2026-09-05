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

use crate::process::line::CommandLine;
use crate::redaction::HeldSecrets;
// `tools::mode::Layer` below is a re-export of `config::layer::Layer`, so
// there is one `Layer` in this file and not two.
use crate::config::{
    Contribution, Field, FieldKind, ProjectPolicy, Resolution, Schema, Source, Table, Value,
};
use crate::tools::allowlist::{self, Allowed, AllowlistRefused, Entry};
use crate::tools::decision::{
    Assessment, DESTRUCTIVE_MARKING, Decision, Invocation, Permission, RefusedBecause, Requirement,
};
use crate::tools::destructive::{Category, Shapes};
use crate::tools::fixtures::{
    FailingConfirmer, RecordedConfirmer, RefusingOverflow, ScratchOverflow, ScratchTree,
    StagedAllowlist, StagedDestructive, nonce,
};
use crate::tools::mode::{self, Layer, Mode, ModeRefused, Tier};
use crate::tools::name::{Effect, ToolName};
use crate::tools::notice::SessionNotice;
use crate::tools::output::{
    BudgetIsZero, Captured, ELISION_PREFIX, OutputBudget, PresentationRefused,
};
use crate::tools::port::{Allowlist as _, DestructiveMatch as _};
use crate::tools::tree::{Placement, WorkingDirectory};
use crate::tools::{fixtures, prompt};
use std::path::PathBuf;

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
    // Enumerated over `config`'s `Layer`, which is now the crate's only
    // declaration of ADR-0014 D1's five, through D6's own predicate.
    let cloned: Vec<&str> = Layer::ALL
        .into_iter()
        .filter(|layer| layer.bound_by_the_escalation_ceiling())
        .map(Layer::label)
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

/// **Corpus case 1, the behavioural arm — a tool reaching outside the working
/// directory is classified as having left it.**
///
/// ADR-0011 D4's one invariant over its three phrasings. Every hostile case
/// below has an in-tree sibling the same rule must **accept**, because a
/// classifier that refuses everything satisfies a one-sided table perfectly
/// and is not a boundary ([Verification lessons] §9).
///
/// Every mismatch is reported, not the first — [Testing]: "A check with
/// several clauses reports them all." A table that returned on the first
/// disagreement would leave every case after it unwatchable by that mutation,
/// which is the defect the loop arc fixed on sight.
///
/// [Testing]: https://100monkeys-ai.cortex.page/zaru/p/operations/testing
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn every_way_out_of_the_working_directory_is_classified_as_out_of_it() {
    let tree = ScratchTree::new();
    let root = tree.project();
    let working = WorkingDirectory::at(&root).expect("the project directory resolves");

    let outside_absolute = tree.base().join("elsewhere").join("secret");
    let evil_absolute = tree.base().join("projectevil").join("loot");
    let inside_absolute = root.join("inside").join("file");

    let cases: Vec<(PathBuf, Placement, &str)> = vec![
        // In-tree. These are what stop "refuse everything" from passing.
        (PathBuf::from("."), Placement::InTree, "the root itself"),
        (
            PathBuf::from("inside/file"),
            Placement::InTree,
            "an ordinary relative path",
        ),
        (
            PathBuf::from("./inside/./file"),
            Placement::InTree,
            "a relative path carrying redundant current-directory components",
        ),
        (
            PathBuf::from("inside/../inside/file"),
            Placement::InTree,
            "a `..` that stays inside the tree",
        ),
        (
            PathBuf::from("does/not/exist/yet.txt"),
            Placement::InTree,
            "a path fs.write would create; a classifier that needed the path to exist would \
             refuse every file creation in the project",
        ),
        (
            inside_absolute.clone(),
            Placement::InTree,
            "an absolute path that is nonetheless below the working directory. D4's \"anywhere \
             absolute\" cannot mean every absolute path, or nothing in the project could be \
             written by its own full path",
        ),
        (
            PathBuf::from("inside/od\u{d}d\u{a}name"),
            Placement::InTree,
            "a name carrying control characters, which is still inside the tree",
        ),
        // Out of tree.
        (
            PathBuf::from("../elsewhere/secret"),
            Placement::OutOfTree,
            "a relative `..` above the working directory",
        ),
        (
            PathBuf::from("/etc/passwd"),
            Placement::OutOfTree,
            "the absolute path ADR-0004 D6's own worked example refuses",
        ),
        (
            outside_absolute.clone(),
            Placement::OutOfTree,
            "an absolute path to a sibling directory",
        ),
        (
            PathBuf::from("does-not-exist/../../elsewhere/secret"),
            Placement::OutOfTree,
            "`..` walking out through a segment that does not exist. A classifier that \
             canonicalised only what exists and appended the rest verbatim would call this \
             in-tree",
        ),
        (
            PathBuf::from("escape/secret"),
            Placement::OutOfTree,
            "a symlink out of the tree wearing an ordinary name. A purely lexical normalisation \
             would call this in-tree",
        ),
        (
            PathBuf::from("escape"),
            Placement::OutOfTree,
            "the escaping symlink itself",
        ),
        (
            evil_absolute.clone(),
            Placement::OutOfTree,
            "a sibling directory whose name extends the root's. A byte-wise prefix test calls \
             this in-tree",
        ),
        (
            PathBuf::from("../projectevil/loot"),
            Placement::OutOfTree,
            "the same sibling reached relatively",
        ),
        (
            PathBuf::from("../.."),
            Placement::OutOfTree,
            "two levels above the working directory",
        ),
        (
            PathBuf::from("/"),
            Placement::OutOfTree,
            "the filesystem root, which contains the tree rather than sitting in it",
        ),
    ];

    let mut wrong = Vec::new();
    for (candidate, expected, why) in &cases {
        let target = working.classify(candidate);
        if target.placement() != *expected {
            wrong.push(format!(
                "{candidate:?} was classified {:?} and should be {expected:?} ({why}); it \
                 resolved to {:?}",
                target.placement(),
                target.resolved()
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "ADR-0011 D4's boundary misclassified {} of {} paths against the working directory \
         {root:?}:\n  {}",
        wrong.len(),
        cases.len(),
        wrong.join("\n  ")
    );

    // The denominator comes from the table this check wrote, not from anything
    // the classifier produced (Verification lessons §17).
    assert_eq!(
        cases.len(),
        17,
        "the hostile corpus for ADR-0011 D4 shrank; Testing: the security corpus only grows"
    );
}

/// **The prefix case, on its own, because it is the sharpest mutant.**
///
/// A sibling directory whose name extends the root's is outside it. The
/// mutant is one line: comparing the two paths as strings rather than as
/// components. `Path::starts_with` is component-wise and a `str::starts_with`
/// over the same two values is not, and nothing about the two spellings looks
/// different in review.
#[test]
fn a_sibling_whose_name_merely_extends_the_roots_is_outside_it() {
    let tree = ScratchTree::new();
    let root = tree.project();
    let working = WorkingDirectory::at(&root).expect("the project directory resolves");
    let sibling = tree.base().join("projectevil").join("loot");

    // The staging is asserted, so this cannot pass because the sibling was
    // never created (Verification lessons §4).
    assert!(
        sibling.exists(),
        "staging failed: the sibling {sibling:?} that extends the root's name was not created"
    );
    assert!(
        sibling
            .to_string_lossy()
            .starts_with(&*root.to_string_lossy()),
        "this check asserts nothing unless the sibling's path really is a byte-wise prefix \
         match for the root: {sibling:?} against {root:?}"
    );

    assert_eq!(
        working.classify(&sibling).placement(),
        Placement::OutOfTree,
        "{sibling:?} is a sibling of the working directory {root:?}, not a child of it. Its path \
         starts with the root's bytes and not with the root's components, which is exactly the \
         difference between a string prefix test and a path prefix test"
    );
}

/// **The symlink case, on its own, for the same reason.**
///
/// The mutant is dropping the canonicalisation of the longest existing
/// ancestor and normalising lexically instead, which is what a reader reaches
/// for when a path does not exist yet.
#[test]
fn a_symlink_out_of_the_tree_does_not_carry_a_tool_with_it() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");

    let through = working.classify("escape/secret");
    assert!(
        tree.project().join("escape").exists(),
        "staging failed: the escaping symlink was not created"
    );
    assert_eq!(
        through.placement(),
        Placement::OutOfTree,
        "`escape/secret` is a symlink out of the working directory and was classified {:?}; it \
         resolved to {:?}",
        through.placement(),
        through.resolved()
    );
    assert!(
        through.resolved().ends_with("elsewhere/secret"),
        "the resolved target should be what the symlink actually reaches, so that a transcript \
         shows the path rather than the disguise; it was {:?}",
        through.resolved()
    );
}

/// The working directory is canonicalised once, at construction.
///
/// The mutant: keeping the caller's spelling. A root reached through a
/// symlink would then never be a component prefix of anything resolved
/// through the real path, and every in-tree path in the project would be
/// reported as having left it — a boundary that is wrong in the safe
/// direction is still wrong, and it is the direction nobody reports.
#[test]
fn a_working_directory_reached_through_a_symlink_is_the_same_directory() {
    let tree = ScratchTree::new();
    let direct = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let linked =
        WorkingDirectory::at(tree.project_by_link()).expect("the symlinked route resolves");

    assert_eq!(
        direct.root(),
        linked.root(),
        "two spellings of one working directory produced two roots: {:?} and {:?}",
        direct.root(),
        linked.root()
    );
    assert_eq!(
        linked.classify("inside/file").placement(),
        Placement::InTree,
        "a path inside the project was classified as outside it when the root was reached \
         through a symlink"
    );
}

/// A working directory that cannot be resolved is refused, not guessed at.
#[test]
fn a_working_directory_that_does_not_exist_is_refused() {
    let tree = ScratchTree::new();
    let missing = tree.base().join(nonce("no-such-directory"));
    let refusal = WorkingDirectory::at(&missing).expect_err("the directory does not exist");
    let rendered = refusal.to_string();
    assert!(
        rendered.contains(&missing.display().to_string()),
        "the refusal does not name the path it refused: {rendered:?}"
    );
    assert!(
        rendered.contains("a boundary whose root is a guess is not one"),
        "the refusal does not give its reason: {rendered:?}"
    );
    // And the arm that discriminates: a directory that does exist is taken.
    assert!(WorkingDirectory::at(tree.project()).is_ok());
}

/// **ADR-0011 D3's and D4's prompting rule, at every mode, for every reason a
/// prompt is raised.**
///
/// The expected column is written here as literals. [Verification lessons]
/// §11: at least one arm of a comparison must not travel through the thing
/// being checked, and §10: assert the consequence, never a proxy the code
/// already computes.
///
/// Every disagreement is reported, so no single mutation can hide the rows
/// after the one it breaks.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_prompting_rule_is_the_records_at_every_mode() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let inside = working.classify("inside/file");
    let outside = working.classify("../elsewhere/secret");

    let allowed = Assessment {
        allowlisted: true,
        destructive: false,
    };
    let not_allowed = Assessment::default();

    // (mode, tool, target is out of tree, allowlisted, expected, why)
    let cases: Vec<(Mode, ToolName, bool, bool, Requirement, &str)> = vec![
        // `ask`: writes and commands prompt; reads do not.
        (
            Mode::Ask,
            ToolName::FsWrite,
            false,
            false,
            Requirement::Ask,
            "D3: `ask` prompts before any write",
        ),
        (
            Mode::Ask,
            ToolName::FsEdit,
            false,
            false,
            Requirement::Ask,
            "D3: an edit is a write",
        ),
        (
            Mode::Ask,
            ToolName::CmdRun,
            false,
            false,
            Requirement::Ask,
            "D3: `ask` prompts before any command",
        ),
        (
            Mode::Ask,
            ToolName::FsRead,
            false,
            false,
            Requirement::Proceed,
            "D3 names writes and commands, and a read in the tree is neither",
        ),
        (
            Mode::Ask,
            ToolName::FsList,
            false,
            false,
            Requirement::Proceed,
            "a listing in the tree is not a write or a command",
        ),
        // D4: out of tree prompts in `ask` whatever the effect is.
        (
            Mode::Ask,
            ToolName::FsRead,
            true,
            false,
            Requirement::Ask,
            "D4: out-of-tree access prompts in `ask`, and a read is out-of-tree access",
        ),
        // `allow`: the allowlist decides, read literally.
        (
            Mode::Allow,
            ToolName::FsWrite,
            false,
            true,
            Requirement::Proceed,
            "D3: `allow` runs the allowlist without prompting",
        ),
        (
            Mode::Allow,
            ToolName::CmdRun,
            false,
            true,
            Requirement::Proceed,
            "an allowlisted command is what the mode exists for",
        ),
        (
            Mode::Allow,
            ToolName::FsWrite,
            false,
            false,
            Requirement::Ask,
            "D3: `allow` prompts for anything outside the allowlist",
        ),
        (
            Mode::Allow,
            ToolName::FsRead,
            false,
            false,
            Requirement::Ask,
            "D3's `allow` row says \"anything outside it\", read literally; the alternative reading \
          is an open question on the record",
        ),
        (
            Mode::Allow,
            ToolName::FsRead,
            true,
            false,
            Requirement::Ask,
            "D4: out-of-tree access prompts in `allow` too",
        ),
        (
            Mode::Allow,
            ToolName::FsRead,
            true,
            true,
            Requirement::Proceed,
            "an allowlisted call is allowlisted wherever it points; D3's allowlist sentence carries \
          no exception and inventing one would be authoring a permission rule",
        ),
        // `yolo`: nothing prompts, including out of tree.
        (
            Mode::Yolo,
            ToolName::FsWrite,
            false,
            false,
            Requirement::Proceed,
            "D3: `yolo` has no prompts",
        ),
        (
            Mode::Yolo,
            ToolName::FsWrite,
            true,
            false,
            Requirement::Proceed,
            "D4 removes the prompt at `yolo` and keeps the record; this row is the first half. \
          It was a `cmd.run` row until 2026-09-05, when a command stopped being measured \
          against D4's boundary as though it were a path — a command's boundary is the \
          working directory it starts in",
        ),
        (
            Mode::Yolo,
            ToolName::CmdRun,
            false,
            false,
            Requirement::Proceed,
            "`yolo` prompts for nothing at all, a command included",
        ),
        (
            Mode::Yolo,
            ToolName::FsRead,
            true,
            false,
            Requirement::Proceed,
            "`yolo` prompts for nothing at all",
        ),
    ];

    // `cmd.run` addresses a command line rather than a path, so its rows are
    // built through the constructor that takes one. A row that asked for a
    // command *out of tree* is a staging error rather than a case: a command
    // has no placement at all, and silently ignoring the flag would make the
    // table read as covering something it cannot.
    let command = CommandLine::split("printf hello").expect("a command line");
    let mut wrong = Vec::new();
    for (mode, tool, out_of_tree, allowlisted, expected, why) in &cases {
        let target = if *out_of_tree { &outside } else { &inside };
        let invocation = if *tool == ToolName::CmdRun {
            assert!(
                !*out_of_tree,
                "the case {why:?} asks for a command line out of tree, and a command line has no \
                 placement against the working directory"
            );
            Invocation::running(&command)
        } else {
            Invocation::on_path(*tool, target).expect("these tools address paths")
        };
        let assessment = if *allowlisted { allowed } else { not_allowed };
        let got = Decision::reach(*mode, &invocation, assessment).requirement();
        if got != *expected {
            wrong.push(format!(
                "{mode} + {tool} + {} + {} gave {got:?}, expected {expected:?} ({why})",
                if *out_of_tree {
                    "out of tree"
                } else {
                    "in tree"
                },
                if *allowlisted {
                    "allowlisted"
                } else {
                    "not allowlisted"
                },
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "ADR-0011's prompting rule disagreed on {} of {} cases:\n  {}",
        wrong.len(),
        cases.len(),
        wrong.join("\n  ")
    );
    assert_eq!(
        cases.len(),
        16,
        "the prompting table shrank; each row is a clause of D3 or D4"
    );
}

/// **Corpus case 3 — a denial is a code path, and the path is the user's
/// answer.**
///
/// ADR-0011 D6 gives the harness no veto, so the only refusals in the system
/// are a user saying no and there being nobody to ask. The second is the one
/// that matters: **a call needing a prompt with no confirmer is refused, not
/// performed.** That is the shape ADR-0007 D8's apex gate already uses in
/// this crate.
///
/// Three arms, because two of them would each pass a one-sided assertion: no
/// confirmer refuses, a declining confirmer refuses for a *different* stated
/// reason, and an accepting confirmer grants. Without the third, "refuse
/// always" passes.
#[test]
fn a_call_that_needs_the_user_is_refused_when_there_is_nobody_to_ask() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::FsWrite, &target).expect("fs.write addresses a path");
    let decision = Decision::reach(Mode::Ask, &invocation, Assessment::default());

    assert_eq!(
        decision.requirement(),
        Requirement::Ask,
        "staging: a write at `ask` must need a prompt, or this check asserts nothing"
    );

    assert_eq!(
        decision.permit(None),
        Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
        "a call needing the user's confirmation, with no confirmer, must be refused rather than \
         performed"
    );

    let declining = RecordedConfirmer::declining();
    assert_eq!(
        decision.permit(Some(&declining)),
        Permission::Refused(RefusedBecause::TheUserDeclined),
        "a user who was asked and said no must refuse for that reason and not for the other one"
    );
    assert_eq!(
        declining.asked().len(),
        1,
        "the user was not actually asked; a refusal that skipped the question is the silent \
         default D3 forbids, wearing the right answer"
    );

    let accepting = RecordedConfirmer::accepting();
    assert_eq!(
        decision.permit(Some(&accepting)),
        Permission::Granted,
        "a user who was asked and said yes must be able to permit the call, or the refusals \
         above are \"refuse everything\" rather than a decision"
    );

    // The refusals say different things, so a reader can tell them apart.
    let nobody = RefusedBecause::ThereWasNobodyToAsk.to_string();
    let declined = RefusedBecause::TheUserDeclined.to_string();
    assert_ne!(nobody, declined);
    assert!(
        nobody.contains("silent default"),
        "the no-confirmer refusal does not say why it is a refusal: {nobody:?}"
    );
}

/// A call that needs no prompt is granted with no confirmer at all.
///
/// The arm that stops the check above from being satisfied by "refuse
/// whenever the confirmer is absent".
#[test]
fn a_call_that_needs_no_prompt_needs_no_confirmer() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::FsRead, &target).expect("fs.read addresses a path");
    let decision = Decision::reach(Mode::Ask, &invocation, Assessment::default());

    assert_eq!(decision.requirement(), Requirement::Proceed);
    assert_eq!(
        decision.question(),
        None,
        "a call that proceeds asks nothing"
    );
    assert_eq!(decision.permit(None), Permission::Granted);
}

/// **Corpus case 4 — mode may remove the prompt; it never removes the
/// record.**
///
/// ADR-0011 D4, in two clauses that are asserted **together and reported
/// together**. A check that returned on the first clause would leave the
/// second — the one the record is actually about — unwatchable by any
/// mutation that broke the first.
///
/// Clause one: at `yolo`, an out-of-tree call raises no prompt.
/// Clause two: its transcript entry is byte-identical at all three modes, and
/// still renders the out-of-tree marking at every one of them.
#[test]
fn a_mode_may_remove_the_prompt_and_never_the_record() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let outside = working.classify("../elsewhere/secret");
    let invocation =
        Invocation::on_path(ToolName::FsRead, &outside).expect("fs.read addresses a path");

    let mut complaints = Vec::new();
    let mut rendered = Vec::new();

    for mode in Mode::ALL {
        let decision = Decision::reach(mode, &invocation, Assessment::default());
        let line = decision.entry().render();

        // Clause one, for the mode the record singles out.
        if mode == Mode::Yolo && decision.requirement() != Requirement::Proceed {
            complaints.push(format!(
                "`yolo` raised {:?} for an out-of-tree read; D3 says it has no prompts",
                decision.requirement()
            ));
        }
        // Clause two, at every mode.
        if !decision.entry().is_out_of_tree() {
            complaints.push(format!(
                "at {mode} the entry does not record that the call left the tree"
            ));
        }
        if !line.contains("OUTSIDE the working directory") {
            complaints.push(format!(
                "at {mode} the rendered entry carries no out-of-tree marking: {line:?}"
            ));
        }
        rendered.push(line);
    }

    if rendered.iter().any(|line| *line != rendered[0]) {
        complaints.push(format!(
            "the record differs by mode, so a mode removed part of it: {rendered:?}"
        ));
    }

    assert!(
        complaints.is_empty(),
        "ADR-0011 D4: \"Mode may remove the prompt; it never removes the record.\" {} clause(s) \
         failed:\n  {}",
        complaints.len(),
        complaints.join("\n  ")
    );

    // And the prompt really was removed at `yolo` while kept at `ask`, so the
    // check is about a difference rather than about nothing changing.
    assert_eq!(
        Decision::reach(Mode::Ask, &invocation, Assessment::default()).requirement(),
        Requirement::Ask,
        "`ask` must prompt for this call, or \"mode may remove the prompt\" is untested"
    );
}

/// An out-of-tree call renders differently from an in-tree one — ADR-0011 D4.
///
/// Both arms: the marking is present on one and absent on the other. An
/// assertion that only looked for the marking would be satisfied by a
/// renderer that marked everything.
#[test]
fn an_out_of_tree_call_renders_differently_from_an_ordinary_one() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let inside = working.classify("inside/file");
    let outside = working.classify("../elsewhere/secret");

    let ordinary = Decision::reach(
        Mode::Yolo,
        &Invocation::on_path(ToolName::FsRead, &inside).expect("addresses a path"),
        Assessment::default(),
    );
    let escaping = Decision::reach(
        Mode::Yolo,
        &Invocation::on_path(ToolName::FsRead, &outside).expect("addresses a path"),
        Assessment::default(),
    );

    let ordinary_line = ordinary.entry().render();
    let escaping_line = escaping.entry().render();

    assert!(
        !ordinary_line.contains("OUTSIDE the working directory"),
        "an ordinary in-tree call was marked as having left the tree: {ordinary_line:?}"
    );
    assert!(
        escaping_line.contains("OUTSIDE the working directory"),
        "a call outside the working directory was not marked: {escaping_line:?}"
    );
    assert_ne!(
        ordinary_line, escaping_line,
        "ADR-0011 D4 requires out-of-tree access to render differently in the transcript"
    );
    assert!(
        ordinary_line.starts_with("fs.read "),
        "the entry does not name the tool that was called: {ordinary_line:?}"
    );
}

/// ADR-0011 D6 annotates and raises prominence, and never vetoes.
///
/// D6: "It does not veto." The mutant is a `Requirement` that changes when
/// the matcher fires — which is what an implementer reaches for when a
/// pattern list feels like it should stop something.
#[test]
fn a_destructive_match_annotates_and_raises_prominence_and_never_vetoes() {
    // No working directory is staged: a command line is not measured against
    // one, which is the whole of the 2026-09-05 correction.
    let line = CommandLine::split("rm -rf inside").expect("a command line");
    let invocation = Invocation::running(&line);

    let quiet = Decision::assess(
        Mode::Yolo,
        &invocation,
        &StagedAllowlist::empty(),
        &StagedDestructive::quiet(),
    );
    let matched = Decision::assess(
        Mode::Yolo,
        &invocation,
        &StagedAllowlist::empty(),
        &StagedDestructive::matching(),
    );

    assert_eq!(
        matched.requirement(),
        quiet.requirement(),
        "a destructive match changed what the harness requires; ADR-0011 D6: \"It does not veto.\""
    );
    assert!(
        matched.entry().is_destructive() && !quiet.entry().is_destructive(),
        "the annotation does not distinguish a match from a non-match"
    );
    assert!(
        matched.entry().render().contains(DESTRUCTIVE_MARKING),
        "D6 requires the transcript entry be annotated: {:?}",
        matched.entry().render()
    );
    assert!(
        !quiet.entry().render().contains(DESTRUCTIVE_MARKING),
        "an unmatched call was annotated anyway: {:?}",
        quiet.entry().render()
    );

    // Prominence reaches the prompt, at a mode where there is one.
    let asked = Decision::assess(
        Mode::Ask,
        &invocation,
        &StagedAllowlist::empty(),
        &StagedDestructive::matching(),
    );
    let question = asked
        .question()
        .expect("a command at `ask` is prompted for");
    assert!(
        question.prominent,
        "D6 raises the prompt's prominence and the question does not carry it"
    );
    assert!(
        !Decision::assess(
            Mode::Ask,
            &invocation,
            &StagedAllowlist::empty(),
            &StagedDestructive::quiet(),
        )
        .question()
        .expect("a command at `ask` is prompted for")
        .prominent,
        "an unmatched call raised the prompt's prominence anyway"
    );
}

/// The prompt and the transcript describe one call one way.
///
/// Both are rendered through `TranscriptEntry::render`, so a user who is told
/// one thing and a transcript that records another is not a state this code
/// can reach. The mutant: composing the question's sentence separately.
#[test]
fn the_prompt_states_what_the_transcript_will_record() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let outside = working.classify("../elsewhere/secret");
    let invocation =
        Invocation::on_path(ToolName::FsWrite, &outside).expect("fs.write addresses a path");

    let decision = Decision::assess(
        Mode::Ask,
        &invocation,
        &StagedAllowlist::empty(),
        &StagedDestructive::matching(),
    );
    let question = decision.question().expect("this call is prompted for");
    let line = decision.entry().render();

    assert!(
        question.statement.contains(&line),
        "the prompt does not state what the transcript will record.\n  prompt: {:?}\n  entry:  \
         {line:?}",
        question.statement
    );
    assert!(
        question.statement.contains("OUTSIDE the working directory")
            && question.statement.contains(DESTRUCTIVE_MARKING),
        "the prompt drops a marking the entry carries: {:?}",
        question.statement
    );

    // The confirmer is handed exactly that sentence, rather than composing one.
    let confirmer = RecordedConfirmer::accepting();
    let _ = decision.permit(Some(&confirmer));
    assert_eq!(
        confirmer.asked(),
        vec![question],
        "the confirmer was asked something other than the decision's own question"
    );
}

/// Both ports are asked about the whole call, not about a name.
///
/// "Pre-approved" is a property of the tool and its target together: a user
/// who approved reading one path has said nothing about running a command.
#[test]
fn the_allowlist_is_asked_about_the_tool_and_its_target_together() {
    // No working directory is staged: a command line is not measured against
    // one, which is the whole of the 2026-09-05 correction.
    let line = CommandLine::split("printf inside/file").expect("a command line");
    let invocation = Invocation::running(&line);
    let allowlist = StagedAllowlist::approving();

    let _ = Decision::assess(
        Mode::Allow,
        &invocation,
        &allowlist,
        &StagedDestructive::quiet(),
    );

    let asked = allowlist.asked();
    assert_eq!(
        asked.len(),
        1,
        "the allowlist was asked {} times",
        asked.len()
    );
    assert!(
        asked[0].contains("cmd.run"),
        "the allowlist was not told which tool: {:?}",
        asked[0]
    );
    assert!(
        asked[0].contains("inside/file"),
        "the allowlist was not told the target: {:?}",
        asked[0]
    );
}

/// Two built-ins do not address a path, and a path tool is neither of them.
///
/// ADR-0011 D4's boundary is about paths. Modelling a URL as one would make
/// the classifier answer a question it has no rule for; no record defines a
/// boundary for outbound destinations, and inventing one would be authoring a
/// security vocabulary.
#[test]
fn two_built_ins_do_not_address_a_path_and_carry_no_placement() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");

    let refusal = Invocation::on_path(ToolName::WebFetch, &target)
        .expect_err("web.fetch does not address a filesystem path");
    assert!(
        refusal.to_string().contains("`web.fetch` addresses a URL"),
        "the refusal does not say why: {refusal}"
    );

    let url = crate::web::RequestedUrl::parse("https://example.invalid/thing")
        .expect("staging: a https URL parses");
    let fetch = Invocation::fetching(&url);
    assert_eq!(fetch.tool(), ToolName::WebFetch);
    assert_eq!(
        fetch.placement(),
        None,
        "a URL has no placement against the working directory, and reporting one would be a rule \
         no record states"
    );

    let line = CommandLine::split("printf hello").expect("a command line");
    let command = Invocation::running(&line);
    assert_eq!(command.tool(), ToolName::CmdRun);
    assert_eq!(
        command.placement(),
        None,
        "a command line has no placement against the working directory: its boundary is the \
         directory it is started in, which `Spawn` fixes at D4's root"
    );
    assert_eq!(
        command.subject_text(),
        "printf hello",
        "a command's transcript subject is the command line itself, which is what makes \
         ADR-0010's \"a rendered `cmd.run` line is a command line\" true"
    );
    assert!(
        Invocation::on_path(ToolName::CmdRun, &target).is_err(),
        "a command line was accepted as a path, which is the classification corrected on \
         2026-09-05: D4 measures the paths a tool addresses, and a command addresses none"
    );

    // `fs.search` addresses a path -- D4 applies to where it looks -- and is
    // not described by one alone, because it carries a root and a needle.
    // Added 2026-09-05 with the wire contract: the two questions are
    // different, and `subject_kind` is the one that decides the constructor.
    assert!(
        ToolName::FsSearch.addresses_a_path(),
        "D4 applies to a search's root exactly as it applies to a read's target"
    );
    assert!(
        Invocation::on_path(ToolName::FsSearch, &target).is_err(),
        "a search is not described by a bare path: a record that said only where it looked would \
         not carry its arguments, which is ADR-0011 clause 1's second half"
    );
    let search = Invocation::searching(&target, "need\"le");
    assert_eq!(search.tool(), ToolName::FsSearch);
    assert_eq!(
        search.placement(),
        Some(target.placement()),
        "a search's placement is its root's; D4 is not suspended because the call carries a \
         second argument"
    );
    let rendered = search.subject_text();
    assert!(
        rendered.starts_with(&target.resolved().display().to_string())
            && rendered.contains("need\\\"le"),
        "a search's transcript subject carries the root and the needle, the needle quoted so one \
         holding a space or a quote cannot be read as part of the path: {rendered}"
    );

    // The arm that discriminates: the four addressed BY a bare path still are.
    for tool in ToolName::ALL {
        if matches!(
            tool,
            ToolName::WebFetch | ToolName::CmdRun | ToolName::FsSearch
        ) {
            continue;
        }
        assert!(
            Invocation::on_path(tool, &target).is_ok(),
            "{tool} is addressed by a path and was refused one"
        );
    }
}

/// An output budget of zero is refused, and a budget of one is not.
///
/// The second arm is what stops "refuse everything" from passing.
#[test]
fn an_output_budget_of_zero_is_refused() {
    assert_eq!(OutputBudget::new(0), Err(BudgetIsZero));
    let rendered = BudgetIsZero.to_string();
    assert!(
        rendered.contains("head and the tail") && rendered.contains("mark the elision"),
        "the refusal does not say what a zero budget cannot do: {rendered:?}"
    );
    assert_eq!(
        OutputBudget::new(1).map(OutputBudget::get),
        Ok(1),
        "a budget of one byte is a budget somebody chose and must be taken"
    );
}

/// **Corpus case 5 — output is truncated head and tail, and the elision is
/// marked.**
///
/// ADR-0011 D5: "truncated head-and-tail with the elision marked... A
/// truncation the user cannot notice is how a diagnosis gets built on a
/// fragment."
///
/// The mutant this catches is the naive `String::truncate`, which keeps the
/// head and drops the tail — so the tail sentinel is what discriminates. The
/// boundary is staged at one under, exactly at, and one over the budget, so
/// an off-by-one that marks an elision that did not happen reddens too.
#[test]
fn output_is_truncated_head_and_tail_with_the_elision_marked() {
    let budget = OutputBudget::new(32).expect("a non-zero budget");
    let head = "HEADSENTINEL";
    let tail = "TAILSENTINEL";
    let long = format!("{head}{}{tail}", "MIDDLE".repeat(200));
    let captured = Captured {
        exit_code: 0,
        stdout: long.clone(),
        stderr: String::new(),
    };
    let base = std::env::temp_dir().join(nonce("ts-overflow"));
    let mut sink = ScratchOverflow::in_directory(base.clone());
    let shown = captured
        .present(budget, &HeldSecrets::none(), Some(&mut sink))
        .expect("a sink was supplied");
    let text = shown.stdout.as_str();

    let mut complaints = Vec::new();
    if !text.starts_with(head) {
        complaints.push(format!("the head of the output was dropped: {text:?}"));
    }
    if !text.ends_with(tail) {
        complaints.push(format!(
            "the tail was dropped, which is what a plain truncation does: {text:?}"
        ));
    }
    if !text.contains(ELISION_PREFIX) {
        complaints.push(format!("the elision was not marked: {text:?}"));
    }
    if text.contains("MIDDLEMIDDLE") {
        complaints.push(format!("nothing was actually elided: {text:?}"));
    }
    if shown.stdout.elided_bytes().is_none() {
        complaints.push("the excerpt does not report how much went".to_owned());
    }
    assert!(
        complaints.is_empty(),
        "ADR-0011 D5's truncation failed {} clause(s):\n  {}",
        complaints.len(),
        complaints.join("\n  ")
    );

    // Text at and under the budget is carried byte-for-byte with no marker;
    // one byte over is marked. An off-by-one shows up here and nowhere else.
    for length in [31_usize, 32, 33] {
        let body = "x".repeat(length);
        let mut sink = ScratchOverflow::in_directory(base.clone());
        let shown = Captured {
            exit_code: 0,
            stdout: body.clone(),
            stderr: String::new(),
        }
        .present(budget, &HeldSecrets::none(), Some(&mut sink))
        .expect("a sink was supplied");
        if length <= 32 {
            assert_eq!(
                shown.stdout.as_str(),
                body,
                "{length} bytes fits a 32-byte budget and must be carried unchanged"
            );
            assert!(
                !shown.stdout.was_truncated(),
                "{length} bytes fits and must not be marked as elided"
            );
            assert_eq!(
                shown.full_text_at, None,
                "{length} bytes fits, so nothing was elided and no overflow file is owed"
            );
        } else {
            assert!(
                shown.stdout.was_truncated(),
                "{length} bytes exceeds a 32-byte budget and must be marked"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// The two streams are carried separately and neither leaks into the other.
///
/// ADR-0011 D5: "Stdout and stderr are captured separately, both surfaced".
/// Each carries its own nonce, so a merged capture is visible rather than
/// plausible.
#[test]
fn both_streams_are_carried_separately_and_neither_leaks_into_the_other() {
    let out = nonce("stdout");
    let err = nonce("stderr");
    let captured = Captured {
        exit_code: 3,
        stdout: out.clone(),
        stderr: err.clone(),
    };
    let shown = captured
        .present(
            OutputBudget::new(4096).expect("a non-zero budget"),
            &HeldSecrets::none(),
            None,
        )
        .expect("nothing is elided within this budget");

    assert_eq!(
        shown.stdout.as_str(),
        out,
        "standard output did not reach the caller as it was captured"
    );
    assert_eq!(
        shown.stderr.as_str(),
        err,
        "standard error did not reach the caller as it was captured"
    );
    assert!(
        !shown.stdout.as_str().contains(&err),
        "standard error leaked into standard output"
    );
    assert!(
        !shown.stderr.as_str().contains(&out),
        "standard output leaked into standard error"
    );
    assert_eq!(
        shown.exit_code, 3,
        "the exit code the call reported was not carried"
    );
}

/// **Corpus case 5, second arm — output nobody can preserve is refused, not
/// clipped.**
///
/// D5 promises the whole output survives where the user can read it. The
/// session directory that would hold it is ADR-0010's and does not exist, so
/// a capture that overflows with no sink is refused — the same refusal, for
/// the same reason, this crate already makes for ADR-0007 D8's apex
/// confirmation.
///
/// Three arms: no sink refuses, a refusing sink refuses differently, and a
/// working sink succeeds and reports the path. Without the third, "refuse
/// always" passes.
#[test]
fn output_that_overflows_with_nowhere_to_keep_it_is_refused_rather_than_clipped() {
    let budget = OutputBudget::new(16).expect("a non-zero budget");
    let captured = Captured {
        exit_code: 0,
        stdout: "x".repeat(4096),
        stderr: String::new(),
    };

    let refusal = captured
        .present(budget, &HeldSecrets::none(), None)
        .expect_err("nothing can hold the rest of this output");
    assert!(
        matches!(
            refusal,
            PresentationRefused::ThereWasNowhereToKeepTheRest { .. }
        ),
        "refused for the wrong reason: {refusal:?}"
    );
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("a truncation the user cannot notice"),
        "the refusal does not give D5's reason: {rendered:?}"
    );

    let mut refusing = RefusingOverflow;
    let not_preserved = captured
        .present(budget, &HeldSecrets::none(), Some(&mut refusing))
        .expect_err("the sink refused");
    assert!(
        matches!(not_preserved, PresentationRefused::NotPreserved(_)),
        "a sink that refused was reported as no sink at all: {not_preserved:?}"
    );

    let base = std::env::temp_dir().join(nonce("ts-overflow"));
    let mut sink = ScratchOverflow::in_directory(base.clone());
    let shown = captured
        .present(budget, &HeldSecrets::none(), Some(&mut sink))
        .expect("a working sink preserves it");
    let path = shown
        .full_text_at
        .clone()
        .expect("D5 requires the path be shown when anything was elided");
    assert_eq!(
        sink.written(),
        vec![path.clone()],
        "the path shown to the caller is not the path the sink wrote"
    );
    let preserved = std::fs::read_to_string(&path).expect("the sink wrote the file it named");
    assert!(
        preserved.contains(&captured.stdout),
        "the preserved file does not carry the whole output it exists to keep"
    );
    assert!(
        shown.stdout.as_str().len() < captured.stdout.len(),
        "nothing was actually truncated, so this check says nothing about overflow"
    );
    let _ = std::fs::remove_dir_all(&base);

    // And the arm that discriminates: within budget, no sink is needed at all.
    assert!(
        Captured {
            exit_code: 0,
            stdout: "short".to_owned(),
            stderr: String::new(),
        }
        .present(budget, &HeldSecrets::none(), None)
        .is_ok(),
        "output within the budget needs no overflow sink"
    );
}

/// Tool output reaches the caller byte for byte.
///
/// This is the check that keeps the identity seam on the live path. ADR-0008's
/// trigger clause 6 — secret redaction in failure text — is open, and this
/// arc adds no redaction; the seam is one named function and nothing passes
/// behaviour through it. The nonce carries a newline, a combining mark and an
/// astral-plane character, so an implementation that normalised, escaped or
/// cut on a byte boundary could not produce it.
#[test]
fn tool_output_reaches_the_caller_byte_for_byte() {
    // Deliberately awkward in every direction a transformation could tidy:
    // leading and trailing whitespace for a `trim`, an embedded newline for a
    // line-wise reader, a combining mark for a normaliser, and an
    // astral-plane character for anything cutting on a byte boundary. A
    // fixture with no trailing whitespace let a `trim_end` through this check
    // once — Verification lessons §9, met from the direction that a fixture is
    // awkward but not awkward enough.
    let awkward = format!("  {}\nline two\u{301}\u{1f701}  \n", nonce("tool-output"));
    let captured = Captured {
        exit_code: 0,
        stdout: awkward.clone(),
        stderr: awkward.clone(),
    };
    let shown = captured
        .present(
            OutputBudget::new(4096).expect("a non-zero budget"),
            &HeldSecrets::none(),
            None,
        )
        .expect("nothing is elided within this budget");

    assert_eq!(
        shown.stdout.as_str().as_bytes(),
        awkward.as_bytes(),
        "standard output did not reach the caller byte for byte"
    );
    assert_eq!(
        shown.stderr.as_str().as_bytes(),
        awkward.as_bytes(),
        "standard error did not reach the caller byte for byte"
    );
}

/// Where an allowlist check builds its resolution from.
///
/// A caller's schema, declaring [`allowlist::KEY`] **free at every layer** —
/// which is deliberately *not* what the product declares. It is the only way
/// to reach [`Allowed::from_configuration`]'s own escalation arm at all: the
/// product's [`allowlist::field`] makes the fold refuse a project layer
/// first, so a resolution carrying a project allowlist cannot otherwise
/// exist. That is the point of the two arms being independent, and it is why
/// this helper does not call `cli::layers::schema` ([Verification lessons]
/// §11: one arm of a comparison must not travel through the thing being
/// checked).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn permissive_schema() -> Schema {
    Schema::new().with(allowlist::key(), Field::free(FieldKind::Array))
}

/// A resolution in which one layer set the allowlist to `entries`.
fn allowlist_from(layer: Layer, entries: &[&str]) -> Resolution {
    let mut document = Table::new();
    document.insert_path(
        &allowlist::key(),
        Value::Array(
            entries
                .iter()
                .map(|entry| Value::Text((*entry).to_owned()))
                .collect(),
        ),
    );
    Resolution::resolve(
        &permissive_schema(),
        [Contribution::new(
            layer,
            Source::named(format!("{} (staged)", layer.label())),
            document,
        )],
    )
    .expect("a permissive schema takes an array at any layer")
}

/// **ADR-0011 D3's key is declared once, holds a list, and is refused to a
/// project.**
///
/// The declaration is asked of the record's own [`allowlist::field`] and the
/// binary's own [`crate::cli::layers::schema`], so this is the key a user
/// actually writes rather than a spelling retyped here.
///
/// Three mutants. Dropping `tools::allowlist::declare` from `schema()` fails
/// the first assertion. Declaring it [`FieldKind::Text`] fails the second —
/// and that one matters, because text would let layers 4 and 5 set a grant
/// through a separator nobody decided. Declaring it `Free` or `LowerOnly`
/// fails the third, which is ADR-0014 D6's sixth escalation.
#[test]
fn the_allowlist_key_is_declared_once_holds_a_list_and_is_refused_to_a_project() {
    let schema = crate::cli::layers::schema();
    let key = allowlist::key();

    assert_eq!(
        key.as_str(),
        "tools.allowlist",
        "the key ADR-0011 D3's allowlist is read from moved; it is user-facing and is spelled in \
         the record"
    );

    let field = schema
        .field(&key)
        .expect("the binary's schema must declare ADR-0011 D3's allowlist key");

    assert_eq!(
        field.kind,
        FieldKind::Array,
        "the allowlist must hold a list: ADR-0014 D2 replaces a list wholesale, which is what lets \
         a user say \"exactly these and nothing inherited\" about a permission grant"
    );

    match &field.project {
        ProjectPolicy::Refused { reason } => assert_eq!(
            reason,
            allowlist::PROJECT_REFUSAL,
            "the fold's reason and the module's reason must be one string, or a user gets two \
             different explanations of one rule"
        ),
        other => panic!(
            "ADR-0014 D6's sixth escalation must refuse the project layer outright, and the \
             policy is {other:?}"
        ),
    }
}

/// **`tools.mode` is declared once, holds text, and is refused to a project.**
///
/// The mirror of the check above, for [ADR-0011] D3's other key — the one
/// that arrived on 2026-09-05 and gave [ADR-0014] D6's **first** escalation
/// the name it had been waiting for since the record was written.
///
/// The declaration is asked of [`mode::field`] and the binary's own
/// [`crate::cli::layers::schema`], so this is the key a user actually writes.
///
/// Three mutants. Dropping `tools::mode::declare` from `schema()` fails the
/// first assertion. Declaring it [`FieldKind::Array`] fails the second, which
/// would put the mode out of reach of layers 4 and 5 — the two that supply
/// text and the two that make `ZARU_TOOLS_MODE` and `--mode` work at all.
/// Declaring it `Free` or `LowerOnly` fails the third: `LowerOnly` is defined
/// on whole numbers and would need an ordering over `ask`, `allow` and `yolo`
/// that no record states.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn the_mode_key_is_declared_once_holds_text_and_is_refused_to_a_project() {
    let schema = crate::cli::layers::schema();
    let key = mode::key();

    assert_eq!(
        key.as_str(),
        "tools.mode",
        "the key ADR-0011 D3's permission mode is read from moved; it is user-facing and is \
         spelled in the record"
    );

    let field = schema
        .field(&key)
        .expect("the binary's schema must declare ADR-0011 D3's permission mode key");

    assert_eq!(
        field.kind,
        FieldKind::Text,
        "the mode must hold text: it is one of three words, and text is the one kind layers 4 and \
         5 can carry, which is what makes ZARU_TOOLS_MODE and --mode reach it at all"
    );

    match &field.project {
        ProjectPolicy::Refused { reason } => assert_eq!(
            reason,
            mode::PROJECT_REFUSAL,
            "the fold's reason and the module's reason must be one string, or a user gets two \
             different explanations of one rule"
        ),
        other => panic!(
            "ADR-0014 D6's first escalation must refuse the project layer outright, and the \
             policy is {other:?}"
        ),
    }
}

/// **A mode set in the user's own configuration reaches a real decision, and
/// an unset key is still `ask`.**
///
/// The accepting arm of ADR-0014 D6 for [ADR-0011] D3's mode key, and the
/// thing declaring the key was *for*: until 2026-09-05 the composition passed
/// `Mode::default()` and the record said in as many words that "a user who
/// wants `allow` or `yolo` has nowhere to say so from the terminal".
///
/// The prompting rule itself is checked exhaustively by
/// `the_prompting_rule_is_the_records_at_every_mode`, and this check does not
/// re-state it. What it asserts is the **path**: a value in layer 2 becomes
/// the `Mode` a [`Decision`] is reached at. So an allowlisted `cmd.run` needs
/// no prompt when the user wrote `allow`, and the *same call* with the key
/// unset does — which is what makes the first assertion mean something rather
/// than being satisfied by a rule that never prompts.
///
/// Two mutants. Returning `Mode::default()` from `from_configuration`
/// regardless of the layer fails the first. Returning `Mode::Allow` for an
/// unset key fails the second, and that is the one worth having: it is the
/// direction a mistake here goes, because it is the direction with fewer
/// prompts in it.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[test]
fn a_mode_the_user_configured_reaches_the_decision_and_an_unset_key_is_ask() {
    let command = CommandLine::split("cargo test").expect("a command line");
    let invocation = Invocation::running(&command);
    let allowlisted = Assessment {
        allowlisted: true,
        destructive: false,
    };

    let granted = Mode::from_configuration(&mode_from(Layer::User, "allow"))
        .expect("the user's own layer is not bound by D6's escalation ceiling");
    assert_eq!(
        granted,
        Mode::Allow,
        "layer 2's value is not the mode taken"
    );
    assert_eq!(
        Decision::reach(granted, &invocation, allowlisted).requirement(),
        Requirement::Proceed,
        "ADR-0011 D3: `allow` runs the user's allowlist without prompting, and the mode the user \
         wrote did not reach the decision"
    );

    // The discriminating sibling: the same call, the same allowlist, no key.
    let unset = Mode::from_configuration(
        &Resolution::resolve(&permissive_mode_schema(), Vec::new())
            .expect("an empty resolution folds"),
    )
    .expect("an unset key is not a refusal");
    assert_eq!(
        unset,
        Mode::Ask,
        "ADR-0011 D3's table says `ask` is the default, and layer 1 declares none so this is \
         `Mode::default`"
    );
    assert_eq!(
        Decision::reach(unset, &invocation, allowlisted).requirement(),
        Requirement::Ask,
        "with no key set the harness must still prompt before a command, or the first assertion \
         above is satisfied by a rule that never prompts"
    );
}

/// **A project may not set the permission mode, and the user may.**
///
/// The second of [`Mode::from_configuration`]'s two arms — the one the fold
/// does not run, reached by a caller holding a resolution built some other
/// way. The fold's own arm is
/// `the_mode_key_is_declared_once_holds_text_and_is_refused_to_a_project`,
/// and each reddens alone: this helper deliberately declares the key
/// [`Field::free`] so that a resolution *can* be built with the project layer
/// carrying it, which is what leaves this arm something to refuse
/// ([Verification lessons] §11: one arm of a comparison must not travel
/// through the thing being checked).
///
/// The accepting sibling is the same value at the user's layer, in the check
/// above. **The misspelling arm is not re-checked here**:
/// `from_configuration` delegates it to `Mode::from_layer`, whose sentence is
/// asserted over `Mode::ALL` by
/// `a_value_that_names_no_mode_is_refused_naming_the_three_that_exist`. What
/// this check does add for that path is the **key**: `from_configuration`
/// hands `mode::KEY` down, and a refusal naming some other key is one its
/// reader cannot act on.
///
/// The mutant is `from_configuration` handing `Layer::User` down instead of
/// the layer the resolution says was effective — which is the mistake this
/// function can actually make, since the ceiling test itself is
/// `from_layer`'s and is checked there.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_project_may_not_set_the_permission_mode_however_the_resolution_was_built() {
    let refusal = Mode::from_configuration(&mode_from(Layer::Project, "yolo"))
        .expect_err("ADR-0014 D6's first escalation");

    let ModeRefused::FromAClonedRepository { key, offered, .. } = &refusal else {
        panic!("expected D6's escalation refusal, got {refusal:?}");
    };
    assert_eq!(
        key,
        mode::KEY,
        "the refusal must name the key the user has to find"
    );
    assert_eq!(
        offered, "yolo",
        "the refusal must quote back what the project wrote"
    );

    let rendered = refusal.to_string();
    assert!(
        rendered.contains("more privilege than the user granted"),
        "D6's own sentence is what says why: {rendered:?}"
    );
}

/// A schema that lets any layer carry the mode, so both of
/// [`Mode::from_configuration`]'s arms have something to refuse.
///
/// Deliberately **not** `cli::layers::schema`: that one declares the key
/// `Refused`, so the fold would refuse a project's value before this module's
/// own arm was reached and the check would be measuring the fold twice.
fn permissive_mode_schema() -> Schema {
    Schema::new().with(mode::key(), Field::free(FieldKind::Text))
}

/// A resolution in which one layer set the mode to `value`.
fn mode_from(layer: Layer, value: &str) -> Resolution {
    let mut document = Table::new();
    document.insert_path(&mode::key(), Value::Text(value.to_owned()));
    Resolution::resolve(
        &permissive_mode_schema(),
        [Contribution::new(
            layer,
            Source::named(format!("{} (staged)", layer.label())),
            document,
        )],
    )
    .expect("a permissive schema takes text at any layer")
}

/// **The two keys under `[tools]` are siblings, and `tools` itself is not a
/// key.**
///
/// [Verification lessons] §62 is a leaf-and-branch collision: one name held
/// as both a value and a table, where the write order decides which survives
/// and nothing is reported. `tools::allowlist`'s module documentation said
/// "`tools.allowlist` is the only key under that table" until 2026-09-05,
/// which was true and is no longer; what makes §62 unreachable is not the
/// count but that **`tools` is declared by nothing**, and that is asserted
/// here rather than remembered in a comment.
///
/// The mutant is declaring `tools` itself.
#[test]
fn tools_is_a_table_and_never_a_key_however_many_keys_sit_under_it() {
    let schema = crate::cli::layers::schema();

    let table = allowlist::KEY
        .split_once('.')
        .expect("ADR-0011 D3's allowlist key is a dotted path")
        .0;
    assert_eq!(
        table,
        mode::KEY
            .split_once('.')
            .expect("ADR-0011 D3's mode key is a dotted path")
            .0,
        "both of ADR-0011 D3's keys are expected under one table"
    );

    let branch = crate::config::Key::new(table).expect("`tools` is a well-formed key");
    assert!(
        schema.field(&branch).is_none(),
        "`{table}` is declared as a key as well as a table, which is Verification lessons section 62: \
         one write order keeps the value and the other keeps the table, and nothing is reported"
    );

    for key in [allowlist::key(), mode::key()] {
        assert!(
            schema.field(&key).is_some(),
            "`{key}` sits under `{table}` and the schema does not declare it"
        );
    }
}

/// **Corpus case: an approved pair approves that pair and nothing else.**
///
/// The approved entry is staged **third of five**, with entries on both sides
/// — [Verification lessons] §54: "never stage that member first or last",
/// because *the one that matters*, *the first* and *the last* are different
/// rules that agree whenever the interesting element is at an end.
///
/// Four mutants, and each is a rule somebody could plausibly write.
/// Comparing only the target passes an `fs.read` grant off as a `cmd.run`
/// one. Comparing only the tool approves every path once one is approved.
/// `starts_with` instead of `==` is the glob arriving without anybody calling
/// it one, and the fixture carries a target that is a **prefix of another**
/// so that it reddens. Taking the first or the last entry is what the middle
/// staging kills.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn an_approved_pair_approves_that_pair_and_nothing_else() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");

    let approved = working.classify("inside/file");
    let sibling = working.classify("inside/other");
    // A path whose text extends the approved one's, so a prefix match is not
    // the same rule as an equality match.
    let extending = working.classify("inside/file-and-more");

    let entry = format!("fs.read {}", approved.resolved().display());
    let allowed = Allowed::from_configuration(&allowlist_from(
        Layer::User,
        &[
            "fs.list /somewhere/before",
            "cmd.run true",
            // The interesting element, third of five.
            &entry,
            "cmd.run false",
            "fs.list /somewhere/after",
        ],
    ))
    .expect("the staged entries are well formed");

    assert_eq!(
        allowed.len(),
        5,
        "staging: five entries must have been taken, or the positional argument below is not the \
         one this check is named for"
    );

    let read_approved =
        Invocation::on_path(ToolName::FsRead, &approved).expect("fs.read addresses a path");
    assert!(
        allowed.approves(&read_approved),
        "the approved pair was not approved; every assertion below is then vacuous"
    );

    let read_sibling =
        Invocation::on_path(ToolName::FsRead, &sibling).expect("fs.read addresses a path");
    assert!(
        !allowed.approves(&read_sibling),
        "a different target was approved: {:?} was granted on the strength of {:?}",
        sibling.resolved(),
        approved.resolved()
    );

    let read_extending =
        Invocation::on_path(ToolName::FsRead, &extending).expect("fs.read addresses a path");
    assert!(
        !allowed.approves(&read_extending),
        "a target whose text merely extends the approved one was approved: {:?} on the strength \
         of {:?}. ADR-0011 D3's allowlist matches byte for byte and never by prefix",
        extending.resolved(),
        approved.resolved()
    );

    let write_approved =
        Invocation::on_path(ToolName::FsWrite, &approved).expect("fs.write addresses a path");
    assert!(
        !allowed.approves(&write_approved),
        "a different tool was approved on the same target: approving a read of {:?} says nothing \
         about writing it",
        approved.resolved()
    );
}

/// **Corpus case, the second arm — a project's allowlist is refused by the
/// constructor even where a fold let it through.**
///
/// The fold is the first arm and is checked over real files in
/// `tests/permission_from_outside.rs`. This is the arm that holds for a
/// caller who built a resolution some other way, and it is reached here
/// through a permissive schema for exactly that reason.
///
/// The accepting sibling is the same value, the same entries, from
/// [`Layer::User`] — without it a constructor that refused every layer would
/// pass. The mutant is flipping
/// [`Layer::bound_by_the_escalation_ceiling`](crate::config::Layer::bound_by_the_escalation_ceiling)'s
/// `Project` arm, or deleting the check in `from_configuration`.
#[test]
fn a_project_allowlist_is_refused_by_the_constructor_and_the_users_is_not() {
    let entries = ["cmd.run cargo test"];

    let refusal = Allowed::from_configuration(&allowlist_from(Layer::Project, &entries))
        .expect_err("ADR-0014 D6: a cloned repository may not grant itself fewer prompts");
    assert_eq!(
        refusal,
        AllowlistRefused::FromAClonedRepository {
            layer: Layer::Project
        },
        "the project layer must be refused for being the project layer"
    );
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("tools.allowlist") && rendered.contains(allowlist::PROJECT_REFUSAL),
        "ADR-0014 D6 requires the error name the key and the reason: {rendered:?}"
    );

    for granting in [Layer::BuiltIn, Layer::User] {
        let allowed = Allowed::from_configuration(&allowlist_from(granting, &entries))
            .unwrap_or_else(|refusal| {
                panic!("{} is the user's own grant: {refusal}", granting.label())
            });
        assert_eq!(
            allowed.len(),
            1,
            "{} supplied an entry and it did not survive",
            granting.label()
        );
    }
}

/// **An unset key is a grant of nothing, and it is not a failure.**
///
/// A machine with no `~/.zaru/config.toml` has no allowlist, and ADR-0011
/// D3's `allow` mode then prompts for everything — which is `ask`'s behaviour
/// and is the safe direction. The mutant is returning an error for an unset
/// key, which would make a fresh machine unable to start.
#[test]
fn an_unset_allowlist_is_a_grant_of_nothing_rather_than_a_refusal() {
    let resolution =
        Resolution::resolve(&permissive_schema(), []).expect("an empty resolution is a resolution");
    let allowed = Allowed::from_configuration(&resolution)
        .expect("no layer set the key, which is not an error");
    assert!(
        allowed.is_empty(),
        "an unset allowlist granted {} entries",
        allowed.len()
    );

    let line = CommandLine::split("rm -rf /").expect("a command line");
    assert!(
        !allowed.approves(&Invocation::running(&line)),
        "an empty allowlist approved something"
    );
}

/// **An entry that is not a line the prompt showed is refused, naming its
/// position and quoting what it held.**
///
/// Every arm has an accepting sibling in the table, so an implementation that
/// refused every entry cannot pass. The positions are deliberately not 1, so
/// a refusal hard-coding the first entry reddens.
///
/// The mutant for the tool arm is accepting any first word; for the target
/// arm, accepting an entry with nothing after the space.
#[test]
fn an_entry_that_is_not_a_line_the_prompt_showed_is_refused_naming_its_position() {
    let good = "fs.read /tmp/x";
    // Named rather than closured, so the table stays readable and the
    // expected reason is a literal rather than a predicate that could agree
    // with the implementation by construction.
    let cases = [
        ("", "EmptyEntry"),
        ("fs.read", "NoTarget"),
        ("fs.read ", "NoTarget"),
        ("fs.grep /tmp/x", "NoSuchTool"),
        ("FS.READ /tmp/x", "NoSuchTool"),
    ];

    let mut wrong = Vec::new();
    for (offered, expected) in cases {
        // The bad entry is second, so a refusal that only ever looks at the
        // first entry reddens, and the good entry on each side is what a
        // refuse-everything implementation fails on.
        match Allowed::from_configuration(&allowlist_from(Layer::User, &[good, offered, good])) {
            Ok(allowed) => wrong.push(format!(
                "{offered:?} was accepted, giving {} entries",
                allowed.len()
            )),
            Err(refusal) => {
                let (name, position) = match &refusal {
                    AllowlistRefused::EmptyEntry { position } => ("EmptyEntry", *position),
                    AllowlistRefused::NoTarget { position, .. } => ("NoTarget", *position),
                    AllowlistRefused::NoSuchTool { position, .. } => ("NoSuchTool", *position),
                    AllowlistRefused::EntryWrongShape { position, .. } => {
                        ("EntryWrongShape", *position)
                    }
                    AllowlistRefused::FromAClonedRepository { .. } => ("FromAClonedRepository", 0),
                    AllowlistRefused::WrongShape { .. } => ("WrongShape", 0),
                };
                if name != expected {
                    wrong.push(format!(
                        "{offered:?} was refused as {name} and not {expected}"
                    ));
                }
                if position != 2 {
                    wrong.push(format!(
                        "{offered:?} was refused at position {position} and it is the second entry"
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("; "));

    // The accepting arm: three good entries are three good entries.
    let allowed = Allowed::from_configuration(&allowlist_from(Layer::User, &[good, good, good]))
        .expect("a well-formed entry must be taken");
    assert_eq!(allowed.len(), 3, "well-formed entries did not survive");
}

/// **An entry is the line ADR-0011 D3's prompt showed, and that is checkable
/// rather than asserted in prose.**
///
/// The expected entry is built from [`Decision::question`]'s own statement by
/// stripping the `Allow ` and the `?` the question adds — so if the prompt's
/// wording and the allowlist's grammar ever drift apart, this reddens. It is
/// the one place the two are compared, and the comparison's other arm is the
/// user's own configuration string.
///
/// The mutant is changing either side's rendering: `Entry::parse` splitting
/// on something other than the first space, or `question` rendering the tool
/// and the subject in the other order.
#[test]
fn an_allowlist_entry_is_the_line_the_prompt_showed() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::FsWrite, &target).expect("fs.write addresses a path");

    let decision = Decision::reach(Mode::Ask, &invocation, Assessment::default());
    let question = decision
        .question()
        .expect("staging: a write at `ask` must raise a question");

    let shown = question
        .statement
        .strip_prefix("Allow ")
        .and_then(|rest| rest.strip_suffix('?'))
        .expect("the question is `Allow {line}?`");

    let entry = Entry::parse(1, shown).unwrap_or_else(|refusal| {
        panic!("the line the prompt showed is not an allowlist entry: {refusal}")
    });
    assert_eq!(entry.tool(), ToolName::FsWrite, "the tool did not survive");
    assert_eq!(
        entry.target(),
        invocation.subject_text(),
        "the target the prompt showed and the target the allowlist matches are different strings"
    );
    assert!(
        entry.approves(&invocation),
        "the entry taken from the prompt's own line does not approve the call it was shown for"
    );
}

/// **The allowlist reaches ADR-0011 D3's `allow` mode through the rule, not
/// past it.**
///
/// [Verification lessons] §25: a mechanism whose only callers are its own
/// checks is a mechanism nobody has been shown to reach. This drives the
/// product allowlist through [`Decision::assess`], which is the door the
/// executor uses.
///
/// The mutant is `Decision::reach` ignoring `assessment.allowlisted`, which
/// would make `allow` prompt for everything.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_product_allowlist_is_what_allow_mode_consults() {
    let line = CommandLine::split("cargo test").expect("a command line");
    let invocation = Invocation::running(&line);

    let allowed = Allowed::from_configuration(&allowlist_from(
        Layer::User,
        &["fs.list /elsewhere", "cmd.run cargo test", "fs.list /other"],
    ))
    .expect("the staged entries are well formed");

    let decision = Decision::assess(
        Mode::Allow,
        &invocation,
        &allowed,
        &StagedDestructive::quiet(),
    );
    assert_eq!(
        decision.requirement(),
        Requirement::Proceed,
        "ADR-0011 D3: `allow` \"Runs the allowlist without prompting\""
    );

    let other = CommandLine::split("cargo build").expect("a command line");
    let outside = Decision::assess(
        Mode::Allow,
        &Invocation::running(&other),
        &allowed,
        &StagedDestructive::quiet(),
    );
    assert_eq!(
        outside.requirement(),
        Requirement::Ask,
        "ADR-0011 D3: `allow` \"prompts for anything outside it\""
    );
}

/// **Corpus case: a destructive shape is surfaced and never vetoed, and the
/// shapes that are not destructive are not annotated.**
///
/// The accepting siblings are the load-bearing half — `rm` with no recursive
/// flag, `git` that is not a push, `git push` with no force flag — because a
/// matcher that annotated every command would satisfy the hostile arm alone.
///
/// The mutants: dropping the flag test from `RecursiveRemoval` (so any `rm`
/// matches); dropping the `push` test from `ForcePush` (so `git status -f`
/// matches); comparing the short cluster with `== "-r"` (so `-rf` escapes);
/// and any route from a match to a refusal, which does not compile because
/// `Requirement` has no such variant.
#[test]
fn d6s_two_transcribed_categories_match_their_shapes_and_nothing_beside_them() {
    let matcher = Shapes::new();

    let destructive = [
        ("rm -rf build", Category::RecursiveRemoval),
        ("rm -r build", Category::RecursiveRemoval),
        ("rm --recursive build", Category::RecursiveRemoval),
        ("rm -fR build", Category::RecursiveRemoval),
        ("/bin/rm -r build", Category::RecursiveRemoval),
        ("git push --force origin main", Category::ForcePush),
        ("git push -f origin main", Category::ForcePush),
        (
            "git push --force-with-lease origin main",
            Category::ForcePush,
        ),
        ("/usr/bin/git push -f origin main", Category::ForcePush),
    ];

    // Each accepting sibling differs from a hostile case by exactly the one
    // token the rule is about, so a matcher that ignored that token would
    // annotate it too.
    let ordinary = [
        "rm build/one",
        "rm -f build/one",
        "rm -- -r",
        "git push origin main",
        "git status -f",
        "git commit --force",
        "cargo test",
        "dd if=/dev/zero of=/dev/sda",
        "npm install -g typescript",
    ];

    let mut wrong = Vec::new();
    for (text, expected) in destructive {
        let line = CommandLine::split(text).expect("a command line");
        let invocation = Invocation::running(&line);
        match matcher.category(&invocation) {
            Some(found) if found == expected => {}
            Some(found) => wrong.push(format!("{text:?} matched {found} and not {expected}")),
            None => wrong.push(format!(
                "{text:?} matched nothing and D6 names it {expected}"
            )),
        }
    }
    for text in ordinary {
        let line = CommandLine::split(text).expect("a command line");
        let invocation = Invocation::running(&line);
        if let Some(found) = matcher.category(&invocation) {
            wrong.push(format!(
                "{text:?} was annotated as {found}, and ADR-0011's own reason for keeping the list \
                 short is that \"a prompt that cries wolf gets dismissed reflexively\""
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("; "));
}

/// **The two categories that name no program match nothing, and that is
/// pinned so adding one is a visible act.**
///
/// ADR-0011 D6 names four categories and no patterns. "Disk operations" and
/// "package-manager global installs" name no program, and turning either into
/// a matcher is authoring a security vocabulary. The commands below are the
/// ones an implementer would reach for first; **all of them must be
/// unannotated**, and this check is what makes adding any of them a change
/// somebody has to look at.
///
/// The mutant is giving either category one program — `dd`, or `npm` — which
/// reddens here immediately.
#[test]
fn the_two_categories_that_name_no_program_match_nothing() {
    let matcher = Shapes::new();

    assert_eq!(
        Category::ALL.len(),
        4,
        "ADR-0011 D6 names four categories; a fifth is the record author's"
    );
    for category in Category::ALL {
        assert_eq!(
            category.names_a_shape(),
            matches!(category, Category::RecursiveRemoval | Category::ForcePush),
            "{category} disagrees with itself about whether D6's words determine a shape"
        );
    }

    let unwritten = [
        "dd if=/dev/zero of=/dev/sda bs=1M",
        "mkfs.ext4 /dev/sda1",
        "fdisk /dev/sda",
        "parted /dev/sda mklabel gpt",
        "wipefs -a /dev/sda",
        "shred -u /dev/sda",
        "npm install -g typescript",
        "npm i --global typescript",
        "pip install --user nothing",
        "cargo install cargo-edit",
        "gem install rails",
        "go install example.com/tool@latest",
    ];

    let mut annotated = Vec::new();
    for text in unwritten {
        let line = CommandLine::split(text).expect("a command line");
        if matcher.is_destructive(&Invocation::running(&line)) {
            annotated.push(text);
        }
    }
    assert!(
        annotated.is_empty(),
        "ADR-0011 D6's third and fourth categories name no program, so this matcher writes none. \
         These are now annotated: {annotated:?}. If that is deliberate, it is a change to what the \
         harness tells a user is dangerous -- record it on ADR-0011 D6 and update this check, which \
         exists so the change is visible"
    );

    // The accepting arm: the two categories that do name a shape still match,
    // so an implementation that matched nothing at all would fail here.
    let recursive = CommandLine::split("rm -rf build").expect("a command line");
    assert!(
        matcher.is_destructive(&Invocation::running(&recursive)),
        "the matcher matched nothing at all, so the table above asserted nothing"
    );
}

/// **D6 is about commands, so a path and a URL are never annotated.**
///
/// All four of D6's categories are command shapes and its heading is
/// "Destructive **commands**". Making an `fs.write` destructive would be a
/// fifth category, which is this record's author's.
///
/// The mutant is answering on the rendered subject text rather than on the
/// command line — which is what a matcher built to the brief's original
/// shape would have done, and which annotates `fs.write /tmp/rm -rf` and
/// every path with `rm` in its name.
#[test]
fn a_path_and_a_url_are_never_destructive() {
    let matcher = Shapes::new();
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");

    // A path whose *text* carries every token the matcher looks for. A
    // matcher reading the subject text rather than the command line annotates
    // this one.
    let target = working.classify("inside/rm -rf --force");
    let write = Invocation::on_path(ToolName::FsWrite, &target).expect("fs.write addresses a path");
    assert!(
        !matcher.is_destructive(&write),
        "a filesystem path was annotated as a destructive command: {:?}",
        target.resolved()
    );

    let url = crate::web::RequestedUrl::parse("https://example.invalid/rm?args=-rf")
        .expect("staging: a https URL parses");
    let fetch = Invocation::fetching(&url);
    assert!(
        !matcher.is_destructive(&fetch),
        "a URL was annotated as a destructive command"
    );

    // The accepting arm: the same tokens, as an actual command line.
    let line = CommandLine::split("rm -rf inside").expect("a command line");
    assert!(
        matcher.is_destructive(&Invocation::running(&line)),
        "the matcher recognises nothing, so the two assertions above are vacuous"
    );
}

/// **The product matcher annotates and raises prominence, and there is
/// nowhere for it to veto.**
///
/// The sibling of `a_destructive_match_annotates_and_raises_prominence_and_never_vetoes`,
/// which asserts the same rule against a staged answer. This one drives the
/// **product** matcher through [`Decision::assess`], at `yolo` — the mode
/// with no prompt at all — so what is asserted is that D6's annotation
/// survives where D3 has removed the prompt.
///
/// The mutant is `Decision::reach` letting `assessment.destructive` reach
/// `requirement`, which is the veto D6 forbids; it does not compile as a
/// value, so the mutation is the assignment and this reddens.
#[test]
fn the_product_matcher_annotates_at_yolo_where_there_is_no_prompt_at_all() {
    let line = CommandLine::split("rm -rf build").expect("a command line");
    let invocation = Invocation::running(&line);

    let decision = Decision::assess(Mode::Yolo, &invocation, &Allowed::nothing(), &Shapes::new());

    assert_eq!(
        decision.requirement(),
        Requirement::Proceed,
        "ADR-0011 D6: \"It does not veto.\" D3's `yolo`: \"No prompts.\""
    );
    assert!(
        decision.question().is_none(),
        "a call at `yolo` raised a question"
    );
    assert!(
        decision.entry().is_destructive()
            && decision.entry().render().contains(DESTRUCTIVE_MARKING),
        "D6's annotation did not survive a mode that removes the prompt: {:?}",
        decision.entry().render()
    );

    // The accepting sibling at the same mode: an ordinary command carries no
    // annotation, so the assertion above is about the match and not about the
    // renderer.
    let ordinary = CommandLine::split("cargo test").expect("a command line");
    let quiet = Decision::assess(
        Mode::Yolo,
        &Invocation::running(&ordinary),
        &Allowed::nothing(),
        &Shapes::new(),
    );
    assert!(
        !quiet.entry().is_destructive() && !quiet.entry().render().contains(DESTRUCTIVE_MARKING),
        "an ordinary command was annotated: {:?}",
        quiet.entry().render()
    );
}

/// **Corpus case: an ask that could not reach the user refuses for that
/// reason, and never as a decline.**
///
/// Until 2026-09-05 a confirmer whose terminal had closed had only `false` to
/// answer with, and `false` is [`RefusedBecause::TheUserDeclined`] — a
/// transcript entry saying the user declined when nobody was asked anything.
/// The three refusals below must be **three different events**, and the two
/// that are the same event must be the same one.
///
/// The accepting sibling is the fourth arm: a confirmer that answers `y` must
/// grant, or an implementation that refused every call would satisfy the rest.
///
/// The mutant is mapping `Err` to `TheUserDeclined`, which collapses two of
/// the four rows and reddens here.
#[test]
fn an_ask_that_could_not_reach_the_user_is_not_a_decline() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::FsWrite, &target).expect("fs.write addresses a path");
    let decision = Decision::reach(Mode::Ask, &invocation, Assessment::default());

    assert_eq!(
        decision.requirement(),
        Requirement::Ask,
        "staging: a write at `ask` must need a prompt, or this check asserts nothing"
    );

    let failing = FailingConfirmer::new();
    assert_eq!(
        decision.permit(Some(&failing)),
        Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
        "an ask that could not be put must refuse as an ask that did not reach the user, and \
         never as a decline: the two are different events and a transcript that conflated them \
         would say the user answered a question nobody put to them"
    );
    assert_eq!(
        failing.asked(),
        1,
        "the confirmer was not actually asked, so the refusal came from somewhere else"
    );

    // The other three rows, so the assertion above is about `Err` and not
    // about refusing generally.
    assert_eq!(
        decision.permit(None),
        Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
        "no confirmer at all is the same event and must carry the same reason"
    );
    assert_eq!(
        decision.permit(Some(&RecordedConfirmer::declining())),
        Permission::Refused(RefusedBecause::TheUserDeclined),
        "a user who was asked and said no must refuse for that reason"
    );
    assert_eq!(
        decision.permit(Some(&RecordedConfirmer::accepting())),
        Permission::Granted,
        "a user who said yes must grant, or every row above is satisfied by refusing everything"
    );

    // The sentence a reader gets must not claim nobody was supplied, because
    // in the failing arm somebody was.
    let rendered = RefusedBecause::ThereWasNobodyToAsk.to_string();
    assert!(
        rendered.contains("did not reach"),
        "the refusal's own sentence must be true of both routes: {rendered:?}"
    );
}

/// **ADR-0011 D3's prompt is one line, and it is the line the transcript will
/// record.**
///
/// `zaru-tui`'s richer prompt reaches the same port with the same
/// [`Question`], so what is asserted here is that the plain one adds a `y/N`
/// suffix and **nothing else**: no second sentence, no re-derived annotation,
/// no separate prominence marker. D6's marking is inside the statement
/// already, which is why a destructive call's line carries it here without
/// this module knowing what D6 is.
///
/// The mutants: appending a sentence (the line stops being one line);
/// re-deriving the annotation (it appears twice); dropping the suffix (a user
/// cannot tell which way an empty answer goes).
#[test]
fn the_prompt_is_one_line_and_it_is_the_transcripts_own() {
    let destructive = CommandLine::split("rm -rf build").expect("a command line");
    let invocation = Invocation::running(&destructive);
    let decision = Decision::assess(Mode::Ask, &invocation, &Allowed::nothing(), &Shapes::new());
    let question = decision
        .question()
        .expect("staging: a command at `ask` must raise a question");

    let rendered = prompt::line(&question);
    assert_eq!(
        rendered,
        format!("{}{}", question.statement, prompt::SUFFIX),
        "the prompt writes the statement and the suffix, and nothing else"
    );
    assert_eq!(
        rendered.lines().count(),
        1,
        "the prompt is one line: {rendered:?}"
    );
    assert!(
        rendered.contains(&decision.entry().render()),
        "the prompt and the transcript must describe one call the same way: {rendered:?} against \
         {:?}",
        decision.entry().render()
    );
    assert_eq!(
        rendered.matches(DESTRUCTIVE_MARKING).count(),
        1,
        "D6's annotation is in the statement already; a prompt that re-derived it would show it \
         twice: {rendered:?}"
    );

    // The accepting sibling: an ordinary call's line carries no annotation, so
    // the count above is about the match rather than about the suffix.
    let ordinary = CommandLine::split("cargo test").expect("a command line");
    let quiet = Decision::assess(
        Mode::Ask,
        &Invocation::running(&ordinary),
        &Allowed::nothing(),
        &Shapes::new(),
    );
    let quiet_line = prompt::line(
        &quiet
            .question()
            .expect("a command at `ask` raises a question"),
    );
    assert!(
        !quiet_line.contains(DESTRUCTIVE_MARKING),
        "an ordinary command's prompt was annotated: {quiet_line:?}"
    );
}

/// **N is the default, and it is the default by being everything that is not
/// a yes.**
///
/// The accepting arms are the four spellings of yes; every other row is a no,
/// including the empty line, whitespace, end of input, and the words a user
/// might expect to work. A rule with one accepting shape cannot forget a
/// branch.
///
/// The mutants: returning `true` for an empty line (the default flips);
/// accepting any non-empty line; making the comparison case-sensitive, which
/// the `Y` and `YES` rows catch.
#[test]
fn n_is_the_default_and_only_a_yes_is_a_yes() {
    let yes = ["y", "Y", "yes", "YES", "Yes", " y ", "y\n", "yes\r\n"];
    let no = [
        "", " ", "\n", "n", "N", "no", "NO", "nope", "ye", "yess", "yeah", "1", "true", "ok",
    ];

    let mut wrong = Vec::new();
    for typed in yes {
        if !prompt::answer(Some(typed)) {
            wrong.push(format!("{typed:?} was read as no"));
        }
    }
    for typed in no {
        if prompt::answer(Some(typed)) {
            wrong.push(format!("{typed:?} was read as yes"));
        }
    }
    if prompt::answer(None) {
        wrong.push("end of input was read as yes".to_owned());
    }
    assert!(wrong.is_empty(), "{}", wrong.join("; "));
}

/// **The prompt's I/O, driven over real handles.**
///
/// [`prompt::ask`] is where the writing and the reading live, so it is
/// reachable with a real file and a real buffer; [`prompt::Prompt`] is that
/// plus the terminal gate and the locking. What this asserts is the bytes
/// that reached the output — read back out of the buffer rather than
/// re-rendered — and the answer that came back.
///
/// The mutants: not flushing (the buffer is empty when the answer is read);
/// writing the statement without the suffix; returning the answer to a
/// different question.
#[test]
fn the_prompt_writes_its_line_and_reads_the_answer_back() {
    let question = crate::tools::port::Question {
        statement: format!("Allow {}", fixtures::nonce("statement")),
        prominent: true,
    };

    for (typed, expected) in [("y\n", true), ("n\n", false), ("\n", false), ("", false)] {
        let mut input = typed.as_bytes();
        let mut output: Vec<u8> = Vec::new();
        let answered = prompt::ask(&mut input, &mut output, &question)
            .expect("a readable handle and a writable one cannot fail");
        assert_eq!(
            answered, expected,
            "{typed:?} was read as {answered} and it means {expected}"
        );
        let written = String::from_utf8(output).expect("the prompt writes text");
        assert_eq!(
            written,
            prompt::line(&question),
            "the bytes that reached the handle are not the line the prompt renders"
        );
        assert!(
            written.contains(&question.statement),
            "the statement the decision composed did not reach the user: {written:?}"
        );
    }
}

/// **Corpus case: no terminal is no confirmer, and the call is refused rather
/// than defaulted.**
///
/// The gate is [`IsTerminal`](std::io::IsTerminal), called by the product on
/// a **real** handle the check owns — a file in its own scratch tree, and a
/// pipe's read half. Neither is a fixture answering on the product's behalf.
///
/// A check cannot make a terminal, so the accepting end of this gate is not
/// exercised anywhere and is a `Not verified` line on the record. What *is*
/// exercised is the consequence, which is the half that matters: a caller
/// holding `None` refuses with `ThereWasNobodyToAsk`, and the accepting
/// sibling is the same decision with a confirmer that answers.
///
/// The mutant is `Prompt::over` skipping the `is_terminal` test, which would
/// let a redirected run answer "the user declined" from end of input in a
/// transcript no user was watching.
#[test]
fn a_prompt_without_a_terminal_is_no_confirmer_at_all() {
    let tree = ScratchTree::new();
    let path = tree.project().join("inside").join("answers");
    std::fs::write(&path, b"y\n").expect("staging: a file to answer from");
    let handle = std::fs::File::open(&path).expect("staging: the file opens");

    assert!(
        prompt::Prompt::over(handle, Vec::new()).is_none(),
        "a regular file is not a terminal, and a prompt over one would read `y` from a file the \
         user never typed into"
    );

    // The consequence, which is what the record actually requires.
    let target = tree.project().join("inside").join("file");
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let classified = working.classify(target.to_str().expect("a utf-8 path"));
    let invocation =
        Invocation::on_path(ToolName::FsWrite, &classified).expect("fs.write addresses a path");
    let decision = Decision::reach(Mode::Ask, &invocation, Assessment::default());
    assert_eq!(
        decision.requirement(),
        Requirement::Ask,
        "staging: a write at `ask` must need a prompt"
    );

    let confirmer: Option<&dyn crate::tools::port::Confirm> = None;
    assert_eq!(
        decision.permit(confirmer),
        Permission::Refused(RefusedBecause::ThereWasNobodyToAsk),
        "ADR-0011 D3: a confirmation nobody can answer is the silent default the record forbids"
    );
    assert_eq!(
        decision.permit(Some(&RecordedConfirmer::accepting())),
        Permission::Granted,
        "the accepting sibling: a confirmer that answers must grant, or the refusal above is what \
         this decision does to everything"
    );
}
