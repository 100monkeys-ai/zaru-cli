// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The wire contract for [ADR-0011] D1's seven built-ins: one JSON object per
//! call, and this is the only door from a request's text into a call.
//!
//! # The contract was an open question and it is now decided
//!
//! [ADR-0011]'s Status tracking carried it: "D1 names seven tools and no
//! argument schema for any of them. A model has to be told what arguments a
//! tool takes, and the descriptors offered carry an empty schema — which says
//! truthfully that the record specifies none. Inventing one would be authoring
//! the surface's wire contract."
//!
//! It is authored here rather than silently, under **directive 20 of
//! 2026-09-05** — Jeshua, "make the coherent and deterministic decisions to
//! normalize everything across the board" — as a delegated coordinator ruling
//! recorded on ADR-0011 D1 as an accepted Update and open to his veto. Three
//! measurements decided the shape rather than a preference:
//!
//! 1. **A provider already sends a JSON object.** `zaru-cli`'s Gemini client
//!    maps a `functionCall` to a [`ToolRequest`] with `arguments` set to the
//!    `args` object serialised, because that is what the API sends.
//! 2. **A provider already reads the descriptor's `parameters` as JSON.** It
//!    parses that field with `serde_json` and treats a descriptor whose
//!    parameters are not JSON as a defect of whoever supplied it.
//! 3. **An empty string is not JSON.** Measured 2026-09-05:
//!    `serde_json::from_str::<Value>("")` is `Err(EOF while parsing a value at
//!    line 1 column 0)`. So the seven empty schemas this surface offered would
//!    have been refused, all seven, by the first provider client to be handed
//!    them — a collision that existed before this module and independently of
//!    it.
//!
//! **No dependency arrives.** `serde_json` is already `zaru-cli`'s, so
//! [ADR-0003] D2's table is untouched and its clause 7 does not move.
//!
//! # The parse happens before the permission decision, and that is the rule
//!
//! [`crate::process::line::CommandLine::split`] is the precedent and the reason
//! is the same one: [ADR-0011] D4's transcript entry and D3's prompt both show
//! the target, and **a path that has not been extracted from the arguments is
//! not yet a target**. Measuring the whole arguments text against D4's boundary
//! is the defect `ToolName::addresses_a_path` was corrected for on 2026-09-05,
//! arriving a second time through a different door: with a JSON object as the
//! arguments, `{"path":"src/main.rs"}` would resolve to
//! `<root>/{"path":"src/main.rs"}` and count as in-tree.
//!
//! # No shim, because the harness is pre-alpha
//!
//! All seven move together. A parse that accepted a bare string *or* an object
//! would be the backward-compatibility path the harness's lifecycle rule
//! forbids, and it would also be a surface with two wire contracts — the
//! incoherence directive 20 exists to remove.
//!
//! # What a refusal is, and what it is not
//!
//! A request whose arguments are not an object with exactly the declared
//! fields is **the model having asked for something that is not a call**, in
//! the shape [`NotACall`](super::execute::NotACall) already holds: it is told
//! to the model, the turn carries on, and **no transcript record is written**,
//! because the refusal is reached before the decision and before the first
//! `Phase::Started`. It is never a panic and never a defect.
//!
//! **No refusal carries a value out of the arguments.** A field's *name* is
//! quoted, because a reader has to see which one; a field's *value* never is
//! — `fs.write`'s `contents` is a whole file, and the value the harness is
//! least entitled to publish is the one it was asked to store. This is
//! [`FileRefused`](crate::config::FileRefused)'s rule one layer up, and it is
//! why the JSON is walked as a [`serde_json::Value`] rather than deserialised
//! into a struct: `serde_json`'s own type errors render the offending value.
//!
//! [ADR-0003]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0003-build-strategy-and-licensing
//! [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
//! [`ToolRequest`]: zaru_core::tool_call::ToolRequest

use crate::tools::name::{FieldKind, SubjectKind, ToolName};
use core::fmt;
use serde_json::Value;

