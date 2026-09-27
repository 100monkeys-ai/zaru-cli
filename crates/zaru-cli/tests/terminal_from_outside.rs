// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The built `zaru`, in a real pseudo-terminal, read by the bytes it writes.
//!
//! # Evidence about the binary, and how a check gets a terminal
//!
//! `shell_from_outside.rs` says that no check can allocate a pseudo-terminal
//! without a dependency [ADR-0003] D2's table does not name, and drives the
//! shell over ratatui's `TestBackend`. That is still true of a *Rust*
//! dependency. It is not true of the machine: `script(1)` from util-linux
//! allocates one, puts a command's standard streams on it, relays its own
//! standard input to the terminal and copies everything the terminal is sent
//! to its own standard output. So what this file reads is exactly the byte
//! stream a terminal emulator would have been handed, from the release or
//! debug `zaru` Cargo built for this test, and nothing in the product is
//! staged. `script` is on every Linux this harness supports and on CI's
//! runner; a machine without it fails the check by name rather than passing
//! it.
//!
//! # What a terminal is owed back
//!
//! Taking the terminal is raw mode, the alternate screen, bracketed paste and
//! the mouse modes (`terminal::driver::arm`). Giving it back is the reverse,
//! and a person whose `zaru` ended by any route but the ones the harness wrote
//! is left with a shell that does not echo, on a screen that is not theirs,
//! typing mouse reports into their prompt. The check here is stated over
//! **whatever the binary set** rather than over a list, so it keeps holding
//! when the set of modes changes: every private mode the harness turned on is
//! turned off again, and the line discipline reads `isig icanon echo`.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "zaru-terminal-{label}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory can be created");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `zaru` in a pseudo-terminal, with everything the terminal was sent so far.
struct InATerminal {
    child: Child,
    seen: Arc<Mutex<Vec<u8>>>,
    _home: Scratch,
    _work: Scratch,
}

/// What the wrapping shell prints once `zaru` has ended, so the check can read
/// the exit status and the line discipline off the same terminal.
const STATUS: &str = "ZARU-STATUS=";
/// What the wrapping shell prints before `zaru` starts: the process id `zaru`
/// will have, because the shell that prints it then `exec`s it.
const PID: &str = "ZARU-PID=";

