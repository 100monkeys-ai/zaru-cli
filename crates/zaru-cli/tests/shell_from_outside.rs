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
use zaru_cli::terminal::driver::{Restore, Surface, Turnable, run};
use zaru_cli::terminal::source::{Pace, Source};
use zaru_cli::terminal::vocabulary::{Transcript as Pane, Vocabulary};
use zaru_cli::terminal::{NOTHING_CACHED, NotesTrie};
use zaru_notes::trie::{CachedEntry, EntryKind};
use zaru_tui::shell::{COMPOSER_ROWS, Input, Key, Queued, Shell, Status};

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
        // **A call is a pair, and this fixture wrote only the second half
        // until 2026-09-06.** `tools::execute` appends a `Phase::Started`
        // before a call and a `Phase::Completed` after it, and a `Completed`
        // with no `Started` before it is a shape the product cannot produce:
        // ADR-0010 D4's interruption is derived from exactly that pairing.
        // Writing the pair is what makes this staging's own claim above --
        // that the session is built through the crate's public door, so what
        // the shell reads back is what the product writes -- true.
        for line in [
            format!("fs.read `notes/{NONCE}.md`"),
            format!("cmd.run `just test` · {NONCE}"),
        ] {
            for phase in [Phase::Started, Phase::Completed] {
                transcript
                    .record(&Record::ToolCall(ToolCall {
                        line: line.clone(),
                        out_of_tree: false,
                        destructive: false,
                        phase,
                    }))
                    .expect("a record appended");
            }
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

/// A terminal a check owns: a buffer, and a count of restores.
///
/// **Keys are `Source::scripted`'s** since 2026-09-05, not this type's: the
/// reader and the painter are two things, which is what lets a turn and the
/// terminal be waited on at once.
struct Recorded {
    terminal: Terminal<TestBackend>,
    restores: usize,
    frames: Vec<Vec<String>>,
}

impl Recorded {
    fn of() -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(72, 14)).expect("test terminal"),
            restores: 0,
            frames: Vec::new(),
        }
    }

    /// The same, on a terminal wide enough not to clip a line.
    ///
    /// **The pane clips every line at its own width, silently** — an Open
    /// High row on `operations/known-defects`, found by the
    /// `harness-look-and-feel` survey and owned by the `pane-text` arc. A
    /// check about *what a line says* must not also be a check about how wide
    /// the terminal is, or it fails for a reason it is not about and passes
    /// again when somebody widens the constant above. 160 columns is wider
    /// than any line this file asserts on.
    fn wide() -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(160, 14)).expect("test terminal"),
            restores: 0,
            frames: Vec::new(),
        }
    }

    /// The rows of the last frame painted.
    ///
    /// Read out of the buffer rather than out of whatever was handed to the
    /// shell, so what is asserted is what a person would see.
    fn rows(&self) -> Vec<String> {
        self.frames.last().cloned().unwrap_or_default()
    }
}

/// A beat an outside caller implements, so the port is asserted reachable from
/// outside this crate rather than only from its own fixtures.
///
/// It never sleeps: a check that waited on a clock would pass or fail on how
/// the machine scheduled (library verification-lessons §57).
#[derive(Debug, Default)]
struct Instant;

impl Pace for Instant {
    fn wait(&self) {}

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        tokio::task::yield_now()
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
        4,
        "the session the check wrote did not come back, so nothing below is about a session"
    );

    let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
    shell.refresh(&Pane::of(&resumed.tail));

    let mut keys = typed("/runtime");
    keys.extend(typed("/exit"));
    let mut surface = Recorded::of();
    let source = Source::scripted(keys);
    let runner = zaru_cli::cli::Run {
        version: env!("CARGO_PKG_VERSION"),
        report_at: env!("CARGO_PKG_REPOSITORY"),
    };
    let trie = NotesTrie::nothing_cached("zaru");
    shell.composer_mut().set_absence(trie.absence());
    // The product's own runtime constructor, which is what the binary uses to
    // hold a session -- not a second executor written beside it.
    let pumped = zaru_cli::compose::turn::runtime()
        .expect("a runtime")
        .block_on(run(
            &mut shell,
            &mut surface,
            &source,
            &Instant,
            &runner,
            &trie,
            &Vocabulary,
            &mut Turnable::Cannot(Vec::new()),
        ))
        .expect("the pump");
    assert_eq!(
        source.contended(),
        0,
        "the source was contended, which a single-threaded pump cannot do"
    );

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
    let exit = match pumped.outcome {
        zaru_cli::terminal::Pumped::Left(exit) => exit,
        zaru_cli::terminal::Pumped::Switch(id) => {
            panic!("the pump asked to switch to {id} rather than leaving")
        }
    };
    println!("   exit {}", exit.code());

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

    assert_eq!(exit.code(), 0, "the shell did not exit 0 on `/exit`");
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
        stdout.contains("4 record(s) in the transcript"),
        "the resume did not read the four records the check wrote -- two calls, each a \
         `Phase::Started` and a `Phase::Completed`, which is the pair `tools::execute` writes"
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
        let mut surface = Recorded::of();
        let source = Source::scripted(keys);
        let runner = zaru_cli::cli::Run {
            version: env!("CARGO_PKG_VERSION"),
            report_at: env!("CARGO_PKG_REPOSITORY"),
        };
        zaru_cli::compose::turn::runtime()
            .expect("a runtime")
            .block_on(run(
                &mut shell,
                &mut surface,
                &source,
                &Instant,
                &runner,
                &trie,
                &Vocabulary,
                &mut Turnable::Cannot(Vec::new()),
            ))
            .expect("the pump");

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

