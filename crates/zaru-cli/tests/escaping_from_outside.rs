// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! A value a refusal quotes back is escaped once, and the escape is the
//! render's.
//!
//! # The rule, and the clause it comes from
//!
//! [ADR-0016] D2: "An error message whose reader cannot act is a stack trace
//! with better grammar." A remedy that misquotes the reader's own input is
//! worse than one that says nothing, because it sends them looking for a
//! character that is not there. `in-session-remedy` fixed the first two
//! instances on 2026-09-14 and deliberately left the sweep; this file is the
//! rule the sweep was taken under.
//!
//! **A value reaches the reader escaped exactly once.** A construction site
//! that applies `escape_debug`, or that builds the field through a `format!`
//! carrying a `{:?}`, has spent that one escape; the arm that renders the
//! field must then render it with `{}`. A construction site that stores the
//! value raw leaves the escape to a `{:?}` at the render. **Two is the
//! defect** and one is the contract, in either order.
//!
//! # The two shapes this counts, because the second is the one a narrower
//! rule misses
//!
//! The register's row stated the rule as "pre-escape plus a `{:?}` render
//! doubles", and twenty-nine of the thirty sites were exactly that.
//! `manifest::file`'s no-argument `expect` site was not: it was
//! `format!("{:?}", kind.escape_debug().to_string())`, **both escapes on one
//! line at construction**, rendered with a bare `{offered}`. A check written
//! against the row's wording would have read that render site as correct and
//! passed. So this counts escapes rather than matching a shape, and the count
//! at the construction site can be two on its own.
//!
//! # A fourth classification, which is correct and is not touched
//!
//! The row named three forms. There is a fourth: **pre-escape rendered with a
//! bare `{}`, inside neither quotes nor backticks.** Twelve sites in this tree
//! are that -- `NotForAChild`'s two names, `RefusedDestination`'s two hosts,
//! `SessionIdRefused::NotInTheAlphabet`'s character,
//! `HopRefused::AnotherHost`'s two hosts and `NotACommandLine`'s five. One
//! escape reaches the reader, so the rule above passes them and nothing here
//! moves them. They are named so that a later reader does not take the row's
//! "inside quotes or backticks" as excluding them.
//!
//! # What this reads, and the three things it cannot
//!
//! It resolves a construction site's type from the `Type::Variant {` it is
//! written as, and a render arm's type from the enclosing
//! `impl fmt::Display for T` or `impl From<T> for Classified` -- the second
//! because `failure::classify`'s remedy composers are a second surface the
//! same field reaches, and three of the thirty sites were doubled in both the
//! statement and the remedy of one refusal.
//!
//! **Its four named limits, stated rather than left to be discovered:**
//!
//! 1. **A value that reaches a field through a helper is invisible.**
//!    `process::line::CommandLine::split` builds five `NotACommandLine`
//!    variants from a `let escaped = || offered.escape_debug().to_string()`
//!    closure, so the five construction sites carry no `escape_debug` of
//!    their own and this walk does not pair them. They are correct today --
//!    every render of that field is a bare `{}` -- and they would not be
//!    caught if a render there became `{:?}` tomorrow.
//! 2. **An arm must be bounded at the next arm, never by a lookahead.** A
//!    fixed window over `CommandRefused::UnknownVerb`'s arm reads the
//!    `{offered:?}` belonging to `UnexpectedWord` three lines below it, and
//!    reports a correct site as broken. The release binary disproved exactly
//!    that reading while this sweep was being measured.
//! 3. **A type is resolved by its written name, not by its path.** This tree
//!    holds two enums spelled `SessionIdRefused`, one in `failure::defect` and
//!    one in `session::id`, and the walk gives them one key. They share no
//!    variant name, so nothing is mispaired today; a type added later that
//!    shares both a name and a variant with another would be.
//! 4. **A bare variant name is not a key.** `Self::Control` names seven
//!    different enums in this tree, so pairing on the variant alone matches
//!    `NameRefused::Control`'s construction against `KeyRefused`'s arm. The
//!    type is resolved on both sides, always.
//!
//! **There is no allow-list.** The rule is the pairing and the count; a site
//! is correct because the arithmetic says so, never because it is named here.
//!
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A field of one enum variant: the key both halves of the pairing resolve to.
type Field = (String, String, String);

