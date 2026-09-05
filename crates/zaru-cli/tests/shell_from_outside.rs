// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The shell, driven by a caller outside this crate, over a session a check
//! created.
//!
//! # Two kinds of evidence, and this file carries both, labelled
//!
//! **The pipe half is evidence about the binary.** It runs the built `zaru`
//! with `--resume <id>` under a scratch `HOME`, with standard output on a pipe
//! rather than a terminal, and asserts what it printed — which is [ADR-0010]
//! D4's out-of-session reading, for the reader it was written for.
//!
//! **The terminal half is evidence about the mechanism and must not be quoted
//! as evidence about the binary.** No check here can allocate a pseudo-terminal
//! without a dependency ADR-0003 D2's table does not name, so the tty branch is
//! driven through the same [`Surface`] the product implements, over ratatui's
//! `TestBackend`, and the rendered rows are read out of the buffer. What that
//! establishes is the pump, the adapters and the frame. **Whether a person can
//! read the result is not something any check here says anything about** —
//! [ADR-0005]'s own "someone has to look at it".
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [`Surface`]: zaru_cli::terminal::Surface

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::{Path, PathBuf};
use std::process::Command;
use zaru_cli::session::{Phase, Record, SessionId, SessionStore, ToolCall, Transcript};
use zaru_cli::terminal::driver::{Restore, Surface, run};
use zaru_cli::terminal::vocabulary::{Transcript as Pane, Vocabulary};
use zaru_cli::terminal::{NOTHING_CACHED, NotesTrie};
use zaru_notes::trie::{CachedEntry, EntryKind};
use zaru_tui::shell::{COMPOSER_ROWS, Input, Key, Shell, Status};

/// A value planted in the session's transcript, so what is read back could
/// only have come from the file the check wrote.
const NONCE: &str = "planted-4d81f";

/// A scratch `HOME` with one session in it.
struct Scratch {
    path: PathBuf,
    id: SessionId,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "zaru-shell-from-outside-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("project")).expect("a scratch working directory");

        // The session is built through the crate's public door -- the store,
        // the transcript writer, the record -- rather than by writing JSON, so
        // what the shell reads back is what the product writes.
        // `SessionStore::default_root()` is `~/.zaru`, so a store whose root
        // is the scratch home itself would put sessions somewhere the binary
        // does not look. Measured by running the binary against one.
        let store =
            SessionStore::open(path.join(".zaru")).expect("a session store under the scratch home");
        let id = SessionId::mint(&zaru_cli::session::SystemWallClock).expect("a ULID");
        let session = store.start(id.clone()).expect("a session directory");
        let mut transcript =
            Transcript::append_to(session.transcript_path()).expect("a transcript to append to");
        for line in [
            format!("fs.read `notes/{NONCE}.md`"),
            format!("cmd.run `just test` · {NONCE}"),
        ] {
            transcript
                .record(&Record::ToolCall(ToolCall {
                    line,
                    out_of_tree: false,
                    destructive: false,
                    phase: Phase::Completed,
                }))
                .expect("a record appended");
        }
        Self { path, id }
    }

    fn project(&self) -> PathBuf {
        self.path.join("project")
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A terminal a check owns: a script, a buffer, and a count of restores.
struct Recorded {
    terminal: Terminal<TestBackend>,
    script: std::vec::IntoIter<Input>,
    restores: usize,
    frames: Vec<Vec<String>>,
}

impl Recorded {
    fn of(keys: Vec<Input>) -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(72, 14)).expect("test terminal"),
            script: keys.into_iter(),
            restores: 0,
            frames: Vec::new(),
        }
    }
}

impl Restore for Recorded {
    fn restore(&mut self) {
        self.restores += 1;
    }
}

impl Surface for Recorded {
    fn draw(&mut self, shell: &Shell) -> std::io::Result<()> {
        self.terminal
            .draw(|frame| shell.render(frame, frame.area()))?;
        let buffer = self.terminal.backend().buffer();
        self.frames.push(
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect(),
        );
        Ok(())
    }

    fn next(&mut self) -> std::io::Result<Option<Input>> {
        Ok(self.script.next())
    }
}

fn press(key: Key) -> Input {
    Input {
        key,
        ctrl: false,
        alt: false,
        shift: false,
    }
}

fn typed(text: &str) -> Vec<Input> {
    let mut keys: Vec<Input> = text.chars().map(|ch| press(Key::Char(ch))).collect();
    keys.push(press(Key::Enter));
    keys
}

