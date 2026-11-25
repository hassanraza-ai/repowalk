//! tree-sitter-go extraction: functions, methods, types, calls, imports.
//!
//! Same symbol id convention as the other languages: `{path}:{start_line}`.
//! A parse failure returns empty lists; it never panics.
use crate::model::{CallSite, Import, Symbol, SymbolKind};
use crate::syntax;
use std::sync::OnceLock;
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};

const DEFS: &str = r#"
(function_declaration name: (identifier) @name) @def
(method_declaration name: (field_identifier) @name) @def
(type_spec name: (type_identifier) @name) @def
"#;

const CALLS: &str = r#"
(call_expression function: (identifier) @callee) @call
(call_expression function: (selector_expression field: (field_identifier) @callee)) @call
"#;

const IMPORTS: &str = r#"
(import_spec path: (interpreted_string_literal) @target) @imp
"#;

/// Extract symbols, call sites and imports from one Go file.
pub fn extract(source: &str, path: &str) -> (Vec<Symbol>, Vec<CallSite>, Vec<Import>) {
    let Some(tree) = syntax::parse_go(source) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let root = tree.root_node();
    let lang: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    (
        run_defs(root, source, path, lang.clone()),
        run_calls(root, source, path, lang.clone()),
        run_imports(root, source, path, lang),
    )
}

fn run_defs(root: Node, source: &str, path: &str, lang: tree_sitter::Language) -> Vec<Symbol> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    let Some(query) = syntax::query(&QUERY, lang, DEFS) else {
        return Vec::new();
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source.as_bytes());
    let names = query.capture_names();
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let (Some(def), Some(name_node)) = (cap(m, names, "def"), cap(m, names, "name")) else {
            continue;
        };
        let Some(name) = text(name_node, source) else {
            continue;
        };
        let start = def.start_position().row + 1;
        let kind = match def.kind() {
            "method_declaration" => SymbolKind::Method,
            "type_spec" => type_kind(def),
            _ => SymbolKind::Function,
        };
        out.push(Symbol {
            id: format!("{path}:{start}"),
            name: name.to_string(),
            kind,
            signature: signature(def, source),
            doc: String::new(),
            start_line: start,
            end_line: def.end_position().row + 1,
            container: method_receiver(def, source),
            file: path.to_string(),
        });
    }
    out
}

fn run_calls(root: Node, source: &str, path: &str, lang: tree_sitter::Language) -> Vec<CallSite> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    let Some(query) = syntax::query(&QUERY, lang, CALLS) else {
        return Vec::new();
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source.as_bytes());
    let names = query.capture_names();
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let (Some(call), Some(callee)) = (cap(m, names, "call"), cap(m, names, "callee")) else {
            continue;
        };
        let Some(callee) = text(callee, source) else {
            continue;
        };
        out.push(CallSite {
            file: path.to_string(),
            callee: callee.to_string(),
            line: call.start_position().row + 1,
            enclosing: enclosing_name(call, source),
        });
    }
    out
}

fn run_imports(root: Node, source: &str, path: &str, lang: tree_sitter::Language) -> Vec<Import> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    let Some(query) = syntax::query(&QUERY, lang, IMPORTS) else {
        return Vec::new();
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source.as_bytes());
    let names = query.capture_names();
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let Some(target) = cap(m, names, "target").and_then(|n| text(n, source)) else {
            continue;
        };
        let target = target.trim_matches('"').trim_matches('`').to_string();
        if target.is_empty() {
            continue;
        }
        out.push(Import {
            file: path.to_string(),
            target,
        });
    }
    out
}

fn type_kind(spec: Node) -> SymbolKind {
    let mut cursor = spec.walk();
    for child in spec.named_children(&mut cursor) {
        match child.kind() {
            "struct_type" => return SymbolKind::Struct,
            "interface_type" => return SymbolKind::Trait,
            _ => {}
        }
    }
    SymbolKind::Struct
}

fn method_receiver(def: Node, source: &str) -> Option<String> {
    if def.kind() != "method_declaration" {
        return None;
    }
    let recv = def.child_by_field_name("receiver")?;
    let mut cursor = recv.walk();
    for child in recv.named_children(&mut cursor) {
        if child.kind() == "parameter_declaration" {
            if let Some(ty) = child.child_by_field_name("type") {
                return text(ty, source).map(|s| s.trim_start_matches('*').to_string());
            }
        }
    }
    None
}

fn enclosing_name(mut node: Node, source: &str) -> Option<String> {
    while let Some(parent) = node.parent() {
        if matches!(parent.kind(), "function_declaration" | "method_declaration") {
            return parent
                .child_by_field_name("name")
                .and_then(|n| text(n, source))
                .map(str::to_string);
        }
        node = parent;
    }
    None
}

fn signature(def: Node, source: &str) -> String {
    let Some(raw) = text(def, source) else {
        return String::new();
    };
    let end = raw.find('{').unwrap_or(raw.len());
    raw[..end].split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cap<'a>(m: &tree_sitter::QueryMatch<'_, 'a>, names: &[&str], want: &str) -> Option<Node<'a>> {
    m.captures()
        .iter()
        .find(|c| names.get(c.index as usize) == Some(&want))
        .map(|c| c.node)
}

fn text<'a>(node: Node, source: &'a str) -> Option<&'a str> {
    node.utf8_text(source.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
package p

import "fmt"

func main() {
    if ready() {
        fmt.Println("hi")
    }
}

func ready() bool { return true }

type Box struct { N int }
"#;

    #[test]
    fn extracts_functions_types_calls_and_imports() {
        let (symbols, calls, imports) = extract(SRC, "main.go");
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"), "{names:?}");
        assert!(names.contains(&"ready"), "{names:?}");
        assert!(names.contains(&"Box"), "{names:?}");
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "Box" && s.kind == SymbolKind::Struct),
            "{symbols:?}"
        );
        assert!(calls.iter().any(|c| c.callee == "Println"), "{calls:?}");
        assert!(
            calls
                .iter()
                .any(|c| c.callee == "Println" && c.enclosing.as_deref() == Some("main")),
            "{calls:?}"
        );
        assert!(imports.iter().any(|i| i.target == "fmt"), "{imports:?}");
    }

    #[test]
    fn empty_and_broken_input_does_not_panic() {
        let (symbols, _, _) = extract("", "x.go");
        assert!(symbols.is_empty());
        let _ = extract("func (\n", "x.go");
    }
}
