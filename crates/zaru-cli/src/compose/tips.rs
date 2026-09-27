// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! [ADR-0002] D8's standing tip, and the two sentences D6's retrieval
//! commands say when there is nothing to retrieve.
//!
//! # What this module is for, and why it is called `tips`
//!
//! D8 names two kinds of recommendation. The **event-anchored** kind is built
//! and lives with the condition that produces it —
//! [`crate::manifest::absent`] is [ADR-0009] D4's, carried by
//! [`Owed`](crate::compose::Owed) and emitted at the end of the turn that
//! observed it. The **standing tip** is the other kind: "about a capability
//! the user has not discovered, tied to no particular event. Rendered in the
//! composer's hint strip when the prompt is empty, per [ADR-0005] D1.
//! Ephemeral: it yields the instant the user types." Everything a standing
//! tip needs that is not already in `zaru-tui` is here.
//!
//! [`NO_DEPOSITS`] and [`NOTHING_LEARNED`] are D6's rather than D8's, and
//! they are here because this is ADR-0002's only module in this crate. The
//! module is named for its substance rather than for its record, under the
//! coordinator ruling of 2026-09-15 06:58Z.
//!
//! # What was already built, and what was not
//!
//! `zaru-tui` has rendered a standing tip since 2026-09-05:
//! `StripContent::Tip` paints it, `Composer::set_standing` takes it,
//! [ADR-0005] D1's "deposits outrank tips" decides between it and a deposit
//! count, and D1's "typing dismisses a tip instantly — no fade, no delay" is
//! the strip being a pure function of composer state. That half is
//! **untouched here** and no line of it is re-decided.
//!
//! What had no existence at all was the producer, the budget and the count.
//! `Composer::set_standing`'s only caller in the whole tree was a check. This
//! module is the producer; [`Owed::has_room_for_a_tip`](crate::compose::Owed::has_room_for_a_tip)
//! is the budget; [`Tips`] is the count.
//!
//! # The count is a file and not the transcript, and two clauses say so
//!
//! D8: a standing tip "is suppressed after three displays without action".
//! This record's trigger clause 10 says the counter must survive "a session
//! restart". [ADR-0010] D2's [`Record::Said`](crate::session::Record::Said)
//! is the carrier for a line said once **per session** — that is what it was
//! built for and what its own documentation says — and a tip that was never
//! shown in *this* session leaves no trace in this session's transcript. So
//! the counter cannot be derived from it, and `~/.zaru/tips.jsonl` is where
//! it goes: one line per tip, outliving every session, exactly as
//! [`crate::session::History`] and [`crate::commands::Admissions`] outlive
//! one.
//!
//! **Keyed by the tip's name and by nothing else.** This is the one place the
//! file differs from those two, which both key on a working directory: a tip
//! is about a **capability**, not about a project, and a person who has been
//! shown a capability three times in one checkout has been shown it, whatever
//! directory they open next.
//!
//! # A display is one session's showing
//!
//! **A reading, accepted 2026-09-15 under directive 20 and open to Jeshua's
//! veto**, written on [ADR-0002's amendments page]. D8's word is "displays"
//! and the strip repaints on the beat, so counting paints would spend all
//! three inside a third of a second — which is the *opposite* of the reason
//! D8 gives for the number: an ephemeral strip "may have been visible for
//! 200ms and never read, and burning a tip the user never saw is worse than
//! showing it twice more". Counting sessions is the reading that makes the
//! sentence mean what its own reason says.
//!
//! The display is recorded when the tip is handed to the composer, which is
//! before the session's first frame and therefore before the row that carries
//! it is painted. A process that dies in between costs one count, which is
//! the bound [ADR-0010] D2 already accepts for the event in flight.
//!
//! # "Without action" needs no observer
//!
//! A tip's condition is a fact this harness can re-read. When the user takes
//! the action the tip names, the condition goes false and the tip stops being
//! eligible however low its count is — so there is nothing here that watches
//! for an action, and [`Conditions`] is the whole of it.
//!
//! [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
//! [ADR-0002's amendments page]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output-updates
//! [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
//! [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
//! [ADR-0010]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy

use crate::config::{Field, FieldKind, Key, Resolution, Schema, Value};
use crate::session::store::FILE_MODE;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// The file, inside `~/.zaru/`. A **ninth** thing under that directory.
///
/// After [ADR-0004](https://100monkeys-ai.cortex.page/zaru/p/adrs/0004-native-seal-in-the-harness)
/// D3's `node.key`, [ADR-0010] D1's `sessions/`,
/// [ADR-0014](https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy)
/// D1's `config.toml`,
/// [ADR-0007](https://100monkeys-ai.cortex.page/zaru/p/adrs/0007-credential-store)'s
/// `credentials.json`,
/// [ADR-0015](https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility)
/// D3's `commands/`, [ADR-0010] D1's `history.jsonl`, ADR-0015 D4's
/// `admissions.jsonl` and [ADR-0005] D3's `corpus.jsonl`. **A tenth joined
/// them on 2026-09-15**, [ADR-0027](https://100monkeys-ai.cortex.page/zaru/p/adrs/0027-zaru-persona-as-a-served-contract)
/// D1's `persona.jsonl`; this constant's own count is unchanged, because it
/// says where this file sits in the order rather than how many there are.
/// Accepted 2026-09-15
/// under directives 20, 25, 31 and 35 as a delegated coordinator ruling, open
/// to Jeshua's veto, and written on [ADR-0010's amendments volume 3].
///
/// [ADR-0010's amendments volume 3]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0010-session-and-transcript-updates-3
pub const TIPS_FILE: &str = "tips.jsonl";

/// How many showings suppress a standing tip.
///
/// **D8's own number, not a drafted one**: "a standing tip is suppressed after
/// three displays without action". It is named here rather than typed at the
/// call site so the record and the code have one spelling between them, which
/// is the shape [`crate::session::HISTORY_LINES`] and
/// [`zaru_tui::shell::STRIP_ROWS`] take — with the difference that those two
/// are numbers nobody decided and this one is the record's.
pub const SUPPRESS_AFTER: u32 = 3;

/// The configuration key D8 names, spelled here and nowhere else.
///
/// D8: "`tips = false` in `zaru.toml` disables both." The key is declared
/// **free at every layer**, because that is what D8's own sentence implies and
/// no project policy is invented here.
///
/// # D8's own spelling does not load, and that is recorded rather than worked
/// around
///
/// `zaru.toml` is
/// [ADR-0014](https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy)
/// D1's layer 3, and that file is **also**
/// [ADR-0009](https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators)
/// D1's project manifest, whose reader declares three top-level tables and
/// refuses a fourth key by name. Measured from the binary on 2026-09-15: a
/// `zaru.toml` carrying `tips = false` is refused with *"`tips` in … is not
/// something a manifest declares; it declares `[project]`, `[runtime]` and
/// `[[validator]]`"*. So `tips` is settable at layer 2
/// (`~/.zaru/config.toml`) and layer 4 (`ZARU_TIPS`), and not at the file D8
/// names.
///
/// **Two readings, both stated for Jeshua and neither taken in code.** Either
/// ADR-0009 D1's manifest gains a top-level `tips` key, which is that
/// record's author's and not an arc's; or D8's spelling is the thing that
/// wants correcting, on the argument
/// [`crate::tools::mode::PROJECT_REFUSAL`] already makes for `tools.mode` —
/// "how much the harness prompts is the user's own choice, and a repository
/// they cloned must not be able to grant itself fewer prompts" — which reads
/// at least as strongly for advice Zaru gives a person about their own
/// harness. Adding the manifest key would settle the first by building it and
/// declaring the field `refused_to_projects` would settle the second, so
/// neither is done.
pub const KEY: &str = "tips";

/// What `/inbox` and `zaru inbox` say.
///
/// **One authored line, drafted under the coordinator ruling of 2026-09-15
/// 06:58Z and open to Jeshua's veto.** D3 gives a deposit exactly one
/// producer — an **armed** trigger — and its last paragraph says "Default
/// configuration arms exactly one interrupt … Every other trigger ships
/// disarmed". Nothing in this harness can arm a trigger: D2's four arming
/// acts are "starting a long-running task, setting a watch, scheduling a run,
/// or enabling a named surface in configuration" and none of the four exists.
/// So the honest answer is not a refusal — the command works — it is that
/// there is nothing and why.
///
/// [ADR-0016] D2 wants a reader who can act, and the second clause is what
/// says the absence is a property of this build rather than of their machine.
pub const NO_DEPOSITS: &str =
    "no deposits — nothing is armed to make one, and this harness can arm nothing yet";

