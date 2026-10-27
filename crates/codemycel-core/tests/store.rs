#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError, StoreRead, Tx};
use codemycel_core::text::name_split;
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Edge, EdgeKind, FileKind, FileRecord, ImplRecord,
    Language, Node, NodeKind, NodeRef, Occurrence, RepoPath, Root, RootId, SearchDoc, SemanticPath,
    Span, Unresolved, UnresolvedReason, VerifiedBy,
};
use tempfile::TempDir;

fn fresh() -> (TempDir, PathBuf, Store) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.db");
    let store = Store::create(&db).unwrap();
    (dir, db, store)
}

fn repo(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn sem(text: &str) -> SemanticPath {
    text.parse().unwrap()
}

fn node_ref(text: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(sem(text), kind)
}

fn span(line_start: u32, line_end: u32) -> Span {
    Span {
        bytes: ByteRange {
            start: line_start * 10,
            end: line_end * 10,
        },
        line_start,
        line_end,
    }
}

fn file_record(path: &str, root_id: Option<RootId>) -> FileRecord {
    FileRecord {
        path: repo(path),
        root_id,
        hash: blake3::hash(path.as_bytes()),
        size: 120,
        language: Some(Language::Rust),
        kind: FileKind::Code,
        parsed: true,
        skip_reason: None,
        scip_covered: false,
    }
}

fn node(path: &str, kind: NodeKind, file: &str, lines: (u32, u32)) -> Node {
    let path = sem(path);
    Node {
        name: path.name().to_owned(),
        path,
        kind,
        file: Some(repo(file)),
        span: Some(span(lines.0, lines.1)),
        selection: None,
        signature: None,
        has_explicit_return_type: false,
        body_hash: None,
        doc_comment: None,
        visibility: None,
    }
}

fn edge(src: NodeRef, kind: EdgeKind, dst: NodeRef, occurrences: &[(u32, u32)]) -> Edge {
    Edge {
        src,
        dst,
        kind,
        verified_by: VerifiedBy::Syntactic,
        occurrences: occurrences.iter().copied().map(Occurrence::from).collect(),
    }
}

fn search_doc(node: &Node) -> SearchDoc {
    SearchDoc {
        node: node.node_ref(),
        name: node.name.clone(),
        name_split: name_split(&node.name),
        signature: node.signature.clone(),
        doc_comment: node.doc_comment.clone(),
        path: node.file.clone(),
    }
}

struct Seed {
    files: Vec<FileRecord>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    unresolved: Vec<Unresolved>,
    impls: Vec<ImplRecord>,
}

fn seed_data(root: RootId) -> Seed {
    let a = "src/a.rs";
    let b = "src/b.rs";
    let file_a = node_ref(a, NodeKind::File);
    let foo_struct = node_ref("src/a.rs::Foo", NodeKind::Struct);
    let new = node_ref("src/a.rs::Foo::new", NodeKind::Method);
    let main = node_ref("src/b.rs::main", NodeKind::Function);
    Seed {
        files: vec![file_record(a, Some(root)), file_record(b, Some(root))],
        nodes: vec![
            node(a, NodeKind::File, a, (1, 20)),
            node("src/a.rs::Foo", NodeKind::Struct, a, (1, 3)),
            node("src/a.rs::Foo::new", NodeKind::Method, a, (5, 7)),
            node(b, NodeKind::File, b, (1, 9)),
            node("src/b.rs::main", NodeKind::Function, b, (1, 9)),
        ],
        edges: vec![
            edge(file_a, EdgeKind::Contains, foo_struct.clone(), &[]),
            edge(foo_struct.clone(), EdgeKind::Contains, new.clone(), &[]),
            edge(main.clone(), EdgeKind::Calls, new, &[(3, 4)]),
            edge(
                main.clone(),
                EdgeKind::References,
                foo_struct,
                &[(3, 4), (4, 8)],
            ),
        ],
        unresolved: vec![
            Unresolved {
                candidate: Candidate {
                    file: repo(b),
                    enclosing: main.clone(),
                    text: "println".to_owned(),
                    role: CandidateRole::Call,
                    at: Occurrence { line: 5, col: 4 },
                },
                reason: UnresolvedReason::ExternalDependency,
            },
            Unresolved {
                candidate: Candidate {
                    file: repo(a),
                    enclosing: node_ref("src/a.rs::Foo::new", NodeKind::Method),
                    text: "Bar".to_owned(),
                    role: CandidateRole::Type,
                    at: Occurrence { line: 6, col: 8 },
                },
                reason: UnresolvedReason::NoDefinition,
            },
        ],
        impls: vec![ImplRecord {
            file: repo(a),
            self_type_text: "Foo".to_owned(),
            trait_text: None,
            span: span(4, 8),
            header: ByteRange { start: 40, end: 48 },
            body: ByteRange { start: 49, end: 80 },
            self_type_selection: ByteRange { start: 45, end: 48 },
        }],
    }
}

fn root(tx: &mut Tx<'_>) -> RootId {
    tx.set_root(&Root {
        path: repo(""),
        language: Language::Rust,
    })
    .unwrap()
}

fn seed(store: &mut Store) -> Seed {
    store
        .transaction(|tx| {
            let data = seed_data(root(tx));
            for file in &data.files {
                tx.upsert_file(file)?;
            }
            tx.insert_nodes(&data.nodes)?;
            tx.insert_edges(&data.edges)?;
            tx.insert_unresolved(&data.unresolved)?;
            tx.insert_impl_records(&data.impls)?;
            let docs: Vec<SearchDoc> = data.nodes.iter().map(search_doc).collect();
            tx.insert_search_docs(&docs)?;
            Ok::<_, StoreError>(data)
        })
        .unwrap()
}

#[test]
fn create_then_open_succeeds() {
    let (_dir, db, store) = fresh();
    drop(store);

    let store = Store::open(&db, false).unwrap();
    assert_eq!(
        store.meta_get("schema_version").unwrap(),
        Some(SCHEMA_VERSION.to_string())
    );
    assert_eq!(
        store.meta_get("codemycel_version").unwrap(),
        Some(env!("CARGO_PKG_VERSION").to_owned())
    );
    drop(store);

    assert!(Store::open(&db, true).is_ok());
    assert!(matches!(
        Store::create(&db),
        Err(StoreError::AlreadyExists(_))
    ));
}

#[test]
fn node_identity_is_path_plus_kind() {
    let (_dir, _db, mut store) = fresh();
    let file = "src/a.ts";
    let constant = node("src/a.ts::Foo", NodeKind::Constant, file, (1, 1));
    let alias = node("src/a.ts::Foo", NodeKind::TypeAlias, file, (2, 2));
    store
        .transaction(|tx| {
            tx.upsert_file(&file_record(file, None))?;
            tx.insert_nodes(&[constant.clone(), alias.clone()])
        })
        .unwrap();

    assert_eq!(
        store
            .node_by_semantic_path(&sem("src/a.ts::Foo"), None)
            .unwrap()
            .len(),
        2
    );
    let found = store
        .node_by_semantic_path(&sem("src/a.ts::Foo"), Some(NodeKind::Constant))
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].node, constant);

    let duplicate = store.transaction(|tx| tx.insert_nodes(std::slice::from_ref(&constant)));
    assert!(matches!(duplicate, Err(StoreError::Sqlite(_))));
    assert_eq!(store.counts().unwrap().nodes, 2);
}

