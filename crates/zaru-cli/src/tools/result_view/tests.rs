// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Checks for what a person is shown of a call.

use super::*;

/// The rows of a view as the file that keeps the whole spells them.
fn spelled(view: &ResultView) -> Vec<String> {
    let width = number_width(&view.rows);
    view.rows
        .iter()
        .map(|row| row_text(row, width, true))
        .collect()
}

/// **Security corpus.** A command's output can hold every sequence a terminal
/// acts on, and none of them survives into a row.
///
/// The hostile set: clear the screen, move the cursor, set the window title,
/// switch to the other screen, ring the bell, go back over what was written,
/// return to the start of the line, the one-byte form of the escape that
/// begins a sequence, and a character that reverses the direction text is
/// drawn in. Each is written out where it was, so the person sees the output
/// held it.
///
/// The mutant: `harmless` returning its input, which prints the first
/// control character that reached a row.
#[test]
fn no_control_character_a_command_prints_reaches_a_row() {
    let hostile = "before\n\u{1b}[2J\u{1b}[H\u{1b}]0;TITLE\u{7}\u{1b}[31mred\u{1b}[0m\n\
                   \u{1b}[10;10Hmoved\nvisible\rOVERWRITE\nback\u{8}\u{8}X\n\u{9b}31m one-byte\n\
                   \u{202e}reversed\u{202c}\nafter\n";
    let composed = command(0, hostile, "\u{1b}[?1049h on the other stream\n");
    let view = composed.clone().finished(None);
    for row in view
        .rows
        .iter()
        .map(|row| row.text.as_str())
        .chain([view.summary.as_str()])
    {
        let bad: Vec<char> = row
            .chars()
            .filter(|character| character.is_control() || is_direction_control(*character))
            .collect();
        assert!(
            bad.is_empty(),
            "a row a terminal would draw holds {bad:?}: {row:?}"
        );
    }
    let whole = composed
        .left_out
        .map(|left_out| left_out.whole)
        .unwrap_or_default();
    assert!(
        !whole.contains('\u{1b}') && !whole.contains('\u{9b}') && !whole.contains('\u{7}'),
        "the file that keeps the whole holds a control character a `cat` would act on: \
         {whole:?}"
    );
    let rows = format!("{}\n{whole}", spelled(&view).join("\n"));
    for written_out in [
        "\\u{1b}[2J",
        "\\u{1b}]0;TITLE\\u{7}",
        "\\u{1b}[10;10H",
        "visible\\rOVERWRITE",
        "\\u{9b}31m",
        "\\u{202e}reversed",
    ] {
        assert!(
            rows.contains(written_out),
            "the rows and the whole do not show {written_out:?} where the output held it:\n{rows}"
        );
    }
}

/// A tab is spaces to the next multiple of eight, and ordinary text is left
/// as it was.
#[test]
fn a_tab_is_spaces_and_ordinary_text_is_unchanged() {
    assert_eq!(harmless("a\tb"), "a       b");
    assert_eq!(harmless("12345678\tx"), "12345678        x");
    assert_eq!(harmless("naïve 日本 🦀"), "naïve 日本 🦀");
}

/// A command's view says its exit and how much it printed on each stream, and
/// shows its last lines, standard error marked apart from standard output.
///
/// The mutant: the first lines rather than the last, which prints that the
/// last line of the output is not shown.
#[test]
fn a_command_shows_its_exit_its_counts_and_its_last_lines() {
    let stdout: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let view = command(3, &stdout, "boom\n").finished(Some(Path::new("/s/shown-0001.txt")));

    assert_eq!(
        view.summary,
        "exit 3 · 30 lines on standard output, 1 on standard error"
    );
    let rows = spelled(&view);
    assert_eq!(
        rows.first().map(String::as_str),
        Some("... 24 earlier lines not shown · all of it: /s/shown-0001.txt"),
        "the note does not say how many lines are not shown and where the whole is: {rows:#?}"
    );
    assert!(
        rows.contains(&String::from("out | line 30")),
        "the last line of the output is not shown: {rows:#?}"
    );
    assert!(
        !rows.contains(&String::from("out | line 24")),
        "a line the note says is not shown is shown: {rows:#?}"
    );
    assert_eq!(
        rows.last().map(String::as_str),
        Some("err | boom"),
        "standard error is not shown, or not marked: {rows:#?}"
    );
    assert_eq!(
        view.rows.len(),
        OUTPUT_ROWS + 1,
        "a command shows {OUTPUT_ROWS} lines and a note, and this showed {}",
        view.rows.len()
    );
}

