#![allow(clippy::unwrap_used)]

use codemycel_core::{CandidateRole, LanguageAdapter, Node, ParsedFile, RepoPath};
use codemycel_parse::typescript::{JavaScriptAdapter, TypeScriptAdapter};

fn parse(path: &str, source: &str) -> ParsedFile {
    let path = RepoPath::new(path).unwrap();
    if path.as_str().ends_with("ts") || path.as_str().ends_with("tsx") {
        TypeScriptAdapter::new().parse(&path, source.as_bytes())
    } else {
        JavaScriptAdapter::new().parse(&path, source.as_bytes())
    }
    .unwrap()
}

fn nodes(parsed: &ParsedFile) -> Vec<String> {
    parsed
        .nodes
        .iter()
        .map(|node| {
            let span = node.span.unwrap();
            format!(
                "{} ({}) {}-{}",
                node.path, node.kind, span.line_start, span.line_end
            )
        })
        .collect()
}

fn node<'p>(parsed: &'p ParsedFile, name: &str) -> &'p Node {
    parsed.nodes.iter().find(|node| node.name == name).unwrap()
}

fn candidates(parsed: &ParsedFile) -> Vec<String> {
    parsed
        .candidates
        .iter()
        .map(|c| {
            format!(
                "{}:{} {} {} in {}",
                c.at.line, c.at.col, c.role, c.text, c.enclosing
            )
        })
        .collect()
}

#[test]
fn overloads_collapse_to_one_node() {
    let parsed = parse(
        "a.ts",
        "export function scale(v: number): number;\nexport function scale(v: string): string;\nexport function scale(v: any) {\n  return v;\n}\nclass Box {\n  put(v: A): void;\n  put(v: B): void;\n  put(v: unknown) {}\n}\ninterface Port {\n  send(v: A): void;\n  send(v: B): void;\n}\ndeclare function only(): C;\ndeclare function only(x: D): C;\n",
    );
    assert_eq!(
        nodes(&parsed),
        [
            "a.ts::scale (function) 3-5",
            "a.ts::Box (class) 6-10",
            "a.ts::Box::put (method) 9-9",
            "a.ts::Port (interface) 11-14",
            "a.ts::Port::send (method) 12-12",
            "a.ts::only (function) 15-15",
        ]
    );
    assert_eq!(node(&parsed, "scale").visibility.as_deref(), Some("export"));
    assert!(
        candidates(&parsed)
            .iter()
            .filter(|c| c.contains(" A ") || c.contains(" B "))
            .all(|c| c.ends_with("(method)"))
    );
}

#[test]
fn arrow_const_is_a_function_node() {
    for (path, annotation) in [("a.ts", ": number"), ("a.tsx", ": number"), ("a.js", "")] {
        let parsed = parse(
            path,
            &format!(
                "export const typed = (v){annotation} => v;\nconst inferred = () => {{\n  const inner = () => 1;\n  return inner();\n}};\nconst expression = function () {{}};\nlet later = () => 2;\nconst value = 3;\n"
            ),
        );
        assert_eq!(
            nodes(&parsed),
            [
                format!("{path}::typed (function) 1-1"),
                format!("{path}::inferred (function) 2-5"),
                format!("{path}::expression (constant) 6-6"),
                format!("{path}::later (variable) 7-7"),
                format!("{path}::value (constant) 8-8"),
            ],
            "{path}"
        );
        let typed = node(&parsed, "typed");
        assert_eq!(typed.visibility.as_deref(), Some("export"));
        assert_eq!(typed.has_explicit_return_type, !annotation.is_empty());
        assert_eq!(
            typed.signature.as_deref(),
            Some(&*format!("typed = (v){annotation}"))
        );
        assert!(!node(&parsed, "inferred").has_explicit_return_type);
    }
}

#[test]
fn shadowed_local_is_not_a_candidate() {
    let parsed = parse(
        "a.ts",
        "const config = { depth: 1 };\nfunction local(input: number) {\n  const config = { depth: input };\n  return config.depth;\n}\nfunction global() {\n  return config.depth;\n}\n",
    );
    assert_eq!(
        candidates(&parsed),
        ["7:9 value config in a.ts::global (function)"]
    );
}

#[test]
fn import_specifiers_yield_import_candidates() {
    let parsed = parse(
        "a.ts",
        "import Default, { named, original as alias, type Kind } from \"./a\";\nimport * as space from \"./b\";\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:7 import Default in a.ts (file)",
            "1:18 import named in a.ts (file)",
            "1:25 import original in a.ts (file)",
            "1:49 import Kind in a.ts (file)",
            "2:12 import space in a.ts (file)",
        ]
    );
    let parsed = parse(
        "a.cjs",
        "const fs = require(\"fs\");\nconst { join, base: renamed } = require(\"path\");\n",
    );
    assert!(parsed.nodes.is_empty());
    assert_eq!(
        candidates(&parsed),
        [
            "1:6 import fs in a.cjs (file)",
            "2:8 import join in a.cjs (file)",
            "2:14 import base in a.cjs (file)",
        ]
    );
    assert!(
        parsed
            .candidates
            .iter()
            .all(|c| c.role == CandidateRole::Import)
    );
}

#[test]
fn external_paths_are_outside_node_modules_and_ignored_declarations() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path().canonicalize().unwrap();
    let outside = root.parent().unwrap();
    let adapter = TypeScriptAdapter::new();
    let external = |path: &std::path::Path| adapter.external_path_predicate(path, repo.path());
    assert!(external(
        &outside.join("store/node_modules/react/index.d.ts")
    ));
    assert!(!external(&outside.join("lib/lib.dom.d.ts")));
    assert!(external(&root.join("node_modules/react/index.d.ts")));
    assert!(external(&root.join("packages/core/dist/index.d.ts")));
    assert!(!external(&root.join("packages/core/src/types.d.ts")));
    assert!(!external(&root.join("packages/core/src/index.ts")));
    assert!(!external(&root.join("node_modules/react/index.js")));
}

#[test]
fn export_clauses_and_jsdoc_mark_declarations() {
    let parsed = parse(
        "a.ts",
        "/** Kept. */\nexport function marked() {}\nfunction listed() {}\nclass Fallback {}\n/**\n * Bound.\n */\nconst bound = 1;\nnamespace Inner {\n  function listed() {}\n}\nexport { listed, bound as renamed };\nexport default Fallback;\n",
    );
    let exported: Vec<String> = parsed
        .nodes
        .iter()
        .filter(|node| node.visibility.as_deref() == Some("export"))
        .map(|node| node.path.to_string())
        .collect();
    assert_eq!(
        exported,
        [
            "a.ts::marked",
            "a.ts::listed",
            "a.ts::Fallback",
            "a.ts::bound"
        ]
    );
    assert_eq!(
        node(&parsed, "marked").doc_comment.as_deref(),
        Some("/** Kept. */")
    );
    assert_eq!(
        node(&parsed, "bound").doc_comment.as_deref(),
        Some("/**\n * Bound.\n */")
    );
}
