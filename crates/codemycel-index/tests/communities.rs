#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{
    Community, Edge, EdgeKind, FileKind, FileRecord, LabelSource, Language, Node, NodeKind,
    NodeRef, Note, NoteKind, Occurrence, RepoPath, SemanticPath, VerifiedBy,
};
use codemycel_index::{CommunityDetector, NotesReconciler};

fn function(file: &str, name: &str) -> Node {
    let file = RepoPath::new(file).unwrap();
    Node {
        path: SemanticPath::symbol(&file, &[] as &[&str], name, None).unwrap(),
        kind: NodeKind::Function,
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

fn edge(src: &Node, dst: &Node, kind: EdgeKind, verified_by: VerifiedBy) -> Edge {
    Edge {
        src: src.node_ref(),
        dst: dst.node_ref(),
        kind,
        verified_by,
        occurrences: vec![Occurrence { line: 1, col: 1 }],
    }
}

fn calls(src: &Node, dst: &Node) -> Edge {
    edge(src, dst, EdgeKind::Calls, VerifiedBy::Lsp)
}

/// `count` functions named `<prefix>0..` in `file`, every pair joined by a call.
fn clique(file: &str, prefix: &str, count: usize) -> (Vec<Node>, Vec<Edge>) {
    let nodes: Vec<Node> = (0..count)
        .map(|i| function(file, &format!("{prefix}{i}")))
        .collect();
    let mut edges = Vec::new();
    for a in 0..count {
        for b in a + 1..count {
            edges.push(calls(&nodes[a], &nodes[b]));
        }
    }
    (nodes, edges)
}

fn add(store: &mut Store, nodes: &[Node], edges: &[Edge]) {
    store
        .transaction(|tx| {
            let files: BTreeSet<&RepoPath> = nodes.iter().filter_map(|n| n.file.as_ref()).collect();
            for file in files {
                if tx.file_by_path(file)?.is_none() {
                    tx.upsert_file(&FileRecord {
                        path: file.clone(),
                        root_id: None,
                        hash: blake3::hash(file.as_str().as_bytes()),
                        size: 1,
                        language: Some(Language::Rust),
                        kind: FileKind::Code,
                        parsed: true,
                        skip_reason: None,
                        scip_covered: false,
                    })?;
                }
            }
            tx.insert_nodes(nodes)?;
            tx.insert_edges(edges)
        })
        .unwrap();
}

fn store_with(dir: &tempfile::TempDir, nodes: &[Node], edges: &[Edge]) -> Store {
    let mut store = Store::create(&dir.path().join("index.db")).unwrap();
    add(&mut store, nodes, edges);
    store
}

/// Runs the detector the way a sync does: against the partition stored before it.
fn detect(store: &mut Store) -> Vec<Community> {
    let previous = store.communities().unwrap();
    store
        .transaction(|tx| CommunityDetector::run(tx, &previous))
        .unwrap();
    store.communities().unwrap()
}

fn names(community: &Community) -> Vec<String> {
    community
        .members
        .iter()
        .map(|m| m.path.name().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn holding<'a>(communities: &'a [Community], name: &str) -> &'a Community {
    communities
        .iter()
        .find(|c| c.members.iter().any(|m| m.path.name() == name))
        .unwrap()
}

fn minted_id(members: &[Node]) -> String {
    let mut paths: Vec<&str> = members.iter().map(|n| n.path.as_str()).collect();
    paths.sort_unstable();
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        hasher.update(path.as_bytes());
        hasher.update(b"\n");
    }
    hasher.finalize().to_hex()[..8].to_owned()
}

#[test]
fn partition_is_identical_across_runs_and_node_orderings() {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (file, prefix) in [("a/x.rs", "a"), ("b/y.rs", "b"), ("c/z.rs", "c")] {
        let (n, e) = clique(file, prefix, 4);
        nodes.extend(n);
        edges.extend(e);
    }
    edges.push(calls(&nodes[0], &nodes[4]));
    edges.push(calls(&nodes[5], &nodes[8]));
    let lonely = function("d/w.rs", "lonely");
    edges.push(edge(
        &lonely,
        &nodes[0],
        EdgeKind::Calls,
        VerifiedBy::Syntactic,
    ));
    nodes.push(lonely);

    let forward_dir = tempfile::tempdir().unwrap();
    let mut forward = store_with(&forward_dir, &nodes, &edges);
    nodes.reverse();
    edges.reverse();
    let reverse_dir = tempfile::tempdir().unwrap();
    let mut reverse = store_with(&reverse_dir, &nodes, &edges);

    let communities = detect(&mut forward);
    detect(&mut reverse);
    let dump = forward.dump_communities().unwrap();
    assert_eq!(dump, reverse.dump_communities().unwrap());
    let groups: Vec<Vec<String>> = communities
        .iter()
        .map(names)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    assert_eq!(
        groups,
        vec![
            vec!["a0", "a1", "a2", "a3"],
            vec!["b0", "b1", "b2", "b3"],
            vec!["c0", "c1", "c2", "c3"],
        ]
    );

    detect(&mut forward);
    assert_eq!(forward.dump_communities().unwrap(), dump);
}

#[test]
fn oversized_community_is_repartitioned() {
    let (mut nodes, mut edges) = clique("blocks/left.rs", "l", 6);
    let (right, right_edges) = clique("blocks/right.rs", "r", 6);
    for i in 0..4 {
        edges.push(calls(&nodes[i], &right[i]));
    }
    nodes.extend(right);
    edges.extend(right_edges);
    for prefix in ["p", "q", "s"] {
        let (n, e) = clique(&format!("{prefix}/mod.rs"), prefix, 10);
        nodes.extend(n);
        edges.extend(e);
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &nodes, &edges);

    let communities = detect(&mut store);

    assert_eq!(communities.len(), 5);
    assert!(
        communities
            .iter()
            .all(|c| c.members.len() * 4 <= nodes.len())
    );
    assert_eq!(
        names(holding(&communities, "l0")),
        ["l0", "l1", "l2", "l3", "l4", "l5"]
    );
    assert_eq!(
        names(holding(&communities, "r0")),
        ["r0", "r1", "r2", "r3", "r4", "r5"]
    );
}

#[test]
fn stable_id_survives_small_membership_change() {
    let (mut nodes, mut edges) = clique("c.rs", "c", 5);
    let (d, d_edges) = clique("d.rs", "d", 5);
    edges.push(calls(&nodes[0], &d[0]));
    nodes.extend(d);
    edges.extend(d_edges);
    for prefix in ["e", "f", "g"] {
        let (n, e) = clique(&format!("{prefix}.rs"), prefix, 5);
        nodes.extend(n);
        edges.extend(e);
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &nodes, &edges);
    let before = detect(&mut store);

    let newcomer = function("c.rs", "c5");
    let joins: Vec<Edge> = nodes[..3].iter().map(|n| calls(&newcomer, n)).collect();
    add(&mut store, std::slice::from_ref(&newcomer), &joins);
    let after = detect(&mut store);

    let grown = holding(&after, "c5");
    assert_eq!(names(grown), ["c0", "c1", "c2", "c3", "c4", "c5"]);
    assert_eq!(grown.stable_id, holding(&before, "c0").stable_id);
    assert_eq!(
        holding(&after, "d0").stable_id,
        holding(&before, "d0").stable_id
    );
}

#[test]
fn new_id_when_overlap_below_half() {
    let (nodes, edges) = clique("k.rs", "k", 4);
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &nodes, &edges);
    let gone = |name: &str| function("gone.rs", name).node_ref();
    let mut members: Vec<NodeRef> = vec![
        nodes[0].node_ref(),
        nodes[1].node_ref(),
        gone("x"),
        gone("y"),
        gone("z"),
    ];
    members.sort();
    let previous = [Community {
        stable_id: "0badc0de".to_owned(),
        label: "old".to_owned(),
        label_source: LabelSource::Fallback,
        members,
    }];

    store
        .transaction(|tx| CommunityDetector::run(tx, &previous))
        .unwrap();

    let communities = store.communities().unwrap();
    assert_eq!(communities.len(), 1);
    assert_eq!(communities[0].stable_id, minted_id(&nodes));
}

