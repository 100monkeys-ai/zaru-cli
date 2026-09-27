// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! No sentence a user reads has a hole in it.
//!
//! # The rule, and the clause it comes from
//!
//! [ADR-0016] D2's worked example is a sentence:
//!
//! ```text
//! ✗ provider: no credential for alias `default`
//!   set one:  zaru config set provider.anthropic.key <key>
//!   or:       ZARU_ANTHROPIC_KEY=<key>
//! ```
//!
//! `operations/harness-look-and-feel-audit-2` row 7 measured the smallest
//! possible failure of it. `zaru providers keys add gemini` with a malformed
//! `ZARU_CREDENTIAL_KEY` — the first error a person on a headless machine
//! meets, because that machine has no keyring — answered a 191-byte remedy
//! line made of `set ZARU_CREDENTIAL_KEY to exactly 64 lower-case hexadecimal`,
//! then twenty-two spaces, then `characters; its current value is not, and
//! neither it nor its length is`, then twenty-two more, then `quoted
//! anywhere`. The literal was one source line carrying the indentation of the
//! source lines it had been joined from, and six more literals carried twelve
//! such runs between them.
//!
//! # The unit is a sentence, never a line, and that is the whole design
//!
//! Two consecutive spaces are not wrong everywhere. This tree holds forty-odd
//! literals that carry them deliberately and every one of them is **layout**:
//! a column gutter composed with `{:width$}`, a hand-aligned two-column block,
//! a leading indent on a continuation line. A check over rendered lines would
//! need to exempt each by name, which is a list that goes stale in exactly the
//! way the sentence this file closes went stale.
//!
//! So every arm below reads the **parts** a sentence is written as — a
//! summary, a statement, a headline, a lead, a text — and never a line
//! composed from them. Layout is out of reach by construction rather than by
//! a rule somebody maintains. Where an arm has no choice but to read a line,
//! which is the built artefact's, it strips the leading indent the writer adds
//! and reads what is left.
//!
//! **The fence is a document.** `manifest::TEMPLATE` is a TOML file this
//! harness writes to a user's own disk, and its `name` and `run` keys are
//! aligned on purpose. It is a document rather than a sentence, it is reached
//! by no arm here, and
//! [`a_document_the_harness_writes_keeps_its_alignment`] asserts that it still
//! holds its run — which is also what stops every assertion in this file being
//! satisfied by a predicate that answers `None` for everything.
//!
//! # The other half of this rule is in-crate and is the half with teeth
//!
//! `failure::tests::every_mapped_refusal_says_a_sentence_rather_than_a_laid_out_line`
//! walks all thirty-odd mapped refusals, and each mapping is a wildcard-free
//! `match`, so a variant added tomorrow cannot arrive unseen. What is here is
//! the surfaces a reader is actually on, in the shape
//! `record_citations_from_outside` established for the citation.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use zaru_cli::failure::{
    Action, Class, Classified, DefectReport, Expected, Location, Remedy, SessionEvidence,
    Statement, Wait,
};
use zaru_cli::session::{FailureLine, Record};
use zaru_cli::terminal::Transcript;
use zaru_cli::tools::Tier;
use zaru_tui::shell::TranscriptSource;

/// The length of the longest run of two or more spaces in `text`, if any.
///
/// Two rather than three, because two is what a joined line leaves when the
/// lines it was joined from were indented by one column — and because a
/// sentence never wants two.
fn longest_run_of_spaces(text: &str) -> Option<usize> {
    text.as_bytes()
        .split(|byte| *byte != b' ')
        .map(<[u8]>::len)
        .filter(|run| *run >= 2)
        .max()
}

fn said(text: &str) -> Statement {
    Statement::new(text).expect("the fixtures here carry no control character")
}

