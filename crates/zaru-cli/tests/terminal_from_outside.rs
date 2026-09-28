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
use std::process::Stdio;
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
///
/// `script`, the shell under it and `zaru` are all owned by `child`: dropping
/// this kills the three and reaps `script`, whichever way the check ended.
/// `script` puts its command in a session of its own, so killing `script`
/// alone never reached `zaru`; see `tests/support/owned.rs`.
struct InATerminal {
    child: owned::Owned,
    seen: Arc<Mutex<Vec<u8>>>,
    _home: Scratch,
    work: Scratch,
}

/// What the wrapping shell prints once `zaru` has ended, so the check can read
/// the exit status and the line discipline off the same terminal.
const STATUS: &str = "ZARU-STATUS=";
/// What the wrapping shell prints before `zaru` starts: the process id `zaru`
/// will have, because the shell that prints it then `exec`s it.
const PID: &str = "ZARU-PID=";
/// Where the wrapping shell writes the status too, in the session's working
/// directory.
const STATUS_FILE: &str = "zaru-status";

impl InATerminal {
    /// Open a bare `zaru` — a session in a fresh `HOME` with nothing
    /// configured — at 100 × 30.
    fn open() -> Self {
        Self::open_with(&[])
    }

    /// As [`Self::open`], with `environment` added to what `zaru` is given.
    fn open_with(environment: &[(&str, &str)]) -> Self {
        Self::started(environment, "")
    }

    /// As [`Self::open`], under a shell that ignores `SIGHUP`, as everything
    /// started under `nohup` does.
    ///
    /// That shell leads the terminal's session, so when the terminal goes away
    /// it takes the hang-up and does not end, and the kernel then has no
    /// reason to send one to `zaru`. It is how the eight processes this file
    /// was measured leaving behind on 2026-09-28 were left.
    fn open_ignoring_hangups() -> Self {
        Self::started(&[], "trap '' HUP; ")
    }

    fn started(environment: &[(&str, &str)], prelude: &str) -> Self {
        Self::started_in(environment, prelude, |_, _| {})
    }

    /// As [`Self::started`], with `setup` given the home and the working
    /// directory before `zaru` starts.
    fn started_in(
        environment: &[(&str, &str)],
        prelude: &str,
        setup: impl FnOnce(&std::path::Path, &std::path::Path),
    ) -> Self {
        Self::running(environment, prelude, "", setup)
    }

