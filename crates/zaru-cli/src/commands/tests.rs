// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What [ADR-0015] D1's command kind is, checked where the rules live.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::commands::document::Source;
use crate::commands::{date, front_matter, placeholder};

/// ADR-0015 D3's file splits into a TOML head and a Markdown body, and the
/// body keeps its own bytes.
///
/// **The mutant:** `front_matter::split` returning the body trimmed of its
/// leading newline reddens the third assertion, because a body is a template
/// and a template's first line is the author's.
#[test]
fn a_command_file_splits_at_its_fences_and_the_body_is_verbatim() {
    let raw = "+++\ndescription = \"check a deploy\"\n+++\nRead the workflow.\n\nSay yes or no.\n";
    let split = front_matter::split(raw).expect("a fenced file splits");
    assert_eq!(split.head, "description = \"check a deploy\"\n");
    assert_eq!(split.above, 1, "the head sits one line into the file");
    assert_eq!(
        split.body, "Read the workflow.\n\nSay yes or no.\n",
        "the body is every byte after the closing fence's line"
    );
}

/// A file with no fence, and one whose fence is never closed, are both `None`
/// rather than a parse failure — the parser is never reached, so neither can
/// be reported as a TOML error.
#[test]
fn a_file_with_no_front_matter_and_one_that_never_closes_are_both_unsplit() {
    assert!(
        front_matter::split("Just a body.\n").is_none(),
        "a file with no opening fence has no front matter"
    );
    assert!(
        front_matter::split("+++\nname = \"x\"\nstill open\n").is_none(),
        "an opening fence that is never closed has no front matter"
    );
    assert!(
        front_matter::split("---\nname = \"x\"\n---\nbody\n").is_none(),
        "`---` is a CommonMark thematic break and is not this fence"
    );
}

/// Carriage returns and a byte-order mark are a file a person wrote on
/// another machine, not a file with a fault.
#[test]
fn a_file_written_on_another_platform_still_splits() {
    let raw = "\u{feff}+++\r\nname = \"x\"\r\n+++\r\nbody\r\n";
    let split = front_matter::split(raw).expect("a CRLF file with a mark splits");
    assert_eq!(split.head, "name = \"x\"\r\n");
    assert_eq!(split.body, "body\r\n");
}

/// ADR-0015 D1's "with arguments": `$ARGUMENTS` is the whole tail and `$1` to
/// `$9` are its words.
///
/// **The mutant:** `placeholder::expand`'s `Positional` arm indexing `words`
/// by `index` rather than `index - 1` reddens the second assertion with
/// `second` where `first` was asked for.
#[test]
fn the_grammar_expands_the_whole_tail_and_each_position() {
    assert_eq!(
        placeholder::expand("check $ARGUMENTS now", "the  main branch"),
        "check the  main branch now",
        "$ARGUMENTS is the tail verbatim, interior whitespace and all"
    );
    assert_eq!(
        placeholder::expand("$1 then $2 then $3", "first second"),
        "first then second then ",
        "a position with no word is empty rather than a refusal"
    );
    assert_eq!(
        placeholder::expand("nothing here", "ignored"),
        "nothing here",
        "a body with no placeholder is its own text"
    );
}

/// The one pass, which is the property D1's inertness rests on: an argument
/// that is itself a placeholder is not re-expanded.
///
/// **The mutant:** expanding in a loop until no placeholder remains reddens
/// this with `first` in place of the literal `$1`.
#[test]
fn an_argument_that_is_a_placeholder_is_not_expanded_again() {
    assert_eq!(
        placeholder::expand("run $1 and $2", "$2 first"),
        "run $2 and first",
        "a substituted argument is text and is never re-scanned"
    );
    assert_eq!(
        placeholder::expand("say $ARGUMENTS", "$ARGUMENTS"),
        "say $ARGUMENTS",
        "the tail reaches the task as the user typed it"
    );
}

/// A `$` this grammar has no rule for is text, which is what lets a command
/// body be the English a deploy-check is written in.
///
/// **The mutant:** treating every `$` followed by an uppercase run as a
/// placeholder reddens the first assertion by refusing `$HOME`.
#[test]
fn a_dollar_that_is_not_a_placeholder_is_literal() {
    for body in [
        "cd $HOME and look",
        "a bare $ and a $$ pair",
        "$ARGUMENT is not the word",
        "$_underscore and $-dash",
    ] {
        assert!(
            placeholder::unknown(body).is_none(),
            "`{body}` carries no unknown placeholder"
        );
        assert_eq!(
            placeholder::expand(body, "tail"),
            body,
            "`{body}` expands to itself"
        );
    }
}

