// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every surface the shell paints, captured cell by cell and held against a
//! committed rendering.
//!
//! # Why this exists
//!
//! The harness's look is part of the product: [ADR-0005]'s composer, [ADR-0001]
//! D2's status line, [ADR-0028]'s pane and [ADR-0011] D3's question are laid
//! out by this crate and drawn by `ratatui`. **A move of the rendering library
//! must change nothing a person sees**, and the checks elsewhere in this crate
//! each assert one property of one surface — the tier survives a narrow row,
//! the input row does not move — so a library that changed a default nobody
//! asserted (a wrap point, a clip, a style reset, a wide glyph's trailing cell)
//! would pass every one of them. This module asserts the whole buffer.
//!
//! Each capture is a shell staged through the public API alone, painted into
//! `TestBackend` at a stated size, and serialised here: every row's symbols,
//! every cell whose style is not the default with its foreground, background,
//! modifier and skip flag, and the cursor. **The serialisation is this
//! module's own**, written out variant by variant rather than through the
//! library's `Debug`, so the text cannot change because the library's
//! formatting did. The committed renderings under `captures/` were produced on
//! `ratatui` 0.29.0 and `tui-textarea` 0.7.0, before the move, by this module
//! as it stood then; on 0.30 it reads a cell's diff option where 0.29 had a
//! skip flag, and prints anything else a cell carries -- an underline colour,
//! another diff option -- that an empty cell does not.
//!
//! A mismatch prints both renderings, the committed one and the painted one,
//! for every capture that differs rather than the first, so one run names the
//! whole change.
//!
//! [ADR-0001]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0001-runtime-tiers
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative

use crate::composer::fixtures::{NoPaths, PathsOf, TrieOf, typing, typing_paths};
use crate::shell::fixtures::{StagedTranscript, StagedVocabulary};
use crate::shell::port::{Answers, Confirmation, Line, Palette, Register, SecretRequest};
use crate::shell::{Input, Key, Queued, Segment, Shell, Status};
use core::fmt::Write as _;
use core::time::Duration;
use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::buffer::{Cell, CellDiffOption};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

const NOW: Duration = Duration::from_millis(10);

/// A session id of the shape the product mints, fixed so a capture is stable.
const SESSION: &str = "01JQZX8N3K4M5P6R7S8T9V0W1X";

/// An answer long enough to wrap at every captured width, carrying the three
/// pieces of markup the pane paints as a style rather than as a delimiter.
const WRAPPED_ANSWER: &str = "The composer keeps **one input row** whatever the strip above it \
     shows, and the pane word-wraps a long answer between words rather than inside one, so \
     `Shell::render` never clips a line a person was meant to read. *Nothing here* is a \
     second paragraph's worth of text.";

fn shell() -> Shell {
    Shell::open(Status::new("bare", SESSION))
}

/// Every field the status row can carry, so its arbitration is exercised at
/// every width a capture takes.
fn crowded() -> Shell {
    let mut status = Status::new("bare", SESSION);
    status.credential = Some("apex (no instance boundary)".to_owned());
    let mut shell = Shell::open(status);
    shell.set_context_usage(Some(Segment::new(
        "context 1.2k/1048.5k tokens",
        "1.2k/1048.5k",
    )));
    shell.set_token_usage(Some(Segment::new(
        "tokens: 390 prompt + 79 completion = 469",
        "469 tokens",
    )));
    shell.set_elapsed(Some("12.34s".to_owned()));
    shell.describe(
        Some("gemini-3.6-flash".to_owned()),
        Some("mode ask".to_owned()),
    );
    shell
}

fn press(shell: &mut Shell, key: Key) {
    shell.key(
        Input {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        },
        Rect::default(),
        NOW,
        &TrieOf::new(0),
        &StagedVocabulary,
        &NoPaths,
    );
}

fn wrapped_answer() -> Shell {
    let mut shell = crowded();
    shell.refresh(&StagedTranscript(vec![
        Line::new(Register::Plain, "› explain the composer"),
        Line::answer(Register::Plain, "zaru: ", WRAPPED_ANSWER),
    ]));
    shell
}

fn every_register() -> Shell {
    let mut shell = shell();
    shell.refresh(&StagedTranscript(
        Register::ALL
            .into_iter()
            .map(|register| Line::new(register, format!("a line in {register:?} · 1.25s")))
            .collect(),
    ));
    shell
}