/// What `/learned` and `zaru learned` say.
///
/// **One authored line, drafted under the same ruling and open to the same
/// veto.** D5's craft-memory announcements need a craft-memory writer and
/// there is none in this workspace; [ADR-0031] is decision-blocked on which
/// product serves the prompt it would ride in. D5's own rule is that "a task
/// that learned nothing prints nothing" — but that governs the *push*, and
/// this is D6's pull, which "is always available on demand" and therefore has
/// to answer something.
///
/// [ADR-0031]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0031-relationship-memory
pub const NOTHING_LEARNED: &str =
    "nothing learned this session — this harness has no craft memory to write to yet";

/// One of D8's standing tips.
///
/// **A closed enum with one variant, and a second is a variant rather than a
/// row in a table.** Every match below is exhaustive with no wildcard arm, so
/// a second tip cannot arrive without its condition, its name and its line all
/// being written — which is the discipline
/// [`Namespace`](crate::cli::Namespace) and
/// [`Register`](zaru_tui::shell::Register) already carry.
///
/// **Which capability gets a tip is Jeshua's**, and so is its wording. What is
/// built here is the mechanism with one member, because a mechanism with no
/// member is unfalsifiable — [Verification lessons] §7's "a variant whose
/// condition nothing can satisfy is a permanent exemption dressed as a
/// promise", which is the rule [`crate::session::Record`] states in its own
/// documentation and which this arc declined to break for a deposit.
///
/// [Verification lessons]: https://100monkeys-ai.cortex.page/project-management/p/lessons/verification-lessons
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tip {
    /// No stored token holds [ADR-0006] D4's composer role, so the hint strip
    /// has nothing to search with.
    ///
    /// The capability is the one [the look-and-feel survey] calls "the layer
    /// that most distinguishes Zaru from Claude Code" and "the layer a person
    /// cannot see working".
    ///
    /// **It does not duplicate [`crate::terminal::trie::NOTHING_CACHED`]**,
    /// which [ADR-0005]'s Update of 2026-09-05 puts where `keyword only`
    /// already is — beside a search, while the user is typing — and which
    /// that same Update says "never on an empty prompt, which stays the
    /// tip's". The two lines are two surfaces of one absence and they never
    /// appear together.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    /// [the look-and-feel survey]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel
    NotesToken,
    /// The harness holds the mouse, so a plain click-and-drag no longer
    /// selects text, and the terminal's own selection is behind a modifier.
    ///
    /// The shell asks the terminal for its buttons so that the wheel scrolls
    /// the pane (`terminal::driver::arm`), and a terminal that reports its
    /// buttons stops selecting with them. Holding the terminal's bypass
    /// modifier gives the selection back: Shift in Windows Terminal, in the VS
    /// Code terminal off macOS, and in most others. Added 2026-09-27 under the
    /// coordinator's delegated ruling on [ADR-0005]'s amendments.
    ///
    /// [ADR-0005]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0005-the-composer
    Selection,
}

impl Tip {
    /// Every tip, so a check can walk them rather than list them.
    ///
    /// The length is annotated, so a second fails to compile here as well as
    /// in every exhaustive match below.
    ///
    /// **The order is the offer order**, because D8's budget is one tip per
    /// session and [`eligible`] takes the first that passes. The rule, taken
    /// 2026-09-27 and open to Jeshua's veto: a tip that says how to get back
    /// something the harness took outranks a tip about a capability it adds.
    pub const ALL: [Self; 2] = [Self::Selection, Self::NotesToken];