/// A caller outside this crate opens the shell over a session, sees the
/// transcript, runs a slash command, and leaves.
///
/// This is the arc's real artefact, minus the three system calls a check
/// cannot make. Every row below was read out of `TestBackend`'s buffer.
#[test]
fn a_caller_outside_this_crate_opens_a_shell_over_a_session_and_leaves() {
    let scratch = Scratch::new("open");

    // `shell_for` reads `~/.zaru/sessions`, which is `std::env::home_dir` and
    // therefore the process's own `HOME`. A check cannot set that -- the
    // workspace denies `unsafe_code` and `set_var` is unsafe in this edition --
    // so the pane is built from the store the check owns, through the same
    // adapter `shell_for` uses. What that leaves unasserted is `shell_for`'s
    // own three lines, and the pipe check below covers the same path in the
    // product.
    let store = SessionStore::reading(scratch.path().join(".zaru"));
    let directory = store.sessions_directory().join(scratch.id.as_str());
    let resumed = zaru_cli::session::resume(&directory, usize::MAX).expect("the session resumes");
    assert_eq!(
        resumed.tail.len(),
        2,
        "the session the check wrote did not come back, so nothing below is about a session"
    );

    let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
    shell.refresh(&Pane::of(&resumed.tail));

    let mut keys = typed("/runtime");
    keys.extend(typed("/exit"));
    let mut surface = Recorded::of(keys);
    let runner = zaru_cli::cli::Run {
        version: env!("CARGO_PKG_VERSION"),
        report_at: env!("CARGO_PKG_REPOSITORY"),
    };
    let trie = NotesTrie::nothing_cached("zaru");
    shell.composer_mut().set_absence(trie.absence());
    let pumped = run(&mut shell, &mut surface, &runner, &trie, &Vocabulary).expect("the pump");

    let opened = surface
        .frames
        .first()
        .expect("no frame was painted")
        .clone();
    let last = surface.frames.last().expect("no frame was painted").clone();
    println!("-- the frame the shell opened with, read out of TestBackend --");
    for row in &opened {
        println!("   |{row}|");
    }
    println!("-- the last frame --");
    for row in &last {
        println!("   |{row}|");
    }
    println!("   exit {}", pumped.exit.code());

    // ADR-0010 D4's "re-renders the last stretch of transcript so the user can
    // see where they were" is the frame the shell opens with.
    assert!(
        opened[0].starts_with("runtime.tier = bare · session "),
        "ADR-0001 D2's status line is not the top row: {:?}",
        opened[0]
    );
    assert!(
        opened.join("\n").contains(NONCE),
        "the session's own transcript is not on the opening frame: {opened:#?}"
    );

    // ADR-0001 D2's status line renders the tier **at all times**, so it is
    // still there after a command has filled the pane.
    assert!(
        last[0].starts_with("runtime.tier = bare · session "),
        "the status line went away once the pane filled: {:?}",
        last[0]
    );

    // ADR-0015 D2's "one operation, two entry points": the slash spelling
    // produced what the subcommand produces.
    let said: Vec<String> = shell
        .pane_lines()
        .into_iter()
        .map(|line| line.text)
        .collect();
    assert!(
        said.iter()
            .any(|line| line == "runtime.tier = bare (from built-in)"),
        "`/runtime` did not produce ADR-0001 D2's datum: {said:#?}"
    );
    assert!(
        said.iter().any(|line| line.contains(NONCE)),
        "the transcript left the pane when a command ran"
    );

    assert_eq!(pumped.exit.code(), 0, "the shell did not exit 0 on `/exit`");
    assert_eq!(
        surface.restores, 0,
        "the pump restored the terminal itself; giving it back is the guard's, and a pump that \
         did both would hand it back twice"
    );
}