fn slash_hint() -> Shell {
    let mut shell = shell();
    typing(shell.composer_mut(), "/se", NOW, &TrieOf::new(0));
    shell
}

fn notes_hint() -> Shell {
    let mut shell = shell();
    typing(shell.composer_mut(), "mémo", NOW, &TrieOf::new(3));
    shell
}

fn path_hint() -> Shell {
    let mut shell = shell();
    typing_paths(
        shell.composer_mut(),
        "read @cr",
        NOW,
        &TrieOf::new(0),
        &PathsOf::new([
            "crates/",
            "crates/zaru-cli/",
            "crates/zaru-tui/src/shell.rs",
        ]),
    );
    shell
}

fn tip() -> Shell {
    let mut shell = shell();
    shell.composer_mut().set_standing(
        0,
        Some("tip: hold Shift to select text with the mouse".to_owned()),
    );
    shell
}

fn deposits() -> Shell {
    let mut shell = shell();
    shell
        .composer_mut()
        .set_standing(2, Some("tip: outranked".to_owned()));
    shell
}

fn confirmation_with_detail() -> Shell {
    let mut shell = crowded();
    shell.ask(
        Confirmation::new(
            "Allow fs.write /home/person/project/crates/zaru-cli/src/terminal/driver.rs?",
            "[y/a/N]",
            Answers::ToolCall,
            false,
        )
        .showing(vec![
            "creates it, with:".to_owned(),
            "  alpha".to_owned(),
            "  beta".to_owned(),
            "  gamma".to_owned(),
        ]),
    );
    shell
}

fn confirmation_prominent() -> Shell {
    let mut shell = shell();
    shell.ask(Confirmation::new(
        "delete every file under /tmp",
        "[y/N]",
        Answers::ToolCall,
        true,
    ));
    shell
}

fn secret() -> Shell {
    let mut shell = shell();
    shell.ask_secret(SecretRequest::new(
        "Paste the key for provider gemini.",
        "It is stored sealed; Esc stores nothing.",
    ));
    for character in "AIzaSy".chars() {
        press(&mut shell, Key::Char(character));
    }
    shell
}

fn queued_while_streaming() -> Shell {
    let mut shell = crowded();
    shell.notice(Line::new(
        Register::Call,
        "fs.read crates/zaru-tui/src/shell.rs",
    ));
    shell.stream_delta("An answer arriving in pieces, ");
    shell.stream_delta("long enough to take a second row at the narrow width.");
    shell.queue(Queued::of("and then run the suite"));
    shell
}

fn held_pane() -> Shell {
    let mut shell = shell();
    for at in 1..=40 {
        shell.notice(Line::new(Register::Plain, format!("line {at}")));
    }
    shell.page_up(16, 60);
    shell
}

fn typed_line_with_wide_glyphs() -> Shell {
    let mut shell = shell();
    typing(
        shell.composer_mut(),
        "日本語 and ✦ and é in one line",
        NOW,
        &TrieOf::new(0),
    );
    shell
}

/// A shell staged for one capture.
type Staging = fn() -> Shell;

