use serde::{Deserialize, Serialize};

use super::{
    Candidate, CandidateRole, EdgeKind, NodeKind, NodeRef, Occurrence, RepoPath, SemanticPath,
    VerifiedBy,
};

/// At most one per (src, dst, kind). The edge's root is the root of `src`'s file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub src: NodeRef,
    pub dst: NodeRef,
    pub kind: EdgeKind,
    pub verified_by: VerifiedBy,
    pub occurrences: Vec<Occurrence>,
}

impl Edge {
    /// The edge a confirmed candidate stands for. Imports run file to file and exports from the
    /// file; a call is `calls` only between functions and methods, `references` otherwise. `None`
    /// for an import of the candidate's own file or of a node that lives in no file.
    pub fn for_candidate(
        candidate: &Candidate,
        target: &NodeRef,
        verified_by: VerifiedBy,
    ) -> Option<Edge> {
        let file_node = |path: &RepoPath| {
            SemanticPath::for_file(path)
                .ok()
                .map(|path| NodeRef::new(path, NodeKind::File))
        };
        let callable = |kind| matches!(kind, NodeKind::Function | NodeKind::Method);
        let (src, dst, kind) = match candidate.role {
            CandidateRole::Import => {
                let target_file = RepoPath::new(target.path.file()?).ok()?;
                if target_file == candidate.file {
                    return None;
                }
                (
                    file_node(&candidate.file)?,
                    file_node(&target_file)?,
                    EdgeKind::Imports,
                )
            }
            CandidateRole::Export => (
                file_node(&candidate.file)?,
                target.clone(),
                EdgeKind::Exports,
            ),
            CandidateRole::Call if callable(candidate.enclosing.kind) && callable(target.kind) => {
                (candidate.enclosing.clone(), target.clone(), EdgeKind::Calls)
            }
            CandidateRole::Call | CandidateRole::Type | CandidateRole::Value => (
                candidate.enclosing.clone(),
                target.clone(),
                EdgeKind::References,
            ),
            CandidateRole::Extends => (
                candidate.enclosing.clone(),
                target.clone(),
                EdgeKind::Extends,
            ),
            CandidateRole::Implements => (
                candidate.enclosing.clone(),
                target.clone(),
                EdgeKind::Implements,
            ),
        };
        Some(Edge {
            src,
            dst,
            kind,
            verified_by,
            occurrences: vec![candidate.at],
        })
    }
}