/// One call, with every argument [ADR-0011] D1's row needs.
///
/// The variants are the four subject kinds rather than the seven tools:
/// `fs.read` and `fs.list` take the same field and differ only in what is
/// done with it, which [`ToolName`] already carries.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// A tool addressed to a single path and nothing else: `fs.list`.
    OnPath {
        /// Which tool.
        tool: ToolName,
        /// The `path` field.
        path: String,
    },
    /// `fs.read`: a path, and optionally which lines.
    Read {
        /// The `path` field.
        path: String,
        /// The `start_line` field: the first line wanted, counting from 1.
        start_line: Option<usize>,
        /// The `line_count` field: how many lines at most.
        line_count: Option<usize>,
    },
    /// `fs.write`: a path and the bytes to put there.
    Write {
        /// The `path` field.
        path: String,
        /// The `contents` field.
        contents: String,
    },
    /// `fs.edit`: a path and the exact string to replace within it.
    Edit {
        /// The `path` field.
        path: String,
        /// The `old` field — replaced exactly, never fuzzily.
        old: String,
        /// The `new` field.
        new: String,
        /// The `all` field: replace every occurrence rather than exactly one.
        /// `false` when the call leaves it out.
        all: bool,
    },
    /// `fs.search`: where to look, what to look for, and how.
    Search {
        /// The `root` field.
        root: String,
        /// The `needle` field.
        needle: String,
        /// The optional fields: `exact_case`, `whole_word`, `file_type` and
        /// `include_ignored`.
        options: crate::tools::searching::Options,
    },
    /// `cmd.run`: the command line, not yet split.
    Run {
        /// The `command` field.
        command: String,
    },
    /// `web.fetch`: the URL.
    Fetch {
        /// The `url` field.
        url: String,
    },
}

impl Call {
    /// Which built-in this is.
    #[must_use]
    pub const fn tool(&self) -> ToolName {
        match self {
            Self::OnPath { tool, .. } => *tool,
            Self::Read { .. } => ToolName::FsRead,
            Self::Write { .. } => ToolName::FsWrite,
            Self::Edit { .. } => ToolName::FsEdit,
            Self::Search { .. } => ToolName::FsSearch,
            Self::Run { .. } => ToolName::CmdRun,
            Self::Fetch { .. } => ToolName::WebFetch,
        }
    }

    /// Read `arguments` as the object `tool` declares.
    ///
    /// # Errors
    ///
    /// [`ArgumentsRefused`] when the text is not JSON, is not an object, is
    /// missing a required field, carries one that is not of its declared kind,
    /// or carries a field the tool did not declare.
    pub fn parse(tool: ToolName, arguments: &str) -> Result<Self, ArgumentsRefused> {
        let value: Value = serde_json::from_str(arguments).map_err(|error| {
            // The error's own `Display` is not used. `serde_json` renders the
            // offending value for a type error -- `invalid type: integer `5``
            // -- and the arguments are where a file's whole contents live.
            // Its line and column are positions rather than content.
            ArgumentsRefused::NotJson {
                tool,
                line: error.line(),
                column: error.column(),
            }
        })?;
        let Value::Object(object) = value else {
            return Err(ArgumentsRefused::NotAnObject {
                tool,
                found: kind_of(&value),
            });
        };

        // Declared first, so a call missing a field is told which one before
        // it is told about a field it should not have sent. A reader fixing
        // one thing at a time fixes the required one first.
        let mut given = Given::default();
        for field in tool.fields() {
            let Some(value) = object.get(field.name) else {
                if field.required {
                    return Err(ArgumentsRefused::MissingField {
                        tool,
                        field: field.name,
                    });
                }
                continue;
            };
            let refused = || ArgumentsRefused::WrongKind {
                tool,
                field: field.name,
                wanted: field.kind,
                found: match value {
                    Value::Number(_) => "a number that is below 1 or not whole",
                    other => kind_of(other),
                },
            };
            match field.kind {
                FieldKind::Text => {
                    let Value::String(text) = value else {
                        return Err(refused());
                    };
                    given.texts.push((field.name, text.clone()));
                }
                FieldKind::Number => {
                    let number = whole_number(value).ok_or_else(refused)?;
                    given.numbers.push((field.name, number));
                }
                FieldKind::Flag => {
                    let Value::Bool(flag) = value else {
                        return Err(refused());
                    };
                    given.flags.push((field.name, *flag));
                }
            }
        }
        // "Exactly the declared fields": a field nobody declared is refused
        // rather than ignored, because a model that spelled `contents` as
        // `content` would otherwise write an empty file and be told it
        // succeeded.
        if let Some(extra) = object
            .keys()
            .find(|key| !tool.fields().iter().any(|field| field.name == key.as_str()))
        {
            return Err(ArgumentsRefused::UnexpectedField {
                tool,
                field: extra.to_string(),
            });
        }

        Ok(match tool.subject_kind() {
            SubjectKind::Path => match tool {
                ToolName::FsRead => Self::Read {
                    path: given.text("path"),
                    start_line: given.number("start_line"),
                    line_count: given.number("line_count"),
                },
                ToolName::FsWrite => Self::Write {
                    path: given.text("path"),
                    contents: given.text("contents"),
                },
                ToolName::FsEdit => Self::Edit {
                    path: given.text("path"),
                    old: given.text("old"),
                    new: given.text("new"),
                    all: given.flag("all").unwrap_or(false),
                },
                _ => Self::OnPath {
                    tool,
                    path: given.text("path"),
                },
            },
            SubjectKind::SearchRoot => Self::Search {
                root: given.text("root"),
                needle: given.text("needle"),
                options: crate::tools::searching::Options {
                    exact_case: given.flag("exact_case").unwrap_or(false),
                    whole_word: given.flag("whole_word").unwrap_or(false),
                    file_type: given.optional_text("file_type"),
                    include_ignored: given.flag("include_ignored").unwrap_or(false),
                },
            },
            SubjectKind::CommandLine => Self::Run {
                command: given.text("command"),
            },
            SubjectKind::Url => Self::Fetch {
                url: given.text("url"),
            },
        })
    }
}

