// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The conversation a model is sent: who said what, in order.
//!
//! # One shape for the turn in flight and for every turn before it
//!
//! A model provider's API is stateless. Each request carries the whole
//! conversation: what the person asked, what the model answered, every tool
//! call the model made with its arguments, and every result it was given.
//! This module is that conversation as data, in the roles every provider this
//! harness speaks to defines: a person's message, the model's message, and a
//! tool's result.
//!
//! The same [`Message`] carries the turn in flight
//! ([`crate::tool_call::ModelRequest::turn`]) and every earlier turn
//! ([`crate::context`]'s layer 6), so a result the model read one turn ago is
//! sent back to it in the same shape it was sent the first time.
//!
//! # Nothing a person is shown is in here
//!
//! The harness paints a great deal for a person while a turn runs: status
//! lines, byte counts, permission decisions, notices. None of it is a
//! message. What is here is what the model said, what the tools returned to
//! the model, and what the person typed.
//!
//! # Every text in here has passed the redactor
//!
//! [`crate::tool_call::machine::run`] builds every message from redacted
//! text: the task, the model's own text and arguments, and each result, which
//! is already a [`Redacted`] on its way out of the
//! executor. A message read back from a transcript is redacted again on its
//! way into a prompt by [`crate::iteration::Prompt::assembled`], because the
//! values a harness holds can change between the turn that wrote it and the
//! turn that reads it.

use crate::redaction::{Redacted, Redactor};
use crate::tool_call::port::{ToolRequest, ToolResult};
use serde::{Deserialize, Serialize};

/// One message of the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "role", deny_unknown_fields)]
pub enum Message {
    /// What the person asked.
    User {
        /// The text, as the model is given it.
        text: String,
    },
    /// What the model said: its text and the tool calls it asked for.
    Assistant {
        /// The model's text. Empty when it only asked for tools.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        /// The calls it asked for, in the order it asked.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        calls: Vec<ToolRequest>,
        /// The provider's own record of this message, handed back to that
        /// provider unchanged.
        ///
        /// Opaque to this crate. One provider requires parts of a model's
        /// message back exactly as it sent them (Gemini's thought signatures),
        /// and only the client that received them can say what they are.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        echo: Option<String>,
    },
    /// What one tool call returned to the model.
    Tool {
        /// The id of the call this answers, as the provider gave it.
        id: String,
        /// The tool's name. Two providers name a result by its tool.
        name: String,
        /// What the model was given, exactly: after redaction and after any
        /// cut the output budget made.
        content: String,
        /// Whether the tool reported a failure.
        failed: bool,
    },
}

impl Message {
    /// The person's message, redacted.
    #[must_use]
    pub fn user<R: Redactor + ?Sized>(redactor: &R, text: &str) -> Self {
        Self::User {
            text: Redacted::by(redactor, text).as_str().to_owned(),
        }
    }

    /// The model's message, with its text and every argument redacted.
    #[must_use]
    pub fn assistant<R: Redactor + ?Sized>(
        redactor: &R,
        text: &str,
        calls: &[ToolRequest],
        echo: Option<String>,
    ) -> Self {
        Self::Assistant {
            text: Redacted::by(redactor, text).as_str().to_owned(),
            calls: calls
                .iter()
                .map(|call| ToolRequest {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: Redacted::by(redactor, &call.arguments).as_str().to_owned(),
                })
                .collect(),
            echo: echo.map(|echo| Redacted::by(redactor, &echo).as_str().to_owned()),
        }
    }

    /// What one call returned, as the model was given it.
    ///
    /// The content is already [`Redacted`]; that is the type the executor
    /// hands back.
    #[must_use]
    pub fn result(name: &str, result: &ToolResult) -> Self {
        Self::Tool {
            id: result.id.clone(),
            name: name.to_owned(),
            content: result.content.as_str().to_owned(),
            failed: result.failed,
        }
    }

