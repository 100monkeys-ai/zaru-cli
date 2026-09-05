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
        2,
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
                a_turn_that_stops_between_two_calls(&transcript_path, false),
            ))
    };
    assert!(
        matches!(
            raced,
            zaru_cli::terminal::driver::Raced::Interrupted(zaru_tui::shell::Leaving::Interrupt)
        ),
        "`Ctrl-C` between two tool calls did not leave: {raced:?}"
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
