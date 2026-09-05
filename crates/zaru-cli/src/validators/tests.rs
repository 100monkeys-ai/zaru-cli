// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What the two evaluators decide, and what their refusals may say.

use crate::validators::{PatternCeiling, PatternRefused, Patterns};
use zaru_core::iteration::validator::{Pattern, PatternMatch};

/// A ceiling large enough that reaching it means the pattern asked for it.
fn generous() -> PatternCeiling {
    PatternCeiling::new(1 << 20).expect("a mebibyte is not zero")
}

fn patterns() -> Patterns {
    Patterns::new(generous())
}

fn pattern(text: &str) -> Pattern {
    Pattern::new(text).expect("a non-empty pattern")
}

/// A value that exists nowhere else, carrying a tail no escaping leaves alone.
///
/// [Verification lessons] §50: an absence assertion is blind to whatever the
/// renderer escapes, so the raw value and an ASCII core are asserted
/// separately.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
fn nonce(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_nanos();
    format!(
        "{label}-{}-{nanos}-e\u{301}\u{e9}\u{1f701}",
        std::process::id()
    )
}

/// The part of a nonce no formatter can alter.
fn ascii_core(value: &str) -> &str {
    value
        .strip_suffix("-e\u{301}\u{e9}\u{1f701}")
        .unwrap_or(value)
}

#[test]
fn a_pattern_decides_both_ways_over_the_same_evaluator() {
    let evaluator = patterns();
    assert!(
        evaluator
            .decide(&pattern("ready"), "everything is ready\n")
            .expect("a usable pattern"),
        "ADR-0009 D3 row 3 passes when stdout matches",
    );
    assert!(
        !evaluator
            .decide(&pattern("ready"), "everything is not\n")
            .expect("a usable pattern"),
        "and fails when it does not; a kind that only ever passes is not a validator",
    );
}

/// D3 says "Stdout matches" and names no anchoring, so this is a search.
///
/// The mutant is anchoring the pattern — `^…$` around what the project wrote —
/// which makes this fail while the check above still passes, so the two
/// together say the reading rather than only the mechanism.
#[test]
fn matching_is_a_search_rather_than_a_whole_string_comparison() {
    assert!(
        patterns()
            .decide(&pattern("ok"), "everything ok, 12 passed\n")
            .expect("a usable pattern"),
        "ADR-0009 D3 names no anchoring; `matches = \"ok\"` on `everything ok` is what a reader \
         of grep or of any other tool expects, and anchoring it would fail with nothing saying \
         why",
    );
}

/// The refusal for an unusable pattern carries no part of the pattern.
///
/// **Measured 2026-09-05 before this was written**: `regex::Error`'s `Display`
/// renders the offending pattern under a caret and its `Debug` carries it too,
/// in all four invalid shapes probed. The mutant is building
/// [`PatternRefused`] from either of those renderings, which reddens every arm
/// below.
#[tokio::test]
async fn an_unusable_pattern_is_refused_without_the_pattern_appearing_anywhere() {
    let planted = nonce("pattern");
    // Unclosed character class, with the planted value inside the pattern.
    let offered = format!("{planted}[");
    let refusal = patterns()
        .decide(&pattern(&offered), "")
        .expect_err("an unclosed character class is not a regular expression");
    assert_eq!(refusal, PatternRefused::NotAPattern);

    // The instrument could have found something: the engine itself leaks it.
    let engine = regex::Regex::new(&offered).expect_err("the same pattern");
    assert!(
        engine.to_string().contains(&planted),
        "the measurement this check is built on: `regex::Error` DOES render the pattern, which \
         is why the refusal is built from the kind instead",
    );

    // Through the real door as well as through `decide`, because the defect
    // this guards against is a port implementation that skips its own refusal
    // type and hands the engine's error straight out ([Verification lessons]
    // S25: assert what a caller can reach).
    let through_the_port = PatternMatch::matches(&patterns(), &pattern(&offered), "")
        .await
        .expect_err("the port refuses it too");

    for rendered in [
        refusal.to_string(),
        format!("{refusal:?}"),
        zaru_core::iteration::PortFailure::from(refusal).to_string(),
        through_the_port.to_string(),
        format!("{through_the_port:?}"),
    ] {
        assert!(
            !rendered.contains(&planted),
            "the refusal must not carry the pattern: {rendered}",
        );
        assert!(
            !rendered.contains(ascii_core(&planted)),
            "nor a rendering of it that escaping left intact: {rendered}",
        );
        assert!(
            !rendered.contains(&planted.escape_debug().to_string()),
            "nor its escaped form: {rendered}",
        );
    }
}

/// A valid pattern carrying the same value passes, so the check above could
/// have found something.
///
/// [Verification lessons] §8: "nothing found" is evidence only when the
/// instrument could have found something.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_valid_pattern_carrying_the_same_value_is_used_rather_than_refused() {
    let planted = nonce("sibling");
    let output = format!("the command printed {planted} and stopped\n");
    assert!(
        patterns()
            .decide(&pattern(&regex::escape(&planted)), &output)
            .expect("an escaped literal is a valid pattern"),
        "the accepting sibling of the refusal check: the same value in a pattern that compiles",
    );
}