#[test]
fn schema_mismatch_triggers_rebuild() {
    let (_dir, db, mut store) = fresh();
    store
        .transaction(|tx| tx.meta_set("schema_version", "0"))
        .unwrap();
    drop(store);

    match Store::open(&db, false) {
        Err(StoreError::SchemaMismatch { found, expected }) => {
            assert_eq!(found, "0");
            assert_eq!(expected, SCHEMA_VERSION);
        }
        other => panic!("expected SchemaMismatch, got {other:?}"),
    }

    let garbage = db.with_file_name("garbage.db");
    std::fs::write(
        &garbage,
        b"this is not a sqlite database, just text padding it out",
    )
    .unwrap();
    assert!(matches!(
        Store::open(&garbage, true),
        Err(StoreError::SchemaMismatch { .. })
    ));
}

#[test]
fn insert_and_read_back_node_by_semantic_path() {
    let (_dir, _db, mut store) = fresh();
    let full = Node {
        path: sem("src/fmt.rs::<Vec<Foo> as Display>::fmt"),
        kind: NodeKind::Method,
        name: "fmt".to_owned(),
        file: Some(repo("src/fmt.rs")),
        span: Some(span(3, 9)),
        selection: Some(ByteRange { start: 33, end: 36 }),
        signature: Some("fn fmt(&self, f: &mut Formatter) -> fmt::Result".to_owned()),
        has_explicit_return_type: true,
        body_hash: Some(blake3::hash(b"body")),
        doc_comment: Some("/// Formats.".to_owned()),
        visibility: Some("pub(crate)".to_owned()),
    };
    let (root_id, ids) = store
        .transaction(|tx| {
            let root_id = root(tx);
            tx.upsert_file(&file_record("src/fmt.rs", Some(root_id)))?;
            Ok::<_, StoreError>((root_id, tx.insert_nodes(std::slice::from_ref(&full))?))
        })
        .unwrap();

    let found = store
        .node_by_semantic_path(&full.path, Some(NodeKind::Method))
        .unwrap();
    assert_eq!(found.len(), 1);
    let stored = &found[0];
    assert_eq!(stored.node, full);
    assert_eq!(stored.id, ids[0]);
    assert_eq!(stored.root_id, Some(root_id));
    assert_eq!(store.node_by_id(ids[0]).unwrap().as_ref(), Some(stored));
}

