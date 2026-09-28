// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! `fs.search`: the best matching lines, grouped by file, sized to fit.
//!
//! # What was wrong
//!
//! Until 2026-09-28 `fs.search` returned every line that held the needle,
//! in path order, with each path whole. A common word in a real tree was
//! thousands of lines; the output budget kept the first and last 16 KiB and
//! cut out the middle, often mid-line. It read `.gitignore` nowhere, so it
//! searched `node_modules` and `vendor` folders: on a Rust service with its
//! crates vendored, one search took about 170 seconds and 2.5 GB of memory,
//! because it also parsed every supported source file, even when the plain
//! search had found what was asked for. The needle had to match exactly,
//! case included. When nothing matched, the answer was empty, and what had
//! been skipped went to standard error as one line per file. And the parsed
//! declarations were offered only when no line held the needle at all, and
//! only when every word of it named the same declaration.
//!
//! # What it does now
//!
//! - **It walks what the repository keeps.** The walk honours `.gitignore`,
//!   `.ignore` and `.git/info/exclude`, and skips the folders in
//!   [`SKIPPED_FOLDERS`] and the lock and minified files in
//!   [`GENERATED_NAMES`] and [`GENERATED_ENDINGS`]. `include_ignored`
//!   searches all of them, ranked last. A root the caller names inside a
//!   skipped folder is searched: the rule is about walking into one.
//! - **It answers with the best lines, and says how many more.** At most
//!   [`SHOWN_LINES`] lines, grouped under their file, each with its number
//!   and its text, and never more than fifteen sixteenths of the budget, so
//!   the budget's own cut is never reached. A line longer than
//!   [`LONGEST_SHOWN_LINE`] characters is shortened and says by how much.
//!   The answer says how many lines and files matched, how many are not
//!   shown, and how to narrow.
//! - **It ranks.** A line where a declaration of that name starts comes
//!   first, then the project's source, then its tests, then generated files.
//! - **A needle with no capital letter matches any case**, as most search
//!   tools do; `exact_case` asks for exact case, `whole_word` for whole
//!   words, `file_type` for one kind of file.
//! - **It never misses silently.** Every answer ends by saying how many
//!   files were read and what was not looked at, and why.
//! - **Several words also find declarations** in the project's own source
//!   (not its tests or generated files) whose names or nearby comments hold
//!   those words, in any form of the word, by the parsed structure
//!   `codebase` builds. That is [ADR-0035] D2, now reached for any
//!   needle of two or more words rather than only when nothing else matched.
//! - **It gives way between files**, so the interface keeps painting on a
//!   large tree and dropping the search stops it.
//!
//! Ruled by the coordinator at the `search-quality` arc's spawn under
//! Jeshua's directive 58, and recorded on ADR-0011 D1 and D5 and ADR-0035,
//! open to his veto.
//!
//! # The boundary is untouched
//!
//! The root is the one the permission decision was reached about, and
//! nothing here resolves it again. A symbolic link is never followed: a root
//! that is one is refused, and one met on the walk is counted and skipped,
//! so everything read is below the classified root. A file over the ceiling
//! is skipped on its size and never opened.
//!
//! [ADR-0035]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0035-local-structural-code-retrieval

use crate::config::SizeCeiling;
use crate::tools::codebase::{self, Symbol};
use crate::tools::output::{Captured, OutputBudget};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// What the model is told `fs.search` does.
pub const SEARCH_DESCRIPTION: &str = "Search the files under root for \
    needle, plain text and not a pattern. A needle with no capital letters \
    matches any case. Optional: exact_case, whole_word, file_type (an \
    extension such as rs), include_ignored. Ignored files, build output and \
    dependency folders are skipped unless include_ignored is true. Results \
    are grouped by file and ranked: definitions, then source, then tests, \
    then generated files; at most 40 lines are shown, with a count of the \
    rest. Two or more words also find declarations named or commented with \
    those words. Use it to find where something is; use fs.read to read \
    around a result.";

/// What the model is told `fs.list` does.
pub const LIST_DESCRIPTION: &str = "List the names in one folder, not in \
    its subfolders. To find a file or text anywhere below a folder, use \
    fs.search.";

/// How many matching lines an answer shows at most.
///
/// **40.** A row is its number and the line, about 60 bytes for source, so
/// forty rows and their file headings are about 3 KB: enough to see the
/// pattern of a common word and pick a narrower search, and small beside
/// the 30 KiB an answer may take. The count is stated in the description.
pub const SHOWN_LINES: usize = 40;

