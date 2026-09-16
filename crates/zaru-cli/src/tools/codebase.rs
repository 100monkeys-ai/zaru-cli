// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Local structural retrieval for [`super::files::search`].
//!
//! This is intentionally an in-memory index, not a cache and not an embedding
//! store. It is built from bytes the search has already been permitted to read
//! and dropped with the call. The extractors are explicit: a file is either a
//! supported declaration grammar or ordinary text. Calling a line-oriented
//! declaration extractor an AST would be dishonest; it is the portable
//! structural layer on which language AST adapters can be added without
//! changing the filesystem boundary or result contract.

use std::path::Path;

/// One grounded declaration available to a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Symbol {
    path: String,
    line: usize,
    kind: &'static str,
    name: String,
    signature: String,
    context: String,
}

impl Symbol {
    fn render(&self) -> String {
        format!(
            "symbol: {}:{}: {} {} — {}",
            self.path, self.line, self.kind, self.name, self.context
        )
    }
}

/// Add the declarations in one supported source file to `symbols`.
pub(crate) fn collect(path: &Path, text: &str, symbols: &mut Vec<Symbol>) {
    let Some(language) = path.extension().and_then(|extension| extension.to_str()) else {
        return;
    };
    if !matches!(
        language,
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "java"
    ) {
        return;
    }
    for (offset, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let declaration = match language {
            "rs" => rust_declaration(line),
            "py" => python_declaration(line),
            "js" | "jsx" | "ts" | "tsx" => javascript_declaration(line),
            "go" => go_declaration(line),
            "java" => java_declaration(line),
            _ => None,
        };
        if let Some((kind, name)) = declaration {
            let signature = compact(line);
            let context = nearby_context(text, offset, &signature);
            symbols.push(Symbol {
                path: path.display().to_string(),
                line: offset + 1,
                kind,
                name: name.to_owned(),
                signature,
                context,
            });
        }
    }
}

/// Return bounded, deterministic structural retrieval results.
pub(crate) fn retrieve(symbols: &[Symbol], query: &str) -> Vec<String> {
    const LIMIT: usize = 12;
    let terms = terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(usize, &Symbol)> = symbols
        .iter()
        .filter_map(|symbol| score(symbol, &terms).map(|score| (score, symbol)))
        .collect();
    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.name.cmp(&right.name))
    });
    ranked
        .into_iter()
        .take(LIMIT)
        .map(|(_, symbol)| symbol.render())
        .collect()
}

fn score(symbol: &Symbol, query: &[String]) -> Option<usize> {
    let name = terms(&symbol.name);
    let signature = terms(&symbol.signature);
    let context = terms(&symbol.context);
    let path = terms(&symbol.path);
    let mut score = 0;
    for term in query {
        let in_name = name.iter().any(|value| value == term);
        let in_signature = signature.iter().any(|value| value == term);
        let in_context = context.iter().any(|value| value == term);
        let in_path = path.iter().any(|value| value == term);
        if !(in_name || in_signature || in_context || in_path) {
            return None;
        }
        score += if in_name {
            16
        } else if in_signature {
            8
        } else if in_context {
            4
        } else {
            2
        };
    }
    Some(score)
}

fn terms(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut previous_lower = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase() && previous_lower && !word.is_empty() {
                words.push(word.to_ascii_lowercase());
                word.clear();
            }
            previous_lower = character.is_ascii_lowercase();
            word.push(character);
        } else {
            if !word.is_empty() {
                words.push(word.to_ascii_lowercase());
                word.clear();
            }
            previous_lower = false;
        }
    }
    if !word.is_empty() {
        words.push(word.to_ascii_lowercase());
    }
    words.sort();
    words.dedup();
    words
}

fn nearby_context(text: &str, declaration_line: usize, signature: &str) -> String {
    const CONTEXT_LINES: usize = 3;
    const CONTEXT_BYTES: usize = 240;
    let context = text
        .lines()
        .skip(declaration_line.saturating_sub(CONTEXT_LINES))
        .take(CONTEXT_LINES * 2 + 1)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let context = if context.is_empty() {
        signature.to_owned()
    } else {
        context
    };
    context.chars().take(CONTEXT_BYTES).collect()
}

fn compact(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn after<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix).and_then(identifier)
}
fn identifier(value: &str) -> Option<&str> {
    let end = value
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_alphanumeric() || *character == '_')
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    (end > 0).then_some(&value[..end])
}
fn rust_declaration(line: &str) -> Option<(&'static str, &str)> {
    for (prefix, kind) in [
        ("pub async fn ", "function"),
        ("pub fn ", "function"),
        ("async fn ", "function"),
        ("fn ", "function"),
        ("pub struct ", "struct"),
        ("struct ", "struct"),
        ("pub enum ", "enum"),
        ("enum ", "enum"),
        ("pub trait ", "trait"),
        ("trait ", "trait"),
        ("pub mod ", "module"),
        ("mod ", "module"),
        ("pub type ", "type"),
        ("type ", "type"),
        ("impl ", "implementation"),
    ] {
        if let Some(name) = after(line, prefix) {
            return Some((kind, name));
        }
    }
    None
}
fn python_declaration(line: &str) -> Option<(&'static str, &str)> {
    after(line, "async def ")
        .map(|name| ("function", name))
        .or_else(|| after(line, "def ").map(|name| ("function", name)))
        .or_else(|| after(line, "class ").map(|name| ("class", name)))
}
fn javascript_declaration(line: &str) -> Option<(&'static str, &str)> {
    for (prefix, kind) in [
        ("export async function ", "function"),
        ("export function ", "function"),
        ("async function ", "function"),
        ("function ", "function"),
        ("export class ", "class"),
        ("class ", "class"),
        ("export interface ", "interface"),
        ("interface ", "interface"),
        ("export type ", "type"),
        ("type ", "type"),
    ] {
        if let Some(name) = after(line, prefix) {
            return Some((kind, name));
        }
    }
    None
}
fn go_declaration(line: &str) -> Option<(&'static str, &str)> {
    after(line, "func ")
        .map(|name| ("function", name))
        .or_else(|| after(line, "type ").map(|name| ("type", name)))
}
fn java_declaration(line: &str) -> Option<(&'static str, &str)> {
    for (prefix, kind) in [
        ("public class ", "class"),
        ("class ", "class"),
        ("public interface ", "interface"),
        ("interface ", "interface"),
        ("public enum ", "enum"),
        ("enum ", "enum"),
    ] {
        if let Some(name) = after(line, prefix) {
            return Some((kind, name));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{collect, retrieve};
    use std::path::Path;
    #[test]
    fn a_normalised_concept_query_returns_a_grounded_rust_declaration() {
        let mut symbols = Vec::new();
        collect(
            Path::new("src/turn_clock.rs"),
            "/// Refreshes elapsed time during tool calls.\npub async fn refreshTurnClock() {}",
            &mut symbols,
        );
        assert_eq!(
            retrieve(&symbols, "turn_clock tool calls"),
            vec![
                "symbol: src/turn_clock.rs:2: function refreshTurnClock — /// Refreshes elapsed time during tool calls. pub async fn refreshTurnClock() {}"
            ]
        );
    }
    #[test]
    fn ranking_is_deterministic_and_bounded() {
        let mut symbols = Vec::new();
        for number in 0..20 {
            collect(
                Path::new("src/search.rs"),
                &format!("fn search_{number}() {{}}"),
                &mut symbols,
            );
        }
        let first = retrieve(&symbols, "search");
        assert_eq!(first, retrieve(&symbols, "search"));
        assert_eq!(first.len(), 12);
    }
}