/// One failure of each of D1's five classes, walked from `Class::ALL` so a
/// sixth arrives as a missing arm rather than as silent coverage.
fn one_of_every_class() -> Vec<Classified> {
    Class::ALL
        .into_iter()
        .map(|class| match class {
            Class::Expected => Classified::Expected(Expected::new(said(
                "the validator `build` was not satisfied",
            ))),
            Class::UserCorrectable => Classified::UserCorrectable {
                statement: said(
                    "the key runtime.tier was set to \"nonsense\", which names no tier",
                ),
                remedy: Remedy::one(Action::described(said("set it to one of the three"))),
            },
            Class::Environmental => Classified::Environmental {
                statement: said("the provider could not be reached"),
                wait: Wait::NoWaitWillHelp(said("running the same command again is the retry")),
            },
            Class::Capability => Classified::Capability {
                statement: said("nothing wires a client to a loop in this build"),
                offered_by: Tier::Bare,
            },
            Class::Defect => Classified::Defect(DefectReport::new(
                "0.0.0",
                "https://github.com/100monkeys-ai/zaru-cli/issues",
                Location::unknown(),
                SessionEvidence::NoSessionExists,
            )),
        })
        .collect()
}

/// **Arm 1.** The surfaces that are not failures at all.
///
/// Every command summary, every flag summary and value name, what `zaru init`
/// prints, and `--help`'s closing paragraph read as the paragraph it is rather
/// than as the rows it is wrapped into. The command and flag *rows* are
/// deliberately absent: they are two columns with a gutter between them, and
/// the gutter is the thing this file must not see.
#[test]
fn no_surface_a_user_reads_holds_a_gap() {
    println!("-- ADR-0016 D2: the surfaces that are not failures --");

    for namespace in zaru_cli::cli::Namespace::ALL {
        for (spelling, summary) in zaru_cli::cli::help::summaries_of(namespace) {
            println!("command {spelling}: {summary}");
            assert!(
                longest_run_of_spaces(summary).is_none(),
                "the summary of `{spelling}` renders with a hole in it: {summary}"
            );
        }
    }

    for flag in zaru_cli::cli::Flag::ALL {
        for (part, text) in [
            ("spelling", flag.spelling()),
            ("summary", flag.summary()),
            ("value name", flag.value_name().unwrap_or("")),
        ] {
            println!("flag {} {part}: {text}", flag.spelling());
            assert!(
                longest_run_of_spaces(text).is_none(),
                "the {part} of {} renders with a hole in it: {text}",
                flag.spelling()
            );
        }
    }

    for line in zaru_cli::cli::render::initialised(Path::new("/tmp/zaru.toml")) {
        println!("init: {line}");
        assert!(
            longest_run_of_spaces(line.trim_start()).is_none(),
            "`zaru init` renders a line with a hole in it: {line}"
        );
    }

    // The closing paragraph is one sentence wrapped into rows, so it is read
    // as one sentence: a hole that fell on a row boundary would otherwise be
    // invisible to this arm and visible to every reader.
    let printed = zaru_cli::cli::help::lines("0.0.0");
    let opens = printed
        .iter()
        .position(|line| line.starts_with("`zaru "))
        .expect("the closing paragraph opens with the invocation it describes");
    let paragraph = printed[opens..].join(" ");
    println!("paragraph: {paragraph}");
    assert!(
        longest_run_of_spaces(&paragraph).is_none(),
        "`--help`'s closing paragraph renders with a hole in it: {paragraph}"
    );
}

/// **Arm 2.** The same failures, as the transcript pane renders them.
///
/// The pane goes through `terminal::vocabulary` rather than the binary's own
/// writer, so an arm over one says nothing about the other. Each row's leading
/// indent is the pane's own and is stripped before the row is read.
#[test]
fn no_pane_line_a_user_reads_holds_a_gap() {
    println!("-- ADR-0016 D2: what the transcript pane carries --");
    let records: Vec<Record> = one_of_every_class()
        .iter()
        .map(|classified| Record::Failure(FailureLine::of(classified)))
        .collect();

    let lines = Transcript::of(&records).lines();
    assert!(!lines.is_empty(), "five failures painted no line at all");
    for line in lines {
        println!("{:?}  {}", line.register, line.text);
        assert!(
            longest_run_of_spaces(line.text.trim_start()).is_none(),
            "a pane line renders with a hole in it: {}",
            line.text
        );
    }
}