/// How many rows one file gets before other files get theirs.
const ROWS_PER_FILE_FIRST: usize = 8;

/// How many files whose names match are listed at most.
pub const SHOWN_NAMES: usize = 10;

/// How many characters of a line are shown before it is shortened.
pub const LONGEST_SHOWN_LINE: usize = 200;

/// Folders skipped by default: repository metadata, build output and
/// dependencies. `.git` is skipped even with `include_ignored`.
pub const SKIPPED_FOLDERS: [&str; 9] = [
    ".git",
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

/// Files that a tool writes rather than a person: skipped by default.
pub const GENERATED_NAMES: [&str; 8] = [
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "go.sum",
    "composer.lock",
    "Gemfile.lock",
];

/// Endings of generated file names: skipped by default.
pub const GENERATED_ENDINGS: [&str; 3] = [".min.js", ".min.css", ".map"];

/// Words in the head of a file that say a tool wrote it. Such a file is
/// searched and ranked last.
const GENERATED_MARKS: [&str; 4] = [
    "@generated",
    "DO NOT EDIT",
    "Code generated",
    "autogenerated",
];

/// How many bytes of a file are looked at to tell binary from text, and for
/// a generated mark.
const HEAD_BYTES: usize = 8 * 1024;

/// How many declarations the words part of an answer shows at most.
const SHOWN_DECLARATIONS: usize = 8;

/// The heading of the part of an answer that lists declarations.
pub const DECLARATIONS_HEADING: &str = "Declarations whose names or comments share words with";

/// The optional fields of an `fs.search` call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Match the needle's case exactly, even when it has no capital letter.
    pub exact_case: bool,
    /// Match only where the needle is not part of a longer word.
    pub whole_word: bool,
    /// Search only files with this extension, such as `rs`.
    pub file_type: Option<String>,
    /// Search ignored files, build output, dependency folders and lock files
    /// too.
    pub include_ignored: bool,
}

/// What an answer's first line says was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    /// Matching lines, shown or not.
    pub lines: usize,
    /// Files those lines are in.
    pub files: usize,
    /// Declarations the words part lists.
    pub declarations: usize,
}

/// How many lines matched in how many files, read from an answer's first
/// line. `None` when `answer` is not an `fs.search` answer.
#[must_use]
pub fn counts(answer: &str) -> Option<(usize, usize)> {
    tally(answer).map(|tally| (tally.lines, tally.files))
}

/// What an answer says was found. `None` when `answer` is not an
/// `fs.search` answer.
#[must_use]
pub fn tally(answer: &str) -> Option<Tally> {
    let first = answer.lines().next()?;
    if !first.starts_with("Searched ") {
        return None;
    }
    let declarations = answer
        .split_once(DECLARATIONS_HEADING)
        .map_or(0, |(_, rest)| {
            rest.lines()
                .skip(1)
                .take_while(|line| !line.is_empty())
                .filter(|line| line.starts_with("  "))
                .count()
        });
    if first.ends_with(": no line holds it.") {
        return Some(Tally {
            lines: 0,
            files: 0,
            declarations,
        });
    }
    let (_, said) = first.rsplit_once(": ")?;
    let said = said.strip_suffix('.')?;
    let (lines, files) = said.split_once(" in ")?;
    let number = |text: &str| text.split(' ').next()?.parse::<usize>().ok();
    Some(Tally {
        lines: number(lines)?,
        files: number(files)?,
        declarations,
    })
}

/// A capture that says the search could not run.
fn failed(detail: String) -> Captured {
    Captured {
        exit_code: 1,
        stdout: String::new(),
        stderr: detail,
    }
}

