//! Cognitive complexity, beside cyclomatic complexity.
//!
//! Cyclomatic complexity counts paths (McCabe). Cognitive complexity counts
//! how hard a function is to read: breaks in straight-line flow, extra weight
//! when those breaks are nested, one point per run of the same boolean
//! operator, and one point for a direct recursive call. The rules follow
//! SonarSource's specification (Campbell, *Cognitive Complexity*, version 1.7):
//! `else` / `else if` add one and raise the nesting level for their body, but
//! they do not themselves take a nesting penalty. `switch` / `match` adds one
//! for the whole construct, not one per arm.
//!
//! Each function is scored on its own. Nested functions that already have a
//! control-flow graph are skipped here so a parent is not charged for a child
//! that has its own row. Closures and anonymous callbacks are part of the
//! enclosing function and do raise nesting.

use tree_sitter::Node;

/// Which grammar `score` should apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    Go,
}

/// Cognitive complexity of one function-like node.
///
/// `func` is the function itself (`function_item`, `function_definition`,
/// `function_declaration`, a method, or an arrow). Its own declaration is not
/// a point. The walk starts at its children.
pub fn score(func: Node, source: &str, lang: Language) -> u32 {
    let name = func
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("");
    let mut total = 0u32;
    for child in named(func) {
        visit(child, source, lang, name, 0, &mut total);
    }
    total
}

fn visit(node: Node, source: &str, lang: Language, fn_name: &str, nesting: u32, total: &mut u32) {
    if owns_own_cfg(node, lang) {
        return;
    }
    if is_lambda(node, lang) {
        for child in named(node) {
            visit(child, source, lang, fn_name, nesting + 1, total);
        }
        return;
    }
    if is_if(node, lang) {
        *total = total.saturating_add(1 + nesting);
        visit_if(node, source, lang, fn_name, nesting + 1, total);
        return;
    }
    if is_structural(node, lang) {
        *total = total.saturating_add(1 + nesting);
        for child in named(node) {
            visit(child, source, lang, fn_name, nesting + 1, total);
        }
        return;
    }
    if logical_op(node, source).is_some() {
        let (sequences, leaves) = flatten_logical(node, source);
        *total = total.saturating_add(sequences);
        for leaf in leaves {
            visit(leaf, source, lang, fn_name, nesting, total);
        }
        return;
    }
    if labeled_jump(node) {
        *total = total.saturating_add(1);
    }
    if direct_recursion(node, source, fn_name) {
        *total = total.saturating_add(1);
    }
    for child in named(node) {
        visit(child, source, lang, fn_name, nesting, total);
    }
}

/// `else` / `else if` / `elif`: +1, and the body stays at `body_nesting`
/// (already one deeper than the `if` that opened the chain). The inner `if`
/// of an `else if` is not charged a second time.
fn visit_if(
    node: Node,
    source: &str,
    lang: Language,
    fn_name: &str,
    body_nesting: u32,
    total: &mut u32,
) {
    for child in named(node) {
        if is_else_or_elif(child) {
            *total = total.saturating_add(1);
            for grand in named(child) {
                if is_if(grand, lang) {
                    visit_if(grand, source, lang, fn_name, body_nesting, total);
                } else {
                    visit(grand, source, lang, fn_name, body_nesting, total);
                }
            }
        } else {
            visit(child, source, lang, fn_name, body_nesting, total);
        }
    }
}

fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn is_if(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => node.kind() == "if_expression",
        Language::Python | Language::TypeScript | Language::Go => node.kind() == "if_statement",
    }
}

fn is_else_or_elif(node: Node) -> bool {
    matches!(node.kind(), "else_clause" | "elif_clause")
}

fn is_structural(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(
            node.kind(),
            "for_expression" | "while_expression" | "loop_expression" | "match_expression"
        ),
        Language::Python => matches!(
            node.kind(),
            "for_statement"
                | "while_statement"
                | "match_statement"
                | "except_clause"
                | "conditional_expression"
        ),
        Language::TypeScript => matches!(
            node.kind(),
            "for_statement"
                | "for_in_statement"
                | "while_statement"
                | "do_statement"
                | "switch_statement"
                | "catch_clause"
                | "ternary_expression"
        ),
        Language::Go => matches!(
            node.kind(),
            "for_statement"
                | "expression_switch_statement"
                | "type_switch_statement"
                | "select_statement"
        ),
    }
}

