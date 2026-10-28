#![allow(clippy::unwrap_used)]

use codemycel_core::{LanguageAdapter, ParsedFile, RepoPath};
use codemycel_parse::rust::RustAdapter;

fn parse(source: &str) -> ParsedFile {
    RustAdapter::new()
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

fn candidates(parsed: &ParsedFile) -> Vec<String> {
    parsed
        .candidates
        .iter()
        .map(|c| format!("{}:{} {} {}", c.at.line, c.at.col, c.role, c.text))
        .collect()
}

#[test]
fn trait_impl_methods_use_qualified_container() {
    let parsed = parse(
        "impl fmt::Display for Foo {\n    fn fmt(&self) {}\n}\nimpl fmt::Debug for Foo {\n    fn fmt(&self) {}\n}\nimpl Foo {\n    fn fmt(&self) {}\n}\n",
    );
    assert_eq!(
        paths(&parsed),
        [
            "src/lib.rs::<Foo as fmt::Display>::fmt (method)",
            "src/lib.rs::<Foo as fmt::Debug>::fmt (method)",
            "src/lib.rs::Foo::fmt (method)",
        ]
    );
}

#[test]
fn path_self_type_uses_qualified_container() {
    let parsed = parse(
        "mod ext {\n    impl super::Foo {\n        fn a() {}\n    }\n    impl Vec<crate::B> {\n        fn b() {}\n    }\n}\n",
    );
    assert_eq!(
        paths(&parsed),
        [
            "src/lib.rs::ext (module)",
            "src/lib.rs::ext::<super::Foo>::a (method)",
            "src/lib.rs::ext::<Vec<crate::B>>::b (method)",
        ]
    );
}

#[test]
fn blanket_impl_container_keeps_type_parameter() {
    let parsed = parse("impl<T: Bound> Render for T {\n    fn render(&self) -> String {}\n}\n");
    assert_eq!(
        paths(&parsed),
        ["src/lib.rs::<T as Render>::render (method)"]
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:8 type Bound",
            "1:15 implements Render",
            "2:24 type String"
        ]
    );
}

#[test]
fn shadowed_local_is_not_a_candidate() {
    let parsed = parse(
        "fn config() -> u32 { 1 }\nfn shadowed(limit: u32) -> u32 {\n    let config = limit;\n    config + limit\n}\nfn global() -> u32 {\n    config() + LIMIT\n}\n",
    );
    assert_eq!(candidates(&parsed), ["7:4 call config", "7:15 value LIMIT"]);
    assert_eq!(
        parsed.candidates[0].enclosing.to_string(),
        "src/lib.rs::global (function)"
    );
}

#[test]
fn use_paths_yield_import_candidates_per_leaf() {
    let parsed = parse(
        "use std::fmt;\nuse crate::a::{B, c::{D, E as F}, g::*};\npub use crate::shapes::{Circle, Square};\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:9 import fmt",
            "2:15 import B",
            "2:22 import D",
            "2:25 import E",
            "2:34 import g",
            "3:24 export Circle",
            "3:32 export Square",
        ]
    );
}

#[test]
fn self_super_and_crate_use_leaves_yield_imports() {
    let parsed = parse(
        "use std::io::{self, Read};\nuse super::*;\nuse crate::store::{self as st, Store};\npub use self::a::*;\nuse super::super::*;\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:9 import io",
            "1:20 import Read",
            "2:4 import super",
            "3:11 import store",
            "3:31 import Store",
            "4:14 export a",
            "5:11 import super",
        ]
    );
}

#[test]
fn bindings_do_not_hide_their_own_initialisers() {
    let parsed = parse(
        "use std::env::args;\nfn main() {\n    let args = args();\n    for item in item() {}\n    if let Some(v) = v() { v; }\n    while let Some(w) = w() {}\n    args;\n}\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:14 import args",
            "3:15 call args",
            "4:16 call item",
            "5:11 value Some",
            "5:21 call v",
            "6:14 value Some",
            "6:24 call w",
        ]
    );
}

