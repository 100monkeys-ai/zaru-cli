// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The path corpus's own checks, and the two security-corpus pairs.
//!
//! Every pair here is a refusal beside an **accepting sibling**, because a
//! corpus that offered nothing at all would pass every refusal on this page
//! and would be the worst possible way to satisfy them.

use super::{NOTHING_TO_OFFER, ProjectPaths, WALK_CEILING};
use crate::tools::WorkingDirectory;
use crate::tools::fixtures::ScratchTree;
use zaru_tui::composer::{MATCH_LIMIT, PathEntry, Paths};

/// Everything the corpus offers for a prefix, as plain spellings.
fn offered(paths: &ProjectPaths, prefix: &str) -> Vec<String> {
    paths
        .matches(prefix, MATCH_LIMIT)
        .iter()
        .map(|entry| entry.spelling().to_owned())
        .collect()
}

/// The corpus for a directory, built the way a session builds it.
fn corpus(root: &std::path::Path) -> ProjectPaths {
    ProjectPaths::under(Some(
        WorkingDirectory::at(root).expect("the staged directory resolves"),
    ))
}

/// The tree is offered, a directory carries its separator, and hidden and
/// ignored entries are not there.
///
/// One check over the four rules, because they are one walk: a person cannot
/// meet the ignore rule without meeting the offering.
#[test]
fn the_corpus_offers_the_tree_and_excludes_hidden_and_ignored() {
    let tree = ScratchTree::new();
    let project = tree.project();
    std::fs::create_dir_all(project.join(".hidden")).expect("staging: .hidden");
    std::fs::write(project.join(".hidden").join("private"), b"x").expect("staging: private");
    std::fs::create_dir_all(project.join("build")).expect("staging: build");
    std::fs::write(project.join("build").join("artefact.o"), b"x").expect("staging: artefact");
    std::fs::write(project.join(".gitignore"), b"/build\nnotes.txt\n").expect("staging: ignore");
    std::fs::write(project.join("notes.txt"), b"x").expect("staging: notes");
    std::fs::write(project.join("README.md"), b"x").expect("staging: readme");

    let paths = corpus(&project);
    let all = offered(&paths, "");

    assert!(
        all.contains(&"README.md".to_owned()),
        "an ordinary file is offered: {all:?}"
    );
    assert!(
        all.contains(&"inside/".to_owned()),
        "a directory is offered with its separator, which is what lets Tab descend: {all:?}"
    );
    assert!(
        all.contains(&"inside/file".to_owned()),
        "what is under a directory is offered too: {all:?}"
    );
    assert!(
        !all.iter().any(|spelling| spelling.starts_with(".hidden")),
        "a hidden directory and its contents are not offered: {all:?}"
    );
    assert!(
        !all.contains(&".gitignore".to_owned()),
        "the ignore file is itself hidden, so it is not offered either: {all:?}"
    );
    assert!(
        !all.iter().any(|spelling| spelling.starts_with("build")),
        "a directory `.gitignore` names is not offered, nor is anything under it: {all:?}"
    );
    assert!(
        !all.contains(&"notes.txt".to_owned()),
        "a file `.gitignore` names is not offered: {all:?}"
    );
}

/// A symlink out of the tree is never offered, and an ordinary directory in
/// the same walk is.
///
/// `ScratchTree` stages `project/escape` pointing at `../elsewhere`, whose one
/// file carries a value written nowhere else. The assertion is about the
/// **spellings** and about that value: a corpus that offered the link would
/// hand a person a `Tab` away from naming a path outside the boundary
/// `fs.read` is decided by.
#[test]
fn corpus_a_symlink_out_of_the_tree_is_never_offered() {
    let tree = ScratchTree::new();
    let paths = corpus(&tree.project());
    let all = offered(&paths, "");

    assert!(
        !all.iter().any(|spelling| spelling.starts_with("escape")),
        "a symbolic link out of the tree reached the corpus: {all:?}"
    );
    assert!(
        !all.iter()
            .any(|spelling| spelling.contains(tree.sentinel())),
        "something from outside the tree reached the corpus: {all:?}"
    );
    assert!(
        all.contains(&"inside/".to_owned()),
        "the accepting sibling: an ordinary directory in the same walk is offered, so the \
         assertion above is about the link rather than about an empty corpus: {all:?}"
    );
}