fn is_lambda(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => node.kind() == "closure_expression",
        Language::Python => node.kind() == "lambda",
        Language::Go => node.kind() == "func_literal",
        Language::TypeScript => {
            matches!(
                node.kind(),
                "arrow_function" | "function_expression" | "generator_function"
            ) && !owns_own_cfg(node, lang)
        }
    }
}

fn owns_own_cfg(node: Node, lang: Language) -> bool {
    match lang {
        Language::Rust => matches!(node.kind(), "function_item" | "function_signature_item"),
        Language::Python => node.kind() == "function_definition",
        Language::Go => matches!(node.kind(), "function_declaration" | "method_declaration"),
        Language::TypeScript => match node.kind() {
            "function_declaration" | "generator_function_declaration" | "method_definition" => true,
            "arrow_function" | "function_expression" | "generator_function" => {
                let Some(parent) = node.parent() else {
                    return false;
                };
                matches!(
                    parent.kind(),
                    "variable_declarator" | "public_field_definition" | "field_definition"
                ) || (parent.kind() == "assignment_expression"
                    && parent
                        .child_by_field_name("right")
                        .is_some_and(|right| right.id() == node.id()))
            }
            _ => false,
        },
    }
}

/// `Some(true)` is AND, `Some(false)` is OR.
fn logical_op(node: Node, source: &str) -> Option<bool> {
    if node.kind() != "binary_expression" && node.kind() != "boolean_operator" {
        return None;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let text = child.utf8_text(source.as_bytes()).unwrap_or("");
        if text == "&&" || text == "and" {
            return Some(true);
        }
        if text == "||" || text == "or" {
            return Some(false);
        }
    }
    None
}

fn flatten_logical<'t>(node: Node<'t>, source: &str) -> (u32, Vec<Node<'t>>) {
    let mut ops = Vec::new();
    let mut leaves = Vec::new();
    flatten_rec(node, source, &mut ops, &mut leaves);
    let mut sequences = 0u32;
    let mut previous: Option<bool> = None;
    for op in ops {
        if previous != Some(op) {
            sequences = sequences.saturating_add(1);
            previous = Some(op);
        }
    }
    (sequences, leaves)
}

fn flatten_rec<'t>(node: Node<'t>, source: &str, ops: &mut Vec<bool>, leaves: &mut Vec<Node<'t>>) {
    if logical_op(node, source).is_some() {
        let kids = named(node);
        if let Some(left) = kids.first() {
            flatten_rec(*left, source, ops, leaves);
        }
        if let Some(op) = logical_op(node, source) {
            ops.push(op);
        }
        if let Some(right) = kids.get(1) {
            flatten_rec(*right, source, ops, leaves);
        }
    } else {
        leaves.push(node);
    }
}

fn labeled_jump(node: Node) -> bool {
    match node.kind() {
        "break_expression" | "continue_expression" => {
            let mut cursor = node.walk();
            let mut labeled = false;
            for child in node.children(&mut cursor) {
                if child.kind() == "label" {
                    labeled = true;
                    break;
                }
            }
            labeled
        }
        "break_statement" | "continue_statement" => node.named_child_count() > 0,
        _ => false,
    }
}

