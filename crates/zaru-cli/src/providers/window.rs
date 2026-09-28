// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a session's context window comes from: the provider, the reader's
//! configuration, or this build's default, and which of the three it was.
//!
//! # Asked once, at the start of a session
//!
//! Each of the three kinds with a client can be asked about a model:
//!
//! | Kind | Question | Field read |
//! | --- | --- | --- |
//! | `gemini` | `GET /v1beta/models/<model>` | `inputTokenLimit` |
//! | `ollama` | `POST /api/show` with the model's name | `model_info["<architecture>.context_length"]` |
//! | `openai-compatible` | `GET <endpoint>/models` | the entry whose `id` is the model: `max_model_len` (vLLM), else `context_length`, else `context_window` |
//!
//! The question is asked once when a session is prepared, on its own thread
//! with its own runtime so it can be asked from inside a running session as
//! well as before one, and it is bounded by [`QUESTION_TIMEOUT`]. A question
//! that fails, is refused, or is answered with no figure is not an error: the
//! window then comes from the configuration, or from this build's default,
//! and `zaru models` says which.
//!
//! # `ollama`'s answer is a ceiling, not the window
//!
//! Ollama's `show` reports what the model was **trained** with — 131,072 for
//! `llama3.2:3b` — and its server serves `num_ctx`, which is 4,096 unless the
//! request says otherwise. This client says otherwise on every request: it
//! sends the window it uses as `num_ctx`. Taking the trained figure as the
//! window would have the server hold a cache of 131,072 tokens for every
//! request, several gigabytes on the machine the harness runs on. So for this
//! kind the window is the configured value or the default, **lowered** to the
//! model's own figure where that is smaller, and `zaru models` names the
//! model's figure so a reader who wants more knows how much there is.
//!
//! # A reader's own setting lowers a provider's answer and never raises it
//!
//! Where the provider answered and the reader also set
//! `provider.<kind>.context_tokens`, the smaller is used. A reader who set a
//! smaller window asked to be compacted sooner, which is theirs to ask; a
//! larger one than the provider serves is a window the provider refuses.

use core::fmt;
use core::time::Duration;

/// How long the window question may take before the session goes on without
/// an answer.
///
/// Short, because it is asked while a person waits for a session to open,
/// and a server that does not answer this in ten seconds has told the
/// harness nothing it can use.
pub const QUESTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Which of the three a window came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The provider said so, when asked about the model.
    Provider,
    /// The reader set `provider.<kind>.context_tokens`.
    Configured,
    /// This build's default for the kind.
    BuiltIn,
}

impl Source {
    /// The words `zaru models` uses for it.
    #[must_use]
    pub const fn said(self) -> &'static str {
        match self {
            Self::Provider => "from the provider",
            Self::Configured => "from your configuration",
            Self::BuiltIn => "this build's default",
        }
    }
}

/// A window, where it came from, and what the provider said about the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Tokens.
    pub tokens: u64,
    /// Where `tokens` came from.
    pub source: Source,
    /// What the provider answered, when it answered, even where it is not the
    /// figure used.
    pub provider_said: Option<u64>,
}

impl fmt::Display for Window {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} tokens, {}", self.tokens, self.source.said())?;
        match (self.source, self.provider_said) {
            (Source::Provider, _) => Ok(()),
            (_, Some(said)) => write!(f, " (the provider says the model allows {said})"),
            (_, None) => write!(f, " (the provider was asked and did not say)"),
        }
    }
}

/// Decide a kind's window from what the provider said, what the reader
/// configured, and this build's default.
///
/// `ceiling_only` is `true` for a kind whose answer is the model's trained
/// length rather than what its server serves (`ollama`; see the module
/// documentation). `None` when nothing gave a figure, which for
/// `openai-compatible` is the refusal `require_context_size` already raises.
#[must_use]
pub fn decide(
    provider_said: Option<u64>,
    configured: Option<u64>,
    built_in: Option<u64>,
    ceiling_only: bool,
) -> Option<Window> {
    let provider_said = provider_said.filter(|tokens| *tokens > 0);
    let wanted = match (configured, built_in) {
        (Some(tokens), _) => Some((tokens, Source::Configured)),
        (None, Some(tokens)) => Some((tokens, Source::BuiltIn)),
        (None, None) => None,
    };
    let (tokens, source) = match (provider_said, wanted) {
        (Some(said), Some((tokens, source))) if ceiling_only => {
            if said < tokens {
                (said, Source::Provider)
            } else {
                (tokens, source)
            }
        }
        (Some(said), Some((tokens, Source::Configured))) if tokens < said => {
            (tokens, Source::Configured)
        }
        (Some(said), _) => (said, Source::Provider),
        (None, Some((tokens, source))) => (tokens, source),
        (None, None) => return None,
    };
    Some(Window {
        tokens,
        source,
        provider_said,
    })
}

