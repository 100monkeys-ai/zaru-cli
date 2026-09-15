// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The argument grammar a command body is written in, and the one pass that
//! expands it.
//!
//! # The grammar, and what is deliberately outside it
//!
//! Decided 2026-09-15 under a delegated coordinator ruling, open to Jeshua's
//! veto, and recorded on [ADR-0015's amendments volume 2]. [ADR-0015] D1 says
//! a command is "a named prompt template **with arguments**" and names no
//! spelling; the shape taken is the one a person arriving from another
//! harness already knows.
//!
//! - **`$ARGUMENTS`** — everything typed after the command's name, verbatim,
//!   including interior whitespace. Empty where nothing followed.
//! - **`$1` to `$9`** — that tail split on whitespace, one-indexed. An index
//!   with no word is the empty string rather than a refusal, because a
//!   command whose second argument is optional is an ordinary thing to write.
//! - **A `$` followed by anything else is literal text.** `$HOME`, `$PATH`,
//!   `$5.00`, `$$` and a bare `$` are prose. A command body is prose written
//!   for a model, and a harness that refused `$HOME` would refuse the English
//!   a deploy-check body is made of.
//! - **Refused at load: a `$` followed by a run of ASCII digits that is not a
//!   single `1` to `9`** — `$0`, `$10`, `$007`. That is the mistake a person
//!   makes *inside* the grammar, an off-by-one index, and it is the only
//!   unknown placeholder this grammar can have.
//!
//! **Two costs, stated rather than discovered, and both measured.**
//!
//! `$ARGUMENT`, `$ARGS` and `$ALL` stay literal and are **not** refused. The
//! alternative — refusing every `$` followed by an uppercase ASCII run that
//! is not `ARGUMENTS` — refuses `$HOME` and `$PATH`, which the bodies this
//! feature exists for are full of. Named and rejected.
//!
//! And a `$` beside a digit is this grammar's own shape, so prose carrying one
//! is read as a placeholder: **`it cost $5.00` expands to `it cost .00`**, and
//! **`it cost $0.50` is refused at load naming `$0`**. Both are held by
//! `a_dollar_amount_in_prose_is_read_as_this_grammar_reads_it` rather than
//! left to be discovered. The grammar with no digits in it — `$ARGUMENTS`
//! alone — has neither cost and cannot say "the first argument", which is
//! what a shared workflow is usually about; the spelling a person arriving
//! from another harness already knows was preferred, and the price is this
//! paragraph.
//!
//! # One pass, and why that is a security property rather than an efficiency
//! one
//!
//! [`expand`] walks the body once and appends. A substituted argument is
//! never re-scanned, so an argument whose own text is `$1` reaches the task as
//! `$1` and not as the first word again. A re-expanding substituter would let
//! a typed argument reach into the template that is expanding it, which is an
//! injection D1's inertness claim would not survive.
//!
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility
//! [ADR-0015's amendments volume 2]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility-updates-2

/// What a `$` opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// `$ARGUMENTS`.
    Arguments,
    /// `$1` to `$9`.
    Positional(usize),
    /// A digit run this grammar has no rule for, and how many bytes of it
    /// follow the `$`.
    Unknown(usize),
    /// Not a placeholder at all: the `$` is text.
    Literal,
}

/// The whole spelling `ARGUMENTS` takes.
const ARGUMENTS: &str = "ARGUMENTS";

/// What the `$` at the start of `rest` opened, `rest` being everything after
/// it.
fn read(rest: &str) -> Read {
    if rest.starts_with(ARGUMENTS) {
        return Read::Arguments;
    }
    let digits = rest
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    match digits {
        0 => Read::Literal,
        1 => match rest.as_bytes()[0] {
            b'0' => Read::Unknown(1),
            byte => Read::Positional(usize::from(byte - b'0')),
        },
        more => Read::Unknown(more),
    }
}

/// The first spelling in `body` that is in placeholder shape and is not in the
/// grammar, or `None` when every one of them is.
///
/// The spelling is returned with its `$`, and **nothing around it** — a
/// refusal that carried the line would publish whatever else was on it, which
/// is [`crate::config::file`]'s own rule.
#[must_use]
pub fn unknown(body: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut at = 0;
    while let Some(found) = body[at..].find('$') {
        let dollar = at + found;
        let after = dollar + 1;
        if let Read::Unknown(digits) = read(&body[after..]) {
            return Some(body[dollar..after + digits].to_owned());
        }
        // One byte past the `$`, so a `$$` is two reads rather than one skip.
        at = after.min(bytes.len());
    }
    None
}

/// The body with its placeholders replaced from `tail`, in one pass.
///
/// `tail` is everything the user typed after the command's name, already
/// trimmed of the whitespace that separated the two.
#[must_use]
pub fn expand(body: &str, tail: &str) -> String {
    let words: Vec<&str> = tail.split_whitespace().collect();
    let mut written = String::with_capacity(body.len() + tail.len());
    let mut at = 0;
    while let Some(found) = body[at..].find('$') {
        let dollar = at + found;
        written.push_str(&body[at..dollar]);
        let after = dollar + 1;
        match read(&body[after..]) {
            Read::Arguments => {
                written.push_str(tail);
                at = after + ARGUMENTS.len();
            }
            Read::Positional(index) => {
                written.push_str(words.get(index - 1).copied().unwrap_or_default());
                at = after + 1;
            }
            // Unreachable from a loaded command, because `unknown` refused it
            // at load. Written as text rather than as a panic: a body reaching
            // here has been checked, and a harness that aborted on a value it
            // had already accepted would be a defect report where a character
            // belongs.
            Read::Unknown(digits) => {
                written.push_str(&body[dollar..after + digits]);
                at = after + digits;
            }
            Read::Literal => {
                written.push('$');
                at = after;
            }
        }
    }
    written.push_str(&body[at..]);
    written
}