#[test]
fn label_from_majority_folder_skips_src_like_segments() {
    let members = [
        function("crates/store/src/write/rows.rs", "insert"),
        function("crates/store/src/write/edges.rs", "link"),
        function("crates/store/src/write/notes.rs", "attach"),
        function("crates/other/src/misc.rs", "tidy"),
    ];
    let mut edges = Vec::new();
    for a in 0..members.len() {
        for b in a + 1..members.len() {
            edges.push(calls(&members[a], &members[b]));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &members, &edges);

    let communities = detect(&mut store);

    assert_eq!(communities.len(), 1);
    assert_eq!(communities[0].label, "store/write");
    assert_eq!(communities[0].label_source, LabelSource::Folder);
}

#[test]
fn label_falls_back_to_name_prefix_then_cluster_id() {
    let parsers = [
        function("one.rs", "parse_file"),
        function("two.rs", "parseDir"),
        function("three.rs", "ParseItems"),
    ];
    let others = [function("x/a.rs", "alpha"), function("y/b.rs", "beta")];
    let edges = vec![
        calls(&parsers[0], &parsers[1]),
        calls(&parsers[1], &parsers[2]),
        calls(&parsers[0], &parsers[2]),
        calls(&others[0], &others[1]),
    ];
    let nodes: Vec<Node> = parsers.iter().chain(&others).cloned().collect();
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &nodes, &edges);

    let communities = detect(&mut store);

    let parse = holding(&communities, "parse_file");
    assert_eq!(
        (parse.label.as_str(), parse.label_source),
        ("parse", LabelSource::Prefix)
    );
    let other = holding(&communities, "alpha");
    assert_eq!(other.label, format!("cluster-{}", other.stable_id));
    assert_eq!(other.label_source, LabelSource::Fallback);
}

