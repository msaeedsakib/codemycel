use serde::{Deserialize, Serialize};

use super::{Candidate, Edge, ImplRecord, Node, NodeRef, RepoPath};

/// The syntactic output for one file. `structural_edges` holds only `contains` and `defines`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedFile {
    pub nodes: Vec<Node>,
    pub impl_records: Vec<ImplRecord>,
    pub candidates: Vec<Candidate>,
    pub structural_edges: Vec<Edge>,
}

impl ParsedFile {
    /// Every node name, for the repository-wide name counts behind mentions.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().map(|node| node.name.as_str())
    }
}

/// One row of the search content table. `name_split` comes from `text::name_split`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDoc {
    pub node: NodeRef,
    pub name: String,
    pub name_split: String,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub path: Option<RepoPath>,
}