/// A command that printed nothing says so, and one that printed a few lines
/// shows them all with no note.
#[test]
fn a_command_that_printed_nothing_says_so() {
    let silent = command(0, "", "").finished(None);
    assert_eq!(silent.summary, "exit 0 · printed nothing");
    assert!(silent.rows.is_empty());

    let short = command(1, "", "one\ntwo\n").finished(None);
    assert_eq!(short.summary, "exit 1 · 2 lines on standard error");
    assert_eq!(spelled(&short), vec!["err | one", "err | two"]);
}

/// Standard error keeps at least half the rows when both streams printed
/// more than fits, because that is where most programs say what went wrong.
#[test]
fn standard_error_keeps_half_the_rows_when_both_streams_are_long() {
    let stdout: String = (1..=20).map(|n| format!("o{n}\n")).collect();
    let stderr: String = (1..=20).map(|n| format!("e{n}\n")).collect();
    let composed = command(1, &stdout, &stderr);
    let errors = composed
        .view
        .rows
        .iter()
        .filter(|row| row.mark == Mark::Error)
        .count();
    assert_eq!(errors, OUTPUT_ROWS / 2);
    let outputs = OUTPUT_ROWS - errors;
    assert_eq!(
        composed.left_out.map(|left_out| left_out.sentence),
        Some(format!(
            "{} earlier lines not shown: {} of standard output and {} of standard error",
            40 - OUTPUT_ROWS,
            20 - outputs,
            20 - errors
        ))
    );
}

/// The file behind a view holds every line, even those the pane shows.
#[test]
fn the_whole_holds_every_line_of_both_streams() {
    let stdout: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let whole = command(0, &stdout, "warning\n")
        .left_out
        .expect("thirty lines do not fit")
        .whole;
    for n in 1..=30 {
        assert!(
            whole.contains(&format!("line {n}\n")),
            "line {n} is not kept"
        );
    }
    assert!(whole.contains("--- standard error ---\nwarning"));
}

/// A line long enough to be cut at a pane's edge is kept whole in the file,
/// even when no line is left out, and the note says where.
#[test]
fn a_long_line_is_kept_whole_even_when_nothing_is_left_out() {
    let long = "x".repeat(LONG_LINE + 1);
    let composed = command(0, &format!("{long}\n"), "");
    let left_out = composed
        .left_out
        .clone()
        .expect("a long line is kept whole");
    assert!(left_out.whole.contains(&long));
    let view = composed.finished(Some(Path::new("/s/shown-0002.txt")));
    assert_eq!(
        spelled(&view).first().map(String::as_str),
        Some("... all of it: /s/shown-0002.txt")
    );
}

/// A file of forty lines whose twentieth changes.
fn forty_lines(changed: Option<&str>) -> String {
    (1..=40)
        .map(|n| match changed {
            Some(text) if n == 20 => format!("{text}\n"),
            _ => format!("line {n}\n"),
        })
        .collect()
}

/// **An edit in the middle of a file is shown as its lines: the removed one
/// marked `-`, the added one marked `+`, two lines on each side, and each
/// line's number.**
///
/// The mutant: numbering from the start of the change rather than the file,
/// which prints the rows with the wrong numbers.
#[test]
fn an_edit_in_the_middle_of_a_file_is_shown_with_its_numbers_and_context() {
    let before = Before::Text(forty_lines(None));
    let after = Before::Text(forty_lines(Some("the new line 20")));
    let view = change("src/lib.rs", "changed", &before, &after).finished(None);

    assert_eq!(view.summary, "changed src/lib.rs · 1 line removed, 1 added");
    assert_eq!(
        spelled(&view),
        vec![
            "18   | line 18",
            "19   | line 19",
            "20 - | line 20",
            "20 + | the new line 20",
            "21   | line 21",
            "22   | line 22",
        ]
    );
}