/// The values a call carried, by field name, each already of its declared kind.
#[derive(Default)]
struct Given {
    texts: Vec<(&'static str, String)>,
    numbers: Vec<(&'static str, usize)>,
    flags: Vec<(&'static str, bool)>,
}

impl Given {
    /// A required text field's value.
    ///
    /// # Panics
    ///
    /// When `name` is not a required text field of the tool being parsed,
    /// which is this module asking for a field it did not declare: a defect
    /// here, never anything a model sent.
    fn text(&mut self, name: &str) -> String {
        let at = self
            .texts
            .iter()
            .position(|(field, _)| *field == name)
            .expect("a required text field was taken when it was declared");
        self.texts.swap_remove(at).1
    }

    /// An optional text field's value, if the call carried it.
    fn optional_text(&mut self, name: &str) -> Option<String> {
        let at = self.texts.iter().position(|(field, _)| *field == name)?;
        Some(self.texts.swap_remove(at).1)
    }

    /// An optional flag's value, if the call carried it.
    fn flag(&self, name: &str) -> Option<bool> {
        self.flags
            .iter()
            .find(|(field, _)| *field == name)
            .map(|(_, flag)| *flag)
    }

    /// An optional number field's value, if the call carried it.
    fn number(&self, name: &str) -> Option<usize> {
        self.numbers
            .iter()
            .find(|(field, _)| *field == name)
            .map(|(_, number)| *number)
    }
}

/// A JSON number that is a whole number of 1 or more, as a `usize`.
///
/// **A whole number written with a fraction of zero is taken**, so `10.0` is
/// 10. JSON has one number type, and a provider that carries arguments
/// through a structure of doubles may hand back `10.0` for the 10 its model
/// wrote; refusing that would refuse the model for the provider's spelling.
/// Anything with a real fraction, anything below 1 and anything that is not
/// a number is refused.
fn whole_number(value: &Value) -> Option<usize> {
    let Value::Number(number) = value else {
        return None;
    };
    let whole = number.as_u64().or_else(|| {
        number
            .as_f64()
            .filter(|float| float.fract() == 0.0 && *float >= 1.0 && *float <= 9.0e15)
            .map(|float| {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "checked above: a whole number from 1 to 9e15"
                )]
                let whole = float as u64;
                whole
            })
    })?;
    usize::try_from(whole).ok().filter(|whole| *whole >= 1)
}

/// What a JSON value is, for a refusal that must not render it.
const fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Why a request's arguments are not a call this surface can make.
///
/// Every variant names a field and never a value. See the module
/// documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentsRefused {
    /// The text is not JSON at all.
    NotJson {
        /// Which built-in was asked for.
        tool: ToolName,
        /// Where the parser stopped.
        line: usize,
        /// Where the parser stopped.
        column: usize,
    },
    /// The text is JSON and is not an object.
    NotAnObject {
        /// Which built-in was asked for.
        tool: ToolName,
        /// What arrived instead.
        found: &'static str,
    },
    /// A field the tool declares is absent.
    MissingField {
        /// Which built-in was asked for.
        tool: ToolName,
        /// The field that is missing.
        field: &'static str,
    },
    /// A declared field arrived as something other than its declared kind.
    WrongKind {
        /// Which built-in was asked for.
        tool: ToolName,
        /// The field.
        field: &'static str,
        /// What the field holds.
        wanted: FieldKind,
        /// What arrived instead.
        found: &'static str,
    },
    /// A field the tool does not declare arrived.
    UnexpectedField {
        /// Which built-in was asked for.
        tool: ToolName,
        /// The field's name. Never its value.
        field: String,
    },
}