/// Where a value is put into a field, and how many escapes it has spent.
struct Construction {
    file: PathBuf,
    line: usize,
    field: Field,
    escapes: usize,
}

/// Where a field is rendered, and how many escapes the render spends.
struct Render {
    file: PathBuf,
    line: usize,
    field: Field,
    escapes: usize,
}

/// The rule, over the whole product tree.
///
/// The mutant: any one of the thirty pre-escapes this sweep deleted, restored.
/// Watched red at `cc02eb9` with each of two restored in turn, one of each
/// shape, and the failure names the construction site, the render site and the
/// field.
#[test]
fn no_value_a_refusal_quotes_is_escaped_twice() {
    let sources = product_sources();
    let renders = renders(&sources);
    let constructions = constructions(&sources);

    let by_field: HashMap<&Field, Vec<&Render>> =
        renders.iter().fold(HashMap::new(), |mut found, render| {
            found.entry(&render.field).or_default().push(render);
            found
        });

    let mut paired = 0_usize;
    let mut doubled = Vec::new();
    for construction in &constructions {
        for render in by_field
            .get(&construction.field)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            paired += 1;
            if construction.escapes + render.escapes >= 2 {
                let (kind, variant, field) = &construction.field;
                doubled.push(format!(
                    "{}:{} builds {kind}::{variant}'s `{field}` with {} escape(s) and {}:{} \
                     renders it with {}",
                    construction.file.display(),
                    construction.line,
                    construction.escapes,
                    render.file.display(),
                    render.line,
                    render.escapes,
                ));
            }
        }
    }

    // Three floors, so "nothing found" is evidence that something could have
    // been. Each is well under what the tree holds today -- 235 files, 402
    // rendered fields and 19 pre-escaped constructions at the sweep -- and
    // each fails before the assertion below rather than passing vacuously.
    assert!(
        renders.len() >= 200,
        "the walk resolved only {} rendered fields across the product tree, so it is reading \
         something other than the sources it believes it is",
        renders.len()
    );
    assert!(
        constructions.len() >= 12,
        "the walk found only {} construction sites that escape a field, and this tree keeps \
         more than that deliberately -- see the fourth classification above",
        constructions.len()
    );
    assert!(
        paired >= 12,
        "only {paired} construction site(s) paired with a render, so the two halves are \
         resolving keys that do not meet"
    );

    assert!(
        doubled.is_empty(),
        "ADR-0016 D2: a value a refusal quotes back is escaped once, and the escape is the \
         render's. {} site(s) escape it twice, so the sentence asks the reader to remove a \
         backslash they never typed: {doubled:#?}",
        doubled.len()
    );
}

/// Every field rendered by a `Display` arm or by a `From<T> for Classified`
/// remedy composer, with the escapes that render spends.
///
/// An arm runs from its own pattern to the next one, never a fixed number of
/// lines: limit 2 above.
fn renders(sources: &[PathBuf]) -> Vec<Render> {
    let mut found = Vec::new();
    for file in sources {
        let source = read(file);
        let lines: Vec<&str> = source.lines().collect();
        for (opens, kind) in rendering_impls(&lines) {
            let closes = rendering_impls(&lines)
                .into_iter()
                .map(|(at, _)| at)
                .find(|at| *at > opens)
                .unwrap_or(lines.len());
            let arms: Vec<usize> = (opens..closes)
                .filter(|at| arm_variant(lines[*at], &kind).is_some())
                .collect();
            for (nth, opens_at) in arms.iter().enumerate() {
                let ends_at = arms.get(nth + 1).copied().unwrap_or(closes);
                let variant = arm_variant(lines[*opens_at], &kind).expect("the arm was matched");
                let arm = lines[*opens_at..ends_at].join(" ");
                for (field, debug) in specifiers(&arm) {
                    found.push(Render {
                        file: file.clone(),
                        line: opens_at + 1,
                        field: (kind.clone(), variant.clone(), field),
                        escapes: usize::from(debug),
                    });
                }
            }
        }
    }
    found
}