    /// As [`Self::started_in`], with `arguments` after `zaru` on its command
    /// line, each already quoted for the shell.
    fn running(
        environment: &[(&str, &str)],
        prelude: &str,
        arguments: &str,
        setup: impl FnOnce(&std::path::Path, &std::path::Path),
    ) -> Self {
        let home = Scratch::new("home");
        let work = Scratch::new("work");
        setup(&home.0, &work.0);
        let zaru = env!("CARGO_BIN_EXE_zaru");
        // The inner `sh` prints its own pid and `exec`s `zaru`, so the pid is
        // `zaru`'s and `zaru` is the terminal's foreground process rather than
        // a background job (which a non-interactive shell would hand
        // `/dev/null` for standard input). The outer shell reads the terminal's
        // line discipline once `zaru` is gone.
        // The status is written to a file as well, because a terminal that
        // has gone away is not somewhere anyone can read it from.
        let inner = format!(
            "{prelude}stty rows 30 cols 100; sh -c 'echo {PID}$$; exec \"$0\" \"$@\"' '{zaru}' \
             {arguments}; ended=$?; echo $ended > {STATUS_FILE}; echo {STATUS}$ended; stty -a"
        );
        let mut child = owned::command("script")
            .args(["-q", "-f", "-e", "-c", &inner, "/dev/null"])
            .current_dir(&work.0)
            .env_clear()
            .env("HOME", &home.0)
            .env("PATH", "/usr/bin:/bin")
            .env("TERM", "xterm-256color")
            .env("LANG", "C.UTF-8")
            .envs(environment.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("`script` from util-linux allocates the pseudo-terminal this check reads");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut stdout = child.take_stdout();
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
            work,
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

    /// Type `text` at the terminal.
    fn type_in(&mut self, text: &str) {
        use std::io::Write as _;
        self.child
            .stdin()
            .write_all(text.as_bytes())
            .and_then(|()| self.child.stdin().flush())
            .expect("the keys reach the terminal");
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

        let killed = owned::command("kill")
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
    let stdin = session.child.stdin();
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

/// **`terminal.mouse` is an [ADR-0014] key, `true` at layer 1**, so
/// `zaru config explain` says where a session's answer comes from, and
/// `ZARU_TERMINAL_MOUSE=false` at layer 4 turns it off.
///
/// **The mutant:** the key not declared, which prints the refusal
/// `config explain` gives an unknown key.
///
/// [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
#[test]
fn the_mouse_key_is_held_by_default_and_explained_by_layer() {
    let home = Scratch::new("explain-home");
    let explain = |environment: &[(&str, &str)]| {
        let output = owned::command(env!("CARGO_BIN_EXE_zaru"))
            .args(["config", "explain", "terminal.mouse"])
            .current_dir(&home.0)
            .env_clear()
            .env("HOME", &home.0)
            .envs(environment.iter().copied())
            .output()
            .expect("the built zaru runs");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    let (code, stdout, stderr) = explain(&[]);
    assert_eq!(
        code,
        Some(0),
        "`zaru config explain terminal.mouse` refused: {stderr}"
    );
    assert!(
        stdout.lines().next() == Some("terminal.mouse = true")
            && stdout.lines().any(|line| line.contains("built-in")
                && line.contains("true")
                && line.contains("← effective")),
        "the mouse is not held by default at layer 1: {stdout}"
    );

    let (code, stdout, stderr) = explain(&[("ZARU_TERMINAL_MOUSE", "false")]);
    assert_eq!(code, Some(0), "the layer-4 spelling refused: {stderr}");
    assert!(
        stdout.lines().next() == Some("terminal.mouse = false")
            && stdout.lines().any(|line| {
                line.contains("ZARU_TERMINAL_MOUSE")
                    && line.contains("false")
                    && line.contains("← effective")
            }),
        "ZARU_TERMINAL_MOUSE=false is not the effective answer: {stdout}"
    );
}

/// **With `terminal.mouse = false` no mouse mode is asked for**, the wheel is
/// the terminal's own, and the tip about selecting is not offered, because
/// nothing took the selection away.
///
/// The cost, stated where the key is declared: in Windows Terminal and the VS
/// Code terminal a wheel notch on the alternate screen then arrives as `Up` or
/// `Down`, which walk the history on an empty prompt.
///
/// **The mutant:** `arm` ignoring the key, which prints the modes it asked
/// for.
#[test]
fn a_session_with_the_mouse_key_off_asks_for_no_mouse_mode() {
    let session = InATerminal::open_with(&[("ZARU_TERMINAL_MOUSE", "false")]);
    let armed = session.until(b"\x1b[?25h", "the session's first frame");
    let taken = private_modes(&session.bytes()[..armed], true);
    assert_eq!(
        taken,
        [1049, 2004],
        "with terminal.mouse = false the session asked the terminal for private modes {taken:?}, \
         where it is owed the alternate screen and bracketed paste and no mouse mode"
    );
    std::thread::sleep(Duration::from_millis(1000));
    let painted = glyphs(&session.bytes());
    assert!(
        !painted.contains("holdShifttoselecttext"),
        "the session left the mouse to the terminal and still said to hold Shift: {painted:?}"
    );
}

// --------------------------------- no process outlives the check

/// How long a process this file started may take to be gone once nothing
/// should be holding it. Generous: what is measured is whether it goes at all,
/// and the failure it exists for is a process alive five hours later.
const GONE_WITHIN: Duration = Duration::from_secs(10);

/// Wait until none of `processes` is running, or say which still are.
fn until_gone(processes: &[(&str, owned::Identity)]) -> Result<(), String> {
    let deadline = Instant::now() + GONE_WITHIN;
    loop {
        let running: Vec<String> = processes
            .iter()
            .filter(|(_, identity)| identity.is_still_running())
            .map(|(what, identity)| format!("{what} (pid {})", identity.pid))
            .collect();
        if running.is_empty() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(running.join(", "));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// **A session a check is done with leaves nothing running, even when nothing
/// sends `zaru` a hang-up.**
///
/// Measured on 2026-09-28: a passing run of this file under `nohup` left eight
/// `zaru` processes alive, one for every session its checks opened and did not
/// end themselves, each with its terminal gone and its reader thread spinning.
/// The checks reaped `script` and nothing else; `zaru` is in the session
/// `script` made, so killing `script` reached it only through a hang-up, and
/// the shell leading that session ignored it.
///
/// **The mutant:** the helper killing only the child's process group and not
/// the tree below it, which prints the process left running.
#[test]
fn a_session_the_check_is_done_with_leaves_no_process_behind() {
    let session = InATerminal::open_ignoring_hangups();
    let pid: u32 = session.pid().parse().expect("zaru's pid is a number");
    session.until(b"\x1b[?25h", "the session's first frame");
    let zaru = owned::Identity::of(pid).expect("zaru is running once it has painted a frame");
    let script = owned::Identity::of(session.child.id()).expect("script is running");
    drop(session);

    if let Err(running) = until_gone(&[("script", script), ("zaru", zaru)]) {
        panic!(
            "{running} still running {GONE_WITHIN:?} after the check that started it was done \
             with it, under a shell that ignores SIGHUP as everything under `nohup` does"
        );
    }
}

/// **A session whose terminal goes away ends, as a hang-up ends it, even when
/// no hang-up arrives.**
///
/// The terminal went away and nothing told `zaru`: the shell leading its
/// session ignored `SIGHUP`, as everything under `nohup` does, so it did not
/// end and the kernel sent the foreground nothing. Measured on 2026-09-28
/// before this check: eight such `zaru` processes lived for five hours, each
/// with standard streams on `/dev/pts/N (deleted)`, and each with its terminal
/// reader spinning a core — crossterm's `poll` reads a hung-up terminal's
/// end of file as "nothing yet" and loops inside itself, so the reader never
/// looks at its stop flag again.
///
/// So the session ends when its terminal is gone, whoever says so: the
/// terminal is given back — to nobody, harmlessly — and the process exits
/// `129`, the status a hang-up already gives it.
///
/// **The mutant:** the session not watching for its terminal going away,
/// which prints that `zaru` is still running.
#[test]
fn a_session_whose_terminal_goes_away_ends_as_a_hang_up_ends_it() {
    let mut session = InATerminal::open_ignoring_hangups();
    let pid: u32 = session.pid().parse().expect("zaru's pid is a number");
    session.until(b"\x1b[?25h", "the session's first frame");
    let zaru = owned::Identity::of(pid).expect("zaru is running once it has painted a frame");

    // `script` alone, so the terminal's other end closes and nothing else is
    // touched: the shell and `zaru` are left to find out for themselves.
    session.child.kill_the_child_alone();

    if let Err(running) = until_gone(&[("zaru", zaru)]) {
        panic!(
            "{running} still running {GONE_WITHIN:?} after its terminal went away with no \
             SIGHUP to tell it: a harness that outlives its terminal is a harness nobody can \
             close, and it spins a core while it waits"
        );
    }
    let status = session.work.0.join(STATUS_FILE);
    let deadline = Instant::now() + GONE_WITHIN;
    let ended = loop {
        let written = std::fs::read_to_string(&status).unwrap_or_default();
        if !written.trim().is_empty() {
            break written.trim().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "zaru ended and its shell never wrote the status it ended with to {}",
            status.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        ended, "129",
        "a session whose terminal went away should end with 129, the status a hang-up gives it"
    );
}

/// What the child check below is told, so that it holds a session open only
/// when this file's own check asked it to.
const HOLDS_A_SESSION: &str = "TERMINAL_FROM_OUTSIDE_HOLDS_A_SESSION";

/// **A check killed while its session is open leaves nothing running.**
///
/// SIGKILL to the test binary alone, which is what the machine's watchdog, a
/// `timeout` or an out-of-memory killer sends: no `Drop` runs, so nothing the
/// check owns can reap anything. Measured on 2026-09-28 before
/// `tests/support/owned.rs`: `script` was handed to the machine's reaper and
/// polled a pipe nobody would write to again, and `zaru` under it kept a
/// terminal that never went away, both for as long as the machine stayed up.
///
/// The check runs this binary's own ignored child, which opens a session and
/// holds it, and kills that child and nothing else.
///
/// **The mutant:** `owned::command` without `--pdeathsig`, which prints the
/// processes left running.
#[test]
fn a_check_killed_while_its_session_is_open_leaves_no_process_behind() {
    let mut child =
        owned::command(std::env::current_exe().expect("the test binary knows where it is"))
            .args([
                "--exact",
                "the_killed_checks_child_holds_a_session_open",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(HOLDS_A_SESSION, "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("this crate's own test binary starts");
    let said = Arc::new(Mutex::new(String::new()));
    let mut stdout = child.take_stdout();
    let sink = Arc::clone(&said);
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 1024];
        while let Ok(read) = stdout.read(&mut chunk) {
            if read == 0 {
                break;
            }
            sink.lock()
                .expect("the sink is not poisoned")
                .push_str(&String::from_utf8_lossy(&chunk[..read]));
        }
    });
    let held = |name: &str| -> Option<u32> {
        let text = said.lock().expect("the sink is not poisoned").clone();
        let at = text.find(name)? + name.len();
        text[at..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let (script, zaru) = loop {
        if let (Some(script), Some(zaru)) = (held("HELD-SCRIPT="), held("HELD-ZARU=")) {
            break (script, zaru);
        }
        assert!(
            Instant::now() < deadline,
            "the child check never said it was holding a session: {:?}",
            said.lock().expect("the sink is not poisoned")
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let script = owned::Identity::of(script).expect("the child's script is running");
    let zaru = owned::Identity::of(zaru).expect("the child's zaru is running");

    child.kill_the_child_alone();

    if let Err(running) = until_gone(&[("script", script), ("zaru", zaru)]) {
        panic!(
            "{running} still running {GONE_WITHIN:?} after the check that started it was killed \
             with SIGKILL, so a check that is killed rather than failed leaves its processes \
             behind"
        );
    }
}

/// The child half of the check above. Never run on its own.
#[test]
#[ignore = "re-invoked by `a_check_killed_while_its_session_is_open_leaves_no_process_behind`"]
fn the_killed_checks_child_holds_a_session_open() {
    assert!(
        std::env::var_os(HOLDS_A_SESSION).is_some(),
        "this check holds a session open until it is killed, and is run only by \
         `a_check_killed_while_its_session_is_open_leaves_no_process_behind`"
    );
    let session = InATerminal::open();
    let zaru = session.pid();
    session.until(b"\x1b[?25h", "the session's first frame");
    println!("HELD-SCRIPT={}", session.child.id());
    println!("HELD-ZARU={zaru}");
    // Held until the check above kills this process. Its own deadline, so a
    // child nobody kills still ends.
    std::thread::sleep(Duration::from_secs(120));
}

// ------------------------------------------- a project's validators, approved

/// The sealing key for the store this file's sessions read.
const SEALING_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Inside a session, a project's validators are asked about on the pane before
/// the first turn that would run them, and a no stops the turn before the
/// model or any validator is reached.
///
/// The provider is a closed port on this machine, so nothing leaves it.
#[test]
fn a_session_asks_before_a_projects_validators_run_and_a_no_stops_the_turn() {
    let mut session = InATerminal::started_in(
        &[
            ("ZARU_CREDENTIAL_KEY", SEALING_KEY),
            ("ZARU_PROVIDER_GEMINI_ENDPOINT", "http://127.0.0.1:1"),
            ("ZARU_MODEL_DEFAULT", "gemini-3.6-flash"),
        ],
        "",
        |home, work| {
            use std::io::Write as _;
            let mut child = owned::command(env!("CARGO_BIN_EXE_zaru"))
                .args(["providers", "keys", "add", "gemini"])
                .env_clear()
                .env("HOME", home)
                .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
                .current_dir(work)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("the built binary runs");
            child
                .stdin()
                .write_all(b"nonce-terminal-validators\n")
                .expect("the key reaches the child");
            assert!(
                child.wait().expect("it exits").success(),
                "the key was not stored"
            );
            std::fs::write(
                work.join("zaru.toml"),
                "[[validator]]\nname = \"plant\"\nrun = \"touch VALIDATOR-RAN\"\nexpect = \
                 \"exit-zero\"\n",
            )
            .expect("the manifest is written");
        },
    );
    session.until(b"\x1b[?25h", "the session's first frame");
    session.type_in("build it\r");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !glyphs(&session.bytes()).contains("plant:touchVALIDATOR-RAN") {
        assert!(
            Instant::now() < deadline,
            "the session never asked about the project's validators; it painted {:?}",
            glyphs(&session.bytes())
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let asked = glyphs(&session.bytes());
    assert!(
        asked.contains("Allowthesecommandstoruninthisproject?"),
        "the question did not ask: {asked:?}"
    );
    session.type_in("n");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !glyphs(&session.bytes()).contains("youdidnotapprovethevalidators") {
        assert!(
            Instant::now() < deadline,
            "a no did not stop the turn; the session painted {:?}",
            glyphs(&session.bytes())
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !session.work.0.join("VALIDATOR-RAN").exists(),
        "the validator ran after the person said no"
    );
    let painted = glyphs(&session.bytes());
    assert!(
        !painted.contains("providercouldnotbereached"),
        "the turn went on to the provider after a no: {painted:?}"
    );
}

// --------------------------------- a session ended while a question stands

/// The time the ruling of 2026-09-28 gives a session to be gone once its
/// terminal has gone away or a signal has arrived.
const GONE_WITHIN_A_PROMPT: Duration = Duration::from_secs(2);

/// Wait until `zaru` is gone, for at most [`GONE_WITHIN_A_PROMPT`].
fn gone_within_two_seconds(zaru: &owned::Identity) -> Result<(), String> {
    let deadline = Instant::now() + GONE_WITHIN_A_PROMPT;
    loop {
        if !zaru.is_still_running() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("zaru (pid {})", zaru.pid));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A session in a project that declares a validator, standing at the
/// question that asks whether it may run: the one question a session asks
/// before any model is reached, so it needs no provider. The provider is a
/// closed port on this machine, so nothing leaves it.
fn at_the_validators_question(prelude: &str) -> InATerminal {
    let mut session = with_a_validator(prelude, "");
    session.until(b"\x1b[?25h", "the session's first frame");
    session.type_in("build it\r");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !glyphs(&session.bytes()).contains("plant:touchVALIDATOR-RAN") {
        assert!(
            Instant::now() < deadline,
            "the session never asked about the project's validators; it painted {:?}",
            glyphs(&session.bytes())
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    session
}

/// `zaru` with `arguments`, in a project that declares a validator, with a
/// stored key and a provider on a closed port.
fn with_a_validator(prelude: &str, arguments: &str) -> InATerminal {
    InATerminal::running(
        &[
            ("ZARU_CREDENTIAL_KEY", SEALING_KEY),
            ("ZARU_PROVIDER_GEMINI_ENDPOINT", "http://127.0.0.1:1"),
            ("ZARU_MODEL_DEFAULT", "gemini-3.6-flash"),
        ],
        prelude,
        arguments,
        |home, work| {
            use std::io::Write as _;
            let mut child = owned::command(env!("CARGO_BIN_EXE_zaru"))
                .args(["providers", "keys", "add", "gemini"])
                .env_clear()
                .env("HOME", home)
                .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
                .current_dir(work)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("the built binary runs");
            child
                .stdin()
                .write_all(b"nonce-terminal-ended-at-a-question\n")
                .expect("the key reaches the child");
            assert!(
                child.wait().expect("it exits").success(),
                "the key was not stored"
            );
            std::fs::write(
                work.join("zaru.toml"),
                "[[validator]]\nname = \"plant\"\nrun = \"touch VALIDATOR-RAN\"\nexpect = \
                 \"exit-zero\"\n",
            )
            .expect("the manifest is written");
        },
    )
}

/// The one session the check's home holds, read back by the built binary as
/// a person's `--resume` through a pipe reads it: the transcript, and exit 0.
///
/// The task itself is not on it: a turn records the person's words after the
/// validators are approved, so a turn stopped at that question has said only
/// the session's notice. What is asserted is that what was written reads back.
fn resumes(session: &InATerminal) -> String {
    let sessions = session._home.0.join(".zaru").join("sessions");
    let ids: Vec<String> = std::fs::read_dir(&sessions)
        .expect("the home holds a sessions directory")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(ids.len(), 1, "one session was opened: {ids:?}");
    let output = owned::command(env!("CARGO_BIN_EXE_zaru"))
        .args(["--resume", &ids[0]])
        .env_clear()
        .env("HOME", &session._home.0)
        .env("ZARU_CREDENTIAL_KEY", SEALING_KEY)
        .current_dir(&session.work.0)
        .output()
        .expect("the built binary runs");
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "the session did not resume: {:?}, {printed}, {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        printed.contains("record(s) in the transcript"),
        "the resumed session printed no transcript: {printed}"
    );
    printed
}

/// The status the wrapping shell wrote once `zaru` ended.
fn written_status(session: &InATerminal) -> String {
    let status = session.work.0.join(STATUS_FILE);
    let deadline = Instant::now() + GONE_WITHIN;
    loop {
        let written = std::fs::read_to_string(&status).unwrap_or_default();
        if !written.trim().is_empty() {
            return written.trim().to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "zaru ended and its shell never wrote the status it ended with"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// **A session whose terminal goes away while a question stands ends within
/// two seconds, as a hang-up ends it, and can be resumed.**
///
/// Measured on `e5b9240`: with the terminal closed at a permission question,
/// `zaru` was still running ten seconds later at a full core, holding the
/// session's transcript and its deleted terminal, because the question was
/// answered on the session's only thread and nothing else on it ran while it
/// stood. Under a shell that ignores `SIGHUP`, so only the lost terminal says
/// anything.
#[test]
fn a_session_whose_terminal_goes_away_at_a_question_ends_within_two_seconds() {
    let mut session = at_the_validators_question("trap '' HUP; ");
    let pid: u32 = session.pid().parse().expect("zaru's pid is a number");
    let zaru = owned::Identity::of(pid).expect("zaru is running at the question");

    session.child.kill_the_child_alone();

    if let Err(running) = gone_within_two_seconds(&zaru) {
        panic!(
            "{running} still running {GONE_WITHIN_A_PROMPT:?} after its terminal went away while it asked a question"
        );
    }
    assert_eq!(
        written_status(&session),
        "129",
        "a session whose terminal went away should end with 129, the status a hang-up gives it"
    );
    assert!(
        !session.work.0.join("VALIDATOR-RAN").exists(),
        "the validator ran although nobody answered the question"
    );
    resumes(&session);
}

/// **The same, when the terminal's hang-up reaches `zaru` as `SIGHUP`.**
#[test]
fn a_session_hung_up_at_a_question_ends_within_two_seconds() {
    let mut session = at_the_validators_question("");
    let pid: u32 = session.pid().parse().expect("zaru's pid is a number");
    let zaru = owned::Identity::of(pid).expect("zaru is running at the question");

    session.child.kill_the_child_alone();

    if let Err(running) = gone_within_two_seconds(&zaru) {
        panic!(
            "{running} still running {GONE_WITHIN_A_PROMPT:?} after its terminal hung up while it asked a question"
        );
    }
    assert!(
        !session.work.0.join("VALIDATOR-RAN").exists(),
        "the validator ran although nobody answered the question"
    );
    resumes(&session);
}

/// **A session sent `SIGTERM` while a question stands ends within two
/// seconds with `143`, gives the terminal back, and can be resumed.**
///
/// Measured on `e5b9240`: still running ten seconds after the signal.
#[test]
fn a_session_sent_sigterm_at_a_question_ends_within_two_seconds() {
    let session = at_the_validators_question("");
    let pid = session.pid();
    let zaru = owned::Identity::of(pid.parse().expect("zaru's pid is a number"))
        .expect("zaru is running at the question");

    let killed = owned::command("kill")
        .args(["-TERM", &pid])
        .status()
        .expect("`kill` runs");
    assert!(killed.success(), "`kill -TERM {pid}` failed");

    if let Err(running) = gone_within_two_seconds(&zaru) {
        panic!("{running} still running {GONE_WITHIN_A_PROMPT:?} after SIGTERM at a question");
    }
    assert_eq!(
        written_status(&session),
        "143",
        "SIGTERM should end the session with 143, the status a shell reports for it"
    );
    session.until(b"columns", "`stty -a` to report the line discipline");
    let status_at = session.until(STATUS.as_bytes(), "the shell to report");
    let flags = discipline(&String::from_utf8_lossy(&session.bytes()[status_at..]));
    assert_eq!(
        flags,
        ["isig", "icanon", "echo"],
        "the terminal was not given back after SIGTERM at a question"
    );
    assert!(
        !session.work.0.join("VALIDATOR-RAN").exists(),
        "the validator ran although nobody answered the question"
    );
    resumes(&session);
}

/// **`zaru "<task>"` sent `SIGTERM` while its question stands ends within two
/// seconds with `143`, and can be resumed.**
///
/// The task's question is read on a thread of its own since 2026-09-28, so
/// the turn goes on being polled while a person reads it and a signal stops
/// the turn. This holds that a reader left waiting on standard input does not
/// keep the process alive. On `e5b9240` the signal's default action ended the
/// process, so this passes there too; it pins the status and the resume.
#[test]
fn a_task_sent_sigterm_at_its_question_ends_within_two_seconds() {
    let session = with_a_validator("", "'build it'");
    let pid = session.pid();
    let zaru =
        owned::Identity::of(pid.parse().expect("zaru's pid is a number")).expect("zaru is running");
    session.until(b"Allow these commands", "the task's question");

    let killed = owned::command("kill")
        .args(["-TERM", &pid])
        .status()
        .expect("`kill` runs");
    assert!(killed.success(), "`kill -TERM {pid}` failed");

    if let Err(running) = gone_within_two_seconds(&zaru) {
        panic!("{running} still running {GONE_WITHIN_A_PROMPT:?} after SIGTERM at its question");
    }
    assert_eq!(
        written_status(&session),
        "143",
        "SIGTERM should end the task with 143, the status a shell reports for it"
    );
    assert!(
        !session.work.0.join("VALIDATOR-RAN").exists(),
        "the validator ran although nobody answered the question"
    );
    resumes(&session);
}

// --------------------------------- a home and an environment nobody handed

#[path = "support/decoy.rs"]
mod decoy;
#[path = "support/owned.rs"]
mod owned;

/// Every other check in this file, re-run under a home and an environment none
/// of them was handed. See `tests/support/decoy.rs` for the two defects it
/// holds shut and what the decoy is.
#[test]
fn corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed() {
    decoy::every_other_check_keeps_its_verdict(
        "corpus_no_check_here_reads_a_home_or_an_environment_it_was_not_handed",
    );
}