/// One question to a provider about a model's window, owned so it can be
/// asked on its own thread.
pub struct Question {
    url: String,
    body: Option<serde_json::Value>,
    /// A header to send, where the kind needs its key on the question too.
    header: Option<(&'static str, String)>,
    reading: Reading,
}

impl fmt::Debug for Question {
    /// Names where the question goes and never the header's value, which can
    /// be a key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Question")
            .field("url", &self.url)
            .field("reading", &self.reading)
            .finish_non_exhaustive()
    }
}

/// How an answer is read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reading {
    OllamaShow,
    GeminiModel,
    OpenAiModels { model: String },
}

impl Question {
    /// Ollama's `POST /api/show` for `model`.
    #[must_use]
    pub fn ollama_show(origin: &str, model: &str) -> Self {
        Self {
            url: format!("{origin}/api/show"),
            body: Some(serde_json::json!({ "model": model })),
            header: None,
            reading: Reading::OllamaShow,
        }
    }

    /// Gemini's model description, with the key in its header.
    #[must_use]
    pub fn gemini_model(origin: &str, api_version: &str, model: &str, key: &str) -> Self {
        Self {
            url: format!("{origin}/{api_version}/models/{model}"),
            body: None,
            header: Some((crate::providers::gemini::API_KEY_HEADER, key.to_owned())),
            reading: Reading::GeminiModel,
        }
    }

    /// An OpenAI-compatible server's models list, with the key as a bearer
    /// where one is held.
    #[must_use]
    pub fn openai_models(origin: &str, model: &str, bearer: Option<&str>) -> Self {
        Self {
            url: format!("{origin}/models"),
            body: None,
            header: bearer.map(|key| ("authorization", format!("Bearer {key}"))),
            reading: Reading::OpenAiModels {
                model: model.to_owned(),
            },
        }
    }

    /// Where the question goes.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Read an answer's body.
    #[must_use]
    pub fn read(&self, answer: &serde_json::Value) -> Option<u64> {
        match &self.reading {
            Reading::OllamaShow => ollama_show_window(answer),
            Reading::GeminiModel => answer.get("inputTokenLimit")?.as_u64(),
            Reading::OpenAiModels { model } => openai_models_window(answer, model),
        }
    }
}

/// The trained context length in an Ollama `show` answer.
///
/// `model_info` keys the figure by architecture — `llama.context_length`,
/// `qwen2.context_length` — so the key is read off `general.architecture`,
/// and failing that any key ending `.context_length`.
fn ollama_show_window(answer: &serde_json::Value) -> Option<u64> {
    let info = answer.get("model_info")?.as_object()?;
    if let Some(architecture) = info.get("general.architecture").and_then(|v| v.as_str())
        && let Some(tokens) = info
            .get(&format!("{architecture}.context_length"))
            .and_then(serde_json::Value::as_u64)
    {
        return Some(tokens);
    }
    info.iter()
        .find(|(key, _)| key.ends_with(".context_length"))
        .and_then(|(_, value)| value.as_u64())
}

/// The window of `model` in an OpenAI-shaped models list.
fn openai_models_window(answer: &serde_json::Value, model: &str) -> Option<u64> {
    let entry = answer
        .get("data")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(model))?;
    ["max_model_len", "context_length", "context_window"]
        .iter()
        .find_map(|field| entry.get(*field).and_then(serde_json::Value::as_u64))
}

/// Ask `question` and read its answer, or `None` for any failure.
///
/// On a thread of its own, through [`crate::failure::thread`], with a
/// current-thread runtime of its own: the question is asked while a session
/// is being prepared, and that is sometimes inside a running session's
/// runtime, where a second `block_on` on the same thread is refused.
#[must_use]
pub fn ask(question: Question) -> Option<u64> {
    let asking = crate::failure::thread("window-question", move || {
        let runtime = crate::compose::turn::runtime().ok()?;
        runtime.block_on(async move {
            let http =
                crate::web::client::build(QUESTION_TIMEOUT, reqwest::redirect::Policy::default())
                    .ok()?;
            let mut sending = match &question.body {
                Some(body) => http.post(&question.url).json(body),
                None => http.get(&question.url),
            };
            if let Some((name, value)) = &question.header {
                sending = sending.header(*name, value);
            }
            let response = sending.send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            let answer: serde_json::Value = response.json().await.ok()?;
            question.read(&answer)
        })
    })
    .ok()?;
    asking.join().ok().flatten()
}

#[cfg(test)]
mod tests;