impl InATerminal {
    /// Open a bare `zaru` — a session in a fresh `HOME` with nothing
    /// configured — at 100 × 30.
    fn open() -> Self {
        let home = Scratch::new("home");
        let work = Scratch::new("work");
        let zaru = env!("CARGO_BIN_EXE_zaru");
        // The inner `sh` prints its own pid and `exec`s `zaru`, so the pid is
        // `zaru`'s and `zaru` is the terminal's foreground process rather than
        // a background job (which a non-interactive shell would hand
        // `/dev/null` for standard input). The outer shell reads the terminal's
        // line discipline once `zaru` is gone.
        let inner = format!(
            "stty rows 30 cols 100; sh -c 'echo {PID}$$; exec \"$0\"' '{zaru}'; \
             echo {STATUS}$?; stty -a"
        );
        let mut child = Command::new("script")
            .args(["-q", "-f", "-e", "-c", &inner, "/dev/null"])
            .current_dir(&work.0)
            .env_clear()
            .env("HOME", &home.0)
            .env("PATH", "/usr/bin:/bin")
            .env("TERM", "xterm-256color")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("`script` from util-linux allocates the pseudo-terminal this check reads");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().expect("standard output was piped");
        let sink = Arc::clone(&seen);
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            while let Ok(read) = stdout.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .expect("the sink is not poisoned")
                    .extend_from_slice(&chunk[..read]);
            }
        });
        Self {
            child,
            seen,
            _home: home,
            _work: work,
        }
    }

    fn bytes(&self) -> Vec<u8> {
        self.seen.lock().expect("the sink is not poisoned").clone()
    }

    /// Wait until the terminal has been sent `needle`, and say where.
    fn until(&self, needle: &[u8], what: &str) -> usize {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let bytes = self.bytes();
            if let Some(at) = find(&bytes, needle) {
                return at;
            }
            assert!(
                Instant::now() < deadline,
                "waited thirty seconds for {what} and the terminal was sent {:?}",
                String::from_utf8_lossy(&bytes)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The process id the wrapping shell reported for `zaru`.
    fn pid(&self) -> String {
        let at = self.until(PID.as_bytes(), "the wrapping shell to name zaru's pid") + PID.len();
        let bytes = self.bytes();
        bytes[at..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .map(|byte| char::from(*byte))
            .collect()
    }
}

impl Drop for InATerminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Every DEC private mode number in `bytes` set with `h` or reset with `l`,
/// in order.
fn private_modes(bytes: &[u8], set: bool) -> Vec<u32> {
    let last = if set { b'h' } else { b'l' };
    let mut modes = Vec::new();
    let mut at = 0;
    while let Some(start) = find(&bytes[at..], b"\x1b[?") {
        let digits_from = at + start + 3;
        let digits: Vec<u8> = bytes[digits_from..]
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        let end = digits_from + digits.len();
        if !digits.is_empty()
            && bytes.get(end) == Some(&last)
            && let Ok(number) = String::from_utf8_lossy(&digits).parse()
        {
            modes.push(number);
        }
        at = end.max(digits_from);
    }
    modes
}

/// The line discipline, as `stty -a` spells the three flags a person's shell
/// needs: each either bare (on) or with a leading `-` (off).
fn discipline(after_status: &str) -> Vec<String> {
    after_status
        .split(|character: char| character.is_whitespace() || character == ';')
        .filter(|word| matches!(word.trim_start_matches('-'), "isig" | "icanon" | "echo"))
        .map(str::to_owned)
        .collect()
}

/// **A `zaru` ended by a signal gives the terminal back before it goes.**
///
/// Until 2026-09-27 the terminal was restored only by `Guard`'s `Drop`, which
/// a signal never reaches: measured on the release binary at `2a7544b`, a
/// `SIGTERM` wrote no reset at all and left the person's shell reading
/// `-isig -icanon -echo`, on the alternate screen, with every mouse movement
/// typing a report into their prompt. `SIGKILL` cannot be handled by any
/// process and is not here.
///
/// Each of the three signals the harness now takes — `SIGTERM`, `SIGINT`
/// delivered as a signal (raw mode means `Ctrl-C` is a key, not a signal),
/// and `SIGHUP` — in a session of its own. The status is the one a shell
/// reports for a process a signal ended, `128 + n`, which is what `$?` read
/// before the handler existed.
///
/// **The mutant:** the signal listener never spawned. Every signal then takes
/// the default action and this check prints which modes were left set.
#[test]
fn a_session_ended_by_a_signal_gives_the_terminal_back() {
    for (signal, status) in [("TERM", 143), ("INT", 130), ("HUP", 129)] {
        let session = InATerminal::open();
        let pid = session.pid();
        // `?1049h` is the alternate screen; the modes `arm` writes follow it
        // in the same flush, so the first frame's cursor show is the sign the
        // terminal has been taken and armed.
        let armed = session.until(b"\x1b[?25h", "the session's first frame");
        let before = session.bytes()[..armed].to_vec();
        let taken = private_modes(&before, true);
        assert!(
            taken.contains(&1049),
            "the session never took the alternate screen, so there is nothing to give back: {:?}",
            String::from_utf8_lossy(&before)
        );

        let killed = Command::new("kill")
            .args([format!("-{signal}"), pid.clone()])
            .status()
            .expect("`kill` runs");
        assert!(killed.success(), "`kill -{signal} {pid}` failed");

        let status_at = session.until(STATUS.as_bytes(), "zaru to end and the shell to report");
        session.until(b"columns", "`stty -a` to report the line discipline");
        std::thread::sleep(Duration::from_millis(100));
        let bytes = session.bytes();
        let after_kill = &bytes[armed..status_at];
        let reported = String::from_utf8_lossy(&bytes[status_at..]).into_owned();

        let given_back = private_modes(after_kill, false);
        let left: Vec<u32> = taken
            .iter()
            .copied()
            // `?25h` shows the cursor, which is the restoring direction.
            .filter(|mode| *mode != 25 && !given_back.contains(mode))
            .collect();
        let flags = discipline(&reported);
        assert!(
            left.is_empty() && flags == ["isig", "icanon", "echo"],
            "SIG{signal} left private modes {left:?} set with no reset, and the person's shell \
             reads {flags:?}: the terminal was not given back"
        );
        assert!(
            reported.starts_with(&format!("{STATUS}{status}")),
            "SIG{signal} should end the process with {status}, the status a shell reports for \
             a process that signal ended; the shell reported {:?}",
            reported.lines().next().unwrap_or_default()
        );
    }
}

/// **The binary asks the terminal for the alternate screen, bracketed paste,
/// button reports and their SGR spelling, and nothing else.**
///
/// The unit check over `driver::arm` holds the function. This holds the
/// binary: a second place that asked for more (a stray `EnableMouseCapture`
/// anywhere on the path) would pass that check and fail this one.
///
/// Measured on the release binary at `2a7544b`: `?1002` and `?1003` were
/// requested too, and 300 pointer movements over three seconds cost 300
/// repaints and 8,100 bytes of output with nothing to show for them.
///
/// **The mutant:** `EnableMouseCapture` back in `arm`, which prints the five
/// modes it asks for.
#[test]
fn a_session_asks_the_terminal_for_the_wheel_and_nothing_more() {
    let session = InATerminal::open();
    let armed = session.until(b"\x1b[?25h", "the session's first frame");
    let taken = private_modes(&session.bytes()[..armed], true);
    assert_eq!(
        taken,
        [1049, 2004, 1000, 1006],
        "the session asked the terminal for private modes {taken:?}, where it is owed the \
         alternate screen, bracketed paste, button reports and their SGR spelling: a motion or \
         drag report is input nothing reads, and the terminal still sends it"
    );
}

/// **A pointer report that is not the wheel paints nothing.**
///
/// The session asks for `?1000` alone, so a terminal sends buttons and no
/// motion. But a terminal may send more than it was asked for: a multiplexer
/// in between, or a mode another program left on. So this writes the reports
/// a pointer makes (motion, press, release, drag) straight into the terminal
/// and measures what the binary paints in answer, after the first frame has
/// settled.
///
/// Measured on the release binary at `2a7544b`: 300 motion reports over three
/// seconds cost 300 repaints and 8,100 bytes.
///
/// **The mutant:** a non-wheel pointer event reaching the shell as an empty
/// keystroke again, which prints how many bytes the reports cost.
#[test]
fn pointer_reports_that_are_not_the_wheel_paint_nothing() {
    use std::io::Write;
    let mut session = InATerminal::open();
    session.until(b"\x1b[?25h", "the session's first frame");
    // Settle: the first frames, the strip's absence line and the status row.
    std::thread::sleep(Duration::from_millis(1500));
    let settled = session.bytes().len();
    let mut reports = Vec::new();
    for step in 0..100_u32 {
        let (column, row) = (10 + step % 50, 5 + step % 10);
        // SGR spellings: motion with no button (35), press and release of the
        // left button (0 … M, 0 … m), and a drag with it held (32).
        for cb in ["35", "0", "32"] {
            reports.extend_from_slice(format!("\x1b[<{cb};{column};{row}M").as_bytes());
        }
        reports.extend_from_slice(format!("\x1b[<0;{column};{row}m").as_bytes());
    }
    let stdin = session
        .child
        .stdin
        .as_mut()
        .expect("standard input was piped");
    stdin
        .write_all(&reports)
        .expect("the reports reach the terminal");
    stdin.flush().expect("the reports are flushed");
    std::thread::sleep(Duration::from_millis(1500));
    let painted = session.bytes().len() - settled;
    assert_eq!(
        painted, 0,
        "400 pointer reports that are not the wheel cost {painted} bytes of painting, where \
         they are input nothing reads"
    );
}

/// **A fresh session says how to select text**, in the hint strip, because
/// holding the mouse took the terminal's plain click-and-drag.
///
/// [ADR-0002] D8's standing tip: a capability the person has not discovered,
/// on an empty prompt, one line, at most three sessions. The modifier is
/// Shift in Windows Terminal, in the VS Code terminal off macOS, and in most
/// others.
///
/// **The mutant:** the selection tip's condition never holding, which prints
/// what the first frames carried instead.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
#[test]
fn a_fresh_session_says_how_to_select_text() {
    let session = InATerminal::open();
    session.until(b"\x1b[?25h", "the session's first frame");
    std::thread::sleep(Duration::from_millis(1000));
    let painted = glyphs(&session.bytes());
    assert!(
        painted.contains("holdShifttoselecttext"),
        "a fresh session with the mouse held never said how to select text; its first frames \
         painted {painted:?}"
    );
}

/// What a frame's bytes paint, with every control sequence and every blank
/// taken out.
///
/// ratatui writes only the cells that changed, and moves the cursor over a
/// blank rather than writing it, so a sentence reaches the terminal as its
/// words with a cursor move between each. Taking the sequences and the blanks
/// out leaves the words run together in paint order, which is what a check for
/// a sentence can compare against without reimplementing a terminal.
fn glyphs(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.peek() == Some(&'[') {
                characters.next();
                // Parameters and intermediates, then one final byte.
                for next in characters.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if !character.is_whitespace() {
            out.push(character);
        }
    }
    out
}