/// Look-around and backreferences are refused, which is the engine having no
/// backtracking rather than a missing feature.
///
/// This is the **structural** arm of the exponential-time claim: a backtracking
/// engine would compile all three. It is deterministic, so it says what a
/// wall-clock measurement on a machine carrying six builds cannot
/// ([Verification lessons] §57).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn look_around_and_backreferences_are_refused_because_the_engine_cannot_backtrack() {
    let evaluator = patterns();
    for construct in ["(?=x)", "(?!x)", "(?<=x)", r"(a)\1"] {
        assert_eq!(
            evaluator.decide(&pattern(construct), "x"),
            Err(PatternRefused::NotAPattern),
            "`{construct}` needs backtracking, and ADR-0003 D2's amendment 4 took an engine that \
             omits it so that every search is bounded; an engine that compiled this would be a \
             different decision",
        );
    }
}

/// A pattern that is catastrophic under a backtracking engine completes here.
///
/// `(a+)+$` against `a…a!` is the textbook exponential case: a backtracking
/// engine explores every partition of the run of `a`s. Under `regex` it is
/// `O(m * n)`, so the check simply finishes — which is the claim.
#[test]
fn a_catastrophic_pattern_completes_and_decides_both_ways() {
    let evaluator = patterns();
    let refusing = format!("{}!", "a".repeat(64));
    assert!(
        !evaluator
            .decide(&pattern("(a+)+$"), &refusing)
            .expect("a usable pattern"),
        "the pattern does not match, and reaching this line at all is the property: a \
         backtracking engine would still be exploring",
    );
    assert!(
        evaluator
            .decide(&pattern("(a+)+$"), &"a".repeat(64))
            .expect("a usable pattern"),
        "the accepting sibling, so the case above is not passing because nothing ran",
    );
}

/// A pattern past the CALLER'S ceiling is refused naming the ceiling and not
/// the pattern.
///
/// **The fixture is chosen so that the caller's ceiling is the only thing that
/// can refuse it.** `a{2000}` compiles under `regex`'s own default
/// `size_limit` of ten mebibytes and does not compile under four kibibytes,
/// measured 2026-09-05. An earlier fixture, `a{1000}{1000}{1000}`, is past the
/// default too — so the mutation that drops `.size_limit(..)` altogether left
/// this check green on its first run, because the engine refused the pattern
/// for its own reason and the refusal reports `self.ceiling` either way. That
/// is [Verification lessons] §10 exactly: the assertion read a proxy the code
/// computes rather than the consequence. The fixture was restaked rather than
/// the mutant dismissed (§52).
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[test]
fn a_pattern_larger_than_the_callers_ceiling_is_refused_naming_the_ceiling() {
    // Under the engine's own default, and over four kibibytes.
    let between = "a{2000}";
    assert!(
        regex::Regex::new(between).is_ok(),
        "staging: the fixture must be one only the caller's ceiling refuses, or this check          cannot tell a configured ceiling from the engine's default",
    );

    let small = Patterns::new(PatternCeiling::new(4096).expect("four kibibytes is not zero"));
    let refusal = small
        .decide(&pattern(between), "aaa")
        .expect_err("two thousand states is past four kibibytes");
    assert_eq!(refusal, PatternRefused::TooLarge { ceiling: 4096 });
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("4096"),
        "the refusal names the harness's own number: {rendered}",
    );
    assert!(
        !rendered.contains(between),
        "and not the project's text: {rendered}",
    );

    // The accepting sibling, and the arm that makes the ceiling's effect
    // visible: the SAME pattern under a generous ceiling is used rather than
    // refused, so the refusal above is the ceiling and not the pattern.
    assert!(
        patterns()
            .decide(&pattern(between), &"a".repeat(2000))
            .expect("the same pattern compiles under a mebibyte"),
    );
}

/// A ceiling of zero is refused, and the refusal says why rather than that it
/// was zero.
#[test]
fn a_ceiling_of_zero_is_refused() {
    let refusal = PatternCeiling::new(0).expect_err("zero bounds nothing");
    let rendered = refusal.to_string();
    assert!(
        rendered.contains("turning the `matches` kind off"),
        "the refusal says what a zero ceiling would actually do: {rendered}",
    );
}

/// The `unicode` feature is named in the manifest, and this is what it buys.
///
/// Not decoration: with `default-features = false` and `unicode` removed, every
/// pattern below stops compiling and this check reddens — which makes the
/// feature list in `[workspace.dependencies]` a thing a check reads rather than
/// a line a reader trusts.
#[test]
fn the_unicode_feature_is_what_lets_a_project_spell_a_character_class() {
    let evaluator = patterns();
    for (declared, output) in [
        (r"\d+ passed", "128 passed"),
        (r"\w+ ok", "build ok"),
        (r"\p{Greek}", "λ"),
        (r"(?i)ÉCHEC", "échec"),
    ] {
        assert!(
            evaluator
                .decide(&pattern(declared), output)
                .expect("the `unicode` feature is named in `[workspace.dependencies]`"),
            "`{declared}` is what a project would write; refusing it would be this crate \
             inventing a restriction ADR-0009 states nowhere",
        );
    }
}
