// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0015] D1's **Skill**: "a named procedure: instructions plus optional
//! validators", executing "only through existing tools".
//!
//! # What a skill is, beside a command
//!
//! A skill file is a command file with one more key. It sits under the same
//! two locations D3 names, it is admitted by the same question D4 asks, it is
//! attributed by the same line D6 writes, and its body is expanded by the
//! same one-pass grammar. **Everything D5 adds is the `[[validator]]`
//! array**: "A skill may carry `expect` clauses in the vocabulary of
//! [ADR-0009] D3. A skill with validators runs inside the iteration loop and
//! is refined against them; one without runs as instructions."
//!
//! So a skill with no validators is a command with a different word in its
//! attribution, and nothing else — which is D5's second sentence, built
//! rather than written.
//!
//! # The filename, and the thing it collides with
//!
//! `<name>.skill.md`, with the stem **before** `.skill` the name. That is not
//! a free namespace: [`COMMAND_EXTENSION`](super::document::COMMAND_EXTENSION)
//! is `md`, so before this module the command loader already read
//! `triage.skill.md` and called the command `triage.skill` — measured from
//! the release binary at `6bdf080`, where such a file was admitted under that
//! name and shown in the picker. So the skill loader **claims** the spelling
//! rather than adding one, and `<name>.md` beside `<name>.skill.md` is the
//! one collision a directory cannot prevent by itself:
//! [`CommandRefused::NameCollision`](super::document::CommandRefused::NameCollision)
//! refuses both, naming both paths.
//!
//! # One parser, not two
//!
//! The `[[validator]]` blocks are parsed by
//! [`ManifestFile::validators`](crate::manifest::ManifestFile::validators) —
//! the same function that reads `zaru.toml`'s — so a skill's block is
//! byte-identical to the one a person already knows, and a malformed one is
//! refused in the same words. A second parser here would be two readings of
//! one grammar, which is exactly what `manifest::file` refuses for a
//! manifest's own tables.
//!
//! # What executes, stated rather than implied
//!
//! D1's table says a skill executes "only through existing tools". A declared
//! validator's `run` reaches [`crate::process::Spawn`] through [ADR-0009]
//! D3's validator port — **a third port**, in that record's own words,
//! "neither the loop's `Executor` nor [ADR-0011] D1's `cmd.run`" — so it is
//! outside [ADR-0011] D3's permission model and no confirmation stands for
//! it. That is the existing port a project's own `zaru.toml` already reaches,
//! and it reaches it with **no admission at all**; a skill's is reached only
//! after D4's gate and only on a turn the person typed. So a skill is
//! strictly more gated than the manifest beside it, and D1's sentence is
//! amended to say so rather than the port being changed. The gate is made
//! informed rather than nominal by
//! [`Command::offered_rows`](super::document::Command::offered_rows), which
//! puts every `run` line verbatim into the admission question.
//!
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [ADR-0015]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility

use crate::commands::document::{CommandRefused, Kind, VALIDATOR_TABLE};
use crate::config::Table;
use std::path::Path;
use zaru_core::iteration::validator::Declared;

/// What a skill's stem carries before its name.
///
/// `triage.skill.md` has the file stem `triage.skill`, so this is what is
/// stripped from it.
pub const SKILL_INFIX: &str = ".skill";

/// The extension a skill file is spelled with, for a refusal that has to name
/// one.
pub const SKILL_EXTENSION: &str = ".skill.md";

/// The name and the kind a file stem spells.
///
/// **The stem decides, and nothing else.** D3 spells the file `<name>.md`, so
/// the file names the command; `<name>.skill.md` is the same rule with one
/// more thing in the name. A `name` key may only agree with what this
/// returns.
#[must_use]
pub fn of_stem(stem: &str) -> (&str, Kind) {
    match stem.strip_suffix(SKILL_INFIX) {
        Some(name) => (name, Kind::Skill),
        None => (stem, Kind::Command),
    }
}

/// The `[[validator]]` blocks a skill's front matter declares, in declaration
/// order.
///
/// An absent key is no validators, which is D5's "one without runs as
/// instructions" rather than a refusal.
///
/// # Errors
///
/// [`CommandRefused::Validator`] carrying the manifest reader's own refusal,
/// naming `path` so a reader knows which file the position is inside.
pub fn validators_of(head: &Table, path: &Path) -> Result<Vec<Declared>, CommandRefused> {
    let Some(value) = head.get(VALIDATOR_TABLE) else {
        return Ok(Vec::new());
    };
    crate::manifest::ManifestFile::validators(value).map_err(|source| CommandRefused::Validator {
        path: path.to_path_buf(),
        source,
    })
}

/// Refuse a `<name>.md` that carries a `[[validator]]`.
///
/// A person who wrote validators into a command file meant to write a skill,
/// and the remedy is the filename rather than deleting what they wrote —
/// which is [ADR-0016](https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy)
/// D2's "every error names the remedy".
///
/// # Errors
///
/// [`CommandRefused::ValidatorInACommand`] when `head` carries the key.
pub fn refuse_a_validator_in_a_command(
    head: &Table,
    name: &str,
    path: &Path,
) -> Result<(), CommandRefused> {
    if head.get(VALIDATOR_TABLE).is_some() {
        return Err(CommandRefused::ValidatorInACommand {
            path: path.to_path_buf(),
            skill: format!("{name}{SKILL_EXTENSION}"),
        });
    }
    Ok(())
}
