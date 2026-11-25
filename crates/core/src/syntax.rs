//! Reused tree-sitter parsers and compiled queries.
//!
//! A [`tree_sitter::Parser`] is not shareable across threads, so each thread
//! keeps one parser per grammar and reuses it for every file. A
//! [`tree_sitter::Query`] is immutable after it is built, so each static query
//! is compiled once per process. Creating both per file dominated cold `learn`
//! runs; the grammars themselves are unchanged, so extraction stays the same.

use std::cell::RefCell;
use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, Tree};

fn parser_for(lang: Language) -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&lang)
        .expect("bundled tree-sitter grammar");
    parser
}

thread_local! {
    static RUST: RefCell<Parser> = RefCell::new(parser_for(tree_sitter_rust::LANGUAGE.into()));
    static PYTHON: RefCell<Parser> = RefCell::new(parser_for(tree_sitter_python::LANGUAGE.into()));
    static TYPESCRIPT: RefCell<Parser> =
        RefCell::new(parser_for(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()));
    static TSX: RefCell<Parser> =
        RefCell::new(parser_for(tree_sitter_typescript::LANGUAGE_TSX.into()));
    static JAVASCRIPT: RefCell<Parser> =
        RefCell::new(parser_for(tree_sitter_javascript::LANGUAGE.into()));
    static GO: RefCell<Parser> = RefCell::new(parser_for(tree_sitter_go::LANGUAGE.into()));
}

/// Which script grammar a path needs. TSX is not a superset the plain
/// TypeScript grammar can parse, and JavaScript is not TypeScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptKind {
    TypeScript,
    Tsx,
    JavaScript,
}

pub fn script_kind(file: &str) -> ScriptKind {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".tsx") {
        ScriptKind::Tsx
    } else if lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".mjs")
        || lower.ends_with(".cjs")
    {
        ScriptKind::JavaScript
    } else {
        ScriptKind::TypeScript
    }
}

pub fn parse_rust(source: &str) -> Option<Tree> {
    RUST.with(|parser| parser.borrow_mut().parse(source, None))
}

pub fn parse_python(source: &str) -> Option<Tree> {
    PYTHON.with(|parser| parser.borrow_mut().parse(source, None))
}

pub fn parse_go(source: &str) -> Option<Tree> {
    GO.with(|parser| parser.borrow_mut().parse(source, None))
}

pub fn parse_script(kind: ScriptKind, source: &str) -> Option<Tree> {
    match kind {
        ScriptKind::TypeScript => TYPESCRIPT.with(|parser| parser.borrow_mut().parse(source, None)),
        ScriptKind::Tsx => TSX.with(|parser| parser.borrow_mut().parse(source, None)),
        ScriptKind::JavaScript => JAVASCRIPT.with(|parser| parser.borrow_mut().parse(source, None)),
    }
}

/// Compile `source` the first time `slot` is touched, then reuse it.
///
/// A query that fails to compile (it does not, for the queries checked in)
/// yields `None` forever after, which is the same empty extraction the
/// per-file `Query::new(...).ok()` path used.
pub fn query(
    slot: &'static OnceLock<Option<Query>>,
    lang: Language,
    source: &str,
) -> Option<&'static Query> {
    slot.get_or_init(|| Query::new(&lang, source).ok()).as_ref()
}
