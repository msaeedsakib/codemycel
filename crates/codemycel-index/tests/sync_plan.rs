#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::store::{Store, StoreRead, StoredFile};
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Edge, EdgeKind, FileKind, FileRecord, ImplRecord,
    Language, Node, NodeKind, Occurrence, RepoPath, Root, SemanticPath, Span, Unresolved,
    UnresolvedReason, VerifiedBy,
};
use codemycel_index::{ChangeSet, ChangedSymbols, SymbolsDiff, SyncPlan};

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap()
}

fn record(file: &str, content: &str) -> FileRecord {
    FileRecord {
        path: path(file),
        root_id: None,
        hash: blake3::hash(content.as_bytes()),
        size: content.len() as u64,
        language: Some(Language::Rust),
        kind: FileKind::Code,
        parsed: true,
        skip_reason: None,
        scip_covered: false,
    }
}

fn symbol(file: &str, name: &str, kind: NodeKind, signature: &str, body: &str) -> Node {
    let file = path(file);
    Node {
        path: SemanticPath::symbol(&file, &[] as &[&str], name, None).unwrap(),
        kind,
        name: name.to_owned(),
        file: Some(file),
        span: Some(Span {
            bytes: ByteRange { start: 0, end: 10 },
            line_start: 1,
            line_end: 1,
        }),
        selection: Some(ByteRange { start: 3, end: 4 }),
        signature: Some(signature.to_owned()),
        has_explicit_return_type: signature.contains("->"),
        body_hash: Some(blake3::hash(body.as_bytes())),
        doc_comment: None,
        visibility: None,
    }
}

fn function(file: &str, name: &str) -> Node {
    symbol(
        file,
        name,
        NodeKind::Function,
        &format!("fn {name}() -> u8"),
        "{ 1 }",
    )
}

fn file_node(file: &str) -> Node {
    let file = path(file);
    Node {
        path: SemanticPath::for_file(&file).unwrap(),
        kind: NodeKind::File,
        name: file.as_str().to_owned(),
        file: Some(file),
        span: None,
        selection: None,
        signature: None,
        has_explicit_return_type: false,
        body_hash: None,
        doc_comment: None,
        visibility: None,
    }
}

fn edge(src: &Node, dst: &Node, kind: EdgeKind) -> Edge {
    Edge {
        src: src.node_ref(),
        dst: dst.node_ref(),
        kind,
        verified_by: VerifiedBy::Lsp,
        occurrences: vec![Occurrence { line: 1, col: 1 }],
    }
}

/// A store holding `nodes` (file nodes added) in their files and `edges` between them.
fn store(dir: &tempfile::TempDir, nodes: &[Node], edges: &[Edge]) -> Store {
    let mut store = Store::create(&dir.path().join("index.db")).unwrap();
    let files: BTreeSet<&RepoPath> = nodes.iter().filter_map(|n| n.file.as_ref()).collect();
    store
        .transaction(|tx| {
            let root = tx.set_root(&Root {
                path: path(""),
                language: Language::Rust,
            })?;
            for file in &files {
                let mut file_record = record(file.as_str(), file.as_str());
                file_record.root_id = Some(root);
                tx.upsert_file(&file_record)?;
            }
            let file_nodes: Vec<Node> = files.iter().map(|f| file_node(f.as_str())).collect();
            tx.insert_nodes(&file_nodes)?;
            tx.insert_nodes(nodes)?;
            tx.insert_edges(edges)
        })
        .unwrap();
    store
}

fn modified(files: &[&str]) -> ChangeSet {
    ChangeSet {
        modified: files.iter().map(|f| path(f)).collect(),
        ..ChangeSet::default()
    }
}