/// The one unknown this grammar can have: an index outside `1` to `9`.
///
/// **The mutant:** `placeholder::read` returning `Positional` for every
/// single digit reddens the `$0` arm, which is the off-by-one a person
/// actually makes.
#[test]
fn a_positional_outside_one_to_nine_is_the_unknown_placeholder() {
    for (body, spelling) in [
        ("use $0 please", "$0"),
        ("use $10 please", "$10"),
        ("use $007 please", "$007"),
    ] {
        assert_eq!(
            placeholder::unknown(body).as_deref(),
            Some(spelling),
            "`{body}` names `{spelling}` and nothing around it"
        );
    }
    assert!(
        placeholder::unknown("use $1 and $9 and $ARGUMENTS").is_none(),
        "the accepting sibling: every spelling the grammar has is accepted"
    );
}

/// The refusal names the spelling and **nothing else on the line**, which is
/// `config::file`'s own rule for the same reason.
#[test]
fn the_unknown_placeholder_refusal_carries_no_other_word_of_the_line() {
    let spelling = placeholder::unknown("the token is hunter2 and the index is $0\n")
        .expect("an unknown placeholder");
    assert_eq!(spelling, "$0");
    assert!(
        !spelling.contains("hunter2"),
        "a refusal quoting the line would publish whatever was on it"
    );
}

/// D6's attribution line, in the record's own shape, with the glyph left to
/// the register that owns it.
///
/// **The mutant:** dropping the source word reddens the first assertion; the
/// second holds the user half, which has no admission date because nobody was
/// asked.
#[test]
fn the_attribution_line_is_the_records_own_shape() {
    let project = crate::commands::document::Expanded {
        name: "deploy-check".to_owned(),
        source: Source::Project,
        admitted: Some("2026-08-19".to_owned()),
        typed: "/deploy-check main".to_owned(),
        task: "check main".to_owned(),
    };
    assert_eq!(
        project.attribution(),
        "/deploy-check (project · admitted 2026-08-19)",
        "D6's own example, less the glyph the register paints"
    );
    let user = crate::commands::document::Expanded {
        admitted: None,
        source: Source::User,
        ..project
    };
    assert_eq!(
        user.attribution(),
        "/deploy-check (user)",
        "a user command was never admitted, so saying it was would be false"
    );
}

/// The civil date, without a dependency. Four days nobody can get wrong by
/// accident: the epoch, a leap day, the day after a century that is not a
/// leap year, and a day this workspace has already written down.
///
/// **The mutant:** dropping the `month <= 2` year adjustment reddens the leap
/// day, which is the whole reason the algorithm shifts the epoch to March.
#[test]
fn the_civil_date_is_arithmetic_rather_than_a_dependency() {
    assert_eq!(date::civil(0), "1970-01-01", "the epoch");
    assert_eq!(date::civil(19_416), "2023-02-28");
    assert_eq!(date::civil(19_782), "2024-02-29", "a leap day");
    assert_eq!(date::civil(20_711), "2026-09-15", "the day this was written");
    assert_eq!(date::civil(11_016), "2000-02-29", "a leap century");
    assert_eq!(date::civil(-1), "1969-12-31", "a day before the epoch");
}

/// The two costs this grammar has, measured rather than predicted, and
/// recorded here so a reader meets them as assertions rather than as
/// surprises.
///
/// A `$` beside a digit is the grammar's own shape, so prose that carries one
/// is read as a placeholder: `$5.00` loses its `$5` to the fifth argument, and
/// `$0.50` is **refused at load** naming `$0`. Both are named on
/// [ADR-0015's amendments volume 2] as the price of the spelling a person
/// arriving from another harness already knows, with the alternative — a
/// grammar with no digits in it — named and rejected there.
///
/// [ADR-0015's amendments volume 2]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates-2
#[test]
fn a_dollar_amount_in_prose_is_read_as_this_grammar_reads_it() {
    assert_eq!(
        placeholder::expand("it cost $5.00", "one two"),
        "it cost .00",
        "a digit one to nine beside a `$` is the positional it spells"
    );
    assert_eq!(
        placeholder::unknown("it cost $0.50").as_deref(),
        Some("$0"),
        "and a zero is the unknown placeholder, refused at load"
    );
}