#[test]
fn label_note_overrides_display_but_not_identity() {
    let (nodes, edges) = clique("src/engine.rs", "run", 4);
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_with(&dir, &nodes, &edges);
    let before = detect(&mut store);
    let id = before[0].stable_id.clone();
    let notes_dir = dir.path().join("notes");
    let uuid = NotesReconciler::write(
        &mut store,
        &notes_dir,
        Note {
            uuid: NotesReconciler::new_id(),
            target_semantic_path: format!("community:{id}"),
            kind: NoteKind::Label,
            body: "\n  Engine loop  \nsecond line".to_owned(),
            created: "2026-10-28T09:00:00Z".to_owned(),
            updated: "2026-10-28T09:00:00Z".to_owned(),
        },
    )
    .unwrap();

    let previous = store.communities().unwrap();
    store
        .transaction(|tx| {
            NotesReconciler::load(tx, &notes_dir)?;
            CommunityDetector::run(tx, &previous)?;
            NotesReconciler::attach(tx)
        })
        .unwrap();

    let after = store.communities().unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].stable_id, id);
    assert_eq!(after[0].members, before[0].members);
    assert_eq!(after[0].label, "Engine loop");
    assert_eq!(after[0].label_source, LabelSource::Note);
    let community = NodeRef::new(SemanticPath::community(&id).unwrap(), NodeKind::Community);
    let note = store.note_by_uuid(&uuid).unwrap().unwrap();
    assert_eq!(note.target, Some(community));
    assert!(!note.orphaned);

    let fresh_dir = tempfile::tempdir().unwrap();
    let mut fresh = store_with(&fresh_dir, &nodes, &edges);
    let unlabelled = detect(&mut fresh);
    assert_eq!(unlabelled[0].stable_id, id);
    assert_eq!(unlabelled[0].members, after[0].members);
    assert_ne!(unlabelled[0].label, after[0].label);
}