fn plan(
    store: &Store,
    changes: &ChangeSet,
    symbols: BTreeMap<RepoPath, ChangedSymbols>,
) -> BTreeSet<String> {
    let parsed: BTreeSet<RepoPath> = store
        .files()
        .unwrap()
        .into_iter()
        .map(|f| f.record.path)
        .filter(|f| !changes.deleted.contains(f))
        .collect();
    SyncPlan::build(changes, &symbols, &BTreeSet::new(), &parsed, store)
        .unwrap()
        .reverify
        .iter()
        .map(|f| f.as_str().to_owned())
        .collect()
}

fn diff_of(
    store: &Store,
    file: &str,
    new_nodes: &[Node],
    new_impls: &[ImplRecord],
) -> ChangedSymbols {
    let old: Vec<Node> = store
        .nodes_in_file(&path(file))
        .unwrap()
        .into_iter()
        .map(|s| s.node)
        .collect();
    let old_impls = store.impl_records_in_file(&path(file)).unwrap();
    SymbolsDiff::compute(&old, &old_impls, new_nodes, new_impls)
}

/// `b.rs::b` calls `a.rs::a`, which calls `c.rs::c`; `d.rs::d` is unrelated.
fn chain(dir: &tempfile::TempDir, a: Node) -> Store {
    let (b, c, d) = (
        function("b.rs", "b"),
        function("c.rs", "c"),
        function("d.rs", "d"),
    );
    let edges = [edge(&b, &a, EdgeKind::Calls), edge(&a, &c, EdgeKind::Calls)];
    store(dir, &[a, b, c, d], &edges)
}

fn names(files: &[&str]) -> BTreeSet<String> {
    files.iter().map(|f| (*f).to_owned()).collect()
}

#[test]
fn body_only_change_plans_only_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = chain(&dir, function("a.rs", "a"));
    let edited = symbol("a.rs", "a", NodeKind::Function, "fn a() -> u8", "{ 2 }");

    let diff = diff_of(&store, "a.rs", &[edited], &[]);
    assert!(diff.is_empty());
    let planned = plan(
        &store,
        &modified(&["a.rs"]),
        BTreeMap::from([(path("a.rs"), diff)]),
    );

    assert_eq!(planned, names(&["a.rs"]));
}

#[test]
fn signature_change_plans_files_with_edges_in_both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let store = chain(&dir, function("a.rs", "a"));
    let edited = symbol(
        "a.rs",
        "a",
        NodeKind::Function,
        "fn a(x: u8) -> u8",
        "{ 1 }",
    );

    let diff = diff_of(&store, "a.rs", &[edited], &[]);
    let planned = plan(
        &store,
        &modified(&["a.rs"]),
        BTreeMap::from([(path("a.rs"), diff)]),
    );

    assert_eq!(planned, names(&["a.rs", "b.rs", "c.rs"]));
}

#[test]
fn inferred_return_type_body_change_counts_as_signature_change() {
    let dir = tempfile::tempdir().unwrap();
    let inferred = |body| symbol("a.rs", "a", NodeKind::Function, "fn a()", body);
    let store = chain(&dir, inferred("{ 1 }"));

    let diff = diff_of(&store, "a.rs", &[inferred("{ \"one\" }")], &[]);
    assert_eq!(diff.changed.len(), 1);
    let planned = plan(
        &store,
        &modified(&["a.rs"]),
        BTreeMap::from([(path("a.rs"), diff)]),
    );

    assert_eq!(planned, names(&["a.rs", "b.rs", "c.rs"]));
}