/// Search the files under `root` for `needle`, in an answer that fits
/// `budget`.
///
/// `root` is the path the permission decision was reached about. Paths are
/// shown relative to `base`, the working directory, when they are inside it.
pub async fn search(
    root: &Path,
    base: &Path,
    needle: &str,
    options: &Options,
    ceiling: SizeCeiling,
    budget: OutputBudget,
) -> Captured {
    if needle.is_empty() {
        return failed(String::from(
            "the string to search for is empty, which occurs everywhere in every file. fs.search \
             needs something to look for",
        ));
    }
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(source) => return failed(format!("could not search {}: {source}", root.display())),
    };
    if metadata.file_type().is_symlink() {
        return failed(format!(
            "{} is a symbolic link, and fs.search does not follow one. A link is what lets a walk \
             leave the tree the call was classified against, so the search would be somewhere \
             nobody was asked about",
            root.display()
        ));
    }

    let matcher = Matcher::new(needle, options);
    // A call that asks for exact case or whole words wants the text as
    // written, so the words part, which matches any form of a word, is left
    // out.
    // The words part is for a needle of two or more words as typed; an
    // identifier alone is found by its lines.
    let words =
        if options.exact_case || options.whole_word || needle.split_whitespace().nth(1).is_none() {
            Vec::new()
        } else {
            codebase::query_terms(needle)
        };
    let mut walked = Walked::default();
    let folders = Arc::new(Mutex::new(BTreeSet::new()));
    let walker = walker(root, options, Arc::clone(&folders));
    for entry in walker {
        // Give way between entries: the interface paints, and a turn that is
        // stopped drops this future here.
        tokio::task::yield_now().await;
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                walked.skipped.unreadable.push(error.to_string());
                continue;
            }
        };
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            walked.skipped.links.push(shown(entry.path(), base));
            continue;
        }
        if kind.is_file() {
            walked.consider(entry.path(), root, base, &matcher, &words, options, ceiling);
        }
    }
    walked.skipped.folders = folders
        .lock()
        .map(|folders| folders.clone())
        .unwrap_or_default();

    Captured {
        exit_code: 0,
        stdout: compose(
            &shown(root, base),
            needle,
            options,
            &matcher,
            &words,
            walked,
            budget,
        ),
        stderr: String::new(),
    }
}

/// The walk: ignore files honoured, skipped folders pruned and counted, no
/// link followed, in path order.
fn walker(root: &Path, options: &Options, folders: Arc<Mutex<BTreeSet<String>>>) -> ignore::Walk {
    let honour = !options.include_ignored;
    let include = options.include_ignored;
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(true)
        .ignore(honour)
        .git_ignore(honour)
        .git_exclude(honour)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_path(Ord::cmp)
        .filter_entry(move |entry| {
            if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if name == ".git" {
                return false;
            }
            if !include && SKIPPED_FOLDERS.contains(&name.as_ref()) {
                if let Ok(mut folders) = folders.lock() {
                    folders.insert(name.into_owned());
                }
                return false;
            }
            true
        })
        .build()
}

/// A path as the person knows it: relative to `base` when inside it.
fn shown(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(relative) if relative.as_os_str().is_empty() => String::from("."),
        Ok(relative) => relative.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// How the needle is matched.
struct Matcher {
    needle: String,
    any_case: bool,
    whole_word: bool,
}

impl Matcher {
    fn new(needle: &str, options: &Options) -> Self {
        let any_case = !options.exact_case && !needle.chars().any(char::is_uppercase);
        Self {
            needle: if any_case {
                needle.to_lowercase()
            } else {
                needle.to_owned()
            },
            any_case,
            whole_word: options.whole_word,
        }
    }

    /// Whether `name` is the needle itself, in the case this search uses.
    fn is(&self, name: &str) -> bool {
        if self.any_case {
            name.to_lowercase() == self.needle
        } else {
            name == self.needle
        }
    }

    fn matches(&self, text: &str) -> bool {
        let folded;
        let haystack = if self.any_case {
            folded = text.to_lowercase();
            folded.as_str()
        } else {
            text
        };
        if !self.whole_word {
            return haystack.contains(&self.needle);
        }
        let in_a_word = |character: char| character.is_alphanumeric() || character == '_';
        haystack.match_indices(&self.needle).any(|(at, _)| {
            let before = haystack[..at].chars().next_back();
            let after = haystack[at + self.needle.len()..].chars().next();
            !before.is_some_and(in_a_word) && !after.is_some_and(in_a_word)
        })
    }
}

/// How a matching line ranks. Lower comes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// A declaration whose name is the needle starts on this line.
    Named,
    /// A declaration whose name holds the needle starts on this line.
    Definition,
    /// The project's own source.
    Source,
    /// A test.
    Test,
    /// A file a tool wrote, or one in a folder skipped by default.
    Generated,
}

/// One matching line.
struct Hit {
    file: usize,
    line: usize,
    text: String,
    tier: Tier,
}