#[test]
fn uppercase_pattern_identifiers_are_not_bindings() {
    let parsed = parse(
        "fn f(x: Option<u32>) -> Option<u32> {\n    match x {\n        None => None,\n        Some(MAX) => MAX,\n        Some(n) => n,\n    }\n}\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "1:8 type Option",
            "1:24 type Option",
            "3:16 value None",
            "4:8 value Some",
            "4:21 value MAX",
            "5:8 value Some",
        ]
    );
}

#[test]
fn generic_parameters_are_locals() {
    let parsed = parse("fn g<T: Bound, const N: usize>(t: T) -> [u8; N] {\n    [0; N]\n}\n");
    assert_eq!(candidates(&parsed), ["1:8 type Bound"]);
}

#[test]
fn union_is_a_struct_with_fields() {
    let parsed = parse("union U {\n    a: u32,\n    pub b: f32,\n}\n");
    assert_eq!(
        paths(&parsed),
        [
            "src/lib.rs::U (struct)",
            "src/lib.rs::U::a (field)",
            "src/lib.rs::U::b (field)",
        ]
    );
}

#[test]
fn macro_turbofish_is_classified_by_what_follows() {
    let parsed = parse(
        "fn f(s: &str) {\n    assert_eq!(s.parse::<u32>(), Vec::<u8>::new(), make::<u8>, wrap::<Option<u8>>(1));\n}\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "2:17 call parse",
            "2:44 call new",
            "2:51 value make",
            "2:63 call wrap",
            "2:70 value Option",
        ]
    );
}

#[test]
fn macro_token_trees_yield_calls_and_values() {
    let parsed = parse(
        "#[derive(Debug, Clone)]\nstruct S;\nfn f(x: u32) {\n    println!(\"{} {}\", total(x), a::b::LIMIT, x.len(), x.field, inner!(y), Shape::Empty);\n}\n",
    );
    assert_eq!(
        candidates(&parsed),
        [
            "4:22 call total",
            "4:38 value LIMIT",
            "4:47 call len",
            "4:70 value y",
            "4:81 value Empty",
        ]
    );
}

#[test]
fn visibility_body_and_doc_follow_the_source() {
    let parsed = parse(
        "pub mod a;\nmod b {\n    pub(super) const X: u32 = 1;\n    pub(in crate::b) fn f() -> u32 { X }\n}\n/// Documented.\n#[derive(Debug)]\npub(crate) struct S {\n    field: u32,\n}\n",
    );
    let found: Vec<String> = parsed
        .nodes
        .iter()
        .map(|node| {
            format!(
                "{} vis={:?} body={} returns={} doc={:?}",
                node.name,
                node.visibility,
                node.body_hash.is_some(),
                node.has_explicit_return_type,
                node.doc_comment,
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            "a vis=Some(\"pub\") body=false returns=false doc=None",
            "b vis=None body=true returns=false doc=None",
            "X vis=Some(\"pub(super)\") body=true returns=false doc=None",
            "f vis=Some(\"pub(in crate::b)\") body=true returns=true doc=None",
            "S vis=Some(\"pub(crate)\") body=true returns=false doc=Some(\"/// Documented.\")",
            "field vis=None body=false returns=false doc=None",
        ]
    );
    assert_eq!(
        parsed.nodes[2].signature.as_deref(),
        Some("pub(super) const X: u32")
    );
}

#[test]
fn impl_records_capture_self_type_selection_range() {
    let source = "impl<T> Wrapper<T> {}\nimpl super::Foo {}\nimpl Display for Bar {}\n";
    let parsed = parse(source);
    let selected: Vec<(&str, Option<&str>, &str)> = parsed
        .impl_records
        .iter()
        .map(|record| {
            let range = record.self_type_selection;
            (
                record.self_type_text.as_str(),
                record.trait_text.as_deref(),
                &source[range.start as usize..range.end as usize],
            )
        })
        .collect();
    assert_eq!(
        selected,
        [
            ("Wrapper<T>", None, "Wrapper"),
            ("super::Foo", None, "Foo"),
            ("Bar", Some("Display"), "Bar"),
        ]
    );
}