    /// The stable identifier `~/.zaru/tips.jsonl` records.
    ///
    /// **A name and never the line.** The file is a count of showings, so it
    /// carries what was shown by reference; storing the sentence would put
    /// authored prose on disk for no reader, and would make a reworded tip a
    /// new tip with a fresh budget.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotesToken => "notes-token",
            Self::Selection => "select-text",
        }
    }

    /// The one line the hint strip paints.
    ///
    /// **Authored, drafted under the coordinator ruling of 2026-09-15 06:58Z
    /// and open to Jeshua's veto.** D8's form is "one line either way. Never a
    /// modal, never a separate emission, never a second paragraph explaining
    /// itself", and its own worked examples are a condition and an action
    /// separated by `·`. This is thirty-eight columns, which fits the
    /// forty-column frame the strip's other lines are written for; the strip
    /// clips rather than wraps, which is `pane-navigation`'s open defect, so
    /// the width is a constraint on the wording rather than a mechanism.
    ///
    /// The action is the **in-session** spelling, because a standing tip is
    /// only ever painted inside a session.
    #[must_use]
    pub const fn line(self) -> &'static str {
        match self {
            Self::NotesToken => "search your notes here · /notes tokens",
            // Twenty-five columns. It names Shift and nothing else, because
            // Shift is the modifier on every terminal this harness's platforms
            // (Linux and WSL) put in front of it; iTerm2's Option and
            // Terminal.app's Fn are macOS's.
            Self::Selection => "hold Shift to select text",
        }
    }
}

/// What this harness can read about the capabilities a tip is about.
///
/// **Facts, never judgements.** Each field is something the harness already
/// knows at session open; nothing here asks a model and nothing here is a
/// preference. See the module documentation for why this is also the whole of
/// "without action".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Conditions {
    /// Whether a stored token holds [ADR-0006] D4's composer role.
    ///
    /// [ADR-0006]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0006-nuclear-notes-surfaces
    pub composer_token: bool,
    /// Whether the shell asked the terminal for its mouse buttons, which is
    /// what takes the terminal's plain click-and-drag selection away.
    pub mouse_captured: bool,
}

impl Conditions {
    /// Whether this tip's capability is still undiscovered.
    ///
    /// Exhaustive with no wildcard arm, so a second tip needs a condition
    /// written for it.
    #[must_use]
    pub const fn holds(self, tip: Tip) -> bool {
        match tip {
            Tip::NotesToken => !self.composer_token,
            Tip::Selection => self.mouse_captured,
        }
    }
}

/// One line of `~/.zaru/tips.jsonl`, as the file holds it.
///
/// **Three fields and none of them is prose the session produced.** `tip` is
/// [`Tip::name`], a literal this module owns; `displays` is a count; `shown`
/// is a civil date. There is no field a task, an answer, a path or a
/// credential could ride, which is what makes this file's security property
/// structural rather than filtered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shown {
    /// Which tip, by [`Tip::name`].
    pub tip: String,
    /// How many sessions have shown it.
    pub displays: u32,
    /// `YYYY-MM-DD`, the last session that did.
    pub shown: String,
}

/// Something went wrong with the tips file.
///
/// **No variant carries a tip's line, a path inside a project, or anything a
/// session produced.** A refusal is the text that gets pasted into a bug
/// report, which is [`crate::session::history`]'s own rule for the same
/// reason.
#[derive(Debug)]
pub enum TipsError {
    /// A filesystem operation failed.
    Io {
        /// What was being attempted.
        action: &'static str,
        /// The path it was attempted on.
        path: PathBuf,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// A **complete** line did not parse.
    ///
    /// Distinct from a trailing fragment, which is the line that was in
    /// flight when a machine lost power, exactly as it is on the transcript.
    Malformed {
        /// The file.
        path: PathBuf,
        /// Which line, counting from one.
        line: usize,
        /// What the parser said. Positional; it quotes no field value.
        detail: String,
    },
    /// A line could not be rendered.
    NotSerialisable {
        /// What the serialiser said. Positional; it quotes no field value.
        detail: String,
    },
}

impl fmt::Display for TipsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} at {}: {source}", path.display()),
            Self::Malformed { path, line, detail } => write!(
                f,
                "line {line} of the tips at {} did not parse: {detail}",
                path.display()
            ),
            Self::NotSerialisable { detail } => {
                write!(f, "a tips line could not be rendered: {detail}")
            }
        }
    }
}

impl std::error::Error for TipsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } | Self::NotSerialisable { .. } => None,
        }
    }
}

/// The tips file.
///
/// The discipline is [`crate::session::History`]'s and
/// [`crate::commands::Admissions`]', and it is the same discipline rather than
/// a third description of it: append-only, one JSON object per line, the last
/// line for a tip winning, a trailing fragment never counted.
///
/// **Append rather than rewrite**, which means a tip shown four times has four
/// lines and the reader takes the greatest count. A rewrite would need the
/// file locked against a second session, and this file is written at most once
/// per session by construction.
#[derive(Debug, Clone)]
pub struct Tips {
    path: PathBuf,
}

