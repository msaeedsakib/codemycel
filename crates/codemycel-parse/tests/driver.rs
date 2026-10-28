#![allow(clippy::unwrap_used)]

use codemycel_core::{CandidateRole, EdgeKind, NodeKind, ParsedFile, RepoPath};
use codemycel_parse::driver::{Driver, LanguageHooks, QuerySet};

const SYMBOLS: &str = r#"
(mod_item name: (identifier) @def.name body: (declaration_list) @def.body) @def.module
(struct_item name: (type_identifier) @def.name body: (_)? @def.body) @def.struct
(function_item
  (visibility_modifier)? @def.visibility
  name: (identifier) @def.name
  return_type: (_)? @def.return_type
  body: (block) @def.body) @def.function
(impl_item
  trait: (_)? @impl.trait
  type: (_) @impl.type
  body: (declaration_list) @impl.body) @impl
"#;

const CANDIDATES: &str = r#"
(call_expression function: (identifier) @cand.call)
(call_expression function: (scoped_identifier name: (identifier) @cand.call))
(arguments (identifier) @cand.value)
"#;

const LOCALS: &str = r#"
(function_item) @local.scope
(block) @local.scope
(let_declaration pattern: (identifier) @local.definition)
(parameter pattern: (identifier) @local.definition)
"#;

struct Hooks;

impl LanguageHooks for Hooks {
    fn impl_container(&self, self_type: &str, trait_text: Option<&str>) -> String {
        match trait_text {
            Some(trait_text) => format!("<{self_type} as {trait_text}>"),
            None => self_type.to_owned(),
        }
    }

    fn is_doc_comment(&self, text: &str) -> bool {
        text.starts_with("///")
    }

    fn doc_skip_kinds(&self) -> &'static [&'static str] {
        &["attribute_item"]
    }
}

fn parse(source: &str) -> ParsedFile {
    let driver = Driver::new(
        tree_sitter_rust::LANGUAGE.into(),
        QuerySet {
            symbols: SYMBOLS,
            candidates: CANDIDATES,
            locals: LOCALS,
        },
        Box::new(Hooks),
    )
    .unwrap();
    driver
        .parse(&RepoPath::new("src/lib.rs").unwrap(), source.as_bytes())
        .unwrap()
}

fn paths(parsed: &ParsedFile) -> Vec<String> {
    parsed
        .nodes
        .iter()
        .map(|node| format!("{} ({})", node.path, node.kind))
        .collect()
}

#[test]
fn nested_containers_build_semantic_paths() {
    let parsed = parse(
        "mod outer {\n    mod inner {\n        fn leaf() {}\n    }\n}\nstruct Foo;\nimpl Foo {\n    fn new() {}\n}\nimpl Clone for Foo {\n    fn clone() {}\n}\n",
    );
    assert_eq!(
        paths(&parsed),
        [
            "src/lib.rs::outer (module)",
            "src/lib.rs::outer::inner (module)",
            "src/lib.rs::outer::inner::leaf (function)",
            "src/lib.rs::Foo (struct)",
            "src/lib.rs::Foo::new (function)",
            "src/lib.rs::<Foo as Clone>::clone (function)",
        ]
    );
    let edges: Vec<String> = parsed
        .structural_edges
        .iter()
        .map(|edge| format!("{} {} {}", edge.src, edge.kind, edge.dst))
        .collect();
    assert!(edges.contains(&"src/lib.rs (file) contains src/lib.rs::outer (module)".to_owned()));
    assert!(edges.contains(
        &"src/lib.rs::outer (module) defines src/lib.rs::outer::inner (module)".to_owned()
    ));
    assert!(
        edges.contains(&"src/lib.rs (file) contains src/lib.rs::Foo::new (function)".to_owned())
    );
    assert!(
        edges
            .iter()
            .all(|edge| !edge.contains("contains src/lib.rs::outer::inner "))
    );
    assert_eq!(parsed.impl_records.len(), 2);
    assert_eq!(parsed.impl_records[1].trait_text.as_deref(), Some("Clone"));
    assert_eq!(parsed.impl_records[1].self_type_text, "Foo");
}

#[test]
fn same_name_siblings_get_source_order_suffixes() {
    let parsed = parse("fn f() {}\nstruct f;\nfn f() {}\nfn f() {}\n");
    assert_eq!(
        paths(&parsed),
        [
            "src/lib.rs::f (function)",
            "src/lib.rs::f (struct)",
            "src/lib.rs::f[1] (function)",
            "src/lib.rs::f[2] (function)",
        ]
    );
}