/// **Arm 3.** Every refusal a caller outside the crate can raise, read through
/// its own `Display`.
///
/// This is the arm that reaches the *raising sites* rather than a projection,
/// and it is `record_citations_from_outside`'s arm 5 with a different
/// predicate over the same corpus and the same floor — deliberately, because a
/// second list of refusals would be a second thing to keep current.
#[test]
fn no_refusal_a_caller_can_raise_holds_a_gap() {
    println!("-- ADR-0016 D2: what each raising site composed --");

    let mut rendered: Vec<(&str, String)> = Vec::new();
    let mut note = |what: &'static str, text: String| rendered.push((what, text));

    note(
        "Statement::new(\"\")",
        Statement::new("")
            .expect_err("an empty statement is refused")
            .to_string(),
    );
    for offered in ["", ".", "a:b", "a\u{7}b"] {
        note(
            "Alias::new",
            zaru_cli::credentials::Alias::new(offered)
                .expect_err("this alias is refused")
                .to_string(),
        );
    }
    for offered in ["", "a\u{7}b"] {
        note(
            "Key::new",
            zaru_cli::config::Key::new(offered)
                .expect_err("this key is refused")
                .to_string(),
        );
    }
    note(
        "RetentionWindow::new(0)",
        zaru_cli::session::RetentionWindow::new(core::time::Duration::ZERO)
            .expect_err("a zero window is refused")
            .to_string(),
    );
    note(
        "OutputBudget::new(0)",
        zaru_cli::tools::OutputBudget::new(0)
            .expect_err("a zero budget is refused")
            .to_string(),
    );
    note(
        "RetryCeiling::new(0)",
        zaru_cli::failure::RetryCeiling::new(0)
            .expect_err("a zero ceiling is refused")
            .to_string(),
    );
    for offered in ["", "not-a-ulid", "ZZZZZZZZZZZZZZZZZZZZZZZZZZ"] {
        note(
            "SessionId::parse",
            zaru_cli::session::SessionId::parse(offered)
                .expect_err("this id is refused")
                .to_string(),
        );
    }
    for offered in ["not a url", "file:///etc/passwd", "https://"] {
        note(
            "RequestedUrl::parse",
            zaru_cli::web::RequestedUrl::parse(offered)
                .expect_err("this url is refused")
                .to_string(),
        );
    }
    for offered in ["", "a \"", "a \\"] {
        note(
            "CommandLine::split",
            zaru_cli::process::CommandLine::split(offered)
                .expect_err("this command line is refused")
                .to_string(),
        );
    }
    for offered in ["", "a\u{7}b"] {
        note(
            "validator Name::new",
            zaru_core::iteration::validator::Name::new(offered)
                .expect_err("this validator name is refused")
                .to_string(),
        );
    }
    for refusal in [
        zaru_tui::shell::Refused::Empty,
        zaru_tui::shell::Refused::NotBuilt {
            slash: "/stack",
            governs: "AEGIS component fetch and status",
        },
        zaru_tui::shell::Refused::UnknownCommand {
            offered: "help".to_owned(),
            nearest: Some("/models"),
        },
        zaru_tui::shell::Refused::VerbMissing {
            slash: "/session",
            verbs: &["resume", "continue"],
        },
        zaru_tui::shell::Refused::UnknownVerb {
            slash: "/session",
            offered: "restart".to_owned(),
            nearest: Some("resume"),
        },
    ] {
        note("zaru-tui Refused", refusal.to_string());
    }

    assert!(
        rendered.len() >= 20,
        "the corpus shrank to {} refusals; a check over fewer sites than it had is a check that \
         stopped covering what it covered",
        rendered.len()
    );
    for (what, text) in &rendered {
        println!("{what}: {text}");
        assert!(
            longest_run_of_spaces(text).is_none(),
            "{what} composed a sentence with a hole in it: {text}"
        );
    }
}

