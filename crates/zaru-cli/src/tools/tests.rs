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

use crate::tools::decision::{
    Assessment, DESTRUCTIVE_MARKING, Decision, Invocation, Permission, RefusedBecause, Requirement,
};
use crate::tools::fixtures::{
    RecordedConfirmer, RefusingOverflow, ScratchOverflow, ScratchTree, StagedAllowlist,
    StagedDestructive, nonce,
};
use crate::tools::mode::{Layer, Mode, ModeRefused, Tier};
use crate::tools::name::{Effect, ToolName};
use crate::tools::notice::SessionNotice;
use crate::tools::output::{
    BudgetIsZero, Captured, ELISION_PREFIX, OutputBudget, PresentationRefused,
};
use crate::tools::tree::{Placement, WorkingDirectory};
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
            ToolName::CmdRun,
            true,
            false,
            Requirement::Proceed,
            "D4 removes the prompt at `yolo` and keeps the record; this row is the first half",
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

    let mut wrong = Vec::new();
    for (mode, tool, out_of_tree, allowlisted, expected, why) in &cases {
        let target = if *out_of_tree { &outside } else { &inside };
        let invocation = Invocation::on_path(*tool, target).expect("these tools address paths");
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
        15,
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
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::CmdRun, &target).expect("cmd.run addresses a path");

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
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");
    let invocation =
        Invocation::on_path(ToolName::CmdRun, &target).expect("cmd.run addresses a path");
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

/// `web.fetch` cannot be described as a call on a path, and a path tool
/// cannot be described as a fetch.
///
/// ADR-0011 D4's boundary is about paths. Modelling a URL as one would make
/// the classifier answer a question it has no rule for; no record defines a
/// boundary for outbound destinations, and inventing one would be authoring a
/// security vocabulary.
#[test]
fn a_url_is_not_a_path_and_carries_no_placement() {
    let tree = ScratchTree::new();
    let working = WorkingDirectory::at(tree.project()).expect("the project directory resolves");
    let target = working.classify("inside/file");

    let refusal = Invocation::on_path(ToolName::WebFetch, &target)
        .expect_err("web.fetch does not address a filesystem path");
    assert!(
        refusal.to_string().contains("`web.fetch` addresses a URL"),
        "the refusal does not say why: {refusal}"
    );

    let fetch = Invocation::fetching("https://example.invalid/thing");
    assert_eq!(fetch.tool(), ToolName::WebFetch);
    assert_eq!(
        fetch.placement(),
        None,
        "a URL has no placement against the working directory, and reporting one would be a rule \
         no record states"
    );

    // The arm that discriminates: every other built-in does take a path.
    for tool in ToolName::ALL {
        if tool == ToolName::WebFetch {
            continue;
        }
        assert!(
            Invocation::on_path(tool, &target).is_ok(),
            "{tool} addresses a path and was refused one"
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
        .present(budget, Some(&mut sink))
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
        .present(budget, Some(&mut sink))
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
        .present(OutputBudget::new(4096).expect("a non-zero budget"), None)
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
        .present(budget, None)
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
        .present(budget, Some(&mut refusing))
        .expect_err("the sink refused");
    assert!(
        matches!(not_preserved, PresentationRefused::NotPreserved(_)),
        "a sink that refused was reported as no sink at all: {not_preserved:?}"
    );

    let base = std::env::temp_dir().join(nonce("ts-overflow"));
    let mut sink = ScratchOverflow::in_directory(base.clone());
    let shown = captured
        .present(budget, Some(&mut sink))
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
        .present(budget, None)
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
        .present(OutputBudget::new(4096).expect("a non-zero budget"), None)
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
