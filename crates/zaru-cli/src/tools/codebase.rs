// Copyright 2026 100monkeys AI, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Parser-backed local code retrieval for [`super::files::search`].
//!
//! Every structural fact here comes from a Tree-sitter concrete syntax tree.
//! Error recovery may leave a tree incomplete, but never turns text into a
//! guessed declaration: only concrete nodes with their grammar-defined fields
//! become facts.

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

/// Whether a file has a grammar this retrieval layer can parse locally.
pub(crate) fn is_supported(path: &Path) -> bool {
    language_for(path).is_some()
}

/// Parse one supported file and collect tree-derived facts relevant to `query`.
///
/// Declarations and imports are always useful structural context. References
/// are retained only when they match the query, preventing an identifier-rich
/// source file from becoming an in-memory index of every local variable.
pub(crate) fn collect(path: &Path, text: &str, query: &str, symbols: &mut Vec<Symbol>) {
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
    let query = terms(query);
    visit(tree.root_node(), text, path, "", &query, symbols);
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

fn visit(
    node: Node<'_>,
    text: &str,
    path: &Path,
    scope: &str,
    query: &[String],
    symbols: &mut Vec<Symbol>,
) {
    let declaration = declaration(node, text);
    let next_scope = if let Some((kind, name)) = declaration {
        push(path, node, kind, &name, scope, text, symbols);
        if matches!(kind, "import" | "reference") {
            scope.to_owned()
        } else {
            join_scope(scope, &name)
        }
    } else if is_import(node.kind()) {
        let import = compact(node_text(node, text).unwrap_or_default());
        if !import.is_empty() {
            push(path, node, "import", &import, scope, text, symbols);
        }
        scope.to_owned()
    } else if is_reference(node) && matches_query(node, text, query) {
        if let Some(name) = node_text(node, text) {
            push(path, node, "reference", name, scope, text, symbols);
        }
        scope.to_owned()
    } else {
        scope.to_owned()
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, text, path, &next_scope, query, symbols);
    }
}

fn declaration(node: Node<'_>, text: &str) -> Option<(&'static str, String)> {
    let kind = node_kind(node)?;
    declaration_name(node, text).map(|name| (kind, name.to_owned()))
}

fn node_kind(node: Node<'_>) -> Option<&'static str> {
    Some(match node.kind() {
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
        "type_item" | "type_alias_declaration" | "type_spec" => "type",
        "const_item" => "constant",
        "variable_declarator"
            if node.child_by_field_name("value").is_some_and(|value| {
                matches!(value.kind(), "arrow_function" | "function_expression")
            }) =>
        {
            "function"
        }
        _ => return None,
    })
}

fn declaration_name<'a>(node: Node<'_>, text: &'a str) -> Option<&'a str> {
    let named = node
        .child_by_field_name("name")
        .or_else(|| match node.kind() {
            // Rust implementation blocks name their implemented type rather than
            // exposing a `name` field. Keeping it as scope makes methods citable
            // as `Type::method`.
            "impl_item" => node.child_by_field_name("type"),
            _ => None,
        });
    named.and_then(|node| node_text(node, text))
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
    if !matches!(node.kind(), "identifier" | "type_identifier") {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    parent
        .child_by_field_name("name")
        .is_none_or(|name| name.id() != node.id())
}

fn matches_query(node: Node<'_>, text: &str, query: &[String]) -> bool {
    node_text(node, text)
        .map(terms)
        .is_some_and(|reference| query.iter().any(|term| reference.contains(term)))
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
    let mut matched = 0;
    for term in query {
        let in_name = name.iter().any(|value| value == term);
        let in_scope = scope.iter().any(|value| value == term);
        let in_context = context.iter().any(|value| value == term);
        let in_path = path.iter().any(|value| value == term);
        if in_name || in_scope || in_context || in_path {
            matched += 1;
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
    }
    // A multi-term query should still locate a declaration when one concept
    // belongs to a caller and another belongs to its callee. Coverage remains
    // the primary sort key, then the field-specific relevance above.
    (matched > 0).then_some(score + matched * 32)
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
            "turn_clock refresh",
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
    fn a_syntax_error_keeps_concrete_declarations_before_it() {
        let mut symbols = Vec::new();
        collect(
            Path::new("broken.rs"),
            "fn valid_before_error() {}\nfn not actually valid(",
            "valid error",
            &mut symbols,
        );
        let hit = retrieve(&symbols, "valid error").join("\n");
        assert!(hit.contains("function valid_before_error"), "{hit}");
    }
    #[test]
    fn typescript_is_parsed_by_its_own_grammar() {
        let mut symbols = Vec::new();
        collect(
            Path::new("view.ts"),
            "export interface TurnClock { refreshTurnClock(): void }",
            "turn clock",
            &mut symbols,
        );
        assert!(
            retrieve(&symbols, "turn clock")
                .join("\n")
                .contains("interface TurnClock")
        );
    }
    #[test]
    fn declarations_cover_rust_impls_typescript_arrows_and_go_types() {
        let cases = [
            (
                "model.rs",
                "struct TurnClock; impl TurnClock { fn refresh(&self) {} }",
                "TurnClock refresh",
                "function refresh in TurnClock",
            ),
            (
                "view.ts",
                "const refreshTurnClock = () => {};",
                "refresh turn clock",
                "function refreshTurnClock",
            ),
            (
                "model.go",
                "type TurnClock struct {}",
                "turn clock",
                "type TurnClock",
            ),
        ];
        for (path, source, query, expected) in cases {
            let mut symbols = Vec::new();
            collect(Path::new(path), source, query, &mut symbols);
            let hit = retrieve(&symbols, query).join("\n");
            assert!(hit.contains(expected), "{path}: {hit}");
        }
    }
    #[test]
    fn a_multi_term_query_keeps_a_partial_structural_match() {
        let mut symbols = Vec::new();
        collect(
            Path::new("clock.rs"),
            "fn refresh_turn_clock() {}",
            "refresh caller",
            &mut symbols,
        );
        assert!(
            retrieve(&symbols, "refresh caller")
                .join("\n")
                .contains("refresh_turn_clock")
        );
    }
}
