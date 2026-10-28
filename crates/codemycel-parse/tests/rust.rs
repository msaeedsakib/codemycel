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