/// Every place a field is given a value that is already escaped, with the
/// number of escapes spent there.
///
/// One for an `escape_debug`, one for a `format!` carrying a `{:?}`, and two
/// for a line doing both -- which is a failure on its own, whatever the render
/// does, and is the shape `manifest::file` carried.
fn constructions(sources: &[PathBuf]) -> Vec<Construction> {
    let mut found = Vec::new();
    for file in sources {
        let source = read(file);
        let lines: Vec<&str> = source.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            let escapes =
                usize::from(line.contains("escape_debug")) + usize::from(formats_with_debug(line));
            if escapes == 0 {
                continue;
            }
            // The field the value is going into, which is this line or the
            // head of a multi-line initialiser a few lines above it.
            let Some((opens_at, field)) = (at.saturating_sub(7)..=at)
                .rev()
                .find_map(|above| field_name(lines[above]).map(|name| (above, name)))
            else {
                continue;
            };
            let Some((kind, variant)) = (opens_at.saturating_sub(24)..=opens_at)
                .rev()
                .find_map(|above| constructed_variant(lines[above]))
            else {
                continue;
            };
            found.push(Construction {
                file: file.clone(),
                line: at + 1,
                field: (kind, variant, field),
                escapes,
            });
        }
    }
    found
}

/// The type each `impl` block below renders, with the line it opens on.
///
/// Two kinds: a `Display`, where an arm is spelled `Self::Variant`; and a
/// `From<T> for Classified`, where it is spelled `T::Variant` because the
/// scrutinee is the refusal rather than the thing being built.
fn rendering_impls(lines: &[&str]) -> Vec<(usize, String)> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(at, line)| {
            let rest = line
                .strip_prefix("impl fmt::Display for ")
                .or_else(|| line.strip_prefix("impl core::fmt::Display for "))
                .map(|rest| rest.trim_end_matches(" {").to_owned())
                .or_else(|| {
                    let inner = line.strip_prefix("impl From<")?;
                    let (kind, tail) = inner.split_once('>')?;
                    tail.starts_with(" for ").then(|| kind.to_owned())
                })?;
            identifier(&rest).map(|kind| (at, kind))
        })
        .collect()
}

/// The variant an arm matches, when the line opens one for `kind`.
fn arm_variant(line: &str, kind: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("Self::")
        .or_else(|| trimmed.strip_prefix(&format!("{kind}::")))?;
    let variant = identifier(rest)?;
    variant
        .starts_with(|first: char| first.is_ascii_uppercase())
        .then_some(variant)
}

/// The `Type::Variant` a line opens a struct-variant literal for.
fn constructed_variant(line: &str) -> Option<(String, String)> {
    let head = line.trim_end().strip_suffix('{')?.trim_end();
    let (before, variant) = head.rsplit_once("::")?;
    let kind = identifier_backwards(before)?;
    let variant = identifier(variant)?;
    (variant == head.rsplit("::").next()?
        && kind.starts_with(|first: char| first.is_ascii_uppercase())
        && variant.starts_with(|first: char| first.is_ascii_uppercase()))
    .then_some((kind, variant))
}

/// The field a line opens an initialiser for: `name: ` at the head of it.
fn field_name(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let (name, rest) = trimmed.split_once(':')?;
    (!name.is_empty()
        && name
            .chars()
            .all(|each| each.is_ascii_lowercase() || each == '_')
        && rest.starts_with(' '))
    .then(|| name.to_owned())
}

/// Whether a line builds a string through a `format!` that carries a `{:?}`.
fn formats_with_debug(line: &str) -> bool {
    line.contains("format!(")
        && (line.contains("{:?}") || specifiers(line).into_iter().any(|(_, debug)| debug))
}