impl fmt::Display for ArgumentsRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson { tool, line, column } => write!(
                f,
                "the arguments to {tool} are not JSON: the parser stopped at line {line} column \
                 {column}. Every built-in takes one JSON object; {tool} takes {}",
                declared(*tool)
            ),
            Self::NotAnObject { tool, found } => write!(
                f,
                "the arguments to {tool} are {found} rather than a JSON object. {tool} takes {}",
                declared(*tool)
            ),
            Self::MissingField { tool, field } => write!(
                f,
                "the arguments to {tool} carry no {field:?} field, so there is nothing to address \
                 the call to. {tool} takes {}",
                declared(*tool)
            ),
            Self::WrongKind {
                tool,
                field,
                wanted,
                found,
            } => write!(
                f,
                "the {field:?} field of {tool} is {found} rather than {}. {tool} takes {}",
                wanted.described(),
                declared(*tool)
            ),
            Self::UnexpectedField { tool, field } => write!(
                f,
                "the arguments to {tool} carry a {field:?} field, which {tool} does not take. A \
                 field nobody declared is refused rather than ignored, because a misspelled one \
                 would otherwise be silently dropped and the call reported as having succeeded. \
                 {tool} takes {}",
                declared(*tool)
            ),
        }
    }
}

impl std::error::Error for ArgumentsRefused {}

/// The fields a tool declares, as a refusal names them.
///
/// Every required field is text, so they are named together; an optional one
/// is named with what it holds.
fn declared(tool: ToolName) -> String {
    let required: Vec<String> = tool
        .fields()
        .iter()
        .filter(|field| field.required)
        .map(|field| format!("{:?}", field.name))
        .collect();
    let optional: Vec<String> = tool
        .fields()
        .iter()
        .filter(|field| !field.required)
        .map(|field| format!("{:?} ({})", field.name, field.kind.described()))
        .collect();
    let mut said = format!("exactly {}, each a string", required.join(" and "));
    if !optional.is_empty() {
        said.push_str(&format!(", and may also carry {}", optional.join(" and ")));
    }
    said
}

/// The JSON Schema a tool is offered under.
///
/// # It is derived, never retyped
///
/// Built from [`ToolName::fields`], so the schema a model is shown and the
/// object [`Call::parse`] will accept are **one list walked twice** rather than
/// two lists that can disagree. A field added to `fields` appears in the schema
/// in the same edit.
///
/// # There are no field descriptions, and that is deliberate
///
/// [ADR-0011] D1 gives each tool one sentence and says nothing about any
/// field. Since 2026-09-28 the tool's own description says what the optional
/// fields of `fs.read` do, in the same few sentences, rather than a second
/// sentence per field: every byte of this schema is sent on every request
/// and counted against the context window, and an `ollama` window is 4,096.
///
/// `additionalProperties` is `false`, which is the schema saying what
/// [`Call::parse`] enforces. A number field says `minimum: 1`, which is the
/// schema saying what [`Call::parse`] enforces for a number.
///
/// [ADR-0011]: https://100monkeys-ai.cortex.page/zaru/p/adrs/0011-local-tool-surface
#[must_use]
pub fn schema(tool: ToolName) -> String {
    let properties: serde_json::Map<String, Value> = tool
        .fields()
        .iter()
        .map(|field| {
            let property = match field.kind {
                FieldKind::Text => serde_json::json!({ "type": "string" }),
                FieldKind::Number => serde_json::json!({ "type": "integer", "minimum": 1 }),
                FieldKind::Flag => serde_json::json!({ "type": "boolean" }),
            };
            (field.name.to_owned(), property)
        })
        .collect();
    let required: Vec<&str> = tool
        .fields()
        .iter()
        .filter(|field| field.required)
        .map(|field| field.name)
        .collect();
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
    .to_string()
}

#[cfg(test)]
mod tests;
