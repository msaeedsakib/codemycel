#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_core::store::{Store, StoreRead, StoredNote};
use codemycel_core::{
    EdgeKind, FileKind, FileRecord, Language, Node, NodeKind, NodeRef, Note, NoteId, NoteKind,
    RepoPath, SemanticPath,
};
use codemycel_index::NotesReconciler;
use codemycel_index::notes::REPOSITORY_TARGET;

fn symbol(file: &str, containers: &[&str], name: &str, kind: NodeKind) -> Node {
    let file = RepoPath::new(file).unwrap();
    Node {
        path: SemanticPath::symbol(&file, containers, name, None).unwrap(),
        kind,
        name: name.to_owned(),
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

fn store_with(dir: &Path, nodes: &[Node]) -> Store {
    let mut store = Store::create(&dir.join("index.db")).unwrap();
    store
        .transaction(|tx| {
            for node in nodes {
                let path = node.file.clone().unwrap();
                if tx.file_by_path(&path)?.is_none() {
                    tx.upsert_file(&FileRecord {
                        path: path.clone(),
                        root_id: None,
                        hash: blake3::hash(path.as_str().as_bytes()),
                        size: 1,
                        language: Some(Language::Rust),
                        kind: FileKind::Code,
                        parsed: true,
                        skip_reason: None,
                        scip_covered: false,
                    })?;
                }
            }
            tx.insert_nodes(nodes).map(|_| ())
        })
        .unwrap();
    store
}

fn note(target: &str, body: &str) -> Note {
    Note {
        uuid: NotesReconciler::new_id(),
        target_semantic_path: target.to_owned(),
        kind: NoteKind::Note,
        body: body.to_owned(),
        created: "2026-10-28T09:00:00Z".to_owned(),
        updated: "2026-10-28T09:00:00Z".to_owned(),
    }
}

/// The notes stages of a sync, in their order, in one transaction.
fn reconcile(store: &mut Store, notes_dir: &Path) {
    store
        .transaction(|tx| {
            NotesReconciler::load(tx, notes_dir)?;
            NotesReconciler::attach(tx)
        })
        .unwrap();
}

fn stored(store: &Store, uuid: &NoteId) -> StoredNote {
    store.note_by_uuid(uuid).unwrap().unwrap()
}

fn annotated(store: &Store, note: &StoredNote) -> Vec<NodeRef> {
    store
        .edges_from(note.node_id, &[EdgeKind::Annotates])
        .unwrap()
        .into_iter()
        .map(|e| e.edge.dst)
        .collect()
}

fn set_file_nodes(store: &mut Store, file: &str, nodes: &[Node]) {
    store
        .transaction(|tx| {
            tx.replace_file_nodes(&RepoPath::new(file).unwrap(), nodes, &[], &[], &[])
        })
        .unwrap();
}

#[test]
fn note_orphans_and_reattaches() {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let foo = symbol("src/a.rs", &["<Foo as Display>"], "fmt", NodeKind::Method);
    let mut store = store_with(dir.path(), std::slice::from_ref(&foo));
    let uuid = NotesReconciler::write(
        &mut store,
        &notes_dir,
        note(foo.path.as_str(), "Formats with padding."),
    )
    .unwrap();
    let attached = stored(&store, &uuid);
    assert_eq!(attached.target, Some(foo.node_ref()));
    assert!(!attached.orphaned);
    assert_eq!(annotated(&store, &attached), vec![foo.node_ref()]);

    set_file_nodes(&mut store, "src/a.rs", &[]);
    reconcile(&mut store, &notes_dir);
    let orphan = stored(&store, &uuid);
    assert!(orphan.orphaned);
    assert_eq!(orphan.target, None);
    assert_eq!(orphan.note.target_semantic_path, foo.path.as_str());
    assert_eq!(orphan.note.body, "Formats with padding.");
    assert!(annotated(&store, &orphan).is_empty());

    set_file_nodes(&mut store, "src/a.rs", std::slice::from_ref(&foo));
    reconcile(&mut store, &notes_dir);
    let back = stored(&store, &uuid);
    assert!(!back.orphaned);
    assert_eq!(back.target, Some(foo.node_ref()));
    assert_eq!(annotated(&store, &back), vec![foo.node_ref()]);
}

#[test]
fn note_file_is_source_of_truth_over_db_row() {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let foo = symbol("src/a.rs", &[], "foo", NodeKind::Function);
    let bar = symbol("src/b.rs", &[], "bar", NodeKind::Function);
    let mut store = store_with(dir.path(), &[foo.clone(), bar.clone()]);
    let edited =
        NotesReconciler::write(&mut store, &notes_dir, note(foo.path.as_str(), "old")).unwrap();
    let broken =
        NotesReconciler::write(&mut store, &notes_dir, note(foo.path.as_str(), "kept")).unwrap();
    let removed =
        NotesReconciler::write(&mut store, &notes_dir, note(foo.path.as_str(), "gone")).unwrap();
    let file = |uuid: &NoteId| notes_dir.join(format!("{}.md", uuid.0));
    let hand_written = NoteId("6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b".to_owned());

    std::fs::write(
        file(&edited),
        format!(
            "---\ntarget: \"{}\"\nkind: note\ncreated: 2026-10-28T09:00:00Z\nupdated: 2026-10-29T08:00:00Z\n---\nnew body\n",
            bar.path
        ),
    )
    .unwrap();
    std::fs::write(file(&broken), "not a note").unwrap();
    std::fs::remove_file(file(&removed)).unwrap();
    std::fs::write(
        file(&hand_written),
        "---\r\ntarget: src/a.rs::foo\r\nkind: note\r\ncreated: 2026-10-28T09:00:00Z\r\nupdated: 2026-10-28T09:00:00Z\r\n---\r\nadded by hand\r\n",
    )
    .unwrap();
    std::fs::write(notes_dir.join("README.md"), "not a note either").unwrap();
    reconcile(&mut store, &notes_dir);

    let edited = stored(&store, &edited);
    assert_eq!(edited.note.body, "new body");
    assert_eq!(edited.note.updated, "2026-10-29T08:00:00Z");
    assert_eq!(edited.target, Some(bar.node_ref()));
    assert_eq!(stored(&store, &broken).note.body, "kept");
    assert!(store.note_by_uuid(&removed).unwrap().is_none());
    let note_node = SemanticPath::note(&removed.0).unwrap();
    assert!(
        store
            .node_by_semantic_path(&note_node, Some(NodeKind::Note))
            .unwrap()
            .is_empty()
    );
    let by_hand = stored(&store, &hand_written);
    assert_eq!(by_hand.note.body, "added by hand");
    assert_eq!(by_hand.target, Some(foo.node_ref()));
    assert_eq!(store.notes().unwrap().len(), 3);

    std::fs::remove_dir_all(&notes_dir).unwrap();
    reconcile(&mut store, &notes_dir);
    assert!(store.notes().unwrap().is_empty());
}

#[test]
fn written_file_has_quoted_target_then_body() {
    let dir = tempfile::tempdir().unwrap();
    let notes_dir = dir.path().join("notes");
    let mut store = store_with(dir.path(), &[]);
    let mut written = note("src/a.rs::<Foo as Display>::fmt", "Line one.\n\nLine two.");
    written.kind = NoteKind::Label;
    let uuid = NotesReconciler::write(&mut store, &notes_dir, written).unwrap();

    let text = std::fs::read_to_string(notes_dir.join(format!("{}.md", uuid.0))).unwrap();
    assert_eq!(
        text,
        "---\ntarget: \"src/a.rs::<Foo as Display>::fmt\"\nkind: label\ncreated: 2026-10-28T09:00:00Z\nupdated: 2026-10-28T09:00:00Z\n---\nLine one.\n\nLine two.\n"
    );
    assert!(stored(&store, &uuid).orphaned);
}

#[test]
fn a_path_shared_by_two_kinds_annotates_the_type() {
    let dir = tempfile::tempdir().unwrap();
    let constant = symbol("src/a.ts", &[], "Foo", NodeKind::Constant);
    let alias = symbol("src/a.ts", &[], "Foo", NodeKind::TypeAlias);
    let mut store = store_with(dir.path(), &[constant.clone(), alias.clone()]);
    let uuid = NotesReconciler::write(
        &mut store,
        &dir.path().join("notes"),
        note(constant.path.as_str(), "shared"),
    )
    .unwrap();
    assert_eq!(stored(&store, &uuid).target, Some(alias.node_ref()));
}

#[test]
fn write_rejects_an_id_that_is_not_a_uuid() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(dir.path(), &[]);
    let mut escaping = note("repo", "x");
    escaping.uuid = NoteId("../../outside".to_owned());
    assert!(NotesReconciler::write(&mut store, &dir.path().join("notes"), escaping).is_err());
    assert!(store.notes().unwrap().is_empty());
}

#[test]
fn repository_note_is_attached_without_an_edge() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(dir.path(), &[]);
    let uuid = NotesReconciler::write(
        &mut store,
        &dir.path().join("notes"),
        note(REPOSITORY_TARGET, "Monorepo with two crates."),
    )
    .unwrap();
    let repo = stored(&store, &uuid);
    assert!(!repo.orphaned);
    assert_eq!(repo.target, None);
    assert!(annotated(&store, &repo).is_empty());
}
