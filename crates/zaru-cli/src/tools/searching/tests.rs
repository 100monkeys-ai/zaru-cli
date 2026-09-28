// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for `fs.search`, over trees each check builds for itself.
//!
//! Every tree is a scratch directory the check owns and removes. None is a
//! person's tree. Each check has an accepting arm, so a search that finds
//! nothing at all cannot pass one by accident.

use super::{LONGEST_SHOWN_LINE, Options, SHOWN_LINES, counts, search};
use crate::config::SizeCeiling;
use crate::tools::fixtures::nonce;
use crate::tools::output::OutputBudget;
use std::future::Future;
use std::path::{Path, PathBuf};

/// A directory this check owns, removed when it ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("the temporary directory resolves")
            .join(nonce("ft-search"));
        std::fs::create_dir_all(&base).expect("staging: the scratch directory");
        Self(base)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Write `text` at `name`, making its folders.
    fn put(&self, name: &str, text: impl AsRef<[u8]>) {
        let path = self.at(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("staging: a folder");
        }
        std::fs::write(path, text).expect("staging: a file");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn roomy() -> SizeCeiling {
    SizeCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

/// The binary's budget: 32 KiB.
fn budget() -> OutputBudget {
    OutputBudget::new(32 * 1024).expect("not zero")
}

/// Search `root` inside `scratch`, with the scratch directory as the working
/// directory, and return what the model would be given.
async fn find(scratch: &Scratch, root: &Path, needle: &str, options: &Options) -> String {
    let found = search(root, &scratch.0, needle, options, roomy(), budget(), None).await;
    assert_eq!(
        found.exit_code, 0,
        "the search did not run: {}",
        found.stderr
    );
    assert!(
        found.stderr.is_empty(),
        "the whole answer is one text, so nothing goes on standard error: {}",
        found.stderr
    );
    found.stdout
}

/// The rows of an answer that show a matching line: `  <n>: <text>`.
fn shown_rows(answer: &str) -> Vec<&str> {
    answer
        .lines()
        .filter(|line| {
            line.strip_prefix("  ")
                .and_then(|rest| rest.split_once(": "))
                .is_some_and(|(number, _)| number.parse::<usize>().is_ok())
        })
        .collect()
}

/// **The floor: a search never floods.** A word on 3,000 lines of 60 files
/// comes back as at most [`SHOWN_LINES`] lines, grouped under their files,
/// well under the budget, and the answer says how many more there are and
/// how to narrow.
///
/// Red on the unfixed tree: every one of the 3,000 lines was returned and the
/// budget cut the answer in the middle.
#[tokio::test]
async fn a_common_word_comes_back_as_the_best_lines_grouped_by_file_with_a_count_of_the_rest() {
    let scratch = Scratch::new();
    for file in 0..60 {
        let text: String = (0..50)
            .map(|line| format!("let budget_{file}_{line} = spend(budget);\n"))
            .collect();
        scratch.put(&format!("src/part_{file:02}.rs"), text);
    }

    let answer = find(&scratch, &scratch.0, "budget", &Options::default()).await;
    println!("{answer}");
    let again = find(&scratch, &scratch.0, "budget", &Options::default()).await;
    assert_eq!(
        answer, again,
        "two identical searches answered differently, so the order is the filesystem's"
    );

    assert!(
        answer.len() <= 30 * 1024,
        "the answer is {} bytes, over 30 KiB, so the budget would cut it",
        answer.len()
    );
    let rows = shown_rows(&answer);
    assert!(
        !rows.is_empty() && rows.len() <= SHOWN_LINES,
        "{} lines were shown, where at most {SHOWN_LINES} should be and at least one must be",
        rows.len()
    );
    assert!(
        answer.lines().any(|line| line == "src/part_00.rs"),
        "the lines are not grouped under a heading naming their file, relative to the working \
         directory: {answer}"
    );
    assert_eq!(
        counts(&answer),
        Some((3_000, 60)),
        "the first line does not say how many lines matched in how many files: {answer}"
    );
    assert!(
        answer.contains(&format!("{} more lines", 3_000 - rows.len())),
        "the answer does not say how many more lines there are: {answer}"
    );
    for narrowing in ["root", "file_type", "whole_word", "exact_case"] {
        assert!(
            answer.contains(narrowing),
            "the answer does not say how to narrow with {narrowing}: {answer}"
        );
    }
}

/// **The floor: nothing is cut in the middle.** With a small budget, every
/// row shown is a whole line of the file, and a line longer than
/// [`LONGEST_SHOWN_LINE`] characters says it was shortened and by how much.
///
/// Red on the unfixed tree: the unfixed search sized nothing, so its answer
/// was bigger than the budget and the budget's own cut fell mid-line.
#[tokio::test]
async fn an_answer_fits_its_budget_and_never_ends_in_half_a_line() {
    let scratch = Scratch::new();
    for file in 0..20 {
        let text: String = (0..20)
            .map(|line| format!("needle number {line} in file {file} with some words after it\n"))
            .collect();
        scratch.put(&format!("f{file:02}.txt"), text);
    }
    let long = format!("needle {}\n", "x".repeat(5_000));
    scratch.put("long.txt", &long);

    let small = OutputBudget::new(4 * 1024).expect("not zero");
    let found = search(
        &scratch.0,
        &scratch.0,
        "needle",
        &Options::default(),
        roomy(),
        small,
        None,
    )
    .await;
    println!("{}", found.stdout);
    assert!(
        found.stdout.len() <= small.get() - small.get() / 16,
        "the answer is {} bytes and the budget is {}, so the budget would cut it",
        found.stdout.len(),
        small.get()
    );
    for row in shown_rows(&found.stdout) {
        let text = row.split_once(": ").expect("a row").1;
        assert!(
            text.ends_with("after it") || text.contains("more characters"),
            "a row is not a whole line and does not say it was shortened: {row:?}"
        );
    }
    assert!(
        found.stdout.contains("more lines"),
        "lines were left out and the answer does not say so: {}",
        found.stdout
    );
    assert!(
        !found.stdout.contains(&"x".repeat(LONGEST_SHOWN_LINE + 1)),
        "a 5,000-character line was shown whole"
    );
}

/// **The floor: a search never misses silently.** When nothing matches, the
/// answer says what was searched, where, how many files were read, and what
/// was not looked at and why, so "absent" and "not looked at" differ.
///
/// Red on the unfixed tree: the answer was empty.
#[tokio::test]
async fn nothing_found_says_what_was_searched_where_and_what_was_skipped() {
    let scratch = Scratch::new();
    scratch.put("src/lib.rs", "pub fn present() {}\n");
    scratch.put("src/notes.txt", "nothing here\n");
    scratch.put("image.png", b"\x89PNG\r\n\x1a\n\x00\x00\x00binary");
    scratch.put("node_modules/pkg/index.js", "module.exports = 1;\n");
    scratch.put("big.log", "y".repeat(2_000));
    scratch.put("zqxjv-absent.dat", b"zqxjv-absent \xff\xfe rest");
    std::os::unix::fs::symlink("/etc", scratch.at("elsewhere")).expect("staging: a link");
    let small = SizeCeiling::new(1_000).expect("not zero");

    let found = search(
        &scratch.0,
        &scratch.0,
        "zqxjv-absent",
        &Options::default(),
        small,
        budget(),
        None,
    )
    .await;
    let answer = found.stdout;
    println!("{answer}");

    assert!(
        answer.contains("zqxjv-absent") && answer.contains("no line"),
        "the answer does not say that nothing held the needle: {answer:?}"
    );
    assert!(
        answer.contains("Searched 2 files"),
        "the answer does not say how many files were read: {answer:?}"
    );
    assert!(
        answer.contains("Files whose names hold \"zqxjv-absent\":\n  zqxjv-absent.dat"),
        "a file that is not UTF-8 is still matched by its name: {answer:?}"
    );
    for skipped in [
        "node_modules",
        "1 file that is not UTF-8",
        "1 binary file",
        "1 file over 1000 bytes",
        "1 symbolic link",
        "include_ignored",
    ] {
        assert!(
            answer.contains(skipped),
            "the answer does not name what was skipped ({skipped}): {answer:?}"
        );
    }
}

/// Ignored paths follow the repository's own ignore files, and build output
/// and dependency folders are skipped by default. `include_ignored` searches
/// them, ranked last, and a root inside a skipped folder is searched.
///
/// Red on the unfixed tree: `.gitignore` was not read and `node_modules` and
/// `vendor` were searched.
#[tokio::test]
async fn ignore_files_build_output_and_dependency_folders_are_skipped_unless_asked_for() {
    let scratch = Scratch::new();
    scratch.put(".gitignore", "/secret-output/\n*.gen\n");
    scratch.put("src/main.rs", "fn needle_here() {}\n");
    scratch.put("secret-output/made.txt", "needle IGNOREDOUTPUT\n");
    scratch.put("src/table.gen", "needle IGNOREDFILE\n");
    scratch.put("node_modules/dep/index.js", "needle DEPENDENCY\n");
    scratch.put("vendor/crate/lib.rs", "needle VENDORED\n");
    scratch.put("target/debug/out.rs", "needle BUILDOUTPUT\n");
    scratch.put("Cargo.lock", "needle LOCKFILE\n");
    scratch.put(".git/objects/index", "needle GITMETADATA\n");

    let answer = find(&scratch, &scratch.0, "needle", &Options::default()).await;
    println!("{answer}");
    assert!(
        answer.contains("src/main.rs"),
        "the source file was not searched: {answer}"
    );
    for skipped in [
        "IGNOREDOUTPUT",
        "IGNOREDFILE",
        "DEPENDENCY",
        "VENDORED",
        "BUILDOUTPUT",
        "LOCKFILE",
        "GITMETADATA",
    ] {
        assert!(
            !answer.contains(skipped),
            "a line from a skipped place came back ({skipped}): {answer}"
        );
    }

    let everything = find(
        &scratch,
        &scratch.0,
        "needle",
        &Options {
            include_ignored: true,
            ..Options::default()
        },
    )
    .await;
    println!("{everything}");
    for included in [
        "IGNOREDOUTPUT",
        "IGNOREDFILE",
        "DEPENDENCY",
        "VENDORED",
        "BUILDOUTPUT",
        "LOCKFILE",
    ] {
        assert!(
            everything.contains(included),
            "include_ignored did not search {included}: {everything}"
        );
    }
    assert!(
        !everything.contains("GITMETADATA"),
        "repository metadata was searched, even with include_ignored: {everything}"
    );
    let source = everything.find("src/main.rs").expect("the source file");
    let vendored = everything
        .find("vendor/crate/lib.rs")
        .expect("the vendored file");
    assert!(
        source < vendored,
        "a vendored file was ranked before the project's own source: {everything}"
    );

    let inside = find(
        &scratch,
        &scratch.at("node_modules"),
        "needle",
        &Options::default(),
    )
    .await;
    assert!(
        inside.contains("DEPENDENCY"),
        "a root the caller named inside a skipped folder was not searched: {inside}"
    );
}

/// **Ranking**: a definition before a use, source before tests before
/// generated files.
///
/// Red on the unfixed tree: the lines came back in path order, so a test and
/// a generated file came before the definition.
#[tokio::test]
async fn a_definition_comes_before_uses_and_source_before_tests_before_generated_files() {
    let scratch = Scratch::new();
    scratch.put(
        "a_generated.rs",
        "// @generated by a tool\nuse crate::Widget;\n",
    );
    scratch.put(
        "b_tests/widget_test.rs",
        "struct Widget;\nfn check() { let _ = Widget; }\n",
    );
    scratch.put("c_src/uses.rs", "fn build() -> Widget { Widget }\n");
    scratch.put("d_src/widget.rs", "/// A widget.\npub struct Widget;\n");
    scratch.put("a_src/factory.rs", "pub struct WidgetFactory;\n");

    let answer = find(&scratch, &scratch.0, "Widget", &Options::default()).await;
    println!("{answer}");
    let at = |needle: &str| {
        answer
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} is not in the answer: {answer}"))
    };
    let definition = at("2: pub struct Widget;");
    let used = at("c_src/uses.rs");
    let tested = at("b_tests/widget_test.rs");
    let generated = at("a_generated.rs");
    assert!(
        definition < used && used < tested && tested < generated,
        "the order is not definition, source, test, generated: {answer}"
    );
    assert!(
        answer.contains("(definition)"),
        "the definition is not marked: {answer}"
    );
    assert!(
        definition < at("pub struct WidgetFactory;"),
        "a definition whose name only holds the needle came before the one named exactly: \
         {answer}"
    );
}

/// A needle with no capital letter matches any case; one with a capital, or
/// `exact_case`, matches exactly; `whole_word` skips a match inside a longer
/// word; `file_type` searches one kind of file.
///
/// Red on the unfixed tree: a lowercase needle found nothing in "No
/// expansion", and the options did not exist.
#[tokio::test]
async fn case_whole_words_and_file_type_narrow_a_search() {
    let scratch = Scratch::new();
    scratch.put(
        "line.rs",
        "/// No expansion of any kind happens.\nfn split() {}\n",
    );
    scratch.put("notes.md", "no expansion here either\n");
    scratch.put("words.txt", "expansions are plural\n");

    let any_case = find(&scratch, &scratch.0, "no expansion", &Options::default()).await;
    assert!(
        any_case.contains("line.rs") && any_case.contains("notes.md"),
        "a lowercase needle did not match both cases: {any_case}"
    );
    let exact = find(
        &scratch,
        &scratch.0,
        "no expansion",
        &Options {
            exact_case: true,
            ..Options::default()
        },
    )
    .await;
    assert!(
        !exact.contains("line.rs") && exact.contains("notes.md"),
        "exact_case did not match exactly: {exact}"
    );
    let whole = find(
        &scratch,
        &scratch.0,
        "expansion",
        &Options {
            whole_word: true,
            ..Options::default()
        },
    )
    .await;
    assert!(
        !whole.contains("words.txt") && whole.contains("notes.md"),
        "whole_word matched inside a longer word: {whole}"
    );
    let typed = find(
        &scratch,
        &scratch.0,
        "expansion",
        &Options {
            file_type: Some(String::from("rs")),
            ..Options::default()
        },
    )
    .await;
    assert!(
        typed.contains("line.rs") && !typed.contains("notes.md"),
        "file_type did not keep the search to one kind of file: {typed}"
    );
}

/// A file's name matches too, shown first and relative to the working
/// directory, and a root given as one file searches that file.
#[tokio::test]
async fn a_name_match_and_a_single_file_root_are_answered() {
    let scratch = Scratch::new();
    scratch.put("src/codebase.rs", "fn retrieve() {}\n");
    scratch.put("src/other.rs", "// codebase is mentioned here\n");

    let answer = find(&scratch, &scratch.0, "codebase", &Options::default()).await;
    println!("{answer}");
    assert!(
        answer.contains("Files whose names hold \"codebase\":\n  src/codebase.rs"),
        "the file whose name matches is not listed first: {answer}"
    );
    assert!(
        !answer.contains(&scratch.0.display().to_string()),
        "a path is shown whole where it could be relative: {answer}"
    );
    let one = find(
        &scratch,
        &scratch.at("src/other.rs"),
        "codebase",
        &Options::default(),
    )
    .await;
    assert!(
        one.contains("  1: // codebase is mentioned here"),
        "a root that is a file was not searched: {one}"
    );
}

/// **Security corpus, carried over.** The walk never follows a symbolic
/// link, a root that is a link is refused, and a file over the ceiling is
/// never read. An empty needle is refused.
#[tokio::test]
async fn links_are_never_followed_and_an_oversized_file_is_never_read() {
    let scratch = Scratch::new();
    let sentinel = nonce("only-behind-the-link");
    scratch.put("outside/secret.txt", format!("NEEDLE {sentinel}\n"));
    scratch.put("inside/ordinary.txt", "NEEDLE in the tree\n");
    scratch.put("inside/huge.txt", format!("NEEDLE {}\n", "x".repeat(200)));
    std::os::unix::fs::symlink(scratch.at("outside"), scratch.at("inside/escape"))
        .expect("staging: the link");
    let ceiling = SizeCeiling::new(64).expect("not zero");

    let found = search(
        &scratch.at("inside"),
        &scratch.0,
        "NEEDLE",
        &Options::default(),
        ceiling,
        budget(),
        None,
    )
    .await;
    println!("{}", found.stdout);
    assert!(
        !found.stdout.contains(&sentinel),
        "the walk followed a link out of its root: {}",
        found.stdout
    );
    assert!(
        !found.stdout.contains(&"x".repeat(200)),
        "the oversized file was read: {}",
        found.stdout
    );
    assert!(
        found.stdout.contains("inside/ordinary.txt") && found.stdout.contains("1 symbolic link"),
        "the ordinary file was not found or the link was not named: {}",
        found.stdout
    );

    let refused = search(
        &scratch.at("inside/escape"),
        &scratch.0,
        "NEEDLE",
        &Options::default(),
        roomy(),
        budget(),
        None,
    )
    .await;
    assert_eq!(refused.exit_code, 1, "a root that is a link was searched");
    assert!(!refused.stdout.contains(&sentinel));

    let empty = search(
        &scratch.0,
        &scratch.0,
        "",
        &Options::default(),
        roomy(),
        budget(),
        None,
    )
    .await;
    assert_eq!(empty.exit_code, 1, "an empty needle was searched for");
}

/// A needle of several words also finds declarations whose names or nearby
/// comments hold those words, in any form of the word, and never a mere use.
///
/// Red on the unfixed tree: every word had to appear whole, "retry logic"
/// found nothing, and uses crowded out declarations.
#[tokio::test]
async fn several_words_find_declarations_by_their_names_and_comments() {
    let scratch = Scratch::new();
    scratch.put(
        "src/retry.rs",
        "/// Waits longer after each failed attempt.\n\
         pub fn retry_with_backoff() {}\n\
         pub fn unrelated() { retry_with_backoff(); }\n",
    );
    scratch.put(
        "src/seal.rs",
        "/// Encrypts a stored key before it is written.\npub fn seal_blob() {}\n",
    );

    let answer = find(
        &scratch,
        &scratch.0,
        "where is the retry logic",
        &Options::default(),
    )
    .await;
    println!("{answer}");
    assert!(
        answer.contains("src/retry.rs") && answer.contains("2: function retry_with_backoff"),
        "the declaration named for the words was not found: {answer}"
    );
    assert!(
        !answer.contains("reference"),
        "a use was offered as a declaration: {answer}"
    );

    let encrypted = find(&scratch, &scratch.0, "encrypted key", &Options::default()).await;
    println!("{encrypted}");
    assert!(
        encrypted.contains("2: function seal_blob"),
        "a declaration whose comment holds another form of the words was not found: {encrypted}"
    );
}

/// A search yields to the rest of the program between files, so the
/// interface keeps painting while a large tree is walked, and dropping the
/// search stops it.
///
/// Red on the unfixed tree only if the walk stops yielding; this pins the
/// property the old search had through `tokio::fs`.
#[test]
fn a_search_yields_between_files() {
    let scratch = Scratch::new();
    for file in 0..40 {
        scratch.put(&format!("f{file:02}.txt"), "needle\n");
    }
    let options = Options::default();
    let mut future = Box::pin(search(
        &scratch.0,
        &scratch.0,
        "needle",
        &options,
        roomy(),
        budget(),
        None,
    ));
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut pending = 0;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let _entered = runtime.enter();
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(found) => {
                assert!(found.stdout.contains("40 files"), "{}", found.stdout);
                break;
            }
            std::task::Poll::Pending => pending += 1,
        }
        assert!(pending < 10_000, "the search never finished");
    }
    assert!(
        pending >= 40,
        "the search gave way {pending} times over 40 files, so it holds the interface still"
    );
}