/// Every capture: its name, the committed rendering, how it is staged, the
/// size and the palette it is painted at.
const CAPTURES: &[(&str, &str, Staging, u16, u16, Palette)] = &[
    (
        "status-at-120",
        include_str!("captures/status-at-120.txt"),
        crowded,
        120,
        12,
        Palette::Coloured,
    ),
    (
        "status-at-72",
        include_str!("captures/status-at-72.txt"),
        crowded,
        72,
        12,
        Palette::Coloured,
    ),
    (
        "status-at-40",
        include_str!("captures/status-at-40.txt"),
        crowded,
        40,
        12,
        Palette::Coloured,
    ),
    (
        "pane-wrapped-answer-at-72",
        include_str!("captures/pane-wrapped-answer-at-72.txt"),
        wrapped_answer,
        72,
        20,
        Palette::Coloured,
    ),
    (
        "pane-wrapped-answer-at-40",
        include_str!("captures/pane-wrapped-answer-at-40.txt"),
        wrapped_answer,
        40,
        24,
        Palette::Coloured,
    ),
    (
        "pane-every-register-coloured",
        include_str!("captures/pane-every-register-coloured.txt"),
        every_register,
        60,
        16,
        Palette::Coloured,
    ),
    (
        "pane-every-register-monochrome",
        include_str!("captures/pane-every-register-monochrome.txt"),
        every_register,
        60,
        16,
        Palette::Monochrome,
    ),
    (
        "composer-slash-hint",
        include_str!("captures/composer-slash-hint.txt"),
        slash_hint,
        60,
        16,
        Palette::Coloured,
    ),
    (
        "composer-notes-hint",
        include_str!("captures/composer-notes-hint.txt"),
        notes_hint,
        60,
        16,
        Palette::Coloured,
    ),
    (
        "composer-path-hint",
        include_str!("captures/composer-path-hint.txt"),
        path_hint,
        60,
        16,
        Palette::Coloured,
    ),
    (
        "composer-wide-glyphs",
        include_str!("captures/composer-wide-glyphs.txt"),
        typed_line_with_wide_glyphs,
        40,
        12,
        Palette::Coloured,
    ),
    (
        "tips-line",
        include_str!("captures/tips-line.txt"),
        tip,
        60,
        12,
        Palette::Coloured,
    ),
    (
        "deposits-line",
        include_str!("captures/deposits-line.txt"),
        deposits,
        60,
        12,
        Palette::Coloured,
    ),
    (
        "confirmation-with-detail-at-72",
        include_str!("captures/confirmation-with-detail-at-72.txt"),
        confirmation_with_detail,
        72,
        16,
        Palette::Coloured,
    ),
    (
        "confirmation-with-detail-at-40",
        include_str!("captures/confirmation-with-detail-at-40.txt"),
        confirmation_with_detail,
        40,
        16,
        Palette::Coloured,
    ),
    (
        "confirmation-prominent",
        include_str!("captures/confirmation-prominent.txt"),
        confirmation_prominent,
        60,
        12,
        Palette::Coloured,
    ),
    (
        "secret-question",
        include_str!("captures/secret-question.txt"),
        secret,
        60,
        12,
        Palette::Coloured,
    ),
    (
        "queued-while-streaming",
        include_str!("captures/queued-while-streaming.txt"),
        queued_while_streaming,
        40,
        16,
        Palette::Coloured,
    ),
    (
        "held-pane",
        include_str!("captures/held-pane.txt"),
        held_pane,
        60,
        16,
        Palette::Coloured,
    ),
];

/// A colour, spelled here rather than by the library's `Debug`.
fn colour(colour: Color) -> String {
    match colour {
        Color::Reset => "reset".to_owned(),
        Color::Black => "black".to_owned(),
        Color::Red => "red".to_owned(),
        Color::Green => "green".to_owned(),
        Color::Yellow => "yellow".to_owned(),
        Color::Blue => "blue".to_owned(),
        Color::Magenta => "magenta".to_owned(),
        Color::Cyan => "cyan".to_owned(),
        Color::Gray => "gray".to_owned(),
        Color::DarkGray => "dark-gray".to_owned(),
        Color::LightRed => "light-red".to_owned(),
        Color::LightGreen => "light-green".to_owned(),
        Color::LightYellow => "light-yellow".to_owned(),
        Color::LightBlue => "light-blue".to_owned(),
        Color::LightMagenta => "light-magenta".to_owned(),
        Color::LightCyan => "light-cyan".to_owned(),
        Color::White => "white".to_owned(),
        Color::Rgb(red, green, blue) => format!("rgb({red},{green},{blue})"),
        Color::Indexed(index) => format!("indexed({index})"),
    }
}

/// A modifier, spelled flag by flag here rather than by the library's `Debug`.
fn modifier(modifier: Modifier) -> String {
    const FLAGS: [(Modifier, &str); 9] = [
        (Modifier::BOLD, "bold"),
        (Modifier::DIM, "dim"),
        (Modifier::ITALIC, "italic"),
        (Modifier::UNDERLINED, "underlined"),
        (Modifier::SLOW_BLINK, "slow-blink"),
        (Modifier::RAPID_BLINK, "rapid-blink"),
        (Modifier::REVERSED, "reversed"),
        (Modifier::HIDDEN, "hidden"),
        (Modifier::CROSSED_OUT, "crossed-out"),
    ];
    let named: Vec<&str> = FLAGS
        .iter()
        .filter(|(flag, _)| modifier.contains(*flag))
        .map(|(_, name)| *name)
        .collect();
    if named.is_empty() {
        "none".to_owned()
    } else {
        named.join("+")
    }
}