/// What was not looked at, and why.
#[derive(Default)]
struct Skipped {
    folders: BTreeSet<String>,
    generated: Vec<String>,
    binary: Vec<String>,
    large: Vec<String>,
    not_utf8: Vec<String>,
    links: Vec<String>,
    unreadable: Vec<String>,
    ceiling: u64,
}

/// Everything the walk found.
#[derive(Default)]
struct Walked {
    files: Vec<String>,
    hits: Vec<Hit>,
    names: Vec<String>,
    symbols: Vec<Symbol>,
    searched: usize,
    skipped: Skipped,
}

impl Walked {
    /// Match one file by name and, where it can be read, by its lines.
    #[allow(
        clippy::too_many_arguments,
        reason = "each is one fact about this search, handed down from `search`"
    )]
    fn consider(
        &mut self,
        path: &Path,
        root: &Path,
        base: &Path,
        matcher: &Matcher,
        words: &[String],
        options: &Options,
        ceiling: SizeCeiling,
    ) {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(wanted) = &options.file_type {
            let wanted = wanted.trim_start_matches('.');
            let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase());
            if extension.as_deref() != Some(&wanted.to_lowercase()) {
                return;
            }
        }
        let display = shown(path, base);
        let generated_name = GENERATED_NAMES.contains(&name.as_str())
            || GENERATED_ENDINGS
                .iter()
                .any(|ending| name.ends_with(ending));
        if generated_name && !options.include_ignored {
            self.skipped.generated.push(display);
            return;
        }
        if matcher.matches(&name) {
            self.names.push(display.clone());
        }

        self.skipped.ceiling = ceiling.get();
        let size = std::fs::symlink_metadata(path).map_or(0, |metadata| metadata.len());
        if size > ceiling.get() {
            self.skipped.large.push(display);
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.skipped.unreadable.push(format!("{display} ({error})"));
                return;
            }
        };
        let head = &bytes[..bytes.len().min(HEAD_BYTES)];
        if head.contains(&0) {
            self.skipped.binary.push(display);
            return;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            self.skipped.not_utf8.push(display);
            return;
        };
        self.searched += 1;

        let in_skipped_folder = path.strip_prefix(root).is_ok_and(|relative| {
            relative
                .components()
                .any(|part| SKIPPED_FOLDERS.contains(&part.as_os_str().to_string_lossy().as_ref()))
        });
        let head_text: String = text.chars().take(HEAD_BYTES).collect();
        let tier = if generated_name
            || in_skipped_folder
            || GENERATED_MARKS.iter().any(|mark| head_text.contains(mark))
        {
            Tier::Generated
        } else if is_test(&display) {
            Tier::Test
        } else {
            Tier::Source
        };

        let file = self.files.len();
        let first_hit = self.hits.len();
        for (at, line) in text.lines().enumerate() {
            if matcher.matches(line) {
                self.hits.push(Hit {
                    file,
                    line: at + 1,
                    text: line.to_owned(),
                    tier,
                });
            }
        }
        let has_hits = self.hits.len() > first_hit;
        // Parsed only where the result is used: to lift a source line that
        // declares the needle, or for the words part. A generated or ignored
        // file is never parsed.
        if tier == Tier::Source && (has_hits || words.len() >= 2) {
            let mut here = Vec::new();
            codebase::collect(Path::new(&display), &text, &mut here);
            here.retain(Symbol::is_declaration);
            // Only the project's own source is lifted: a declaration in a
            // test or a generated file keeps that file's place.
            if has_hits {
                for hit in &mut self.hits[first_hit..] {
                    for symbol in here.iter().filter(|symbol| symbol.line == hit.line) {
                        if matcher.is(&symbol.name) {
                            hit.tier = Tier::Named;
                        } else if matcher.matches(&symbol.name) && hit.tier != Tier::Named {
                            hit.tier = Tier::Definition;
                        }
                    }
                }
            }
            if words.len() >= 2 {
                self.symbols.extend(here);
            }
        }
        self.files.push(display);
    }
}

/// Whether a path is a test by the common conventions.
fn is_test(path: &str) -> bool {
    let path = Path::new(path);
    let in_tests = path.components().any(|part| {
        matches!(
            part.as_os_str().to_string_lossy().as_ref(),
            "tests" | "test" | "__tests__" | "spec"
        )
    });
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.split('.').next().unwrap_or_default();
    in_tests
        || name == "tests.rs"
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.starts_with("test_")
        || name.contains(".test.")
        || name.contains(".spec.")
}

