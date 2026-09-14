// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! No string a user reads names a decision record.
//!
//! # The rule, and the clause it comes from
//!
//! [ADR-0016] D2: "An error message whose reader cannot act is a stack trace
//! with better grammar." That clause is not silent about what an error looks
//! like — it carries a worked example, in its own voice, and **the example
//! names no record**:
//!
//! ```text
//! ✗ provider: no credential for alias `default`
//!   set one:  zaru config set provider.anthropic.key <key>
//!   or:       ZARU_ANTHROPIC_KEY=<key>
//! ```
//!
//! A reader at a terminal cannot open a decision record, so a sentence whose
//! subject is one is a sentence they cannot act on. `operations/harness-look-
//! and-feel` row 20 measured the shape this closes: `✗ the program "cargo"
//! could not be started: No such file or directory (os error 2). ADR-0011 D1
//! name` — clipped at the pane's width, so the citation was all the reader got
//! of the sentence's second half.
//!
//! # What this checks, and why it is not a grep
//!
//! A grep over the sources would pass on a citation that no longer reaches a
//! reader and fail on one that never did. **Every arm below drives a path a
//! reader is actually on**: the failure projection the binary writes to
//! standard error, the same projection as the transcript pane renders it, the
//! surfaces `--help` and `zaru init` print, the manifest this harness writes
//! to the user's own disk, and the built artefact itself.
//!
//! The predicate is deliberately the crudest one that can be wrong in only one
//! direction: any `ADR-` followed by four digits. A sweep that renamed the
//! prefix would pass this and is not the failure it guards.
//!
//! # The developer-facing half is out of scope and is asserted to be
//!
//! [`a_developer_facing_string_still_names_its_record`] is the arm that stops
//! this file asserting the absence of something nothing produces. A panic's
//! message reaches no reader at all — `failure::guard`'s `Presentation` is
//! built from the report alone, and its `OwnWords` has no consumer in any
//! product tree — so `expect`, `panic!` and `compile_error!` keep their
//! citations, and the predicate must be able to see one.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::path::{Path, PathBuf};
use std::process::Command;
use zaru_cli::failure::{
    Action, Class, Classified, DefectReport, Expected, Location, Presentation, Remedy,
    SessionEvidence, Statement, Wait,
};
use zaru_cli::session::{FailureLine, Record};
use zaru_cli::terminal::Transcript;
use zaru_cli::tools::Tier;
use zaru_tui::shell::TranscriptSource;

/// Any decision record's number, in the spelling every citation in this
/// workspace uses.
fn names_a_record(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    bytes.windows(8).find_map(|window| {
        let looks_right = window.starts_with(b"ADR-")
            && window[4..].iter().all(u8::is_ascii_digit)
            && window[4] == b'0';
        looks_right.then(|| String::from_utf8_lossy(window).into_owned())
    })
}

fn said(text: &str) -> Statement {
    Statement::new(text).expect("the fixtures here carry no control character")
}

/// One failure of each of D1's five classes, carrying a citation in none of
/// them.
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

/// **Arm 1.** The projection the binary writes to standard error carries no
/// citation, in its headline or in any line under it.
///
/// Walked from `Class::ALL` rather than from a list retyped here, so a sixth
/// class arrives as a missing arm rather than as silent coverage.
#[test]
fn no_presentation_a_user_reads_names_a_decision_record() {
    println!("-- ADR-0016 D2: what standard error carries --");
    for classified in one_of_every_class() {
        let presentation = Presentation::of(&classified);
        println!("{:?}  {}", presentation.class, presentation.headline);
        assert!(
            names_a_record(&presentation.headline).is_none(),
            "a {:?} failure's headline names a record, which a reader cannot open: {}",
            presentation.class,
            presentation.headline
        );
        for line in &presentation.lines {
            let rendered = match &line.lead {
                Some(lead) => format!("{lead} {}", line.text),
                None => line.text.clone(),
            };
            println!("    {rendered}");
            assert!(
                names_a_record(&rendered).is_none(),
                "a {:?} failure's remedy names a record, which is the one line D2 says must be \
                 something the reader can act on: {rendered}",
                presentation.class
            );
        }
    }
}

/// **Arm 2.** The same failure, as the transcript pane renders it.
///
/// The pane is the other surface a person meets a failure on, and it is the
/// one row 20 was measured from. It goes through `terminal::vocabulary`, not
/// through the binary's own writer, so an arm over one says nothing about the
/// other.
#[test]
fn no_pane_line_a_user_reads_names_a_decision_record() {
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
            names_a_record(&line.text).is_none(),
            "a pane line names a record: {}",
            line.text
        );
    }
}