/// **Arm 4.** The built artefact, which is where row 7 was measured.
///
/// Three refusals, none of which needs a provider key or a network. The last
/// is the row itself: `providers keys add gemini` on a machine whose
/// `ZARU_CREDENTIAL_KEY` is not a key, which is the ordinary state of a
/// headless machine and is the first error a person there meets.
///
/// **Standard error alone, and that is the principled line rather than an
/// exemption.** [ADR-0016] clause 1 puts a failure's projection on standard
/// error and the data on standard output, for D5's own reason that CI wraps
/// this harness; so standard error is where the sentences are, and standard
/// output is where the tables are — `--help`'s two-column rows, `zaru models`'
/// three — which arm 1 reads part by part instead. An arm that read both would
/// have to carry a list of laid-out surfaces, which is the list that goes
/// stale.
///
/// Each line's leading indent is the writer's own — `failure::present` opens a
/// remedy line with two columns — so it is stripped and what is left is read.
/// Nothing else is stripped: a hole in the middle of a sentence stays where it
/// is.
///
/// **Each invocation must say something**, asserted, because the defect
/// `silent-refusal` closed on 2026-09-15 was a refusal path that printed
/// nothing at all — and an arm over an empty stream is green for the wrong
/// reason.
#[test]
fn the_built_binary_says_no_sentence_with_a_hole_in_it() {
    let home = scratch("artefact");
    std::fs::create_dir_all(&home).expect("a scratch home");

    // A value that is not a key, so the store refuses the sealing rather than
    // the credential. Nothing here is a credential: the bytes on standard
    // input are never read, because the key is refused first.
    let not_a_key = "zzznothex";

    // Whether each invocation reads standard input. Only `providers keys add`
    // does: it reads the value before the store seals it, so with an empty
    // pipe the refusal is the empty secret's and the sealing sentence — the
    // one row 7 measured — is never reached. The word it is given is not a
    // credential and never becomes one; the sealing key is refused first.
    //
    // **The other two are handed no pipe at all.** Until 2026-09-27 every
    // invocation was handed a word on a pipe, and a child that refuses
    // without reading can exit before the word is written: the write then
    // fails with `EPIPE` and this check went red for a race of its own, not
    // for a sentence. Measured by `test-env-isolation` when the decoy re-run
    // doubled how often the race was run.
    for (arguments, reads_standard_input) in [
        (["--runtime", "nonsense", "runtime"].as_slice(), false),
        (["sessions", "rm", "not-a-ulid"].as_slice(), false),
        (["providers", "keys", "add", "gemini"].as_slice(), true),
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zaru"))
            .args(arguments)
            .env_clear()
            .env("HOME", &home)
            .env("ZARU_CREDENTIAL_KEY", not_a_key)
            .current_dir(&home)
            .stdin(if reads_standard_input {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to execute the built zaru binary");
        if reads_standard_input {
            child
                .stdin
                .take()
                .expect("the child was given a pipe")
                .write_all(b"not-a-credential\n")
                .expect("the child reads its input");
        }
        let output = child
            .wait_with_output()
            .expect("the built zaru binary answered");

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        println!("-- zaru {} --\n{stderr}", arguments.join(" "));

        assert!(
            !stderr.trim().is_empty(),
            "`zaru {}` refused and said nothing, so this arm has nothing to read",
            arguments.join(" ")
        );
        for line in stderr.lines() {
            let sentence = line.trim_start();
            assert!(
                longest_run_of_spaces(sentence).is_none(),
                "`zaru {}` put a sentence with a hole in it on standard error: {line:?}",
                arguments.join(" ")
            );
        }
    }

    std::fs::remove_dir_all(&home).ok();
}

/// **Arm 5.** Every sentence in the tree, whether a reader is on it today or
/// not.
///
/// The arms above drive the surfaces the defect was measured on, which is what
/// keeps them honest about reachability. This one is the sweep the ruling
/// asked for, and it is worth having for a reason the citation sweep's own
/// argument against greps does not reach: **a hole in a sentence is wrong
/// whether or not the sentence is reachable today.** A citation that no reader
/// meets costs nothing; a joined literal waiting in an unreachable arm becomes
/// a hole the day the arm becomes reachable, which is exactly how the six
/// literals beside row 7's arrived.
///
/// # What a sentence is, stated once and mechanically
///
/// **A literal is a sentence when it opens with a lower-case letter or a
/// backtick and carries no width specifier.** Everything else this tree writes
/// is layout or a document, and each is excluded by the shape it opens with
/// rather than by being named:
///
/// - a row opens with its own indent — `"  {id}"`, `"  completed:      {step}"`
/// - a format string opens with a placeholder — `"{line}  {EFFECTIVE_MARKER}"`
/// - a column is composed with a width — `"  {spelling:width$}  {summary}"`
/// - the manifest template opens with `[project]`, and it is the fence: a TOML
///   document written to a user's own disk, whose `name` and `run` keys are
///   aligned on purpose
/// - a fixture staging a compiler's own indented output opens with `{NONCE}`
///
/// So there is **no allow-list**, which matters because an allow-list is a
/// list that goes stale in exactly the way the sentence `help-truth` closed
/// went stale. Measured when this arc landed: the rule flags eight literals on
/// the tree before it and none on the tree after, so the exemption costs no
/// false negative anywhere in six crates.
///
/// The mutant: any of the seven literals re-spelled as it was, which this
/// names with its file and line.
#[test]
fn no_sentence_in_the_tree_holds_a_gap() {
    let mut holed = Vec::new();
    let mut sentences = 0_usize;
    for file in product_sources() {
        let source = std::fs::read_to_string(&file).expect("a source file this crate ships");
        for (line, literal) in literals(&source) {
            if !is_a_sentence(&literal) {
                continue;
            }
            sentences += 1;
            if let Some(run) = longest_run_of_spaces(&literal) {
                holed.push(format!(
                    "{}:{line} carries a run of {run} spaces inside a sentence: {literal}",
                    file.display()
                ));
            }
        }
    }

    assert!(
        sentences >= 400,
        "the sweep found only {sentences} sentences in the product tree, so it is reading \
         something other than the sources it believes it is"
    );
    assert!(
        holed.is_empty(),
        "ADR-0016 D2's worked example is a sentence. {} literal(s) in this tree are not: {holed:#?}",
        holed.len()
    );
}

/// Every product source file: no `tests/`, no `tests.rs`, no `fixtures.rs`,
/// and everything from the first module-level `#[cfg(test)]` onwards dropped.
///
/// Test code carries the same joined literals in its assertion messages and
/// they reach no reader, so they are out of scope rather than fixed — said
/// here rather than left for someone to discover as an inconsistency.
fn product_sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the crates directory resolves from this crate's manifest");
    let mut found = Vec::new();
    walk(&crates, &mut found);
    found.sort();
    assert!(
        found.len() >= 60,
        "the walk found {} product source files across six crates, which is too few to be the \
         tree",
        found.len()
    );
    found
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("a directory this crate ships") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests" || name == "target" {
                continue;
            }
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests.rs" || name == "fixtures.rs" {
                continue;
            }
            found.push(path);
        }
    }
}