#[test]
fn insert_nodes_rejects_unknown_file() {
    let (_dir, _db, mut store) = fresh();
    let orphan = node("src/x.rs::f", NodeKind::Function, "src/x.rs", (1, 1));
    let result = store.transaction(|tx| tx.insert_nodes(&[orphan]));
    assert!(matches!(result, Err(StoreError::UnknownFile(_))));
}

#[test]
fn edge_upsert_merges_occurrences() {
    let (_dir, _db, mut store) = fresh();
    let main = node_ref("src/b.rs::main", NodeKind::Function);
    let new = node_ref("src/a.rs::Foo::new", NodeKind::Method);
    seed(&mut store);
    store
        .transaction(|tx| {
            tx.insert_edges(&[edge(
                main.clone(),
                EdgeKind::Calls,
                new.clone(),
                &[(9, 1), (3, 4), (2, 0)],
            )])
        })
        .unwrap();

    let main_id = store.node_by_semantic_path(&main.path, None).unwrap()[0].id;
    let calls = store.edges_from(main_id, &[EdgeKind::Calls]).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].edge.dst, new);
    let lines: Vec<(u32, u32)> = calls[0]
        .edge
        .occurrences
        .iter()
        .map(|o| (o.line, o.col))
        .collect();
    assert_eq!(lines, vec![(2, 0), (3, 4), (9, 1)]);
    assert!(calls[0].root_id.is_some());

    let new_id = store.node_by_semantic_path(&new.path, None).unwrap()[0].id;
    assert_eq!(store.edges_to(new_id, &[EdgeKind::Calls]).unwrap(), calls);
}

#[test]
fn delete_file_cascades() {
    let (_dir, _db, mut store) = fresh();
    seed(&mut store);
    let a = repo("src/a.rs");
    store.transaction(|tx| tx.delete_file(&a)).unwrap();

    assert!(store.file_by_path(&a).unwrap().is_none());
    assert!(store.nodes_in_file(&a).unwrap().is_empty());
    assert!(store.unresolved_for_file(&a).unwrap().is_empty());
    assert!(store.impl_records_in_file(&a).unwrap().is_empty());
    assert!(store.search_fts("foo", 10).unwrap().is_empty());
    assert_eq!(store.dump_impl_records().unwrap(), "");
    assert_eq!(
        store.dump_nodes().unwrap(),
        "src/b.rs\tfile\t1-9\nsrc/b.rs::main\tfunction\t1-9\n"
    );
    assert_eq!(store.dump_edges().unwrap(), "");
    assert_eq!(
        store.dump_unresolved().unwrap(),
        "src/b.rs:5:4\tcall\tprintln\texternal_dependency\n"
    );
    assert_eq!(store.search_fts("main", 10).unwrap().len(), 1);
}

#[test]
fn paths_are_forward_slash_and_relative() {
    let (_dir, _db, mut store) = fresh();
    store
        .transaction(|tx| tx.upsert_file(&file_record("src\\db\\store.rs", None)))
        .unwrap();

    let files = store.files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].record.path.as_str(), "src/db/store.rs");
    assert!(
        store
            .file_by_path(&repo("src/db/store.rs"))
            .unwrap()
            .is_some()
    );

    assert!(RepoPath::new("/home/me/repo/src/a.rs").is_err());
    assert!(RepoPath::new("C:\\repo\\src\\a.rs").is_err());
}

#[test]
fn fts_finds_split_name() {
    let (_dir, _db, mut store) = fresh();
    let limiter = node(
        "src/rate.rs::RateLimiter",
        NodeKind::Struct,
        "src/rate.rs",
        (1, 4),
    );
    store
        .transaction(|tx| {
            tx.upsert_file(&file_record("src/rate.rs", None))?;
            tx.insert_nodes(std::slice::from_ref(&limiter))?;
            tx.insert_search_docs(&[search_doc(&limiter)])
        })
        .unwrap();

    let hits = store.search_fts("limiter", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "RateLimiter");
    assert_eq!(hits[0].kind, NodeKind::Struct);
    assert_eq!(hits[0].file, Some(repo("src/rate.rs")));
}