/// **Arm 3.** The surfaces that are not failures at all.
///
/// `--help`, every flag's summary, what `zaru init` prints, and the manifest
/// it writes to the user's own disk. The last is the strongest of the four:
/// it is a citation this harness would leave behind in a file ADR-0009 D6
/// says it reads and never writes again.
#[test]
fn no_surface_a_user_reads_names_a_decision_record() {
    println!("-- ADR-0016 D2: the surfaces that are not failures --");

    for line in zaru_cli::cli::help::lines("0.0.0") {
        println!("help: {line}");
        assert!(
            names_a_record(&line).is_none(),
            "`--help` names a record: {line}"
        );
    }

    for flag in zaru_cli::cli::Flag::ALL {
        let summary = flag.summary();
        println!("flag {}: {summary}", flag.spelling());
        assert!(
            names_a_record(summary).is_none(),
            "the flag {} names a record in its summary: {summary}",
            flag.spelling()
        );
    }

    for line in zaru_cli::cli::render::initialised(Path::new("/tmp/zaru.toml")) {
        println!("init: {line}");
        assert!(
            names_a_record(&line).is_none(),
            "`zaru init` names a record: {line}"
        );
    }

    println!(
        "manifest template, {} bytes",
        zaru_cli::manifest::TEMPLATE.len()
    );
    assert!(
        names_a_record(zaru_cli::manifest::TEMPLATE).is_none(),
        "the manifest this harness writes to a user's own disk names a record:\n{}",
        zaru_cli::manifest::TEMPLATE
    );
}

/// **Arm 5.** Every refusal a caller outside the crate can actually raise,
/// read through its own `Display`.
///
/// This is the arm that reaches the *raising sites* rather than the
/// projection. Arms 1 and 2 are built from a synthetic corpus, so they pin
/// what a `Presentation` and a pane line carry and say nothing about the
/// sentence a module composed; a citation restored to one of the modules below
/// reddens here and nowhere else.
///
/// **It is deliberately not exhaustive and says so rather than looking it.**
/// What is here is every refusal this crate's public door lets an outside
/// caller construct without a filesystem, a network or a credential. The
/// refusals that need one of those are reached by arm 4, through the binary.
#[test]
fn no_refusal_a_caller_can_raise_names_a_decision_record() {
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

    assert!(
        rendered.len() >= 15,
        "the corpus shrank to {} refusals; a check over fewer sites than it had is a check that \
         stopped covering what it covered",
        rendered.len()
    );
    for (what, text) in &rendered {
        println!("{what}: {text}");
        assert!(
            names_a_record(text).is_none(),
            "{what} composed a sentence naming a record, which a reader cannot open: {text}"
        );
    }
}

/// **Arm 4.** The built artefact, which is the only place any of this is
/// observable as a person actually meets it.
///
/// Two refusals and one help screen, all reachable with no provider key and
/// no network. `--runtime nonsense` is the capture the `record-citations`
/// arc's leg 1 measured: its headline listed the three tiers and then cited
/// the record that lists them, and its *remedy* pointed the reader at a
/// document they cannot open for a list already on the screen above.
#[test]
fn the_built_binary_names_no_decision_record_at_a_user() {
    let home = scratch("artefact");
    std::fs::create_dir_all(&home).expect("a scratch home");

    for arguments in [
        ["--runtime", "nonsense", "runtime"].as_slice(),
        ["--help"].as_slice(),
        ["init"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
            .args(arguments)
            .env_clear()
            .env("HOME", &home)
            .current_dir(&home)
            .output()
            .expect("failed to execute the built zaru binary");

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        println!("-- zaru {} --\n{stdout}{stderr}", arguments.join(" "));

        for (stream, text) in [("standard output", &stdout), ("standard error", &stderr)] {
            assert!(
                names_a_record(text).is_none(),
                "`zaru {}` put a record's number on {stream}, where a reader cannot open it:\n{text}",
                arguments.join(" ")
            );
        }
    }

    // What `zaru init` left on the user's disk, read back off the disk rather
    // than off the constant, because the constant is arm 3's and a writer that
    // added a line would pass that arm and fail here.
    let written = std::fs::read_to_string(home.join("zaru.toml")).expect("`zaru init` wrote one");
    println!("-- the written zaru.toml --\n{written}");
    assert!(
        names_a_record(&written).is_none(),
        "the manifest on the user's disk names a record:\n{written}"
    );

    std::fs::remove_dir_all(&home).ok();
}

/// **The accepting sibling.** The predicate can see a citation that is there.
///
/// Without this arm every assertion above is satisfied by a predicate that
/// answers `None` for everything, which is the vacuous green
/// [Verification lessons] §4 names. The two strings here are the two shapes
/// that deliberately keep their citations: an `expect` message, whose words
/// reach no reader because a defect report is built from the report alone,
/// and a `compile_error!`, which never runs.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_developer_facing_string_still_names_its_record() {
    let developer_facing = [
        // `crates/zaru-cli/src/providers/kind.rs`, an `expect` message.
        "ADR-0012 D3's kind segments are well-formed credential aliases",
        // `crates/zaru-cli/src/session/store.rs`, a `compile_error!`.
        "the session store enforces ADR-0010 D5's file modes through std::os::unix",
        // The sentence row 20 measured, before this arc.
        "the program \"cargo\" could not be started. ADR-0011 D1 names no allowlist of programs",
    ];
    for text in developer_facing {
        let found = names_a_record(text);
        println!("{text}  ->  {found:?}");
        assert!(
            found.is_some(),
            "the predicate every other check in this file depends on cannot see a citation that \
             is there, so every green above is vacuous: {text}"
        );
    }
}

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "zaru-record-citations-{label}-{}",
        std::process::id()
    ))
}