/// A line as a row: its text without leading space, shortened when long.
fn row_text(line: &str) -> String {
    let line = line.trim();
    let length = line.chars().count();
    if length <= LONGEST_SHOWN_LINE {
        return line.to_owned();
    }
    let kept: String = line.chars().take(LONGEST_SHOWN_LINE).collect();
    format!("{kept}… ({} more characters)", length - LONGEST_SHOWN_LINE)
}

/// `n` and a noun, singular for one.
fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Up to three examples, and how many more.
fn such_as(names: &[String]) -> String {
    let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
    let mut said = format!("such as {}", shown.join(", "));
    if names.len() > shown.len() {
        said.push_str(&format!(" and {} more", names.len() - shown.len()));
    }
    said
}

/// The answer.
fn compose(
    root: &str,
    needle: &str,
    options: &Options,
    matcher: &Matcher,
    words: &[String],
    walked: Walked,
    budget: OutputBudget,
) -> String {
    let room = (budget.get() - budget.get() / 16).saturating_sub(700 + root.len());

    let mut what = format!("\"{needle}\"");
    what.push_str(if matcher.any_case {
        " in any case"
    } else {
        " in exact case"
    });
    if options.whole_word {
        what.push_str(", whole words only");
    }
    if let Some(kind) = &options.file_type {
        what.push_str(&format!(
            ", in .{} files only",
            kind.trim_start_matches('.')
        ));
    }
    let matched_files: BTreeSet<usize> = walked.hits.iter().map(|hit| hit.file).collect();
    let header = if walked.hits.is_empty() {
        format!("Searched {root} for {what}: no line holds it.")
    } else {
        format!(
            "Searched {root} for {what}: {} in {}.",
            counted(walked.hits.len(), "line", "lines"),
            counted(matched_files.len(), "file", "files")
        )
    };
    let closing = closing(&walked, options);

    let mut answer = header;
    answer.push('\n');

    if !walked.names.is_empty() {
        answer.push_str(&format!("\nFiles whose names hold \"{needle}\":\n"));
        for name in walked.names.iter().take(SHOWN_NAMES) {
            answer.push_str(&format!("  {name}\n"));
        }
        if walked.names.len() > SHOWN_NAMES {
            answer.push_str(&format!(
                "  and {} more\n",
                walked.names.len() - SHOWN_NAMES
            ));
        }
    }

    // Room kept for the closing and the count of what is not shown.
    let keep = closing.len() + 400;
    let (lines, shown) = lines_part(&walked, room.saturating_sub(answer.len() + keep));
    let shown_lines = shown.iter().sum::<usize>();
    let files_with_hidden = matched_files
        .iter()
        .filter(|file| {
            let all = walked.hits.iter().filter(|hit| hit.file == **file).count();
            shown.get(**file).copied().unwrap_or(0) < all
        })
        .count();
    answer.push_str(&lines);

    if words.len() >= 2 {
        let declarations =
            declarations_part(needle, &walked, room.saturating_sub(answer.len() + keep));
        answer.push_str(&declarations);
    }

    let hidden = walked.hits.len() - shown_lines;
    if hidden > 0 {
        answer.push_str(&format!(
            "\n{} more {} not shown, in {} of the {} that match. To narrow the search, set root \
             to a folder or a file, file_type to an extension such as rs, whole_word or \
             exact_case to true, or search for a longer needle.\n",
            hidden,
            if hidden == 1 { "line is" } else { "lines are" },
            files_with_hidden,
            counted(matched_files.len(), "file", "files"),
        ));
    }
    answer.push('\n');
    answer.push_str(&closing);
    answer
}