/// Every `{name}` and `{name:?}` in a fragment, with whether it is the debug
/// one. A `{:?}` with no name belongs to a positional argument and is counted
/// by [`formats_with_debug`] instead.
fn specifiers(fragment: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let bytes: Vec<char> = fragment.chars().collect();
    let mut at = 0_usize;
    while at < bytes.len() {
        if bytes[at] == '{' {
            let Some(closes) = (at + 1..bytes.len()).find(|over| bytes[*over] == '}') else {
                break;
            };
            let inner: String = bytes[at + 1..closes].iter().collect();
            let (name, debug) = match inner.strip_suffix(":?") {
                Some(name) => (name, true),
                None => (inner.as_str(), false),
            };
            if !name.is_empty()
                && name
                    .chars()
                    .all(|each| each.is_ascii_lowercase() || each == '_')
            {
                found.push((name.to_owned(), debug));
            }
            at = closes + 1;
        } else {
            at += 1;
        }
    }
    found
}

/// The leading Rust identifier of a fragment.
fn identifier(fragment: &str) -> Option<String> {
    let taken: String = fragment
        .chars()
        .take_while(|each| each.is_ascii_alphanumeric() || *each == '_')
        .collect();
    (!taken.is_empty()).then_some(taken)
}

/// The trailing Rust identifier of a fragment, which is how a `Type::Variant`
/// written as `Err(Type::Variant` gives up its type.
fn identifier_backwards(fragment: &str) -> Option<String> {
    let taken: String = fragment
        .chars()
        .rev()
        .take_while(|each| each.is_ascii_alphanumeric() || *each == '_')
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    (!taken.is_empty()).then_some(taken)
}

fn read(file: &Path) -> String {
    std::fs::read_to_string(file).expect("a source file this crate ships")
}

/// Every product source file, in the shape
/// `sentence_spacing_from_outside`'s sweep established: no `tests/`, no
/// `tests.rs`, no `fixtures.rs`.
fn product_sources() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the crates directory resolves from this crate's manifest");
    let mut found = Vec::new();
    walk(&crates, &mut found);
    found.sort();
    assert!(
        found.len() >= 60,
        "the walk found {} product source files across six crates, which is too few to be the \
         tree",
        found.len()
    );
    found
}

fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("a directory this crate ships") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests" || name == "target" {
                continue;
            }
            walk(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name == "tests.rs" || name == "fixtures.rs" {
                continue;
            }
            found.push(path);
        }
    }
}

/// **The sentence the reader sees, on the two surfaces one refusal reaches.**
///
/// The sweep above is a source walk, and a source walk can be right about the
/// tree and wrong about the reader. These arms drive the real constructors and
/// read what a person is shown: the statement, which is the refusal's own
/// `Display`, and the remedy, which `failure::classify` composes and which for
/// three of the thirty sites quoted the same doubled value a second time.
///
/// The value is `a\tb`, one typed tab. Escaped once it reads `"a\tb"` and
/// escaped twice `"a\\tb"` -- what the register's row quoted from the release
/// binary at `2a49c97` and what this arc reproduced at `cc02eb9` before the
/// fix.
///
/// The mutant: the pre-escape restored at `config::key`. Red with *"the
/// statement shows the reader `\"a\\\\tb\"` where they typed one tab"*.
#[test]
fn the_sentence_a_reader_sees_quotes_what_they_typed() {
    use zaru_cli::config::Key;
    use zaru_cli::failure::{Classified, Presentation};

    let refusal = Key::new("a\tb").expect_err("a tab is a control character");
    let classified = Classified::from(refusal.clone());
    let presentation = Presentation::of(&classified);

    let statement = refusal.to_string();
    let remedy: String = presentation
        .lines
        .iter()
        .map(|line| match &line.lead {
            Some(lead) => format!("{lead} {}", line.text),
            None => line.text.clone(),
        })
        .collect::<Vec<String>>()
        .join(" ");

    for (surface, rendered) in [("statement", &statement), ("remedy", &remedy)] {
        println!("-- {surface}: {rendered}");
        assert!(
            rendered.contains("\"a\\tb\""),
            "the {surface} does not quote the key as it was typed: {rendered}"
        );
        assert!(
            !rendered.contains("a\\\\tb"),
            "the {surface} shows the reader `\"a\\\\tb\"` where they typed one tab: {rendered}"
        );
    }

    // The accepting sibling, which is what stops the two assertions above
    // being satisfiable by a renderer that quotes nothing at all: an ordinary
    // value reaches both surfaces verbatim, with no escaping to remove.
    let ordinary = Key::new(" runtime.tier").expect_err("a leading space is refused");
    let ordinary_statement = ordinary.to_string();
    println!("-- accepting sibling: {ordinary_statement}");
    assert!(
        ordinary_statement.contains("\" runtime.tier\"") && !ordinary_statement.contains('\\'),
        "an ordinary value is not carried verbatim: {ordinary_statement}"
    );
}

