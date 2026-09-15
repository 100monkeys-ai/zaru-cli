// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a CommonMark answer paints as, asserted from a rendered buffer.
//!
//! **Every check here reads a `TestBackend` rather than the renderer's return
//! value**, at a wide pane and a narrow one, because the claim being made is
//! about what a person sees and a claim about a `Vec<Row>` is a claim about a
//! function agreeing with itself — [Verification lessons] §12.
//!
//! The two widths are 100 and 40, which is the pair the 2026-09-15 ruling
//! names and the pair the release binary was driven at over a pseudo-terminal
//! when row 4 was re-measured.
//!
//! [Verification lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons

use crate::shell::fixtures::{StagedTranscript, styled_cells};
use crate::shell::port::{Line, Palette, Register};
use crate::shell::{COMPOSER_ROWS, Shell, Status};
use ratatui::style::{Color, Modifier};

/// The wide pane, from the ruling.
const WIDE: u16 = 100;

/// The narrow pane, from the ruling. Tall enough that a whole answer fits, so
/// a check about rendering is never accidentally a check about the tail.
const NARROW: u16 = 40;

/// Tall enough for the answers below plus the status row and the composer.
const TALL: u16 = 40;

/// The answer the release binary was measured on, character for character.
///
/// **Taken from `transcript.jsonl` rather than retyped**: the `Record::
/// Conversation` `zaru` half of session `01M2H6QEGBW433ESK0H5FQF9HN`, written
/// by `gemini-3.6-flash` on 2026-09-14 against the release binary at
/// `a8eedf7`. Using the real answer is what stops these checks asserting
/// something about a document a check author wrote to be easy to render.
const MEASURED: &str = "## Greetings\n\nThis paragraph has **bold text** and *emphasis* and inline code `zaru run`.\n\n- Item 1\n- Item 2\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\n[Example](https://example.com)";

fn shell() -> Shell {
    Shell::open(Status::new("bare", "01JQZX8N3K4M5P6R7S8T9V0W1X"))
}

/// A shell whose pane holds one answer, and nothing else.
fn answering(answer: &str) -> Shell {
    let mut shell = shell();
    shell.refresh(&StagedTranscript(vec![Line::answer(
        Register::Plain,
        "",
        answer,
    )]));
    shell
}

