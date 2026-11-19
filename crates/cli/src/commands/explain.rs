//! `repowalk explain <symbol> <path>` — card: signature, docs, callers, callees.
//! OWNER: RSK-13.
use repowalk_core::model::{EdgeKind, Index, Symbol};
use repowalk_core::pipeline;
use std::path::Path;

pub fn run(symbol: &str, path: &Path) -> anyhow::Result<()> {
    let idx = pipeline::build_index(path, &|_done| {})?;
    print!("{}", render_explain(&idx, symbol));
    Ok(())
}

/// Explain card for `symbol`: signature, doc summary, file:line, callers,
/// callees, nested items. Honest "not found" when the symbol is unknown.
pub fn render_explain(idx: &Index, symbol: &str) -> String {
    use crate::render::{dim, head};
    let sym = match find_symbol(idx, symbol) {
        Ok(sym) => sym,
        Err(message) => return message,
    };

    let mut out = String::new();
    out.push_str(&head(&sym.name));
    out.push('\n');
    out.push_str(&format!("  {}:{}\n", sym.file, sym.start_line));
    if !sym.signature.is_empty() {
        out.push_str(&format!("  {}\n", sym.signature));
    }
    if !sym.doc.is_empty() {
        let summary = sym.doc.lines().next().unwrap_or_default();
        out.push_str(&format!("  {}\n", summary.trim()));
    }

    let mut callers: Vec<String> = idx
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Call && e.callee == sym.id)
        .map(|e| format!("{}:{} ({:?})", e.caller, e.line, e.confidence))
        .collect();
    callers.sort();
    callers.dedup();
    out.push_str(&head("Callers"));
    out.push('\n');
    if callers.is_empty() {
        out.push_str(&format!("  {}\n", dim("none")));
    }
    for c in callers {
        out.push_str(&format!("  {c}\n"));
    }

    let mut callees: Vec<String> = idx
        .edges
        .iter()
        .filter(|e| {
            e.kind == EdgeKind::Call
                && repowalk_core::flow::caller_symbol(idx, &e.caller, e.line)
                    .is_some_and(|owner| owner.id == sym.id)
        })
        .map(|e| format!("{}:{} ({:?})", e.callee, e.line, e.confidence))
        .collect();
    callees.sort();
    callees.dedup();
    out.push_str(&head("Callees"));
    out.push('\n');
    if callees.is_empty() {
        out.push_str(&format!("  {}\n", dim("none")));
    }
    for c in callees {
        out.push_str(&format!("  {c}\n"));
    }

    let nested: Vec<&repowalk_core::model::Symbol> = idx
        .symbols
        .iter()
        .filter(|s| s.container.as_deref() == Some(sym.name.as_str()))
        .collect();
    if !nested.is_empty() {
        out.push_str(&head("Nested"));
        out.push('\n');
        for s in nested {
            out.push_str(&format!("  {} ({}:{})\n", s.name, s.file, s.start_line));
        }
    }
    out
}

/// Id, then a unique name. Several symbols with that name is an error: the
/// first one in index order is not a definition.
fn find_symbol<'a>(idx: &'a Index, query: &str) -> Result<&'a Symbol, String> {
    if let Some(sym) = idx.symbols.iter().find(|s| s.id == query) {
        return Ok(sym);
    }
    let mut matches: Vec<&Symbol> = idx.symbols.iter().filter(|s| s.name == query).collect();
    matches.sort_by(|a, b| a.id.cmp(&b.id));
    match matches.as_slice() {
        [only] => Ok(only),
        [] => Err(format!("not found: {query}\n")),
        many => {
            let lines: Vec<String> = many.iter().map(|s| format!("  {}", s.id)).collect();
            Err(format!("ambiguous: {query}\n{}\n", lines.join("\n")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rw-cli-expl-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample_index(root: &Path) -> Index {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "/// Adds two numbers.\npub fn helper(a: i32, b: i32) -> i32 { a + b }\nfn main() { let _ = helper(1, 2); }\n",
        )
        .unwrap();
        pipeline::build_index(root, &|_| {}).unwrap()
    }

    #[test]
    fn explain_card_shows_signature_doc_callers_callees() {
        let root = temp_root("content");
        let idx = sample_index(&root);
        let out = render_explain(&idx, "helper");

        assert!(out.contains("helper"), "name: {out}");
        assert!(
            out.contains("pub fn helper(a: i32, b: i32) -> i32"),
            "signature: {out}"
        );
        assert!(out.contains("Adds two numbers"), "doc: {out}");
        assert!(out.contains("src/main.rs"), "file:line: {out}");
        assert!(out.contains("Callers"), "callers section: {out}");
        assert!(out.contains("Callees"), "callees section: {out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_unknown_symbol_is_honest() {
        let root = temp_root("missing");
        let idx = sample_index(&root);
        let out = render_explain(&idx, "no_such_symbol");
        assert!(out.contains("not found"), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_lists_callees_when_the_caller_is_stored_as_a_name() {
        let root = temp_root("callees");
        let idx = sample_index(&root);
        let out = render_explain(&idx, "main");
        let callees = out.split("Callees").nth(1).unwrap_or("");
        assert!(
            callees.contains("src/main.rs:2"),
            "main's callee list should name helper's symbol id: {out}"
        );
        let helper = render_explain(&idx, "helper");
        assert!(
            helper.contains("main"),
            "helper's callers should include main: {helper}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn explain_refuses_an_ambiguous_name() {
        let root = temp_root("ambiguous");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn helper() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "fn helper() {}\n").unwrap();
        let idx = pipeline::build_index(&root, &|_| {}).unwrap();
        let out = render_explain(&idx, "helper");
        assert!(out.contains("ambiguous"), "{out}");
        assert!(out.contains("src/a.rs:"), "{out}");
        assert!(out.contains("src/b.rs:"), "{out}");
        let id = idx
            .symbols
            .iter()
            .find(|s| s.file == "src/b.rs")
            .unwrap()
            .id
            .clone();
        let one = render_explain(&idx, &id);
        assert!(one.contains("src/b.rs:"), "{one}");
        assert!(!one.contains("ambiguous"), "{one}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