// ------------------------------------- the security corpus: an interrupted turn

/// A beat that never releases anything and never sleeps.
///
/// The races below are ended by the terminal, not by time.
#[derive(Debug, Default)]
struct Beats(std::sync::atomic::AtomicUsize);

impl Pace for Beats {
    fn wait(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn elapse(&self) -> impl Future<Output = ()> + Send {
        let counter = &self.0;
        std::future::poll_fn(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::task::Poll::Ready(())
        })
    }
}

/// A turn that writes tool-call records and then never finishes.
///
/// It stands in for the real turn at exactly the point that matters: a pair
/// written around a completed call, a lone `Started` for one still in flight,
/// and then an await nothing will complete. What it is *not* is a model — this
/// file's own header says the terminal half is evidence about the mechanism.
fn a_turn_that_stops_between_two_calls(
    path: &Path,
    finish_the_second: bool,
) -> impl Future<Output = &'static str> + use<'_> {
    let path = path.to_path_buf();
    let mut wrote = false;
    std::future::poll_fn(move |context| {
        if !wrote {
            wrote = true;
            let mut transcript = Transcript::append_to(&path).expect("the transcript opens");
            let call = |line: &str, phase| {
                Record::ToolCall(ToolCall {
                    line: line.to_owned(),
                    out_of_tree: false,
                    destructive: false,
                    phase,
                })
            };
            transcript
                .record(&call("fs.read `notes/one.md`", Phase::Started))
                .expect("a record");
            transcript
                .record(&call("fs.read `notes/one.md`", Phase::Completed))
                .expect("a record");
            transcript
                .record(&call("cmd.run `just test`", Phase::Started))
                .expect("a record");
            if finish_the_second {
                transcript
                    .record(&call("cmd.run `just test`", Phase::Completed))
                    .expect("a record");
                return std::task::Poll::Ready("the turn finished");
            }
        }
        // Woken every poll, standing in for a provider await that a socket
        // wakes rather than a clock.
        context.waker().wake_by_ref();
        std::task::Poll::Pending
    })
}

/// Read a session's transcript back through the product's own resume.
fn resumed(directory: &Path) -> zaru_cli::session::Resumed {
    zaru_cli::session::resume(directory, usize::MAX).expect("the session resumes")
}

/// An interrupt between two tool calls leaves the transcript with at most the
/// event in flight, and the next resume says which one.
///
/// ADR-0010 D2: "Append-only means a crash loses at most the event in flight."
/// Its Update: "a `Started` with no matching `Completed` **is** the
/// interruption". D4: an interrupted call "is recorded as `Interrupted` and
/// the model is told it did not complete".
///
/// **A mid-turn `Ctrl-C` produces exactly that, with nothing authored for it.**
/// The race drops the turn's future; whatever the turn had already written is
/// on disk because every record is appended and synced as it occurs, and
/// nothing after it is.
///
/// The second reader is the product's own `session::resume`, which does not
/// share a code path with the pump (library verification-lessons §11), and the
/// interrupt is staged in the middle of the keys rather than last (§54).
#[test]
fn corpus_an_interrupt_between_two_tool_calls_leaves_at_most_the_event_in_flight() {
    let scratch = Scratch::new("interrupt-corpus");
    let directory = scratch
        .path()
        .join(".zaru")
        .join("sessions")
        .join(scratch.id.to_string());
    let transcript_path = directory.join("transcript.jsonl");
    let before = resumed(&directory).tail.len();

    let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
    let trie = NotesTrie::nothing_cached("zaru");
    let mut surface = Recorded::of();
    let source = Source::scripted(vec![
        press(Key::Char('h')),
        Input {
            key: Key::Char('c'),
            ctrl: true,
            alt: false,
            shift: false,
        },
        press(Key::Char('x')),
    ]);
    let mut now = std::time::Duration::ZERO;

    let raced = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        zaru_cli::compose::turn::runtime()
            .expect("a runtime")
            .block_on(zaru_cli::terminal::driver::race(
                &pane,
                &source,
                &Beats::default(),
                &trie,
                &mut now,
                None,
                None,
                a_turn_that_stops_between_two_calls(&transcript_path, false),
            ))
    };
    assert!(
        matches!(raced, zaru_cli::terminal::driver::Raced::Interrupted),
        "`Ctrl-C` between two tool calls did not stop the turn: {raced:?}"
    );

    let after = resumed(&directory);
    assert_eq!(
        after.tail.len(),
        before + 3,
        "the transcript gained {} record(s) rather than the three the turn wrote before it was \
         interrupted",
        after.tail.len() - before
    );
    let interrupted = after
        .interrupted
        .as_ref()
        .expect("a `Started` with no `Completed` is the interruption, and resume found none");
    assert!(
        format!("{interrupted:?}").contains("just test"),
        "the interruption names the wrong call: {interrupted:?}"
    );
    assert_eq!(
        after.fragment, None,
        "the transcript ends mid-line, so a record was torn rather than merely not written; \
         ADR-0010 D2 loses at most the event in flight and this lost part of one"
    );
}