/// Every pane row of a rendered frame, trimmed, at `width`.
fn rows(shell: &Shell, width: u16) -> Vec<String> {
    let painted = styled_cells(shell, width, TALL, Palette::Coloured);
    let last = painted.len() - usize::from(COMPOSER_ROWS);
    painted[1..last]
        .iter()
        .map(|row| {
            row.iter()
                .map(|(symbol, _, _)| symbol.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

/// Every cell of the pane that carries a symbol other than a space, as
/// `(symbol, colour, modifier)`.
fn inked(shell: &Shell, width: u16) -> Vec<(String, Color, Modifier)> {
    let painted = styled_cells(shell, width, TALL, Palette::Coloured);
    let last = painted.len() - usize::from(COMPOSER_ROWS);
    painted[1..last]
        .iter()
        .flat_map(|row| row.iter().cloned())
        .filter(|(symbol, _, _)| symbol.trim() != "")
        .collect()
}

/// The modifier the cells spelling `needle` carry, at `width`.
///
/// Panics rather than returning an option: a check that cannot find its own
/// subject is asserting nothing, and [Verification lessons] §14 is the rule
/// that a check which can decline can pass vacuously.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/zaru/p/operations/verification-lessons
fn modifier_over(shell: &Shell, width: u16, needle: &str) -> Modifier {
    let painted = styled_cells(shell, width, TALL, Palette::Coloured);
    for row in &painted {
        let text: String = row.iter().map(|(symbol, _, _)| symbol.as_str()).collect();
        if let Some(at) = text.find(needle) {
            let columns: Vec<Modifier> = row[at..at + needle.chars().count()]
                .iter()
                .map(|(_, _, modifier)| *modifier)
                .collect();
            assert!(
                columns.windows(2).all(|pair| pair[0] == pair[1]),
                "the cells of {needle:?} do not all carry one modifier: {columns:?}"
            );
            return columns[0];
        }
    }
    panic!("no row of the pane at {width} columns holds {needle:?}:\n{painted:#?}");
}

/// Every width a check here asserts at, so a check cannot quietly test one.
const BOTH: [u16; 2] = [WIDE, NARROW];

// ---------------------------------------------------------------------------
// The constructs
// ---------------------------------------------------------------------------

/// # The mutants
///
/// Render the heading verbatim — the `#` returns and this reddens on the
/// second assertion. Paint it bold **and** keep the `#` — this reddens on the
/// first, which is why the two are asserted apart rather than as one frame
/// comparison.
#[test]
fn a_heading_paints_its_words_without_its_hashes_and_in_bold() {
    let shell = answering(MEASURED);
    for width in BOTH {
        let painted = rows(&shell, width);
        assert!(
            painted.iter().any(|row| row.contains("Greetings")),
            "the heading's words are not on the pane at {width}: {painted:#?}"
        );
        assert!(
            !painted.iter().any(|row| row.contains('#')),
            "a hash reached the pane at {width}: {painted:#?}"
        );
        assert_eq!(
            modifier_over(&shell, width, "Greetings"),
            super::HEADING,
            "the heading's words do not carry HEADING at {width}"
        );
    }
}

/// # The mutants
///
/// Treat `Strong` as text — the delimiters return. Leave the delimiters in
/// while still applying the modifier — the second assertion reddens.
#[test]
fn strong_and_emphasis_are_modifiers_and_their_delimiters_are_gone() {
    let shell = answering(MEASURED);
    for width in BOTH {
        assert_eq!(
            modifier_over(&shell, width, "bold text"),
            super::STRONG,
            "a strong span does not carry STRONG at {width}"
        );
        assert_eq!(
            modifier_over(&shell, width, "emphasis"),
            super::EMPHASIS,
            "an emphasis span does not carry EMPHASIS at {width}"
        );
        let painted = rows(&shell, width);
        assert!(
            !painted.iter().any(|row| row.contains('*')),
            "an asterisk reached the pane at {width}: {painted:#?}"
        );
    }
}

/// # The mutants
///
/// Drop the `Code` event — `zaru run` vanishes. Keep the back-ticks — the
/// third assertion reddens.
#[test]
fn inline_code_loses_its_backticks_and_carries_its_own_modifier() {
    let shell = answering(MEASURED);
    for width in BOTH {
        assert_eq!(
            modifier_over(&shell, width, "zaru run"),
            super::INLINE_CODE,
            "inline code does not carry INLINE_CODE at {width}"
        );
        let painted = rows(&shell, width);
        assert!(
            !painted.iter().any(|row| row.contains('`')),
            "a back-tick reached the pane at {width}: {painted:#?}"
        );
    }
}

/// # The mutants
///
/// Paint the fence — the second assertion reddens. Indent by the info
/// string's width instead of `BLOCK_INDENT` — the third reddens, because
/// `rust` is four columns and `BLOCK_INDENT` is two.
#[test]
fn a_fenced_block_is_indented_under_the_glyph_column_and_carries_no_fence() {
    let shell = answering(MEASURED);
    for width in BOTH {
        let painted = rows(&shell, width);
        let code = painted
            .iter()
            .find(|row| row.contains("fn main()"))
            .unwrap_or_else(|| panic!("no row holds the code at {width}: {painted:#?}"));
        assert!(
            !painted.iter().any(|row| row.contains("```")),
            "a fence reached the pane at {width}: {painted:#?}"
        );
        assert!(
            !painted.iter().any(|row| row.trim() == "rust"),
            "the info string reached the pane at {width}: {painted:#?}"
        );
        let glyph = crate::shell::wrap::columns(Register::Plain.glyph()) + 1;
        assert_eq!(
            code.len() - code.trim_start().len(),
            glyph + super::BLOCK_INDENT,
            "the code is not indented BLOCK_INDENT past the glyph column at {width}: {code:?}"
        );
    }
}

/// [ADR-0028]'s amendment: a fenced block is **uncoloured and unmodified**,
/// which is what "no highlighting" means as a check rather than as a promise.
///
/// # The mutant
///
/// Any highlighter at all, and any modifier applied to a code block's text.
///
/// [ADR-0028]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative-updates
#[test]
fn a_fenced_block_carries_no_modifier_and_no_colour_anywhere() {
    let shell = answering("```rust\nfn main() {\n    println!(\"hello\");\n}\n```");
    for width in BOTH {
        for (symbol, colour, modifier) in inked(&shell, width) {
            assert_eq!(
                modifier,
                Modifier::empty(),
                "a code cell {symbol:?} carries a modifier at {width}"
            );
            assert_eq!(
                colour,
                Color::Reset,
                "a code cell {symbol:?} carries a colour at {width}"
            );
        }
    }
}

/// # The mutant
///
/// Put the bullet in the text rather than in the lead — the item's words then
/// start two columns right of where a continuation row would, and the second
/// assertion reddens.
#[test]
fn a_list_marker_is_the_lines_glyph_column() {
    let shell = answering("- Item 1\n- Item 2");
    for width in BOTH {
        let painted = rows(&shell, width);
        let item = painted
            .iter()
            .find(|row| row.contains("Item 1"))
            .unwrap_or_else(|| panic!("no row holds the item at {width}: {painted:#?}"));
        assert!(
            item.contains(super::BULLET),
            "the bullet is not on the item's row at {width}: {item:?}"
        );
        let glyph = crate::shell::wrap::columns(Register::Plain.glyph()) + 1;
        assert_eq!(
            item.find("Item 1"),
            Some(glyph + super::BULLET.len() + 1),
            "the item's words are not one space past the marker at {width}: {item:?}"
        );
    }
    // The rows a `Line` produces carry the marker in `lead`, not in `text`.
    let line = Line::answer(Register::Plain, "", "- Item 1");
    let row = &line.rows(WIDE)[0];
    assert!(
        row.lead.contains(super::BULLET) && !row.text.contains(super::BULLET),
        "the marker is not in the lead: {row:?}"
    );
}

/// The mutant the 2026-09-15 ruling names by name: render a link's text alone.
///
/// A destination a reader cannot see is the one thing this rendering must not
/// produce, on a surface whose whole thesis is that it shows its work.
#[test]
fn a_link_paints_its_text_then_its_url_and_hides_nothing() {
    let shell = answering("[Example](https://example.com)");
    for width in BOTH {
        let painted = rows(&shell, width).join(" ");
        assert!(
            painted.contains("Example"),
            "the link's text is not on the pane at {width}: {painted:?}"
        );
        assert!(
            painted.contains("https://example.com"),
            "the link's destination is not on the pane at {width}: {painted:?}"
        );
        assert!(
            painted.contains(super::URL_OPEN.trim_start()) && painted.contains(super::URL_CLOSE),
            "the destination is not parenthesised at {width}: {painted:?}"
        );
        assert!(
            !painted.contains('['),
            "a bracket reached the pane at {width}: {painted:?}"
        );
    }
}

/// An autolink's text **is** its destination, so the parenthetical is omitted.
///
/// # Why this is not a second rule
///
/// The parenthetical exists so that nothing is hidden. Where the text already
/// is the destination there is nothing left to reveal, and printing it twice
/// would be the rendering asserting a difference that is not there.
#[test]
fn an_autolink_is_not_printed_twice() {
    let shell = answering("<https://example.com>");
    let painted = rows(&shell, WIDE).join(" ");
    assert_eq!(
        painted.matches("https://example.com").count(),
        1,
        "the autolink is printed more than once: {painted:?}"
    );
}

/// # The mutant, and the one that was tried first and survived
///
/// **The discriminating mutant is deleting the catch-all arm** that paints any
/// construct this renderer does not handle as its own source text. Without it
/// a table's rows vanish rather than degrading.
///
/// `Options::ENABLE_TABLES` was tried first and **survived**, which is a
/// finding rather than a pass: the catch-all arm paints a table's source text
/// whether the parser produced table events or paragraph text, so the
/// observable property holds under both settings. That is a stronger renderer
/// than `Options::empty()` alone would give — an unhandled construct degrades
/// to its own characters instead of disappearing — and it is why this check
/// cannot be the thing that pins the parser's options. `the_parser_takes_no_/// gfm_extension` below pins that separately, at the source.
#[test]
fn a_table_and_an_image_reach_the_pane_as_their_own_source_text() {
    let shell = answering("| a | b |\n| --- | --- |\n| 1 | 2 |\n\n![alt](./p.png)");
    for width in BOTH {
        let painted = rows(&shell, width).join(" ");
        assert!(
            painted.contains("| a | b |"),
            "the table's own source text is not on the pane at {width}: {painted:?}"
        );
        assert!(
            painted.contains("![alt](./p.png)"),
            "the image's own source text is not on the pane at {width}: {painted:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

/// A streamed answer paints the same rows when it completes as a whole answer
/// would.
///
/// # Why this is the load-bearing check of the whole change
///
/// A CommonMark parse of a *prefix* of a document is not a prefix of the parse
/// of the document — `**bo` is literal where `**bold**` is strong. So a
/// renderer that parsed only the last delta, or only on completion, would
/// paint a frame at the end of the turn that differs from the frame a resumed
/// session paints of the same answer. That is the drift [ADR-0010] D2's
/// "re-rendering it reproduces what the user saw" forbids.
///
/// The deltas are split at deliberately awkward byte offsets — inside a
/// delimiter run, inside a fence, and inside a link's destination — rather
/// than at whitespace, because a split at a token boundary is the case that
/// would pass under the mutant.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
#[test]
fn a_streamed_answer_paints_the_same_rows_when_it_completes_as_a_whole_answer_would() {
    for width in BOTH {
        let whole = rows(&answering(MEASURED), width);

        let mut streamed = shell();
        let bytes = MEASURED.as_bytes();
        let mut at = 0;
        // Seven is coprime with nothing in the document, so the splits land
        // inside `**`, inside the fence and inside the destination.
        while at < bytes.len() {
            let mut end = (at + 7).min(bytes.len());
            while !MEASURED.is_char_boundary(end) {
                end += 1;
            }
            streamed.stream_delta(&MEASURED[at..end]);
            at = end;
        }
        // **Asserted before `clear_streaming`, and that is what makes this
        // check about streaming at all.** A version that compared only the
        // frame *after* the turn ends compares two renderings of the same
        // `Line::answer` and cannot tell "parsed on every beat" from "parsed
        // once at the end" — measured, by watching that mutant survive on
        // 2026-09-15 and strengthening the check rather than recording a pass.
        assert_eq!(
            rows(&streamed, width),
            whole,
            "the last streamed frame differs from a whole answer's at {width}"
        );

        // And the turn ends: the provisional line goes and the answer arrives
        // as the line the pane keeps, exactly as the driver does it. The frame
        // must not move.
        streamed.clear_streaming();
        streamed.refresh(&StagedTranscript(vec![Line::answer(
            Register::Plain,
            "",
            MEASURED,
        )]));

        assert_eq!(
            rows(&streamed, width),
            whole,
            "a streamed answer's completed frame differs from a whole answer's at {width}"
        );
    }
}

/// A fence that has opened and not closed is painted as a block while it
/// streams, not as literal text that reflows when the closer arrives.
///
/// # The mutant
///
/// Leave an unterminated block as literal text until it closes. The pane then
/// shows a fence and an un-indented `fn main() {` for as long as the model
/// takes to finish the block, and flips when it does — which is the flicker
/// this asserts against.
#[test]
fn an_unclosed_fence_paints_as_a_block_while_it_streams() {
    let mut shell = shell();
    shell.stream_delta("here:\n\n```rust\nfn main() {\n");
    let painted = rows(&shell, WIDE);
    assert!(
        !painted.iter().any(|row| row.contains("```")),
        "an unclosed fence reached the pane: {painted:#?}"
    );
    let code = painted
        .iter()
        .find(|row| row.contains("fn main()"))
        .unwrap_or_else(|| panic!("the open block's code is not on the pane: {painted:#?}"));
    let glyph = crate::shell::wrap::columns(Register::Plain.glyph()) + 1;
    assert_eq!(
        code.len() - code.trim_start().len(),
        glyph + super::BLOCK_INDENT,
        "an open block's code is not indented: {code:?}"
    );
}

// ---------------------------------------------------------------------------
// What is not parsed
// ---------------------------------------------------------------------------

/// Only a line built by [`Line::answer`] is parsed; every other line keeps
/// every character it was given.
///
/// # The mutant
///
/// Dispatch on `Register::Plain` instead of on [`crate::shell::port::Prose`].
/// [ADR-0011] D2's not-a-sandbox notice, [ADR-0016] D2's remedy lines,
/// `config explain`'s layer rows and [ADR-0012] D7's usage line are all
/// `Plain` and all carry CommonMark delimiters as ordinary characters, so the
/// mutant eats them.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [ADR-0012]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0012-provider-abstraction
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn only_an_answer_is_parsed_as_commonmark() {
    // Each is a real product line's shape: a remedy naming a command in
    // back-ticks, a manifest path, a call line, and a status figure.
    let staged = [
        (
            Register::Plain,
            "run `zaru notes tokens` or `zaru providers keys` to see what this machine holds",
        ),
        (
            Register::Plain,
            "no validators are declared, so the iteration loop cannot run \u{b7} declare one in `./zaru.toml`",
        ),
        (Register::Call, "fs.write /tmp/a_*_b/note.txt"),
        (
            Register::Failed,
            "## not a heading, an error that starts with hashes",
        ),
    ];
    let mut shell = shell();
    shell.refresh(&StagedTranscript(
        staged
            .iter()
            .map(|(register, text)| Line::new(*register, *text))
            .collect(),
    ));
    for width in BOTH {
        let painted = rows(&shell, width).join("\n");
        for (_, text) in &staged {
            for fragment in text.split(' ') {
                assert!(
                    painted.contains(fragment),
                    "{fragment:?} did not reach the pane at {width}; \
                     a verbatim line was parsed:\n{painted}"
                );
            }
        }
    }
}

/// [`Line::answer`] is the only constructor of [`Prose::CommonMark`] in the
/// product tree.
///
/// # Why a source walk rather than a comment
///
/// A fourth construction site is how a line nobody meant to parse gets parsed,
/// and nothing about the type stops one being written. This is the shape
/// `zaru-cli`'s `corpus_one_place_in_the_terminal_renders_a_classified_failure`
/// already uses.
///
/// [`Prose::CommonMark`]: crate::shell::port::Prose::CommonMark
#[test]
fn corpus_one_place_constructs_prose_commonmark() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sites: Vec<String> = Vec::new();
    let mut walked = 0_usize;
    let mut stack = vec![root];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).expect("the crate's own source is readable") {
            let entry = entry.expect("a directory entry").path();
            if entry.is_dir() {
                stack.push(entry);
                continue;
            }
            if entry.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            let body = std::fs::read_to_string(&entry).expect("a source file is UTF-8");
            walked += 1;
            // `tests.rs` files stage lines on purpose; the claim is about the
            // product tree, exactly as the `zaru-cli` walk this follows.
            if entry.file_name().is_some_and(|name| name == "tests.rs") {
                continue;
            }
            let here = entry.file_name().is_some_and(|name| name == "port.rs");
            for (number, line) in body.lines().enumerate() {
                // **Two properties, and the second is the one that holds.**
                // Outside `port.rs` the variant may not be *named* at all, so
                // no other module can construct it however it spells the
                // construction; inside it, the field initialisation appears
                // once. A walk that counted `Prose::CommonMark {` alone would
                // count `Line::rows`' own match arm, which is a pattern rather
                // than a construction -- caught by watching this red.
                // A doc comment naming the variant is a link, not a
                // construction -- caught by watching this red against
                // `markdown.rs`'s own `[`...Prose::CommonMark`]` reference.
                let comment = line.trim_start().starts_with("//");
                if line.contains("Prose::CommonMark") && !here && !comment {
                    sites.push(format!("{}:{} (names it)", entry.display(), number + 1));
                }
                if here && line.contains("prose: Prose::CommonMark {") {
                    sites.push(format!("{}:{}", entry.display(), number + 1));
                }
            }
        }
    }
    assert!(
        walked > 5,
        "the walk read {walked} file(s), so it could not have found anything"
    );
    assert_eq!(
        sites.len(),
        1,
        "Prose::CommonMark is constructed in {} place(s), not one: {sites:#?}",
        sites.len()
    );
    assert!(
        sites[0].contains("port.rs"),
        "the one construction is not in port.rs: {sites:#?}"
    );
}

// ---------------------------------------------------------------------------
// The security corpus, on the answer path
// ---------------------------------------------------------------------------

/// A held value inside an **answer** reaches the buffer whole, at both widths
/// and across a wrap boundary.
///
/// # Why this is new rather than covered
///
/// `corpus_a_held_secret_split_across_a_wrap_still_reaches_the_buffer_whole`
/// is on a [`Register::Call`] line, which is [`Prose::Verbatim`] and is not
/// parsed. **The answer is the one path into the pane with no redactor on
/// it** — [ADR-0010]'s seventh-producer amendment says so in as many words —
/// so it is the path a parser can consume characters on, and it had no check.
///
/// The accepting sibling is the first arm: a value carrying no CommonMark
/// delimiter is whole, which is what says the instrument works before the
/// second arm claims anything.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
/// [`Prose::Verbatim`]: crate::shell::port::Prose::Verbatim
#[test]
fn corpus_a_held_secret_in_an_answer_reaches_the_buffer_whole() {
    let value = crate::shell::fixtures::SECRET_NONCE;
    let shell = answering(&format!("the key it used was {value} and nothing else"));
    for width in [WIDE, NARROW, 24, 14] {
        let joined: String = rows(&shell, width)
            .iter()
            .map(|row| row.trim_start().to_owned())
            .collect();
        assert!(
            joined.contains(value),
            "the held value did not reach the buffer whole at {width}: {joined:?}"
        );
    }
}

/// A held value carrying a CommonMark delimiter run, measured rather than
/// predicted.
///
/// # What this check is for
///
/// The leg-1 survey recorded that a value containing `**` is the case that
/// decides whether the renderer can consume a held value's characters, and
/// that it would be **built as a check and its result recorded, not
/// predicted**. This is that check, and what it records is the answer:
/// **a matched pair of delimiters inside one value is consumed**, exactly as
/// it would be anywhere else in the answer, while `SECRET_NONCE`'s own
/// underscores survive **by the specification rather than by luck** — CommonMark
/// does not open emphasis on an intraword `_`.
///
/// So the honest statement is asserted rather than a comfortable one: the
/// rendering is faithful to the markup, and a value that *is* markup is
/// rendered as markup. The cost is named on the [ADR-0010] amendment, and the
/// mitigation is that the file keeps every byte.
///
/// [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript-updates-2
#[test]
fn corpus_a_held_value_that_is_itself_markup_is_rendered_as_markup() {
    // The accepting sibling: intraword underscores are not emphasis, so this
    // value is whole.
    let underscored = crate::shell::fixtures::SECRET_NONCE;
    assert!(
        underscored.contains('_'),
        "the fixture lost its underscores"
    );
    let whole = answering(&format!("key {underscored} end"));
    assert!(
        rows(&whole, WIDE).join("").contains(underscored),
        "an underscored value was consumed, which CommonMark does not do"
    );

    // The recorded case: a matched `**` pair inside a value is emphasis, and
    // its delimiters are consumed.
    let starred = "nn_live_**7a41c3e9**";
    let shell = answering(&format!("key {starred} end"));
    let painted = rows(&shell, WIDE).join("");
    assert!(
        !painted.contains(starred),
        "a matched delimiter pair survived, so this check records the wrong \
         answer and the amendment's stated cost is wrong: {painted:?}"
    );
    assert!(
        painted.contains("nn_live_7a41c3e9"),
        "the value's own characters did not reach the buffer: {painted:?}"
    );
}

// ---------------------------------------------------------------------------
// What did not change
// ---------------------------------------------------------------------------

/// A verbatim line's rows are byte-identical to what they were, and its rows
/// carry no modifier.
///
/// # The mutant
///
/// Parse every line. Every assertion above about a verbatim line reddens, and
/// so does this one, which is the cheapest statement of the whole property.
#[test]
fn a_verbatim_line_carries_no_emphasis_at_all() {
    let line = Line::new(Register::Call, "fs.write /tmp/a/note.txt **not bold**");
    for width in BOTH {
        for row in line.rows(width) {
            assert!(
                row.emphasis.is_empty(),
                "a verbatim row carries emphasis: {row:?}"
            );
            assert!(
                row.joined().contains('*') || !row.text.contains("bold"),
                "a verbatim row lost its asterisks: {row:?}"
            );
        }
    }
}

/// The parser takes no GFM extension, pinned where the decision is.
///
/// # Why this is a source assertion rather than a rendered one
///
/// The renderer's catch-all arm paints an unhandled construct as its own
/// source text, so a table survives with tables enabled *and* with them
/// disabled — measured on 2026-09-15 by watching `ENABLE_TABLES` fail to
/// redden `a_table_and_an_image_reach_the_pane_as_their_own_source_text`. The
/// options are still a decision, recorded on [ADR-0028's amendments page], and
/// a decision nothing holds is a comment. This holds it.
///
/// [ADR-0028's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0028-execution-narrative-updates
#[test]
fn the_parser_takes_no_gfm_extension() {
    let body = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shell/markdown.rs"),
    )
    .expect("the renderer's own source is readable");
    let constructions: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("Parser::new_ext") && !line.starts_with("//"))
        .collect();
    assert_eq!(
        constructions.len(),
        1,
        "the parser is constructed in {} place(s), not one: {constructions:#?}",
        constructions.len()
    );
    assert!(
        constructions[0].contains("Options::empty()"),
        "the parser takes an option set other than none: {:?}",
        constructions[0]
    );
}