/// A search the stand-in model answers: the project of `meaning::tests`,
/// indexed, then searched for `needle` with retrieval by meaning on.
async fn by_meaning(needle: &str) -> (String, crate::meaning::tests::Scratch) {
    use crate::meaning::tests::{Words, ready, roomy as ceiling, words_loader};
    let project = crate::meaning::tests::Scratch::new("search-meaning");
    project.put(
        "src/limits.rs",
        "/// Per-capability rate limit configuration.\npub struct RateLimit {\n    pub calls: u32,\n    pub per_seconds: u32,\n}\n",
    );
    project.put(
        "src/tokens.rs",
        "/// Reject a replayed token: a duplicate identifier was seen before.\npub fn record_jti(seen: &mut Vec<String>, jti: &str) -> bool {\n    seen.iter().all(|one| one != jti)\n}\n",
    );
    project.put(
        "src/greeting.rs",
        "/// Say hello to a person by name.\npub fn greet(name: &str) -> String {\n    format!(\"hello {name}\")\n}\n",
    );
    let kept = crate::meaning::tests::Scratch::new("search-meaning-index");
    let meaning =
        crate::meaning::Meaning::start(&project.0, &kept.0, ceiling(), words_loader(Words::new()));
    let waited = project.0.clone();
    let meaning = tokio::task::spawn_blocking(move || {
        ready(&meaning);
        let _ = waited;
        meaning
    })
    .await
    .expect("the index is built");
    let found = search(
        &project.0,
        &project.0,
        needle,
        &Options::default(),
        roomy(),
        budget(),
        Some(&meaning),
    )
    .await;
    drop(meaning);
    drop(kept);
    (found.stdout, project)
}