/// Its accepting sibling: a turn that was not interrupted leaves a matched
/// pair for every call, and resume finds no interruption.
///
/// Without this, the check above would pass against a `resume` that reported
/// an interruption for every session it read.
#[test]
fn an_uninterrupted_turn_leaves_a_matched_pair_for_every_call() {
    let scratch = Scratch::new("interrupt-corpus-sibling");
    let directory = scratch
        .path()
        .join(".zaru")
        .join("sessions")
        .join(scratch.id.to_string());
    let transcript_path = directory.join("transcript.jsonl");
    let before = resumed(&directory).tail.len();

    let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
    let trie = NotesTrie::nothing_cached("zaru");
    let mut surface = Recorded::of();
    let source = Source::scripted(Vec::new());
    let mut now = std::time::Duration::ZERO;

    let raced = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        zaru_cli::compose::turn::runtime()
            .expect("a runtime")
            .block_on(zaru_cli::terminal::driver::race(
                &pane,
                &source,
                &Beats::default(),
                &trie,
                &mut now,
                None,
                None,
                a_turn_that_stops_between_two_calls(&transcript_path, true),
            ))
    };
    assert!(
        matches!(
            raced,
            zaru_cli::terminal::driver::Raced::Ran("the turn finished")
        ),
        "the uninterrupted turn did not finish: {raced:?}"
    );

    let after = resumed(&directory);
    assert_eq!(after.tail.len(), before + 4);
    assert!(
        after.interrupted.is_none(),
        "resume reported an interruption for a turn that finished: {:?}",
        after.interrupted
    );
}

/// ADR-0002 D8's standing tip yields on the first keystroke, mid-turn.
///
/// D8: a standing tip is "**Ephemeral: it yields the instant the user types.**"
/// ADR-0005 D1: "Typing dismisses a tip instantly — no fade, no delay. The
/// first keystroke switches the strip to search." Until a source could be read
/// beside the turn there was no keystroke to yield to while one ran, which is
/// what ADR-0005's gap paragraph named.
///
/// **The tip is handed in**, because nothing in this build produces one: D8's
/// one-per-session budget and its three-displays-without-action counter are
/// unbuilt, so this asserts the composer's half of clause 10 and claims
/// nothing about the other. The strip is read out of a painted frame before
/// and after, so what is compared is what a person would have seen.
#[test]
fn a_standing_tip_yields_on_the_first_keystroke_during_a_turn() {
    const TIP: &str = "declaring validators would let the loop catch this";

    let mut shell = Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"));
    let trie = NotesTrie::nothing_cached("zaru");
    shell.composer_mut().set_absence(trie.absence());
    shell.composer_mut().set_standing(0, Some(TIP.to_owned()));

    let mut surface = Recorded::of();
    let source = Source::scripted(vec![press(Key::Char('s'))]);
    let mut now = std::time::Duration::ZERO;

    // The frame before the keystroke, painted through the same `Surface` the
    // race paints through. `race` paints on a beat and on a keystroke; it does
    // not paint on entry, because the pump has already drawn the shell before
    // it reaches a task.
    surface.draw(&shell).expect("the opening frame");

    let raced = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        zaru_cli::compose::turn::runtime()
            .expect("a runtime")
            .block_on(zaru_cli::terminal::driver::race(
                &pane,
                &source,
                &Beats::default(),
                &trie,
                &mut now,
                None,
                None,
                std::future::pending::<&'static str>(),
            ))
    };
    assert!(
        matches!(raced, zaru_cli::terminal::driver::Raced::SourceEnded),
        "the race ended some other way: {raced:?}"
    );

    // Staging: the tip was on the strip before the keystroke. Without this the
    // assertion below would pass against a strip that never showed it.
    let painted_before = surface
        .frames
        .first()
        .expect("no frame was painted before the keystroke")
        .join("\n");
    assert!(
        painted_before.contains(TIP),
        "the tip was never on the strip, so nothing could yield: {painted_before}"
    );

    let painted_after = surface
        .frames
        .last()
        .expect("no frame was painted after the keystroke")
        .join("\n");
    assert!(
        !painted_after.contains(TIP),
        "the tip is still on the strip after a keystroke read mid-turn; ADR-0002 D8 says it \
         yields the instant the user types:\n{painted_after}"
    );
}

