use std::fmt;

use serde::{Deserialize, Serialize};

use super::{ByteRange, NodeId, NodeKind, RepoPath, RootId, SemanticPath, Span};

/// A node's identity: semantic path and kind (spec §6.4). The key `Store` maps to a `NodeId`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeRef {
    pub path: SemanticPath,
    pub kind: NodeKind,
}

impl NodeRef {
    pub fn new(path: SemanticPath, kind: NodeKind) -> Self {
        Self { path, kind }
    }
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.path, self.kind)
    }
}

/// A node as extracted (spec §6.2). Database ids and the attributes computed after extraction
/// (root, community, unresolved counts) live on `StoredNode`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub path: SemanticPath,
    pub kind: NodeKind,
    pub name: String,
    /// The file holding the node; a workspace package's manifest, so deleting the manifest
    /// deletes the package. `None` for external packages, communities, and notes.
    pub file: Option<RepoPath>,
    pub span: Option<Span>,
    pub selection: Option<ByteRange>,
    pub signature: Option<String>,
    pub has_explicit_return_type: bool,
    pub body_hash: Option<blake3::Hash>,
    pub doc_comment: Option<String>,
    /// As the language spells it (`pub`, `pub(crate)`, `export`); `None` when private.
    pub visibility: Option<String>,
}

impl Node {
    pub fn node_ref(&self) -> NodeRef {
        NodeRef::new(self.path.clone(), self.kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredNode {
    pub id: NodeId,
    pub root_id: Option<RootId>,
    pub community_id: Option<NodeId>,
    pub unresolved_out: u32,
    pub external_out: u32,
    pub node: Node,
}