#[test]
fn new_trait_impl_plans_type_and_trait_files() {
    let dir = tempfile::tempdir().unwrap();
    let foo = symbol("foo.rs", "Foo", NodeKind::Struct, "struct Foo", "");
    let shape = symbol("shape.rs", "Shape", NodeKind::Trait, "trait Shape", "");
    let unrelated = symbol("other.rs", "Shape", NodeKind::Function, "fn Shape()", "");
    let host = function("host.rs", "host");
    let store = store(&dir, &[foo, shape, unrelated, host.clone()], &[]);
    let record = ImplRecord {
        file: path("host.rs"),
        self_type_text: "crate::foo::Foo".to_owned(),
        trait_text: Some("Shape".to_owned()),
        span: Span {
            bytes: ByteRange { start: 20, end: 40 },
            line_start: 3,
            line_end: 4,
        },
        header: ByteRange { start: 20, end: 30 },
        body: ByteRange { start: 30, end: 40 },
        self_type_selection: ByteRange { start: 25, end: 28 },
    };

    let diff = diff_of(&store, "host.rs", &[host], &[record]);
    assert!(diff.is_empty());
    assert_eq!(diff.impls_added.len(), 1);
    let planned = plan(
        &store,
        &modified(&["host.rs"]),
        BTreeMap::from([(path("host.rs"), diff)]),
    );

    assert_eq!(planned, names(&["foo.rs", "host.rs", "shape.rs"]));
}

#[test]
fn added_symbol_plans_files_whose_candidates_name_it() {
    let dir = tempfile::tempdir().unwrap();
    let caller = function("caller.rs", "caller");
    let host = function("host.rs", "host");
    let store = {
        let mut store = store(&dir, &[caller.clone(), host.clone()], &[]);
        store
            .transaction(|tx| {
                tx.insert_unresolved(&[Unresolved {
                    candidate: Candidate {
                        file: path("caller.rs"),
                        enclosing: caller.node_ref(),
                        text: "helper".to_owned(),
                        role: CandidateRole::Call,
                        at: Occurrence { line: 1, col: 5 },
                    },
                    reason: UnresolvedReason::NoDefinition,
                }])
            })
            .unwrap();
        store
    };

    let diff = diff_of(
        &store,
        "host.rs",
        &[host, function("host.rs", "helper")],
        &[],
    );
    let planned = plan(
        &store,
        &modified(&["host.rs"]),
        BTreeMap::from([(path("host.rs"), diff)]),
    );

    assert_eq!(planned, names(&["caller.rs", "host.rs"]));
}

#[test]
fn delete_plans_dependents_and_cascades() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = chain(&dir, function("a.rs", "a"));
    let changes = ChangeSet {
        deleted: [path("b.rs")].into(),
        ..ChangeSet::default()
    };

    let diff = diff_of(&store, "b.rs", &[], &[]);
    assert_eq!(diff.removed.len(), 1);
    let planned = plan(&store, &changes, BTreeMap::from([(path("b.rs"), diff)]));
    assert_eq!(planned, names(&["a.rs"]));

    store
        .transaction(|tx| tx.delete_file(&path("b.rs")))
        .unwrap();
    let a = &store
        .node_by_semantic_path(&function("a.rs", "a").path, Some(NodeKind::Function))
        .unwrap()[0];
    assert!(store.edges_to(a.id, &[]).unwrap().is_empty());
    assert!(store.nodes_in_file(&path("b.rs")).unwrap().is_empty());
    assert_eq!(store.dump_edges().unwrap().lines().count(), 1);
}

#[test]
fn touched_but_identical_file_is_not_a_change() {
    let dir = tempfile::tempdir().unwrap();
    let store = chain(&dir, function("a.rs", "a"));
    let stored: Vec<StoredFile> = store.files().unwrap();
    let mut walked: Vec<FileRecord> = stored.iter().map(|f| f.record.clone()).collect();
    walked.retain(|f| f.path.as_str() != "d.rs");
    walked.push(record("e.rs", "new"));
    let edited = walked
        .iter_mut()
        .find(|f| f.path.as_str() == "c.rs")
        .unwrap();
    edited.hash = blake3::hash(b"edited");

    let changes = ChangeSet::between(&stored, &walked);

    assert_eq!(changes.added, [path("e.rs")].into());
    assert_eq!(changes.modified, [path("c.rs")].into());
    assert_eq!(changes.deleted, [path("d.rs")].into());
}