impl Tips {
    /// The tips at `path`.
    #[must_use]
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The tips under a `~/.zaru`-equivalent `root`.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self::at(root.join(TIPS_FILE))
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every line in the file, oldest first, with the fragment separated.
    ///
    /// **An absent file is no showings rather than a fault**, which is the
    /// distinction [`crate::session::History::entries`] already draws: a
    /// machine that has never run this harness has nothing to report.
    ///
    /// # Errors
    ///
    /// [`TipsError::Io`] when the file is there and cannot be read, and
    /// [`TipsError::Malformed`] when a complete line does not parse.
    pub fn entries(&self) -> Result<Vec<Shown>, TipsError> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(TipsError::Io {
                    action: "read the tips",
                    path: self.path.clone(),
                    source,
                });
            }
        };
        // Everything before the last newline is complete; whatever follows it
        // is the line that was in flight, which is what a power cut costs and
        // is never counted -- `session::transcript`'s own rule.
        let complete = raw.rfind('\n').map_or("", |at| &raw[..=at]);
        complete
            .lines()
            .enumerate()
            .map(|(at, line)| {
                serde_json::from_str(line).map_err(|error| TipsError::Malformed {
                    path: self.path.clone(),
                    line: at + 1,
                    detail: error.to_string(),
                })
            })
            .collect()
    }

    /// How many sessions have shown `tip`.
    ///
    /// The **greatest** count on the file rather than the last, so a line
    /// written by a session whose clock or ordering was odd cannot lower a
    /// count that has already been reached.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn displays_of(&self, tip: Tip) -> Result<u32, TipsError> {
        Ok(self
            .entries()?
            .into_iter()
            .filter(|shown| shown.tip == tip.name())
            .map(|shown| shown.displays)
            .max()
            .unwrap_or(0))
    }

    /// Whether `tip` has had [`SUPPRESS_AFTER`] showings.
    ///
    /// # Errors
    ///
    /// [`Self::entries`]'s.
    pub fn suppressed(&self, tip: Tip) -> Result<bool, TipsError> {
        Ok(self.displays_of(tip)? >= SUPPRESS_AFTER)
    }

    /// Record one session's showing of `tip`.
    ///
    /// **Named `record_a_showing` rather than `record`**, and that is a
    /// verification decision rather than prose: `only_one_place_in_the_product_records_a_tip_showing`
    /// counts this call by name over the crate's own source, and a method
    /// called `record` cannot be told from
    /// [`Transcript::record`](crate::session::Transcript::record), which is
    /// called in fifteen places. The first form of that check counted
    /// `.record(tip` and a mutant that spelled the argument out passed it.
    ///
    /// The discipline is [`crate::session::History::append`]'s: the file is
    /// opened `append`, the line and its newline go out in **one**
    /// `write_all` so a kill cannot split them, then `flush`, then
    /// `sync_data`.
    ///
    /// # Errors
    ///
    /// [`TipsError::Io`] when the file cannot be opened or written,
    /// [`TipsError::NotSerialisable`] when the line cannot be rendered, and
    /// [`Self::entries`]'s when the count it is raising cannot be read.
    pub fn record_a_showing(&self, tip: Tip, today: &str) -> Result<(), TipsError> {
        let entry = Shown {
            tip: tip.name().to_owned(),
            displays: self.displays_of(tip)?.saturating_add(1),
            shown: today.to_owned(),
        };
        let mut rendered =
            serde_json::to_string(&entry).map_err(|error| TipsError::NotSerialisable {
                detail: error.to_string(),
            })?;
        rendered.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(FILE_MODE)
            .open(&self.path)
            .map_err(|source| TipsError::Io {
                action: "open the tips for appending",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(rendered.as_bytes())
            .map_err(|source| TipsError::Io {
                action: "append to the tips",
                path: self.path.clone(),
                source,
            })?;
        file.flush().map_err(|source| TipsError::Io {
            action: "flush the tips",
            path: self.path.clone(),
            source,
        })?;
        file.sync_data().map_err(|source| TipsError::Io {
            action: "sync the tips",
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }
}

/// [`KEY`] as a [`Key`].
///
/// # Panics
///
/// Never. [`KEY`] is a literal this module owns and is well formed.
#[must_use]
pub fn key() -> Key {
    Key::new(KEY).expect("tips is a well-formed key")
}

/// What [`KEY`] holds.
///
/// A boolean, free at every layer. See [`KEY`] for why no project policy is
/// declared.
#[must_use]
pub fn field() -> Field {
    Field::free(FieldKind::Bool)
}

/// Declare D8's configuration key into a caller's schema.
///
/// The shape [`crate::tools::mode::declare`] already uses, so a caller
/// building a schema asks each record for its own keys rather than
/// transcribing them.
#[must_use]
pub fn declare(schema: Schema) -> Schema {
    schema.with(key(), field())
}

/// Whether D8's recommendations are enabled, from a resolved configuration.
///
/// **Absent is enabled**, which is D8's own shape: the clause names
/// `tips = false` as the thing a person writes to turn them off, so the
/// default is not a value at [ADR-0014](https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy)
/// D1's layer 1 and no built-in row is declared for it. A key set to anything
/// that is not a boolean has already been refused by the schema before this
/// is called.
#[must_use]
pub fn enabled(resolution: &Resolution) -> bool {
    !matches!(resolution.get(&key()), Some(Value::Bool(false)))
}

/// [ADR-0002] D8's budget, for a session that may not be able to run a turn.
///
/// # Why the absence of an `Owed` is not the absence of room
///
/// Row 5 of [the second look-and-feel audit] measured the standing tip
/// painting on no machine this workspace could produce: `terminal::open`
/// offered it inside `if let Turnable::Ready(turns)`, so **the session whose
/// person has discovered nothing** — the one with no model configured, or a
/// model and no key — was the one session the tip was withheld from. The guard
/// is the status refresh's beside it, where it is correct, and it was borrowed.
///
/// A session that resolved no provider has no [`Owed`](crate::compose::Owed) because it has no
/// [`Prepared`](crate::compose::Prepared) to build one from. It also cannot
/// run a turn, so **nothing else can spend D8's one-per-session budget**:
/// [ADR-0011] D2's notice and [ADR-0009] D4's recommendation are both a turn's,
/// and neither can arise. What is left of the budget is `tips = false`, which
/// D8 says "disables both", and that is the whole of the `None` arm.
///
/// `tips` is read only where there is no `Owed`; where there is one it
/// already holds the switch, which is that type's own reason for holding it.
/// [`Owed::has_room_for_a_tip`](crate::compose::Owed::has_room_for_a_tip) is
/// unedited.
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
/// [ADR-0009]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0009-project-manifest-and-validators
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
/// [the second look-and-feel audit]: https://100monkeys-ai.cortex.page/zaru/p/operations/harness-look-and-feel-audit-2
#[must_use]
pub fn room_for_a_tip(owed: Option<&crate::compose::Owed>, tips: bool) -> bool {
    owed.map_or(tips, crate::compose::Owed::has_room_for_a_tip)
}

/// The tip this session may offer, if any.
///
/// Three gates, in this order, and each is somebody else's rule:
///
/// 1. `room` is [ADR-0002] D8's budget, read from
///    [`Owed::has_room_for_a_tip`](crate::compose::Owed::has_room_for_a_tip).
///    It carries the `tips = false` half too, so this function does not read
///    the configuration a second time.
/// 2. [`Conditions::holds`] is the capability still being undiscovered, which
///    is also D8's "without action".
/// 3. [`Tips::suppressed`] is the three showings.
///
/// The first tip of [`Tip::ALL`] that passes all three is the one. That order
/// became a decision on 2026-09-27, when the second tip arrived. It is stated
/// on [`Tip::ALL`].
///
/// # Errors
///
/// [`Tips::entries`]'s, when the count cannot be read. A caller that cannot
/// read the file offers no tip rather than ending the session; see
/// [`crate::terminal::open`].
///
/// [ADR-0002]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0002-unprompted-output
pub fn eligible(room: bool, conditions: Conditions, tips: &Tips) -> Result<Option<Tip>, TipsError> {
    if !room {
        return Ok(None);
    }
    for tip in Tip::ALL {
        if conditions.holds(tip) && !tips.suppressed(tip)? {
            return Ok(Some(tip));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