    /// The same message with every text passed through `redactor` again.
    #[must_use]
    pub fn redacted<R: Redactor + ?Sized>(&self, redactor: &R) -> Self {
        let again = |text: &str| Redacted::by(redactor, text).as_str().to_owned();
        match self {
            Self::User { text } => Self::User { text: again(text) },
            Self::Assistant { text, calls, echo } => Self::Assistant {
                text: again(text),
                calls: calls
                    .iter()
                    .map(|call| ToolRequest {
                        id: call.id.clone(),
                        name: call.name.clone(),
                        arguments: again(&call.arguments),
                    })
                    .collect(),
                echo: echo.as_deref().map(again),
            },
            Self::Tool {
                id,
                name,
                content,
                failed,
            } => Self::Tool {
                id: id.clone(),
                name: name.clone(),
                content: again(content),
                failed: *failed,
            },
        }
    }

    /// The message as plain text, for measuring.
    ///
    /// **This is not what a provider is sent.** Each provider maps a
    /// [`Message`] to its own wire shape. This is one rendering, used where a
    /// length has to be counted, so that every byte a message carries is
    /// counted once: the text, each call's name and arguments, and a result's
    /// content.
    #[must_use]
    pub fn rendered(&self) -> String {
        match self {
            Self::User { text } => text.clone(),
            Self::Assistant { text, calls, .. } => {
                let mut out = text.clone();
                for call in calls {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&call.name);
                    out.push(' ');
                    out.push_str(&call.arguments);
                }
                out
            }
            Self::Tool { content, .. } => content.clone(),
        }
    }
}

/// Close every call that has no result, with `because` as its result.
///
/// A provider refuses a conversation in which a tool call has no answer, and
/// a model given one would not know whether the call ran. A call is left
/// without a result when the turn that asked for it was interrupted, or the
/// process died while it ran. This gives it the result `because` says,
/// marked as a failure, placed directly after the other results of the same
/// message, so the conversation stays in the order a provider requires.
#[must_use]
pub fn closed(messages: Vec<Message>, because: &str) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    let mut open: Vec<(String, String)> = Vec::new();
    for message in messages {
        match &message {
            Message::Tool { id, .. } => {
                if let Some(at) = open.iter().position(|(open_id, _)| open_id == id) {
                    open.remove(at);
                }
            }
            Message::User { .. } | Message::Assistant { .. } => {
                close_all(&mut out, &mut open, because);
            }
        }
        if let Message::Assistant { calls, .. } = &message {
            open.extend(
                calls
                    .iter()
                    .map(|call| (call.id.clone(), call.name.clone())),
            );
        }
        out.push(message);
    }
    close_all(&mut out, &mut open, because);
    out
}

fn close_all(out: &mut Vec<Message>, open: &mut Vec<(String, String)>, because: &str) {
    for (id, name) in open.drain(..) {
        out.push(Message::Tool {
            id,
            name,
            content: because.to_owned(),
            failed: true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str) -> ToolRequest {
        ToolRequest {
            id: id.to_owned(),
            name: "fs.read".to_owned(),
            arguments: "{}".to_owned(),
        }
    }

    fn answered(id: &str) -> Message {
        Message::Tool {
            id: id.to_owned(),
            name: "fs.read".to_owned(),
            content: "bytes".to_owned(),
            failed: false,
        }
    }

    #[test]
    fn a_call_with_no_result_is_closed_where_its_results_belong() {
        let messages = vec![
            Message::User {
                text: "go".to_owned(),
            },
            Message::Assistant {
                text: String::new(),
                calls: vec![call("a"), call("b")],
                echo: None,
            },
            answered("a"),
            Message::User {
                text: "next".to_owned(),
            },
        ];
        let closed = closed(messages, "it did not complete");
        let shape: Vec<String> = closed
            .iter()
            .map(|message| match message {
                Message::User { text } => format!("user {text}"),
                Message::Assistant { calls, .. } => format!("assistant {}", calls.len()),
                Message::Tool {
                    id,
                    content,
                    failed,
                    ..
                } => format!("tool {id} {content} {failed}"),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "user go",
                "assistant 2",
                "tool a bytes false",
                "tool b it did not complete true",
                "user next",
            ],
            "the call with no result must be answered directly after its sibling's result, and \
             before the next message"
        );
    }

    #[test]
    fn a_conversation_whose_calls_all_have_results_is_unchanged() {
        let messages = vec![
            Message::Assistant {
                text: String::new(),
                calls: vec![call("a")],
                echo: None,
            },
            answered("a"),
        ];
        assert_eq!(closed(messages.clone(), "unused"), messages);
    }
}