#[test]
fn local_definitions_suppress_candidates() {
    let parsed = parse(
        "fn config() {}\nfn a(input: u32) {\n    let config = 1;\n    use_it(config, input);\n}\nfn b() {\n    use_it(config);\n    Foo::make();\n}\n",
    );
    let found: Vec<String> = parsed
        .candidates
        .iter()
        .map(|c| {
            format!(
                "{}:{} {} {} in {}",
                c.at.line, c.at.col, c.role, c.text, c.enclosing
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            "4:4 call use_it in src/lib.rs::a (function)",
            "7:4 call use_it in src/lib.rs::b (function)",
            "7:11 value config in src/lib.rs::b (function)",
            "8:9 call make in src/lib.rs::b (function)",
        ]
    );
    assert_eq!(parsed.candidates[2].role, CandidateRole::Value);
}

#[test]
fn signature_is_single_line_up_to_body() {
    let parsed =
        parse("pub fn spread(\r\n    a: u32,\r\n    b: u32,\r\n) -> u32 {\r\n    a\r\n}\r\n");
    let node = &parsed.nodes[0];
    assert_eq!(
        node.signature.as_deref(),
        Some("pub fn spread( a: u32, b: u32, ) -> u32")
    );
    assert_eq!(node.visibility.as_deref(), Some("pub"));
    assert!(node.has_explicit_return_type);
    assert!(node.body_hash.is_some());
    let span = node.span.unwrap();
    assert_eq!((span.line_start, span.line_end), (1, 6));
}

#[test]
fn doc_comment_is_attached_to_following_item() {
    let parsed = parse(
        "/// not this one\nfn before() {}\n\n/// First line.\n/// Second line.\n// plain note\n#[derive(Debug)]\nstruct Documented;\n\nfn bare() {}\n",
    );
    let doc = |name: &str| {
        parsed
            .nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap()
            .doc_comment
            .clone()
    };
    assert_eq!(doc("before").as_deref(), Some("/// not this one"));
    assert_eq!(
        doc("Documented").as_deref(),
        Some("/// First line.\n/// Second line.")
    );
    assert_eq!(doc("bare"), None);
}

#[test]
fn bom_keeps_lines_and_columns_and_shifts_bytes() {
    let plain = parse("fn f() { g(); }\n");
    let with_bom = parse("\u{feff}fn f() { g(); }\n");
    assert_eq!(plain.candidates, with_bom.candidates);
    let (a, b) = (
        plain.nodes[0].span.unwrap(),
        with_bom.nodes[0].span.unwrap(),
    );
    assert_eq!((a.line_start, a.line_end), (b.line_start, b.line_end));
    assert_eq!(a.bytes.start + 3, b.bytes.start);
}

#[test]
fn overload_signatures_collapse_into_implementation() {
    let driver = Driver::new(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        QuerySet {
            symbols: r#"
(function_signature name: (identifier) @def.name) @def.function @def.overload
(function_declaration name: (identifier) @def.name body: (statement_block) @def.body) @def.function
"#,
            candidates: "(type_identifier) @cand.type",
            locals: "(statement_block) @local.scope",
        },
        Box::new(Hooks),
    )
    .unwrap();
    let parsed = driver
        .parse(
            &RepoPath::new("a.ts").unwrap(),
            b"function f(a: A): void;\nfunction f(a: B): void;\nfunction f(a: any) {}\ndeclare function g(): C;\ndeclare function g(x: D): C;\n",
        )
        .unwrap();
    assert_eq!(paths(&parsed), ["a.ts::f (function)", "a.ts::g (function)"]);
    assert_eq!(parsed.nodes[0].span.unwrap().line_start, 3);
    assert_eq!(parsed.nodes[1].span.unwrap().line_start, 4);
    let enclosing: Vec<String> = parsed
        .candidates
        .iter()
        .map(|c| format!("{} {}", c.text, c.enclosing))
        .collect();
    assert_eq!(
        enclosing,
        [
            "A a.ts::f (function)",
            "B a.ts::f (function)",
            "C a.ts::g (function)",
            "D a.ts::g (function)",
            "C a.ts::g (function)",
        ]
    );
    assert!(
        parsed
            .structural_edges
            .iter()
            .all(|edge| edge.kind == EdgeKind::Contains && edge.src.kind == NodeKind::File)
    );
}

#[test]
fn unknown_capture_names_are_rejected() {
    let result = Driver::new(
        tree_sitter_rust::LANGUAGE.into(),
        QuerySet {
            symbols: "(function_item name: (identifier) @def.nmae) @def.function",
            candidates: "",
            locals: "",
        },
        Box::new(Hooks),
    );
    assert!(result.is_err());
}
