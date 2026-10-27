use crate::{NodeId, NodeKind, RepoPath};

/// One full-text match before ranking.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub node_id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub file: Option<RepoPath>,
    /// FTS5 BM25 with the column weights of spec §10.1; lower is better, as SQLite reports it.
    pub bm25: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoredHit {
    pub hit: SearchHit,
    /// Higher is better.
    pub score: f64,
}

/// Orders full-text matches for `search` (spec §10.1). Seam for embeddings (spec §18).
pub trait Ranker: Send + Sync {
    fn rank(&self, query: &str, hits: Vec<SearchHit>) -> Vec<ScoredHit>;
}