/// **Retrieval by meaning finds code whose words differ from the
/// question's.** "throttle calls" shares no word with "rate limit"; the
/// stand-in model counts the two as one meaning, as a real model does.
#[tokio::test]
async fn several_words_also_list_the_places_nearest_in_meaning() {
    let (answer, _project) = by_meaning("where are calls throttled").await;
    println!("{answer}");
    let part = answer
        .split_once(super::MEANING_HEADING)
        .expect("the answer has a part found by meaning")
        .1;
    let first = part.lines().nth(1).expect("a file heading");
    assert_eq!(
        first, "src/limits.rs",
        "the nearest place is not first: {answer}"
    );
    assert!(
        part.contains("pub struct RateLimit {") && part.contains("[meaning 0."),
        "a place shows its declaration and says it came by meaning: {answer}"
    );
    assert!(
        part.contains("/// Per-capability rate limit configuration."),
        "a place shows its comment: {answer}"
    );
    assert!(
        part.contains("the index covers 3 of 3 files"),
        "the answer says how much of the tree the index covers: {answer}"
    );
    let tally = super::tally(&answer).expect("an fs.search answer");
    assert!(
        tally.meanings >= 1,
        "the tally counts the places: {tally:?}"
    );
    let view = crate::tools::result_view::search(".", "where are calls throttled", &answer);
    let line = view.view.summary.clone();
    assert!(
        line.contains("by meaning"),
        "the pane's one line says so: {line}"
    );
}