/// The pipe half, and this one **is** evidence about the binary.
///
/// ADR-0010 D4's other reader. Standard output is a pipe rather than a
/// terminal, so no shell opens and the transcript's own bytes are printed.
#[test]
fn the_binary_prints_the_transcripts_bytes_when_nobody_is_watching() {
    let scratch = Scratch::new("pipe");

    let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(["--resume", scratch.id.as_str()])
        .env_clear()
        .env("HOME", scratch.path())
        .current_dir(scratch.project())
        .output()
        .expect("failed to execute the built zaru binary");

    let stdout = String::from_utf8(output.stdout).expect("zaru printed invalid UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("zaru printed invalid UTF-8 on stderr");
    let code = output
        .status
        .code()
        .expect("the binary was killed by a signal");

    println!("-- zaru --resume {} , stdout on a pipe --", scratch.id);
    for line in stdout.lines() {
        println!("   {line}");
    }
    for line in stderr.lines() {
        println!(" ! {line}");
    }
    println!("   exit {code}");

    assert!(
        stdout.contains(&format!("session {}", scratch.id)),
        "the print does not name the session it resumed"
    );
    assert!(
        stdout.contains(NONCE),
        "the transcript's own bytes are not on standard output"
    );
    assert!(
        stdout.contains("2 record(s) in the transcript"),
        "the resume did not read the two records the check wrote"
    );
    // **This asserted `2` or `4` until 2026-09-05, and the assertion was doing
    // its job when it changed.** It was written by the `tui-shell` arc to pin
    // an asymmetry it had found and could not settle: the terminal path exits
    // `0` on `/exit` while the non-terminal path exited with the no-provider
    // refusal's code, over a command that asks for no provider. ADR-0010 D4's
    // Update recorded it as wanting a person's answer; it was answered on
    // 2026-09-05 under Jeshua's directive of that day, as an accepted Update
    // on D4 and open to his veto, and the answer is `0`.
    //
    // The reason, which is what this comment exists to carry rather than the
    // number: **a bare `--resume` that printed the transcript did what it was
    // asked.** ADR-0016 D5's reader here is a wrapper — "CI wraps this
    // harness" — and to a wrapper a non-zero code means the thing it asked for
    // did not happen. It happened; the bytes are on standard output and this
    // check has just read them. A refusal about there being no provider is an
    // answer to a question nobody put.
    //
    // So the pin is kept and inverted rather than deleted: deciding it back
    // reddens here, exactly as deciding it forward reddened here.
    assert_eq!(
        code, 0,
        "a bare `--resume` printed the transcript it was asked for and then exited {code}. \
         ADR-0010 D4: a resume asks for no task, so a refusal about running one is not what it \
         ended with. If this was deliberate it is a decision, and D4 is where it goes"
    );
    assert!(
        stderr.is_empty(),
        "a resume that succeeded wrote to standard error, so a wrapper reading it cannot tell \
         this run from a failed one: {stderr:?}"
    );
}

/// The absence a person would otherwise have to discover: nothing here starts
/// a session.
///
/// ADR-0010 D1 makes a session a directory on disk, and `zaru --resume` on a
/// session that does not exist must not create one.
#[test]
fn resuming_a_session_that_does_not_exist_creates_nothing() {
    let scratch = Scratch::new("absent");
    let absent = "01JQZX8N3K4M5P6R7S8T9V0W1X";

    let output = Command::new(env!("CARGO_BIN_EXE_zaru"))
        .args(["--resume", absent])
        .env_clear()
        .env("HOME", scratch.path())
        .current_dir(scratch.project())
        .output()
        .expect("failed to execute the built zaru binary");

    let code = output
        .status
        .code()
        .expect("the binary was killed by a signal");
    println!("   exit {code}");
    assert_ne!(code, 0, "resuming a session that does not exist succeeded");
    assert!(
        !scratch.path().join(".zaru/sessions").join(absent).exists(),
        "a directory was created for a session that was only asked about"
    );
}

/// The strip a stranger can drive, with a fast tier they populated themselves.
///
/// # What this is evidence about
///
/// The **mechanism**, reached through `zaru-cli`'s public door using nothing
/// the crate does not export: `zaru_notes::trie::CachedEntry`,
/// `NotesTrie::attached_to`, and the same `terminal::driver::run` the binary
/// calls. **It is not evidence about the `zaru` binary**, which cannot populate
/// a trie on any machine today — reaching Nuclear Notes needs a transport that
/// is a port with no implementation, and a token nothing here can add. What the
/// binary does show is the other half of this check: the absence line.
///
/// It prints both strips. Run it with
/// `cargo test -p zaru-cli --test shell_from_outside -- --nocapture` to read
/// what a user would see.
#[test]
fn a_caller_outside_this_crate_populates_the_fast_tier_and_reads_the_strip() {
    let scratch = Scratch::new("strip");
    let store = SessionStore::reading(scratch.path().join(".zaru"));
    let directory = store.sessions_directory().join(scratch.id.as_str());
    let resumed = zaru_cli::session::resume(&directory, usize::MAX).expect("the session resumes");

    let corpus = vec![
        CachedEntry::new(
            "zaru",
            "architecture/bóunded-contexts",
            "Bóunded Contexts ✦",
            EntryKind::Page,
        ),
        CachedEntry::new("zaru", "atoms/mémbrane", "Mémbrane ✦", EntryKind::Atom),
        CachedEntry::new("zaru", "operations/téstingi", "Tésting ✦", EntryKind::Page),
    ];

    for (label, trie, typing, expected) in [
        (
            "a populated fast tier",
            NotesTrie::attached_to(corpus, "zaru"),
            "té",
            vec!["Tésting ✦".to_owned()],
        ),
        (
            "nothing cached, which is every machine today",
            NotesTrie::nothing_cached("zaru"),
            "té",
            vec![NOTHING_CACHED.to_owned()],
        ),
    ] {
        let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
        shell.refresh(&Pane::of(&resumed.tail));
        shell.composer_mut().set_absence(trie.absence());

        let keys: Vec<Input> = typing.chars().map(|ch| press(Key::Char(ch))).collect();
        let mut surface = Recorded::of(keys);
        let runner = zaru_cli::cli::Run {
            version: env!("CARGO_PKG_VERSION"),
            report_at: env!("CARGO_PKG_REPOSITORY"),
        };
        run(&mut shell, &mut surface, &runner, &trie, &Vocabulary).expect("the pump");

        let frame = surface.frames.last().expect("a frame was painted").clone();
        println!("-- {label}: the frame after typing {typing:?} --");
        for row in &frame {
            println!("   |{row}|");
        }

        let input_row = frame.len() - usize::from(COMPOSER_ROWS);
        let strip: Vec<String> = frame[input_row + 1..]
            .iter()
            .map(|row| row.trim_end().to_owned())
            .filter(|row| !row.is_empty())
            .collect();
        assert_eq!(
            strip, expected,
            "{label}: the strip's rows, read out of TestBackend's buffer and compared against \
             literals written in this check"
        );
        assert_eq!(
            frame[input_row].trim_end(),
            typing,
            "and the input row holds what was typed, unmoved by whatever the strip showed"
        );
    }
}