/// Paint `shell` at `width` by `height` and serialise every cell.
fn capture(shell: &Shell, width: u16, height: u16, palette: Palette) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| shell.render(frame, frame.area(), palette))
        .expect("draw");
    let cursor = terminal
        .backend_mut()
        .get_cursor_position()
        .expect("the test backend records the cursor");
    let buffer = terminal.backend().buffer();

    let mut out = String::new();
    writeln!(out, "size {width}x{height}").expect("write to a String");
    writeln!(out, "cursor {},{}", cursor.x, cursor.y).expect("write to a String");
    writeln!(out, "rows").expect("write to a String");
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        writeln!(out, "|{row}|").expect("write to a String");
    }
    writeln!(out, "styled cells").expect("write to a String");
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let cell = &buffer[(x, y)];
            let skip = cell.diff_option == CellDiffOption::Skip;
            // Whatever else a cell carries beyond its symbol, its two colours,
            // its modifier and whether it is skipped -- an underline colour, a
            // diff option other than a skip -- is found by clearing those four
            // and comparing what is left with an empty cell, so a property a
            // later library adds is printed rather than passed over.
            let mut rest = cell.clone();
            rest.set_symbol(" ");
            rest.fg = Color::Reset;
            rest.bg = Color::Reset;
            rest.modifier = Modifier::empty();
            if skip {
                rest.set_diff_option(CellDiffOption::None);
            }
            let other = (rest != Cell::EMPTY).then(|| format!(" other={rest:?}"));
            let plain = cell.fg == Color::Reset
                && cell.bg == Color::Reset
                && cell.modifier.is_empty()
                && !skip
                && other.is_none();
            if !plain {
                writeln!(
                    out,
                    "{x},{y} {:?} fg={} bg={} mod={}{}{}",
                    cell.symbol(),
                    colour(cell.fg),
                    colour(cell.bg),
                    modifier(cell.modifier),
                    if skip { " skip" } else { "" },
                    other.unwrap_or_default(),
                )
                .expect("write to a String");
            }
        }
    }
    out
}

/// Every surface paints, cell for cell, what it painted before the rendering
/// library moved.
///
/// Every capture is compared and every mismatch is reported with both
/// renderings, so a run names the whole of a change rather than its first
/// surface.
#[test]
fn every_surface_paints_byte_for_byte_what_it_painted_before_the_library_moved() {
    let mut differ = Vec::new();
    for (name, committed, staging, width, height, palette) in CAPTURES {
        let painted = capture(&staging(), *width, *height, *palette);
        if painted != *committed {
            differ.push(format!(
                "=== capture {name}: committed ===\n{committed}=== capture {name}: painted ===\n\
                 {painted}=== end {name} ==="
            ));
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} captures differ from the committed rendering:\n{}",
        differ.len(),
        CAPTURES.len(),
        differ.join("\n")
    );
}

/// The capture reads what it claims to: a colour, a modifier, a wide glyph and
/// the cursor each reach the serialisation, so a capture that wrote only
/// symbols could not stand in for this one.
///
/// **What a wide glyph's trailing cell holds is not asserted here**: it is
/// part of what the committed renderings pin, and on 0.29 it is a blank cell
/// rather than a `skip` one, which a self-check must not decide for them.
#[test]
fn a_capture_carries_colour_modifier_wide_glyphs_and_cursor() {
    let coloured = capture(&every_register(), 60, 16, Palette::Coloured);
    let monochrome = capture(&every_register(), 60, 16, Palette::Monochrome);
    assert_ne!(
        coloured, monochrome,
        "the same shell under two palettes serialised identically, so the capture does not read a \
         cell's colour"
    );
    let answer = capture(&wrapped_answer(), 72, 20, Palette::Coloured);
    assert!(
        answer.contains("mod=bold"),
        "the answer's bold run did not reach the capture:\n{answer}"
    );
    let wide = capture(&typed_line_with_wide_glyphs(), 40, 12, Palette::Coloured);
    assert!(
        wide.contains('日') && wide.contains('✦'),
        "a wide glyph did not reach the capture:\n{wide}"
    );
    let moved = capture(&slash_hint(), 60, 16, Palette::Coloured);
    let still = capture(&shell(), 60, 16, Palette::Coloured);
    let cursor_of = |capture: &str| capture.lines().nth(1).map(str::to_owned);
    assert_ne!(
        cursor_of(&moved),
        cursor_of(&still),
        "typing moved no cursor in the capture, so the capture does not read the cursor"
    );
}