/// Two changes far apart are two parts with a gap between them, and a change
/// longer than the rows a view shows says how many rows are not shown.
#[test]
fn changes_far_apart_are_parted_and_a_long_change_says_what_is_not_shown() {
    let before: String = (1..=60).map(|n| format!("line {n}\n")).collect();
    let after: String = (1..=60)
        .map(|n| match n {
            5 => String::from("changed 5\n"),
            50 => String::from("changed 50\n"),
            n => format!("line {n}\n"),
        })
        .collect();
    let composed = change(
        "a.txt",
        "changed",
        &Before::Text(before),
        &Before::Text(after),
    );
    let every = composed
        .left_out
        .clone()
        .expect("two parts do not fit")
        .whole;
    assert!(
        every.contains("   ...\n"),
        "the two parts are not parted by a gap:\n{every}"
    );
    assert!(every.contains("50 + | changed 50"));
    let view = composed.finished(None);
    assert_eq!(view.rows.len(), CHANGE_ROWS + 1);
    assert_eq!(
        spelled(&view).last().map(String::as_str),
        Some("... 6 more rows of the change not shown")
    );
}

/// A new file shows its first lines and its length, and says how many more
/// lines it has.
#[test]
fn a_new_file_shows_its_first_lines_and_its_length() {
    let text: String = (1..=40).map(|n| format!("note {n}\n")).collect();
    let view = change(
        "notes.txt",
        "replaced",
        &Before::Absent,
        &Before::Text(text.clone()),
    )
    .finished(Some(Path::new("/s/shown-0003.txt")));
    assert_eq!(
        view.summary,
        format!("created notes.txt · 40 lines, {} bytes", text.len())
    );
    let rows = spelled(&view);
    assert_eq!(rows.first().map(String::as_str), Some("1 + | note 1"));
    assert_eq!(rows.len(), NEW_FILE_ROWS + 1);
    assert_eq!(
        rows.last().map(String::as_str),
        Some("... 35 more lines not shown · all of it: /s/shown-0003.txt")
    );
}

/// A write of the same text, and a file whose text cannot be shown, each say
/// so in one line.
#[test]
fn a_change_with_nothing_to_show_says_why_in_one_line() {
    let same = Before::Text(String::from("a\nb\n"));
    let unchanged = change("a.txt", "replaced", &same, &same).finished(None);
    assert_eq!(unchanged.summary, "replaced a.txt · no line changed");
    assert!(unchanged.rows.is_empty());

    let big = change(
        "big.log",
        "replaced",
        &Before::NotShown("the file is larger than 1 MiB"),
        &same,
    )
    .finished(None);
    assert_eq!(
        big.summary,
        "replaced big.log · the change is not shown: the file is larger than 1 MiB"
    );
}

/// **The diff is a true diff**: read in order, its kept and removed lines are
/// the old text and its kept and added lines are the new, for a set of edits
/// shaped the ways edits are.
///
/// The mutant: an insert numbered by the old text, which reads back as a
/// different new text.
#[test]
fn the_diff_rebuilds_both_texts_exactly() {
    let base: Vec<String> = (0..50).map(|n| format!("l{}", n % 7)).collect();
    let mut cases: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    for at in [0_usize, 1, 10, 25, 48, 49] {
        let mut removed = base.clone();
        removed.remove(at);
        cases.push((base.clone(), removed.clone()));
        cases.push((removed, base.clone()));
        let mut replaced = base.clone();
        replaced[at] = String::from("new");
        replaced.insert(at, String::from("inserted"));
        cases.push((base.clone(), replaced));
    }
    cases.push((Vec::new(), base.clone()));
    cases.push((base.clone(), Vec::new()));
    cases.push((base.clone(), base.iter().rev().cloned().collect()));

    for (old, new) in cases {
        let old: Vec<&str> = old.iter().map(String::as_str).collect();
        let new: Vec<&str> = new.iter().map(String::as_str).collect();
        let ops = diff(&old, &new);
        let mut rebuilt_old = Vec::new();
        let mut rebuilt_new = Vec::new();
        for op in &ops {
            match *op {
                Op::Equal(i, j) => {
                    assert_eq!(old[i], new[j], "a kept line differs between the two texts");
                    rebuilt_old.push(old[i]);
                    rebuilt_new.push(new[j]);
                }
                Op::Delete(i) => rebuilt_old.push(old[i]),
                Op::Insert(j) => rebuilt_new.push(new[j]),
            }
        }
        assert_eq!(
            rebuilt_old, old,
            "the diff does not read back as the old text"
        );
        assert_eq!(
            rebuilt_new, new,
            "the diff does not read back as the new text"
        );
    }
}