/// **With it off, or for one word, the answer is the one it was.** A needle
/// of one word is found by its lines, as before, and never waits on a model.
#[tokio::test]
async fn one_word_or_retrieval_off_leaves_the_answer_as_it_was() {
    let (with_meaning, project) = by_meaning("RateLimit").await;
    let without = search(
        &project.0,
        &project.0,
        "RateLimit",
        &Options::default(),
        roomy(),
        budget(),
        None,
    )
    .await;
    assert_eq!(with_meaning, without.stdout, "one word changed the answer");
    assert!(!without.stdout.contains(super::MEANING_HEADING));
    let (several, _) = by_meaning("rate limit configuration").await;
    assert!(
        several.contains(super::MEANING_HEADING),
        "the accepting arm: several words are answered by meaning too: {several}"
    );
}

/// **Before the index can answer, the search says so and answers by text.**
#[tokio::test]
async fn before_retrieval_by_meaning_can_answer_the_search_says_so_and_answers_by_text() {
    let scratch = Scratch::new();
    scratch.put(
        "src/lib.rs",
        "/// Throttle the calls.\npub fn throttle_calls() {}\n",
    );
    let meaning = crate::meaning::Meaning::unavailable(
        "retrieval by meaning is on, but its model is not fetched. Run zaru index fetch",
    );
    let found = search(
        &scratch.0,
        &scratch.0,
        "throttle calls",
        &Options::default(),
        roomy(),
        budget(),
        Some(&meaning),
    )
    .await;
    let answer = found.stdout;
    assert!(
        answer.contains(
            "Retrieval by meaning is on, but it cannot answer yet: retrieval by meaning is on, \
             but its model is not fetched. Run zaru index fetch. The results below are by text \
             alone."
        ),
        "the answer does not say why meaning did not answer: {answer}"
    );
    assert!(
        answer.contains("src/lib.rs"),
        "the text results are still there: {answer}"
    );
}

