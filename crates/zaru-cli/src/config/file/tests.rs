// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The one TOML reader's checks.
//!
//! Every check names the mutant that would make it redden, and every mutant
//! named here has been run: the failure sentence is quoted in the commit that
//! carries the check.

use crate::config::file::{CeilingRefused, FileRefused, Position, SizeCeiling, TomlFile};
use crate::config::value::{Table, Value};
use crate::credentials::fixtures::{ScratchRoot, ascii_core, personal_secret_nonce};

/// A ceiling large enough that no check here meets it by accident.
fn roomy() -> SizeCeiling {
    SizeCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

/// Write `body` into the scratch tree and read it back through the product.
fn staged(root: &ScratchRoot, name: &str, body: &[u8]) -> (std::path::PathBuf, TomlFile) {
    let path = root.base().join(name);
    std::fs::write(&path, body).expect("could not stage the file");
    let file = TomlFile::at(&path, roomy());
    (path, file)
}

/// Every TOML kind this value model has, converted, at every depth.
///
/// The mutant is dropping the recursive arm for a nested table, which reddens
/// with the nested key missing.
#[test]
fn a_toml_file_becomes_this_crates_own_value_model() {
    let root = ScratchRoot::new();
    let (_, file) = staged(
        &root,
        "kinds.toml",
        b"flag = true\nwhole = 3\nfree = \"text\"\nlist = [1, \"two\"]\n\n[nested]\ninner = \"deep\"\n\n[nested.deeper]\nleaf = false\n",
    );

    let document = file
        .read()
        .expect("the file is well-formed TOML")
        .expect("the file is there");

    assert_eq!(document.get("flag"), Some(&Value::Bool(true)));
    assert_eq!(document.get("whole"), Some(&Value::Integer(3)));
    assert_eq!(document.get("free"), Some(&Value::Text("text".to_owned())));
    assert_eq!(
        document.get("list"),
        Some(&Value::Array(vec![
            Value::Integer(1),
            Value::Text("two".to_owned())
        ]))
    );

    // The depths a top-level-only conversion would miss, read through the
    // dotted path a schema would use rather than by unwrapping tables here.
    let deep = crate::config::key::Key::new("nested.deeper.leaf").expect("a well-formed key");
    assert_eq!(
        document.get_path(&deep),
        Some(&Value::Bool(false)),
        "a nested table must convert at every depth: {document:?}"
    );
    println!("kinds.toml became {document:?}");
}

/// An absent file is not a refusal, because who is owed what by one differs
/// per caller.
///
/// The mutant is returning `NotRead` for a missing file, which reddens with
/// the refusal printed.
#[test]
fn an_absent_file_is_no_document_rather_than_a_refusal() {
    let root = ScratchRoot::new();
    let missing = root.base().join("nothing-here.toml");
    assert!(!missing.exists(), "the fixture must not stage this file");

    let read = TomlFile::at(&missing, roomy())
        .read()
        .expect("an absent file is not a failure");
    assert_eq!(read, None, "an absent file is no document");

    // And nothing was created in order to find that out.
    assert!(
        !missing.exists(),
        "the reader created {}, and a loader that creates state to read state is the thing \
         ADR-0014's port forbids",
        missing.display()
    );
}

/// The refusal names the file and the position and carries nothing off the
/// line.
///
/// **The second arm is the one that discriminates.** The parser's own
/// `Display` *does* publish the planted value, so a check that only asserted
/// absence from our refusal could not tell a redaction from a parser that
/// happens to say little. Asserting the leak exists in the source the refusal
/// was built from is [Verification lessons] §26 — an absence is evidence only
/// when the instrument could have found something.
///
/// Two arms on the absence, per [Verification lessons] §50: the raw nonce and
/// its ASCII core, so an escaping formatter cannot publish every byte of the
/// value while the assertion reads it as absent.
///
/// The mutant is building `FileRefused::NotToml`'s `detail` from
/// `error.to_string()` instead of `error.message()`.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn the_refusal_for_a_malformed_file_carries_neither_the_line_nor_a_value_on_it() {
    let root = ScratchRoot::new();
    let planted = personal_secret_nonce();
    let core = ascii_core(&planted);
    // The value is on the line the parser chokes on, which is the only
    // placement that puts it in the rendering under test.
    let body = format!("provider.credential = \"{planted}\" and then some nonsense\n");
    let (path, file) = staged(&root, "malformed.toml", body.as_bytes());

    let refusal = file.read().expect_err("the file is not TOML");
    let rendered = refusal.to_string();

    // The instrument could have found it: the parser's own rendering of the
    // same failure publishes every byte.
    let raw: toml::de::Error = body
        .parse::<toml::Table>()
        .expect_err("the same bytes fail the same way");
    assert!(
        raw.to_string().contains(&planted),
        "the parser's own Display no longer quotes the offending line, so this check asserts \
         nothing: {raw}"
    );

    assert!(
        !rendered.contains(&planted),
        "the refusal published the planted value: {rendered}"
    );
    assert!(
        !rendered.contains(core),
        "the refusal published the planted value's ASCII core, which no escaping alters: \
         {rendered}"
    );
    assert!(
        rendered.contains(&path.display().to_string()),
        "a refusal about a file names the file: {rendered}"
    );
    println!("refused: {rendered}");
}