/// [ADR-0010] D4: a resume "restores `context.json`", and what comes back is
/// [ADR-0013] D1's layer 6 as the last process left it.
///
/// # The mutant this is written against
///
/// `terminal::open` opened a **fresh** `SessionContext` and threw the
/// checkpoint away, so a resumed session answered its first typed line from a
/// context that had never heard of the session it was sitting in. Restoring
/// nothing and restoring correctly both produce a shell that opens, which is
/// why this reads the exchanges rather than the pane.
///
/// # Three sessions, and the third is what makes the first two mean anything
///
/// A session whose checkpoint holds two exchanges comes back holding both, in
/// order. A session whose checkpoint holds a **summary** and whose transcript
/// holds the `Compacted` record carrying the span it replaced comes back
/// holding the summary and **not** the span — D2 says "only the model's view
/// is compacted", so rebuilding layer 6 from the transcript would restore the
/// raw span, and that mutant is the one the second arm catches. And a session
/// that never checkpointed comes back empty, which is the accepting sibling:
/// without it, a restorer that invented exchanges would pass the first two.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0013]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0013-context-management
#[test]
fn a_resumed_session_restores_layer_six_from_the_checkpoint_and_not_the_transcript() {
    use zaru_cli::compose::SessionContext;

    let scratch = Scratch::new("restore-layer-six");
    let directory = scratch
        .path()
        .join(".zaru")
        .join("sessions")
        .join(scratch.id.to_string());

    let limits = zaru_cli::cli::layers::context_limits();
    let held = zaru_cli::redaction::HeldSecrets::none();

    // A session that said two things, checkpointed through the product's own
    // writer rather than by writing JSON here.
    let mut said = SessionContext::opened(zaru_cli::compose::prefix_for(), limits);
    said.record(zaru_core::context::Exchange::of_turn(
        "user: remember the word saffron",
        &[],
        "zaru: ok",
    ));
    said.record(zaru_core::context::Exchange::of_turn(
        "user: and the number nine",
        &[format!("fs.read `notes/{NONCE}.md`")],
        "zaru: noted",
    ));
    zaru_cli::session::Checkpoint::at(directory.join("context.json"))
        .write(&said.checkpoint())
        .expect("the checkpoint is written");

    let reopened = resumed(&directory);
    assert!(
        reopened.checkpoint.is_some(),
        "the session checkpointed, so a resume carries one and the staging holds"
    );
    // **Through the product's own door**, which is what `terminal::open` calls
    // before it takes the terminal — not `SessionContext::restored` directly.
    // A check that called the constructor would prove the mechanism and say
    // nothing about whether anything reaches it
    // (library verification-lessons §25).
    let restored = zaru_cli::terminal::open::restored_context(&reopened, &classifier(), evidence())
        .expect("a checkpoint this harness wrote reads back");
    let held_texts: Vec<&str> = restored
        .exchanges()
        .iter()
        .map(zaru_core::context::Exchange::as_str)
        .collect();
    assert_eq!(
        held_texts.len(),
        2,
        "a resumed session restores what it said; it restored {:?}",
        held_texts
    );
    assert!(
        held_texts[0].contains("saffron") && held_texts[1].contains("nine"),
        "layer 6 came back out of order or incomplete: {held_texts:?}"
    );

    // A compacted session: the checkpoint holds the summary, the transcript
    // holds the span it replaced. Restoring the transcript's records instead
    // would bring the span back.
    let span = format!("user: the raw span {NONCE} nobody should restore");
    let mut compacted = SessionContext::opened(zaru_cli::compose::prefix_for(), limits);
    compacted.record(zaru_core::context::Exchange::summary(
        "a summary standing for earlier turns",
    ));
    zaru_cli::session::Checkpoint::at(directory.join("context.json"))
        .write(&compacted.checkpoint())
        .expect("the checkpoint is written");
    let mut transcript =
        Transcript::append_to(directory.join("transcript.jsonl")).expect("a transcript");
    transcript
        .record(&Record::Compacted(zaru_core::context::Compaction {
            announcements: Vec::new(),
            raw: Some(zaru_core::context::Span::new(vec![
                zaru_core::context::Exchange::verbatim(span.clone()),
            ])),
        }))
        .expect("the compaction is recorded");

    let reopened = resumed(&directory);
    let restored = zaru_cli::terminal::open::restored_context(&reopened, &classifier(), evidence())
        .expect("the compacted checkpoint reads back");
    let rendered: String = restored
        .exchanges()
        .iter()
        .map(zaru_core::context::Exchange::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("a summary standing for earlier turns"),
        "D2 replaces the span with the summary, and the summary is not what came back: {rendered}"
    );
    assert!(
        !rendered.contains(&span),
        "the raw span was restored into layer 6. D2 says only the model's view is compacted, so \
         the span belongs in the transcript and nowhere else: {rendered}"
    );
    // The staging is asserted, so a transcript that never held the span could
    // not satisfy the arm above by there being nothing to find.
    let on_disk =
        std::fs::read_to_string(directory.join("transcript.jsonl")).expect("the transcript reads");
    assert!(
        on_disk.contains(&span),
        "the span is not in the transcript either, so this check asserted nothing"
    );
    let _ = held;

    // The accepting sibling: a session that never checkpointed.
    let store = SessionStore::open(scratch.path().join(".zaru")).expect("the store");
    let untried = store
        .start(SessionId::mint(&zaru_cli::session::SystemWallClock).expect("a ULID"))
        .expect("a session directory");
    let reopened = resumed(untried.directory());
    assert!(
        reopened.checkpoint.is_none(),
        "a session with no turns has written no checkpoint, which `Checkpoint::read` calls not \
         an error",
    );
    assert_eq!(
        reopened.turns, 0,
        "and it has had no turns, so its next turn is turn one",
    );
    let restored = zaru_cli::terminal::open::restored_context(&reopened, &classifier(), evidence())
        .expect("an absent checkpoint is not a failure");
    assert!(
        restored.exchanges().is_empty(),
        "a session that never checkpointed opens with an empty layer 6, and this one did not",
    );
    let _ = limits;
}

/// The classifier the binary builds, so a check reads the same class it does.
fn classifier() -> zaru_cli::cli::classify::Surface<'static> {
    zaru_cli::cli::classify::Surface::new(env!("CARGO_PKG_VERSION"), env!("CARGO_PKG_REPOSITORY"))
}

/// The evidence a resumed session carries into a classification.
fn evidence() -> zaru_cli::failure::SessionEvidence {
    zaru_cli::failure::SessionEvidence::NoSessionExists
}