/// A literal is a sentence when it opens with a lower-case letter or a
/// backtick and carries no width specifier.
///
/// See the arm above for why each half is there and what it excludes.
fn is_a_sentence(literal: &str) -> bool {
    let opens_as_prose = literal
        .chars()
        .next()
        .is_some_and(|first| first == '`' || (first.is_alphabetic() && first.is_lowercase()));
    opens_as_prose && !holds_a_width(literal)
}

/// Whether the literal carries a `{…:…}` width specifier, which is how every
/// column in this tree is composed.
fn holds_a_width(literal: &str) -> bool {
    let bytes = literal.as_bytes();
    let mut opened = None;
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            b'{' => opened = Some(at),
            b'}' => {
                if let Some(from) = opened.take()
                    && bytes[from..at].contains(&b':')
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Every string literal in `source`, with the line it opens on.
///
/// Comments are skipped, so a doc comment quoting a broken sentence is prose
/// about the rule rather than an instance of it — the discrimination
/// `Door::ALL`'s own source walk already makes. Raw strings are read whole and
/// a `\`-continuation is joined the way the compiler joins it, leading
/// whitespace dropped, so a correctly spelled literal cannot look broken here.
/// Everything from the first module-level `#[cfg(test)]` is dropped first.
fn literals(source: &str) -> Vec<(usize, String)> {
    let source = match source.find("\n#[cfg(test)]") {
        Some(at) => &source[..at],
        None => source,
    };
    let bytes: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0_usize;
    let mut line = 1_usize;
    while at < bytes.len() {
        let here = bytes[at];
        if here == '\n' {
            line += 1;
            at += 1;
        } else if here == '/' && bytes.get(at + 1) == Some(&'/') {
            while at < bytes.len() && bytes[at] != '\n' {
                at += 1;
            }
        } else if here == '/' && bytes.get(at + 1) == Some(&'*') {
            at += 2;
            while at + 1 < bytes.len() && !(bytes[at] == '*' && bytes[at + 1] == '/') {
                if bytes[at] == '\n' {
                    line += 1;
                }
                at += 1;
            }
            at += 2;
        } else if here == '\'' {
            // A char literal or a lifetime. Both are skipped; neither can
            // open a string.
            at += 1;
            if bytes.get(at) == Some(&'\\') {
                at += 2;
            }
            if bytes.get(at + 1) == Some(&'\'') {
                at += 2;
            }
        } else if here == 'r' && matches!(bytes.get(at + 1), Some('"') | Some('#')) {
            let mut end = at + 1;
            let mut hashes = 0;
            while bytes.get(end) == Some(&'#') {
                hashes += 1;
                end += 1;
            }
            if bytes.get(end) != Some(&'"') {
                at += 1;
                continue;
            }
            let opened_on = line;
            end += 1;
            let mut body = String::new();
            loop {
                let closes = bytes.get(end) == Some(&'"')
                    && (1..=hashes).all(|step| bytes.get(end + step) == Some(&'#'));
                if closes || end >= bytes.len() {
                    break;
                }
                if bytes[end] == '\n' {
                    line += 1;
                }
                body.push(bytes[end]);
                end += 1;
            }
            found.push((opened_on, body));
            at = end + 1 + hashes;
        } else if here == '"' {
            let opened_on = line;
            let mut end = at + 1;
            let mut body = String::new();
            while end < bytes.len() && bytes[end] != '"' {
                if bytes[end] == '\\' {
                    if bytes.get(end + 1) == Some(&'\n') {
                        // The compiler's own line continuation: the newline
                        // and the indentation after it are not in the value.
                        line += 1;
                        end += 2;
                        while matches!(bytes.get(end), Some(' ') | Some('\t')) {
                            end += 1;
                        }
                        continue;
                    }
                    body.push(bytes[end]);
                    if let Some(escaped) = bytes.get(end + 1) {
                        body.push(*escaped);
                    }
                    end += 2;
                    continue;
                }
                if bytes[end] == '\n' {
                    line += 1;
                }
                body.push(bytes[end]);
                end += 1;
            }
            found.push((opened_on, body));
            at = end + 1;
        } else {
            at += 1;
        }
    }
    found
}

/// **The accepting sibling.** A document the harness writes keeps its
/// alignment, and the predicate can see that it does.
///
/// Without this arm every assertion above is satisfied by a predicate that
/// answers `None` for everything, which is the vacuous green [Verification
/// lessons] §4 names. It is also the statement of the exemption: the manifest
/// `zaru init` writes is a TOML document rather than a sentence, its `name`
/// and `run` keys are aligned on purpose, and it is reached by no arm above.
/// The gutter `--help`'s own table is composed with is the second shape, held
/// here for the same reason.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_document_the_harness_writes_keeps_its_alignment() {
    let template = zaru_cli::manifest::TEMPLATE;
    println!(
        "-- the manifest template, {} bytes --\n{template}",
        template.len()
    );
    assert_eq!(
        longest_run_of_spaces(template),
        Some(3),
        "the manifest template aligns its comment gutter deliberately, and either that is gone \
         or the predicate every check in this file rests on cannot see a run at all, which \
         would make every green above vacuous:\n{template}"
    );
    let runs: Vec<&str> = template
        .lines()
        .filter(|line| line.starts_with("run"))
        .collect();
    assert!(
        runs.len() >= 3,
        "the template declares {} validators with a `run` key, which is too few to be the \
         template:\n{template}",
        runs.len()
    );
    for line in runs {
        assert!(
            line.starts_with("run  = "),
            "every `run` key in the template is padded to line up with the `name` above it, and \
             a document a person opens in an editor is the one place in this tree where that is \
             the right thing to do -- so this is the exemption, asserted rather than assumed. \
             This one is not: {line:?}"
        );
    }

    let table: Vec<String> = zaru_cli::cli::help::lines("0.0.0")
        .into_iter()
        .filter(|line| line.starts_with("  runtime"))
        .collect();
    let row = table.first().expect("`--help` lists the `runtime` command");
    println!("-- a help row --\n{row}");
    assert!(
        longest_run_of_spaces(row).is_some(),
        "`--help`'s rows are two columns with a gutter between them, and this one has none, so \
         either the table stopped being a table or the predicate is blind: {row}"
    );
}

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zaru-sentence-spacing-{label}-{}",
        std::process::id()
    ))
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