/// A `..` component and an absolute path outside the tree reach nothing, and
/// an in-tree prefix reaches its children.
///
/// The filter is whatever follows the sigil, so this is what a person typing
/// `@../` or `@/etc/passwd` gets: the corpus holds spellings relative to the
/// working directory and none of them begins with either.
#[test]
fn corpus_a_parent_component_never_reaches_the_strip() {
    let tree = ScratchTree::new();
    let paths = corpus(&tree.project());

    assert!(
        offered(&paths, "../").is_empty(),
        "`@../` offered something: {:?}",
        offered(&paths, "../")
    );
    assert!(
        offered(&paths, "../projectevil/").is_empty(),
        "a sibling directory whose name is a string-prefix of the project's reached the strip: \
         {:?}",
        offered(&paths, "../projectevil/")
    );
    assert!(
        offered(&paths, "/etc/").is_empty(),
        "an absolute path outside the tree reached the strip: {:?}",
        offered(&paths, "/etc/")
    );
    assert_eq!(
        offered(&paths, "inside/"),
        vec!["inside/file".to_owned()],
        "the accepting sibling: an in-tree prefix reaches its children, so the three assertions \
         above are about the boundary rather than about a corpus that answers nothing"
    );
}

/// The prefix itself is not among its own matches, which is what lets `Tab`
/// descend into a directory.
#[test]
fn a_directory_is_not_offered_back_to_a_person_who_typed_it() {
    let tree = ScratchTree::new();
    let paths = corpus(&tree.project());

    let under = offered(&paths, "inside/");
    assert!(
        !under.contains(&"inside/".to_owned()),
        "the directory was offered back as a match for itself: {under:?}"
    );
    assert!(
        offered(&paths, "insid").contains(&"inside/".to_owned()),
        "the accepting sibling: a shorter prefix does reach it, so the assertion above is about \
         equality rather than about the directory being missing: {:?}",
        offered(&paths, "insid")
    );
}

/// The absence line is said once a walk has found nothing, and not before, and
/// not when the walk found something.
#[test]
fn the_absence_is_said_only_after_a_walk_that_found_nothing() {
    let tree = ScratchTree::new();
    let bare = tree.base().join("bare");
    std::fs::create_dir_all(bare.join(".git")).expect("staging: bare/.git");
    std::fs::write(bare.join(".git").join("HEAD"), b"x").expect("staging: HEAD");

    let paths = corpus(&bare);
    assert_eq!(
        paths.absence(),
        None,
        "nothing has been walked yet, so there is nothing to say and the walk is not provoked"
    );
    assert!(
        offered(&paths, "").is_empty(),
        "a directory holding only hidden entries offers nothing"
    );
    assert_eq!(
        paths.absence(),
        Some(NOTHING_TO_OFFER.to_owned()),
        "after a walk that found nothing, the strip is given a sentence rather than blank rows"
    );

    let offering = corpus(&tree.project());
    assert!(!offered(&offering, "").is_empty());
    assert_eq!(
        offering.absence(),
        None,
        "the accepting sibling: a corpus with something in it says nothing, so a filter matching \
         none of it stays an ordinary miss"
    );
}

/// A turn is what re-walks the tree, and nothing else does.
#[test]
fn a_turn_is_what_re_walks_the_tree() {
    let tree = ScratchTree::new();
    let project = tree.project();
    let paths = corpus(&project);

    assert!(
        !offered(&paths, "arrived").contains(&"arrived.md".to_owned()),
        "the file does not exist yet"
    );
    std::fs::write(project.join("arrived.md"), b"x").expect("staging: arrived.md");
    assert!(
        !offered(&paths, "arrived").contains(&"arrived.md".to_owned()),
        "the corpus is kept between turns rather than re-walked on every keystroke"
    );
    paths.turn_ended();
    assert!(
        offered(&paths, "arrived").contains(&"arrived.md".to_owned()),
        "a turn ending is what re-walks the tree: {:?}",
        offered(&paths, "arrived")
    );
}

/// The walk stops at the ceiling.
///
/// Staged rather than asserted about the constant, because a cap nothing
/// exercises is a number in a comment. The tree is one entry over the line, so
/// the check fails both if the cap is missing and if it is off by one.
#[test]
fn the_walk_stops_at_the_ceiling() {
    let tree = ScratchTree::new();
    let wide = tree.base().join("wide");
    std::fs::create_dir_all(&wide).expect("staging: wide");
    for n in 0..=WALK_CEILING {
        std::fs::write(wide.join(format!("f{n:06}")), b"").expect("staging: a file");
    }

    let paths = corpus(&wide);
    let all: Vec<PathEntry> = paths.matches("", usize::MAX);
    assert_eq!(
        all.len(),
        WALK_CEILING,
        "the walk kept more than the ceiling, or stopped short of it"
    );
}