/// A checkpoint this harness did not write is refused, and its contents reach
/// nothing a person or a log can read.
///
/// # Two properties, and the second is why this is a corpus case
///
/// [ADR-0010] D3's accepted Update: "A document this type did not write is
/// **refused** rather than read as an empty conversation, which would drop a
/// session's history and look exactly like a session that had none." That is
/// the first arm.
///
/// The second is that the refusal carries **nothing of the document**. A
/// `serde_json::Error`'s own message quotes the value it tripped on, and this
/// file holds a session's whole conversation — so a hand-edited or corrupted
/// `context.json` is exactly the shape that puts a session's text into a
/// defect report. [ADR-0016] D3's Update already decided the same question for
/// a panic's message, and `Classify::checkpoint_contents` carries no field of
/// the error at all.
///
/// Asserted by **value and by ASCII core**, because a `Debug` rendering
/// escapes a combining mark and an absence assertion written against the value
/// as typed is blind to a rendering that published every byte of it
/// (library verification-lessons §50 and §63).
///
/// The accepting sibling is the same document made well-formed, which
/// restores — so this cannot pass against a reader that refuses everything.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn corpus_a_checkpoint_this_harness_did_not_write_is_refused_without_quoting_its_contents() {
    use zaru_cli::compose::SessionContext;

    let scratch = Scratch::new("foreign-checkpoint");
    let directory = scratch
        .path()
        .join(".zaru")
        .join("sessions")
        .join(scratch.id.to_string());
    let limits = zaru_cli::cli::layers::context_limits();

    // A secret in the position a session's own text occupies. The combining
    // mark is what makes the ASCII-core arm necessary rather than decorative.
    let secret = format!("nn_mcp_{NONCE}e\u{301}\u{1f701}");
    let core = secret
        .split(|c: char| !c.is_ascii())
        .next()
        .expect("an ASCII core")
        .to_owned();
    assert!(
        core.len() > 8,
        "the ASCII core has to be long enough that finding it is finding the value: {core}"
    );

    // Valid JSON, wrong shape: `exchanges` is a string where the type writes
    // an array. `serde_json` accepts the document and `SessionContext` does
    // not, which is the case a checkpoint that will not parse at all cannot
    // reach.
    let foreign = serde_json::json!({ "exchanges": secret });
    zaru_cli::session::Checkpoint::at(directory.join("context.json"))
        .write(&foreign)
        .expect("the document is written");

    let reopened = resumed(&directory);
    let error = SessionContext::restored(
        zaru_cli::compose::prefix_for(),
        limits,
        reopened
            .checkpoint
            .as_ref()
            .expect("a checkpoint is on disk"),
    )
    .expect_err("a document this type did not write is refused, not read as an empty session");
    // And the door refuses it too, rather than only the constructor.
    let refused = zaru_cli::terminal::open::restored_context(&reopened, &classifier(), evidence())
        .expect_err("the shell refuses to open over a checkpoint it cannot read");

    let classified = match *refused {
        zaru_cli::failure::Exit::Failed(classified) => classified,
        zaru_cli::failure::Exit::Succeeded => {
            panic!("the shell opened over a checkpoint it could not read")
        }
    };
    assert!(
        matches!(classified, zaru_cli::failure::Classified::Defect(_)),
        "ADR-0016 D1 makes a file only this harness writes and cannot read back a defect, and \
         this was classified as something else",
    );
    assert_eq!(
        zaru_cli::failure::Exit::Failed(classified.clone()).code(),
        70,
        "D5's defect code",
    );

    // **The instrument is shown to work on the thing it is guarding against,
    // and that thing is the reason this classifier exists.** A
    // `serde_json::Error` quotes the value it tripped on, in its `Display` and
    // in its `Debug` alike — measured here rather than asserted, so the
    // absences below are a finding rather than a search of an empty string
    // (library verification-lessons §26). This is precisely what
    // `Classify::checkpoint_contents` must not pass on: the value it quotes is
    // a line of the user's conversation.
    let quoted = format!("{error}\n{error:?}");
    assert!(
        quoted.contains(&core),
        "the error does not quote the document, so this check is guarding against nothing \
         and the classifier below could carry anything: {quoted}"
    );

    // Everything the harness renders about this failure: what the surface
    // prints, and the `Debug` that would reach a panic message or a log.
    let readable = format!(
        "{}\n{:?}",
        zaru_cli::failure::Presentation::of(&classified),
        classified,
    );
    for (what, needle) in [("the value", secret.as_str()), ("its ASCII core", &core)] {
        assert!(
            !readable.contains(needle),
            "{what} from a corrupt checkpoint reached what a person reads. The file holds a \
             session's whole conversation, so nothing of it may travel with the refusal:\n{readable}"
        );
    }

    // The accepting sibling: the same document, well-formed.
    let mut said = SessionContext::opened(zaru_cli::compose::prefix_for(), limits);
    said.record(zaru_core::context::Exchange::verbatim(secret.clone()));
    zaru_cli::session::Checkpoint::at(directory.join("context.json"))
        .write(&said.checkpoint())
        .expect("the document is written");
    let reopened = resumed(&directory);
    let restored = zaru_cli::terminal::open::restored_context(&reopened, &classifier(), evidence())
        .expect("a checkpoint this harness wrote reads back");
    assert_eq!(
        restored.exchanges().len(),
        1,
        "the sibling must restore, or the refusal above is a reader that refuses everything",
    );
}

