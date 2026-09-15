// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Drives the composer from outside the crate, through the door a caller uses.
//!
//! Every other check on the composer lives inside `zaru-tui` and reaches its
//! subject directly. That proves the mechanism and says nothing about whether
//! the mechanism can be reached: a capability whose only callers are unit
//! tests is a capability nobody has been shown able to use, and the missing
//! piece is invisible to a green suite because there is no mutant for a
//! declaration that was never made public.
//!
//! So this file implements the fast tier using only what `zaru-tui` exports,
//! types into the composer, lets the debounce elapse in a clock it controls,
//! hands back a response, and reads the frame out of `TestBackend`. Nothing
//! here opens a terminal and nothing here touches a network — which is also
//! ADR-0005 D3's "the fast tier never touches the network" stated as something
//! a stranger can reproduce.
//!
//! It prints the frame it read. Run it with
//! `cargo test -p zaru-tui --test composer_from_outside -- --nocapture` to see
//! what the composer actually paints.

use core::time::Duration;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tui_textarea::{Input, Key};
use zaru_tui::composer::{
    Composer, Entries, Entry, EntryKind, KEYWORD_ONLY, Scope, SearchResponse,
};
use zaru_tui::shell::{CommandVocabulary, Namespace};

/// A trie a stranger could write, holding two entities.
struct Notes;

impl Entries for Notes {
    fn matches(&self, prefix: &str, limit: usize) -> Vec<Entry> {
        [
            Entry::new(
                "zaru",
                "architecture/bounded-contexts",
                "Bóunded Contexts ✦",
                EntryKind::Page,
            ),
            Entry::new("zaru", "atoms/membrane", "Mémbrane ✦", EntryKind::Atom),
        ]
        .into_iter()
        .filter(|entry| entry.title.to_lowercase().contains(&prefix.to_lowercase()))
        .take(limit)
        .collect()
    }
}

/// A vocabulary a stranger could write, holding two namespaces that share a
/// prefix and one that shares none.
///
/// Two sharing a prefix is the only shape that can tell "completes a unique
/// prefix" from "completes the first match", and it is why this is not a
/// one-row fixture.
struct Commands;

const STRANGER: [(&str, &str); 3] = [
    ("/session", "resume, list, remove"),
    ("/settings", "nothing this harness has"),
    ("/runtime", "tier and membrane"),
];

impl CommandVocabulary for Commands {
    fn namespaces(&self) -> Vec<Namespace> {
        STRANGER
            .into_iter()
            .map(|(slash, governs)| Namespace {
                slash,
                governs,
                built: true,
                verbs: &[],
            })
            .collect()
    }

    fn nearest(&self, _offered: &str) -> Option<&'static str> {
        None
    }

    fn nearest_verb(&self, _slash: &str, _offered: &str) -> Option<&'static str> {
        None
    }
}

#[test]
fn a_caller_outside_this_crate_can_drive_the_composer_to_a_frame() {
    let notes = Notes;
    let mut composer = Composer::new();
    composer.set_scope(Scope::Workspace);

    // Type "mém". Every keystroke lands at the same instant in this test's own
    // clock, so nothing here is measured against the machine.
    let typed_at = Duration::from_secs(7);
    for ch in "mém".chars() {
        composer.key(
            Input {
                key: Key::Char(ch),
                ctrl: false,
                alt: false,
                shift: false,
            },
            typed_at,
            &notes,
            &Commands,
        );
    }

    assert!(
        composer
            .step(typed_at + Duration::from_millis(249))
            .is_none(),
        "the debounce has not elapsed 249 ms after the last keystroke"
    );
    let request = composer
        .step(typed_at + Duration::from_millis(250))
        .expect("250 ms after the last keystroke the request is due");
    assert_eq!(request.query(), "mém");
    assert_eq!(request.scope(), Scope::Workspace);

    composer.deliver(SearchResponse {
        results: vec![Entry::new(
            "zaru",
            "operations/testing",
            "Tésting ✦",
            EntryKind::Page,
        )],
        semantic_available: false,
    });

    let mut terminal = Terminal::new(TestBackend::new(32, 6)).expect("test terminal");
    terminal
        .draw(|frame| composer.render(frame, frame.area()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();

    println!("--- the frame the composer painted, 32x6 ---");
    for row in &rows {
        println!("|{row}|");
    }
    println!("--- composer.height() = {} ---", composer.height());

    // A capture is a fact about a viewport.
    assert_eq!(
        rows[0].trim_end(),
        "mém",
        "the input row should hold what a stranger typed; the frame was {rows:?}"
    );
    assert_eq!(
        rows[1].trim_end(),
        "Mémbrane ✦",
        "the trie's only match for \"mém\" should be the first strip row; the frame was {rows:?}"
    );
    assert_eq!(
        rows[2].trim_end(),
        "Tésting ✦",
        "the server's only result should follow it; the frame was {rows:?}"
    );
    assert_eq!(
        rows[3].trim_end(),
        KEYWORD_ONLY,
        "semantic ranking was unavailable, so D8 has the strip say so; the frame was {rows:?}"
    );

    // A count is a fact about the mechanism.
    assert_eq!(
        composer.height(),
        4,
        "one input row, two entries and D8's line is four rows"
    );
    assert!(
        composer.step(typed_at + Duration::from_secs(60)).is_none(),
        "one query emits one request, however long a stranger keeps stepping"
    );
}
