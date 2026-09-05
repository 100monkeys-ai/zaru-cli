// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0011] D6's four categories, transcribed — and the two of them that
//! name no program.
//!
//! # D6, in full, because every line of it constrains this module
//!
//! > A pattern list — recursive removal, force-push, disk operations,
//! > package-manager global installs — raises the prompt's prominence and
//! > annotates the transcript entry. **It does not veto.**
//! >
//! > The harness is not the authority on whether a command is right. A
//! > blocklist that cannot be overridden gets worked around by pasting the
//! > command into another terminal, which moves the action out of the
//! > transcript and makes things worse. **Surfacing beats forbidding.**
//!
//! [`Category`] is those four names and nothing else. There is no fifth, and
//! a fifth is this record's author's to add.
//!
//! # Two of the four are built, and two match nothing
//!
//! | Category | What D6's words determine | Matches |
//! | --- | --- | --- |
//! | [`Category::RecursiveRemoval`] | `rm` with a recursive flag | yes |
//! | [`Category::ForcePush`] | `git push` with a force flag | yes |
//! | [`Category::DiskOperations`] | nothing — no program is named | **nothing** |
//! | [`Category::PackageManagerGlobalInstalls`] | nothing — no manager is named | **nothing** |
//!
//! D6 names four *categories* and **no patterns**. Two of them name a shape
//! the record's own words determine: "recursive removal" is `rm` with a
//! recursive flag, and "force-push" is `git push` with a force flag. The
//! other two name no program at all. There is no route from "disk
//! operations" to `dd`, `mkfs`, `fdisk`, `parted`, `wipefs` or `shred`, nor
//! from "package-manager global installs" to `npm`, `pip`, `cargo`, `gem` or
//! `go`, that is not a list somebody wrote — and writing one is **authoring a
//! security vocabulary**, which [Autonomous Development] puts on the human
//! side of the boundary: "Adding a name to a capability set, to a permission
//! taxonomy, or to anything else deciding what a model-driven action may
//! reach **is a decision rather than the implementation of one**."
//!
//! This record's own reasoning agrees, and says why getting it wrong is
//! expensive in a direction that is hard to see: "Pattern-matching
//! destructive commands produces false positives, and a prompt that cries
//! wolf gets dismissed reflexively. **The list must stay short.**" A list
//! drafted by an implementer is a list nobody chose to keep short.
//!
//! **The two empty categories are pinned by a check** — see
//! `the_two_categories_that_name_no_program_match_nothing` — so that adding a
//! program later is a visible act rather than a quiet widening. That is the
//! shape this crate already uses for `cmd.run`'s absence of a program
//! allowlist.
//!
//! # It matches commands, and only commands
//!
//! All four of D6's categories are command shapes, and its own heading is
//! "Destructive **commands** are recognised and surfaced". So a
//! [`Subject::Path`] and a [`Subject::Url`] are never destructive here.
//! Making an `fs.write` destructive would be a fifth category, which is this
//! record's author's to add and not a matcher's to assume. Recorded as a
//! delegated coordinator ruling of 2026-09-05, open to Jeshua's veto.
//!
//! # It never vetoes, and that is held by the types rather than by this text
//!
//! [`DestructiveMatch`] answers a `bool` that reaches only the prompt's
//! prominence and the transcript annotation.
//! [`Requirement`](crate::tools::Requirement) has two variants and neither is
//! a refusal, so there is nothing a match could be written into that would
//! forbid a command.
//!
//! # The matching, stated exactly, and what it does not reach
//!
//! Literal and token comparison over
//! [`CommandLine::program`](crate::process::CommandLine::program) and
//! [`CommandLine::arguments`](crate::process::CommandLine::arguments), with
//! **no regular-expression engine and no new dependency**. The program is
//! compared by its file stem, so `/bin/rm` and `rm` both match — a small
//! reading D6 does not state, recorded here because without it the matcher is
//! evaded by spelling.
//!
//! A short cluster is read the way every POSIX utility reads one: `-rf` is
//! `r` and `f`. That is a convention rather than an invention, and without it
//! `rm -rf` — the shape D6 is most obviously about — would not match.
//! Anything after a bare `--` is a positional and is not read as a flag.
//!
//! **Residues, named rather than closed.** `git clean -fdx`, `find … -delete`,
//! `rmdir -p`, `xargs rm -r`, and `git push origin +main` (a force push by
//! refspec) are **not** matched, nor is anything in the two empty categories.
//! Widening any of it is this record's author's.
//!
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [Autonomous Development]: https://100monkeys-ai.cortex.page/project-management/p/process/autonomous-development

use crate::process::line::CommandLine;
use crate::tools::decision::{Invocation, Subject};
use crate::tools::port::DestructiveMatch;
use core::fmt;

/// One of ADR-0011 D6's four categories.
///
/// The set is closed and there is no way to build a fifth, for the reason
/// [`ToolName`](crate::tools::ToolName)'s is closed: a category added here is
/// a change to what the harness tells a user is dangerous, and that is a
/// decision rather than an implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// D6's "recursive removal".
    RecursiveRemoval,
    /// D6's "force-push".
    ForcePush,
    /// D6's "disk operations". **Names no program, so it matches nothing.**
    DiskOperations,
    /// D6's "package-manager global installs". **Names no manager, so it
    /// matches nothing.**
    PackageManagerGlobalInstalls,
}