#[test]
fn name_split_handles_camel_snake_kebab_and_digits() {
    assert_eq!(name_split("RateLimiter"), "rate limiter");
    assert_eq!(name_split("rateLimiter"), "rate limiter");
    assert_eq!(name_split("rate_limiter"), "rate limiter");
    assert_eq!(name_split("rate-limiter"), "rate limiter");
    assert_eq!(name_split("http2Server"), "http 2 server");
    assert_eq!(name_split("HTTPServer"), "http server");
    assert_eq!(name_split("__RATE__LIMIT_2__"), "rate limit 2");
}

#[test]
fn name_counts_add_removes_row_at_zero() {
    let (_dir, db, mut store) = fresh();
    store
        .transaction(|tx| {
            tx.name_counts_add("Foo", 2)?;
            tx.name_counts_add("Foo", -1)
        })
        .unwrap();
    assert_eq!(store.name_count("Foo").unwrap(), 1);

    store
        .transaction(|tx| tx.name_counts_add("Foo", -1))
        .unwrap();
    assert_eq!(store.name_count("Foo").unwrap(), 0);
    let probe = rusqlite::Connection::open(&db).unwrap();
    let rows: i64 = probe
        .query_row("SELECT count(*) FROM name_counts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn dumps_are_sorted_and_stable() {
    let (_dir, _db, mut forward) = fresh();
    let data = seed(&mut forward);

    let (_dir2, _db2, mut backward) = fresh();
    backward
        .transaction(|tx| {
            let root_id = root(tx);
            let mut data = seed_data(root_id);
            data.files.reverse();
            data.nodes.reverse();
            data.edges.reverse();
            data.unresolved.reverse();
            for file in &data.files {
                tx.upsert_file(file)?;
            }
            tx.insert_nodes(&data.nodes)?;
            tx.insert_edges(&data.edges)?;
            tx.insert_unresolved(&data.unresolved)?;
            tx.insert_impl_records(&data.impls)
        })
        .unwrap();

    assert_eq!(
        forward.dump_nodes().unwrap(),
        "src/a.rs\tfile\t1-20\n\
         src/a.rs::Foo\tstruct\t1-3\n\
         src/a.rs::Foo::new\tmethod\t5-7\n\
         src/b.rs\tfile\t1-9\n\
         src/b.rs::main\tfunction\t1-9\n"
    );
    assert_eq!(
        forward.dump_edges().unwrap(),
        "src/a.rs (file)\tcontains\tsrc/a.rs::Foo (struct)\t0\n\
         src/a.rs::Foo (struct)\tcontains\tsrc/a.rs::Foo::new (method)\t0\n\
         src/b.rs::main (function)\tcalls\tsrc/a.rs::Foo::new (method)\t1\n\
         src/b.rs::main (function)\treferences\tsrc/a.rs::Foo (struct)\t2\n"
    );
    assert_eq!(
        forward.dump_unresolved().unwrap(),
        "src/a.rs:6:8\ttype\tBar\tno_definition\n\
         src/b.rs:5:4\tcall\tprintln\texternal_dependency\n"
    );
    assert_eq!(
        forward.dump_impl_records().unwrap(),
        "src/a.rs\tFoo\t-\t4-8\n"
    );

    assert_eq!(
        forward.dump_nodes().unwrap(),
        backward.dump_nodes().unwrap()
    );
    assert_eq!(
        forward.dump_edges().unwrap(),
        backward.dump_edges().unwrap()
    );
    assert_eq!(
        forward.dump_unresolved().unwrap(),
        backward.dump_unresolved().unwrap()
    );
    assert_eq!(
        forward.dump_impl_records().unwrap(),
        backward.dump_impl_records().unwrap()
    );
    assert_eq!(data.nodes.len(), 5);
}

#[test]
fn transaction_rolls_back_on_error() {
    let (_dir, _db, mut store) = fresh();
    let result = store.transaction(|tx| {
        tx.meta_set("indexed_commit", "abc")?;
        Err::<(), _>(StoreError::UnknownFile(repo("nope.rs")))
    });
    assert!(result.is_err());
    assert_eq!(store.meta_get("indexed_commit").unwrap(), None);
    assert_eq!(store.commit_count(), 0);

    store
        .transaction(|tx| tx.meta_set("indexed_commit", "abc"))
        .unwrap();
    assert_eq!(store.commit_count(), 1);
}

#[test]
fn wal_mode_is_on() {
    let (_dir, db, store) = fresh();
    drop(store);
    let _store = Store::open(&db, false).unwrap();
    let probe = rusqlite::Connection::open(&db).unwrap();
    let mode: String = probe
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
}