/// **The stated rule: a place near in meaning that also holds a text match
/// ranks above one a little nearer that holds none.** Reciprocal rank
/// fusion over the three lists, with [`super::FUSION_K`].
#[test]
fn a_place_found_by_meaning_and_by_text_ranks_above_one_found_by_meaning_alone() {
    let place = |path: &str, score: f32| super::Place {
        path: path.to_owned(),
        start: 1,
        end: 3,
        score,
        lines: vec![String::from("pub fn here() {}")],
    };
    let meant = super::Meant::Places {
        places: vec![place("a.rs", 0.90), place("b.rs", 0.85)],
        indexed: 2,
        files: 2,
        building: None,
    };
    let mut walked = super::Walked::default();
    walked.files = vec![String::from("b.rs")];
    walked.hits.push(super::Hit {
        file: 0,
        line: 2,
        text: String::from("the needle"),
        tier: super::Tier::Source,
    });
    let part = super::meaning_part("the needle", &meant, &walked, 10_000);
    let b = part.find("b.rs").expect("b is listed");
    let a = part.find("a.rs").expect("a is listed");
    assert!(
        b < a,
        "the place that also holds text did not rank first: {part}"
    );
    assert!(part.contains("[meaning 0.85; also by text]"), "{part}");
    assert!(part.contains("[meaning 0.90]"), "{part}");
}