impl Category {
    /// Every category ADR-0011 D6 names, in the order the record lists them.
    ///
    /// A hand-written list guarded by the exhaustive match in
    /// [`Category::phrase`]: a fifth variant stops that compiling, and the
    /// annotated length catches a variant added alongside a widened `ALL`.
    pub const ALL: [Self; 4] = [
        Self::RecursiveRemoval,
        Self::ForcePush,
        Self::DiskOperations,
        Self::PackageManagerGlobalInstalls,
    ];

    /// D6's own words for this category.
    ///
    /// Transcribed from the record rather than derived from the variant, so a
    /// renamed variant cannot silently restate what the record said.
    #[must_use]
    pub const fn phrase(self) -> &'static str {
        match self {
            Self::RecursiveRemoval => "recursive removal",
            Self::ForcePush => "force-push",
            Self::DiskOperations => "disk operations",
            Self::PackageManagerGlobalInstalls => "package-manager global installs",
        }
    }

    /// Whether D6's words determine a shape this matcher can hold.
    ///
    /// `false` for the two that name no program. **This is not a gap with a
    /// comment on it** — it is the record's own instruction, and
    /// [`Category::matches`] returning `false` for both is pinned by a check
    /// so that adding a program later is a visible act.
    #[must_use]
    pub const fn names_a_shape(self) -> bool {
        match self {
            Self::RecursiveRemoval | Self::ForcePush => true,
            Self::DiskOperations | Self::PackageManagerGlobalInstalls => false,
        }
    }

    /// Whether this command line is one of this category's shapes.
    #[must_use]
    pub fn matches(self, line: &CommandLine) -> bool {
        match self {
            Self::RecursiveRemoval => {
                program_is(line, "rm") && has_flag(line, RECURSIVE_LONG, RECURSIVE_SHORT)
            }
            Self::ForcePush => {
                program_is(line, "git")
                    && has_subcommand(line, "push")
                    && has_flag(line, FORCE_LONG, FORCE_SHORT)
            }
            // D6 names no program for either. See the module documentation.
            Self::DiskOperations | Self::PackageManagerGlobalInstalls => false,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.phrase())
    }
}

/// The long spellings of a recursive flag.
const RECURSIVE_LONG: &[&str] = &["--recursive"];

/// The short letters that make a removal recursive.
///
/// Both cases, because `rm -R` is `rm -r` on every implementation that has
/// the flag at all.
const RECURSIVE_SHORT: &[char] = &['r', 'R'];

/// The long spellings of a force flag on a push.
///
/// `--force-with-lease` and `--force-if-includes` are here because both *are*
/// force pushes — they are safer ones, and D6 surfaces rather than forbids,
/// so surfacing a safer force push costs a prompt's prominence and hides
/// nothing.
const FORCE_LONG: &[&str] = &["--force", "--force-with-lease", "--force-if-includes"];

/// The short letter that forces a push.
const FORCE_SHORT: &[char] = &['f'];

/// Whether the command's program is this name, compared by file stem.
///
/// `/bin/rm`, `./rm` and `rm` all match `rm`. See the module documentation
/// for why the stem rather than the whole string.
fn program_is(line: &CommandLine, name: &str) -> bool {
    std::path::Path::new(line.program())
        .file_name()
        .is_some_and(|stem| stem == name)
}

/// Whether the first argument that is not a flag is this word.
///
/// ADR-0004 D6's own worked example decides a `cmd.run` by its program and
/// its subcommand — `SUBCOMMAND_DENIED — curl not in allowed_subcommands` —
/// which is the shape this follows.
fn has_subcommand(line: &CommandLine, word: &str) -> bool {
    line.arguments()
        .iter()
        .find(|argument| !argument.starts_with('-'))
        .is_some_and(|argument| argument == word)
}

/// Whether any argument is one of `long`, or a short cluster carrying one of
/// `short`.
///
/// Everything after a bare `--` is a positional and is not read as a flag,
/// which is the same rule ADR-0015's own flag surface states for the command
/// line.
fn has_flag(line: &CommandLine, long: &[&str], short: &[char]) -> bool {
    for argument in line.arguments() {
        if argument == "--" {
            return false;
        }
        if long.contains(&argument.as_str()) {
            return true;
        }
        if let Some(cluster) = argument.strip_prefix('-')
            && !cluster.is_empty()
            && !cluster.starts_with('-')
            && cluster.chars().any(|letter| short.contains(&letter))
        {
            return true;
        }
    }
    false
}

/// ADR-0011 D6's pattern list, as the four categories the record names.
///
/// The product implementation of [`DestructiveMatch`]. Before 2026-09-05 that
/// trait had none anywhere in this workspace, so a `cmd.run` could not be
/// given the prompt prominence D6 requires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shapes;

impl Shapes {
    /// The matcher.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Which of D6's categories this call matches, if any.
    ///
    /// The category is returned rather than only a `bool` so that a caller
    /// that wants to say *which* shape it recognised can, without a second
    /// pass over the command line. [`DestructiveMatch::is_destructive`] is
    /// this, forgotten down to the `bool` the port answers.
    #[must_use]
    pub fn category(self, invocation: &Invocation<'_>) -> Option<Category> {
        // D6 is about commands. A path and a URL are not measured against it
        // — see the module documentation.
        let Subject::Command(line) = invocation.subject() else {
            return None;
        };
        Category::ALL
            .into_iter()
            .find(|category| category.matches(line))
    }
}

impl DestructiveMatch for Shapes {
    fn is_destructive(&self, invocation: &Invocation<'_>) -> bool {
        self.category(invocation).is_some()
    }
}