/// One line changed in a long file is one line removed and one added, not
/// the rest of the file.
#[test]
fn one_changed_line_in_a_long_file_is_one_line_each_way() {
    let old: Vec<String> = (0..5_000).map(|n| format!("line {n}")).collect();
    let mut new = old.clone();
    new[2_500] = String::from("changed");
    let old: Vec<&str> = old.iter().map(String::as_str).collect();
    let new: Vec<&str> = new.iter().map(String::as_str).collect();
    let ops = diff(&old, &new);
    assert_eq!(
        ops.iter().filter(|op| !matches!(op, Op::Equal(..))).count(),
        2
    );
}

/// **What a read says is what the reader wrote**: the summary is read off the
/// answer `tools::reading` gives the model, so this pins the two together.
/// A change to the answer's first two lines reddens here.
#[test]
fn a_reads_summary_names_the_lines_that_came_back() {
    let directory = std::env::temp_dir().join(format!(
        "trv-read-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).expect("a scratch directory");
    let path = directory.join("f.txt");
    std::fs::write(
        &path,
        (1..=30).map(|n| format!("{n}\n")).collect::<String>(),
    )
    .expect("a scratch file");
    let budget = crate::tools::OutputBudget::new(32 * 1024).expect("a budget");
    let ranged = crate::tools::reading::read(
        &path,
        crate::tools::reading::Lines {
            start: Some(5),
            count: Some(10),
        },
        budget,
    );
    let one = crate::tools::reading::read(
        &path,
        crate::tools::reading::Lines {
            start: Some(7),
            count: Some(1),
        },
        budget,
    );
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(
        read("f.txt", &ranged.stdout).view.summary,
        "read f.txt · lines 5 to 14 of 30"
    );
    assert_eq!(
        read("f.txt", &one.stdout).view.summary,
        "read f.txt · line 7 of 30"
    );
}

/// A search says how many lines matched in how many files; a fetch its
/// status, its size and its page's title; a listing how many entries.
#[test]
fn a_search_a_fetch_and_a_listing_are_one_line_each() {
    let found = "Searched . for \"retry\" in any case: 3 lines in 2 files.\n\nsrc/a.rs\n  3: let \
                 retry = 1;\n  9: retry += 1;\n\nsrc/b.rs\n  1: // retry\n\nSearched 2 files.\n";
    assert_eq!(
        search(".", "retry", found).view.summary,
        "searched . for \"retry\" · 3 lines found in 2 files"
    );
    let absent =
        "Searched src for \"absent\" in any case: no line holds it.\n\nSearched 4 files.\n";
    assert_eq!(
        search("src", "absent", absent).view.summary,
        "searched src for \"absent\" · nothing found"
    );
    let words = format!(
        "Searched . for \"retry logic\" in any case: no line holds it.\n\n{} \"retry logic\":\n\
         src/a.rs\n  4: function retry_with_backoff — pub fn retry_with_backoff() {{}}\n\n\
         Searched 2 files.\n",
        crate::tools::searching::DECLARATIONS_HEADING
    );
    assert_eq!(
        search(".", "retry logic", &words).view.summary,
        "searched . for \"retry logic\" · 1 declaration found"
    );

    let page = "web.fetch https://example.com/ — 200 — content-type: text/html\n\
                <html><head><TITLE>\n  Example   Domain\n</title></head><body>hi</body></html>";
    let body = page.split_once('\n').map_or("", |(_, body)| body);
    assert_eq!(
        fetch("https://example.com/", page).view.summary,
        format!(
            "fetched https://example.com/ · status 200 · {} bytes · title \"Example Domain\"",
            body.len()
        )
    );

    assert_eq!(
        list("pkg", "a.py\nb.py\n").view.summary,
        "listed pkg · 2 entries"
    );
}

/// A call that failed shows its first line as the summary and the rest as
/// rows.
#[test]
fn a_failed_call_says_what_it_said() {
    let view =
        failed("the string to replace does not occur in a.txt.\nnearest:\n 3│x\n").finished(None);
    assert_eq!(
        view.summary,
        "failed: the string to replace does not occur in a.txt."
    );
    assert_eq!(spelled(&view), vec!["err | nearest:", "err |  3│x"]);
}
