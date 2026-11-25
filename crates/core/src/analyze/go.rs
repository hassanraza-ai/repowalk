//! Go analysis: functions, calls, and named types.
//!
//! A parse failure appends nothing. Nested functions are their own symbols;
//! a function's complexity walk does not descend into them.
use super::snippet;
use crate::analysis::{
    Analysis, CfgEdge, CfgEdgeKind, CfgKind, CfgNode, FunctionCfg, TypeKind, TypeNode, UseKind,
    UseSite,
};
use crate::cognitive::{self, Language};
use crate::model::{Symbol, SymbolKind};
use crate::syntax;
use tree_sitter::Node;

pub fn analyze(file: &str, source: &str, symbols: &[Symbol], out: &mut Analysis) {
    let Some(tree) = syntax::parse_go(source) else {
        return;
    };
    let root = tree.root_node();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "function_declaration" | "method_declaration" => {
                push_cfg(file, source, symbols, node, out);
            }
            "type_spec" => push_type(file, source, node, out),
            "call_expression" => push_call(file, source, symbols, node, out),
            "import_spec" => push_import(file, source, node, out),
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }
    out.cfgs.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    out.types.sort_by(|a, b| a.id.cmp(&b.id));
}

fn push_cfg(file: &str, source: &str, symbols: &[Symbol], node: Node, out: &mut Analysis) {
    let start = node.start_position().row + 1;
    let end = node.end_position().row + 1;
    let symbol = symbols
        .iter()
        .find(|s| {
            s.start_line == start && matches!(s.kind, SymbolKind::Function | SymbolKind::Method)
        })
        .map(|s| s.id.clone())
        .unwrap_or_else(|| format!("{file}:{start}"));
    let entry = CfgNode {
        id: 0,
        kind: CfgKind::Entry,
        start_line: start,
        end_line: start,
        label: "entry".into(),
    };
    let exit = CfgNode {
        id: 1,
        kind: CfgKind::Exit,
        start_line: end,
        end_line: end,
        label: "exit".into(),
    };
    out.cfgs.push(FunctionCfg {
        symbol,
        nodes: vec![entry, exit],
        edges: vec![CfgEdge {
            from: 0,
            to: 1,
            kind: CfgEdgeKind::Normal,
            line: start,
        }],
        cyclomatic: 1 + decisions(node),
        max_nesting: max_nest(node, 0),
        cognitive: cognitive::score(node, source, Language::Go),
    });
}

fn decisions(node: Node) -> u32 {
    let mut n = 0u32;
    let mut stack = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        stack.push(child);
    }
    while let Some(cur) = stack.pop() {
        if matches!(
            cur.kind(),
            "if_statement"
                | "for_statement"
                | "expression_switch_statement"
                | "type_switch_statement"
                | "select_statement"
        ) {
            n = n.saturating_add(1);
        }
        if matches!(cur.kind(), "function_declaration" | "method_declaration") {
            continue;
        }
        let mut c = cur.walk();
        for child in cur.named_children(&mut c) {
            stack.push(child);
        }
    }
    n
}

fn max_nest(node: Node, depth: u32) -> u32 {
    let mut best = depth;
    if matches!(node.kind(), "function_declaration" | "method_declaration") && depth > 0 {
        return 0;
    }
    let deeper = if matches!(
        node.kind(),
        "if_statement" | "for_statement" | "expression_switch_statement" | "select_statement"
    ) {
        depth + 1
    } else {
        depth
    };
    best = best.max(deeper);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        best = best.max(max_nest(child, deeper));
    }
    best
}

fn push_type(file: &str, source: &str, node: Node, out: &mut Analysis) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
        return;
    };
    let kind = {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        children.into_iter().find_map(|c| match c.kind() {
            "struct_type" => Some(TypeKind::Struct),
            "interface_type" => Some(TypeKind::Interface),
            _ => None,
        })
    };
    let Some(kind) = kind else {
        return;
    };
    let line = node.start_position().row + 1;
    out.types.push(TypeNode {
        id: format!("{file}:{name}"),
        name: name.to_string(),
        kind,
        file: file.to_string(),
        line,
        fields: Vec::new(),
        methods: Vec::new(),
        doc: String::new(),
        module_path: String::new(),
    });
}

fn push_call(file: &str, source: &str, symbols: &[Symbol], node: Node, out: &mut Analysis) {
    let Some(fun) = node.child_by_field_name("function") else {
        return;
    };
    let name = match fun.kind() {
        "identifier" => fun.utf8_text(source.as_bytes()).unwrap_or("").to_string(),
        "selector_expression" => fun
            .child_by_field_name("field")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .unwrap_or("")
            .to_string(),
        _ => return,
    };
    if name.is_empty() {
        return;
    }
    let line = node.start_position().row + 1;
    let target = symbols
        .iter()
        .find(|s| s.name == name && s.file == file)
        .map(|s| s.id.clone())
        .unwrap_or(name);
    out.uses.push(UseSite {
        target,
        file: file.to_string(),
        line,
        col: node.start_position().column + 1,
        kind: UseKind::Call,
        snippet: snippet(source, line),
    });
}

fn push_import(file: &str, source: &str, node: Node, out: &mut Analysis) {
    let Some(path) = node.child_by_field_name("path") else {
        return;
    };
    let Ok(raw) = path.utf8_text(source.as_bytes()) else {
        return;
    };
    let target = raw.trim_matches('"').to_string();
    if target.is_empty() {
        return;
    }
    let line = node.start_position().row + 1;
    out.uses.push(UseSite {
        target,
        file: file.to_string(),
        line,
        col: node.start_position().column + 1,
        kind: UseKind::Import,
        snippet: snippet(source, line),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_go;

    #[test]
    fn if_adds_one_to_cyclomatic_and_box_is_a_struct() {
        let src = "package p\nfunc main() {\n    if ready() {\n        use()\n    }\n}\nfunc ready() bool { return true }\nfunc use() {}\ntype Box struct { N int }\n";
        let (symbols, _, _) = parse_go::extract(src, "main.go");
        let mut out = Analysis::new();
        analyze("main.go", src, &symbols, &mut out);
        let main = out
            .cfgs
            .iter()
            .find(|c| {
                c.symbol.ends_with(":2")
                    || symbols.iter().any(|s| s.name == "main" && s.id == c.symbol)
            })
            .expect("main cfg");
        assert!(main.cyclomatic >= 2, "if is a decision: {main:?}");
        assert!(out
            .types
            .iter()
            .any(|t| t.name == "Box" && t.kind == TypeKind::Struct));
        assert!(out.uses.iter().any(|u| u.kind == UseKind::Call));
    }
}
