#![allow(clippy::unwrap_used)]

use codemycel_core::{
    Candidate, CandidateRole, Edge, EdgeKind, NodeKind, NodeRef, Occurrence, RepoPath, VerifiedBy,
};

fn node(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(path.parse().unwrap(), kind)
}

fn candidate(role: CandidateRole, enclosing: NodeRef) -> Candidate {
    Candidate {
        file: RepoPath::new("src/a.rs").unwrap(),
        enclosing,
        text: "X".to_owned(),
        role,
        at: Occurrence { line: 3, col: 4 },
    }
}

fn kind_of(
    role: CandidateRole,
    enclosing: NodeRef,
    target: &NodeRef,
) -> Option<(NodeRef, EdgeKind, NodeRef)> {
    Edge::for_candidate(&candidate(role, enclosing), target, VerifiedBy::Lsp)
        .map(|edge| (edge.src, edge.kind, edge.dst))
}

#[test]
fn confirmed_candidates_become_the_edge_their_role_names() {
    let file = node("src/a.rs", NodeKind::File);
    let run = node("src/a.rs::run", NodeKind::Function);
    let helper = node("src/b.rs::helper", NodeKind::Function);
    let shape = node("src/b.rs::Shape", NodeKind::Struct);
    assert_eq!(
        kind_of(CandidateRole::Call, run.clone(), &helper),
        Some((run.clone(), EdgeKind::Calls, helper.clone()))
    );
    assert_eq!(
        kind_of(CandidateRole::Call, run.clone(), &shape),
        Some((run.clone(), EdgeKind::References, shape.clone()))
    );
    assert_eq!(
        kind_of(CandidateRole::Import, run.clone(), &helper),
        Some((
            file.clone(),
            EdgeKind::Imports,
            node("src/b.rs", NodeKind::File)
        ))
    );
    assert_eq!(kind_of(CandidateRole::Import, run.clone(), &file), None);
    assert_eq!(
        kind_of(CandidateRole::Export, run, &helper),
        Some((file, EdgeKind::Exports, helper))
    );
}

#[test]
fn implements_clause_outside_a_type_leaves_the_edge_to_the_implementers() {
    let trait_node = node("src/b.rs::Store", NodeKind::Trait);
    let class = node("src/a.rs::Memory", NodeKind::Class);
    assert_eq!(
        kind_of(CandidateRole::Implements, class.clone(), &trait_node),
        Some((class, EdgeKind::Implements, trait_node.clone()))
    );
    assert_eq!(
        kind_of(
            CandidateRole::Implements,
            node("src/a.rs", NodeKind::File),
            &trait_node
        ),
        None
    );
}