/// An interrupted turn is the one ending the pump carries on from, and it
/// leaves the next turn owing the model the call that did not complete.
///
/// # The 2026-09-06 ruling, held where a check can reach it
///
/// [ADR-0015]'s Status tracking said, until this arc: "**Interrupt-and-stay
/// was considered and not built, because giving one key two meanings depending
/// on whether a turn is running is a decision no record makes.**" It is made
/// now, as an accepted Update on that record under directive 25.
///
/// **Reaching this through `driver::run` would need a `Prepared`, which needs
/// a provider client and a key**, so the one decision that says whether a
/// session survives its own interruption would be checkable only on a machine
/// holding a credential. `driver::after` is that decision, taking what it
/// needs and nothing else, for the same reason `request_for` is separate from
/// `dispatch`.
///
/// The three arms discriminate: a build that stopped on every ending, or
/// carried on from every one, fails a different arm. The interrupted arm also
/// asserts [ADR-0010] D4's carrier is re-derived, with the uninterrupted
/// session as its accepting sibling — without which the check would pass
/// against an `after` that reported something owed for every turn.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[test]
fn corpus_an_interrupted_turn_is_the_one_ending_the_pump_carries_on_from() {
    use zaru_cli::terminal::driver::{Pending, Turned};
    use zaru_cli::terminal::{AfterTurn, after};
    use zaru_tui::shell::port::{Line, Register};

    let redactor = zaru_cli::redaction::HeldSecrets::none();

    // Two real sessions: one whose turn was interrupted between two calls, and
    // one whose every call closed.
    let interrupted_scratch = Scratch::new("after-interrupted");
    let whole_scratch = Scratch::new("after-whole");
    let mut sessions = Vec::new();
    for (scratch, interrupt) in [(&interrupted_scratch, true), (&whole_scratch, false)] {
        let directory = scratch
            .path()
            .join(".zaru")
            .join("sessions")
            .join(scratch.id.to_string());
        let transcript_path = directory.join("transcript.jsonl");
        let mut shell = Shell::open(Status::new("bare", scratch.id.to_string()));
        let trie = NotesTrie::nothing_cached("zaru");
        let mut surface = Recorded::of();
        let source = Source::scripted(if interrupt {
            vec![
                press(Key::Char('h')),
                Input {
                    key: Key::Char('c'),
                    ctrl: true,
                    alt: false,
                    shift: false,
                },
            ]
        } else {
            vec![press(Key::Char('h'))]
        });
        let mut now = std::time::Duration::ZERO;
        {
            let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
                &mut shell,
                &mut surface,
            ));
            let _ = zaru_cli::compose::turn::runtime()
                .expect("a runtime")
                .block_on(zaru_cli::terminal::driver::race(
                    &pane,
                    &source,
                    &Beats::default(),
                    &trie,
                    &mut now,
                    None,
                    None,
                    a_turn_that_stops_between_two_calls(&transcript_path, !interrupt),
                ));
        }
        let store = zaru_cli::session::SessionStore::reading(scratch.path().join(".zaru"));
        sessions.push(
            store
                .existing(&scratch.id)
                .expect("the staged session directory is there"),
        );
    }
    let (interrupted_session, whole_session) = (&sessions[0], &sessions[1]);

    // **Over the *interrupted* session on purpose.** A turn that ran to
    // completion owes the model nothing whatever is on disk beside it, so an
    // `after` that re-derived on every ending would report something owed
    // here — and against a session with no interruption it would not, which
    // is how that mutation survives a check staged the obvious way round.
    let mut owed = Pending::none();
    // A task is queued across all four arms, so what each does with it is
    // asserted rather than assumed: only the interrupted one discards.
    let mut queued = Some(Queued::of("the next thing"));
    let ran = after(
        Turned::Ran(vec![Line::new(Register::Plain, "an answer")]),
        &mut owed,
        interrupted_session,
        &redactor,
        &mut queued,
    );
    let AfterTurn::Carries(lines) = ran else {
        panic!("a turn that ran ended the session: {ran:?}");
    };
    assert_eq!(
        lines.len(),
        1,
        "a turn that ran must hand its own lines to the pane"
    );
    assert!(
        !owed.is_owed(),
        "a turn that ran to completion left the next one owing the model something, so the \
         re-derivation fires on every ending rather than on an interruption"
    );
    assert_eq!(
        queued.as_ref().map(|task| task.task.as_str()),
        Some("the next thing"),
        "a turn that ran discarded the queued task, and only an interruption may: the queue \
         holds {queued:?}"
    );

    // The arm this arc changed. Until 2026-09-06 it produced a `Pump` and the
    // process left the alternate screen.
    //
    // **The witness is why this staging goes through a real narrator.**
    // `Turned::Interrupted` carries a `compose::Narrated`, which has no
    // constructor outside `compose::iterate`, so the only way to reach this
    // arm at all is to have told a pane — which is the property a mutation
    // deleting the call from `run_a_turn` used to leave green.
    let mut shell = Shell::open(Status::new("bare", "01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    let mut surface = Recorded::wide();
    let narrated = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        let narrator = zaru_cli::terminal::driver::PaneNarrator::over(&pane);
        zaru_cli::compose::Narrator::interrupted(&narrator)
    };
    let mut owed = Pending::none();
    let mut queued = Some(Queued::of("the next thing"));
    let interrupted = after(
        Turned::Interrupted(narrated),
        &mut owed,
        interrupted_session,
        &redactor,
        &mut queued,
    );
    let AfterTurn::Carries(lines) = interrupted else {
        panic!(
            "`Ctrl-C` during a turn ended the whole session, and the ruling of 2026-09-06 is that \
             it stops the turn and the session stays: {interrupted:?}"
        );
    };
    assert!(
        lines.is_empty(),
        "an interrupt adds no line here: the narrator has already painted the one there is, and a \
         second would be two statements of one event"
    );
    assert!(
        owed.is_owed(),
        "the interrupt left a `Started` with no `Completed` on disk and the next turn owes the \
         model nothing about it, so ADR-0010 D4's carrier was not re-derived in this process"
    );
    assert_eq!(
        queued, None,
        "`Ctrl-C` mid-turn left a task queued, and the turn it was the next one of has been \
         stopped: the queue holds {queued:?}"
    );

    // The accepting sibling, through the same arm: a session whose every call
    // closed owes nothing, so the assertion above cannot pass against an
    // `after` that reports an interruption for every turn.
    let mut owed = Pending::none();
    let mut shell = Shell::open(Status::new("bare", "01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    let mut surface = Recorded::wide();
    let narrated = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        let narrator = zaru_cli::terminal::driver::PaneNarrator::over(&pane);
        zaru_cli::compose::Narrator::interrupted(&narrator)
    };
    let _ = after(
        Turned::Interrupted(narrated),
        &mut owed,
        whole_session,
        &redactor,
        &mut None,
    );
    assert!(
        !owed.is_owed(),
        "a session whose every call closed owes the model nothing, and this reported an \
         interruption for a session that had none"
    );

    // The arm that discriminates in the other direction. A mapping that
    // carried on from everything would leave a check's pump hanging on a
    // source that has stopped answering.
    let mut owed = Pending::none();
    let mut queued = Some(Queued::of("the next thing"));
    let ended = after(
        Turned::SourceEnded,
        &mut owed,
        whole_session,
        &redactor,
        &mut queued,
    );
    assert!(
        matches!(ended, AfterTurn::Stops(_)),
        "a terminal that stopped answering must end the pump: {ended:?}"
    );
    assert!(
        queued.is_some(),
        "a source that ended discarded the queued task, and only an interruption may"
    );
}

/// The one line an interrupt-and-stay paints, and where its words come from.
///
/// [ADR-0028] D3 makes the pane a consumer of the loop's own events, so the
/// consumer that narrates the loop is the consumer that says the narration
/// stopped. The wording is `compose::prose::INTERRUPTED` — one authored
/// constant, quoted verbatim on ADR-0015's Updates — and this check reads it
/// out of the rendered frame rather than out of the constant it came from, so
/// a line composed somewhere else would not satisfy it.
///
/// `Register::Announced` and not `Register::Failed`: an interruption is the
/// user's decision rather than one of [ADR-0016] D1's five classes, which is
/// the rule `BUSY` already follows and which [ADR-0016] states for an
/// interruption in as many words.
///
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative
#[test]
fn corpus_an_interrupt_says_so_on_the_pane_in_the_register_a_decision_takes() {
    use zaru_cli::compose::Narrator;

    let mut shell = Shell::open(Status::new("bare", "01ARZ3NDEKTSV4RRFFQ69G5FAV"));
    let mut surface = Recorded::wide();

    let painted = {
        let pane = std::sync::Mutex::new(zaru_cli::terminal::driver::Pane::of(
            &mut shell,
            &mut surface,
        ));
        let narrator = zaru_cli::terminal::driver::PaneNarrator::over(&pane);
        let _: zaru_cli::compose::Narrated = Narrator::interrupted(&narrator);
        narrator.contended()
    };
    assert_eq!(
        painted, 0,
        "the pane refused the line, so an interrupt would be silent on a contended lock"
    );

    let rows = surface.rows();
    let joined = rows.join("\n");
    assert!(
        joined.contains("turn interrupted"),
        "an interrupt paints nothing, so a person cannot tell it from a hang:\n{joined}"
    );
    assert!(
        joined.contains("the session stays open"),
        "the line must say the session survived, which is the half that distinguishes this from \
         leaving:\n{joined}"
    );
    assert!(
        joined.contains("in the transcript"),
        "and the half that is ADR-0010 D2's own promise about what was already written:\n{joined}"
    );
    // The register, read off the glyph the frame carries rather than off the
    // enum: `◈` is ADR-0002 D4's announcement marker, and `✗` is ADR-0016 D2's.
    let line = rows
        .iter()
        .find(|row| row.contains("turn interrupted"))
        .expect("the line is on the frame");
    assert!(
        line.contains('◈') && !line.contains('✗'),
        "an interruption is a decision rather than a failure, and this is in the error register: \
         {line}"
    );
}

/// A bare `zaru` at a terminal opens a **new** session's shell.
///
/// # The terminal half of [ADR-0015] D2's 2026-09-06 Update
///
/// `terminal::open::mint` is what a bare `zaru` reaches when somebody is
/// watching. It is driven directly here rather than through `take_over`,
/// because that function's other half is `std::io::IsTerminal` over this
/// process's own standard output and no check owns that.
///
/// What is asserted is that a session appeared that was not there before, that
/// it holds [ADR-0010] D1's three files, and that its `meta.toml` records this
/// process's own directory — the field D4's `--continue` selects on, so a mint
/// that recorded nothing would produce sessions `--continue` could never find.
///
/// **The accepting sibling is `Opening::Existing`**, which resolves a staged
/// session and mints nothing: without it, a resolver that minted for every
/// opening would pass the first three assertions.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
#[test]
fn corpus_a_bare_zaru_at_a_terminal_opens_a_new_sessions_shell() {
    use zaru_cli::terminal::Opening;

    let scratch = Scratch::new("bare-terminal");
    let sessions = scratch.path().join(".zaru").join("sessions");
    let before = std::fs::read_dir(&sessions)
        .map(|entries| entries.count())
        .unwrap_or(0);

    // **The whole dispatch, under a scratch root.** `resolve` reads `$HOME` to
    // find the store, so a check driving it would mint into the developer's
    // own `~/.zaru` -- which it did, once, before `resolve_in` took the root
    // as a parameter. `resolve_in` is the implementation and `resolve` is it
    // with the default root, so what is driven here is the product's own
    // three-arm dispatch rather than a function beside it.
    //
    // This machine holds no provider key under a scratch home, so the mint
    // takes the `None` branch -- which is the case that matters: a person on a
    // fresh machine has no key, and refusing to *start* a session for them is
    // the survey's row 1, that they could not reach the interactive surface at
    // all.
    let overrides = zaru_cli::cli::invocation::Overrides::default();
    let minted = zaru_cli::terminal::resolve_in(
        &Opening::New,
        scratch.path().join(".zaru"),
        "0.0.0",
        "https://x",
        &overrides,
    )
    .expect("a bare `zaru` at a terminal mints a session");
    assert_ne!(
        minted, scratch.id,
        "the mint answered with the session this check staged rather than a new one"
    );

    let directory = sessions.join(minted.as_str());
    for file in ["meta.toml", "transcript.jsonl", "context.json"] {
        assert!(
            directory.join(file).exists(),
            "ADR-0010 D1's `{file}` is missing from a session this harness minted"
        );
    }
    let after = std::fs::read_dir(&sessions)
        .expect("the store exists")
        .count();
    assert_eq!(
        after,
        before + 1,
        "a bare `zaru` at a terminal must mint exactly one session"
    );

    let meta = zaru_cli::session::MetaFile::at(directory.join("meta.toml"))
        .read_if_present()
        .expect("the meta this harness just wrote parses")
        .expect("and it is there");
    let here = zaru_cli::tools::WorkingDirectory::of_this_process().expect("a working directory");
    assert_eq!(
        meta.directory,
        here.root(),
        "a minted session records nowhere the directory it began in, so `--continue` could never \
         find it"
    );

    assert_eq!(
        meta.provider, None,
        "a session minted with no provider recorded a kind it never reached"
    );

    // The accepting sibling: naming a session resolves to it and mints
    // nothing, so the assertions above cannot pass against a resolver that
    // minted for every opening. This half reaches no `$HOME`, because
    // `Opening::Existing` is answered without touching a store at all.
    let named = zaru_cli::terminal::resolve_in(
        &Opening::Existing(scratch.id.clone()),
        scratch.path().join(".zaru"),
        "0.0.0",
        "https://x",
        &overrides,
    )
    .expect("a named session resolves to itself");
    assert_eq!(named, scratch.id, "`--resume <id>` resolved to another id");
    assert_eq!(
        std::fs::read_dir(&sessions).expect("the store").count(),
        after,
        "resolving a named session minted one"
    );
}

/// `/session resume <id>` and `/session continue` reach the same operation the
/// flags do.
///
/// [ADR-0010] D4: "**Inside a session the same operation is `/session resume
/// <id>` and `/session continue`** — one operation with two entry points, per
/// ADR-0015 D2's namespace table, which governs both spellings. Settled
/// 2026-09-05 under directive 20." Both verbs refused with a full sentence
/// until 2026-09-06, which is the survey's row 17.
///
/// `switch_for` is the mapping and it is pure, for the reason `request_for` is
/// pure: what a spelling *means* and what running it *does* are two things,
/// and doing it here would mint or open a session directory.
///
/// The accepting sibling is `/session list`, which is a command rather than a
/// switch: without it, a mapping that switched on every `/session` verb would
/// pass.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn corpus_session_resume_in_session_names_what_the_flag_names() {
    use zaru_cli::terminal::{Opening, switch_for};
    use zaru_tui::shell::Command;

    let id = zaru_cli::session::SessionId::parse("01JQZX8N3K4M5P6R7S8T9V0W1X").expect("a ULID");

    assert_eq!(
        switch_for(&Command {
            slash: "/session",
            verb: Some("resume"),
            words: vec![id.to_string()],
        }),
        Some(Opening::Existing(id.clone())),
        "`/session resume <id>` must name the session `--resume <id>` names"
    );
    assert_eq!(
        switch_for(&Command {
            slash: "/session",
            verb: Some("continue"),
            words: Vec::new(),
        }),
        Some(Opening::MostRecentHere),
        "`/session continue` must mean what `--continue` means"
    );

    // The accepting siblings. A command is not a switch, and a `resume` whose
    // word is not a session id falls through to the refusal that surface
    // already has rather than being answered with a guess.
    for command in [
        Command {
            slash: "/session",
            verb: Some("list"),
            words: Vec::new(),
        },
        Command {
            slash: "/session",
            verb: Some("rm"),
            words: vec![id.to_string()],
        },
        Command {
            slash: "/runtime",
            verb: None,
            words: Vec::new(),
        },
        Command {
            slash: "/session",
            verb: Some("resume"),
            words: vec!["not-a-ulid".to_owned()],
        },
        Command {
            slash: "/session",
            verb: Some("resume"),
            words: Vec::new(),
        },
    ] {
        assert_eq!(
            switch_for(&command),
            None,
            "`{} {:?}` was read as a switch",
            command.slash,
            command.verb
        );
    }
}