/// **Every refusal this sweep touched carries the value exactly as offered.**
///
/// The rule under the sentences: one storage rule across a type's variants is
/// what makes one rendering rule correct, which is the reading
/// `every_alias_refusal_carries_the_value_exactly_as_offered` wrote down when
/// `in-session-remedy` fixed the first two instances. These are the four
/// refusals in the sweep whose constructor takes a string directly; the rest
/// are held by the tree-wide pairing above.
///
/// The mutant: any of the four pre-escapes restored. Red with *"a control
/// character: the refusal for \"a\\tb\" carries \"a\\\\tb\" instead"*.
#[test]
fn every_refusal_this_sweep_touched_carries_the_value_as_offered() {
    use zaru_cli::config::{Key, KeyRefused};
    use zaru_cli::failure::{Statement, StatementRefused};
    use zaru_cli::providers::{EndpointRefused, ProviderEndpoint};
    use zaru_cli::session::{SessionId, SessionIdRefused};
    use zaru_core::iteration::validator::{Name, NameRefused};

    let offered = "a\tb";
    let mut carried = 0_usize;

    let KeyRefused::Control { offered: held } =
        Key::new(offered).expect_err("a tab is refused as a key")
    else {
        panic!("a tab in a key is the control refusal");
    };
    assert_eq!(held, offered, "config::Key: {held:?} instead");
    carried += 1;

    let StatementRefused::Control { offered: held } =
        Statement::new(offered).expect_err("a tab is refused in a statement")
    else {
        panic!("a tab in a statement is the control refusal");
    };
    assert_eq!(held, offered, "failure::Statement: {held:?} instead");
    carried += 1;

    let EndpointRefused::Control { offered: held } =
        ProviderEndpoint::new(offered).expect_err("a tab is refused in an endpoint")
    else {
        panic!("a tab in an endpoint is the control refusal");
    };
    assert_eq!(
        held, offered,
        "providers::ProviderEndpoint: {held:?} instead"
    );
    carried += 1;

    let NameRefused::Control { offered: held } =
        Name::new(offered).expect_err("a tab is refused in a validator name")
    else {
        panic!("a tab in a validator name is the control refusal");
    };
    assert_eq!(held, offered, "validator::Name: {held:?} instead");
    carried += 1;

    // The accepting sibling on the type whose sibling variant was already
    // raw: `SessionIdRefused::NotInTheAlphabet` pre-escapes its character and
    // renders it with a bare `{}`, which is the fourth classification. It is
    // correct, it is untouched, and asserting it here is what shows this check
    // distinguishes the two rather than flagging every pre-escape.
    let SessionIdRefused::NotInTheAlphabet { found, .. } =
        SessionId::parse("0123456789ABCDEFGHJKMNPQR!").expect_err("a `!` is not base32")
    else {
        panic!("a `!` in a session id is the alphabet refusal");
    };
    assert_eq!(found, "!", "session::Id: {found:?} instead");

    assert_eq!(
        carried, 4,
        "four refusals in this sweep take a string directly and this check reached {carried}"
    );
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
