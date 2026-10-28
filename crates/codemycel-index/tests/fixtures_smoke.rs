#![allow(clippy::unwrap_used)]

mod harness;

use std::path::Path;

use codemycel_core::{
    AdapterError, ByteRange, Candidate, CandidateRole, Edge, EdgeKind, Language, LanguageAdapter,
    Node, NodeKind, NodeRef, Occurrence, ParsedFile, RepoPath, Root, SemanticPath, SkipReason,
    Span, VerifiedBy,
};
use codemycel_index::Registry;
use harness::{Dump, index_fixture, index_fixture_with};

/// Claims `.rs`, roots at `crate/`, and reports one function `f` calling `g` in every file.
struct Stub;

impl LanguageAdapter for Stub {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn detect_roots(&self, _: &Path, _: &[RepoPath]) -> Result<Vec<Root>, AdapterError> {
        Ok(vec![Root {
            path: RepoPath::new("crate").unwrap(),
            language: Language::Rust,
        }])
    }
    fn parse(&self, file: &RepoPath, _: &[u8]) -> Result<ParsedFile, AdapterError> {
        let file_ref = NodeRef::new(SemanticPath::for_file(file).unwrap(), NodeKind::File);
        let f = NodeRef::new(
            SemanticPath::symbol(file, &[] as &[&str], "f", None).unwrap(),
            NodeKind::Function,
        );
        Ok(ParsedFile {
            nodes: vec![Node {
                path: f.path.clone(),
                kind: NodeKind::Function,
                name: "f".to_owned(),
                file: Some(file.clone()),
                span: Some(Span {
                    bytes: ByteRange { start: 0, end: 16 },
                    line_start: 1,
                    line_end: 1,
                }),
                selection: Some(ByteRange { start: 3, end: 4 }),
                signature: Some("fn f()".to_owned()),
                has_explicit_return_type: false,
                body_hash: None,
                doc_comment: None,
                visibility: None,
            }],
            impl_records: Vec::new(),
            candidates: vec![Candidate {
                file: file.clone(),
                enclosing: f.clone(),
                text: "g".to_owned(),
                role: CandidateRole::Call,
                at: Occurrence { line: 1, col: 9 },
            }],
            structural_edges: vec![Edge {
                src: file_ref,
                dst: f,
                kind: EdgeKind::Contains,
                verified_by: VerifiedBy::Syntactic,
                occurrences: Vec::new(),
            }],
        })
    }
    fn external_path_predicate(&self, _: &Path, _: &Path) -> bool {
        false
    }
}

#[test]
fn empty_fixture_produces_empty_dumps() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join(".gitignore"), "ignored.rs\n").unwrap();
    std::fs::write(repo.path().join("ignored.rs"), "fn hidden() {}\n").unwrap();
    let index = index_fixture(repo.path());
    for dump in [
        Dump::Nodes,
        Dump::Edges,
        Dump::Unresolved,
        Dump::ImplRecords,
    ] {
        assert_eq!(index.dump(dump), "", "{dump:?}");
    }
    assert_eq!(index.report.files, 1);
    assert_eq!(
        index.report.skipped,
        [(
            RepoPath::new(".gitignore").unwrap(),
            SkipReason::Unsupported
        )]
    );
}

#[test]
fn unrooted_code_file_is_listed_not_parsed() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("crate/src")).unwrap();
    std::fs::write(repo.path().join("crate/src/lib.rs"), "fn f() { g(); }\n").unwrap();
    std::fs::write(repo.path().join("loose.rs"), "fn f() { g(); }\n").unwrap();
    let index = index_fixture_with(repo.path(), Registry::with_adapters(vec![Box::new(Stub)]));
    assert_eq!(
        index.dump(Dump::Nodes),
        "crate/src/lib.rs\tfile\t1-1\ncrate/src/lib.rs::f\tfunction\t1-1\n"
    );
    assert_eq!(
        index.dump(Dump::Edges),
        "crate/src/lib.rs (file)\tcontains\tcrate/src/lib.rs::f (function)\t0\n"
    );
    assert_eq!(
        index.dump(Dump::Unresolved),
        "crate/src/lib.rs:1:9\tcall\tg\tserver_unavailable\n"
    );
    assert_eq!(
        index.report.skipped,
        [(RepoPath::new("loose.rs").unwrap(), SkipReason::NoRoot)]
    );
    assert_eq!(index.report.parsed, 1);
}