/// The line and column are the parser's own, read back out of its rendering.
///
/// One arm of the comparison must not travel through the thing being checked
/// ([Verification lessons] §11), so the expectation is parsed out of the
/// parser's own `Display` — a reader that shares no code with
/// [`Position::of`].
///
/// The mutant is counting the column in bytes rather than characters.
///
/// **It survived its first run, and the fixture was the reason.** The awkward
/// characters were on the line *above* the error, and a column is counted from
/// the last newline — so the fixture was awkward on encoding and ordinary on
/// the axis the mutant moves, which is [Verification lessons] §51 exactly. The
/// multi-byte characters are now on the offending line and before the offending
/// column, and the mutant reddens.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_malformed_file_is_refused_at_the_line_and_column_the_parser_itself_reports() {
    let root = ScratchRoot::new();
    // Multi-byte characters on the offending line, before the offending
    // column, so a byte column and a character column differ *here*.
    let body = "ok = 1\nkéy = \"é𝄞value\" and then nonsense\n";
    let (_, file) = staged(&root, "position.toml", body.as_bytes());

    let refusal = file.read().expect_err("the file is not TOML");
    let FileRefused::NotToml { at: Some(at), .. } = &refusal else {
        panic!("expected a parse refusal carrying a position, got {refusal:?}");
    };

    let raw: toml::de::Error = body
        .parse::<toml::Table>()
        .expect_err("the same bytes fail the same way");
    let rendered = raw.to_string();
    let (line, column) = rendered
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix("TOML parse error at line ")?;
            let (number, rest) = rest.split_once(", column ")?;
            Some((
                number.parse::<usize>().ok()?,
                rest.trim().parse::<usize>().ok()?,
            ))
        })
        .expect("the parser renders its own line and column");

    assert_eq!(
        (at.line, at.column),
        (line, column),
        "the position this refusal names must be the parser's own: {rendered}"
    );
    println!("refused at {at}, which is what the parser itself printed");
}

/// A ceiling of zero is refused rather than taken.
#[test]
fn a_ceiling_of_zero_is_refused_because_it_would_accept_only_an_empty_file() {
    assert_eq!(SizeCeiling::new(0), Err(CeilingRefused));
    assert_eq!(
        SizeCeiling::new(1).map(SizeCeiling::get),
        Ok(1),
        "one byte is a ceiling"
    );
    println!("{}", CeilingRefused);
}

/// A file past the ceiling is refused before it is parsed.
///
/// **The fixture is both oversized and malformed**, which is what makes the
/// check discriminate: an implementation that parsed first and measured after
/// would produce `NotToml` and this asserts `TooLarge`.
///
/// The mutant is moving the size test after the parse.
#[test]
fn a_file_past_the_ceiling_is_refused_before_anything_parses_it() {
    let root = ScratchRoot::new();
    let path = root.base().join("oversized.toml");
    // Malformed on purpose, and larger than the ceiling below.
    std::fs::write(&path, b"this is not toml at all = = =\n").expect("could not stage the file");

    let ceiling = SizeCeiling::new(8).expect("eight bytes is a ceiling");
    let refusal = TomlFile::at(&path, ceiling)
        .read()
        .expect_err("the file is past the ceiling");

    let FileRefused::TooLarge {
        bytes, ceiling: at, ..
    } = &refusal
    else {
        panic!(
            "a file past the ceiling must be refused before it is parsed; a parse refusal here \
             means the bytes were read and parsed first: {refusal:?}"
        );
    };
    assert_eq!(*at, 8);
    assert!(*bytes > 8, "the fixture must actually be oversized");
    println!("refused: {refusal}");
}

/// A file that is not UTF-8 is refused naming the byte offset.
///
/// The mutant is reading the file lossily, which turns the invalid bytes into
/// replacement characters and lets a nonsense document through.
#[test]
fn a_file_that_is_not_utf8_is_refused_naming_the_byte_it_stopped_at() {
    let root = ScratchRoot::new();
    let (_, file) = staged(&root, "not-utf8.toml", b"key = \"\xff\xfe\"\n");

    let refusal = file.read().expect_err("the file is not UTF-8");
    let FileRefused::NotText { valid_up_to, .. } = &refusal else {
        panic!("expected a UTF-8 refusal, got {refusal:?}");
    };
    assert_eq!(
        *valid_up_to, 7,
        "the offset is where the valid prefix ends: {refusal}"
    );
    println!("refused: {refusal}");
}

