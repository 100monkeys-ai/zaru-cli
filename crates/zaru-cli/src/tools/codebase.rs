// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Parser-backed local code retrieval for [`super::files::search`].
//!
//! Every structural fact here comes from a Tree-sitter concrete syntax tree.
//! A source file that does not parse is not given a guessed declaration.

use std::path::Path;
use tree_sitter::{Language, Node, Parser};

/// A citable declaration, import, or reference from a concrete syntax tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Symbol {
    path: String,
    line: usize,
    kind: &'static str,
    name: String,
    scope: String,
    context: String,
}

impl Symbol {
    fn render(&self) -> String {
        let scope = (!self.scope.is_empty()).then(|| format!(" in {}", self.scope));
        format!(
            "symbol: {}:{}: {} {}{} — {}",
            self.path,
            self.line,
            self.kind,
            self.name,
            scope.unwrap_or_default(),
            self.context
        )
    }
}

/// Parse one supported file and collect only tree-derived facts.
pub(crate) fn collect(path: &Path, text: &str, symbols: &mut Vec<Symbol>) {
    let Some(language) = language_for(path) else {
        return;
    };
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return;
    }
    let Some(tree) = parser.parse(text, None) else {
        return;
    };
    if tree.root_node().has_error() {
        return;
    }
    visit(tree.root_node(), text, path, "", symbols);
}

/// Return the twelve highest-scoring deterministic structural results.
pub(crate) fn retrieve(symbols: &[Symbol], query: &str) -> Vec<String> {
    let query = terms(query);
    if query.is_empty() {
        return Vec::new();
    }
    let mut ranked: Vec<(usize, &Symbol)> = symbols
        .iter()
        .filter_map(|symbol| score(symbol, &query).map(|score| (score, symbol)))
        .collect();
    ranked.sort_by(|(ls, left), (rs, right)| {
        rs.cmp(ls)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.name.cmp(&right.name))
    });
    ranked
        .into_iter()
        .take(12)
        .map(|(_, symbol)| symbol.render())
        .collect()
}

fn language_for(path: &Path) -> Option<Language> {
    Some(match path.extension()?.to_str()? {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "js" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        _ => return None,
    })
}

fn visit(node: Node<'_>, text: &str, path: &Path, scope: &str, symbols: &mut Vec<Symbol>) {
    let kind = node_kind(node.kind());
    let name = node
        .child_by_field_name("name")
        .and_then(|node| node_text(node, text));
    let next_scope = if let (Some(kind), Some(name)) = (kind, name) {
        push(path, node, kind, name, scope, text, symbols);
        if matches!(kind, "import" | "reference") {
            scope.to_owned()
        } else {
            join_scope(scope, name)
        }
    } else if is_import(node.kind()) {
        let import = compact(node_text(node, text).unwrap_or_default());
        if !import.is_empty() {
            push(path, node, "import", &import, scope, text, symbols);
        }
        scope.to_owned()
    } else if node.kind() == "identifier" && is_reference(node) {
        if let Some(name) = node_text(node, text) {
            push(path, node, "reference", name, scope, text, symbols);
        }
        scope.to_owned()
    } else {
        scope.to_owned()
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, text, path, &next_scope, symbols);
    }
}

fn node_kind(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "function_item"
        | "function_definition"
        | "function_declaration"
        | "method_declaration"
        | "method_definition" => "function",
        "struct_item" | "class_definition" | "class_declaration" => "class",
        "enum_item" | "enum_declaration" => "enum",
        "trait_item" | "interface_declaration" => "interface",
        "impl_item" => "implementation",
        "mod_item" | "module" => "module",
        "type_item" | "type_alias_declaration" | "type_declaration" => "type",
        "const_item" => "constant",
        _ => return None,
    })
}

fn is_import(kind: &str) -> bool {
    matches!(
        kind,
        "use_declaration"
            | "import_statement"
            | "import_from_statement"
            | "import_declaration"
            | "package_clause"
    )
}
fn is_reference(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .child_by_field_name("name")
        .is_none_or(|name| name.id() != node.id())
}
fn push(
    path: &Path,
    node: Node<'_>,
    kind: &'static str,
    name: &str,
    scope: &str,
    text: &str,
    symbols: &mut Vec<Symbol>,
) {
    symbols.push(Symbol {
        path: path.display().to_string(),
        line: node.start_position().row + 1,
        kind,
        name: name.to_owned(),
        scope: scope.to_owned(),
        context: excerpt(node, text),
    });
}
fn node_text<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    text.get(node.byte_range())
}
fn join_scope(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.to_owned()
    } else {
        format!("{scope}::{name}")
    }
}
fn compact(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn excerpt(node: Node<'_>, text: &str) -> String {
    let start = node.start_position().row;
    let leading = text
        .lines()
        .skip(start.saturating_sub(3))
        .take(start.saturating_sub(start.saturating_sub(3)))
        .map(str::trim)
        .filter(|line| line.starts_with("///") || line.starts_with("//") || line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{leading} {}",
        node_text(node, text).map(compact).unwrap_or_default()
    )
    .chars()
    .take(240)
    .collect()
}
fn score(symbol: &Symbol, query: &[String]) -> Option<usize> {
    let name = terms(&symbol.name);
    let scope = terms(&symbol.scope);
    let context = terms(&symbol.context);
    let path = terms(&symbol.path);
    let mut score = 0;
    for term in query {
        let in_name = name.iter().any(|value| value == term);
        let in_scope = scope.iter().any(|value| value == term);
        let in_context = context.iter().any(|value| value == term);
        let in_path = path.iter().any(|value| value == term);
        if !(in_name || in_scope || in_context || in_path) {
            return None;
        }
        score += if in_name {
            16
        } else if in_scope {
            10
        } else if in_context {
            5
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

#[cfg(test)]
mod tests {
    use super::{collect, retrieve};
    use std::path::Path;
    #[test]
    fn tree_sitter_returns_a_scoped_rust_declaration_and_reference() {
        let mut symbols = Vec::new();
        collect(
            Path::new("src/clock.rs"),
            "mod turn_clock { pub fn refreshTurnClock() { refreshTurnClock(); } }",
            &mut symbols,
        );
        let hit = retrieve(&symbols, "turn_clock refresh").join("\n");
        assert!(
            hit.contains("function refreshTurnClock in turn_clock"),
            "{hit}"
        );
        assert!(hit.contains("reference refreshTurnClock"), "{hit}");
    }
    #[test]
    fn a_syntax_error_is_not_promoted_to_a_guessed_symbol() {
        let mut symbols = Vec::new();
        collect(
            Path::new("broken.rs"),
            "fn not actually valid(",
            &mut symbols,
        );
        assert!(symbols.is_empty());
    }
    #[test]
    fn typescript_is_parsed_by_its_own_grammar() {
        let mut symbols = Vec::new();
        collect(
            Path::new("view.ts"),
            "export interface TurnClock { refreshTurnClock(): void }",
            &mut symbols,
        );
        assert!(
            retrieve(&symbols, "turn clock")
                .join("\n")
                .contains("interface TurnClock")
        );
    }
}