/// The matching lines, best first, grouped by file, within `room` bytes.
/// Returns the text and how many lines it shows of each file, by file index.
fn lines_part(walked: &Walked, room: usize) -> (String, Vec<usize>) {
    // Files in the order of their best line, then by path.
    let mut order: Vec<usize> = walked.hits.iter().map(|hit| hit.file).collect();
    order.sort_unstable();
    order.dedup();
    let best = |file: usize| {
        walked
            .hits
            .iter()
            .filter(|hit| hit.file == file)
            .map(|hit| hit.tier)
            .min()
            .unwrap_or(Tier::Generated)
    };
    order.sort_by_key(|file| (best(*file), walked.files[*file].clone()));

    // Each file's lines: declarations first, then in line order.
    let rows_of = |file: usize| {
        let mut rows: Vec<&Hit> = walked.hits.iter().filter(|hit| hit.file == file).collect();
        rows.sort_by_key(|hit| (hit.tier.min(Tier::Source), hit.line));
        rows
    };
    // First every file gets a few, then the first files get the rest.
    let mut taken: Vec<usize> = vec![0; order.len()];
    let mut total = 0;
    for pass in [ROWS_PER_FILE_FIRST, usize::MAX] {
        for (at, file) in order.iter().enumerate() {
            let available = rows_of(*file).len().min(pass);
            while taken[at] < available && total < SHOWN_LINES {
                taken[at] += 1;
                total += 1;
            }
        }
    }

    let mut text = String::new();
    let mut shown = vec![0; walked.files.len()];
    'files: for (at, file) in order.iter().enumerate() {
        if taken[at] == 0 {
            continue;
        }
        let tier = best(*file);
        let heading = match tier {
            Tier::Test => format!("\n{} (test)\n", walked.files[*file]),
            Tier::Generated => format!("\n{} (generated or ignored)\n", walked.files[*file]),
            Tier::Named | Tier::Definition | Tier::Source => {
                format!("\n{}\n", walked.files[*file])
            }
        };
        let mut group = heading;
        for (in_group, hit) in rows_of(*file).into_iter().take(taken[at]).enumerate() {
            let mark = if hit.tier <= Tier::Definition {
                "  (definition)"
            } else {
                ""
            };
            let row = format!("  {}: {}{mark}\n", hit.line, row_text(&hit.text));
            if text.len() + group.len() + row.len() > room {
                if in_group > 0 {
                    text.push_str(&group);
                }
                break 'files;
            }
            group.push_str(&row);
            shown[*file] += 1;
        }
        text.push_str(&group);
    }
    (text, shown)
}

/// The declarations whose names or comments share words with the needle.
fn declarations_part(needle: &str, walked: &Walked, room: usize) -> String {
    let found = codebase::retrieve(&walked.symbols, needle);
    if found.is_empty() {
        return String::new();
    }
    let mut text = format!("\n{DECLARATIONS_HEADING} \"{needle}\":\n");
    let mut current = "";
    for symbol in found.into_iter().take(SHOWN_DECLARATIONS) {
        let mut piece = String::new();
        if symbol.path != current {
            piece.push_str(&format!("{}\n", symbol.path));
        }
        piece.push_str(&format!("  {}\n", symbol.row()));
        if text.len() + piece.len() > room {
            break;
        }
        current = &symbol.path;
        text.push_str(&piece);
    }
    text
}

/// What was read and what was not, and how to include it.
fn closing(walked: &Walked, options: &Options) -> String {
    let skipped = &walked.skipped;
    let mut not = Vec::new();
    if !options.include_ignored {
        not.push(String::from(
            "files and folders that .gitignore or .ignore files name",
        ));
    }
    if !skipped.folders.is_empty() {
        let folders: Vec<&str> = skipped.folders.iter().map(String::as_str).collect();
        not.push(format!(
            "the {} {} (build output, dependencies or repository data)",
            if folders.len() == 1 {
                "folder"
            } else {
                "folders"
            },
            folders.join(", ")
        ));
    }
    let mut kind = |list: &[String], one: &str, many: &str| {
        if !list.is_empty() {
            not.push(format!(
                "{} ({})",
                counted(list.len(), one, many),
                such_as(list)
            ));
        }
    };
    kind(
        &skipped.generated,
        "lock or minified file",
        "lock or minified files",
    );
    kind(&skipped.binary, "binary file", "binary files");
    kind(
        &skipped.large,
        &format!("file over {} bytes", skipped.ceiling),
        &format!("files over {} bytes", skipped.ceiling),
    );
    kind(
        &skipped.not_utf8,
        "file that is not UTF-8",
        "files that are not UTF-8",
    );
    kind(
        &skipped.links,
        "symbolic link, which is never followed",
        "symbolic links, which are never followed",
    );
    kind(
        &skipped.unreadable,
        "entry that could not be read",
        "entries that could not be read",
    );

    let mut said = format!("Searched {}.", counted(walked.searched, "file", "files"));
    if !not.is_empty() {
        said.push_str(&format!(" Not searched: {}.", not.join("; ")));
    }
    if !options.include_ignored {
        said.push_str(
            " Set include_ignored to true to search ignored files, build output, dependency \
             folders and lock files too.",
        );
    }
    said.push('\n');
    said
}

#[cfg(test)]
mod tests;