/// TOML's float and datetime are refused naming the key and the kind.
///
/// Both are asserted, not one: a reader that refused only the first would pass
/// a check that staged only a float.
///
/// The mutant is coercing a float to an integer, which reddens with the
/// document printed instead of a refusal.
#[test]
fn a_kind_this_value_model_has_no_variant_for_is_refused_naming_the_key() {
    let root = ScratchRoot::new();

    for (name, body, key, kind) in [
        (
            "float.toml",
            "[runtime]\nbudget = 1.5\n",
            "runtime.budget",
            "float",
        ),
        (
            "datetime.toml",
            "[project]\nstarted = 1979-05-27T07:32:00Z\n",
            "project.started",
            "datetime",
        ),
    ] {
        let (_, file) = staged(&root, name, body.as_bytes());
        let refusal = file
            .read()
            .expect_err("no configuration key can hold this kind");
        let FileRefused::UnrepresentableKind {
            key: named,
            kind: reported,
            ..
        } = &refusal
        else {
            panic!("expected an unrepresentable-kind refusal for {name}, got {refusal:?}");
        };
        assert_eq!((named.as_str(), *reported), (key, kind));
        let rendered = refusal.to_string();
        assert!(
            rendered.contains(key) && !rendered.contains("1.5") && !rendered.contains("1979"),
            "the refusal names the key and never the value: {rendered}"
        );
        println!("refused: {rendered}");
    }
}

/// A key that is both a value and a table is refused by the parser, in **both**
/// write orders.
///
/// **This is a measurement that narrows an earlier one.** ADR-0012's arc
/// measured on 2026-09-05 that writing the nested key *first* lets the later
/// scalar replace it "with nothing reported", and recorded that as the worse of
/// the two orders. That is true of ADR-0014 D2's cross-layer merge, where two
/// documents are folded; it is **not** true inside one file, where the parser
/// refuses both orders as a duplicate key with a position. The silent loss is
/// a merge phenomenon and not a parse one, and the record is corrected to say
/// so.
///
/// The mutant is any reader that accepted either order.
#[test]
fn one_file_cannot_make_a_key_both_a_value_and_a_table_in_either_order() {
    let root = ScratchRoot::new();

    for (name, body) in [
        (
            "scalar-first.toml",
            "[model]\ndefault = \"a-model\"\n\n[model.default]\ninference = \"local\"\n",
        ),
        (
            "table-first.toml",
            "[model.default]\ninference = \"local\"\n\n[model]\ndefault = \"a-model\"\n",
        ),
    ] {
        let (_, file) = staged(&root, name, body.as_bytes());
        let refusal = file
            .read()
            .expect_err("one key cannot be a leaf and a branch");
        let FileRefused::NotToml { at, detail, .. } = &refusal else {
            panic!("expected a parse refusal for {name}, got {refusal:?}");
        };
        assert_eq!(detail, "duplicate key");
        assert!(at.is_some(), "the refusal names where: {refusal}");
        println!("{name}: refused at {}", at.expect("a position"));
    }
}

/// A position is one-based and counts characters, as an editor does.
#[test]
fn a_position_counts_lines_and_characters_from_one() {
    let source = "é𝄞x\nsecond\n";
    assert_eq!(
        Position::of(source, 0),
        Position { line: 1, column: 1 },
        "the first byte is line 1, column 1"
    );
    // Byte 8 is the start of "second": 2 + 4 + 1 for the first line, plus the
    // newline. A byte-counted column would say 9 here rather than 1.
    let second = source
        .find("second")
        .expect("the fixture has a second line");
    assert_eq!(
        Position::of(source, second),
        Position { line: 2, column: 1 }
    );
    // An offset inside a multi-byte character backs up rather than panicking.
    assert_eq!(Position::of(source, 1), Position { line: 1, column: 1 });
    // An offset past the end lands at the end.
    let end = Position::of(source, source.len() + 99);
    assert_eq!(end.line, 3, "the source ends with a newline: {end}");
}

/// An empty file is an empty document rather than a refusal.
#[test]
fn an_empty_file_is_an_empty_document() {
    let root = ScratchRoot::new();
    let (_, file) = staged(&root, "empty.toml", b"");
    assert_eq!(
        file.read().expect("an empty file is valid TOML"),
        Some(Table::new())
    );
}