fn direct_recursion(node: Node, source: &str, fn_name: &str) -> bool {
    if fn_name.is_empty() || !matches!(node.kind(), "call_expression" | "call") {
        return false;
    }
    let Some(function) = node.child_by_field_name("function") else {
        return false;
    };
    if function.kind() != "identifier" {
        return false;
    }
    function.utf8_text(source.as_bytes()).ok() == Some(fn_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn rust_fn(source: &str, name: &str) -> u32 {
        score_named(
            source,
            name,
            Language::Rust,
            tree_sitter_rust::LANGUAGE.into(),
        )
    }

    fn python_fn(source: &str, name: &str) -> u32 {
        score_named(
            source,
            name,
            Language::Python,
            tree_sitter_python::LANGUAGE.into(),
        )
    }

    fn score_named(
        source: &str,
        name: &str,
        lang: Language,
        grammar: tree_sitter::Language,
    ) -> u32 {
        let mut parser = Parser::new();
        parser.set_language(&grammar).expect("grammar");
        let tree = parser.parse(source, None).expect("parse");
        let found = find_fn(tree.root_node(), name, source).unwrap_or_else(|| {
            panic!(
                "no function {name} in {source}\n{}",
                tree.root_node().to_sexp()
            )
        });
        score(found, source, lang)
    }

    fn find_fn<'t>(node: Node<'t>, name: &str, source: &str) -> Option<Node<'t>> {
        let own = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok());
        if own == Some(name)
            && matches!(
                node.kind(),
                "function_item" | "function_definition" | "function_declaration"
            )
        {
            return Some(node);
        }
        for child in named(node) {
            if let Some(found) = find_fn(child, name, source) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn match_is_one_point_not_one_per_arm() {
        let src = r#"
fn get_words(n: i32) -> &'static str {
    match n {
        1 => "one",
        2 => "a couple",
        3 => "a few",
        _ => "lots",
    }
}
"#;
        assert_eq!(rust_fn(src, "get_words"), 1);
    }

    #[test]
    fn nested_loops_and_a_labeled_continue_match_the_sum_of_primes_example() {
        // Campbell's sumOfPrimes: for +1, nested for +2, nested if +3,
        // labeled continue +1. An unlabeled continue would not add a point.
        let src = r#"
fn sum_of_primes(max: i32) -> i32 {
    let mut total = 0;
    'outer: for i in 1..=max {
        for j in 2..i {
            if i % j == 0 {
                continue 'outer;
            }
        }
        total += i;
    }
    total
}
"#;
        assert_eq!(rust_fn(src, "sum_of_primes"), 7);
    }

    #[test]
    fn boolean_runs_and_nested_and_are_separate_sequences() {
        let chain = r#"
fn f(a: bool, b: bool, c: bool, d: bool, e: bool, g: bool) {
    if a && b && c || d || e && g {}
}
"#;
        assert_eq!(rust_fn(chain, "f"), 4, "if + three operator runs");

        let grouped = r#"
fn g(a: bool, b: bool, c: bool) {
    if a && !(b && c) {}
}
"#;
        assert_eq!(rust_fn(grouped, "g"), 3, "if + outer && + inner &&");
    }

    #[test]
    fn else_if_does_not_double_charge_and_does_nest() {
        let src = r#"
fn h(a: bool, b: bool, c: bool) {
    if a {
    } else if b {
        if c {}
    } else {
    }
}
"#;
        // if +1, else if +1, inner if +2, else +1.
        assert_eq!(rust_fn(src, "h"), 5);
    }

    #[test]
    fn direct_recursion_adds_one_and_a_nested_fn_is_not_charged_to_the_parent() {
        let src = r#"
fn fact(n: u32) -> u32 {
    if n == 0 { 1 } else { fact(n - 1) }
}
fn outer() {
    fn inner() {
        if true {}
    }
}
"#;
        assert_eq!(rust_fn(src, "fact"), 3, "if + else + recursive call");
        assert_eq!(rust_fn(src, "outer"), 0, "the nested fn has its own score");
        assert_eq!(rust_fn(src, "inner"), 1);
    }

    #[test]
    fn python_try_except_matches_the_nesting_example() {
        let src = r#"
def my_method():
    try:
        if condition1:
            for i in range(10):
                while condition2:
                    pass
    except Exception:
        if condition2:
            pass
"#;
        // if +1, for +2, while +3, except +1, if +2.
        assert_eq!(python_fn(src, "my_method"), 9);
    }

    #[test]
    fn python_boolean_runs_match_the_rust_rule() {
        let src = r#"
def f(a, b, c, d, e, g):
    if a and b and c or d or e and g:
        pass
"#;
        assert_eq!(python_fn(src, "f"), 4);
    }
}
