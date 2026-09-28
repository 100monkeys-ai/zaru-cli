// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Where a window comes from, and how each kind's answer is read.
//!
//! No check here is answered by a provider. The answers are the shapes each
//! provider documents, written out here; the one check that reaches a socket
//! reaches a listener that answers every question with HTTP 500, so what it
//! reads is the question on the wire and never a window.

use super::{Question, Source, Window, ask, decide};

/// The three sources, each where it is the one used.
///
/// Watched red with `decide` ignoring the provider's answer: "the provider
/// answered 1048576 and the window came from BuiltIn"; and with `ollama`'s
/// answer taken as the window: "ollama's trained length raised the window to
/// 131072 where the server is told 4096".
#[test]
fn a_window_comes_from_the_provider_the_configuration_or_the_default() {
    let window = |said, configured, built_in, ceiling| {
        decide(said, configured, built_in, ceiling).expect("something gave a figure")
    };

    // `gemini`: the provider answered, nothing configured.
    let provider = window(Some(1_048_576), None, Some(1_048_576), false);
    assert_eq!(
        (provider.tokens, provider.source),
        (1_048_576, Source::Provider),
        "the provider answered 1048576 and the window came from {:?}",
        provider.source
    );
    // A reader who set less is compacted sooner; one who set more is not
    // given a window the provider refuses.
    assert_eq!(
        window(Some(1_048_576), Some(200_000), None, false).source,
        Source::Configured
    );
    assert_eq!(
        window(Some(32_768), Some(200_000), None, false),
        Window {
            tokens: 32_768,
            source: Source::Provider,
            provider_said: Some(32_768)
        }
    );
    // Nobody answered.
    assert_eq!(
        window(None, Some(8_000), None, false).source,
        Source::Configured
    );
    assert_eq!(
        window(None, None, Some(4_096), false).source,
        Source::BuiltIn
    );
    // `openai-compatible` with no answer and nothing set has no window.
    assert_eq!(decide(None, None, None, false), None);

    // `ollama`: the trained length is a ceiling on what the server is told.
    let ollama = window(Some(131_072), None, Some(4_096), true);
    assert_eq!(
        (ollama.tokens, ollama.source),
        (4_096, Source::BuiltIn),
        "ollama's trained length raised the window to {} where the server is told 4096",
        ollama.tokens
    );
    assert_eq!(
        window(Some(2_048), None, Some(4_096), true),
        Window {
            tokens: 2_048,
            source: Source::Provider,
            provider_said: Some(2_048)
        },
        "a model trained on less than the default lowers the window to its own figure"
    );
    assert_eq!(
        window(Some(131_072), Some(16_384), Some(4_096), true).source,
        Source::Configured
    );
}

/// What `zaru models` prints for each source.
#[test]
fn a_window_says_where_it_came_from() {
    let said = |window: Window| window.to_string();
    assert_eq!(
        said(Window {
            tokens: 1_048_576,
            source: Source::Provider,
            provider_said: Some(1_048_576)
        }),
        "1048576 tokens, from the provider"
    );
    assert_eq!(
        said(Window {
            tokens: 4_096,
            source: Source::BuiltIn,
            provider_said: Some(131_072)
        }),
        "4096 tokens, this build's default (the provider says the model allows 131072)"
    );
    assert_eq!(
        said(Window {
            tokens: 8_000,
            source: Source::Configured,
            provider_said: None
        }),
        "8000 tokens, from your configuration (the provider was asked and did not say)"
    );
}

/// Each kind's answer is read from the field its API documents.
///
/// The bodies are the documented shapes, trimmed: Ollama's `show`
/// (`model_info` keyed by architecture), Gemini's model resource
/// (`inputTokenLimit`), and an OpenAI-shaped models list with vLLM's
/// `max_model_len` on one entry and `context_length` on another.
#[test]
fn each_kinds_answer_is_read_from_its_own_field() {
    let show = serde_json::json!({
        "model_info": {"general.architecture": "llama", "llama.context_length": 131072}
    });
    assert_eq!(
        Question::ollama_show("http://h", "llama3.2:3b").read(&show),
        Some(131_072)
    );
    let other_architecture = serde_json::json!({
        "model_info": {"qwen2.context_length": 32768}
    });
    assert_eq!(
        Question::ollama_show("http://h", "qwen").read(&other_architecture),
        Some(32_768)
    );

    let gemini = serde_json::json!({
        "name": "models/gemini-3.6-flash", "inputTokenLimit": 1048576, "outputTokenLimit": 65536
    });
    assert_eq!(
        Question::gemini_model("https://h", "v1beta", "gemini-3.6-flash", "k").read(&gemini),
        Some(1_048_576)
    );

    let models = serde_json::json!({
        "object": "list",
        "data": [
            {"id": "other", "max_model_len": 8192},
            {"id": "served", "max_model_len": 32768},
            {"id": "routed", "context_length": 200000}
        ]
    });
    assert_eq!(
        Question::openai_models("http://h/v1", "served", None).read(&models),
        Some(32_768)
    );
    assert_eq!(
        Question::openai_models("http://h/v1", "routed", None).read(&models),
        Some(200_000)
    );
    assert_eq!(
        Question::openai_models("http://h/v1", "absent", None).read(&models),
        None,
        "a model the list does not carry has no window from it"
    );
}

/// A question nobody answers is not an error: there is no window from it.
#[test]
fn a_question_to_a_closed_port_gives_no_window() {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
    let port = listener.local_addr().expect("an address").port();
    drop(listener);
    assert_eq!(
        ask(Question::ollama_show(
            &format!("http://127.0.0.1:{port}"),
            "llama3.2:3b"
        )),
        None
    );
}

/// The `ollama` question is `POST /api/show` naming the model, and a refusal
/// gives no window.
///
/// The listener answers HTTP 500 to the one request it takes, so nothing
/// here is a provider's answer: what is read is the question.
#[test]
fn the_ollama_question_names_the_model_to_show() {
    use std::io::{Read as _, Write as _};

    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("loopback accepts a bind on port 0");
    let origin = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("an address").port()
    );
    let (tell, seen) = std::sync::mpsc::channel::<String>();
    let keeper = std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(core::time::Duration::from_secs(5)));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        while let Ok(got) = stream.read(&mut buffer) {
            if got == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..got]);
            let text = String::from_utf8_lossy(&request).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let _ = stream.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        );
        let _ = tell.send(String::from_utf8_lossy(&request).into_owned());
    });

    let answered = ask(Question::ollama_show(&origin, "llama3.2:3b"));
    let request = seen
        .recv_timeout(core::time::Duration::from_secs(10))
        .expect("the question reached the listener");
    keeper.join().expect("the listener ends");
    assert!(
        request.starts_with("POST /api/show ") && request.contains(r#""model":"llama3.2:3b""#),
        "the ollama question is not a show of the model: {request}"
    );
    assert_eq!(answered, None, "a refused question gives no window");
}
