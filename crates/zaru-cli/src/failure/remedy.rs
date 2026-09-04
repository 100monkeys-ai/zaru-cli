// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! What a failure says, and what it tells the reader to do about it.
//!
//! [ADR-0016] D2: "An error message whose reader cannot act is a stack trace
//! with better grammar."
//!
//! # The remedy is composed where the failure is raised, never where it is shown
//!
//! A [`Remedy`] arrives as data from the code that knows what went wrong. It is
//! not assembled by a renderer from the class and a template, because then what
//! the user was told and what the harness believes it said could drift apart.
//! That is the same shape [`credentials::Confirm`](crate::credentials::Confirm)
//! uses for ADR-0007 D8's `grants` sentence and
//! [`tools::SessionNotice`](crate::tools::SessionNotice) uses for ADR-0011 D2's
//! not-a-sandbox line.
//!
//! # No command is written here
//!
//! D2's worked example prints `zaru config set provider.anthropic.key <key>`.
//! **This module hard-codes no command at all**, and that is not fastidiousness:
//! whether a configuration command may set a secret is an open question on
//! [operations/adr-status] — ADR-0016 D2 against [ADR-0014] D4, which says
//! configuration holds a reference and never a credential and that the
//! protection is "refusing to have a field to put one in". An [`Action`]'s
//! command is whatever its raising site passed, so no code here has picked an
//! answer.
//!
//! # Nothing here may carry a value
//!
//! A remedy is exactly the text that gets pasted into a report. The library's
//! [Credentials] page is blunt about it: "A failing test that quotes the value
//! it was handed has published it." So a raising site names the key, the alias
//! or the file and never the value, exactly as
//! [`ConfigRefused`](crate::config::ConfigRefused) and
//! [`SecretRefused`](crate::credentials::SecretRefused) already do — and the
//! checks on this module assert the absence of a planted value **and of its
//! ASCII core**, because `{:?}` escapes a combining mark and an absence
//! assertion over the raw value alone reads a published leak as absence.
//!
//! [ADR-0014]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0014-configuration-hierarchy
//! [ADR-0016]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0016-error-taxonomy
//! [Credentials]: https://100monkeys-ai.cortex.page/project-management/p/process/credentials
//! [operations/adr-status]: https://100monkeys-ai.cortex.page/zaru/p/operations/adr-status

use core::fmt;

/// Why a sentence was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementRefused {
    /// The sentence was empty, or was nothing but whitespace.
    ///
    /// Refused rather than accepted, because D1 has the class decide the
    /// presentation and every presentation has a headline; a blank one is a
    /// failure the reader cannot even name.
    Empty,
    /// The sentence carried a control character.
    ///
    /// Every presentation is rendered into a terminal. A control character
    /// there can move the cursor or erase a neighbouring row, so the report
    /// stops being evidence about what happened — the same argument
    /// [`AliasRefused::Control`](crate::credentials::AliasRefused::Control)
    /// and [`KeyRefused::Control`](crate::config::KeyRefused::Control) make.
    Control {
        /// The sentence as it was offered, escaped.
        offered: String,
    },
}

impl fmt::Display for StatementRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str(
                "a failure's statement is empty; ADR-0016 D1 has the class decide the \
                 presentation and every presentation has a headline",
            ),
            Self::Control { offered } => write!(
                f,
                "the statement {offered:?} carries a control character; every presentation is \
                 rendered into a terminal, where one can erase or overwrite a neighbouring row"
            ),
        }
    }
}

impl std::error::Error for StatementRefused {}

/// One sentence saying what happened.
///
/// Constructed only through [`Statement::new`], so a statement that reached
/// this type has already been refused every shape [`StatementRefused`] names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement(String);

impl Statement {
    /// Take a statement, refusing one that cannot be rendered.
    ///
    /// # Errors
    ///
    /// [`StatementRefused`] for an empty sentence or one carrying a control
    /// character.
    pub fn new(text: impl Into<String>) -> Result<Self, StatementRefused> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(StatementRefused::Empty);
        }
        if text.chars().any(char::is_control) {
            return Err(StatementRefused::Control {
                offered: text.escape_debug().to_string(),
            });
        }
        Ok(Self(text))
    }

    /// The sentence.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One thing the reader can do about a failure.
///
/// D2's worked example has two, each a lead-in and a command:
///
/// ```text
///   set one:  <a command>
///   or:       <another>
/// ```
///
/// The command is optional because not every action is one — "add a
/// `[[validator]]` block to `./zaru.toml`" is an action with no command, and
/// pretending otherwise would push a raising site into inventing a command
/// surface that [ADR-0015](https://100monkeys-ai.cortex.page/zaru/p/adrs/0015-commands-and-extensibility)
/// owns and that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    lead: Statement,
    command: Option<String>,
}

impl Action {
    /// An action with no command to run.
    #[must_use]
    pub const fn described(lead: Statement) -> Self {
        Self {
            lead,
            command: None,
        }
    }

    /// An action the reader can run verbatim.
    ///
    /// # Errors
    ///
    /// [`StatementRefused`] when `command` is empty or carries a control
    /// character — a command a terminal would mangle is not one a reader can
    /// paste.
    pub fn runnable(lead: Statement, command: impl Into<String>) -> Result<Self, StatementRefused> {
        let command = Statement::new(command)?;
        Ok(Self {
            lead,
            command: Some(command.as_str().to_owned()),
        })
    }

    /// What the action says.
    #[must_use]
    pub const fn lead(&self) -> &Statement {
        &self.lead
    }

    /// The command to run, where there is one.
    #[must_use]
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }
}

/// What ADR-0016 D2 says a user-correctable failure must carry.
///
/// **It cannot be empty.** There is no constructor taking a collection, so the
/// first action is always present and a remedy with nothing in it is not a
/// value this crate can build. That is what makes trigger clause 3 — "Every
/// user-correctable error carries a remedy" — a property of the type rather
/// than of a check, since
/// [`Classified::UserCorrectable`](crate::failure::Classified::UserCorrectable)
/// takes one positionally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remedy {
    first: Action,
    rest: Vec<Action>,
}

impl Remedy {
    /// A remedy with one action.
    #[must_use]
    pub const fn one(action: Action) -> Self {
        Self {
            first: action,
            rest: Vec::new(),
        }
    }

    /// The same remedy with another action after it — D2's `or:` line.
    #[must_use]
    pub fn also(mut self, action: Action) -> Self {
        self.rest.push(action);
        self
    }

    /// Every action, in the order the raising site gave them.
    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        core::iter::once(&self.first).chain(self.rest.iter())
    }

    /// How many actions there are. Never zero.
    #[must_use]
    pub const fn len(&self) -> usize {
        1 + self.rest.len()
    }

    /// Always `false`. Present because clippy asks for it beside `len`, and it
    /// is a true statement about this type rather than a courtesy.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }
}
